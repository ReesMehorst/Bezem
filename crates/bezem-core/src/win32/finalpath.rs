// SPDX-License-Identifier: MIT

//! `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)`, growing the buffer.
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::error::Win32Error;

/// The raw final path (no NUL). Validation happens in `path::parse_final`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn final_path(h: &OwnedHandle) -> Result<Vec<u16>, Win32Error> {
    unimplemented!()
}
