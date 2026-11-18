// SPDX-License-Identifier: MIT

//! `set_disposition`: `FileDispositionInfoEx` (DELETE | POSIX_SEMANTICS |
//! FORCE_IMAGE_SECTION_CHECK | optional IGNORE_READONLY_ATTRIBUTE), falling back to
//! `FileDispositionInfo { DeleteFile = TRUE }` on `ERROR_INVALID_PARAMETER`,
//! `ERROR_NOT_SUPPORTED` or `ERROR_INVALID_FUNCTION`. Attributes are never cleared.
//!
//! Ownership: work package **A**.

use core::ffi::c_void;
use core::mem::size_of;

use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, FileDispositionInfoEx, SetFileInformationByHandle,
    FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_INFO_BY_HANDLE_CLASS,
};

use super::info::attribute_tag_info;
use super::{win32_code, OwnedHandle};
use crate::fs::status::{self, ERROR_ACCESS_DENIED};
use crate::fs::{attr, DispositionError, DispositionKind, DispositionOpts, FsError};
use crate::types::SkipReason;

/// Sets one disposition struct. Returns the Win32 code on failure.
///
/// # Safety
///
/// `T` must be the documented input struct for `class` (`FILE_DISPOSITION_INFO_EX` for
/// `FileDispositionInfoEx`, `FILE_DISPOSITION_INFO` for `FileDispositionInfo`).
unsafe fn set_info<T>(
    h: &OwnedHandle,
    class: FILE_INFO_BY_HANDLE_CLASS,
    info: &T,
) -> Result<(), u32> {
    // Disposition structs are 1 or 4 bytes: the cast cannot truncate.
    let len = size_of::<T>() as u32;
    // SAFETY: `h` is an open handle (OwnedHandle invariant, kept alive by the borrow) opened
    // with DELETE access by `open_item`; `info` is a live, initialised `T` of exactly `len`
    // bytes that the API only reads; the caller guarantees `T` matches `class`.
    unsafe { SetFileInformationByHandle(h.raw(), class, (info as *const T).cast::<c_void>(), len) }
        .map_err(|e| win32_code(&e))
}

/// Marks the item for deletion. `ERROR_DIR_NOT_EMPTY` => `NotEmpty`; `ERROR_ACCESS_DENIED` on
/// an item whose `READONLY` attribute was not overridden (legacy fallback, or
/// `ignore_readonly` false) => `ReadOnly`; `ERROR_USER_MAPPED_FILE` => `Locked`; others via
/// the status table. Never clears attributes; never retries with a weaker method other than
/// the documented legacy class when the Ex class itself is unsupported.
pub(crate) fn set_disposition(
    h: &OwnedHandle,
    opts: DispositionOpts,
) -> Result<DispositionKind, DispositionError> {
    let mut flags = FILE_DISPOSITION_FLAG_DELETE.0
        | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0
        | FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK.0;
    if opts.ignore_readonly {
        flags |= FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0;
    }
    let ex = FILE_DISPOSITION_INFO_EX {
        Flags: windows::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO_EX_FLAGS(flags),
    };
    // SAFETY: FILE_DISPOSITION_INFO_EX is the documented input of FileDispositionInfoEx.
    let code = match unsafe { set_info(h, FileDispositionInfoEx, &ex) } {
        Ok(()) => return Ok(DispositionKind::Posix),
        Err(code) => code,
    };
    if !status::is_disposition_unsupported_win32(code) {
        return Err(map_failure(h, code, opts.ignore_readonly));
    }

    // Ex class (or one of its flags) unsupported: FAT/exFAT, older builds, some filters.
    let legacy = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: FILE_DISPOSITION_INFO is the documented input of FileDispositionInfo.
    match unsafe { set_info(h, FileDispositionInfo, &legacy) } {
        Ok(()) => Ok(DispositionKind::Legacy),
        // The legacy class cannot override READONLY.
        Err(code) => Err(map_failure(h, code, false)),
    }
}

fn map_failure(h: &OwnedHandle, code: u32, readonly_overridden: bool) -> DispositionError {
    if status::is_dir_not_empty_win32(code) {
        return DispositionError::NotEmpty;
    }
    let (mut reason, ecode) = status::from_win32(code);
    if code == ERROR_ACCESS_DENIED && !readonly_overridden {
        // Report the fact that explains the denial. If the attributes cannot be read the
        // generic AccessDenied stands (still a skip).
        if let Ok((attrs, _)) = attribute_tag_info(h) {
            if attrs & attr::READONLY != 0 {
                reason = SkipReason::ReadOnly;
            }
        }
    }
    DispositionError::Fs(FsError::new(reason, ecode))
}
