// SPDX-License-Identifier: MIT

//! The safety policy: allowlisted bases, protected paths, root verification and the per-item
//! verdict (M1 design sections 2.8 and 4.4; ARCHITECTURE A:850-901).
//!
//! Ownership: work package **B**. `SafetyPolicy` has no loosening setters: `with_*` methods
//! can only tighten it.

use std::sync::Arc;
use std::time::Duration;

use crate::fs::FsBackend;
use crate::path::{RootToken, WidePath};
use crate::resolve::KnownFolders;
use crate::target::Target;
use crate::types::{FileIdentity, Privilege, Scope};

mod bases;
mod error;
mod verdict;
mod verify;

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
    #[allow(dead_code)] // M1-STUB (B): read by verify_root
    allowed_bases: Vec<AllowedBase>,
    #[allow(dead_code)] // M1-STUB (B): read by verify_root
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
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn new<F: FsBackend>(
        fs: &F,
        known: &KnownFolders,
        privilege: Privilege,
    ) -> Result<Self, SafetyError> {
        unimplemented!()
    }

    /// `new(&Win32Fs::default(), known, privilege)`.
    #[cfg(windows)]
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn windows_default(
        known: &KnownFolders,
        privilege: Privilege,
    ) -> Result<Self, SafetyError> {
        unimplemented!()
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
