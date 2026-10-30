// SPDX-License-Identifier: MIT

//! Error types for the win32 layer (ARCHITECTURE.md section 13.2).
//!
//! Every error keeps the name of the API that failed and the raw code it returned, so the
//! deletion log can record exactly what Windows said. Pure data: compiles on every platform.

use std::fmt;

use crate::path::PathError;

/// Win32 error codes (`GetLastError` values) referenced by this module. Values from
/// winerror.h, see <https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--0-499->.
pub mod code {
    /// The system cannot find the file specified.
    pub const ERROR_FILE_NOT_FOUND: u32 = 2;
    /// The system cannot find the path specified.
    pub const ERROR_PATH_NOT_FOUND: u32 = 3;
    /// Access is denied.
    pub const ERROR_ACCESS_DENIED: u32 = 5;
    /// The file is in use by another process.
    pub const ERROR_SHARING_VIOLATION: u32 = 32;
    /// The request is not supported (used for every API on non-Windows builds).
    pub const ERROR_NOT_SUPPORTED: u32 = 50;
    /// The parameter is incorrect.
    pub const ERROR_INVALID_PARAMETER: u32 = 87;
    /// This function is not supported on this system (used by not-yet-implemented wrappers).
    pub const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;
    /// The directory is not empty.
    pub const ERROR_DIR_NOT_EMPTY: u32 = 145;
    /// The file is too large (used by `read_to_end` when a file exceeds its cap).
    pub const ERROR_FILE_TOO_LARGE: u32 = 223;
}

/// NTSTATUS values referenced by this module, from MS-ERREF section 2.3.1.
pub mod status {
    /// The requested operation is not implemented.
    pub const STATUS_NOT_IMPLEMENTED: i32 = 0xC000_0002_u32 as i32;
    /// Access denied.
    pub const STATUS_ACCESS_DENIED: i32 = 0xC000_0022_u32 as i32;
    /// The object name is invalid.
    pub const STATUS_OBJECT_NAME_INVALID: i32 = 0xC000_0033_u32 as i32;
    /// The object name is not found.
    pub const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034_u32 as i32;
    /// The path does not exist.
    pub const STATUS_OBJECT_PATH_NOT_FOUND: i32 = 0xC000_003A_u32 as i32;
    /// Share access flags are incompatible with an existing open.
    pub const STATUS_SHARING_VIOLATION: i32 = 0xC000_0043_u32 as i32;
    /// The file has a delete pending.
    pub const STATUS_DELETE_PENDING: i32 = 0xC000_0056_u32 as i32;
    /// A non-directory open hit a directory.
    pub const STATUS_FILE_IS_A_DIRECTORY: i32 = 0xC000_00BA_u32 as i32;
    /// The request is not supported.
    pub const STATUS_NOT_SUPPORTED: i32 = 0xC000_00BB_u32 as i32;
    /// The directory to delete is not empty.
    pub const STATUS_DIRECTORY_NOT_EMPTY: i32 = 0xC000_0101_u32 as i32;
    /// A directory open hit something that is not a directory.
    pub const STATUS_NOT_A_DIRECTORY: i32 = 0xC000_0103_u32 as i32;
    /// A reparse point was encountered (ntstatus.h). `nt_open_dir` / `nt_open_read_file` return
    /// it when the opened object is a reparse point that is not a cloud placeholder.
    pub const STATUS_REPARSE_POINT_ENCOUNTERED: i32 = 0xC000_050B_u32 as i32;
    /// First and last error codes of the Cloud Files facility (`STATUS_CLOUD_FILE_*`).
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub const STATUS_CLOUD_FILE_FIRST: i32 = 0xC000_CF00_u32 as i32;
    /// See [`STATUS_CLOUD_FILE_FIRST`].
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub const STATUS_CLOUD_FILE_LAST: i32 = 0xC000_CFFF_u32 as i32;
    /// **Bezem-defined** (customer bit 29 set, so Windows never returns it): the opened object
    /// is a cloud or projected placeholder (`fs::is_cloud_placeholder`).
    pub const STATUS_BEZEM_CLOUD_PLACEHOLDER: i32 = 0xE000_0001_u32 as i32;
}

/// HRESULT values referenced by this module (winerror.h).
pub mod hresult {
    /// Not implemented.
    pub const E_NOTIMPL: i32 = 0x8000_4001_u32 as i32;
    /// `HRESULT_FROM_WIN32(ERROR_NOT_SUPPORTED)`.
    pub const E_NOT_SUPPORTED: i32 = 0x8007_0032_u32 as i32;
}

/// A failed Win32 call: the API name and its `GetLastError` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Win32Error {
    /// Name of the Windows API that failed, e.g. `"CreateFileW"`.
    pub api: &'static str,
    /// The Win32 error code.
    pub code: u32,
}

impl Win32Error {
    /// The wrapper for `api` exists but its body is not implemented yet
    /// (`ERROR_CALL_NOT_IMPLEMENTED`). Callers must treat it like any other failure: skip.
    pub const fn not_implemented(api: &'static str) -> Self {
        Self {
            api,
            code: code::ERROR_CALL_NOT_IMPLEMENTED,
        }
    }

    /// `api` does not exist on this platform (`ERROR_NOT_SUPPORTED`); returned by every
    /// wrapper on non-Windows builds so pure-logic code compiles and tests on Linux.
    pub const fn unsupported_platform(api: &'static str) -> Self {
        Self {
            api,
            code: code::ERROR_NOT_SUPPORTED,
        }
    }

    fn symbol(self) -> Option<&'static str> {
        Some(match self.code {
            code::ERROR_FILE_NOT_FOUND => "ERROR_FILE_NOT_FOUND",
            code::ERROR_PATH_NOT_FOUND => "ERROR_PATH_NOT_FOUND",
            code::ERROR_ACCESS_DENIED => "ERROR_ACCESS_DENIED",
            code::ERROR_SHARING_VIOLATION => "ERROR_SHARING_VIOLATION",
            code::ERROR_NOT_SUPPORTED => "ERROR_NOT_SUPPORTED",
            code::ERROR_INVALID_PARAMETER => "ERROR_INVALID_PARAMETER",
            code::ERROR_CALL_NOT_IMPLEMENTED => "ERROR_CALL_NOT_IMPLEMENTED",
            code::ERROR_DIR_NOT_EMPTY => "ERROR_DIR_NOT_EMPTY",
            code::ERROR_FILE_TOO_LARGE => "ERROR_FILE_TOO_LARGE",
            _ => return None,
        })
    }
}

impl fmt::Display for Win32Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.symbol() {
            Some(name) => write!(f, "{} failed: {name} ({})", self.api, self.code),
            None => write!(f, "{} failed: Win32 error {}", self.api, self.code),
        }
    }
}

impl std::error::Error for Win32Error {}

/// A failed native (`Nt*`) call: the API name and its NTSTATUS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NtError {
    /// Name of the native API that failed, e.g. `"NtCreateFile"`.
    pub api: &'static str,
    /// The NTSTATUS (negative for errors).
    pub status: i32,
}

impl NtError {
    /// Not implemented yet (`STATUS_NOT_IMPLEMENTED`); see [`Win32Error::not_implemented`].
    pub const fn not_implemented(api: &'static str) -> Self {
        Self {
            api,
            status: status::STATUS_NOT_IMPLEMENTED,
        }
    }

    /// Not available on this platform (`STATUS_NOT_SUPPORTED`).
    pub const fn unsupported_platform(api: &'static str) -> Self {
        Self {
            api,
            status: status::STATUS_NOT_SUPPORTED,
        }
    }

    fn symbol(self) -> Option<&'static str> {
        use status::*;
        Some(match self.status {
            STATUS_NOT_IMPLEMENTED => "STATUS_NOT_IMPLEMENTED",
            STATUS_ACCESS_DENIED => "STATUS_ACCESS_DENIED",
            STATUS_OBJECT_NAME_INVALID => "STATUS_OBJECT_NAME_INVALID",
            STATUS_OBJECT_NAME_NOT_FOUND => "STATUS_OBJECT_NAME_NOT_FOUND",
            STATUS_OBJECT_PATH_NOT_FOUND => "STATUS_OBJECT_PATH_NOT_FOUND",
            STATUS_SHARING_VIOLATION => "STATUS_SHARING_VIOLATION",
            STATUS_DELETE_PENDING => "STATUS_DELETE_PENDING",
            STATUS_FILE_IS_A_DIRECTORY => "STATUS_FILE_IS_A_DIRECTORY",
            STATUS_NOT_SUPPORTED => "STATUS_NOT_SUPPORTED",
            STATUS_DIRECTORY_NOT_EMPTY => "STATUS_DIRECTORY_NOT_EMPTY",
            STATUS_NOT_A_DIRECTORY => "STATUS_NOT_A_DIRECTORY",
            STATUS_REPARSE_POINT_ENCOUNTERED => "STATUS_REPARSE_POINT_ENCOUNTERED",
            STATUS_BEZEM_CLOUD_PLACEHOLDER => "STATUS_BEZEM_CLOUD_PLACEHOLDER",
            _ => return None,
        })
    }
}

impl fmt::Display for NtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // NTSTATUS values are conventionally shown as 32-bit hex.
        let hex = self.status as u32;
        match self.symbol() {
            Some(name) => write!(f, "{} failed: {name} (0x{hex:08X})", self.api),
            None => write!(f, "{} failed: NTSTATUS 0x{hex:08X}", self.api),
        }
    }
}

impl std::error::Error for NtError {}

/// A failed COM or shell call: the API name and its HRESULT (e.g. `SHGetKnownFolderPath`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HresultError {
    /// Name of the API that failed.
    pub api: &'static str,
    /// The failing HRESULT (negative).
    pub hr: i32,
}

impl HresultError {
    /// Not implemented yet (`E_NOTIMPL`).
    pub const fn not_implemented(api: &'static str) -> Self {
        Self {
            api,
            hr: hresult::E_NOTIMPL,
        }
    }

    /// Not available on this platform (`HRESULT_FROM_WIN32(ERROR_NOT_SUPPORTED)`).
    pub const fn unsupported_platform(api: &'static str) -> Self {
        Self {
            api,
            hr: hresult::E_NOT_SUPPORTED,
        }
    }
}

impl fmt::Display for HresultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.hr {
            hresult::E_NOTIMPL => "E_NOTIMPL ",
            _ => "",
        };
        write!(
            f,
            "{} failed: {name}HRESULT 0x{:08X}",
            self.api, self.hr as u32
        )
    }
}

impl std::error::Error for HresultError {}

/// Any win32-layer failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A Win32 API failed.
    Win32(Win32Error),
    /// A native API failed.
    Nt(NtError),
    /// A COM or shell API failed.
    Hresult(HresultError),
    /// An API succeeded but returned a path that is not a valid `WidePath` (fail closed).
    Path {
        /// The API whose output was rejected.
        api: &'static str,
        /// Why it was rejected.
        reason: PathError,
    },
}

impl From<Win32Error> for Error {
    fn from(e: Win32Error) -> Self {
        Self::Win32(e)
    }
}

impl From<NtError> for Error {
    fn from(e: NtError) -> Self {
        Self::Nt(e)
    }
}

impl From<HresultError> for Error {
    fn from(e: HresultError) -> Self {
        Self::Hresult(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win32(e) => e.fmt(f),
            Self::Nt(e) => e.fmt(f),
            Self::Hresult(e) => e.fmt(f),
            Self::Path { api, reason } => write!(f, "{api} returned an unusable path: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Win32(e) => Some(e),
            Self::Nt(e) => Some(e),
            Self::Hresult(e) => Some(e),
            Self::Path { reason, .. } => Some(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_keeps_api_and_code() {
        let e = Win32Error {
            api: "CreateFileW",
            code: 5,
        };
        assert_eq!(e.to_string(), "CreateFileW failed: ERROR_ACCESS_DENIED (5)");
        let e = Win32Error {
            api: "CreateFileW",
            code: 1234,
        };
        assert_eq!(e.to_string(), "CreateFileW failed: Win32 error 1234");
        let e = NtError::not_implemented("NtCreateFile");
        assert_eq!(
            e.to_string(),
            "NtCreateFile failed: STATUS_NOT_IMPLEMENTED (0xC0000002)"
        );
        let e = NtError {
            api: "NtCreateFile",
            status: status::STATUS_NOT_A_DIRECTORY,
        };
        assert_eq!(
            e.to_string(),
            "NtCreateFile failed: STATUS_NOT_A_DIRECTORY (0xC0000103)"
        );
        let e = NtError {
            api: "NtCreateFile",
            status: 0xC000_9999_u32 as i32,
        };
        assert_eq!(e.to_string(), "NtCreateFile failed: NTSTATUS 0xC0009999");
        let e = HresultError {
            api: "SHGetKnownFolderPath",
            hr: 0x8007_0002_u32 as i32,
        };
        assert_eq!(
            e.to_string(),
            "SHGetKnownFolderPath failed: HRESULT 0x80070002"
        );
        let e = Error::Path {
            api: "GetTempPath2W",
            reason: PathError::EmptyComponent,
        };
        assert_eq!(
            e.to_string(),
            "GetTempPath2W returned an unusable path: empty path component"
        );
    }

    #[test]
    fn stub_errors_are_distinct() {
        assert_eq!(Win32Error::not_implemented("X").code, 120);
        assert_eq!(Win32Error::unsupported_platform("X").code, 50);
        assert_eq!(
            Error::from(NtError::unsupported_platform("X")).to_string(),
            "X failed: STATUS_NOT_SUPPORTED (0xC00000BB)"
        );
        assert_eq!(
            Error::from(HresultError::not_implemented("X")).to_string(),
            "X failed: E_NOTIMPL HRESULT 0x80004001"
        );
    }
}
