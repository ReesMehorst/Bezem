// SPDX-License-Identifier: MIT

//! Safety errors: [`SafetyViolation`] (safety tier: target aborted, error-level log),
//! [`SafetyError`] (policy construction) and [`RootError`] (`verify_root` outcome).
//!
//! Ownership: work package **B**.

use std::fmt;

use crate::fs::FsError;
use crate::path::PathReject;
use crate::resolve::{KnownFolderError, KnownFolderId};

/// A safety-tier failure: the target is aborted and the run exits non-zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyViolation {
    pub kind: ViolationKind,
    /// Display path (without `\\?\`), lossy.
    pub path: String,
    pub detail: String,
}

/// What rule a [`SafetyViolation`] broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationKind {
    ScopeMismatch,
    RootNotAllowlisted,
    RootProtected,
    RootAncestorOfProtected,
    RootIsDriveRoot,
    RootIsReparsePoint,
    RootNotDirectory,
    BadFinalPathForm(PathReject),
    UncOrRemoteVolume,
    RootIdentityChanged,
    /// Delete-time tripwire: an item's final path is not where the plan says it is.
    FinalPathMismatch,
    Path(PathReject),
}

impl fmt::Display for SafetyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "safety violation {:?} at {}", self.kind, self.path)?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for SafetyViolation {}

/// Building a `SafetyPolicy` failed.
#[derive(Debug, thiserror::Error)]
pub enum SafetyError {
    #[error("known folder {id:?}: {source}")]
    KnownFolder {
        id: KnownFolderId,
        source: KnownFolderError,
    },
    #[error("canonicalize {what}: {err:?}")]
    Canonicalize { what: String, err: FsError },
    #[error("no allowlisted base resolved")]
    NoBases,
}

/// Outcome of a failed `verify_root`. `Missing` is mapped by the caller using
/// `Target::optional` (Missing vs Unavailable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootError {
    Missing,
    Failed(FsError),
    Violation(SafetyViolation),
}
