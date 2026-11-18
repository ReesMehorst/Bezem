// SPDX-License-Identifier: MIT

//! `impl FsBackend for Win32Fs` (flags and status mapping per M1 design section 3.1).
//!
//! Ownership: work package **A**.

use std::ops::ControlFlow;
use std::sync::Mutex;

use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_FOR_BACKUP_INTENT,
    FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TRAVERSE, SYNCHRONIZE,
};

use super::dirent::{self, ReadDirError};
use super::info::{self, attribute_tag_info};
use super::nt::nt_open_relative;
use super::{dispose, finalpath, open, OwnedHandle};
use crate::error::{ErrorCode, NtError, Win32Error};
use crate::fs::{
    attr, check_open_name, status, DispositionError, DispositionKind, DispositionOpts, FsBackend,
    FsError, IdQuality, ItemKind, ItemQuery, OpenRootError, RawEntry, RootOpenMode, RootProbe,
};
use crate::path::WidePath;
use crate::types::SkipReason;

/// Directory listing/traversal: `FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES |
/// SYNCHRONIZE`. Nothing that can write.
const DIR_ACCESS: u32 =
    FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
/// Delete opens: `DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`.
const ITEM_ACCESS: u32 = DELETE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
/// Directory opens: read + write sharing, never `FILE_SHARE_DELETE` (pins the directory).
const SHARE_DIR: u32 = FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0;
/// File delete opens: `FILE_SHARE_READ` only (anyone writing or deleting => sharing violation).
const SHARE_FILE: u32 = FILE_SHARE_READ.0;
/// Directory create options (no-follow on the final component, synchronous I/O).
const DIR_OPTIONS: u32 =
    FILE_DIRECTORY_FILE.0 | FILE_OPEN_REPARSE_POINT.0 | FILE_SYNCHRONOUS_IO_NONALERT.0;
/// File create options (no-follow on the final component, synchronous I/O).
const FILE_OPTIONS: u32 =
    FILE_NON_DIRECTORY_FILE.0 | FILE_OPEN_REPARSE_POINT.0 | FILE_SYNCHRONOUS_IO_NONALERT.0;

/// The real Windows backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32Fs {
    /// Adds `FILE_OPEN_FOR_BACKUP_INTENT` to directory opens. Always false in M1.
    backup_intent: bool,
}

/// An open directory handle (no `FILE_SHARE_DELETE`). Opaque: the raw handle never leaves
/// `win32`.
#[derive(Debug)]
pub struct Win32Dir {
    handle: OwnedHandle,
    /// The enumeration cursor lives in the kernel file object, so two concurrent listings of
    /// one handle would interleave. `read_dir` holds this lock for a whole listing.
    listing: Mutex<()>,
}

impl Win32Dir {
    pub(super) fn new(handle: OwnedHandle) -> Win32Dir {
        Win32Dir {
            handle,
            listing: Mutex::new(()),
        }
    }
}

/// An item opened for delete. Opaque: the raw handle never leaves `win32`.
#[derive(Debug)]
pub struct Win32Item(OwnedHandle);

fn from_nt(e: NtError) -> FsError {
    let (reason, code) = status::from_nt(e.status);
    FsError::new(reason, code)
}

fn from_win32(e: Win32Error) -> FsError {
    let (reason, code) = status::from_win32(e.code);
    FsError::new(reason, code)
}

impl FsBackend for Win32Fs {
    type Dir = Win32Dir;
    type Item = Win32Item;

    fn open_root(
        &self,
        path: &WidePath,
        mode: RootOpenMode,
    ) -> Result<RootProbe<Self::Dir>, OpenRootError> {
        let p = open::open_root(path, mode)?;
        Ok(RootProbe {
            dir: Win32Dir::new(p.dir),
            final_path: p.final_path,
            identity: p.identity,
            is_directory: p.is_directory,
            is_reparse: p.is_reparse,
            volume: p.volume,
        })
    }

    fn open_subdir(&self, parent: &Self::Dir, name: &[u16]) -> Result<Self::Dir, FsError> {
        check_open_name(name)?;
        let mut options = DIR_OPTIONS;
        if self.backup_intent {
            options |= FILE_OPEN_FOR_BACKUP_INTENT.0;
        }
        let h = nt_open_relative(Some(&parent.handle), name, DIR_ACCESS, SHARE_DIR, options)
            .map_err(from_nt)?;
        // Hard rule 2: a reparse directory is reported, never traversed. The handle is closed
        // (dropped) before returning.
        let (attributes, tag) = attribute_tag_info(&h).map_err(from_win32)?;
        if attr::is_reparse(attributes, tag) {
            return Err(FsError::new(SkipReason::ReparsePoint, ErrorCode::None));
        }
        Ok(Win32Dir::new(h))
    }

    fn read_dir(
        &self,
        dir: &Self::Dir,
        sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<IdQuality, FsError> {
        // A poisoned lock only means an earlier listing panicked in its sink; the cursor is
        // reset by the restart class, so continuing is sound.
        let _guard = dir
            .listing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        dirent::read_dir(&dir.handle, sink).map_err(|e| match e {
            ReadDirError::Api(w) => from_win32(w),
            ReadDirError::Malformed(_) => {
                FsError::new(SkipReason::FsError, ErrorCode::Sim("bad_dir_buffer"))
            }
        })
    }

    fn open_item(
        &self,
        parent: &Self::Dir,
        name: &[u16],
        kind: ItemKind,
    ) -> Result<Self::Item, FsError> {
        check_open_name(name)?;
        let (share, options) = match kind {
            ItemKind::File => (SHARE_FILE, FILE_OPTIONS),
            ItemKind::Dir => (SHARE_DIR, DIR_OPTIONS),
        };
        nt_open_relative(Some(&parent.handle), name, ITEM_ACCESS, share, options)
            .map(Win32Item)
            .map_err(from_nt)
    }

    fn query_item(&self, item: &Self::Item) -> Result<ItemQuery, FsError> {
        info::query_item(&item.0).map_err(from_win32)
    }

    fn final_path(&self, item: &Self::Item) -> Result<Vec<u16>, FsError> {
        finalpath::final_path(&item.0).map_err(from_win32)
    }

    fn set_disposition(
        &self,
        item: &Self::Item,
        o: DispositionOpts,
    ) -> Result<DispositionKind, DispositionError> {
        dispose::set_disposition(&item.0, o)
    }
}
