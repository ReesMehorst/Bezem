// SPDX-License-Identifier: MIT

//! Unit tests for the safety policy: `SafetyPolicy::new`, `verify_root`, `verdict` and
//! `check_against_plan`. Uses a tiny private fake [`FsBackend`] (only `open_root` does
//! anything), so these tests do not depend on `MemFs`.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::error::ErrorCode;
use crate::fs::{
    attr, DispositionError, DispositionKind, DispositionOpts, FsError, IdQuality, ItemKind,
    ItemQuery, OpenRootError, RawEntry, RootOpenMode, RootProbe, VolumeKind,
};
use crate::path::{parse_root_spec, PathReject};
use crate::resolve::{resolve_root, KnownFolderError, KnownFolderId, ResolvedRoot};
use crate::target::Target;
use crate::types::{CleanerId, FileTime, SkipReason, Verdict};
use crate::walk::{DirIndex, PlanEntry};

// ---------------------------------------------------------------------------------------------
// Fake backend

#[derive(Debug, Clone)]
enum Entry {
    Dir {
        /// Final path for a no-follow open; `None` = the requested path itself.
        final_path: Option<String>,
        /// Final path for a following open (a junction-ed / relocated folder).
        follow_final: Option<String>,
        is_dir: bool,
        is_reparse: bool,
        volume: VolumeKind,
        serial: u64,
    },
    NotFound,
    Fail(FsError),
}

impl Entry {
    fn dir() -> Entry {
        Entry::Dir {
            final_path: None,
            follow_final: None,
            is_dir: true,
            is_reparse: false,
            volume: VolumeKind::LocalFixed,
            serial: 1,
        }
    }
}

#[derive(Debug, Default)]
struct FakeFs {
    /// Keyed by the upper-cased display path (`C:\...`).
    over: BTreeMap<String, Entry>,
    calls: Mutex<Vec<(String, RootOpenMode)>>,
}

fn fail() -> FsError {
    FsError::new(SkipReason::AccessDenied, ErrorCode::Win32(5))
}

fn not_impl() -> FsError {
    FsError::new(SkipReason::FsError, ErrorCode::Sim("fake"))
}

impl FakeFs {
    fn with(mut self, path: &str, e: Entry) -> Self {
        self.over.insert(path.to_ascii_uppercase(), e);
        self
    }
    fn modes(&self) -> Vec<(String, RootOpenMode)> {
        self.calls.lock().unwrap().clone()
    }
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl FsBackend for FakeFs {
    type Dir = ();
    type Item = ();

    fn open_root(
        &self,
        path: &WidePath,
        mode: RootOpenMode,
    ) -> Result<RootProbe<()>, OpenRootError> {
        let shown = path.display_string();
        self.calls.lock().unwrap().push((shown.clone(), mode));
        let entry = self
            .over
            .get(&shown.to_ascii_uppercase())
            .cloned()
            .unwrap_or_else(Entry::dir);
        match entry {
            Entry::NotFound => Err(OpenRootError::NotFound),
            Entry::Fail(e) => Err(OpenRootError::Fs(e)),
            Entry::Dir {
                final_path,
                follow_final,
                is_dir,
                is_reparse,
                volume,
                serial,
            } => {
                let (fp, reparse) = match (mode, follow_final) {
                    (RootOpenMode::CanonicalizeFollow, Some(f)) => (w(&f), false),
                    _ => (
                        final_path.map_or_else(|| path.as_wide().to_vec(), |f| w(&f)),
                        is_reparse,
                    ),
                };
                Ok(RootProbe {
                    dir: (),
                    final_path: fp,
                    identity: FileIdentity {
                        volume_serial: serial,
                        file_id: [7; 16],
                    },
                    is_directory: is_dir,
                    is_reparse: reparse,
                    volume,
                })
            }
        }
    }

    fn open_subdir(&self, _: &(), _: &[u16]) -> Result<(), FsError> {
        Err(not_impl())
    }
    fn read_dir(
        &self,
        _: &(),
        _: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<IdQuality, FsError> {
        Err(not_impl())
    }
    fn open_item(&self, _: &(), _: &[u16], _: ItemKind) -> Result<(), FsError> {
        Err(not_impl())
    }
    fn query_item(&self, _: &()) -> Result<ItemQuery, FsError> {
        Err(not_impl())
    }
    fn final_path(&self, _: &()) -> Result<Vec<u16>, FsError> {
        Err(not_impl())
    }
    fn set_disposition(
        &self,
        _: &(),
        _: DispositionOpts,
    ) -> Result<DispositionKind, DispositionError> {
        Err(DispositionError::Fs(not_impl()))
    }
}

#[test]
fn fake_backend_rest_is_inert() {
    let fs = FakeFs::default();
    assert!(fs.open_subdir(&(), &w("x")).is_err());
    assert!(fs
        .read_dir(&(), &mut |_| ControlFlow::Continue(()))
        .is_err());
    assert!(fs.open_item(&(), &w("x"), ItemKind::File).is_err());
    assert!(fs.query_item(&()).is_err());
    assert!(fs.final_path(&()).is_err());
    assert!(fs
        .set_disposition(
            &(),
            DispositionOpts {
                ignore_readonly: false
            }
        )
        .is_err());
}

// ---------------------------------------------------------------------------------------------
// Fixtures

const LAYOUT: &[(KnownFolderId, &str)] = &[
    (KnownFolderId::Temp, r"C:\Users\u\AppData\Local\Temp"),
    (KnownFolderId::LocalAppData, r"C:\Users\u\AppData\Local"),
    (
        KnownFolderId::LocalAppDataLow,
        r"C:\Users\u\AppData\LocalLow",
    ),
    (KnownFolderId::RoamingAppData, r"C:\Users\u\AppData\Roaming"),
    (
        KnownFolderId::Recent,
        r"C:\Users\u\AppData\Roaming\Microsoft\Windows\Recent",
    ),
    (KnownFolderId::Profile, r"C:\Users\u"),
    (KnownFolderId::ProgramData, r"C:\ProgramData"),
    (KnownFolderId::Windows, r"C:\Windows"),
    (KnownFolderId::ProgramFiles, r"C:\Program Files"),
    (KnownFolderId::ProgramFilesX86, r"C:\Program Files (x86)"),
    (KnownFolderId::Desktop, r"C:\Users\u\Desktop"),
    (KnownFolderId::Documents, r"C:\Users\u\Documents"),
    (KnownFolderId::Downloads, r"C:\Users\u\Downloads"),
    (KnownFolderId::Pictures, r"C:\Users\u\Pictures"),
    (KnownFolderId::Music, r"C:\Users\u\Music"),
    (KnownFolderId::Videos, r"C:\Users\u\Videos"),
    (KnownFolderId::OneDrive, r"C:\Users\u\OneDrive"),
];

fn layout_map(skip: &[KnownFolderId]) -> BTreeMap<KnownFolderId, Vec<u16>> {
    LAYOUT
        .iter()
        .filter(|(id, _)| !skip.contains(id))
        .map(|&(id, v)| (id, w(v)))
        .collect()
}

fn known(privilege: Privilege) -> KnownFolders {
    KnownFolders::from_map(layout_map(&[]), privilege)
}

fn policy(fs: &FakeFs) -> SafetyPolicy {
    SafetyPolicy::new(fs, &known(Privilege::Unelevated), Privilege::Unelevated).unwrap()
}

fn cid() -> CleanerId {
    CleanerId::new("test.cleaner").unwrap()
}

fn root(spec: &str) -> ResolvedRoot {
    root_with(spec, &known(Privilege::Unelevated))
}

fn root_with(spec: &str, k: &KnownFolders) -> ResolvedRoot {
    resolve_root(&parse_root_spec(spec).unwrap(), k, cid(), 0).unwrap()
}

fn kind_of(r: Result<VerifiedRoot<()>, RootError>) -> ViolationKind {
    match r {
        Err(RootError::Violation(v)) => {
            assert!(!v.to_string().is_empty());
            v.kind
        }
        Err(e) => panic!("expected a violation, got {e:?}"),
        Ok(v) => panic!("expected a violation, got Ok({:?})", v.final_path()),
    }
}

fn scope_of(spec: &str) -> Scope {
    parse_root_spec(spec).unwrap().token.scope()
}

// ---------------------------------------------------------------------------------------------
// SafetyPolicy::new

#[test]
fn new_canonicalizes_every_base_and_protected_path_with_follow() {
    let fs = FakeFs::default();
    let p = policy(&fs);
    assert_eq!(p.allowed_bases().len(), BASE_TABLE.len());
    for (i, b) in p.allowed_bases().iter().enumerate() {
        assert_eq!(b.id, AllowedBaseId(u8::try_from(i).unwrap()));
        assert_eq!(b.family, BASE_TABLE[i].family);
        assert_eq!(b.allow, BASE_TABLE[i].allow);
        assert_eq!(b.scope, b.family.scope());
        assert_eq!(b.volume_serial, 1);
    }
    assert_eq!(p.protected_paths().len(), PROTECTED_TABLE.len());
    assert!(fs
        .modes()
        .iter()
        .all(|(_, m)| *m == RootOpenMode::CanonicalizeFollow));
    let labels: Vec<_> = p.protected_paths().iter().map(|p| p.label).collect();
    for want in [
        "Windows",
        "System32",
        "SysWOW64",
        "WinSxS",
        "Installer",
        "OneDrive",
    ] {
        assert!(labels.contains(&want), "{want}");
    }
    assert_eq!(p.privilege(), Privilege::Unelevated);
    assert_eq!(p.max_entries_per_target(), DEFAULT_MAX_ENTRIES_PER_TARGET);
}

#[test]
fn bases_that_cannot_be_verified_are_dropped() {
    let not_dir = Entry::Dir {
        final_path: None,
        follow_final: None,
        is_dir: false,
        is_reparse: false,
        volume: VolumeKind::LocalFixed,
        serial: 1,
    };
    let remote = Entry::Dir {
        final_path: None,
        follow_final: None,
        is_dir: true,
        is_reparse: false,
        volume: VolumeKind::Remote,
        serial: 1,
    };
    let to_drive_root = Entry::Dir {
        final_path: None,
        follow_final: Some(r"\\?\C:\".into()),
        is_dir: true,
        is_reparse: false,
        volume: VolumeKind::LocalFixed,
        serial: 1,
    };
    let to_unc = Entry::Dir {
        final_path: None,
        follow_final: Some(r"\\?\UNC\server\share\Temp".into()),
        is_dir: true,
        is_reparse: false,
        volume: VolumeKind::LocalFixed,
        serial: 1,
    };
    let fs = FakeFs::default()
        .with(r"C:\Windows\Minidump", Entry::NotFound)
        .with(r"C:\Windows\SystemTemp", Entry::Fail(fail()))
        .with(r"C:\Windows\SoftwareDistribution\Download", not_dir)
        .with(r"C:\ProgramData\Microsoft\Windows\WER", remote)
        .with(r"C:\Users\u\AppData\Local\Temp", to_drive_root)
        .with(
            r"C:\Users\u\AppData\Roaming\Microsoft\Windows\Recent",
            to_unc,
        );
    let p = policy(&fs);
    let fams: Vec<_> = p.allowed_bases().iter().map(|b| b.family).collect();
    assert_eq!(p.allowed_bases().len(), BASE_TABLE.len() - 6);
    assert!(!fams.contains(&RootToken::Temp));
    assert!(!fams.contains(&RootToken::Recent));
    assert!(!fams.contains(&RootToken::ProgramData));

    // A dropped %TEMP% base means %TEMP% roots are refused, never trusted.
    let r = p.verify_root(&fs, &root("%TEMP%"), Scope::User);
    assert_eq!(kind_of(r), ViolationKind::RootNotAllowlisted);
}

#[test]
fn no_bases_is_an_error() {
    let fs = FakeFs::default();
    let k = KnownFolders::from_map(BTreeMap::new(), Privilege::Unelevated);
    assert!(matches!(
        SafetyPolicy::new(&fs, &k, Privilege::Unelevated),
        Err(SafetyError::NoBases)
    ));
    // Bases resolve but none can be opened.
    let mut fs = FakeFs::default();
    for spec in BASE_TABLE {
        let mut p = known(Privilege::Unelevated)
            .get(spec.family.known_folder())
            .unwrap()
            .clone();
        for c in spec.rel {
            p.push_template_component(&w(c)).unwrap();
        }
        fs = fs.with(&p.display_string(), Entry::NotFound);
    }
    assert!(matches!(
        SafetyPolicy::new(&fs, &known(Privilege::Unelevated), Privilege::Unelevated),
        Err(SafetyError::NoBases)
    ));
}

#[test]
fn required_protected_folder_unresolved_is_an_error() {
    let fs = FakeFs::default();
    for id in [
        KnownFolderId::Windows,
        KnownFolderId::ProgramFiles,
        KnownFolderId::Profile,
        KnownFolderId::LocalAppData,
        KnownFolderId::RoamingAppData,
        KnownFolderId::ProgramData,
    ] {
        assert!(!is_optional_protected(id));
        let k = KnownFolders::from_map(layout_map(&[id]), Privilege::Unelevated);
        let r = SafetyPolicy::new(&fs, &k, Privilege::Unelevated);
        match r {
            Err(SafetyError::KnownFolder { id: got, .. }) => assert_eq!(got, id),
            other => panic!("{id:?}: {other:?}"),
        }
    }
    // A drive-root value is unresolved too, so it fails the same way.
    let mut m = layout_map(&[]);
    m.insert(KnownFolderId::Windows, w(r"C:\"));
    let k = KnownFolders::from_map(m, Privilege::Unelevated);
    let err = SafetyPolicy::new(&fs, &k, Privilege::Unelevated).unwrap_err();
    assert!(err.to_string().contains("Windows"), "{err}");
}

#[test]
fn optional_protected_folders_may_be_absent() {
    let fs = FakeFs::default();
    let optional = [
        KnownFolderId::OneDrive,
        KnownFolderId::Desktop,
        KnownFolderId::Documents,
        KnownFolderId::Downloads,
        KnownFolderId::Pictures,
        KnownFolderId::Music,
        KnownFolderId::Videos,
        KnownFolderId::ProgramFilesX86,
    ];
    let k = KnownFolders::from_map(layout_map(&optional), Privilege::Unelevated);
    let p = SafetyPolicy::new(&fs, &k, Privilege::Unelevated).unwrap();
    assert_eq!(
        p.protected_paths().len(),
        PROTECTED_TABLE.len() - optional.len()
    );
}

#[test]
fn elevated_worker_has_only_machine_bases() {
    let fs = FakeFs::default();
    let k = known(Privilege::ElevatedWorker);
    let p = SafetyPolicy::new(&fs, &k, Privilege::ElevatedWorker).unwrap();
    assert!(p.allowed_bases().iter().all(|b| b.scope == Scope::Machine));
    assert_eq!(p.allowed_bases().len(), 6);
    // User-scope protected folders are unavailable (ScopeError) and dropped, not fatal.
    assert!(p.protected_paths().iter().all(|pp| {
        let s = pp.canonical.display_string();
        !s.starts_with(r"C:\Users")
    }));
    // ElevatedWorker: user-scope roots are refused even if the cleaner claims User scope.
    let r = p.verify_root(&fs, &root_with(r"%WINDIR%\Temp", &k), Scope::Machine);
    assert!(r.is_ok());
    let user_root = root(r"%LOCALAPPDATA%\x");
    let r = p.verify_root(&fs, &user_root, Scope::User);
    assert_eq!(kind_of(r), ViolationKind::ScopeMismatch);
}

#[test]
fn protected_paths_keep_raw_and_followed_spellings() {
    let relocated_docs = Entry::Dir {
        final_path: None,
        follow_final: Some(r"\\?\D:\Docs".into()),
        is_dir: true,
        is_reparse: true,
        volume: VolumeKind::LocalFixed,
        serial: 2,
    };
    let redirected_unc = Entry::Dir {
        final_path: None,
        follow_final: Some(r"\\?\UNC\server\home\Desktop".into()),
        is_dir: true,
        is_reparse: true,
        volume: VolumeKind::Remote,
        serial: 3,
    };
    let fs = FakeFs::default()
        .with(r"C:\Users\u\Documents", relocated_docs)
        .with(r"C:\Users\u\Desktop", redirected_unc)
        .with(r"C:\Windows\Installer", Entry::NotFound);
    let p = policy(&fs);
    let shown: Vec<String> = p
        .protected_paths()
        .iter()
        .map(|pp| pp.canonical.display_string())
        .collect();
    assert!(shown.contains(&r"C:\Users\u\Documents".to_owned()));
    assert!(shown.contains(&r"D:\Docs".to_owned()));
    assert!(shown.contains(&r"C:\Users\u\Desktop".to_owned()));
    assert!(shown.contains(&r"C:\Windows\Installer".to_owned()));
    assert_eq!(p.protected_paths().len(), PROTECTED_TABLE.len() + 1);
}

#[test]
fn protected_path_that_cannot_be_opened_is_an_error() {
    let fs = FakeFs::default().with(r"C:\Windows\WinSxS", Entry::Fail(fail()));
    let r = SafetyPolicy::new(&fs, &known(Privilege::Unelevated), Privilege::Unelevated);
    match r {
        Err(e @ SafetyError::Canonicalize { .. }) => {
            assert!(e.to_string().contains("WinSxS"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tighten_only_setters() {
    let p = policy(&FakeFs::default());
    let p = p.with_max_entries(10).with_max_entries(1_000);
    assert_eq!(p.max_entries_per_target(), 10);
    let p = p
        .with_temp_floor(Duration::from_secs(60))
        .with_temp_floor(Duration::from_secs(48 * 3600));
    let t = Target::builder(parse_root_spec("%TEMP%").unwrap())
        .build()
        .unwrap();
    assert_eq!(p.effective_min_age(&t), Duration::from_secs(48 * 3600));
    let t = Target::builder(parse_root_spec(r"%LOCALAPPDATA%\x").unwrap())
        .min_age_hours(1)
        .build()
        .unwrap();
    assert_eq!(p.effective_min_age(&t), Duration::from_secs(3600));
}

// ---------------------------------------------------------------------------------------------
// verify_root

#[test]
fn verify_ok_and_accessors() {
    let fs = FakeFs::default();
    let p = policy(&fs);
    let v = p.verify_root(&fs, &root(r"%TEMP%"), Scope::User).unwrap();
    assert_eq!(
        v.final_path().display_string(),
        r"C:\Users\u\AppData\Local\Temp"
    );
    assert_eq!(v.token(), RootToken::Temp);
    assert_eq!(v.scope(), Scope::User);
    assert_eq!(v.identity().volume_serial, 1);
    assert_eq!(v.base(), AllowedBaseId(3));
    let _: &std::sync::Arc<()> = v.dir();
    // The root itself is opened without following reparse points.
    let modes = fs.modes();
    assert_eq!(
        modes.last(),
        Some(&(
            r"C:\Users\u\AppData\Local\Temp".to_owned(),
            RootOpenMode::VerifyNoFollow
        ))
    );

    for (spec, base) in [
        (r"%TEMP%\sub", 3),
        (r"%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Cache", 0),
        (r"%LOCALAPPDATALOW%\x", 1),
        (r"%APPDATA%\x\y", 2),
        ("%RECENT%", 4),
        (r"%WINDIR%\Temp", 5),
        (r"%WINDIR%\SoftwareDistribution\Download", 6),
        (r"%WINDIR%\Minidump", 7),
        (
            r"%WINDIR%\ServiceProfiles\LocalService\AppData\Local\FontCache",
            8,
        ),
        (r"%WINDIR%\SystemTemp\x", 9),
        (r"%PROGRAMDATA%\Microsoft\Windows\WER\ReportQueue", 10),
    ] {
        let v = p
            .verify_root(&fs, &root(spec), scope_of(spec))
            .unwrap_or_else(|e| panic!("{spec}: {e:?}"));
        assert_eq!(v.base(), AllowedBaseId(base), "{spec}");
    }
}

/// Q:31-42 policy-level cases: forbidden roots are violations even though they parse.
#[test]
fn q_policy_level_roots() {
    let fs = FakeFs::default();
    let p = policy(&fs);
    use ViolationKind as K;
    let cases: &[(&str, ViolationKind)] = &[
        ("%WINDIR%", K::RootNotAllowlisted),
        (r"%WINDIR%\System32", K::RootNotAllowlisted),
        (r"%WINDIR%\SysWOW64", K::RootNotAllowlisted),
        (r"%SYSTEMROOT%\WinSxS", K::RootNotAllowlisted),
        ("%USERPROFILE%", K::RootNotAllowlisted),
        (r"%USERPROFILE%\Documents", K::RootNotAllowlisted),
        ("%LOCALAPPDATA%", K::RootNotAllowlisted),
        ("%APPDATA%", K::RootNotAllowlisted),
        ("%LOCALAPPDATALOW%", K::RootNotAllowlisted),
        ("%PROGRAMDATA%", K::RootNotAllowlisted),
        (r"%PROGRAMDATA%\Microsoft", K::RootNotAllowlisted),
        (r"%PROGRAMDATA%\Microsoft\Windows", K::RootNotAllowlisted),
        // Same name under a different token family is not enough.
        (r"%WINDIR%\TempX", K::RootNotAllowlisted),
    ];
    for (spec, want) in cases {
        let r = p.verify_root(&fs, &root(spec), scope_of(spec));
        assert_eq!(&kind_of(r), want, "{spec}");
    }
}

#[test]
fn verify_step_failures() {
    use ViolationKind as K;
    let tmp = r"C:\Users\u\AppData\Local\Temp";
    let entry = |final_path: Option<&str>, is_dir, is_reparse, volume, serial| Entry::Dir {
        final_path: final_path.map(str::to_owned),
        follow_final: None,
        is_dir,
        is_reparse,
        volume,
        serial,
    };
    let lf = VolumeKind::LocalFixed;
    let cases: Vec<(&str, Entry, Result<ViolationKind, RootError>)> = vec![
        ("x", Entry::NotFound, Err(RootError::Missing)),
        ("x", Entry::Fail(fail()), Err(RootError::Failed(fail()))),
        (
            "x",
            entry(None, false, false, lf, 1),
            Ok(K::RootNotDirectory),
        ),
        (
            "x",
            entry(None, true, true, lf, 1),
            Ok(K::RootIsReparsePoint),
        ),
        (
            "x",
            entry(Some(r"\\?\UNC\server\share\x"), true, false, lf, 1),
            Ok(K::BadFinalPathForm(PathReject::UncFinal)),
        ),
        (
            "x",
            entry(Some(r"\\?\Volume{abc}\x"), true, false, lf, 1),
            Ok(K::BadFinalPathForm(PathReject::BadFinalPathForm)),
        ),
        (
            "x",
            entry(Some(r"C:\Users\u\AppData\Local\Temp\x"), true, false, lf, 1),
            Ok(K::BadFinalPathForm(PathReject::BadFinalPathForm)),
        ),
        (
            "x",
            entry(None, true, false, VolumeKind::Remote, 1),
            Ok(K::UncOrRemoteVolume),
        ),
        (
            "x",
            entry(None, true, false, VolumeKind::Removable, 1),
            Ok(K::UncOrRemoteVolume),
        ),
        (
            "x",
            entry(None, true, false, VolumeKind::Other, 1),
            Ok(K::UncOrRemoteVolume),
        ),
        // Same path text on another volume (serial) is not inside the base.
        (
            "x",
            entry(None, true, false, lf, 9),
            Ok(K::RootNotAllowlisted),
        ),
        // The final path escaped the base (e.g. an ancestor was swapped for a junction).
        (
            "x",
            entry(Some(r"\\?\C:\Windows\System32"), true, false, lf, 1),
            Ok(K::RootNotAllowlisted),
        ),
        (
            "x",
            entry(
                Some(r"\\?\C:\Users\u\AppData\Local\TempX"),
                true,
                false,
                lf,
                1,
            ),
            Ok(K::RootNotAllowlisted),
        ),
        (
            "x",
            entry(Some(r"\\?\C:\"), true, false, lf, 1),
            Ok(K::RootNotAllowlisted),
        ),
    ];
    for (sub, e, want) in cases {
        let path = format!(r"{tmp}\{sub}");
        let fs = FakeFs::default().with(&path, e);
        let p = policy(&fs);
        let r = p.verify_root(&fs, &root(&format!(r"%TEMP%\{sub}")), Scope::User);
        match want {
            Ok(kind) => assert_eq!(kind_of(r), kind, "{path}"),
            Err(err) => assert_eq!(r.err(), Some(err), "{path}"),
        }
    }
}

#[test]
fn scope_mismatch() {
    let fs = FakeFs::default();
    let p = policy(&fs);
    let r = p.verify_root(&fs, &root(r"%TEMP%\x"), Scope::Machine);
    assert_eq!(kind_of(r), ViolationKind::ScopeMismatch);
    let r = p.verify_root(&fs, &root(r"%WINDIR%\Temp"), Scope::User);
    assert_eq!(kind_of(r), ViolationKind::ScopeMismatch);
    // Nothing was opened for a scope violation.
    assert!(fs
        .modes()
        .iter()
        .all(|(_, m)| *m == RootOpenMode::CanonicalizeFollow));
}

#[test]
fn strictly_below_bases_reject_equality_in_any_casing() {
    // %LOCALAPPDATA% itself is not a valid root even when the OS reports other casing.
    let fs = FakeFs::default().with(
        r"C:\Users\u\AppData\Local",
        Entry::Dir {
            final_path: Some(r"\\?\C:\USERS\U\APPDATA\LOCAL".into()),
            follow_final: Some(r"\\?\C:\Users\u\AppData\Local".into()),
            is_dir: true,
            is_reparse: false,
            volume: VolumeKind::LocalFixed,
            serial: 1,
        },
    );
    let p = policy(&fs);
    let r = p.verify_root(&fs, &root("%LOCALAPPDATA%"), Scope::User);
    assert_eq!(kind_of(r), ViolationKind::RootNotAllowlisted);
}

#[test]
fn relocated_base_follows_to_the_real_location() {
    // %TEMP% lives on D: through a junction-ed ancestor; roots below it resolve to D:.
    let fs = FakeFs::default()
        .with(
            r"C:\Users\u\AppData\Local\Temp",
            Entry::Dir {
                final_path: Some(r"\\?\D:\Temp".into()),
                follow_final: Some(r"\\?\D:\Temp".into()),
                is_dir: true,
                is_reparse: false,
                volume: VolumeKind::LocalFixed,
                serial: 2,
            },
        )
        .with(
            r"C:\Users\u\AppData\Local\Temp\x",
            Entry::Dir {
                final_path: Some(r"\\?\D:\Temp\x".into()),
                follow_final: None,
                is_dir: true,
                is_reparse: false,
                volume: VolumeKind::LocalFixed,
                serial: 2,
            },
        );
    let p = policy(&fs);
    let v = p.verify_root(&fs, &root(r"%TEMP%\x"), Scope::User).unwrap();
    assert_eq!(v.final_path().display_string(), r"D:\Temp\x");
}

#[test]
fn drive_root_and_protected_checks() {
    // These are reachable only with a base that covers them, so build the policy by hand
    // (the constructor never admits a drive-root base).
    let fs = FakeFs::default();
    let real = policy(&fs);
    let wp = |s: &str| WidePath::from_final(&w(s)).unwrap();
    let drive_base = AllowedBase {
        id: AllowedBaseId(42),
        family: RootToken::Temp,
        scope: Scope::User,
        allow: AllowKind::EqualOrBelow,
        canonical: wp(r"\\?\C:\"),
        volume_serial: 1,
    };
    let p = SafetyPolicy {
        allowed_bases: vec![drive_base],
        ..real
    };
    let at = |final_path: &str| {
        FakeFs::default().with(
            r"C:\Users\u\AppData\Local\Temp",
            Entry::Dir {
                final_path: Some(final_path.into()),
                follow_final: None,
                is_dir: true,
                is_reparse: false,
                volume: VolumeKind::LocalFixed,
                serial: 1,
            },
        )
    };
    use ViolationKind as K;
    let cases: &[(&str, Option<ViolationKind>)] = &[
        (r"\\?\C:\", Some(K::RootIsDriveRoot)),
        (r"\\?\C:\Windows", Some(K::RootProtected)),
        (r"\\?\C:\WINDOWS", Some(K::RootProtected)),
        (r"\\?\c:\windows", Some(K::RootProtected)),
        (r"\\?\C:\WİNDOWS", Some(K::RootProtected)),
        (r"\\?\C:\Windows\System32", Some(K::RootProtected)),
        (r"\\?\C:\Windows\SYSWOW64", Some(K::RootProtected)),
        (r"\\?\C:\Program Files", Some(K::RootProtected)),
        (r"\\?\C:\Program Files (x86)", Some(K::RootProtected)),
        (r"\\?\C:\Users\u", Some(K::RootProtected)),
        (r"\\?\C:\Users\U\OneDrive", Some(K::RootProtected)),
        (r"\\?\C:\Users\u\AppData\Local", Some(K::RootProtected)),
        (r"\\?\C:\Users\u\AppData\Roaming", Some(K::RootProtected)),
        (r"\\?\C:\ProgramData", Some(K::RootProtected)),
        (r"\\?\C:\Users", Some(K::RootAncestorOfProtected)),
        (r"\\?\C:\Users\u\AppData", Some(K::RootAncestorOfProtected)),
        (r"\\?\C:\Users\\u", None), // malformed final path, see below
        (r"\\?\C:\Windows\Temp", None),
        (r"\\?\C:\Users\u\AppData\Local\Temp", None),
        (r"\\?\C:\Windows\System32x", None),
    ];
    for &(final_path, ref want) in cases {
        let fs = at(final_path);
        let r = p.verify_root(&fs, &root("%TEMP%"), Scope::User);
        if final_path.contains(r"\\u") {
            assert!(matches!(
                kind_of(r),
                K::BadFinalPathForm(PathReject::EmptyComponent)
            ));
            continue;
        }
        match want {
            Some(kind) => assert_eq!(&kind_of(r), kind, "{final_path}"),
            None => assert!(r.is_ok(), "{final_path}: {:?}", r.err()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// verdict

const DAY: u64 = 24 * 3600 * FileTime::TICKS_PER_SEC;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn now_ft() -> FileTime {
    FileTime::from_system_time(now())
}

fn facts<'a>(rel: &'a [u16], attrs: u32, age_ticks: u64) -> ItemFacts<'a> {
    let t = FileTime(now_ft().0 - age_ticks);
    ItemFacts {
        kind: ItemKind::File,
        rel_path: rel,
        attributes: attrs,
        reparse_tag: 0,
        size: 10,
        last_write: t,
        change_time: t,
        link_count: None,
        delete_pending: None,
    }
}

fn target(spec: &str) -> Target {
    Target::builder(parse_root_spec(spec).unwrap())
        .exclude(&["keep", "*.lock"])
        .build()
        .unwrap()
}

#[test]
fn verdict_rules() {
    let p = policy(&FakeFs::default());
    let t = target(r"%LOCALAPPDATA%\x");
    let rel = w("a.tmp");
    let old = 2 * DAY;
    let v = |f: &ItemFacts<'_>| p.verdict(f, &t, now());
    use SkipReason as R;

    assert_eq!(v(&facts(&rel, 0, old)), Verdict::Eligible);
    assert_eq!(v(&facts(&rel, attr::HIDDEN, old)), Verdict::Eligible);
    assert_eq!(
        v(&facts(&rel, attr::REPARSE_POINT, old)),
        Verdict::Skip(R::ReparsePoint)
    );
    let mut f = facts(&rel, 0, old);
    f.reparse_tag = 0xA000_000C;
    assert_eq!(v(&f), Verdict::Skip(R::ReparsePoint));
    for a in [
        attr::OFFLINE,
        attr::RECALL_ON_OPEN,
        attr::RECALL_ON_DATA_ACCESS,
    ] {
        assert_eq!(v(&facts(&rel, a, old)), Verdict::Skip(R::CloudPlaceholder));
    }
    assert_eq!(v(&facts(&w("x.lock"), 0, old)), Verdict::Skip(R::Excluded));
    assert_eq!(v(&facts(&w("keep/y"), 0, old)), Verdict::Skip(R::Excluded));
    assert_eq!(
        v(&facts(&rel, attr::SYSTEM, old)),
        Verdict::Skip(R::SystemFile)
    );
    assert_eq!(
        v(&facts(&rel, attr::READONLY, old)),
        Verdict::Skip(R::ReadOnly)
    );
    let mut f = facts(&rel, 0, old);
    f.link_count = Some(2);
    assert_eq!(v(&f), Verdict::Skip(R::HardLinked));
    f.link_count = Some(0);
    assert_eq!(v(&f), Verdict::Skip(R::HardLinked));
    f.link_count = Some(1);
    assert_eq!(v(&f), Verdict::Eligible);
    f.kind = ItemKind::Dir;
    f.link_count = Some(3);
    assert_eq!(v(&f), Verdict::Eligible, "link count only applies to files");
    let mut f = facts(&rel, 0, old);
    f.delete_pending = Some(true);
    assert_eq!(v(&f), Verdict::Skip(R::DeletePending));
    f.delete_pending = Some(false);
    assert_eq!(v(&f), Verdict::Eligible);

    // include_system / delete_readonly opt-ins.
    let t2 = Target::builder(parse_root_spec(r"%LOCALAPPDATA%\x").unwrap())
        .include_system(true)
        .delete_readonly(true)
        .build()
        .unwrap();
    let both = attr::SYSTEM | attr::READONLY;
    assert_eq!(
        p.verdict(&facts(&rel, both, old), &t2, now()),
        Verdict::Eligible
    );
}

#[test]
fn verdict_order_first_hit_wins() {
    let p = policy(&FakeFs::default());
    let t = target(r"%LOCALAPPDATA%\x");
    use SkipReason as R;
    let excluded = w("keep");
    let plain = w("a");
    let all = attr::REPARSE_POINT | attr::OFFLINE | attr::SYSTEM | attr::READONLY;
    let mk = |rel, attrs| {
        let mut f = facts(rel, attrs, 0); // also too new
        f.link_count = Some(5);
        f.delete_pending = Some(true);
        f
    };
    let chain: &[(&[u16], u32, R)] = &[
        (&excluded, all, R::ReparsePoint),
        (&excluded, all & !attr::REPARSE_POINT, R::CloudPlaceholder),
        (&excluded, attr::SYSTEM | attr::READONLY, R::Excluded),
        (&plain, attr::SYSTEM | attr::READONLY, R::SystemFile),
        (&plain, attr::READONLY, R::ReadOnly),
        (&plain, 0, R::HardLinked),
    ];
    for &(rel, attrs, want) in chain {
        assert_eq!(p.verdict(&mk(rel, attrs), &t, now()), Verdict::Skip(want));
    }
    let mut f = mk(&plain, 0);
    f.link_count = Some(1);
    assert_eq!(p.verdict(&f, &t, now()), Verdict::Skip(R::DeletePending));
    f.delete_pending = None;
    assert_eq!(p.verdict(&f, &t, now()), Verdict::Skip(R::TooNew));
}

/// Q:42 age boundaries: exactly 24 h, 24 h - 1 tick, future mtime (clock skew), change time
/// newer than last write.
#[test]
fn verdict_age_boundaries() {
    let p = policy(&FakeFs::default());
    let rel = w("a");
    use SkipReason as R;
    for spec in [r"%LOCALAPPDATA%\x", "%TEMP%"] {
        let t = Target::builder(parse_root_spec(spec).unwrap())
            .build()
            .unwrap();
        let v = |f: &ItemFacts<'_>| p.verdict(f, &t, now());
        assert_eq!(
            v(&facts(&rel, 0, DAY)),
            Verdict::Eligible,
            "{spec} exactly 24h"
        );
        assert_eq!(
            v(&facts(&rel, 0, DAY - 1)),
            Verdict::Skip(R::TooNew),
            "{spec} 24h-1tick"
        );
        assert_eq!(v(&facts(&rel, 0, DAY + 1)), Verdict::Eligible, "{spec}");
        assert_eq!(
            v(&facts(&rel, 0, 0)),
            Verdict::Skip(R::TooNew),
            "{spec} now"
        );

        // Future timestamps (clock skew) are too new, even far in the future.
        let mut f = facts(&rel, 0, DAY * 10);
        f.last_write = FileTime(now_ft().0 + 1);
        assert_eq!(v(&f), Verdict::Skip(R::TooNew));
        f.last_write = FileTime(u64::MAX);
        assert_eq!(v(&f), Verdict::Skip(R::TooNew));

        // The change time counts: an old write time with a recent metadata change is too new.
        let mut f = facts(&rel, 0, DAY * 10);
        f.change_time = FileTime(now_ft().0 - (DAY - 1));
        assert_eq!(v(&f), Verdict::Skip(R::TooNew));
        f.change_time = FileTime(now_ft().0 - DAY);
        assert_eq!(v(&f), Verdict::Eligible);
        // A future change time alone is enough.
        f.change_time = FileTime(now_ft().0 + DAY);
        assert_eq!(v(&f), Verdict::Skip(R::TooNew));

        // Unknown (zero) timestamps are treated as too new.
        let mut f = facts(&rel, 0, 0);
        f.last_write = FileTime(0);
        f.change_time = FileTime(0);
        assert_eq!(v(&f), Verdict::Skip(R::TooNew));
    }

    // min_age 0 on a non-temp root: anything not in the future is old enough.
    let t0 = Target::builder(parse_root_spec(r"%LOCALAPPDATA%\x").unwrap())
        .min_age_hours(0)
        .build()
        .unwrap();
    assert_eq!(p.verdict(&facts(&rel, 0, 0), &t0, now()), Verdict::Eligible);
    let mut f = facts(&rel, 0, 0);
    f.last_write = FileTime(now_ft().0 + 1);
    assert_eq!(p.verdict(&f, &t0, now()), Verdict::Skip(R::TooNew));

    // The temp floor applies even if a temp target was somehow built with less.
    let mut tt = Target::builder(parse_root_spec("%TEMP%").unwrap())
        .build()
        .unwrap();
    tt.min_age = Duration::ZERO;
    assert_eq!(p.effective_min_age(&tt), TEMP_MIN_AGE_FLOOR);
    assert_eq!(
        p.verdict(&facts(&rel, 0, DAY - 1), &tt, now()),
        Verdict::Skip(R::TooNew)
    );

    // A clock before 1601 makes everything too new.
    assert_eq!(
        p.verdict(
            &facts(&rel, 0, DAY),
            &t0,
            UNIX_EPOCH - Duration::from_secs(FileTime::UNIX_EPOCH_DIFF_SECS + 1)
        ),
        Verdict::Skip(R::TooNew)
    );
}

#[test]
fn facts_constructors() {
    let rel = w("a/b");
    let e = RawEntry {
        name: &rel,
        attributes: attr::HIDDEN,
        reparse_tag: 3,
        size: 5,
        allocated: 8,
        creation: FileTime(1),
        last_write: FileTime(2),
        change: FileTime(3),
        file_id: None,
    };
    let f = ItemFacts::from_raw(&e, &rel, ItemKind::File);
    assert_eq!(f.link_count, None);
    assert_eq!(f.delete_pending, None);
    assert_eq!(f.change_time, FileTime(3));
    let q = query();
    let f = ItemFacts::from_query(&q, &rel);
    assert_eq!(f.kind, ItemKind::File);
    assert_eq!(f.link_count, Some(1));
    assert_eq!(f.delete_pending, Some(false));
    let f = ItemFacts::from_query(
        &ItemQuery {
            is_directory: true,
            ..q
        },
        &rel,
    );
    assert_eq!(f.kind, ItemKind::Dir);
}

fn query() -> ItemQuery {
    ItemQuery {
        attributes: 0,
        reparse_tag: 0,
        identity: FileIdentity {
            volume_serial: 1,
            file_id: [1; 16],
        },
        link_count: 1,
        delete_pending: false,
        is_directory: false,
        size: 10,
        allocated: 16,
        creation: FileTime(1),
        last_write: FileTime(100),
        change: FileTime(100),
    }
}

#[test]
fn check_against_plan_order() {
    let p = policy(&FakeFs::default());
    let plan = PlanEntry {
        dir: DirIndex::ROOT,
        name: w("a").into_boxed_slice(),
        identity: query().identity,
        size: 10,
        last_write: FileTime(100),
    };
    let changed = Some(SkipReason::ChangedSinceScan);
    let q = query();
    for idq in [IdQuality::Full128, IdQuality::Id64, IdQuality::None] {
        assert_eq!(p.check_against_plan(&q, &plan, idq), None);
    }
    let other_id = ItemQuery {
        identity: FileIdentity {
            volume_serial: 1,
            file_id: [2; 16],
        },
        ..q
    };
    assert_eq!(
        p.check_against_plan(&other_id, &plan, IdQuality::Full128),
        changed
    );
    assert_eq!(
        p.check_against_plan(&other_id, &plan, IdQuality::Id64),
        changed
    );
    // Without ids the identity is not compared, but size and write time still are.
    assert_eq!(
        p.check_against_plan(&other_id, &plan, IdQuality::None),
        None
    );
    let other_vol = ItemQuery {
        identity: FileIdentity {
            volume_serial: 2,
            file_id: [1; 16],
        },
        ..q
    };
    assert_eq!(
        p.check_against_plan(&other_vol, &plan, IdQuality::Full128),
        changed
    );
    let bigger = ItemQuery { size: 11, ..q };
    assert_eq!(
        p.check_against_plan(&bigger, &plan, IdQuality::None),
        changed
    );
    let touched = ItemQuery {
        last_write: FileTime(101),
        ..q
    };
    assert_eq!(
        p.check_against_plan(&touched, &plan, IdQuality::None),
        changed
    );
    // Change time alone is not a plan mismatch (the verdict's age check covers it).
    let chg = ItemQuery {
        change: FileTime(999),
        ..q
    };
    assert_eq!(p.check_against_plan(&chg, &plan, IdQuality::Full128), None);
    let dir = ItemQuery {
        is_directory: true,
        ..q
    };
    assert_eq!(p.check_against_plan(&dir, &plan, IdQuality::None), changed);
}

#[test]
fn violation_display() {
    let v = SafetyViolation {
        kind: ViolationKind::RootProtected,
        path: r"C:\Windows".into(),
        detail: String::new(),
    };
    assert_eq!(
        v.to_string(),
        r"safety violation RootProtected at C:\Windows"
    );
    let v = SafetyViolation {
        detail: "Windows".into(),
        ..v
    };
    assert!(v.to_string().ends_with(": Windows"));
    let e: &dyn std::error::Error = &v;
    assert!(e.source().is_none());
    let _ = KnownFolderError::ScopeError.to_string();
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Adding exclude patterns never turns a skip into Eligible (globs only narrow).
        #[test]
        fn more_excludes_never_widen(
            attrs in prop::sample::select(vec![0u32, attr::SYSTEM, attr::READONLY, attr::REPARSE_POINT, attr::OFFLINE]),
            age_h in 0u64..72,
            rel in "[ab]{1,2}(/[ab]{1,2}){0,2}",
            extra in prop::sample::subsequence(vec!["a", "b", "*", "a/**", "**/b", "[ab]b"], 0..4),
        ) {
            let p = policy(&FakeFs::default());
            let base = Target::builder(parse_root_spec(r"%LOCALAPPDATA%\x").unwrap())
                .build()
                .unwrap();
            let more = Target::builder(parse_root_spec(r"%LOCALAPPDATA%\x").unwrap())
                .exclude(&extra)
                .build()
                .unwrap();
            let rel = w(&rel);
            let f = facts(&rel, attrs, age_h * 3600 * FileTime::TICKS_PER_SEC);
            if p.verdict(&f, &more, now()) == Verdict::Eligible {
                prop_assert_eq!(p.verdict(&f, &base, now()), Verdict::Eligible);
            }
        }

        /// A verified root is always inside some allowlisted base of its own family and
        /// never a drive root, protected path or ancestor of one.
        #[test]
        fn verified_roots_are_contained(
            spec in prop::sample::select(vec![
                "%TEMP%", r"%TEMP%\a", r"%LOCALAPPDATA%", r"%LOCALAPPDATA%\a", "%APPDATA%",
                "%WINDIR%", r"%WINDIR%\Temp", r"%WINDIR%\System32", "%USERPROFILE%",
                r"%USERPROFILE%\AppData", r"%PROGRAMDATA%\Microsoft\Windows\WER", "%RECENT%",
            ]),
            final_path in prop::sample::select(vec![
                None, Some(r"\\?\C:\"), Some(r"\\?\C:\Windows"), Some(r"\\?\C:\Users\u"),
                Some(r"\\?\C:\Users\u\AppData\Local\Temp\a"), Some(r"\\?\D:\x"),
                Some(r"\\?\C:\Users\u\AppData\Local\a"),
            ]),
        ) {
            let r = root(spec);
            let fs = FakeFs::default().with(
                &r.display.display_string(),
                Entry::Dir {
                    final_path: final_path.map(str::to_owned),
                    follow_final: None,
                    is_dir: true,
                    is_reparse: false,
                    volume: VolumeKind::LocalFixed,
                    serial: 1,
                },
            );
            let p = policy(&fs);
            if let Ok(v) = p.verify_root(&fs, &r, r.token.scope()) {
                let fp = v.final_path();
                prop_assert!(!fp.is_drive_root());
                prop_assert!(p.allowed_bases().iter().any(|b| b.family == r.token
                    && b.id == v.base()
                    && fp.is_same_or_below(&b.canonical, crate::path::Cmp::Strict)));
                for pp in p.protected_paths() {
                    prop_assert!(!pp.canonical.is_same_or_below(fp, crate::path::Cmp::Loose));
                }
            }
        }
    }
}
