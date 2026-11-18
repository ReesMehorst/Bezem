// SPDX-License-Identifier: MIT

//! `KnownFolders::sandbox`: a fake known-folder layout under a test directory (M1 design
//! section 4.3). Only compiled with `cfg(test)` or the `test-support` feature, which shipped
//! crates must never enable.
//!
//! Ownership: work package **D**.
//!
//! The layout is one table ([`FOLDERS`] plus [`EXTRA_DIRS`]) used three ways:
//! - [`KnownFolders::sandbox`]: the paths only (nothing is created);
//! - [`KnownFolders::create_sandbox_dirs`]: creates the directories on a real temp directory;
//! - [`MemFs::add_sandbox_layout`]: creates the same directories in a `MemFs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use super::{KnownFolderError, KnownFolderId, KnownFolders};
use crate::fs::mem::MemFs;
use crate::path::WidePath;

/// Every known folder and its path relative to the sandbox root (`\` separated).
const FOLDERS: [(KnownFolderId, &str); 17] = [
    (KnownFolderId::Profile, r"Users\u"),
    (KnownFolderId::LocalAppData, r"Users\u\AppData\Local"),
    (KnownFolderId::Temp, r"Users\u\AppData\Local\Temp"),
    (KnownFolderId::LocalAppDataLow, r"Users\u\AppData\LocalLow"),
    (KnownFolderId::RoamingAppData, r"Users\u\AppData\Roaming"),
    (
        KnownFolderId::Recent,
        r"Users\u\AppData\Roaming\Microsoft\Windows\Recent",
    ),
    (KnownFolderId::Desktop, r"Users\u\Desktop"),
    (KnownFolderId::Documents, r"Users\u\Documents"),
    (KnownFolderId::Downloads, r"Users\u\Downloads"),
    (KnownFolderId::Pictures, r"Users\u\Pictures"),
    (KnownFolderId::Music, r"Users\u\Music"),
    (KnownFolderId::Videos, r"Users\u\Videos"),
    (KnownFolderId::OneDrive, r"Users\u\OneDrive"),
    (KnownFolderId::ProgramData, "ProgramData"),
    (KnownFolderId::Windows, "Windows"),
    (KnownFolderId::ProgramFiles, "Program Files"),
    (KnownFolderId::ProgramFilesX86, "Program Files (x86)"),
];

/// Further directories of the layout: the machine-scope allowlisted bases and the protected
/// system folders the policy canonicalizes.
const EXTRA_DIRS: [&str; 10] = [
    r"Windows\Temp",
    r"Windows\SystemTemp",
    r"Windows\SoftwareDistribution\Download",
    r"Windows\Minidump",
    r"Windows\ServiceProfiles\LocalService\AppData\Local\FontCache",
    r"Windows\System32",
    r"Windows\SysWOW64",
    r"Windows\WinSxS",
    r"Windows\Installer",
    r"ProgramData\Microsoft\Windows\WER",
];

fn all_relative_dirs() -> impl Iterator<Item = &'static str> {
    FOLDERS
        .iter()
        .map(|&(_, rel)| rel)
        .chain(EXTRA_DIRS.iter().copied())
}

impl KnownFolders {
    /// Every known folder placed under `root`: `Users\u` (Profile), `Users\u\AppData\Local`,
    /// `...\Local\Temp`, `...\LocalLow`, `Users\u\AppData\Roaming`, `...\Microsoft\Windows\Recent`,
    /// `Users\u\{Desktop,Documents,Downloads,Pictures,Music,Videos,OneDrive}`, `ProgramData`,
    /// `Windows`, `Program Files`, `Program Files (x86)`. Paths only: nothing is created (see
    /// [`KnownFolders::create_sandbox_dirs`] and [`MemFs::add_sandbox_layout`]). Values go
    /// through `WidePath::from_known_value`, so an invalid `root` yields `Err(Invalid)` entries.
    pub fn sandbox(root: &WidePath) -> Self {
        let mut base = root.as_wide().to_vec();
        if base.last() != Some(&u16::from(b'\\')) {
            base.push(u16::from(b'\\'));
        }
        let mut map = BTreeMap::new();
        for (id, rel) in FOLDERS {
            let mut raw = base.clone();
            raw.extend(rel.encode_utf16());
            map.insert(
                id,
                WidePath::from_known_value(&raw).map_err(KnownFolderError::Invalid),
            );
        }
        KnownFolders { map }
    }

    /// The sandbox-relative path (`Users\u\AppData\Local\Temp`) of a known folder.
    pub fn sandbox_relative(id: KnownFolderId) -> &'static str {
        FOLDERS
            .iter()
            .find(|&&(i, _)| i == id)
            .map_or("", |&(_, rel)| rel)
    }

    /// Creates the whole layout (known folders plus the allowlisted-base and protected
    /// directories) below the existing directory `root`.
    pub fn create_sandbox_dirs(root: &Path) -> io::Result<()> {
        for rel in all_relative_dirs() {
            let mut p = root.to_path_buf();
            p.extend(rel.split('\\'));
            std::fs::create_dir_all(p)?;
        }
        Ok(())
    }
}

impl MemFs {
    /// Adds the sandbox layout below `root` (`C:\sandbox`, no trailing `\` needed). Creates
    /// the same directories as [`KnownFolders::create_sandbox_dirs`].
    pub fn add_sandbox_layout(&self, root: &str) {
        let root = root.trim_end_matches('\\');
        self.add_dir(&format!("{root}\\"));
        for rel in all_relative_dirs() {
            self.add_dir(&format!(r"{root}\{rel}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fs::RootOpenMode;

    #[test]
    fn layout_covers_every_known_folder_exactly_once() {
        let ids: BTreeSet<_> = FOLDERS.iter().map(|&(i, _)| i).collect();
        assert_eq!(ids.len(), FOLDERS.len());
        assert_eq!(ids, KnownFolderId::ALL.into_iter().collect());
        let rels: BTreeSet<_> = FOLDERS
            .iter()
            .map(|&(_, r)| r.to_ascii_lowercase())
            .collect();
        assert_eq!(
            rels.len(),
            FOLDERS.len(),
            "no two folders share a directory"
        );
    }

    #[test]
    fn layout_paths_are_relative_clean_and_match_the_design() {
        for rel in all_relative_dirs() {
            assert!(!rel.starts_with('\\') && !rel.ends_with('\\'), "{rel}");
            assert!(rel
                .split('\\')
                .all(|c| !c.is_empty() && c != "." && c != ".."));
        }
        let rel = KnownFolders::sandbox_relative;
        assert_eq!(rel(KnownFolderId::Profile), r"Users\u");
        assert_eq!(rel(KnownFolderId::Temp), r"Users\u\AppData\Local\Temp");
        assert_eq!(rel(KnownFolderId::ProgramFilesX86), "Program Files (x86)");
        // Temp is below LocalAppData is below Profile; Recent is below Roaming.
        let below =
            |a: KnownFolderId, b: KnownFolderId| rel(a).starts_with(&format!("{}\\", rel(b)));
        assert!(below(KnownFolderId::Temp, KnownFolderId::LocalAppData));
        assert!(below(KnownFolderId::LocalAppData, KnownFolderId::Profile));
        assert!(below(KnownFolderId::Recent, KnownFolderId::RoamingAppData));
        // Machine folders are siblings of Users, not below the profile.
        assert!(!below(KnownFolderId::Windows, KnownFolderId::Profile));
        assert!(!below(KnownFolderId::ProgramData, KnownFolderId::Profile));
    }

    #[test]
    fn creates_the_layout_on_a_real_temp_dir_and_nowhere_else() {
        let tmp = tempfile::tempdir().unwrap();
        let sentinel = tmp.path().join("outside");
        std::fs::create_dir(&sentinel).unwrap();
        let root = tmp.path().join("sandbox");
        std::fs::create_dir(&root).unwrap();
        KnownFolders::create_sandbox_dirs(&root).unwrap();
        for rel in all_relative_dirs() {
            let mut p = root.clone();
            p.extend(rel.split('\\'));
            assert!(p.is_dir(), "{p:?}");
        }
        // Calling it twice is fine, and nothing appeared beside the sandbox root.
        KnownFolders::create_sandbox_dirs(&root).unwrap();
        let top: BTreeSet<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            top,
            BTreeSet::from(["outside".to_owned(), "sandbox".to_owned()])
        );
        assert_eq!(std::fs::read_dir(&sentinel).unwrap().count(), 0);
    }

    #[test]
    fn creates_the_same_layout_in_memfs() {
        let fs = MemFs::new();
        fs.add_sandbox_layout(r"C:\bezem-sandbox");
        for rel in all_relative_dirs() {
            assert!(fs.exists(&format!(r"C:\bezem-sandbox\{rel}")), "{rel}");
        }
        // Nothing outside the sandbox root.
        let outside: Vec<_> = fs
            .snapshot()
            .into_keys()
            .filter(|k| k != r"C:\" && !k.starts_with(r"C:\bezem-sandbox"))
            .collect();
        assert!(outside.is_empty(), "{outside:?}");
        // A root with a trailing separator and a drive root both work.
        let fs2 = MemFs::new();
        fs2.add_sandbox_layout(r"D:\x\");
        fs2.add_sandbox_layout(r"E:\");
        assert!(fs2.exists(r"D:\x\Windows\System32") && fs2.exists(r"E:\Program Files (x86)"));
        // The Temp folder opens as an ordinary local root.
        let p = fs
            .open_root_str(
                &format!(
                    r"C:\bezem-sandbox\{}",
                    KnownFolders::sandbox_relative(KnownFolderId::Temp)
                ),
                RootOpenMode::VerifyNoFollow,
            )
            .unwrap();
        assert!(p.is_directory && !p.is_reparse);
    }

    #[test]
    fn memfs_layout_is_idempotent() {
        let fs = MemFs::new();
        fs.add_sandbox_layout(r"C:\s");
        fs.add_file(r"C:\s\Windows\Temp\keep.txt", Default::default());
        let before = fs.snapshot();
        fs.add_sandbox_layout(r"C:\s");
        assert_eq!(fs.snapshot(), before);
    }
}
