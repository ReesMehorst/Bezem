// SPDX-License-Identifier: MIT

//! Error types for the four error tiers (M1 design section 2.2; ARCHITECTURE A:1274-1293).
//!
//! - Item: [`SkipReason`](crate::types::SkipReason) (+ [`ErrorCode`] for the log).
//! - Target: [`TargetError`]. Safety: [`SafetyViolation`](crate::safety::SafetyViolation).
//! - Fatal: [`EngineError`].
//!
//! Ownership: Phase 0 contracts; work package **A** owns and extends the symbolic-name tables
//! ([`win32_name`], [`nt_name`]).

use crate::fs::FsError;
use crate::resolve::ResolveError;
use crate::safety::SafetyError;

/// A failed Win32 call: the API name and its `GetLastError` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{api} failed: {} ({code:#x})", win32_name(*.code))]
pub struct Win32Error {
    pub api: &'static str,
    pub code: u32,
}

/// A failed NT native call: the API name and its `NTSTATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{api} failed: {} ({status:#010x})", nt_name(*.status))]
pub struct NtError {
    pub api: &'static str,
    pub status: i32,
}

/// Symbolic name of a Win32 error code, or `"unknown"`.
pub fn win32_name(code: u32) -> &'static str {
    match code {
        1 => "ERROR_INVALID_FUNCTION",
        2 => "ERROR_FILE_NOT_FOUND",
        3 => "ERROR_PATH_NOT_FOUND",
        5 => "ERROR_ACCESS_DENIED",
        6 => "ERROR_INVALID_HANDLE",
        8 => "ERROR_NOT_ENOUGH_MEMORY",
        13 => "ERROR_INVALID_DATA",
        18 => "ERROR_NO_MORE_FILES",
        21 => "ERROR_NOT_READY",
        32 => "ERROR_SHARING_VIOLATION",
        33 => "ERROR_LOCK_VIOLATION",
        50 => "ERROR_NOT_SUPPORTED",
        53 => "ERROR_BAD_NETPATH",
        87 => "ERROR_INVALID_PARAMETER",
        122 => "ERROR_INSUFFICIENT_BUFFER",
        123 => "ERROR_INVALID_NAME",
        127 => "ERROR_PROC_NOT_FOUND",
        145 => "ERROR_DIR_NOT_EMPTY",
        206 => "ERROR_FILENAME_EXCED_RANGE",
        234 => "ERROR_MORE_DATA",
        267 => "ERROR_DIRECTORY",
        303 => "ERROR_DELETE_PENDING",
        1224 => "ERROR_USER_MAPPED_FILE",
        1314 => "ERROR_PRIVILEGE_NOT_HELD",
        1920 => "ERROR_CANT_ACCESS_FILE",
        1921 => "ERROR_CANT_RESOLVE_FILENAME",
        4390 => "ERROR_NOT_A_REPARSE_POINT",
        _ => "unknown",
    }
}

/// Symbolic name of an `NTSTATUS`, or `"unknown"`.
pub fn nt_name(status: i32) -> &'static str {
    // Compare as u32 so the table can use the documented hex spellings.
    match status as u32 {
        0x8000_0005 => "STATUS_BUFFER_OVERFLOW",
        0x8000_0006 => "STATUS_NO_MORE_FILES",
        0x8000_002D => "STATUS_STOPPED_ON_SYMLINK",
        0xC000_0004 => "STATUS_INFO_LENGTH_MISMATCH",
        0xC000_0008 => "STATUS_INVALID_HANDLE",
        0xC000_000D => "STATUS_INVALID_PARAMETER",
        0xC000_000F => "STATUS_NO_SUCH_FILE",
        0xC000_0010 => "STATUS_INVALID_DEVICE_REQUEST",
        0xC000_0022 => "STATUS_ACCESS_DENIED",
        0xC000_0033 => "STATUS_OBJECT_NAME_INVALID",
        0xC000_0034 => "STATUS_OBJECT_NAME_NOT_FOUND",
        0xC000_0035 => "STATUS_OBJECT_NAME_COLLISION",
        0xC000_003A => "STATUS_OBJECT_PATH_NOT_FOUND",
        0xC000_003B => "STATUS_OBJECT_PATH_SYNTAX_BAD",
        0xC000_0043 => "STATUS_SHARING_VIOLATION",
        0xC000_0054 => "STATUS_FILE_LOCK_CONFLICT",
        0xC000_0055 => "STATUS_LOCK_NOT_GRANTED",
        0xC000_0056 => "STATUS_DELETE_PENDING",
        0xC000_0061 => "STATUS_PRIVILEGE_NOT_HELD",
        0xC000_009A => "STATUS_INSUFFICIENT_RESOURCES",
        0xC000_00BA => "STATUS_FILE_IS_A_DIRECTORY",
        0xC000_00BB => "STATUS_NOT_SUPPORTED",
        0xC000_0101 => "STATUS_DIRECTORY_NOT_EMPTY",
        0xC000_0103 => "STATUS_NOT_A_DIRECTORY",
        0xC000_0106 => "STATUS_NAME_TOO_LONG",
        0xC000_0121 => "STATUS_CANNOT_DELETE",
        0xC000_0243 => "STATUS_USER_MAPPED_FILE",
        0xC000_0279 => "STATUS_IO_REPARSE_TAG_NOT_HANDLED",
        0xC000_0280 => "STATUS_REPARSE_POINT_NOT_RESOLVED",
        _ => "unknown",
    }
}

/// The OS code attached to a skip, for the log. `Sim` is a label produced by a test backend
/// or by a pure pre-check (for example `"bad_name"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    None,
    Win32(u32),
    Nt(i32),
    Sim(&'static str),
}

impl ErrorCode {
    /// The `code` field of a log record: `"0xC0000043"` (NT), `"win32:32"`, the sim label, or
    /// `None`.
    pub fn log_string(&self) -> Option<String> {
        match *self {
            ErrorCode::None => None,
            ErrorCode::Win32(c) => Some(format!("win32:{c}")),
            ErrorCode::Nt(s) => Some(format!("{:#010X}", s as u32).replacen("0X", "0x", 1)),
            ErrorCode::Sim(label) => Some(label.to_owned()),
        }
    }
}

/// Target-tier error: that root is abandoned, other targets continue.
#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("root missing")]
    Missing,
    #[error("root unavailable: {0}")]
    Unavailable(String),
    #[error("entry limit {limit} exceeded")]
    TooManyEntries { limit: u64 },
    #[error("resolve: {0}")]
    Resolve(#[from] ResolveError),
    #[error("fs: {0:?}")]
    Fs(FsError),
    #[error("cancelled")]
    Cancelled,
}

/// Fatal error: the operation stops and the log footer is written.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Safety(#[from] SafetyError),
    #[error(transparent)]
    Log(#[from] LogError),
    #[error("thread pool: {0}")]
    Pool(String),
    #[error("log dir required")]
    LogRequired,
}

/// Failure writing the deletion log.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_strings() {
        assert_eq!(ErrorCode::None.log_string(), None);
        assert_eq!(
            ErrorCode::Win32(32).log_string().as_deref(),
            Some("win32:32")
        );
        assert_eq!(
            ErrorCode::Nt(0xC000_0043_u32 as i32)
                .log_string()
                .as_deref(),
            Some("0xC0000043")
        );
        assert_eq!(
            ErrorCode::Sim("bad_name").log_string().as_deref(),
            Some("bad_name")
        );
    }

    #[test]
    fn display_uses_symbolic_names() {
        let e = Win32Error {
            api: "CreateFileW",
            code: 5,
        };
        assert_eq!(
            e.to_string(),
            "CreateFileW failed: ERROR_ACCESS_DENIED (0x5)"
        );
        let n = NtError {
            api: "NtCreateFile",
            status: 0xC000_0043_u32 as i32,
        };
        assert_eq!(
            n.to_string(),
            "NtCreateFile failed: STATUS_SHARING_VIOLATION (0xc0000043)"
        );
    }
}
