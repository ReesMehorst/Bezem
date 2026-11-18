// SPDX-License-Identifier: MIT

//! Error types for the win32 layer (ARCHITECTURE.md section 13.2).
//!
//! Every error keeps the name of the API that failed and the raw code it returned, so the
//! deletion log can record exactly what Windows said. Pure data: compiles on every platform.

use std::fmt;

/// Win32 error codes (`GetLastError` values) referenced by this module. Values from
/// winerror.h, see <https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--0-499->.
pub mod code {
    /// The system cannot find the file specified.
    pub const ERROR_FILE_NOT_FOUND: u32 = 2;
    /// Access is denied.
    pub const ERROR_ACCESS_DENIED: u32 = 5;
    /// The request is not supported (used for every API on non-Windows builds).
    pub const ERROR_NOT_SUPPORTED: u32 = 50;
    /// The parameter is incorrect.
    pub const ERROR_INVALID_PARAMETER: u32 = 87;
    /// This function is not supported on this system (used by not-yet-implemented wrappers).
    pub const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;
}

/// NTSTATUS values referenced by this module, from MS-ERREF section 2.3.1.
pub mod status {
    /// The requested operation is not implemented.
    pub const STATUS_NOT_IMPLEMENTED: i32 = 0xC000_0002_u32 as i32;
    /// The request is not supported.
    pub const STATUS_NOT_SUPPORTED: i32 = 0xC000_00BB_u32 as i32;
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
            code::ERROR_ACCESS_DENIED => "ERROR_ACCESS_DENIED",
            code::ERROR_NOT_SUPPORTED => "ERROR_NOT_SUPPORTED",
            code::ERROR_INVALID_PARAMETER => "ERROR_INVALID_PARAMETER",
            code::ERROR_CALL_NOT_IMPLEMENTED => "ERROR_CALL_NOT_IMPLEMENTED",
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
        Some(match self.status {
            status::STATUS_NOT_IMPLEMENTED => "STATUS_NOT_IMPLEMENTED",
            status::STATUS_NOT_SUPPORTED => "STATUS_NOT_SUPPORTED",
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

/// Either kind of failure, for wrappers that call both Win32 and native APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Error {
    /// A Win32 API failed.
    Win32(Win32Error),
    /// A native API failed.
    Nt(NtError),
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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win32(e) => e.fmt(f),
            Self::Nt(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Win32(e) => Some(e),
            Self::Nt(e) => Some(e),
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
            status: 0xC000_0043_u32 as i32,
        };
        assert_eq!(e.to_string(), "NtCreateFile failed: NTSTATUS 0xC0000043");
    }

    #[test]
    fn stub_errors_are_distinct() {
        assert_eq!(Win32Error::not_implemented("X").code, 120);
        assert_eq!(Win32Error::unsupported_platform("X").code, 50);
        assert_eq!(
            Error::from(NtError::unsupported_platform("X")).to_string(),
            "X failed: STATUS_NOT_SUPPORTED (0xC00000BB)"
        );
    }
}
