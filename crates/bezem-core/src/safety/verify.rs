// SPDX-License-Identifier: MIT

//! `SafetyPolicy::verify_root` (M1 design section 4.4): the pure verification steps around
//! `fs.open_root(.., VerifyNoFollow)`. The only constructor of `VerifiedRoot`.
//!
//! Ownership: work package **B**.

use std::sync::Arc;

use super::{AllowKind, RootError, SafetyPolicy, SafetyViolation, VerifiedRoot, ViolationKind};
use crate::fs::{FsBackend, OpenRootError, RootOpenMode, VolumeKind};
use crate::path::{eq_wide, parse_final, Cmp};
use crate::resolve::ResolvedRoot;
use crate::types::{Privilege, Scope};

impl SafetyPolicy {
    /// Steps: scope check (ElevatedWorker requires Machine) -> `open_root` (NotFound =>
    /// `Missing`) -> directory, not reparse -> `parse_final` -> local fixed volume -> base of the
    /// same family and volume per `AllowKind` (`Cmp::Strict`) -> not a drive root, not equal to
    /// (`Cmp::Loose`) or an ancestor of any protected path -> `VerifiedRoot`.
    pub fn verify_root<F: FsBackend>(
        &self,
        fs: &F,
        root: &ResolvedRoot,
        scope: Scope,
    ) -> Result<VerifiedRoot<F::Dir>, RootError> {
        let token = root.token;
        let shown = root.display.display_string();
        let violation = |kind: ViolationKind, path: &str, detail: String| {
            RootError::Violation(SafetyViolation {
                kind,
                path: path.to_owned(),
                detail,
            })
        };

        // 1. Scope.
        if scope != token.scope() {
            return Err(violation(
                ViolationKind::ScopeMismatch,
                &shown,
                format!(
                    "token {token:?} is {:?}-scope, cleaner is {scope:?}",
                    token.scope()
                ),
            ));
        }
        if self.privilege == Privilege::ElevatedWorker && scope != Scope::Machine {
            return Err(violation(
                ViolationKind::ScopeMismatch,
                &shown,
                "the elevated worker only cleans machine-scope roots".to_owned(),
            ));
        }

        // 2. Open the root itself without following a reparse point.
        let probe = match fs.open_root(&root.display, RootOpenMode::VerifyNoFollow) {
            Ok(p) => p,
            Err(OpenRootError::NotFound) => return Err(RootError::Missing),
            Err(OpenRootError::Fs(e)) => return Err(RootError::Failed(e)),
        };

        // 3. A directory, not a reparse point.
        if !probe.is_directory {
            return Err(violation(
                ViolationKind::RootNotDirectory,
                &shown,
                String::new(),
            ));
        }
        if probe.is_reparse {
            return Err(violation(
                ViolationKind::RootIsReparsePoint,
                &shown,
                String::new(),
            ));
        }

        // 4. The final path must be `\\?\X:\...`.
        let final_path = match parse_final(&probe.final_path) {
            Ok(f) => f,
            Err(e) => {
                let raw = String::from_utf16_lossy(&probe.final_path);
                return Err(violation(
                    ViolationKind::BadFinalPathForm(e),
                    &shown,
                    format!("final path {raw:?}"),
                ));
            }
        };
        let fshown = final_path.display_string();

        // 5. Local fixed volume.
        if probe.volume != VolumeKind::LocalFixed {
            return Err(violation(
                ViolationKind::UncOrRemoteVolume,
                &fshown,
                format!("volume kind {:?}", probe.volume),
            ));
        }

        // 6. An allowlisted base of the same family, scope and volume (strict compare).
        let base = self.allowed_bases.iter().find(|b| {
            b.family == token
                && b.scope == scope
                && b.volume_serial == probe.identity.volume_serial
                && match b.allow {
                    AllowKind::EqualOrBelow => {
                        final_path.is_same_or_below(&b.canonical, Cmp::Strict)
                    }
                    AllowKind::StrictlyBelow => {
                        final_path.is_strictly_below(&b.canonical, Cmp::Strict)
                    }
                }
        });
        let Some(base) = base else {
            return Err(violation(
                ViolationKind::RootNotAllowlisted,
                &fshown,
                format!("no allowlisted {token:?} base contains it"),
            ));
        };

        // 7. Never a drive root, a protected path or an ancestor of one (loose compare).
        if final_path.is_drive_root() {
            return Err(violation(
                ViolationKind::RootIsDriveRoot,
                &fshown,
                String::new(),
            ));
        }
        for p in &self.protected {
            if eq_wide(final_path.as_wide(), p.canonical.as_wide(), Cmp::Loose) {
                return Err(violation(
                    ViolationKind::RootProtected,
                    &fshown,
                    p.label.to_owned(),
                ));
            }
            if p.canonical.is_strictly_below(&final_path, Cmp::Loose) {
                return Err(violation(
                    ViolationKind::RootAncestorOfProtected,
                    &fshown,
                    p.label.to_owned(),
                ));
            }
        }

        // 8. Done: only this function builds a VerifiedRoot.
        Ok(VerifiedRoot {
            dir: Arc::new(probe.dir),
            final_path,
            identity: probe.identity,
            base: base.id,
            token,
            scope,
        })
    }
}
