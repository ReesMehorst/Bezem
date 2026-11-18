// SPDX-License-Identifier: MIT

//! Pure parsers for `GetFileInformationByHandleEx` directory buffers
//! (`FILE_ID_EXTD_DIR_INFO`, `FILE_ID_BOTH_DIR_INFO`, `FILE_FULL_DIR_INFO`), M1 design
//! section 3.3. Safe code with bounds checks; bad offsets are an error, never a panic.
//! `.` and `..` are filtered here.
//!
//! Ownership: work package **A** (verify offsets against the SDK headers).

use std::ops::ControlFlow;

use super::RawEntry;

/// A malformed directory buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DirBufError {
    #[error("entry at offset {offset} overruns the buffer")]
    Truncated { offset: usize },
    #[error("bad file name length at offset {offset}")]
    BadNameLength { offset: usize },
    #[error("bad next-entry offset at offset {offset}")]
    BadNextOffset { offset: usize },
}

/// Parses a `FILE_ID_EXTD_DIR_INFO` chain (128-bit ids).
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn parse_extd(
    buf: &[u8],
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    unimplemented!()
}

/// Parses a `FILE_ID_BOTH_DIR_INFO` chain (64-bit ids, zero-extended to 16 bytes).
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn parse_both(
    buf: &[u8],
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    unimplemented!()
}

/// Parses a `FILE_FULL_DIR_INFO` chain (no ids: `file_id = None`).
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn parse_full(
    buf: &[u8],
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    unimplemented!()
}
