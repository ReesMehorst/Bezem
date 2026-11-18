// SPDX-License-Identifier: MIT

//! `set_disposition`: `FileDispositionInfoEx` (DELETE | POSIX_SEMANTICS |
//! FORCE_IMAGE_SECTION_CHECK | optional IGNORE_READONLY_ATTRIBUTE), falling back to
//! `FileDispositionInfo { DeleteFile = TRUE }` on `ERROR_INVALID_PARAMETER`,
//! `ERROR_NOT_SUPPORTED` or `ERROR_INVALID_FUNCTION`. Attributes are never cleared.
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::fs::{DispositionError, DispositionKind, DispositionOpts};

/// Marks the item for deletion. `ERROR_DIR_NOT_EMPTY` => `NotEmpty`; legacy fallback with
/// `ERROR_ACCESS_DENIED` on a read-only item => `ReadOnly`; `ERROR_USER_MAPPED_FILE` =>
/// `Locked`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn set_disposition(
    h: &OwnedHandle,
    opts: DispositionOpts,
) -> Result<DispositionKind, DispositionError> {
    unimplemented!()
}
