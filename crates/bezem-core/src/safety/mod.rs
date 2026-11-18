// SPDX-License-Identifier: MIT

//! The safety policy: allowlisted bases, protected paths, root verification and the per-item
//! verdict (M1 design sections 2.8 and 4.4; ARCHITECTURE A:850-901).
//!
//! Ownership: work package **B**. `SafetyPolicy` has no loosening setters: `with_*` methods
//! can only tighten it.

use std::sync::Arc;
use std::time::Duration;

use crate::fs::{FsBackend, OpenRootError, RootOpenMode, VolumeKind};
use crate::path::{parse_final, PathReject, RootToken, WidePath};
use crate::resolve::{KnownFolderError, KnownFolderId, KnownFolders};
use crate::target::Target;
use crate::types::{FileIdentity, Privilege, Scope};

mod bases;
mod error;
mod verdict;
mod verify;

#[cfg(test)]
mod tests;

pub use bases::{
    AllowKind, AllowedBase, AllowedBaseId, BaseSpec, ProtectedPath, ProtectedSpec, BASE_TABLE,
    PROTECTED_TABLE,
};
pub use error::{RootError, SafetyError, SafetyViolation, ViolationKind};
pub use verdict::ItemFacts;

/// Default `max_entries_per_target` (ARCHITECTURE A:251).
pub const DEFAULT_MAX_ENTRIES_PER_TARGET: u64 = 5_000_000;
/// Minimum age floor for temp roots (decision 4): 24 h.
pub const TEMP_MIN_AGE_FLOOR: Duration = Duration::from_secs(24 * 60 * 60);

/// A root that passed `SafetyPolicy::verify_root`. Only `verify_root` builds one.
#[derive(Debug)]
pub struct VerifiedRoot<D> {
    dir: Arc<D>,
    final_path: WidePath,
    identity: FileIdentity,
    base: AllowedBaseId,
    token: RootToken,
    scope: Scope,
}

impl<D> VerifiedRoot<D> {
    /// The pinned root directory handle (opened without `FILE_SHARE_DELETE`).
    pub fn dir(&self) -> &Arc<D> {
        &self.dir
    }
    /// The canonical `\\?\X:\...` final path of the root.
    pub fn final_path(&self) -> &WidePath {
        &self.final_path
    }
    /// The root's identity at verification time.
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }
    /// The allowlisted base the root lies in.
    pub fn base(&self) -> AllowedBaseId {
        self.base
    }
    /// The root's token.
    pub fn token(&self) -> RootToken {
        self.token
    }
    /// The scope the root was verified for.
    pub fn scope(&self) -> Scope {
        self.scope
    }
}

/// The safety policy for one run.
#[derive(Debug, Clone)]
pub struct SafetyPolicy {
    allowed_bases: Vec<AllowedBase>,
    protected: Vec<ProtectedPath>,
    privilege: Privilege,
    temp_min_age_floor: Duration,
    max_entries_per_target: u64,
}

impl SafetyPolicy {
    /// Canonicalizes every base in [`BASE_TABLE`] and protected path in [`PROTECTED_TABLE`]
    /// with `fs.open_root(.., CanonicalizeFollow)`. Missing bases are dropped silently; other
    /// failures are errors; no base at all => `NoBases`. For protected paths both the raw
    /// known value and its followed target are stored.
    ///
    /// Fail-closed details:
    /// - A base is dropped (never an error: fewer bases only allow less) when its known folder
    ///   does not resolve, it cannot be opened (missing, access denied, ...), it is not a
    ///   directory, it is not on a local fixed volume, or its final path is malformed or a
    ///   drive root.
    /// - A protected path whose known folder does not resolve is an error
    ///   ([`SafetyError::KnownFolder`]), except for `ScopeError` (the elevated worker has no
    ///   user-scope folders) and for the folders that legitimately may not exist
    ///   ([`is_optional_protected`]: OneDrive, the media/library folders, Program Files (x86)).
    /// - A protected path that exists but cannot be opened is an error
    ///   ([`SafetyError::Canonicalize`]). A malformed final path (for example a redirected
    ///   UNC folder, which no local root can equal) keeps only the raw path.
    pub fn new<F: FsBackend>(
        fs: &F,
        known: &KnownFolders,
        privilege: Privilege,
    ) -> Result<Self, SafetyError> {
        let mut allowed_bases = Vec::new();
        for (i, spec) in BASE_TABLE.iter().enumerate() {
            let Ok(id) = u8::try_from(i).map(AllowedBaseId) else {
                continue;
            };
            if let Some(base) = canonical_base(fs, known, id, spec) {
                allowed_bases.push(base);
            }
        }
        if allowed_bases.is_empty() {
            return Err(SafetyError::NoBases);
        }

        let mut protected: Vec<ProtectedPath> = Vec::new();
        let mut add = |canonical: WidePath, label: &'static str| {
            if !protected.iter().any(|p| p.canonical.eq_strict(&canonical)) {
                protected.push(ProtectedPath { canonical, label });
            }
        };
        for spec in PROTECTED_TABLE {
            let raw = match known.get(spec.folder) {
                Ok(p) => p.clone(),
                Err(KnownFolderError::ScopeError) => continue,
                Err(e) if is_optional_protected(spec.folder) => {
                    tracing::warn!(folder = ?spec.folder, error = %e, "optional protected folder unresolved");
                    continue;
                }
                Err(e) => {
                    return Err(SafetyError::KnownFolder {
                        id: spec.folder,
                        source: e.clone(),
                    })
                }
            };
            let raw = push_static(raw, spec.rel).map_err(|e| SafetyError::KnownFolder {
                id: spec.folder,
                source: KnownFolderError::Invalid(e),
            })?;
            add(raw.clone(), spec.label);
            match fs.open_root(&raw, RootOpenMode::CanonicalizeFollow) {
                Ok(probe) => match parse_final(&probe.final_path) {
                    Ok(c) => add(c, spec.label),
                    Err(e) => {
                        tracing::warn!(label = spec.label, error = %e, "protected path final form not local; raw path kept");
                    }
                },
                Err(OpenRootError::NotFound) => {}
                Err(OpenRootError::Fs(err)) => {
                    return Err(SafetyError::Canonicalize {
                        what: format!("{} ({})", spec.label, raw.display_string()),
                        err,
                    })
                }
            }
        }

        Ok(SafetyPolicy {
            allowed_bases,
            protected,
            privilege,
            temp_min_age_floor: TEMP_MIN_AGE_FLOOR,
            max_entries_per_target: DEFAULT_MAX_ENTRIES_PER_TARGET,
        })
    }

    /// `new(&Win32Fs::default(), known, privilege)`.
    #[cfg(windows)]
    pub fn windows_default(
        known: &KnownFolders,
        privilege: Privilege,
    ) -> Result<Self, SafetyError> {
        Self::new(&crate::Win32Fs::default(), known, privilege)
    }

    /// The canonicalized allowlisted bases (for diagnostics and tests).
    pub fn allowed_bases(&self) -> &[AllowedBase] {
        &self.allowed_bases
    }

    /// The protected paths, raw and canonical spellings (for diagnostics and tests).
    pub fn protected_paths(&self) -> &[ProtectedPath] {
        &self.protected
    }

    /// `temp_root ? max(t.min_age, floor) : t.min_age`.
    pub fn effective_min_age(&self, t: &Target) -> Duration {
        if t.temp_root {
            t.min_age.max(self.temp_min_age_floor)
        } else {
            t.min_age
        }
    }

    /// A target whose plan exceeds this many entries is aborted.
    pub fn max_entries_per_target(&self) -> u64 {
        self.max_entries_per_target
    }

    /// The privilege the policy was built for.
    pub fn privilege(&self) -> Privilege {
        self.privilege
    }

    /// Tighten-only: the limit becomes `min(current, n)`.
    pub fn with_max_entries(mut self, n: u64) -> Self {
        self.max_entries_per_target = self.max_entries_per_target.min(n);
        self
    }

    /// Raise-only: the temp floor becomes `max(current, d)`.
    pub fn with_temp_floor(mut self, d: Duration) -> Self {
        self.temp_min_age_floor = self.temp_min_age_floor.max(d);
        self
    }
}

/// Protected known folders that may legitimately be absent (OneDrive not set up, library
/// folders removed, no WOW64). Their absence drops the entry instead of failing the policy.
/// Roots must still lie below an allowlisted base, so this only removes the second layer for a
/// folder the OS itself cannot locate.
pub fn is_optional_protected(id: KnownFolderId) -> bool {
    matches!(
        id,
        KnownFolderId::OneDrive
            | KnownFolderId::Desktop
            | KnownFolderId::Documents
            | KnownFolderId::Downloads
            | KnownFolderId::Pictures
            | KnownFolderId::Music
            | KnownFolderId::Videos
            | KnownFolderId::ProgramFilesX86
    )
}

/// Appends static table components with the strict template check.
fn push_static(mut p: WidePath, rel: &[&str]) -> Result<WidePath, PathReject> {
    for c in rel {
        let c: Vec<u16> = c.encode_utf16().collect();
        p.push_template_component(&c)?;
    }
    Ok(p)
}

/// Resolves and canonicalizes one base; `None` drops it (see [`SafetyPolicy::new`]).
fn canonical_base<F: FsBackend>(
    fs: &F,
    known: &KnownFolders,
    id: AllowedBaseId,
    spec: &BaseSpec,
) -> Option<AllowedBase> {
    let folder = known.get(spec.family.known_folder()).ok()?.clone();
    let path = push_static(folder, spec.rel).ok()?;
    let probe = match fs.open_root(&path, RootOpenMode::CanonicalizeFollow) {
        Ok(p) => p,
        Err(OpenRootError::NotFound) => return None,
        Err(OpenRootError::Fs(e)) => {
            tracing::warn!(base = %path.display_string(), error = ?e, "allowlisted base dropped");
            return None;
        }
    };
    if !probe.is_directory || probe.volume != VolumeKind::LocalFixed {
        tracing::warn!(base = %path.display_string(), "allowlisted base dropped: not a local directory");
        return None;
    }
    let canonical = parse_final(&probe.final_path).ok()?;
    if canonical.is_drive_root() {
        tracing::warn!(base = %path.display_string(), "allowlisted base dropped: drive root");
        return None;
    }
    Some(AllowedBase {
        id,
        family: spec.family,
        scope: spec.family.scope(),
        allow: spec.allow,
        canonical,
        volume_serial: probe.identity.volume_serial,
    })
}
