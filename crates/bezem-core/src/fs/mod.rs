// SPDX-License-Identifier: MIT

//! The file-system seam: [`FsBackend`] and its data types (M1 design sections 2.7 and 3).
//!
//! Everything above this seam (path, resolve, safety, walk, delete, log, pipeline) is
//! platform-agnostic and generic over `F: FsBackend`, so it compiles and is tested on Linux
//! against [`mem::MemFs`]. The real backend is `Win32Fs` (private `win32` module, Windows only).
//!
//! Ownership:
//! - this file (trait + types + [`check_open_name`]): Phase 0 contract; changes need the lead.
//! - [`attr`]: Phase 0. [`status`] and [`dirbuf`]: work package **A**.
//! - [`mem`] (`MemFs`, test-only): work package **D**.

use std::ops::ControlFlow;

use crate::error::ErrorCode;
use crate::path::WidePath;
use crate::types::{FileIdentity, FileTime, SkipReason};

pub mod attr;
pub mod dirbuf;
#[cfg(any(test, feature = "test-support"))]
pub mod mem;
pub mod status;

/// An item-level file-system failure: the skip reason plus the OS code for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsError {
    pub reason: SkipReason,
    pub code: ErrorCode,
}

impl FsError {
    /// Shorthand constructor.
    pub fn new(reason: SkipReason, code: ErrorCode) -> FsError {
        FsError { reason, code }
    }
}

/// Whether an item is opened as a file or a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Dir,
}

/// How good the file ids from a directory listing are (ARCHITECTURE A:983-984).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdQuality {
    /// 128-bit ids (`FileIdExtdDirectoryInfo`).
    Full128,
    /// 64-bit ids zero-extended (`FileIdBothDirectoryInfo`).
    Id64,
    /// No ids (`FileFullDirectoryInfo`): identity checks are skipped, size and write time are
    /// still enforced.
    None,
}

/// The kind of volume a root lives on. Only `LocalFixed` is accepted for roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    LocalFixed,
    Remote,
    Removable,
    Other,
}

/// How [`FsBackend::open_root`] treats a reparse point on the final component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootOpenMode {
    /// Roots: open the final component itself (`FILE_FLAG_OPEN_REPARSE_POINT`); a reparse root
    /// is reported via `is_reparse` and then rejected by the policy.
    VerifyNoFollow,
    /// Allowlisted bases and protected paths: follow, so relocated folders canonicalize to
    /// their real location.
    CanonicalizeFollow,
}

/// What `open_root` learned about a directory, plus the open handle.
#[derive(Debug)]
pub struct RootProbe<D> {
    /// The open directory handle (no `FILE_SHARE_DELETE`: pins the directory).
    pub dir: D,
    /// Raw `GetFinalPathNameByHandleW` output; the policy runs `path::parse_final` on it.
    pub final_path: Vec<u16>,
    pub identity: FileIdentity,
    pub is_directory: bool,
    pub is_reparse: bool,
    pub volume: VolumeKind,
}

/// Why `open_root` failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRootError {
    /// `ERROR_FILE_NOT_FOUND` / `ERROR_PATH_NOT_FOUND`.
    NotFound,
    Fs(FsError),
}

/// One entry of a directory listing. Backends never emit `.` or `..`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEntry<'a> {
    /// The long name, exactly as listed (never re-parsed).
    pub name: &'a [u16],
    pub attributes: u32,
    /// `ReparsePointTag` from the listing itself; 0 when not a reparse point.
    pub reparse_tag: u32,
    pub size: u64,
    pub allocated: u64,
    pub creation: FileTime,
    pub last_write: FileTime,
    pub change: FileTime,
    /// `None` when the listing class has no ids ([`IdQuality::None`]).
    pub file_id: Option<[u8; 16]>,
}

/// Facts read from an open item handle (attribute-tag, id, standard and basic info).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemQuery {
    pub attributes: u32,
    pub reparse_tag: u32,
    pub identity: FileIdentity,
    pub link_count: u32,
    pub delete_pending: bool,
    pub is_directory: bool,
    /// `EndOfFile`.
    pub size: u64,
    pub allocated: u64,
    pub creation: FileTime,
    pub last_write: FileTime,
    pub change: FileTime,
}

/// Options for [`FsBackend::set_disposition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispositionOpts {
    /// Set `FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE` (only when the target sets
    /// `delete_readonly`). Attributes are never cleared.
    pub ignore_readonly: bool,
}

/// Which disposition class deleted the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionKind {
    /// `FileDispositionInfoEx` with POSIX semantics.
    Posix,
    /// Fallback `FileDispositionInfo { DeleteFile = TRUE }`.
    Legacy,
}

/// Why `set_disposition` failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionError {
    /// A directory that is not empty (left in place silently by the empty-dir pass).
    NotEmpty,
    Fs(FsError),
}

/// The file-system seam. The only way the engine touches the file system.
///
/// # Hard rules for every backend
///
/// 1. Listings never emit `.` or `..`.
/// 2. Every open is no-follow on the final component (`FILE_OPEN_REPARSE_POINT`), except
///    `open_root` with [`RootOpenMode::CanonicalizeFollow`]. A reparse point is reported, never
///    traversed: `open_subdir` on a reparse directory returns `Err` with reason
///    [`SkipReason::ReparsePoint`].
/// 3. Directory handles are opened without `FILE_SHARE_DELETE`, so an open directory cannot be
///    renamed or replaced underneath us.
/// 4. Names passed to `open_subdir`/`open_item` that are empty, `.`, `..`, or contain `\`,
///    `/`, `:` or NUL are refused with `FsError { reason: SkipReason::FsError, .. }` before any
///    OS call ([`check_open_name`]; defence in depth).
/// 5. All opens below the root are relative to a parent handle (`NtCreateFile` with
///    `RootDirectory`), never by absolute path.
/// 6. Scanning opens nothing for write: listing uses `FILE_LIST_DIRECTORY |
///    FILE_READ_ATTRIBUTES` only. Only `open_item` requests `DELETE`.
/// 7. Backends report facts; they make no policy decisions (those live in `safety`).
/// 8. Backends never panic and never expose raw OS handles.
pub trait FsBackend: Send + Sync + 'static {
    /// An open directory handle (no `FILE_SHARE_DELETE`). Dropping it closes it.
    type Dir: Send + Sync;
    /// An item opened for delete (`DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`). Dropping it
    /// closes it.
    type Item: Send;

    /// Opens an absolute directory and probes it (attributes, id, final path, volume).
    /// Missing => [`OpenRootError::NotFound`].
    fn open_root(
        &self,
        path: &WidePath,
        mode: RootOpenMode,
    ) -> Result<RootProbe<Self::Dir>, OpenRootError>;

    /// Opens a child directory relative to `parent`, no-follow. A reparse directory =>
    /// `Err(FsError { reason: ReparsePoint, .. })`.
    fn open_subdir(&self, parent: &Self::Dir, name: &[u16]) -> Result<Self::Dir, FsError>;

    /// Lists `dir`, calling `sink` per entry until it returns `Break`. Returns the id quality
    /// of the listing class that worked.
    fn read_dir(
        &self,
        dir: &Self::Dir,
        sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<IdQuality, FsError>;

    /// Opens a child for delete, relative to `parent`, no-follow. Files are opened with
    /// `FILE_SHARE_READ` only (a held handle => `Locked`); directories with read+write share.
    fn open_item(
        &self,
        parent: &Self::Dir,
        name: &[u16],
        kind: ItemKind,
    ) -> Result<Self::Item, FsError>;

    /// Reads the facts the delete-time re-checks need, from the item handle itself.
    fn query_item(&self, item: &Self::Item) -> Result<ItemQuery, FsError>;

    /// Raw `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)` of the item.
    fn final_path(&self, item: &Self::Item) -> Result<Vec<u16>, FsError>;

    /// Marks the item for deletion through its handle (POSIX semantics, legacy fallback).
    fn set_disposition(
        &self,
        item: &Self::Item,
        o: DispositionOpts,
    ) -> Result<DispositionKind, DispositionError>;
}

/// Pure pre-check for names passed to `open_subdir`/`open_item` (hard rule 4). Rejects empty,
/// `.`, `..`, and names containing `\`, `/`, `:` or NUL with
/// `FsError { reason: FsError, code: Sim("bad_name") }`.
pub fn check_open_name(name: &[u16]) -> Result<(), FsError> {
    const DOT: u16 = b'.' as u16;
    let bad = name.is_empty()
        || name == [DOT]
        || name == [DOT, DOT]
        || name.iter().any(|&u| {
            u == 0 || u == u16::from(b'\\') || u == u16::from(b'/') || u == u16::from(b':')
        });
    if bad {
        Err(FsError::new(
            SkipReason::FsError,
            ErrorCode::Sim("bad_name"),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn open_name_check() {
        for bad in ["", ".", "..", "a\\b", "a/b", "a:b", "x::$DATA", "a\0b"] {
            assert!(check_open_name(&w(bad)).is_err(), "{bad:?} must be refused");
        }
        for good in ["a", "...", ".x", "CON", "file.txt", "naïve", "a b."] {
            assert!(check_open_name(&w(good)).is_ok(), "{good:?} must pass");
        }
    }
}
