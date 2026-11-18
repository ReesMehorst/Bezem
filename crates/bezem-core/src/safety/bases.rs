// SPDX-License-Identifier: MIT

//! Allowlisted bases and protected paths (M1 design section 4.4; ARCHITECTURE A:862-879).
//!
//! Ownership: work package **B**. The static tables here are policy: changing them needs two
//! approvals (CONTRIBUTING section 10).

use crate::path::{RootToken, WidePath};
use crate::resolve::KnownFolderId;
use crate::types::Scope;

/// Index of an allowed base in the policy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AllowedBaseId(pub u8);

/// Whether a root may equal its base or must be strictly below it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllowKind {
    EqualOrBelow,
    StrictlyBelow,
}

/// A canonicalized allowlisted base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedBase {
    pub id: AllowedBaseId,
    /// Roots must use the same token family as their base.
    pub family: RootToken,
    pub scope: Scope,
    pub allow: AllowKind,
    pub canonical: WidePath,
    pub volume_serial: u64,
}

/// A canonicalized protected path: a root may not equal it or be an ancestor of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedPath {
    pub canonical: WidePath,
    pub label: &'static str,
}

/// One row of [`BASE_TABLE`]: `family` token's known folder + `rel` components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseSpec {
    pub family: RootToken,
    pub rel: &'static [&'static str],
    pub allow: AllowKind,
}

/// One row of [`PROTECTED_TABLE`]: `folder` + `rel` components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectedSpec {
    pub folder: KnownFolderId,
    pub rel: &'static [&'static str],
    pub label: &'static str,
}

/// Allowlisted bases. User: `%LOCALAPPDATA%`, `%LOCALAPPDATALOW%`, `%APPDATA%` strictly below
/// (they are also protected); `%TEMP%`, `%RECENT%` equal or below (design deviation 3).
/// Machine: the `%WINDIR%` and `%PROGRAMDATA%` sub-folders, equal or below. The scope of each
/// base is `family.scope()`.
pub static BASE_TABLE: &[BaseSpec] = &[
    BaseSpec {
        family: RootToken::LocalAppData,
        rel: &[],
        allow: AllowKind::StrictlyBelow,
    },
    BaseSpec {
        family: RootToken::LocalAppDataLow,
        rel: &[],
        allow: AllowKind::StrictlyBelow,
    },
    BaseSpec {
        family: RootToken::AppData,
        rel: &[],
        allow: AllowKind::StrictlyBelow,
    },
    BaseSpec {
        family: RootToken::Temp,
        rel: &[],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::Recent,
        rel: &[],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::WinDir,
        rel: &["Temp"],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::WinDir,
        rel: &["SoftwareDistribution", "Download"],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::WinDir,
        rel: &["Minidump"],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::WinDir,
        rel: &[
            "ServiceProfiles",
            "LocalService",
            "AppData",
            "Local",
            "FontCache",
        ],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::WinDir,
        rel: &["SystemTemp"],
        allow: AllowKind::EqualOrBelow,
    },
    BaseSpec {
        family: RootToken::ProgramData,
        rel: &["Microsoft", "Windows", "WER"],
        allow: AllowKind::EqualOrBelow,
    },
];

/// Protected paths (drive roots are handled by form via `WidePath::is_drive_root`).
pub static PROTECTED_TABLE: &[ProtectedSpec] = &[
    ProtectedSpec {
        folder: KnownFolderId::Windows,
        rel: &[],
        label: "Windows",
    },
    ProtectedSpec {
        folder: KnownFolderId::Windows,
        rel: &["System32"],
        label: "System32",
    },
    ProtectedSpec {
        folder: KnownFolderId::Windows,
        rel: &["SysWOW64"],
        label: "SysWOW64",
    },
    ProtectedSpec {
        folder: KnownFolderId::Windows,
        rel: &["WinSxS"],
        label: "WinSxS",
    },
    ProtectedSpec {
        folder: KnownFolderId::Windows,
        rel: &["Installer"],
        label: "Installer",
    },
    ProtectedSpec {
        folder: KnownFolderId::ProgramFiles,
        rel: &[],
        label: "Program Files",
    },
    ProtectedSpec {
        folder: KnownFolderId::ProgramFilesX86,
        rel: &[],
        label: "Program Files (x86)",
    },
    ProtectedSpec {
        folder: KnownFolderId::Profile,
        rel: &[],
        label: "user profile",
    },
    ProtectedSpec {
        folder: KnownFolderId::Desktop,
        rel: &[],
        label: "Desktop",
    },
    ProtectedSpec {
        folder: KnownFolderId::Documents,
        rel: &[],
        label: "Documents",
    },
    ProtectedSpec {
        folder: KnownFolderId::Downloads,
        rel: &[],
        label: "Downloads",
    },
    ProtectedSpec {
        folder: KnownFolderId::Pictures,
        rel: &[],
        label: "Pictures",
    },
    ProtectedSpec {
        folder: KnownFolderId::Music,
        rel: &[],
        label: "Music",
    },
    ProtectedSpec {
        folder: KnownFolderId::Videos,
        rel: &[],
        label: "Videos",
    },
    ProtectedSpec {
        folder: KnownFolderId::OneDrive,
        rel: &[],
        label: "OneDrive",
    },
    ProtectedSpec {
        folder: KnownFolderId::LocalAppData,
        rel: &[],
        label: "LocalAppData",
    },
    ProtectedSpec {
        folder: KnownFolderId::RoamingAppData,
        rel: &[],
        label: "AppData",
    },
    ProtectedSpec {
        folder: KnownFolderId::ProgramData,
        rel: &[],
        label: "ProgramData",
    },
];
