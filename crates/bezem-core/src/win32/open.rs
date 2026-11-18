// SPDX-License-Identifier: MIT

//! `open_root`: `CreateFileW` on a `\\?\` path, then attribute-tag info, `FileIdInfo`, final
//! path and the volume probe.
//!
//! Ownership: work package **A**.

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_TRAVERSE, OPEN_EXISTING, SYNCHRONIZE,
};

use super::finalpath::final_path;
use super::info::{attribute_tag_info, file_id_info};
use super::volume::volume_kind;
use super::{win32_code, OwnedHandle};
use crate::error::{NtError, Win32Error};
use crate::fs::status::{self, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use crate::fs::{attr, FsError, OpenRootError, RootOpenMode, RootProbe};
use crate::path::WidePath;

/// `ERROR_INVALID_HANDLE`: reported if a successful open ever yielded an unusable handle.
const ERROR_INVALID_HANDLE: u32 = 6;

fn fs_win32(e: Win32Error) -> OpenRootError {
    let (reason, code) = status::from_win32(e.code);
    OpenRootError::Fs(FsError::new(reason, code))
}

fn fs_nt(e: NtError) -> OpenRootError {
    let (reason, code) = status::from_nt(e.status);
    OpenRootError::Fs(FsError::new(reason, code))
}

/// `FILE_FLAG_BACKUP_SEMANTICS`, plus `FILE_FLAG_OPEN_REPARSE_POINT` for `VerifyNoFollow`.
/// `ERROR_FILE_NOT_FOUND`/`ERROR_PATH_NOT_FOUND` => `OpenRootError::NotFound`.
///
/// Access is `FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share
/// `FILE_SHARE_READ | FILE_SHARE_WRITE` (no `FILE_SHARE_DELETE`: the open pins the directory).
/// The probe reports facts only (`is_directory`, `is_reparse`, final path, volume kind); the
/// policy decides.
pub(crate) fn open_root(
    path: &WidePath,
    mode: RootOpenMode,
) -> Result<RootProbe<OwnedHandle>, OpenRootError> {
    let wide = path.to_wide_nul();
    let access = FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
    let mut flags = FILE_FLAG_BACKUP_SEMANTICS.0;
    if mode == RootOpenMode::VerifyNoFollow {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT.0;
    }

    // SAFETY: `wide` is a live, NUL-terminated UTF-16 buffer (`to_wide_nul`) that the API only
    // reads; no security attributes and no template handle are passed. The returned handle,
    // if any, is owned by nobody else yet.
    let raw = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(flags),
            None,
        )
    };
    let raw = match raw {
        Ok(h) => h,
        Err(e) => {
            let code = win32_code(&e);
            if code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND {
                return Err(OpenRootError::NotFound);
            }
            return Err(fs_win32(Win32Error {
                api: "CreateFileW",
                code,
            }));
        }
    };
    // SAFETY: CreateFileW succeeded, so `raw` is a new open handle that nothing else owns;
    // ownership moves into the OwnedHandle.
    let Some(dir) = (unsafe { OwnedHandle::from_raw(raw) }) else {
        return Err(fs_win32(Win32Error {
            api: "CreateFileW",
            code: ERROR_INVALID_HANDLE,
        }));
    };

    let (attributes, reparse_tag) = attribute_tag_info(&dir).map_err(fs_win32)?;
    let identity = file_id_info(&dir).map_err(fs_win32)?;
    let final_path = final_path(&dir).map_err(fs_win32)?;
    let volume = volume_kind(&dir, &final_path).map_err(fs_nt)?;
    Ok(RootProbe {
        dir,
        final_path,
        identity,
        is_directory: attributes & attr::DIRECTORY != 0,
        is_reparse: attr::is_reparse(attributes, reparse_tag),
        volume,
    })
}
