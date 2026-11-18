// SPDX-License-Identifier: MIT

//! `SafetyPolicy::verify_root` (M1 design section 4.4): the pure verification steps around
//! `fs.open_root(.., VerifyNoFollow)`. The only constructor of `VerifiedRoot`.
//!
//! Ownership: work package **B**.

use super::{RootError, SafetyPolicy, VerifiedRoot};
use crate::fs::FsBackend;
use crate::resolve::ResolvedRoot;
use crate::types::Scope;

impl SafetyPolicy {
    /// Steps: scope check (ElevatedWorker requires Machine) -> `open_root` (NotFound =>
    /// `Missing`) -> directory, not reparse -> `parse_final` -> local fixed volume -> base of the
    /// same family and volume per `AllowKind` (`Cmp::Strict`) -> not a drive root, not equal to
    /// (`Cmp::Loose`) or an ancestor of any protected path -> `VerifiedRoot`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn verify_root<F: FsBackend>(
        &self,
        fs: &F,
        root: &ResolvedRoot,
        scope: Scope,
    ) -> Result<VerifiedRoot<F::Dir>, RootError> {
        unimplemented!()
    }
}
