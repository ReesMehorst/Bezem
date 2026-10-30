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
//! Every open adds `FILE_OPEN_NO_RECALL`, so no open hydrates a placeholder. The process must
//! call [`expose_placeholders`] once at startup, before any scan, so placeholders show their
//! reparse tag and attributes. If that call fails the process must not scan or clean (fail
//! closed).

use std::time::{Duration, SystemTime};

use super::error::{status, Error, NtError, Win32Error};
use super::handle::OwnedHandle;
use crate::path::{FileIdentity, PathComponent, WidePath};
use crate::report::SkipReason;

/// `FILE_ATTRIBUTE_*` bits the safety layer inspects (values from WinNT.h).
pub(crate) mod attr {
    /// Read-only file.
    pub(crate) const READONLY: u32 = 0x1;
    /// Used by the operating system.
    pub(crate) const SYSTEM: u32 = 0x4;
    /// Directory.
    pub(crate) const DIRECTORY: u32 = 0x10;
    /// Has a reparse point (any tag): never traversed or deleted.
    pub(crate) const REPARSE_POINT: u32 = 0x400;
    /// Data not available locally.
    pub(crate) const OFFLINE: u32 = 0x1000;
    /// Cloud placeholder. Reported **only by directory enumeration**, so it is checked at scan
    /// time from [`DirEntry::attributes`](super::DirEntry), never after an open.
    pub(crate) const RECALL_ON_OPEN: u32 = 0x4_0000;
    /// Hierarchical-storage intent: keep the file local.
    pub(crate) const PINNED: u32 = 0x8_0000;
    /// Hierarchical-storage intent: the file may be dehydrated.
    pub(crate) const UNPINNED: u32 = 0x10_0000;
    /// Not fully present locally; reading the data fetches it from a remote store.
    pub(crate) const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
}

/// Reparse tags that mark cloud or projected files (MS-FSCC 2.1.2.1).
pub(crate) mod reparse_tag {
    /// `IO_REPARSE_TAG_CLOUD`; `IO_REPARSE_TAG_CLOUD_1` to `_F` set bits 12 to 15.
    pub(crate) const CLOUD: u32 = 0x9000_001A;
    /// Clears bits 12 to 15, so every `IO_REPARSE_TAG_CLOUD_n` compares equal to [`CLOUD`].
    pub(crate) const CLOUD_MASK: u32 = 0xFFFF_0FFF;
    /// `IO_REPARSE_TAG_PROJFS` (Projected File System, e.g. VFS for Git).
    pub(crate) const PROJFS: u32 = 0x9000_001C;
}

/// True if an item is (or may be) a cloud or projected placeholder and must be skipped as
/// `CloudPlaceholder`: any of `OFFLINE`, `RECALL_ON_OPEN`, `RECALL_ON_DATA_ACCESS`, `PINNED`,
/// `UNPINNED`, or a reparse point with a cloud or ProjFS tag. Check this before the generic
/// reparse-point rule so the more specific reason is reported.
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
pub(crate) struct FileTime(pub(crate) i64);

impl FileTime {
    /// 100 ns intervals between 1601-01-01 and 1970-01-01.
    const UNIX_EPOCH_TICKS: i128 = 116_444_736_000_000_000;

    /// The same instant as a [`SystemTime`], or `None` if it is out of range. Exact: no
    /// rounding beyond the 100 ns resolution.
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
pub(crate) struct AttributeTagInfo {
    /// `FILE_ATTRIBUTE_*` bits.
    pub(crate) attributes: u32,
    /// Reparse tag; meaningful only if `attributes` has [`attr::REPARSE_POINT`].
    pub(crate) reparse_tag: u32,
}

/// `FILE_STANDARD_INFO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub trait Sealed {
        fn owned(&self) -> &super::OwnedHandle;
    }
}

/// Any handle role that may be inspected with the info queries. Sealed.
pub(crate) trait FileHandle: sealed::Sealed {}

/// A root opened by path and not yet verified. See the module docs.
#[derive(Debug)]
pub(crate) struct UnverifiedRoot(OwnedHandle);

/// A directory opened for listing and traversal (section 8.4 step 1), or a promoted root.
#[derive(Debug)]
pub(crate) struct DirHandle(OwnedHandle);

/// A file or directory opened with `DELETE` access (section 8.4 step 2, "Empty directories").
#[derive(Debug)]
pub(crate) struct DeleteHandle(OwnedHandle);

/// A file opened for reading only (sections 7.4 and 7.5).
#[derive(Debug)]
pub(crate) struct ReadHandle(OwnedHandle);

/// A handle whose identity, attributes, link count, age and final path have been checked on
/// that same handle (section 8.4 steps 3 to 8; invariant 3 of section 8.2).
#[derive(Debug)]
pub(crate) struct Checked<H>(H);

impl UnverifiedRoot {
    /// Promotes the root to a [`DirHandle`]. **Only `safety::verify_root` may call this**, after
    /// steps 4 to 9 of section 8.3 passed on this same handle. Enforced by the CI grep in
    /// `README.md`, because Rust cannot express it without `win32` depending on `safety`.
    pub(crate) fn into_verified_dir(self) -> DirHandle {
        DirHandle(self.0)
    }
}

impl Checked<DeleteHandle> {
    /// Attests that steps 3 to 8 of section 8.4 passed on `handle`. **Only the `delete` module
    /// may call this** (CI grep in `README.md`); the full token design belongs to E3. The
    /// intended pattern: open, query, compare, attest, `set_disposition_ex`, drop, with no
    /// await point or reopen in between.
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

/// Whether an open asks for `FILE_OPEN_FOR_BACKUP_INTENT` (only the elevated worker does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Intent {
    /// Normal access checks.
    Normal,
    /// `FILE_OPEN_FOR_BACKUP_INTENT`; effective only with the backup/restore privileges.
    Backup,
}

/// Opens a root by absolute path (section 8.3 step 3, W3): `CreateFileW(path, FILE_LIST_DIRECTORY
/// | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_SHARE_READ | FILE_SHARE_WRITE,
/// OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT |
/// FILE_FLAG_OPEN_NO_RECALL)`. `WidePath` guarantees the `\\?\X:\` form. Stub.
pub(crate) fn open_root(path: &WidePath) -> Result<UnverifiedRoot, Win32Error> {
    let _ = path;
    Err(pending("CreateFileW"))
}

// Contract shared by the four `nt_open_*` functions (W3):
// NtCreateFile with OBJECT_ATTRIBUTES { RootDirectory = parent, ObjectName = name,
// Attributes = OBJ_CASE_INSENSITIVE }, CreateDisposition = FILE_OPEN, and CreateOptions always
// including FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL | FILE_SYNCHRONOUS_IO_NONALERT, plus
// FILE_OPEN_FOR_BACKUP_INTENT for `Intent::Backup`. ObjectName.Length is the name's size in
// bytes, built with `u16::try_from(units * 2)`: PathComponent caps names at 255 units (510
// bytes), and a failed conversion returns STATUS_OBJECT_NAME_INVALID, never a truncated name.
// No share mode ever includes FILE_SHARE_DELETE. Map failures with `open_skip_reason`.

/// Opens a subdirectory (section 8.4 step 1): `FILE_LIST_DIRECTORY | FILE_TRAVERSE |
/// FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share `READ | WRITE` (pins the directory),
/// `FILE_DIRECTORY_FILE`. The caller still checks `FileAttributeTagInfo` for a reparse point
/// (W3 may move that check in here). Stub.
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
/// Stub.
pub(crate) fn nt_open_delete_file(
    parent: &DirHandle,
    name: &PathComponent,
    intent: Intent,
) -> Result<DeleteHandle, NtError> {
    let _ = (parent, name, intent);
    Err(pending_nt("NtCreateFile"))
}

/// Opens an empty-directory candidate for deletion (section 8.4, "Empty directories"):
/// `DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share `READ | WRITE`, `FILE_DIRECTORY_FILE`.
/// Stub.
pub(crate) fn nt_open_delete_dir(
    parent: &DirHandle,
    name: &PathComponent,
    intent: Intent,
) -> Result<DeleteHandle, NtError> {
    let _ = (parent, name, intent);
    Err(pending_nt("NtCreateFile"))
}

/// Opens a small configuration file for reading (`Local State`, `profiles.ini`):
/// `FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, share `READ | WRITE`,
/// `FILE_NON_DIRECTORY_FILE`. Stub.
pub(crate) fn nt_open_read_file(
    parent: &DirHandle,
    name: &PathComponent,
    intent: Intent,
) -> Result<ReadHandle, NtError> {
    let _ = (parent, name, intent);
    Err(pending_nt("NtCreateFile"))
}

/// Reads the whole file. Fails with `ERROR_FILE_TOO_LARGE` if it is larger than `max_len`
/// bytes, checked before and while reading, so a growing file cannot exceed the cap. Stub.
pub(crate) fn read_to_end(h: &ReadHandle, max_len: u64) -> Result<Vec<u8>, Win32Error> {
    let _ = (h, max_len);
    Err(pending("ReadFile"))
}

/// `GetFileInformationByHandleEx(FileAttributeTagInfo)` (W3). Stub.
pub(crate) fn query_attribute_tag(h: &impl FileHandle) -> Result<AttributeTagInfo, Win32Error> {
    let _ = h.owned();
    Err(pending(
        "GetFileInformationByHandleEx(FileAttributeTagInfo)",
    ))
}

/// `GetFileInformationByHandleEx(FileIdInfo)` (W3). On file systems without `FileIdInfo`
/// (FAT, exFAT) this fails, and so does root verification: FAT/exFAT support is M2. Stub.
pub(crate) fn query_id(h: &impl FileHandle) -> Result<FileIdentity, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileIdInfo)"))
}

/// `GetFileInformationByHandleEx(FileStandardInfo)` (W3). A negative size from the OS is an
/// error, never clamped. Stub.
pub(crate) fn query_standard(h: &impl FileHandle) -> Result<StandardInfo, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileStandardInfo)"))
}

/// `GetFileInformationByHandleEx(FileBasicInfo)` (W3). Stub.
pub(crate) fn query_basic(h: &impl FileHandle) -> Result<BasicInfo, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileBasicInfo)"))
}

/// `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)` (W4): untrusted output,
/// returned verbatim without the trailing NUL. Callers parse it with
/// `WidePath::parse_extended`, which rejects anything but `\\?\X:\...`. Stub.
pub(crate) fn final_path(h: &impl FileHandle) -> Result<Vec<u16>, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFinalPathNameByHandleW"))
}

/// The kind of volume holding a handle. Only [`VolumeKind::LocalFixed`] passes root
/// verification; every other value, including a failed query, rejects the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub(crate) fn volume_kind(h: &impl FileHandle) -> VolumeKind {
    let _ = h.owned();
    VolumeKind::Unknown(pending_nt("NtQueryVolumeInformationFile").into())
}

/// The 64-bit volume serial of the volume holding `h` (W4), taken from `FILE_ID_INFO`, so it
/// equals [`FileIdentity::volume_serial`]. Fails where `FileIdInfo` is unsupported (M2). Stub.
pub(crate) fn volume_serial(h: &impl FileHandle) -> Result<u64, Win32Error> {
    let _ = h.owned();
    Err(pending("GetFileInformationByHandleEx(FileIdInfo)"))
}

/// Which directory information class a [`DirQuery`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub(crate) struct DirQuery {
    class: DirInfoClass,
    restart: bool,
}

impl DirQuery {
    /// A fresh enumeration with [`DirInfoClass::IdExtd`].
    pub(crate) fn new() -> Self {
        Self {
            class: DirInfoClass::IdExtd,
            restart: true,
        }
    }

    /// The class in use.
    pub(crate) fn class(&self) -> DirInfoClass {
        self.class
    }

    /// Restarts from the first entry on the next [`query_dir_batch`] call (uses the
    /// `...RestartInfo` class). Keeps the class.
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
/// (W5). `.` and `..` are omitted; every other name is returned verbatim.
///
/// - `Ok(true)`: the call succeeded and **may have appended zero entries** (for example a
///   batch holding only `.` and `..`); call again.
/// - `Ok(false)`: end of directory (`ERROR_NO_MORE_FILES`); nothing was appended.
/// - `Err`: enumeration failed; entries appended by earlier calls stay valid.
///
/// Stub.
pub(crate) fn query_dir_batch(
    dir: &DirHandle,
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
pub(crate) fn file_id_from_u64(id: u64) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&id.to_le_bytes());
    out
}

/// Which disposition call [`set_disposition_ex`] used, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub(crate) fn set_disposition_ex(
    h: &Checked<DeleteHandle>,
    ignore_readonly: bool,
) -> Result<DispositionMode, Win32Error> {
    let _ = (sealed::Sealed::owned(&h.0), ignore_readonly);
    Err(pending("SetFileInformationByHandle(FileDispositionInfoEx)"))
}

/// Process start-up requirement (W3): `RtlSetProcessPlaceholderCompatibilityMode(
/// PHCM_EXPOSE_PLACEHOLDERS)`, so cloud placeholders show their reparse tag and attributes. A
/// negative return (`PHCM_ERROR_*`) is an error, and the process must then refuse to scan or
/// clean. Call once, before the first open. Stub.
pub(crate) fn expose_placeholders() -> Result<(), Error> {
    Err(pending_nt("RtlSetProcessPlaceholderCompatibilityMode").into())
}

/// Maps a failed `nt_open_*` call to a skip (section 8.4 steps 1 and 2). Anything
/// unrecognized is [`SkipReason::FsError`]: an open failure is always a skip.
pub(crate) fn open_skip_reason(err: &NtError) -> SkipReason {
    match err.status {
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

fn pending(api: &'static str) -> Win32Error {
    if cfg!(windows) {
        Win32Error::not_implemented(api)
    } else {
        Win32Error::unsupported_platform(api)
    }
}

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
    fn stubs_fail_closed() {
        let root = WidePath::drive_root('C').unwrap();
        assert!(open_root(&root).is_err());
        assert!(expose_placeholders().is_err());
        let mut q = DirQuery::new();
        q.reset();
        assert_eq!(q.class(), DirInfoClass::IdExtd);
    }
}
