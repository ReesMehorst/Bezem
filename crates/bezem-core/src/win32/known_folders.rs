// SPDX-License-Identifier: MIT

//! Known Folder resolution (ARCHITECTURE.md sections 7.1 and 8.3, W2).
//!
//! **Signatures frozen at M1** (see `README.md`). Production code uses [`SystemKnownFolders`];
//! tests inject `FixedKnownFolders`. The trait is sealed, so no other implementation can
//! redirect the allowlisted bases.
//!
//! The APIs return the Win32 form `X:\a\b`. This module converts it to a [`WidePath`] by
//! pushing [`PathComponent`]s onto `WidePath::drive_root` ([`parse_win32_drive_path`]); anything
//! that does not fit (UNC, `\\?\`, relative, `/`, empty, `.` or `..` components) is an
//! [`Error::Path`]. The remaining rules of section 7.1 (too short, drive root, non-local) belong
//! to `resolve`.
//!
//! How callers must treat an error here (contract; implemented by S2/S3):
//! - for a folder used as a **token** base (section 7.1), the token is "Unresolved" and every
//!   target that uses it is reported, never guessed;
//! - for a **protected** folder (section 8.3), `SafetyPolicy::windows_default` fails as a whole.
//!   A protection is never silently dropped because its folder could not be resolved.

use super::error::{Error, HresultError, Win32Error};
use crate::path::{PathComponent, PathError, WidePath};

/// The folders Bezem resolves: the token bases of section 7.1 and the protected paths of
/// section 8.3. A closed set; each maps to one `FOLDERID_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) enum KnownFolder {
    /// `FOLDERID_LocalAppData` (`%LOCALAPPDATA%`).
    LocalAppData,
    /// `FOLDERID_LocalAppDataLow` (`%LOCALAPPDATALOW%`).
    LocalAppDataLow,
    /// `FOLDERID_RoamingAppData` (`%APPDATA%`).
    RoamingAppData,
    /// `FOLDERID_Recent` (`%RECENT%`).
    Recent,
    /// `FOLDERID_Profile` (`%USERPROFILE%`).
    Profile,
    /// `FOLDERID_ProgramData` (`%PROGRAMDATA%`).
    ProgramData,
    /// `FOLDERID_Windows` (`%WINDIR%`, `%SYSTEMROOT%`).
    Windows,
    /// `FOLDERID_ProgramFiles` (protected).
    ProgramFiles,
    /// `FOLDERID_ProgramFilesX86` (protected).
    ProgramFilesX86,
    /// `FOLDERID_Desktop` (protected).
    Desktop,
    /// `FOLDERID_Documents` (protected).
    Documents,
    /// `FOLDERID_Downloads` (protected).
    Downloads,
    /// `FOLDERID_Pictures` (protected).
    Pictures,
    /// `FOLDERID_Music` (protected).
    Music,
    /// `FOLDERID_Videos` (protected).
    Videos,
    /// `FOLDERID_SkyDrive`, i.e. OneDrive (protected).
    OneDrive,
}

mod sealed {
    /// Only implementations in this module may exist.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub trait Sealed {}
}

/// Source of Known Folder paths. Sealed; `Send + Sync` so one instance serves every thread.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) trait KnownFolders: sealed::Sealed + Send + Sync + std::fmt::Debug {
    /// `SHGetKnownFolderPath(KF_FLAG_DEFAULT)` (the buffer is freed with `CoTaskMemFree` inside
    /// the wrapper). A failing HRESULT is [`Error::Hresult`].
    fn folder(&self, folder: KnownFolder) -> Result<WidePath, Error>;

    /// The `%TEMP%` / `%TMP%` directory from `GetTempPath2W` (the SYSTEM account gets
    /// `C:\Windows\SystemTemp`), with its trailing `\` removed. **Not a Known Folder:**
    /// `GetTempPath2W` reads the `TMP`, `TEMP` and `USERPROFILE` environment variables, so the
    /// result is attacker-influenced for a same-user process and must pass the canonical
    /// checks in `resolve` and `safety` like any other root.
    fn temp_dir(&self) -> Result<WidePath, Error>;
}

/// The real implementation, backed by the Windows APIs (W2). Stub.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct SystemKnownFolders;

impl sealed::Sealed for SystemKnownFolders {}

impl KnownFolders for SystemKnownFolders {
    fn folder(&self, folder: KnownFolder) -> Result<WidePath, Error> {
        let _ = folder;
        let api = "SHGetKnownFolderPath";
        Err(if cfg!(windows) {
            HresultError::not_implemented(api)
        } else {
            HresultError::unsupported_platform(api)
        }
        .into())
    }

    fn temp_dir(&self) -> Result<WidePath, Error> {
        let api = "GetTempPath2W";
        Err(if cfg!(windows) {
            Win32Error::not_implemented(api)
        } else {
            Win32Error::unsupported_platform(api)
        }
        .into())
    }
}

/// Converts the Win32 drive form `X:\` or `X:\name(\name)*` (no trailing `\` except on the
/// root) into a [`WidePath`]. Every other form fails: `\\?\`, UNC, relative, `/` as a
/// separator, empty, `.` or `..` components. It never reopens what `parse_extended` closes.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn parse_win32_drive_path(units: &[u16]) -> Result<WidePath, PathError> {
    let (drive, rest) = match units {
        [d, colon, sep, rest @ ..] if *colon == u16::from(b':') && *sep == u16::from(b'\\') => {
            (*d, rest)
        }
        _ => return Err(PathError::NotExtendedDrivePath),
    };
    let letter = char::from_u32(u32::from(drive)).ok_or(PathError::NotExtendedDrivePath)?;
    let mut path = WidePath::drive_root(letter)?;
    if !rest.is_empty() {
        for name in rest.split(|&u| u == u16::from(b'\\')) {
            path.push(PathComponent::from_wide(name)?)?;
        }
    }
    Ok(path)
}

/// [`parse_win32_drive_path`] for `GetTempPath2W` output, which always ends in `\`: removes
/// exactly one trailing `\` (unless the result would be the bare `X:`) before parsing. So
/// `C:\` yields the drive root `\\?\C:\`: this function accepts it, and `resolve` must reject
/// it (drive roots are never a valid temp base, section 7.1).
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn parse_temp_path(units: &[u16]) -> Result<WidePath, PathError> {
    match units {
        [init @ .., last] if init.len() > 2 && *last == u16::from(b'\\') => {
            parse_win32_drive_path(init)
        }
        _ => parse_win32_drive_path(units),
    }
}

/// Test override: fixed paths, typically into a throwaway fixture tree. Built only for unit
/// tests; a `test-support` re-export can follow if other crates need it. A folder that was not
/// set fails like an unresolvable one.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct FixedKnownFolders {
    folders: std::collections::BTreeMap<KnownFolder, WidePath>,
    temp: Option<WidePath>,
}

#[cfg(test)]
impl FixedKnownFolders {
    /// No folders set.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Sets the path returned for `folder`.
    pub(crate) fn with(mut self, folder: KnownFolder, path: WidePath) -> Self {
        self.folders.insert(folder, path);
        self
    }

    /// Sets the path returned by [`KnownFolders::temp_dir`].
    pub(crate) fn with_temp(mut self, path: WidePath) -> Self {
        self.temp = Some(path);
        self
    }
}

#[cfg(test)]
impl sealed::Sealed for FixedKnownFolders {}

#[cfg(test)]
impl KnownFolders for FixedKnownFolders {
    fn folder(&self, folder: KnownFolder) -> Result<WidePath, Error> {
        self.folders
            .get(&folder)
            .cloned()
            .ok_or(Error::Hresult(HresultError {
                api: "SHGetKnownFolderPath",
                hr: 0x8007_0002_u32 as i32, // HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)
            }))
    }

    fn temp_dir(&self) -> Result<WidePath, Error> {
        self.temp.clone().ok_or(Error::Win32(Win32Error {
            api: "GetTempPath2W",
            code: super::error::code::ERROR_FILE_NOT_FOUND,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn win32_drive_paths() {
        let p = parse_win32_drive_path(&w(r"c:\Users\Ann\AppData\Local")).unwrap();
        assert_eq!(p.to_wide(), w(r"\\?\C:\Users\Ann\AppData\Local"));
        assert!(parse_win32_drive_path(&w(r"C:\")).unwrap().is_drive_root());
        for bad in [
            "",
            "C:",
            "C:Temp",
            r"\Temp",
            r"\\?\C:\Temp",
            r"\\server\share",
            r"C:\a\",
            r"C:\a\\b",
            r"C:\a\..\b",
            r"C:\.",
            "C:/a",
            r"C:\a/b",
            r"1:\a",
            r"C:\a:b",
        ] {
            assert!(parse_win32_drive_path(&w(bad)).is_err(), "{bad:?} accepted");
        }
        // Device and NT prefixes, a fullwidth drive letter, an ADS suffix, a trailing NUL.
        for bad in [
            r"\\.\C:\x",
            r"\??\C:\x",
            "\u{FF23}:\\x",
            r"C:\a:$DATA",
            "C:\\a\0",
        ] {
            assert!(parse_win32_drive_path(&w(bad)).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn temp_path_strips_one_separator() {
        let p = parse_temp_path(&w(r"C:\Users\Ann\AppData\Local\Temp\")).unwrap();
        assert_eq!(p.to_wide(), w(r"\\?\C:\Users\Ann\AppData\Local\Temp"));
        assert!(parse_temp_path(&w(r"C:\Windows\SystemTemp")).is_ok());
        assert!(parse_temp_path(&w(r"C:\")).unwrap().is_drive_root());
        assert!(parse_temp_path(&w(r"C:\Temp\\")).is_err());
    }

    #[test]
    fn override_returns_only_what_was_set() {
        let local = parse_win32_drive_path(&w(r"C:\fixture\Local")).unwrap();
        let temp = parse_win32_drive_path(&w(r"C:\fixture\Temp")).unwrap();
        let k = FixedKnownFolders::new()
            .with(KnownFolder::LocalAppData, local.clone())
            .with_temp(temp.clone());
        assert_eq!(k.folder(KnownFolder::LocalAppData), Ok(local));
        assert_eq!(k.temp_dir(), Ok(temp));
        let dyn_k: &dyn KnownFolders = &k;
        assert!(matches!(
            dyn_k.folder(KnownFolder::Windows),
            Err(Error::Hresult(_))
        ));
    }

    #[test]
    fn system_stub_fails_closed() {
        assert!(SystemKnownFolders
            .folder(KnownFolder::LocalAppData)
            .is_err());
        assert!(SystemKnownFolders.temp_dir().is_err());
    }
}
