// SPDX-License-Identifier: MIT

//! `GetFileInformationByHandleEx` wrappers: `FileAttributeTagInfo`, `FileIdInfo`,
//! `FileStandardInfo`, `FileBasicInfo`, and `query_item` combining them.
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::error::Win32Error;
use crate::fs::ItemQuery;
use crate::types::{FileIdentity, FileTime};

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
    pub(crate) last_access: FileTime,
    pub(crate) last_write: FileTime,
    pub(crate) change: FileTime,
}

/// `(FileAttributes, ReparseTag)`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn attribute_tag_info(h: &OwnedHandle) -> Result<(u32, u32), Win32Error> {
    unimplemented!()
}

/// `FILE_ID_INFO { VolumeSerialNumber, FileId }`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn file_id_info(h: &OwnedHandle) -> Result<FileIdentity, Win32Error> {
    unimplemented!()
}

/// `FILE_STANDARD_INFO`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn standard_info(h: &OwnedHandle) -> Result<StandardInfo, Win32Error> {
    unimplemented!()
}

/// `FILE_BASIC_INFO`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn basic_info(h: &OwnedHandle) -> Result<BasicInfo, Win32Error> {
    unimplemented!()
}

/// The four queries above combined.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn query_item(h: &OwnedHandle) -> Result<ItemQuery, Win32Error> {
    unimplemented!()
}
