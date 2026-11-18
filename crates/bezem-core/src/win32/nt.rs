// SPDX-License-Identifier: MIT

//! `nt_open_relative`: `NtCreateFile` relative to a parent directory handle
//! (`OBJECT_ATTRIBUTES.RootDirectory`), `FILE_OPEN`, `OBJ_CASE_INSENSITIVE`.
//!
//! Ownership: work package **A**.

use core::ptr;

use windows::core::PWSTR;
use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{NtCreateFile, FILE_OPEN, NTCREATEFILE_CREATE_OPTIONS};
use windows::Win32::Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING};
use windows::Win32::Storage::FileSystem::{
    FILE_ACCESS_RIGHTS, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;

use super::OwnedHandle;
use crate::error::NtError;
use crate::fs::{check_open_name, status};

/// Longest single component, in UTF-16 units (NTFS/ReFS limit). Keeps
/// `UNICODE_STRING.Length` (bytes, `u16`) far from overflow.
const MAX_COMPONENT_UNITS: usize = 255;

const API: &str = "NtCreateFile";

/// Opens `name` (one component, at most 255 units) relative to `parent` with the given access,
/// share and create options. `NTSTATUS < 0` => `NtError { api: "NtCreateFile", status }`.
///
/// `name` must be a single plain component: empty, `.`, `..`, or anything containing `\`,
/// `/`, `:` or NUL is refused with `STATUS_OBJECT_NAME_INVALID` before any OS call, so the
/// kernel never parses more than one component below `parent` (no intermediate component that
/// could be a reparse point, no alternate data stream). Callers also check this
/// (`fs::check_open_name`); this is defence in depth at the lowest layer.
pub(crate) fn nt_open_relative(
    parent: Option<&OwnedHandle>,
    name: &[u16],
    access: u32,
    share: u32,
    options: u32,
) -> Result<OwnedHandle, NtError> {
    let invalid = NtError {
        api: API,
        status: status::STATUS_OBJECT_NAME_INVALID,
    };
    if check_open_name(name).is_err() || name.len() > MAX_COMPONENT_UNITS {
        return Err(invalid);
    }
    // <= 255 units => <= 510 bytes: fits u16.
    let byte_len = u16::try_from(name.len() * 2).map_err(|_| invalid)?;

    let object_name = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        // The kernel only reads `ObjectName` for an open; the `*mut` is a C typing artefact.
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        // Struct size is a few dozen bytes: the cast cannot truncate.
        Length: core::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.map_or(HANDLE::default(), OwnedHandle::raw),
        ObjectName: &object_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let mut iosb = IO_STATUS_BLOCK::default();
    let mut handle = HANDLE::default();

    // SAFETY: every pointer argument refers to a live local for the duration of the call:
    // `handle` and `iosb` are writable out-parameters of the documented types; `attributes`
    // is a fully initialised OBJECT_ATTRIBUTES whose `Length` is its own size, whose
    // `ObjectName` points at `object_name`, which in turn points at `name` (`Length` and
    // `MaximumLength` are exactly `name.len() * 2` bytes, within the borrowed slice, which the
    // kernel only reads), and whose `RootDirectory` is either null or a handle kept open by
    // the `parent` borrow. AllocationSize and EaBuffer are null with EaLength 0, as documented
    // for opening an existing file (FILE_OPEN).
    let st = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_ACCESS_RIGHTS(access),
            &attributes,
            &mut iosb,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            FILE_SHARE_MODE(share),
            FILE_OPEN,
            NTCREATEFILE_CREATE_OPTIONS(options),
            None,
            0,
        )
    };
    if st.0 < 0 {
        // On failure no handle is returned; nothing to close.
        return Err(NtError {
            api: API,
            status: st.0,
        });
    }
    // SAFETY: NtCreateFile succeeded (NT_SUCCESS), so `handle` is a new open handle that
    // nothing else owns; ownership moves into the OwnedHandle (or it is invalid and `None`).
    match unsafe { OwnedHandle::from_raw(handle) } {
        Some(h) => Ok(h),
        // Success without a usable handle is not expected; fail closed.
        None => Err(NtError {
            api: API,
            status: st.0,
        }),
    }
}
