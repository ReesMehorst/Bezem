// SPDX-License-Identifier: MIT

//! Known-folder resolution (M1 design section 2.6; ARCHITECTURE A:713-737).
//!
//! Ownership: work package **B** (pure logic); work package **A** provides the Windows
//! [`KnownFolderSource`] (`win32::SystemKnownFolders`). Values come only from OS APIs; the
//! environment is never read for decisions. Paths are built by pushing validated components,
//! never by string concatenation.

use std::collections::BTreeMap;

use crate::path::{PathReject, RootSpec, RootToken, WidePath};
use crate::types::{CleanerId, Privilege, Scope};

#[cfg(any(test, feature = "test-support"))]
mod sandbox;

/// The known folders M1 resolves (roots, allowlisted bases and protected paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KnownFolderId {
    Temp,
    LocalAppData,
    LocalAppDataLow,
    RoamingAppData,
    Recent,
    Profile,
    ProgramData,
    Windows,
    ProgramFiles,
    ProgramFilesX86,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    OneDrive,
}

impl KnownFolderId {
    /// Every id, in `Ord` order.
    pub const ALL: [KnownFolderId; 17] = [
        KnownFolderId::Temp,
        KnownFolderId::LocalAppData,
        KnownFolderId::LocalAppDataLow,
        KnownFolderId::RoamingAppData,
        KnownFolderId::Recent,
        KnownFolderId::Profile,
        KnownFolderId::ProgramData,
        KnownFolderId::Windows,
        KnownFolderId::ProgramFiles,
        KnownFolderId::ProgramFilesX86,
        KnownFolderId::Desktop,
        KnownFolderId::Documents,
        KnownFolderId::Downloads,
        KnownFolderId::Pictures,
        KnownFolderId::Music,
        KnownFolderId::Videos,
        KnownFolderId::OneDrive,
    ];

    /// `Windows`, `ProgramData`, `ProgramFiles` and `ProgramFilesX86` are `Machine`; the rest
    /// are `User`.
    pub fn scope(self) -> Scope {
        match self {
            KnownFolderId::Windows
            | KnownFolderId::ProgramData
            | KnownFolderId::ProgramFiles
            | KnownFolderId::ProgramFilesX86 => Scope::Machine,
            KnownFolderId::Temp
            | KnownFolderId::LocalAppData
            | KnownFolderId::LocalAppDataLow
            | KnownFolderId::RoamingAppData
            | KnownFolderId::Recent
            | KnownFolderId::Profile
            | KnownFolderId::Desktop
            | KnownFolderId::Documents
            | KnownFolderId::Downloads
            | KnownFolderId::Pictures
            | KnownFolderId::Music
            | KnownFolderId::Videos
            | KnownFolderId::OneDrive => Scope::User,
        }
    }
}

/// Why a known folder did not resolve. Such tokens make their targets Unresolved: reported,
/// never guessed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnownFolderError {
    #[error("API failed: {0}")]
    Failed(String),
    #[error("empty value")]
    Empty,
    #[error("relative path")]
    Relative,
    #[error("value too short")]
    TooShort,
    #[error("drive root")]
    DriveRoot,
    #[error("UNC path")]
    Unc,
    #[error("invalid path: {0}")]
    Invalid(PathReject),
    #[error("user-scope folder requested in the elevated worker")]
    ScopeError,
}

/// Supplies raw known-folder values from the OS (or a test).
pub trait KnownFolderSource {
    /// The raw API value (no trailing NUL), not yet validated.
    fn get(&self, id: KnownFolderId) -> Result<Vec<u16>, KnownFolderError>;
}

/// Every known folder, resolved once and then immutable.
#[derive(Debug, Clone)]
pub struct KnownFolders {
    map: BTreeMap<KnownFolderId, Result<WidePath, KnownFolderError>>,
}

/// Returned by [`KnownFolders::get`] for an id the map does not contain.
static NOT_RESOLVED: KnownFolderError = KnownFolderError::Failed(String::new());

impl KnownFolders {
    /// Resolves every id from `src`; each value goes through `WidePath::from_known_value`.
    /// User ids under `Privilege::ElevatedWorker` resolve to `Err(ScopeError)`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn resolve(src: &dyn KnownFolderSource, privilege: Privilege) -> Self {
        unimplemented!()
    }

    /// Like [`Self::resolve`] from a fixed map (pure tests and fuzzing; non-production callers
    /// only). Ids missing from `m` resolve to `Err(Failed)`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn from_map(m: BTreeMap<KnownFolderId, Vec<u16>>, privilege: Privilege) -> Self {
        unimplemented!()
    }

    /// The resolved path for `id`, or why it did not resolve.
    pub fn get(&self, id: KnownFolderId) -> Result<&WidePath, &KnownFolderError> {
        match self.map.get(&id) {
            Some(Ok(p)) => Ok(p),
            Some(Err(e)) => Err(e),
            None => Err(&NOT_RESOLVED),
        }
    }

    /// `resolve(&win32::SystemKnownFolders, privilege)`.
    #[cfg(windows)]
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn from_system(privilege: Privilege) -> Self {
        unimplemented!()
    }
}

/// A root resolved from a [`RootSpec`]: still a display path, not yet verified. Only
/// `SafetyPolicy::verify_root` turns it into a `VerifiedRoot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub cleaner: CleanerId,
    pub target_index: usize,
    pub token: RootToken,
    pub display: WidePath,
}

/// Why a root spec did not resolve.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("unresolved {token:?}: {source}")]
    Unresolved {
        token: RootToken,
        source: KnownFolderError,
    },
    #[error("component: {0}")]
    Component(#[from] PathReject),
}

/// `known.get(spec.token.known_folder())`, then `push_template_component` for each spec
/// component. No string concatenation.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn resolve_root(
    spec: &RootSpec,
    known: &KnownFolders,
    cleaner: CleanerId,
    target_index: usize,
) -> Result<ResolvedRoot, ResolveError> {
    unimplemented!()
}
