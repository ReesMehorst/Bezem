// SPDX-License-Identifier: MIT

//! [`SystemKnownFolders`]: known-folder values from the OS. `Temp` via `GetTempPath2W`
//! (resolved at runtime, `GetTempPathW` fallback; computed once). Others via
//! `SHGetKnownFolderPath(KF_FLAG_DEFAULT)`, freed with `CoTaskMemFree` in a drop guard.
//!
//! Ownership: work package **A**.
//!
//! Values are returned raw (only the trailing `\` of the temp path is stripped); validation
//! (`WidePath::from_known_value`, scope rules) is the caller's job. The environment is never
//! read here.

use std::sync::OnceLock;

use windows::core::{s, w, GUID, HRESULT, PWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::GetTempPathW;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
    FOLDERID_LocalAppDataLow, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile,
    FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Recent,
    FOLDERID_RoamingAppData, FOLDERID_SkyDrive, FOLDERID_Videos, FOLDERID_Windows, KF_FLAG_DEFAULT,
};

use super::last_error;
use crate::error::Win32Error;
use crate::resolve::{KnownFolderError, KnownFolderId, KnownFolderSource};

/// The production [`KnownFolderSource`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemKnownFolders;

impl KnownFolderSource for SystemKnownFolders {
    fn get(&self, id: KnownFolderId) -> Result<Vec<u16>, KnownFolderError> {
        match folder_guid(id) {
            None => temp_path(),
            Some(guid) => known_folder_path(guid),
        }
    }
}

/// The `KNOWNFOLDERID` for `id`; `None` for `Temp` (not a known folder: `GetTempPath2W`).
fn folder_guid(id: KnownFolderId) -> Option<&'static GUID> {
    Some(match id {
        KnownFolderId::Temp => return None,
        KnownFolderId::LocalAppData => &FOLDERID_LocalAppData,
        KnownFolderId::LocalAppDataLow => &FOLDERID_LocalAppDataLow,
        KnownFolderId::RoamingAppData => &FOLDERID_RoamingAppData,
        KnownFolderId::Recent => &FOLDERID_Recent,
        KnownFolderId::Profile => &FOLDERID_Profile,
        KnownFolderId::ProgramData => &FOLDERID_ProgramData,
        KnownFolderId::Windows => &FOLDERID_Windows,
        KnownFolderId::ProgramFiles => &FOLDERID_ProgramFiles,
        KnownFolderId::ProgramFilesX86 => &FOLDERID_ProgramFilesX86,
        KnownFolderId::Desktop => &FOLDERID_Desktop,
        KnownFolderId::Documents => &FOLDERID_Documents,
        KnownFolderId::Downloads => &FOLDERID_Downloads,
        KnownFolderId::Pictures => &FOLDERID_Pictures,
        KnownFolderId::Music => &FOLDERID_Music,
        KnownFolderId::Videos => &FOLDERID_Videos,
        // FOLDERID_SkyDrive == FOLDERID_OneDrive (same GUID); the brief names SkyDrive.
        KnownFolderId::OneDrive => &FOLDERID_SkyDrive,
    })
}

/// Frees a `CoTaskMemAlloc` string on drop (also when null: `CoTaskMemFree(NULL)` is a no-op).
struct CoTaskMemGuard(PWSTR);

impl Drop for CoTaskMemGuard {
    fn drop(&mut self) {
        // SAFETY: `self.0` is either null or the pointer SHGetKnownFolderPath stored in its
        // out-parameter, which the caller must free with CoTaskMemFree exactly once "whether
        // SHGetKnownFolderPath succeeds or not" (Microsoft Learn). The guard is the only owner
        // and drop runs once; no borrow of the string outlives the guard.
        unsafe { CoTaskMemFree(Some(self.0.as_ptr().cast_const().cast())) };
    }
}

/// `SHGetKnownFolderPath(rfid, KF_FLAG_DEFAULT, NULL)`, copied out and freed.
fn known_folder_path(rfid: &GUID) -> Result<Vec<u16>, KnownFolderError> {
    // Declared through the `windows` crate's own linker macro (same ABI as the crate's
    // declaration) instead of calling the crate wrapper: the wrapper discards the
    // out-pointer on failure, but the docs require freeing it on failure too.
    windows::core::link!("shell32.dll" "system" fn SHGetKnownFolderPath(rfid: *const GUID, dwflags: u32, htoken: HANDLE, ppszpath: *mut PWSTR) -> HRESULT);

    let mut raw = PWSTR::null();
    // SAFETY: `rfid` points at a live GUID; flags are KF_FLAG_DEFAULT (0, so the cast is
    // lossless); a null token means the current user; `raw` is a live, writable out-pointer.
    let hr = unsafe {
        SHGetKnownFolderPath(rfid, KF_FLAG_DEFAULT.0 as u32, HANDLE::default(), &mut raw)
    };
    let guard = CoTaskMemGuard(raw);
    if hr.is_err() {
        return Err(KnownFolderError::Failed(format!(
            "SHGetKnownFolderPath failed: HRESULT {:#010x}",
            hr.0 as u32
        )));
    }
    if guard.0.is_null() {
        return Err(KnownFolderError::Failed(
            "SHGetKnownFolderPath returned a null path".to_owned(),
        ));
    }
    // SAFETY: on success the API stored a non-null pointer to a NUL-terminated string that
    // stays valid until the guard frees it; the slice is copied before `guard` drops.
    let value = unsafe { guard.0.as_wide() }.to_vec();
    drop(guard);
    Ok(value)
}

/// Signature of `GetTempPath2W` (fileapi.h): `DWORD (DWORD BufferLength, LPWSTR Buffer)`.
type GetTempPath2Fn = unsafe extern "system" fn(u32, *mut u16) -> u32;

/// `GetTempPath2W` looked up at runtime: it is missing from kernel32 on Windows 10 builds
/// without the March 2025 update, and a static import would stop the binary from loading.
fn get_temp_path2() -> Option<GetTempPath2Fn> {
    // SAFETY: both arguments are static NUL-terminated literals. GetModuleHandleW does not
    // increment a reference count; kernel32 is mapped in every Win32 process for its whole
    // lifetime, so the module handle (and any export address) stays valid.
    let module = unsafe { GetModuleHandleW(w!("kernel32.dll")) }.ok()?;
    // SAFETY: as above; GetProcAddress only reads the export table of a loaded module.
    let proc = unsafe { GetProcAddress(module, s!("GetTempPath2W")) }?;
    // SAFETY: the export named GetTempPath2W has exactly the documented signature
    // `DWORD WINAPI GetTempPath2W(DWORD, LPWSTR)`, i.e. `extern "system" fn(u32, *mut u16)
    // -> u32`; transmuting between function pointer types of the same ABI is sound when the
    // target type matches the real signature.
    Some(unsafe {
        core::mem::transmute::<unsafe extern "system" fn() -> isize, GetTempPath2Fn>(proc)
    })
}

/// Calls `GetTempPath2W` (or `GetTempPathW`) with growth; returns the path without NUL.
fn query_temp() -> Result<Vec<u16>, Win32Error> {
    let api2 = get_temp_path2();
    let api = if api2.is_some() {
        "GetTempPath2W"
    } else {
        "GetTempPathW"
    };
    // MAX_PATH + 1 plus the NUL; the documented maximum return is MAX_PATH + 1.
    let mut buf = vec![0u16; 262];
    for _ in 0..3 {
        // Bounded by the growth rule below (<= 32_768 + 1): fits u32.
        let len = buf.len() as u32;
        let n = match api2 {
            // SAFETY: `f` is GetTempPath2W (see `get_temp_path2`); `buf` is a live, writable
            // buffer of exactly `len` units, and the API writes at most `len` units.
            Some(f) => unsafe { f(len, buf.as_mut_ptr()) },
            // SAFETY: `buf` is a live, writable slice; the wrapper passes its exact length.
            None => unsafe { GetTempPathW(Some(&mut buf)) },
        } as usize;
        if n == 0 {
            return Err(Win32Error {
                api,
                code: last_error(),
            });
        }
        if n < buf.len() {
            buf.truncate(n);
            return Ok(buf);
        }
        if n > 32_768 {
            break;
        }
        // `n` is the required size; +1 in case it excludes the NUL.
        buf.resize(n + 1, 0);
    }
    Err(Win32Error {
        api,
        code: crate::fs::status::ERROR_INSUFFICIENT_BUFFER,
    })
}

/// The temp path, resolved once per process (A:734); one trailing `\` stripped.
fn temp_path() -> Result<Vec<u16>, KnownFolderError> {
    static TEMP: OnceLock<Result<Vec<u16>, KnownFolderError>> = OnceLock::new();
    TEMP.get_or_init(|| {
        let mut v = query_temp().map_err(|e| KnownFolderError::Failed(e.to_string()))?;
        if v.last() == Some(&u16::from(b'\\')) {
            v.pop();
        }
        Ok(v)
    })
    .clone()
}
