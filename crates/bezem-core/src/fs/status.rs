// SPDX-License-Identifier: MIT

//! NTSTATUS / Win32 code -> [`SkipReason`] tables (M1 design section 3.2). Hard-coded numeric
//! constants so the mapping is unit-tested on Linux.
//!
//! Ownership: work package **A**.

use crate::error::ErrorCode;
use crate::types::SkipReason;

// NTSTATUS values (as i32; the hex spelling is the documented u32 value).
pub const STATUS_NO_SUCH_FILE: i32 = 0xC000_000F_u32 as i32;
pub const STATUS_ACCESS_DENIED: i32 = 0xC000_0022_u32 as i32;
pub const STATUS_OBJECT_NAME_INVALID: i32 = 0xC000_0033_u32 as i32;
pub const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034_u32 as i32;
pub const STATUS_OBJECT_NAME_COLLISION: i32 = 0xC000_0035_u32 as i32;
pub const STATUS_OBJECT_PATH_NOT_FOUND: i32 = 0xC000_003A_u32 as i32;
pub const STATUS_OBJECT_PATH_SYNTAX_BAD: i32 = 0xC000_003B_u32 as i32;
pub const STATUS_SHARING_VIOLATION: i32 = 0xC000_0043_u32 as i32;
pub const STATUS_FILE_LOCK_CONFLICT: i32 = 0xC000_0054_u32 as i32;
pub const STATUS_LOCK_NOT_GRANTED: i32 = 0xC000_0055_u32 as i32;
pub const STATUS_DELETE_PENDING: i32 = 0xC000_0056_u32 as i32;
pub const STATUS_PRIVILEGE_NOT_HELD: i32 = 0xC000_0061_u32 as i32;
pub const STATUS_FILE_IS_A_DIRECTORY: i32 = 0xC000_00BA_u32 as i32;
pub const STATUS_DIRECTORY_NOT_EMPTY: i32 = 0xC000_0101_u32 as i32;
pub const STATUS_NOT_A_DIRECTORY: i32 = 0xC000_0103_u32 as i32;
pub const STATUS_CANNOT_DELETE: i32 = 0xC000_0121_u32 as i32;

// Win32 error codes.
pub const ERROR_INVALID_FUNCTION: u32 = 1;
pub const ERROR_FILE_NOT_FOUND: u32 = 2;
pub const ERROR_PATH_NOT_FOUND: u32 = 3;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const ERROR_NO_MORE_FILES: u32 = 18;
pub const ERROR_SHARING_VIOLATION: u32 = 32;
pub const ERROR_LOCK_VIOLATION: u32 = 33;
pub const ERROR_NOT_SUPPORTED: u32 = 50;
pub const ERROR_INVALID_PARAMETER: u32 = 87;
pub const ERROR_DIR_NOT_EMPTY: u32 = 145;
pub const ERROR_DELETE_PENDING: u32 = 303;
pub const ERROR_USER_MAPPED_FILE: u32 = 1224;

/// Maps an NTSTATUS to a skip reason (unknown => `FsError`); the code is always
/// `ErrorCode::Nt(status)`. `STATUS_DIRECTORY_NOT_EMPTY` is handled by callers via
/// [`is_dir_not_empty_nt`].
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn from_nt(status: i32) -> (SkipReason, ErrorCode) {
    unimplemented!()
}

/// Maps a Win32 error code to a skip reason (unknown => `FsError`); the code is always
/// `ErrorCode::Win32(code)`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn from_win32(code: u32) -> (SkipReason, ErrorCode) {
    unimplemented!()
}

/// `STATUS_DIRECTORY_NOT_EMPTY`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn is_dir_not_empty_nt(status: i32) -> bool {
    unimplemented!()
}

/// `ERROR_DIR_NOT_EMPTY`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn is_dir_not_empty_win32(code: u32) -> bool {
    unimplemented!()
}

/// `ERROR_INVALID_PARAMETER`, `ERROR_NOT_SUPPORTED` or `ERROR_INVALID_FUNCTION`: the caller
/// falls back to the legacy disposition / older listing class.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub fn is_disposition_unsupported_win32(code: u32) -> bool {
    unimplemented!()
}
