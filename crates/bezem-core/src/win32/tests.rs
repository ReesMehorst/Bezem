// SPDX-License-Identifier: MIT

//! Windows-only runtime smoke tests for the wrappers (run by the `windows` CI job; on Linux
//! they are only type-checked via `--target x86_64-pc-windows-msvc`). They open their own
//! directory handle so they do not depend on the path layer. Junction/symlink and full
//! pipeline scenarios live in `tests/` (package D).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write as _;
use std::ops::ControlFlow;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, OPEN_EXISTING,
    SYNCHRONIZE,
};

use super::fs_impl::{Win32Dir, Win32Fs};
use super::known_folders::SystemKnownFolders;
use super::{dirent, finalpath, volume, OwnedHandle};
use crate::error::ErrorCode;
use crate::fs::{
    attr, DispositionError, DispositionKind, DispositionOpts, FsBackend, IdQuality, ItemKind,
    VolumeKind,
};
use crate::resolve::{KnownFolderId, KnownFolderSource};
use crate::types::SkipReason;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Opens `p` like `open_root(VerifyNoFollow)` does, without going through `WidePath`.
fn open_dir(p: &Path) -> OwnedHandle {
    let mut wide: Vec<u16> = p.as_os_str().encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a live NUL-terminated UTF-16 string; no security attributes/template.
    let h = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .unwrap();
    // SAFETY: freshly opened handle, owned by nobody else.
    unsafe { OwnedHandle::from_raw(h) }.unwrap()
}

fn write_file(p: &Path, bytes: &[u8]) {
    File::create(p).unwrap().write_all(bytes).unwrap();
}

fn set_readonly(p: &Path, ro: bool) {
    let mut perm = fs::metadata(p).unwrap().permissions();
    perm.set_readonly(ro);
    fs::set_permissions(p, perm).unwrap();
}

/// name -> (attributes, size, file id)
type Listed = BTreeMap<String, (u32, u64, Option<[u8; 16]>)>;

fn listing(dir: &OwnedHandle) -> (IdQuality, Listed) {
    let mut m = BTreeMap::new();
    let q = dirent::read_dir(dir, &mut |e| {
        m.insert(
            String::from_utf16_lossy(e.name),
            (e.attributes, e.size, e.file_id),
        );
        ControlFlow::Continue(())
    })
    .unwrap();
    (q, m)
}

#[test]
fn list_query_final_path_and_posix_delete() {
    let td = tempfile::tempdir().unwrap();
    write_file(&td.path().join("a.txt"), b"hello");
    fs::create_dir(td.path().join("sub")).unwrap();
    write_file(&td.path().join("sub").join("inner"), b"x");

    let fs_ = Win32Fs::default();
    let dir = Win32Dir::new(open_dir(td.path()));

    let (q, m) = listing(&open_dir(td.path()));
    assert_ne!(q, IdQuality::None, "NTFS/ReFS runners provide file ids");
    assert_eq!(m.keys().collect::<Vec<_>>(), ["a.txt", "sub"]);
    assert_eq!(m["a.txt"].1, 5);
    assert!(m["sub"].0 & attr::DIRECTORY != 0);

    // Read through the trait as well (same results, lock path exercised).
    let mut names = Vec::new();
    fs_.read_dir(&dir, &mut |e| {
        names.push(String::from_utf16_lossy(e.name));
        ControlFlow::Continue(())
    })
    .unwrap();
    names.sort();
    assert_eq!(names, ["a.txt", "sub"]);

    let item = fs_.open_item(&dir, &w("a.txt"), ItemKind::File).unwrap();
    let qi = fs_.query_item(&item).unwrap();
    assert_eq!(qi.size, 5);
    assert_eq!(qi.link_count, 1);
    assert!(!qi.is_directory);
    assert!(!qi.delete_pending);
    assert!(!attr::is_reparse(qi.attributes, qi.reparse_tag));
    if q == IdQuality::Full128 {
        assert_eq!(Some(qi.identity.file_id), m["a.txt"].2);
    }

    let fp = String::from_utf16_lossy(&fs_.final_path(&item).unwrap());
    assert!(fp.starts_with(r"\\?\"), "{fp}");
    assert!(fp.ends_with(r"\a.txt"), "{fp}");

    let kind = fs_
        .set_disposition(
            &item,
            DispositionOpts {
                ignore_readonly: false,
            },
        )
        .unwrap();
    assert_eq!(kind, DispositionKind::Posix, "NTFS supports POSIX delete");
    drop(item);
    assert!(!td.path().join("a.txt").exists());

    // Non-empty directory => NotEmpty, left in place.
    let sub = fs_.open_item(&dir, &w("sub"), ItemKind::Dir).unwrap();
    assert_eq!(
        fs_.set_disposition(
            &sub,
            DispositionOpts {
                ignore_readonly: false
            }
        ),
        Err(DispositionError::NotEmpty)
    );
    drop(sub);
    assert!(td.path().join("sub").is_dir());

    // Subdir open + listing.
    let subdir = fs_.open_subdir(&dir, &w("sub")).unwrap();
    let mut inner = Vec::new();
    fs_.read_dir(&subdir, &mut |e| {
        inner.push(String::from_utf16_lossy(e.name));
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(inner, ["inner"]);
}

#[test]
fn errors_map_to_skip_reasons() {
    let td = tempfile::tempdir().unwrap();
    write_file(&td.path().join("ro.txt"), b"r");
    write_file(&td.path().join("held.txt"), b"h");
    set_readonly(&td.path().join("ro.txt"), true);

    let fs_ = Win32Fs::default();
    let dir = Win32Dir::new(open_dir(td.path()));

    // Bad names are refused before any OS call.
    for bad in ["", ".", "..", r"a\b", "a/b", "ro.txt:s"] {
        let e = fs_.open_item(&dir, &w(bad), ItemKind::File).unwrap_err();
        assert_eq!(e.code, ErrorCode::Sim("bad_name"), "{bad:?}");
    }

    // Missing => Gone. Wrong kind => ChangedSinceScan.
    let e = fs_.open_item(&dir, &w("nope"), ItemKind::File).unwrap_err();
    assert_eq!(e.reason, SkipReason::Gone);
    let e = fs_.open_subdir(&dir, &w("held.txt")).unwrap_err();
    assert_eq!(e.reason, SkipReason::ChangedSinceScan);

    // A writer without our share mode => Locked (FILE_SHARE_READ only on the delete open).
    {
        let _writer = fs::OpenOptions::new()
            .write(true)
            .open(td.path().join("held.txt"))
            .unwrap();
        let e = fs_
            .open_item(&dir, &w("held.txt"), ItemKind::File)
            .unwrap_err();
        assert_eq!(e.reason, SkipReason::Locked);
    }

    // Read-only without the override => ReadOnly, file kept.
    let item = fs_.open_item(&dir, &w("ro.txt"), ItemKind::File).unwrap();
    match fs_.set_disposition(
        &item,
        DispositionOpts {
            ignore_readonly: false,
        },
    ) {
        Err(DispositionError::Fs(e)) => assert_eq!(e.reason, SkipReason::ReadOnly),
        other => panic!("expected ReadOnly, got {other:?}"),
    }
    drop(item);
    assert!(td.path().join("ro.txt").exists());

    // With the override (Windows 10 1809+): deleted, attribute not cleared beforehand.
    let item = fs_.open_item(&dir, &w("ro.txt"), ItemKind::File).unwrap();
    assert!(fs_.query_item(&item).unwrap().attributes & attr::READONLY != 0);
    let r = fs_.set_disposition(
        &item,
        DispositionOpts {
            ignore_readonly: true,
        },
    );
    drop(item);
    assert_eq!(r, Ok(DispositionKind::Posix));
    assert!(!td.path().join("ro.txt").exists());
}

#[test]
fn final_path_and_volume_of_temp_dir() {
    let td = tempfile::tempdir().unwrap();
    let h = open_dir(td.path());
    let fp = finalpath::final_path(&h).unwrap();
    let s = String::from_utf16_lossy(&fp);
    assert!(s.starts_with(r"\\?\") && s.as_bytes()[5] == b':', "{s}");
    assert_eq!(
        volume::volume_kind(&h, &fp).unwrap(),
        VolumeKind::LocalFixed
    );
}

#[test]
fn system_known_folders() {
    let src = SystemKnownFolders;
    for id in [
        KnownFolderId::Temp,
        KnownFolderId::LocalAppData,
        KnownFolderId::RoamingAppData,
        KnownFolderId::Profile,
        KnownFolderId::Windows,
        KnownFolderId::ProgramData,
    ] {
        let v = src.get(id).unwrap();
        let s = String::from_utf16_lossy(&v);
        assert!(s.len() > 3, "{id:?}: {s}");
        assert!(!s.ends_with('\\'), "{id:?}: {s}");
        assert!(!s.contains('\0'), "{id:?}: {s}");
    }
    // Resolved once: identical on repeat.
    assert_eq!(src.get(KnownFolderId::Temp), src.get(KnownFolderId::Temp));
}
