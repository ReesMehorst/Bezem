// SPDX-License-Identifier: MIT

//! `open_root`: `CreateFileW` on a `\\?\` path, then attribute-tag info, `FileIdInfo`, final
//! path and the volume probe.
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::fs::{OpenRootError, RootOpenMode, RootProbe};
use crate::path::WidePath;

/// `FILE_FLAG_BACKUP_SEMANTICS`, plus `FILE_FLAG_OPEN_REPARSE_POINT` for `VerifyNoFollow`.
/// `ERROR_FILE_NOT_FOUND`/`ERROR_PATH_NOT_FOUND` => `OpenRootError::NotFound`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn open_root(
    path: &WidePath,
    mode: RootOpenMode,
) -> Result<RootProbe<OwnedHandle>, OpenRootError> {
    unimplemented!()
}
