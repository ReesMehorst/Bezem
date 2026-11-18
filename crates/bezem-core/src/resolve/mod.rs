// SPDX-License-Identifier: MIT

//! Known-folder resolution (M1 design section 2.6; ARCHITECTURE A:713-737).
//!
//! Ownership: work package **B** (pure logic); work package **A** provides the Windows
//! [`KnownFolderSource`] (`win32::SystemKnownFolders`). Values come only from OS APIs; the
//! environment is never read for decisions. Paths are built by pushing validated components,
//! never by string concatenation.

use std::collections::BTreeMap;

use crate::path::{PathReject, RootSpec, RootToken, WidePath};
use crate::types::{CleanerId, Privilege, Scope};

#[cfg(any(test, feature = "test-support"))]
mod sandbox;

/// The known folders M1 resolves (roots, allowlisted bases and protected paths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KnownFolderId {
    Temp,
    LocalAppData,
    LocalAppDataLow,
    RoamingAppData,
    Recent,
    Profile,
    ProgramData,
    Windows,
    ProgramFiles,
    ProgramFilesX86,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    OneDrive,
}

impl KnownFolderId {
    /// Every id, in `Ord` order.
    pub const ALL: [KnownFolderId; 17] = [
        KnownFolderId::Temp,
        KnownFolderId::LocalAppData,
        KnownFolderId::LocalAppDataLow,
        KnownFolderId::RoamingAppData,
        KnownFolderId::Recent,
        KnownFolderId::Profile,
        KnownFolderId::ProgramData,
        KnownFolderId::Windows,
        KnownFolderId::ProgramFiles,
        KnownFolderId::ProgramFilesX86,
        KnownFolderId::Desktop,
        KnownFolderId::Documents,
        KnownFolderId::Downloads,
        KnownFolderId::Pictures,
        KnownFolderId::Music,
        KnownFolderId::Videos,
        KnownFolderId::OneDrive,
    ];

    /// `Windows`, `ProgramData`, `ProgramFiles` and `ProgramFilesX86` are `Machine`; the rest
    /// are `User`.
    pub fn scope(self) -> Scope {
        match self {
            KnownFolderId::Windows
            | KnownFolderId::ProgramData
            | KnownFolderId::ProgramFiles
            | KnownFolderId::ProgramFilesX86 => Scope::Machine,
            KnownFolderId::Temp
            | KnownFolderId::LocalAppData
            | KnownFolderId::LocalAppDataLow
            | KnownFolderId::RoamingAppData
            | KnownFolderId::Recent
            | KnownFolderId::Profile
            | KnownFolderId::Desktop
            | KnownFolderId::Documents
            | KnownFolderId::Downloads
            | KnownFolderId::Pictures
            | KnownFolderId::Music
            | KnownFolderId::Videos
            | KnownFolderId::OneDrive => Scope::User,
        }
    }
}

/// Why a known folder did not resolve. Such tokens make their targets Unresolved: reported,
/// never guessed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnownFolderError {
    #[error("API failed: {0}")]
    Failed(String),
    #[error("empty value")]
    Empty,
    #[error("relative path")]
    Relative,
    #[error("value too short")]
    TooShort,
    #[error("drive root")]
    DriveRoot,
    #[error("UNC path")]
    Unc,
    #[error("invalid path: {0}")]
    Invalid(PathReject),
    #[error("user-scope folder requested in the elevated worker")]
    ScopeError,
}

/// Supplies raw known-folder values from the OS (or a test).
pub trait KnownFolderSource {
    /// The raw API value (no trailing NUL), not yet validated.
    fn get(&self, id: KnownFolderId) -> Result<Vec<u16>, KnownFolderError>;
}

/// Every known folder, resolved once and then immutable.
#[derive(Debug, Clone)]
pub struct KnownFolders {
    map: BTreeMap<KnownFolderId, Result<WidePath, KnownFolderError>>,
}

/// Returned by [`KnownFolders::get`] for an id the map does not contain.
static NOT_RESOLVED: KnownFolderError = KnownFolderError::Failed(String::new());

impl KnownFolders {
    /// Resolves every id from `src`; each value goes through `WidePath::from_known_value`.
    /// User ids under `Privilege::ElevatedWorker` resolve to `Err(ScopeError)`.
    pub fn resolve(src: &dyn KnownFolderSource, privilege: Privilege) -> Self {
        Self::resolve_with(|id| src.get(id), privilege)
    }

    fn resolve_with(
        get: impl Fn(KnownFolderId) -> Result<Vec<u16>, KnownFolderError>,
        privilege: Privilege,
    ) -> Self {
        let map = KnownFolderId::ALL
            .iter()
            .map(|&id| {
                let value = if privilege == Privilege::ElevatedWorker && id.scope() == Scope::User {
                    // Never even ask the OS: the elevated worker has no user-scope folders.
                    Err(KnownFolderError::ScopeError)
                } else {
                    get(id).and_then(|raw| validate_known_value(&raw))
                };
                (id, value)
            })
            .collect();
        KnownFolders { map }
    }

    /// Like [`Self::resolve`] from a fixed map (pure tests and fuzzing; non-production callers
    /// only). Ids missing from `m` resolve to `Err(Failed)`.
    pub fn from_map(m: BTreeMap<KnownFolderId, Vec<u16>>, privilege: Privilege) -> Self {
        Self::resolve_with(
            |id| {
                m.get(&id)
                    .cloned()
                    .ok_or_else(|| KnownFolderError::Failed(format!("{id:?} not in map")))
            },
            privilege,
        )
    }

    /// The resolved path for `id`, or why it did not resolve.
    pub fn get(&self, id: KnownFolderId) -> Result<&WidePath, &KnownFolderError> {
        match self.map.get(&id) {
            Some(Ok(p)) => Ok(p),
            Some(Err(e)) => Err(e),
            None => Err(&NOT_RESOLVED),
        }
    }

    /// `resolve(&win32::SystemKnownFolders, privilege)`.
    #[cfg(windows)]
    pub fn from_system(privilege: Privilege) -> Self {
        Self::resolve(&crate::win32::SystemKnownFolders, privilege)
    }
}

/// Classifies and validates a raw known-folder value (ARCHITECTURE 7.1): empty, shorter than
/// 4 units, relative, UNC, drive root or otherwise invalid values never resolve.
fn validate_known_value(raw: &[u16]) -> Result<WidePath, KnownFolderError> {
    if raw.is_empty() {
        return Err(KnownFolderError::Empty);
    }
    if raw.len() < 4 {
        return Err(KnownFolderError::TooShort);
    }
    WidePath::from_known_value(raw).map_err(|e| match e {
        PathReject::Empty => KnownFolderError::Empty,
        PathReject::Unc | PathReject::UncFinal => KnownFolderError::Unc,
        PathReject::DriveRoot => KnownFolderError::DriveRoot,
        PathReject::NoLeadingToken | PathReject::RootedNoDrive | PathReject::DriveRelative => {
            KnownFolderError::Relative
        }
        other => KnownFolderError::Invalid(other),
    })
}

/// A root resolved from a [`RootSpec`]: still a display path, not yet verified. Only
/// `SafetyPolicy::verify_root` turns it into a `VerifiedRoot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub cleaner: CleanerId,
    pub target_index: usize,
    pub token: RootToken,
    pub display: WidePath,
}

/// Why a root spec did not resolve.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("unresolved {token:?}: {source}")]
    Unresolved {
        token: RootToken,
        source: KnownFolderError,
    },
    #[error("component: {0}")]
    Component(#[from] PathReject),
}

/// `known.get(spec.token.known_folder())`, then `push_template_component` for each spec
/// component. No string concatenation.
pub fn resolve_root(
    spec: &RootSpec,
    known: &KnownFolders,
    cleaner: CleanerId,
    target_index: usize,
) -> Result<ResolvedRoot, ResolveError> {
    let token = spec.token;
    let mut display = known
        .get(token.known_folder())
        .map_err(|e| ResolveError::Unresolved {
            token,
            source: e.clone(),
        })?
        .clone();
    for c in &spec.components {
        display.push_template_component(c)?;
    }
    Ok(ResolvedRoot {
        cleaner,
        target_index,
        token,
        display,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::parse_root_spec;
    use std::cell::Cell;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn cid() -> CleanerId {
        CleanerId::new("test.cleaner").unwrap()
    }

    fn map(pairs: &[(KnownFolderId, &str)]) -> BTreeMap<KnownFolderId, Vec<u16>> {
        pairs.iter().map(|&(id, v)| (id, w(v))).collect()
    }

    #[test]
    fn known_value_classification() {
        use KnownFolderError as E;
        let cases: &[(&str, Result<&str, KnownFolderError>)] = &[
            (
                r"C:\Users\u\AppData\Local\Temp",
                Ok(r"\\?\C:\Users\u\AppData\Local\Temp"),
            ),
            (
                r"C:\Users\RUNNER~1\AppData\Local\Temp\",
                Ok(r"\\?\C:\Users\RUNNER~1\AppData\Local\Temp"),
            ),
            ("", Err(E::Empty)),
            ("C:", Err(E::TooShort)),
            (r"C:\", Err(E::TooShort)),
            (r"\x", Err(E::TooShort)),
            (r"C:\\", Err(E::DriveRoot)),
            (r"\\?\C:\", Err(E::DriveRoot)),
            (r"\\server\share", Err(E::Unc)),
            (r"\\?\UNC\server\share", Err(E::Unc)),
            (r"\Temp", Err(E::Relative)),
            ("Temp", Err(E::Relative)),
            ("C:Temp", Err(E::Relative)),
            (r"C:\a\..\b", Err(E::Invalid(PathReject::ParentTraversal))),
            (
                r"\\.\PhysicalDrive0",
                Err(E::Invalid(PathReject::DevicePrefix)),
            ),
        ];
        for (raw, want) in cases {
            let got = validate_known_value(&w(raw));
            match want {
                Ok(p) => assert_eq!(got.unwrap().as_wide(), &w(p)[..], "{raw:?}"),
                Err(e) => assert_eq!(got, Err(e.clone()), "{raw:?}"),
            }
        }
    }

    #[test]
    fn from_map_and_scope() {
        let m = map(&[
            (KnownFolderId::Temp, r"C:\Users\u\AppData\Local\Temp"),
            (KnownFolderId::Windows, r"C:\Windows"),
            (KnownFolderId::ProgramData, ""),
        ]);
        let k = KnownFolders::from_map(m.clone(), Privilege::Unelevated);
        assert!(k.get(KnownFolderId::Temp).is_ok());
        assert!(k.get(KnownFolderId::Windows).is_ok());
        assert_eq!(
            k.get(KnownFolderId::ProgramData),
            Err(&KnownFolderError::Empty)
        );
        assert!(matches!(
            k.get(KnownFolderId::Videos),
            Err(KnownFolderError::Failed(_))
        ));

        // The elevated worker never resolves user-scope folders.
        let k = KnownFolders::from_map(m, Privilege::ElevatedWorker);
        assert_eq!(
            k.get(KnownFolderId::Temp),
            Err(&KnownFolderError::ScopeError)
        );
        assert!(k.get(KnownFolderId::Windows).is_ok());
        for id in KnownFolderId::ALL {
            if id.scope() == Scope::User {
                assert_eq!(k.get(id), Err(&KnownFolderError::ScopeError), "{id:?}");
            }
        }
    }

    struct CountingSource(Cell<u32>);
    impl KnownFolderSource for CountingSource {
        fn get(&self, id: KnownFolderId) -> Result<Vec<u16>, KnownFolderError> {
            self.0.set(self.0.get() + 1);
            match id {
                KnownFolderId::Windows => Ok(w(r"C:\Windows")),
                KnownFolderId::OneDrive => Err(KnownFolderError::Failed("E_FAIL".into())),
                _ => Ok(w(r"C:\Users\u")),
            }
        }
    }

    #[test]
    fn resolve_from_source() {
        let src = CountingSource(Cell::new(0));
        let k = KnownFolders::resolve(&src, Privilege::Unelevated);
        assert_eq!(src.0.get(), 17);
        assert!(k.get(KnownFolderId::Windows).is_ok());
        assert!(matches!(
            k.get(KnownFolderId::OneDrive),
            Err(KnownFolderError::Failed(_))
        ));
        // Elevated worker: the source is only asked for machine-scope folders.
        let src = CountingSource(Cell::new(0));
        let _ = KnownFolders::resolve(&src, Privilege::ElevatedWorker);
        assert_eq!(src.0.get(), 4);
    }

    #[test]
    fn resolve_root_pushes_components() {
        let k = KnownFolders::from_map(
            map(&[(KnownFolderId::LocalAppData, r"C:\Users\u\AppData\Local")]),
            Privilege::Unelevated,
        );
        let spec = parse_root_spec(r"%LOCALAPPDATA%\Microsoft\Edge\User Data").unwrap();
        let r = resolve_root(&spec, &k, cid(), 3).unwrap();
        assert_eq!(
            r.display.display_string(),
            r"C:\Users\u\AppData\Local\Microsoft\Edge\User Data"
        );
        assert_eq!(r.token, RootToken::LocalAppData);
        assert_eq!(r.target_index, 3);
        assert_eq!(r.cleaner, cid());
    }

    /// Q:31-42: a token whose folder is empty / a drive root / UNC / relative / missing is
    /// Unresolved, never guessed (the classic `%X%\Temp` -> `\Temp` bug).
    #[test]
    fn unresolved_tokens() {
        let bad = [
            "",
            r"C:\",
            r"\\server\share",
            "relative",
            r"\Temp",
            r"\\?\UNC\s\x",
        ];
        for v in bad {
            let k = KnownFolders::from_map(map(&[(KnownFolderId::Temp, v)]), Privilege::Unelevated);
            let spec = parse_root_spec(r"%TEMP%\x").unwrap();
            let r = resolve_root(&spec, &k, cid(), 0);
            assert!(
                matches!(
                    r,
                    Err(ResolveError::Unresolved {
                        token: RootToken::Temp,
                        ..
                    })
                ),
                "{v:?}: {r:?}"
            );
        }
        // Missing from the source entirely.
        let k = KnownFolders::from_map(BTreeMap::new(), Privilege::Unelevated);
        let spec = parse_root_spec("%WINDIR%").unwrap();
        assert!(matches!(
            resolve_root(&spec, &k, cid(), 0),
            Err(ResolveError::Unresolved {
                token: RootToken::WinDir,
                ..
            })
        ));
        // A component that does not fit any more is a component error, not a truncation.
        let long = format!(r"C:\{}", vec!["a".repeat(250); 127].join("\\"));
        let k = KnownFolders::from_map(map(&[(KnownFolderId::Temp, &long)]), Privilege::Unelevated);
        let spec = parse_root_spec(&format!(r"%TEMP%\{}", "b".repeat(200))).unwrap();
        assert!(matches!(
            resolve_root(&spec, &k, cid(), 0),
            Err(ResolveError::Component(PathReject::PathTooLong))
        ));
    }
}
