// SPDX-License-Identifier: MIT

//! `read_dir`: directory listing via `GetFileInformationByHandleEx` with a 64 KiB 8-aligned
//! buffer; `FileIdExtdDirectory*` with fallbacks to `FileIdBothDirectory*` and
//! `FileFullDirectory*`. Buffers are parsed by `fs::dirbuf` (safe code).
//!
//! Ownership: work package **A**.

use std::ops::ControlFlow;

use super::OwnedHandle;
use crate::error::Win32Error;
use crate::fs::{IdQuality, RawEntry};

/// Lists `dir`; returns the id quality of the class that worked.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn read_dir(
    dir: &OwnedHandle,
    sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
) -> Result<IdQuality, Win32Error> {
    unimplemented!()
}
