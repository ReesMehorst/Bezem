// SPDX-License-Identifier: MIT

//! Handle-based file-system wrappers (ARCHITECTURE.md sections 8.3 and 8.4).
//!
//! **Signatures frozen at M1** (see `README.md` in this directory). Bodies marked "Stub" return
//! `not_implemented` on Windows and `unsupported_platform` elsewhere; W3 to W6 fill them in
//! without changing a signature.
//!
//! # Handle roles
//!
//! Each open returns a type that records what the handle may be used for, so the order of the
//! safety algorithm is checked by the compiler as far as Rust visibility allows:
//!
//! - [`UnverifiedRoot`] from [`open_root`]: can be inspected, but is not a parent for opens and
//!   cannot be enumerated until `safety::verify_root` promotes it with
//!   [`UnverifiedRoot::into_verified_dir`].
//! - [`DirHandle`]: a parent for relative opens and the only input of [`query_dir_batch`].
//! - [`DeleteHandle`]: opened with `DELETE`. It can be inspected, but deleted only once wrapped
//!   in [`Checked`] by the `delete` module after steps 3 to 8 of section 8.4.
//! - [`ReadHandle`]: read-only, for [`read_to_end`].
//!
//! Rust cannot restrict a constructor to one sibling module without `win32` depending on that
//! module, and `win32` must not depend on `safety` or `delete`. The two promotion points
//! ([`UnverifiedRoot::into_verified_dir`] and [`Checked::attest`]) are therefore `pub(crate)`
//! and fenced by a CI grep, listed in `README.md`.
//!
//! # Cloud placeholders
//!
//! Every open adds `FILE_OPEN_NO_RECALL`, so no open hydrates a placeholder. Each process
//! (the GUI/CLI process and, separately, the elevated worker) calls [`expose_placeholders`] once
//! at start-up so placeholders show their reparse tag and attributes. The returned
//! [`PlaceholdersExposed`] token is required by [`open_root`], so the type system enforces the
//! call. Before each target, [`recheck_placeholders_exposed`] confirms the mode has not been
//! changed since. Any failure means no scan or clean (fail closed).
//!
//! [`nt_open_dir`] and [`nt_open_read_file`] refuse reparse points and placeholders themselves,
//! so a [`DirHandle`] or [`ReadHandle`] never refers to one.

use std::time::{Duration, SystemTime};

use super::error::{code, status, Error, NtError, Win32Error};
use super::handle::OwnedHandle;
use crate::path::{FileIdentity, PathComponent, WidePath};
use crate::report::SkipReason;

/// `FILE_ATTRIBUTE_*` bits the safety layer inspects (values from WinNT.h).
pub(crate) mod attr {
    /// Read-only file.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const READONLY: u32 = 0x1;
    /// Used by the operating system.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const SYSTEM: u32 = 0x4;
    /// Directory.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const DIRECTORY: u32 = 0x10;
    /// Has a reparse point (any tag): never traversed or deleted.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const REPARSE_POINT: u32 = 0x400;
    /// Data not available locally.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const OFFLINE: u32 = 0x1000;
    /// Cloud placeholder. Reported **only by directory enumeration**, so it is checked at scan
    /// time from [`DirEntry::attributes`](super::DirEntry), never after an open.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const RECALL_ON_OPEN: u32 = 0x4_0000;
    /// Hierarchical-storage intent: keep the file local.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const PINNED: u32 = 0x8_0000;
    /// Hierarchical-storage intent: the file may be dehydrated.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const UNPINNED: u32 = 0x10_0000;
    /// Not fully present locally; reading the data fetches it from a remote store.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
}

/// Reparse tags that mark cloud or projected files (MS-FSCC 2.1.2.1).
pub(crate) mod reparse_tag {
    /// `IO_REPARSE_TAG_CLOUD`; `IO_REPARSE_TAG_CLOUD_1` to `_F` set bits 12 to 15.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const CLOUD: u32 = 0x9000_001A;
    /// Clears bits 12 to 15, so every `IO_REPARSE_TAG_CLOUD_n` compares equal to [`CLOUD`].
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const CLOUD_MASK: u32 = 0xFFFF_0FFF;
    /// `IO_REPARSE_TAG_PROJFS` (Projected File System, e.g. VFS for Git).
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) const PROJFS: u32 = 0x9000_001C;
}

/// True if an item is (or may be) a cloud or projected placeholder and must be skipped as
/// `CloudPlaceholder`: any of `OFFLINE`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`, `PINNED`,
/// `UNPINNED`, or a reparse point with a cloud or ProjFS tag. Check this before the generic
/// reparse-point rule so the more specific reason is reported.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn is_cloud_placeholder(attributes: u32, reparse_tag: u32) -> bool {
    const ATTRS: u32 = attr::OFFLINE
        | attr::RECALL_ON_OPEN
        | attr::RECALL_ON_DATA_ACCESS
        | attr::PINNED
        | attr::UNPINNED;
    let tagged = attributes & attr::REPARSE_POINT != 0
        && (reparse_tag & reparse_tag::CLOUD_MASK == reparse_tag::CLOUD
            || reparse_tag == reparse_tag::PROJFS);
    attributes & ATTRS != 0 || tagged
}

/// A `FILETIME`: 100-nanosecond intervals since 1601-01-01 UTC, as Windows reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct FileTime(pub(crate) i64);

impl FileTime {
    /// 100 ns intervals between 1601-01-01 and 1970-01-01.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    const UNIX_EPOCH_TICKS: i128 = 116_444_736_000_000_000;

    /// The same instant as a [`SystemTime`], or `None` if it is out of range. Exact: no
    /// rounding beyond the 100 ns resolution.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) fn to_system_time(self) -> Option<SystemTime> {
        let ticks = i128::from(self.0) - Self::UNIX_EPOCH_TICKS;
        let abs = ticks.unsigned_abs();
        let secs = u64::try_from(abs / 10_000_000).ok()?;
        let nanos = u32::try_from(abs % 10_000_000 * 100).ok()?;
        let delta = Duration::new(secs, nanos);
        if ticks >= 0 {
            SystemTime::UNIX_EPOCH.checked_add(delta)
        } else {
            SystemTime::UNIX_EPOCH.checked_sub(delta)
        }
    }
}

/// `FILE_ATTRIBUTE_TAG_INFO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct AttributeTagInfo {
    /// `FILE_ATTRIBUTE_*` bits.
    pub(crate) attributes: u32,
    /// Reparse tag; meaningful only if `attributes` has [`attr::REPARSE_POINT`].
    pub(crate) reparse_tag: u32,
}

/// `FILE_STANDARD_INFO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct StandardInfo {
    /// Bytes allocated on disk.
    pub(crate) allocation_size: u64,
    /// Logical size in bytes.
    pub(crate) end_of_file: u64,
    /// Hard-link count; `> 1` means skip (`HardLinked`).
    pub(crate) number_of_links: u32,
    /// A delete is already pending.
    pub(crate) delete_pending: bool,
    /// The handle refers to a directory.
    pub(crate) directory: bool,
}

/// `FILE_BASIC_INFO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct BasicInfo {
    /// Creation time.
    pub(crate) creation_time: FileTime,
    /// Last access time.
    pub(crate) last_access_time: FileTime,
    /// Last write time.
    pub(crate) last_write_time: FileTime,
    /// Last metadata change time.
    pub(crate) change_time: FileTime,
    /// `FILE_ATTRIBUTE_*` bits.
    pub(crate) attributes: u32,
}

mod sealed {
    /// Gives `fs` access to the owned handle inside a role type. Not nameable outside `fs`,
    /// so no other module can reach the raw handle or add a role.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub trait Sealed {
        fn owned(&self) -> &super::OwnedHandle;
    }
}

/// Any handle role that may be inspected with the info queries. Sealed.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) trait FileHandle: sealed::Sealed {}

/// A root opened by path and not yet verified. See the module docs.
#[derive(Debug)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct UnverifiedRoot(OwnedHandle);

/// A directory opened for listing and traversal (section 8.4 step 1), or a promoted root.
#[derive(Debug)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct DirHandle(OwnedHandle);

/// A file or directory opened with `DELETE` access (section 8.4 step 2, "Empty directories").
#[derive(Debug)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct DeleteHandle(OwnedHandle);

/// A file opened for reading only (sections 7.4 and 7.5).
#[derive(Debug)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct ReadHandle(OwnedHandle);

/// A handle whose identity, attributes, link count, age and final path have been checked on
/// that same handle (section 8.4 steps 3 to 8; invariant 3 of section 8.2).
#[derive(Debug)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct Checked<H>(H);

impl UnverifiedRoot {
    /// Promotes the root to a [`DirHandle`]. **Only `safety::verify_root` may call this**, after
    /// steps 4 to 9 of section 8.3 passed on this same handle. Enforced by the CI grep in
    /// `README.md`, because Rust cannot express it without `win32` depending on `safety`.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) fn into_verified_dir(self) -> DirHandle {
        DirHandle(self.0)
    }
}

impl Checked<DeleteHandle> {
    /// Attests that steps 3 to 8 of section 8.4 passed on `handle`. **Only the `delete` module
    /// may call this** (CI grep in `README.md`); the full token design belongs to E3. The
    /// intended pattern: open, query, compare, attest, `set_disposition_ex`, drop, with no
    /// await point or reopen in between.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) fn attest(handle: DeleteHandle) -> Self {
        Self(handle)
    }
}

macro_rules! file_handle {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {
            fn owned(&self) -> &OwnedHandle {
                &self.0
            }
        }
        impl FileHandle for $t {}
    )*};
}
file_handle!(UnverifiedRoot, DirHandle, DeleteHandle, ReadHandle);

/// Whether a directory open asks for `FILE_OPEN_FOR_BACKUP_INTENT`. Only [`nt_open_dir`] takes
/// it: the elevated worker needs it to traverse machine-scope directories. Delete and read opens
/// never use backup intent.
///
/// **Privileges:** the elevated worker may enable `SeBackupPrivilege` only, for traversal and
/// listing. It never enables `SeRestorePrivilege`, which would let a backup-intent open bypass
/// the `DELETE` access check: a delete succeeds under the normal ACL or not at all (section 8.2
/// invariant 5; nothing is forced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) enum Intent {
    /// Normal access checks.
    Normal,
    /// `FILE_OPEN_FOR_BACKUP_INTENT`; with `SeBackupPrivilege` it grants read and traverse only.
    Backup,
}

/// Proof that [`expose_placeholders`] succeeded in this process. Zero-sized; its only
/// constructor is [`expose_placeholders`], and [`open_root`] requires it, so no scan or clean
/// can start with placeholders disguised. The elevated worker is a separate process and must
/// make its own call.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct PlaceholdersExposed {
    _private: (),
}

/// Opens a root by absolute path (section 8.3 step 3, W3): `CreateFileW(path, FILE_LIST_DIRECTORY
/// | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_SHARE_READ | FILE_SHARE_WRITE,
/// OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT |
/// FILE_FLAG_OPEN_NO_RECALL)`. `WidePath` guarantees the `\\?\X:\` form; `exposed` proves that
/// placeholders are visible. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn open_root(
    path: &WidePath,
    exposed: &PlaceholdersExposed,
) -> Result<UnverifiedRoot, Win32Error> {
    let _ = (path, exposed);
    Err(pending("CreateFileW"))
}

// Contract shared by the four `nt_open_*` functions (W3):
// NtCreateFile with OBJECT_ATTRIBUTES { RootDirectory = parent, ObjectName = name,
// Attributes = OBJ_CASE_INSENSITIVE }, CreateDisposition = FILE_OPEN, and CreateOptions always
// including FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL | FILE_SYNCHRONOUS_IO_NONALERT.
// Only `nt_open_dir` adds FILE_OPEN_FOR_BACKUP_INTENT, and only for `Intent::Backup`.
// ObjectName.Length is the name's size in bytes, built with `u16::try_from(units * 2)`:
// PathComponent caps names at 255 units (510 bytes), and a failed conversion returns
// STATUS_OBJECT_NAME_INVALID, never a truncated name. No share mode ever includes
// FILE_SHARE_DELETE. Map failures with `open_skip_reason`.
//
// Reparse check (`nt_open_dir` and `nt_open_read_file`): before returning, the wrapper queries
// NtQueryInformationFile(FileAttributeTagInformation) on the new handle and applies
// `reparse_rejection`. On a rejection, or if the query fails, it closes the handle and returns
// an NtError, so no DirHandle or ReadHandle can exist for a reparse point. Delete opens skip
// this check: the delete module inspects the DeleteHandle itself (section 8.4 step 3).
// TODO(W3, Windows CI): tests that a junction, a directory symlink, a file symlink and a cloud
// placeholder are refused by `nt_open_dir` / `nt_open_read_file`.

/// Opens a subdirectory (section 8.4 step 1): `FILE_LIST_DIRECTORY | FILE_TRAVERSE |
/// FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share `READ | WRITE` (pins the directory),
/// `FILE_DIRECTORY_FILE`. **Never returns a reparse point** (see the reparse check above);
/// [`open_skip_reason`] maps the refusal to `ReparsePoint` or `CloudPlaceholder`. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn nt_open_dir(
    parent: &DirHandle,
    name: &PathComponent,
    intent: Intent,
) -> Result<DirHandle, NtError> {
    let _ = (parent, name, intent);
    Err(pending_nt("NtCreateFile"))
}

/// Opens a file for deletion (section 8.4 step 2): `DELETE | FILE_READ_ATTRIBUTES |
/// SYNCHRONIZE`, share `READ` only, so the open fails with a sharing violation (`Locked`) while
/// any other handle has write access (section 8.1, "Files in use"). `FILE_NON_DIRECTORY_FILE`.
/// No backup intent. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn nt_open_delete_file(
    parent: &DirHandle,
    name: &PathComponent,
) -> Result<DeleteHandle, NtError> {
    let _ = (parent, name);
    Err(pending_nt("NtCreateFile"))
}

/// Opens an empty-directory candidate for deletion (section 8.4, "Empty directories"):
/// `DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share `READ | WRITE`, `FILE_DIRECTORY_FILE`.
/// No backup intent. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn nt_open_delete_dir(
    parent: &DirHandle,
    name: &PathComponent,
) -> Result<DeleteHandle, NtError> {
    let _ = (parent, name);
    Err(pending_nt("NtCreateFile"))
}

/// Opens a small configuration file for reading (`Local State`, `profiles.ini`; user scope, so
/// never in the elevated worker): `FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share
/// `READ | WRITE`, `FILE_NON_DIRECTORY_FILE`, no backup intent. **Never returns a reparse
/// point** (section 7.4; see the reparse check above). Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn nt_open_read_file(
    parent: &DirHandle,
    name: &PathComponent,
) -> Result<ReadHandle, NtError> {
    let _ = (parent, name);
    Err(pending_nt("NtCreateFile"))
}

/// The refusal status for an object that `nt_open_dir` / `nt_open_read_file` must not hand out,
/// or `None` if it may be returned. Cloud placeholders are checked first so the more specific
/// reason wins: [`status::STATUS_BEZEM_CLOUD_PLACEHOLDER`], then
/// [`status::STATUS_REPARSE_POINT_ENCOUNTERED`] for any other reparse point.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn reparse_rejection(info: &AttributeTagInfo) -> Option<i32> {
    if is_cloud_placeholder(info.attributes, info.reparse_tag) {
        Some(status::STATUS_BEZEM_CLOUD_PLACEHOLDER)
    } else if info.attributes & attr::REPARSE_POINT != 0 {
        Some(status::STATUS_REPARSE_POINT_ENCOUNTERED)
    } else {
        None
    }
}

/// Reads the whole file. Fails with `ERROR_FILE_TOO_LARGE` if it is larger than `max_len`
/// bytes, checked before and while reading, so a growing file cannot exceed the cap. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn read_to_end(h: &ReadHandle, max_len: u64) -> Result<Vec<u8>, Win32Error> {
    let _ = (h, max_len);
    Err(pending("ReadFile"))
}

/// `GetFileInformationByHandleEx(FileAttributeTagInfo)` (W3). Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn query_attribute_tag(h: &impl FileHandle) -> Result<AttributeTagInfo, Win32Error> {
    let _ = h.owned();
    Err(pending(
        "GetFileInformationByHandleEx(FileAttributeTagInfo)",
    ))
}

/// `GetFileInformationByHandleEx(FileIdInfo)` (W3). On file systems without `FileIdInfo`
/// (FAT, exFAT) this fails, and so does root verification: FAT/exFAT support is M2. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn query_id(h: &impl FileHandle) -> Result<FileIdentity, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileIdInfo)"))
}

/// `GetFileInformationByHandleEx(FileStandardInfo)` (W3). A negative size from the OS is an
/// error, never clamped. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn query_standard(h: &impl FileHandle) -> Result<StandardInfo, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileStandardInfo)"))
}

/// `GetFileInformationByHandleEx(FileBasicInfo)` (W3). Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn query_basic(h: &impl FileHandle) -> Result<BasicInfo, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileBasicInfo)"))
}

/// `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)` (W4): untrusted output,
/// returned verbatim without the trailing NUL. Callers parse it with
/// `WidePath::parse_extended`, which rejects anything but `\\?\X:\...`. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn final_path(h: &impl FileHandle) -> Result<Vec<u16>, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFinalPathNameByHandleW"))
}

/// The kind of volume holding a handle. Only [`VolumeKind::LocalFixed`] passes root
/// verification; every other value, including a failed query, rejects the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) enum VolumeKind {
    /// No `FILE_REMOTE_DEVICE` and `GetDriveTypeW` is `DRIVE_FIXED`.
    LocalFixed,
    /// `FILE_REMOTE_DEVICE`, or `DRIVE_REMOTE`.
    Remote,
    /// `DRIVE_REMOVABLE` or `DRIVE_CDROM`.
    Removable,
    /// Any other drive type (`DRIVE_RAMDISK`, `DRIVE_UNKNOWN`, ...).
    Other,
    /// A query failed; carries the error for the log.
    Unknown(Error),
}

/// Classifies the volume holding `h` (W4): `NtQueryVolumeInformationFile(FileFsDeviceInformation)`
/// and `GetDriveTypeW` on the volume root. No `Result`: a failure is
/// [`VolumeKind::Unknown`], which can never be mistaken for local. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn volume_kind(h: &impl FileHandle) -> VolumeKind {
    let _ = h.owned();
    VolumeKind::Unknown(pending_nt("NtQueryVolumeInformationFile").into())
}

/// The 64-bit volume serial of the volume holding `h` (W4), taken from `FILE_ID_INFO`, so it
/// equals [`FileIdentity::volume_serial`]. Fails where `FileIdInfo` is unsupported (M2). Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn volume_serial(h: &impl FileHandle) -> Result<u64, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileIdInfo)"))
}

/// Which directory information class a [`DirQuery`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) enum DirInfoClass {
    /// `FileIdExtdDirectoryInfo`: 128-bit IDs (NTFS, ReFS). The only class used at M1.
    IdExtd,
    /// `FileIdBothDirectoryInfo` fallback (M2): 64-bit IDs, zero-extended by [`file_id_from_u64`].
    IdBoth,
    /// `FileFullDirectoryInfo` fallback (M2): no IDs.
    Full,
}

/// One entry from [`query_dir_batch`] (`FILE_ID_EXTD_DIR_INFO`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct DirEntry {
    /// Long file name, verbatim and **untrusted**. Validate with `PathComponent::from_wide`;
    /// a name that fails is skipped as `SkipReason::UnsupportedName`.
    pub(crate) name: Vec<u16>,
    /// `FILE_ATTRIBUTE_*` bits. The only place [`attr::RECALL_ON_OPEN`] is reported.
    pub(crate) attributes: u32,
    /// Reparse tag; meaningful only if `attributes` has [`attr::REPARSE_POINT`].
    pub(crate) reparse_tag: u32,
    /// Logical size in bytes.
    pub(crate) end_of_file: u64,
    /// Bytes allocated on disk.
    pub(crate) allocation_size: u64,
    /// Creation time.
    pub(crate) creation_time: FileTime,
    /// Last access time.
    pub(crate) last_access_time: FileTime,
    /// Last write time.
    pub(crate) last_write_time: FileTime,
    /// Last metadata change time.
    pub(crate) change_time: FileTime,
    /// 128-bit file ID in `FILE_ID_128` byte order, comparable with
    /// [`FileIdentity::file_id`]. `None` only under [`DirInfoClass::Full`] (M2).
    pub(crate) file_id: Option<[u8; 16]>,
}

/// Enumeration state for one directory: the 64 KiB buffer, the info class and the restart
/// flag. One per directory; never share a [`DirHandle`] between two concurrent enumerations.
#[derive(Debug)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) struct DirQuery {
    class: DirInfoClass,
    restart: bool,
}

impl DirQuery {
    /// A fresh enumeration with [`DirInfoClass::IdExtd`].
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) fn new() -> Self {
        Self {
            class: DirInfoClass::IdExtd,
            restart: true,
        }
    }

    /// The class in use.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) fn class(&self) -> DirInfoClass {
        self.class
    }

    /// Restarts from the first entry on the next [`query_dir_batch`] call (uses the
    /// `...RestartInfo` class). Keeps the class.
    #[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
    pub(crate) fn reset(&mut self) {
        self.restart = true;
    }
}

impl Default for DirQuery {
    fn default() -> Self {
        Self::new()
    }
}

/// Appends the next batch of entries of `dir` to `out` via `GetFileInformationByHandleEx`
/// (W5). `.` and `..` are omitted; every other name is returned verbatim. `dir` is borrowed
/// mutably because enumeration advances the handle's position: two enumerations of one handle
/// cannot interleave.
///
/// - `Ok(true)`: the call succeeded and **may have appended zero entries** (for example a
///   batch holding only `.` and `..`); call again.
/// - `Ok(false)`: end of directory (`ERROR_NO_MORE_FILES`); nothing was appended.
/// - `Err`: enumeration failed; entries appended by earlier calls stay valid.
///
/// Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn query_dir_batch(
    dir: &mut DirHandle,
    query: &mut DirQuery,
    out: &mut Vec<DirEntry>,
) -> Result<bool, Win32Error> {
    let _ = (dir, query.restart, out);
    Err(pending(
        "GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)",
    ))
}

/// Zero-extends a 64-bit file ID (`FileIdBothDirectoryInfo`, M2) to the `FILE_ID_128` layout:
/// the little-endian ID in bytes 0 to 7, zeros in bytes 8 to 15. On NTFS this equals the
/// 128-bit ID that `FileIdInfo` reports for the same file.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn file_id_from_u64(id: u64) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&id.to_le_bytes());
    out
}

/// Which disposition call [`set_disposition_ex`] used, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) enum DispositionMode {
    /// `FileDispositionInfoEx` with `FILE_DISPOSITION_FLAG_POSIX_SEMANTICS`.
    PosixEx,
    /// Fallback `FileDispositionInfo { DeleteFile: TRUE }`.
    Legacy,
}

/// Marks a checked handle for deletion (section 8.4 step 9, W6): `SetFileInformationByHandle(
/// FileDispositionInfoEx, FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
/// [| FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE])`. Only on `ERROR_INVALID_PARAMETER` or
/// `ERROR_NOT_SUPPORTED` does it fall back to `FileDispositionInfo`. Attributes are never
/// modified. Errors name the class in `api`, e.g.
/// `"SetFileInformationByHandle(FileDispositionInfoEx)"`. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn set_disposition_ex(
    h: &Checked<DeleteHandle>,
    ignore_readonly: bool,
) -> Result<DispositionMode, Win32Error> {
    let _ = (sealed::Sealed::owned(&h.0), ignore_readonly);
    Err(pending("SetFileInformationByHandle(FileDispositionInfoEx)"))
}

/// `PHCM_APPLICATION_DEFAULT`: on a thread, "use the process mode".
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
const PHCM_APPLICATION_DEFAULT: i8 = 0;
/// `PHCM_EXPOSE_PLACEHOLDERS`.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
const PHCM_EXPOSE_PLACEHOLDERS: i8 = 2;
/// `PHCM_ERROR_INVALID_PARAMETER`.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
const PHCM_ERROR_INVALID_PARAMETER: i8 = -1;

/// Interprets the `CHAR` returned by the `Rtl*PlaceholderCompatibilityMode` functions: a
/// non-negative value is a mode (for the set functions, the *previous* mode, which is success),
/// and only a negative value is an error. `PHCM_ERROR_INVALID_PARAMETER` maps to
/// `ERROR_INVALID_PARAMETER`; any other negative value (`PHCM_ERROR_NO_TEB`) to
/// `ERROR_NOT_SUPPORTED`.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
fn phcm_result(api: &'static str, ret: i8) -> Result<i8, Win32Error> {
    match ret {
        0.. => Ok(ret),
        PHCM_ERROR_INVALID_PARAMETER => Err(Win32Error {
            api,
            code: code::ERROR_INVALID_PARAMETER,
        }),
        _ => Err(Win32Error {
            api,
            code: code::ERROR_NOT_SUPPORTED,
        }),
    }
}

/// True if placeholders are visible to the calling thread: the process mode is
/// `PHCM_EXPOSE_PLACEHOLDERS`, and the thread either follows it (`PHCM_APPLICATION_DEFAULT`) or
/// exposes them too. A thread-level `PHCM_DISGUISE_PLACEHOLDER` overrides the process mode, so
/// it fails the check.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
fn placeholders_visible(process_mode: i8, thread_mode: i8) -> bool {
    process_mode == PHCM_EXPOSE_PLACEHOLDERS
        && matches!(
            thread_mode,
            PHCM_APPLICATION_DEFAULT | PHCM_EXPOSE_PLACEHOLDERS
        )
}

/// Process start-up requirement (W3): `RtlSetProcessPlaceholderCompatibilityMode(
/// PHCM_EXPOSE_PLACEHOLDERS)`, so cloud placeholders show their reparse tag and attributes. The
/// call returns the previous mode on success; only a negative value is an error (see
/// `phcm_result`). On error the process must not scan or clean. Call once per process, before
/// the first open; the elevated worker is a separate process and makes its own call. The token
/// is the only way to call [`open_root`]. Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn expose_placeholders() -> Result<PlaceholdersExposed, Error> {
    Err(pending("RtlSetProcessPlaceholderCompatibilityMode").into())
}

/// Pre-target re-check (W3), run on the thread that processes each target, before its root is
/// opened or re-verified: `RtlQueryProcessPlaceholderCompatibilityMode` and
/// `RtlQueryThreadPlaceholderCompatibilityMode` must pass `placeholders_visible`. Anything else,
/// including a negative return, is an error and the target is aborted (something in the
/// process changed the mode after start-up). Stub.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn recheck_placeholders_exposed(exposed: &PlaceholdersExposed) -> Result<(), Error> {
    let _ = exposed;
    Err(pending("RtlQueryProcessPlaceholderCompatibilityMode").into())
}

/// Maps a failed `nt_open_*` call to a skip (section 8.4 steps 1 and 2), including the
/// reparse-check refusals of `nt_open_dir` / `nt_open_read_file` and the OS's own
/// `STATUS_CLOUD_FILE_*` errors. Anything unrecognized is [`SkipReason::FsError`]: an open
/// failure is always a skip.
#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
pub(crate) fn open_skip_reason(err: &NtError) -> SkipReason {
    match err.status {
        status::STATUS_REPARSE_POINT_ENCOUNTERED => SkipReason::ReparsePoint,
        status::STATUS_BEZEM_CLOUD_PLACEHOLDER
        | status::STATUS_CLOUD_FILE_FIRST..=status::STATUS_CLOUD_FILE_LAST => {
            SkipReason::CloudPlaceholder
        }
        status::STATUS_SHARING_VIOLATION => SkipReason::Locked,
        status::STATUS_ACCESS_DENIED => SkipReason::AccessDenied,
        status::STATUS_OBJECT_NAME_NOT_FOUND | status::STATUS_OBJECT_PATH_NOT_FOUND => {
            SkipReason::Gone
        }
        status::STATUS_DELETE_PENDING => SkipReason::DeletePending,
        status::STATUS_FILE_IS_A_DIRECTORY | status::STATUS_NOT_A_DIRECTORY => {
            SkipReason::ChangedSinceScan
        }
        _ => SkipReason::FsError,
    }
}

#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
fn pending(api: &'static str) -> Win32Error {
    if cfg!(windows) {
        Win32Error::not_implemented(api)
    } else {
        Win32Error::unsupported_platform(api)
    }
}

#[cfg_attr(not(test), expect(dead_code, reason = "frozen M1 API; no caller yet"))]
fn pending_nt(api: &'static str) -> NtError {
    if cfg!(windows) {
        NtError::not_implemented(api)
    } else {
        NtError::unsupported_platform(api)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_failures_map_to_skips() {
        use status::*;
        let r = |status| {
            open_skip_reason(&NtError {
                api: "NtCreateFile",
                status,
            })
        };
        assert_eq!(r(STATUS_SHARING_VIOLATION), SkipReason::Locked);
        assert_eq!(r(STATUS_ACCESS_DENIED), SkipReason::AccessDenied);
        assert_eq!(r(STATUS_OBJECT_NAME_NOT_FOUND), SkipReason::Gone);
        assert_eq!(r(STATUS_OBJECT_PATH_NOT_FOUND), SkipReason::Gone);
        assert_eq!(r(STATUS_DELETE_PENDING), SkipReason::DeletePending);
        assert_eq!(r(STATUS_FILE_IS_A_DIRECTORY), SkipReason::ChangedSinceScan);
        assert_eq!(r(STATUS_NOT_A_DIRECTORY), SkipReason::ChangedSinceScan);
        assert_eq!(r(STATUS_OBJECT_NAME_INVALID), SkipReason::FsError);
        assert_eq!(r(STATUS_DIRECTORY_NOT_EMPTY), SkipReason::FsError);
        assert_eq!(r(STATUS_NOT_IMPLEMENTED), SkipReason::FsError);
    }

    #[test]
    fn cloud_placeholders() {
        let rp = attr::REPARSE_POINT;
        assert!(!is_cloud_placeholder(0, 0));
        assert!(!is_cloud_placeholder(
            attr::READONLY | attr::SYSTEM,
            reparse_tag::CLOUD
        ));
        for a in [
            attr::OFFLINE,
            attr::RECALL_ON_OPEN,
            attr::RECALL_ON_DATA_ACCESS,
        ] {
            assert!(is_cloud_placeholder(a, 0), "{a:#x}");
        }
        assert!(is_cloud_placeholder(attr::PINNED, 0));
        assert!(is_cloud_placeholder(attr::UNPINNED, 0));
        for tag in [0x9000_001A, 0x9000_101A, 0x9000_F01A, reparse_tag::PROJFS] {
            assert!(is_cloud_placeholder(rp, tag), "{tag:#x}");
        }
        // Junctions, symlinks and the ProjFS tombstone are reparse points, not placeholders.
        for tag in [0xA000_0003, 0xA000_000C, 0xA000_0022] {
            assert!(!is_cloud_placeholder(rp | attr::DIRECTORY, tag), "{tag:#x}");
        }
        // Just outside the CLOUD mask: bit 16, the M/R bits, and the neighbouring value.
        for tag in [0x9001_001A, 0x8000_001A, 0x9000_001B] {
            assert!(!is_cloud_placeholder(rp, tag), "{tag:#x}");
        }
    }

    #[test]
    fn filetime_conversion() {
        let epoch = FileTime(116_444_736_000_000_000);
        assert_eq!(epoch.to_system_time(), Some(SystemTime::UNIX_EPOCH));
        let later = FileTime(116_444_736_000_000_000 + 15_000_001);
        assert_eq!(
            later.to_system_time(),
            Some(SystemTime::UNIX_EPOCH + Duration::new(1, 500_000_100))
        );
        let earlier = FileTime(116_444_736_000_000_000 - 10_000_000);
        assert_eq!(
            earlier.to_system_time(),
            SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(1))
        );
    }

    #[test]
    fn file_id_zero_extension() {
        let id = file_id_from_u64(0x0102_0304_0506_0708);
        assert_eq!(id, [8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn reparse_check_refuses_links_and_placeholders() {
        use status::*;
        let info = |attributes, reparse_tag| AttributeTagInfo {
            attributes,
            reparse_tag,
        };
        let rp = attr::REPARSE_POINT | attr::DIRECTORY;
        assert_eq!(reparse_rejection(&info(attr::DIRECTORY, 0)), None);
        assert_eq!(reparse_rejection(&info(0, 0xA000_0003)), None); // tag ignored without the bit
        let junction = reparse_rejection(&info(rp, 0xA000_0003));
        let symlink = reparse_rejection(&info(rp, 0xA000_000C));
        let cloud = reparse_rejection(&info(rp, 0x9000_301A));
        let offline = reparse_rejection(&info(attr::OFFLINE, 0));
        assert_eq!(junction, Some(STATUS_REPARSE_POINT_ENCOUNTERED));
        assert_eq!(symlink, Some(STATUS_REPARSE_POINT_ENCOUNTERED));
        assert_eq!(cloud, Some(STATUS_BEZEM_CLOUD_PLACEHOLDER));
        assert_eq!(offline, Some(STATUS_BEZEM_CLOUD_PLACEHOLDER));
        // ...and the refusals map to the matching skips.
        let skip = |s: Option<i32>| {
            open_skip_reason(&NtError {
                api: "NtCreateFile",
                status: s.unwrap(),
            })
        };
        assert_eq!(skip(junction), SkipReason::ReparsePoint);
        assert_eq!(skip(cloud), SkipReason::CloudPlaceholder);
        assert_eq!(
            skip(Some(0xC000_CF15_u32 as i32)),
            SkipReason::CloudPlaceholder
        ); // CLOUD_FILE_PINNED
        assert_eq!(skip(Some(0xC000_CE00_u32 as i32)), SkipReason::FsError);
    }

    #[test]
    fn placeholder_mode_results() {
        assert_eq!(phcm_result("X", 0), Ok(0)); // previous mode: success
        assert_eq!(phcm_result("X", 2), Ok(2));
        assert_eq!(
            phcm_result("X", -1).map_err(|e| e.code),
            Err(code::ERROR_INVALID_PARAMETER)
        );
        assert_eq!(
            phcm_result("X", -2).map_err(|e| e.code),
            Err(code::ERROR_NOT_SUPPORTED)
        );
        assert!(placeholders_visible(2, 0));
        assert!(placeholders_visible(2, 2));
        assert!(!placeholders_visible(2, 1)); // thread disguises
        assert!(!placeholders_visible(0, 0));
        assert!(!placeholders_visible(1, 2));
    }

    #[test]
    fn stubs_fail_closed() {
        let root = WidePath::drive_root('C').unwrap();
        let token = PlaceholdersExposed { _private: () };
        assert!(open_root(&root, &token).is_err());
        assert!(recheck_placeholders_exposed(&token).is_err());
        assert!(expose_placeholders().is_err());
        let mut q = DirQuery::new();
        q.reset();
        assert_eq!(q.class(), DirInfoClass::IdExtd);
    }
}
