// SPDX-License-Identifier: MIT

//! NTSTATUS / Win32 code -> [`SkipReason`] tables (M1 design section 3.2). Hard-coded numeric
//! constants so the mapping is unit-tested on Linux.
//!
//! Ownership: work package **A**.
//!
//! The mapping is deliberately conservative: anything not listed is
//! [`SkipReason::FsError`], which is a skip (never a fallback to a weaker method). The values
//! are the documented `ntstatus.h` / `winerror.h` numbers; the Windows-only `win32` module
//! asserts at compile time that the ones it also gets from the `windows` crate agree.

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
/// NT form of `ERROR_USER_MAPPED_FILE` (a mapped view blocks the delete).
pub const STATUS_USER_MAPPED_FILE: i32 = 0xC000_0243_u32 as i32;

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
pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
pub const ERROR_DIR_NOT_EMPTY: u32 = 145;
pub const ERROR_DELETE_PENDING: u32 = 303;
pub const ERROR_USER_MAPPED_FILE: u32 = 1224;

/// Maps an NTSTATUS to a skip reason (unknown => `FsError`); the code is always
/// `ErrorCode::Nt(status)`. `STATUS_DIRECTORY_NOT_EMPTY` is handled by callers via
/// [`is_dir_not_empty_nt`] (here it maps to `FsError`).
pub fn from_nt(status: i32) -> (SkipReason, ErrorCode) {
    let reason = match status {
        STATUS_SHARING_VIOLATION
        | STATUS_FILE_LOCK_CONFLICT
        | STATUS_LOCK_NOT_GRANTED
        | STATUS_CANNOT_DELETE
        | STATUS_USER_MAPPED_FILE => SkipReason::Locked,
        STATUS_ACCESS_DENIED | STATUS_PRIVILEGE_NOT_HELD => SkipReason::AccessDenied,
        STATUS_OBJECT_NAME_NOT_FOUND | STATUS_OBJECT_PATH_NOT_FOUND | STATUS_NO_SUCH_FILE => {
            SkipReason::Gone
        }
        STATUS_DELETE_PENDING => SkipReason::DeletePending,
        STATUS_FILE_IS_A_DIRECTORY | STATUS_NOT_A_DIRECTORY => SkipReason::ChangedSinceScan,
        _ => SkipReason::FsError,
    };
    (reason, ErrorCode::Nt(status))
}

/// Maps a Win32 error code to a skip reason (unknown => `FsError`); the code is always
/// `ErrorCode::Win32(code)`. `ERROR_DIR_NOT_EMPTY` is handled by callers via
/// [`is_dir_not_empty_win32`] (here it maps to `FsError`).
pub fn from_win32(code: u32) -> (SkipReason, ErrorCode) {
    let reason = match code {
        ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_USER_MAPPED_FILE => {
            SkipReason::Locked
        }
        ERROR_ACCESS_DENIED => SkipReason::AccessDenied,
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => SkipReason::Gone,
        ERROR_DELETE_PENDING => SkipReason::DeletePending,
        _ => SkipReason::FsError,
    };
    (reason, ErrorCode::Win32(code))
}

/// `STATUS_DIRECTORY_NOT_EMPTY`.
pub fn is_dir_not_empty_nt(status: i32) -> bool {
    status == STATUS_DIRECTORY_NOT_EMPTY
}

/// `ERROR_DIR_NOT_EMPTY`.
pub fn is_dir_not_empty_win32(code: u32) -> bool {
    code == ERROR_DIR_NOT_EMPTY
}

/// `ERROR_INVALID_PARAMETER`, `ERROR_NOT_SUPPORTED` or `ERROR_INVALID_FUNCTION`: the caller
/// falls back to the legacy disposition / older listing class.
pub fn is_disposition_unsupported_win32(code: u32) -> bool {
    matches!(
        code,
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED | ERROR_INVALID_FUNCTION
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{nt_name, win32_name};

    #[test]
    fn nt_table() {
        let cases = [
            (STATUS_SHARING_VIOLATION, SkipReason::Locked),
            (STATUS_FILE_LOCK_CONFLICT, SkipReason::Locked),
            (STATUS_LOCK_NOT_GRANTED, SkipReason::Locked),
            (STATUS_CANNOT_DELETE, SkipReason::Locked),
            (STATUS_USER_MAPPED_FILE, SkipReason::Locked),
            (STATUS_ACCESS_DENIED, SkipReason::AccessDenied),
            (STATUS_PRIVILEGE_NOT_HELD, SkipReason::AccessDenied),
            (STATUS_OBJECT_NAME_NOT_FOUND, SkipReason::Gone),
            (STATUS_OBJECT_PATH_NOT_FOUND, SkipReason::Gone),
            (STATUS_NO_SUCH_FILE, SkipReason::Gone),
            (STATUS_DELETE_PENDING, SkipReason::DeletePending),
            (STATUS_FILE_IS_A_DIRECTORY, SkipReason::ChangedSinceScan),
            (STATUS_NOT_A_DIRECTORY, SkipReason::ChangedSinceScan),
            (STATUS_OBJECT_NAME_INVALID, SkipReason::FsError),
            (STATUS_OBJECT_NAME_COLLISION, SkipReason::FsError),
            (STATUS_OBJECT_PATH_SYNTAX_BAD, SkipReason::FsError),
            (STATUS_DIRECTORY_NOT_EMPTY, SkipReason::FsError),
            (0, SkipReason::FsError),
            (-1, SkipReason::FsError),
            (i32::MIN, SkipReason::FsError),
            (0x8000_0005_u32 as i32, SkipReason::FsError),
        ];
        for (status, want) in cases {
            assert_eq!(
                from_nt(status),
                (want, ErrorCode::Nt(status)),
                "{status:#x}"
            );
        }
    }

    #[test]
    fn win32_table() {
        let cases = [
            (ERROR_SHARING_VIOLATION, SkipReason::Locked),
            (ERROR_LOCK_VIOLATION, SkipReason::Locked),
            (ERROR_USER_MAPPED_FILE, SkipReason::Locked),
            (ERROR_ACCESS_DENIED, SkipReason::AccessDenied),
            (ERROR_FILE_NOT_FOUND, SkipReason::Gone),
            (ERROR_PATH_NOT_FOUND, SkipReason::Gone),
            (ERROR_DELETE_PENDING, SkipReason::DeletePending),
            (ERROR_DIR_NOT_EMPTY, SkipReason::FsError),
            (ERROR_INVALID_PARAMETER, SkipReason::FsError),
            (ERROR_NO_MORE_FILES, SkipReason::FsError),
            (0, SkipReason::FsError),
            (u32::MAX, SkipReason::FsError),
        ];
        for (code, want) in cases {
            assert_eq!(from_win32(code), (want, ErrorCode::Win32(code)), "{code}");
        }
    }

    #[test]
    fn predicates() {
        assert!(is_dir_not_empty_nt(STATUS_DIRECTORY_NOT_EMPTY));
        assert!(!is_dir_not_empty_nt(STATUS_ACCESS_DENIED));
        assert!(!is_dir_not_empty_nt(ERROR_DIR_NOT_EMPTY as i32));
        assert!(is_dir_not_empty_win32(ERROR_DIR_NOT_EMPTY));
        assert!(!is_dir_not_empty_win32(ERROR_ACCESS_DENIED));
        for c in [
            ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED,
            ERROR_INVALID_FUNCTION,
        ] {
            assert!(is_disposition_unsupported_win32(c), "{c}");
        }
        for c in [
            0,
            ERROR_ACCESS_DENIED,
            ERROR_SHARING_VIOLATION,
            ERROR_DIR_NOT_EMPTY,
        ] {
            assert!(!is_disposition_unsupported_win32(c), "{c}");
        }
    }

    #[test]
    fn every_constant_has_a_symbolic_name() {
        let nt = [
            STATUS_NO_SUCH_FILE,
            STATUS_ACCESS_DENIED,
            STATUS_OBJECT_NAME_INVALID,
            STATUS_OBJECT_NAME_NOT_FOUND,
            STATUS_OBJECT_NAME_COLLISION,
            STATUS_OBJECT_PATH_NOT_FOUND,
            STATUS_OBJECT_PATH_SYNTAX_BAD,
            STATUS_SHARING_VIOLATION,
            STATUS_FILE_LOCK_CONFLICT,
            STATUS_LOCK_NOT_GRANTED,
            STATUS_DELETE_PENDING,
            STATUS_PRIVILEGE_NOT_HELD,
            STATUS_FILE_IS_A_DIRECTORY,
            STATUS_DIRECTORY_NOT_EMPTY,
            STATUS_NOT_A_DIRECTORY,
            STATUS_CANNOT_DELETE,
            STATUS_USER_MAPPED_FILE,
        ];
        for s in nt {
            assert_ne!(nt_name(s), "unknown", "{s:#x}");
        }
        let w = [
            ERROR_INVALID_FUNCTION,
            ERROR_FILE_NOT_FOUND,
            ERROR_PATH_NOT_FOUND,
            ERROR_ACCESS_DENIED,
            ERROR_NO_MORE_FILES,
            ERROR_SHARING_VIOLATION,
            ERROR_LOCK_VIOLATION,
            ERROR_NOT_SUPPORTED,
            ERROR_INVALID_PARAMETER,
            ERROR_INSUFFICIENT_BUFFER,
            ERROR_DIR_NOT_EMPTY,
            ERROR_DELETE_PENDING,
            ERROR_USER_MAPPED_FILE,
        ];
        for c in w {
            assert_ne!(win32_name(c), "unknown", "{c}");
        }
    }
}
