<!-- SPDX-License-Identifier: MIT -->

# `crate::win32`: frozen signatures (M1)

This is the only module with `unsafe` code. It is private to `bezem-core` (ARCHITECTURE.md §2.1);
only the error types leave the crate (`bezem_core::{OsError, Win32Error, NtError, HresultError}`).

These signatures were frozen at M1 so the safety and engine workstreams can build against them
before the bodies exist. A stub returns `not_implemented(api)` on Windows and
`unsupported_platform(api)` elsewhere, and never panics. Changing a signature needs the lead's
approval. The doc comments in the source are the full contract. Every item that has no caller
yet carries `#[cfg_attr(not(test), expect(dead_code, ...))]`; when you add its first caller,
the build fails until you delete that attribute.

Path types come from `crate::path` (`WidePath`, `PathComponent`, `PathError`, `FileIdentity`),
never from `crate::safety`. Untrusted OS output (`DirEntry.name`, `final_path`) stays raw `Vec<u16>`.

## Fences that the compiler cannot enforce

**The authoritative fences live in CI** (`.github/workflows/ci.yml`, added by ci-release). They
match the two promotion definitions literally, so these exact lines must not change without
telling the lead:

```rust
    pub(crate) fn into_verified_dir(self) -> DirHandle {
    pub(crate) fn attest(handle: DeleteHandle) -> Self {
```

For reference, the checks as first proposed (each must print nothing beyond what is noted):

```sh
# unsafe_code relaxed only on the `mod win32;` declaration in lib.rs (exactly one line)
git grep -nE '^[[:space:]]*#!?\[.*(allow|expect|warn)\(.*unsafe_code' -- '*.rs'
# root promotion only in safety (section 8.3)
git grep -n 'into_verified_dir' -- '*.rs' ':!crates/bezem-core/src/win32/' ':!crates/bezem-core/src/safety/'
# delete attestation only in delete (section 8.4 steps 3-8)
git grep -n 'attest(' -- '*.rs' ':!crates/bezem-core/src/win32/' ':!crates/bezem-core/src/delete*'
```

## W1: foundation (implemented)

```rust
pub(crate) struct OwnedHandle;   // closes exactly once on Drop; Send + Sync; raw value only inside win32
pub struct Win32Error   { pub api: &'static str, pub code: u32 }
pub struct NtError      { pub api: &'static str, pub status: i32 }
pub struct HresultError { pub api: &'static str, pub hr: i32 }
pub enum   Error { Win32(Win32Error), Nt(NtError), Hresult(HresultError),
                   Path { api: &'static str, reason: PathError } }      // re-exported as OsError
not_implemented(api) / unsupported_platform(api)                        // on all three structs
error::{code, status, hresult}                                          // constants with symbol names
```

## W2: Known Folders (`win32::known_folders`)

```rust
pub(crate) enum KnownFolder { LocalAppData, LocalAppDataLow, RoamingAppData, Recent, Profile,
    ProgramData, Windows, ProgramFiles, ProgramFilesX86, Desktop, Documents, Downloads, Pictures,
    Music, Videos, OneDrive }
pub(crate) trait KnownFolders: sealed::Sealed + Send + Sync + Debug {
    fn folder(&self, folder: KnownFolder) -> Result<WidePath, Error>;  // SHGetKnownFolderPath -> Error::Hresult
    fn temp_dir(&self) -> Result<WidePath, Error>;                     // GetTempPath2W, trailing '\' removed; reads TMP/TEMP/USERPROFILE
}
pub(crate) struct SystemKnownFolders;                                  // production
pub(crate) struct FixedKnownFolders;   // cfg(test): new().with(folder, WidePath).with_temp(WidePath)
pub(crate) fn parse_win32_drive_path(units: &[u16]) -> Result<WidePath, PathError>;  // implemented
pub(crate) fn parse_temp_path(units: &[u16]) -> Result<WidePath, PathError>;         // implemented; "C:\\" gives a drive root, which resolve must reject
```

## W3: opens, info queries, placeholders (`win32::fs`)

```rust
pub(crate) struct UnverifiedRoot; DirHandle; DeleteHandle; ReadHandle;   // handle roles
pub(crate) struct Checked<H>;                                            // steps 3-8 passed on H
pub(crate) trait FileHandle: sealed::Sealed {}   // UnverifiedRoot, DirHandle, DeleteHandle, ReadHandle
UnverifiedRoot::into_verified_dir(self) -> DirHandle          // safety::verify_root only (fence)
Checked::<DeleteHandle>::attest(DeleteHandle) -> Self         // delete only (fence; token design: E3)
pub(crate) enum Intent { Normal, Backup }        // nt_open_dir only; SeBackupPrivilege only, never SeRestorePrivilege
pub(crate) struct PlaceholdersExposed;            // zero-sized; only expose_placeholders() creates it

pub(crate) fn expose_placeholders() -> Result<PlaceholdersExposed, Error>;   // once per process (worker too); previous mode = success, negative = Err
pub(crate) fn recheck_placeholders_exposed(exposed: &PlaceholdersExposed) -> Result<(), Error>;  // before each target: process + thread mode
pub(crate) fn open_root(path: &WidePath, exposed: &PlaceholdersExposed) -> Result<UnverifiedRoot, Win32Error>;
pub(crate) fn nt_open_dir(parent: &DirHandle, name: &PathComponent, intent: Intent) -> Result<DirHandle, NtError>;   // refuses reparse points
pub(crate) fn nt_open_delete_file(parent: &DirHandle, name: &PathComponent) -> Result<DeleteHandle, NtError>;
pub(crate) fn nt_open_delete_dir(parent: &DirHandle, name: &PathComponent) -> Result<DeleteHandle, NtError>;
pub(crate) fn nt_open_read_file(parent: &DirHandle, name: &PathComponent) -> Result<ReadHandle, NtError>;           // refuses reparse points
pub(crate) fn reparse_rejection(info: &AttributeTagInfo) -> Option<i32>;   // implemented: cloud status, then REPARSE_POINT_ENCOUNTERED
pub(crate) fn read_to_end(h: &ReadHandle, max_len: u64) -> Result<Vec<u8>, Win32Error>;   // ERROR_FILE_TOO_LARGE over cap
pub(crate) fn query_attribute_tag(h: &impl FileHandle) -> Result<AttributeTagInfo, Win32Error>;
pub(crate) fn query_id(h: &impl FileHandle)       -> Result<FileIdentity, Win32Error>;   // Err without FileIdInfo (FAT: M2)
pub(crate) fn query_standard(h: &impl FileHandle) -> Result<StandardInfo, Win32Error>;
pub(crate) fn query_basic(h: &impl FileHandle)    -> Result<BasicInfo, Win32Error>;      // FileTime::to_system_time()
pub(crate) fn is_cloud_placeholder(attributes: u32, reparse_tag: u32) -> bool;          // implemented
attr::{READONLY, SYSTEM, DIRECTORY, REPARSE_POINT, OFFLINE, RECALL_ON_OPEN, PINNED, UNPINNED, RECALL_ON_DATA_ACCESS}
reparse_tag::{CLOUD, CLOUD_MASK, PROJFS}
```

Every `nt_open_*`: `NtCreateFile`, `RootDirectory = parent`, `OBJ_CASE_INSENSITIVE`, `FILE_OPEN`,
`FILE_OPEN_REPARSE_POINT | FILE_OPEN_NO_RECALL | FILE_SYNCHRONOUS_IO_NONALERT`.
`FILE_OPEN_FOR_BACKUP_INTENT` is added only by `nt_open_dir`, only for `Intent::Backup`.
`ObjectName.Length` is `u16::try_from(units * 2)` (at most 510 bytes, because PathComponent caps
names at 255 units); a failed conversion is an error, never a truncated name. No share mode includes
`FILE_SHARE_DELETE`.

**Reparse check** (`nt_open_dir`, `nt_open_read_file`): after the open, the wrapper queries
`NtQueryInformationFile(FileAttributeTagInformation)` and applies `reparse_rejection`. On a
rejection, or if the query fails, it closes the handle and returns an `NtError`, so a `DirHandle`
or `ReadHandle` never refers to a reparse point. `open_skip_reason` maps:

| Status | Skip |
|---|---|
| `STATUS_REPARSE_POINT_ENCOUNTERED` (0xC000050B) | `ReparsePoint` |
| `STATUS_BEZEM_CLOUD_PLACEHOLDER` (0xE0000001, Bezem-defined, customer bit) | `CloudPlaceholder` |
| `STATUS_CLOUD_FILE_*` (0xC000CF00–0xC000CFFF, from the OS) | `CloudPlaceholder` |

Windows test of junctions, directory symlinks, file symlinks and placeholders: TODO(W3), in Windows CI.

| Open | Access | Share | Options |
|---|---|---|---|
| `open_root` | `LIST_DIRECTORY \| TRAVERSE \| READ_ATTRIBUTES \| SYNCHRONIZE` | `READ \| WRITE` | backup semantics, open reparse point, no recall |
| `nt_open_dir` | same | `READ \| WRITE` | `FILE_DIRECTORY_FILE` |
| `nt_open_delete_file` | `DELETE \| READ_ATTRIBUTES \| SYNCHRONIZE` | `READ` (section 8.1 "Files in use") | `FILE_NON_DIRECTORY_FILE` |
| `nt_open_delete_dir` | `DELETE \| READ_ATTRIBUTES \| SYNCHRONIZE` | `READ \| WRITE` | `FILE_DIRECTORY_FILE` |
| `nt_open_read_file` | `READ_DATA \| READ_ATTRIBUTES \| SYNCHRONIZE` | `READ \| WRITE` | `FILE_NON_DIRECTORY_FILE` |

## W4: final path and volume

```rust
pub(crate) fn final_path(h: &impl FileHandle) -> Result<Vec<u16>, Win32Error>;  // raw; parse with WidePath::parse_extended
pub(crate) enum VolumeKind { LocalFixed, Remote, Removable, Other, Unknown(Error) }
pub(crate) fn volume_kind(h: &impl FileHandle) -> VolumeKind;                   // only LocalFixed passes
pub(crate) fn volume_serial(h: &impl FileHandle) -> Result<u64, Win32Error>;    // == FileIdentity::volume_serial
```

## W5: directory enumeration

```rust
pub(crate) enum DirInfoClass { IdExtd, IdBoth /* M2 */, Full /* M2 */ }
pub(crate) struct DirEntry { name: Vec<u16> /* untrusted */, attributes: u32, reparse_tag: u32,
    end_of_file: u64, allocation_size: u64, creation_time, last_access_time, last_write_time,
    change_time: FileTime, file_id: Option<[u8; 16]> }
pub(crate) struct DirQuery;  DirQuery::new(); class(&self); reset(&mut self)
pub(crate) fn query_dir_batch(dir: &mut DirHandle, query: &mut DirQuery, out: &mut Vec<DirEntry>)
    -> Result<bool, Win32Error>;   // Ok(true): may have appended 0 entries, call again; Ok(false): end, nothing appended
pub(crate) fn file_id_from_u64(id: u64) -> [u8; 16];   // implemented: LE bytes 0-7, zeros 8-15
```

A `DirEntry.name` that fails `PathComponent::from_wide` is skipped as `SkipReason::UnsupportedName`.

## W6: deletion

```rust
pub(crate) enum DispositionMode { PosixEx, Legacy }
pub(crate) fn set_disposition_ex(h: &Checked<DeleteHandle>, ignore_readonly: bool)
    -> Result<DispositionMode, Win32Error>;   // api names the class, e.g. "SetFileInformationByHandle(FileDispositionInfoEx)"
pub(crate) fn open_skip_reason(err: &NtError) -> crate::report::SkipReason;   // implemented
```
