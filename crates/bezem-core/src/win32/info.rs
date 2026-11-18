// SPDX-License-Identifier: MIT

//! `GetFileInformationByHandleEx` wrappers: `FileAttributeTagInfo`, `FileIdInfo`,
//! `FileStandardInfo`, `FileBasicInfo`, and `query_item` combining them.
//!
//! Ownership: work package **A**.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};

use windows::Win32::Storage::FileSystem::{
    FileAttributeTagInfo, FileBasicInfo, FileIdInfo, FileStandardInfo,
    GetFileInformationByHandleEx, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO, FILE_ID_INFO,
    FILE_INFO_BY_HANDLE_CLASS, FILE_STANDARD_INFO,
};

use super::{win32_code, OwnedHandle};
use crate::error::Win32Error;
use crate::fs::ItemQuery;
use crate::types::{FileIdentity, FileTime};

const API: &str = "GetFileInformationByHandleEx";

/// `FILE_STANDARD_INFO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StandardInfo {
    pub(crate) allocation_size: u64,
    pub(crate) end_of_file: u64,
    pub(crate) number_of_links: u32,
    pub(crate) delete_pending: bool,
    pub(crate) directory: bool,
}

/// `FILE_BASIC_INFO` times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BasicInfo {
    pub(crate) creation: FileTime,
    pub(crate) last_write: FileTime,
    pub(crate) change: FileTime,
}

/// Byte-for-byte mirror of `FILE_STANDARD_INFO` with the two `BOOLEAN` fields as `u8`.
///
/// The `windows` crate declares them as Rust `bool`, for which any byte other than 0/1 is
/// undefined behavior; the kernel's `BOOLEAN` is a full byte, so it is read as `u8` and
/// compared against 0. The layout equality is asserted at compile time below.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct StandardInfoRaw {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: u32,
    delete_pending: u8,
    directory: u8,
}

const _: () = {
    assert!(size_of::<StandardInfoRaw>() == size_of::<FILE_STANDARD_INFO>());
    assert!(
        offset_of!(StandardInfoRaw, allocation_size)
            == offset_of!(FILE_STANDARD_INFO, AllocationSize)
    );
    assert!(offset_of!(StandardInfoRaw, end_of_file) == offset_of!(FILE_STANDARD_INFO, EndOfFile));
    assert!(
        offset_of!(StandardInfoRaw, number_of_links)
            == offset_of!(FILE_STANDARD_INFO, NumberOfLinks)
    );
    assert!(
        offset_of!(StandardInfoRaw, delete_pending)
            == offset_of!(FILE_STANDARD_INFO, DeletePending)
    );
    assert!(offset_of!(StandardInfoRaw, directory) == offset_of!(FILE_STANDARD_INFO, Directory));
};

/// Plain-old-data info structs: every bit pattern is a valid value (integers and byte arrays
/// only, no `bool`, no references, no enums), and each is the documented output struct of
/// `CLASS` for `GetFileInformationByHandleEx`.
///
/// # Safety
///
/// Implementors must uphold both properties above; `get_info` relies on them.
unsafe trait InfoClass: Copy + Default {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS;
}

// SAFETY: FILE_ATTRIBUTE_TAG_INFO is { u32, u32 }; documented output of FileAttributeTagInfo.
unsafe impl InfoClass for FILE_ATTRIBUTE_TAG_INFO {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileAttributeTagInfo;
}
// SAFETY: FILE_ID_INFO is { u64, FILE_ID_128 { [u8; 16] } }; documented output of FileIdInfo.
unsafe impl InfoClass for FILE_ID_INFO {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileIdInfo;
}
// SAFETY: StandardInfoRaw is { i64, i64, u32, u8, u8 } with FILE_STANDARD_INFO's exact layout
// (asserted above); documented output of FileStandardInfo.
unsafe impl InfoClass for StandardInfoRaw {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileStandardInfo;
}
// SAFETY: FILE_BASIC_INFO is { i64 x4, u32 }; documented output of FileBasicInfo.
unsafe impl InfoClass for FILE_BASIC_INFO {
    const CLASS: FILE_INFO_BY_HANDLE_CLASS = FileBasicInfo;
}

fn get_info<T: InfoClass>(h: &OwnedHandle) -> Result<T, Win32Error> {
    let mut out = T::default();
    // Info structs are a few dozen bytes: the cast cannot truncate.
    let len = size_of::<T>() as u32;
    // SAFETY: `h` is an open handle (OwnedHandle invariant, kept alive by the borrow).
    // `out` is a live, properly aligned, writable `T` of exactly `len` bytes, and by the
    // `InfoClass` contract `T` is the documented output struct for `T::CLASS` and is valid for
    // any bytes the OS writes into it.
    unsafe {
        GetFileInformationByHandleEx(
            h.raw(),
            T::CLASS,
            (&mut out as *mut T).cast::<c_void>(),
            len,
        )
    }
    .map_err(|e| Win32Error {
        api: API,
        code: win32_code(&e),
    })?;
    Ok(out)
}

fn size(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

/// `(FileAttributes, ReparseTag)`.
pub(crate) fn attribute_tag_info(h: &OwnedHandle) -> Result<(u32, u32), Win32Error> {
    let i: FILE_ATTRIBUTE_TAG_INFO = get_info(h)?;
    Ok((i.FileAttributes, i.ReparseTag))
}

/// `FILE_ID_INFO { VolumeSerialNumber, FileId }`.
pub(crate) fn file_id_info(h: &OwnedHandle) -> Result<FileIdentity, Win32Error> {
    let i: FILE_ID_INFO = get_info(h)?;
    Ok(FileIdentity {
        volume_serial: i.VolumeSerialNumber,
        file_id: i.FileId.Identifier,
    })
}

/// `FILE_STANDARD_INFO`.
pub(crate) fn standard_info(h: &OwnedHandle) -> Result<StandardInfo, Win32Error> {
    let i: StandardInfoRaw = get_info(h)?;
    Ok(StandardInfo {
        allocation_size: size(i.allocation_size),
        end_of_file: size(i.end_of_file),
        number_of_links: i.number_of_links,
        delete_pending: i.delete_pending != 0,
        directory: i.directory != 0,
    })
}

/// `FILE_BASIC_INFO`.
pub(crate) fn basic_info(h: &OwnedHandle) -> Result<BasicInfo, Win32Error> {
    let i: FILE_BASIC_INFO = get_info(h)?;
    Ok(BasicInfo {
        creation: FileTime::from_i64(i.CreationTime),
        last_write: FileTime::from_i64(i.LastWriteTime),
        change: FileTime::from_i64(i.ChangeTime),
    })
}

/// The four queries above combined (four calls on the same handle). Facts only.
pub(crate) fn query_item(h: &OwnedHandle) -> Result<ItemQuery, Win32Error> {
    let (attributes, reparse_tag) = attribute_tag_info(h)?;
    let identity = file_id_info(h)?;
    let std = standard_info(h)?;
    let basic = basic_info(h)?;
    Ok(ItemQuery {
        attributes,
        reparse_tag,
        identity,
        link_count: std.number_of_links,
        delete_pending: std.delete_pending,
        is_directory: std.directory,
        size: std.end_of_file,
        allocated: std.allocation_size,
        creation: basic.creation,
        last_write: basic.last_write,
        change: basic.change,
    })
}
