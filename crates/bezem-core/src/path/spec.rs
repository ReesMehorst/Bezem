// SPDX-License-Identifier: MIT

//! Root specifications: `%TOKEN%\comp\comp` templates and the [`RootToken`] table.
//!
//! Ownership: work package **B**. The parser algorithm is M1 design section 4.1; the
//! `RootSpec` it returns is the only input `resolve::resolve_root` accepts.

use super::PathReject;
use crate::resolve::KnownFolderId;
use crate::types::Scope;

/// A known-folder token at the start of a root spec (ARCHITECTURE A:713-722).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RootToken {
    Temp,
    LocalAppData,
    LocalAppDataLow,
    AppData,
    Recent,
    UserProfile,
    ProgramData,
    WinDir,
}

impl RootToken {
    /// Case-insensitive token name without `%`: `TEMP`, `TMP`, `LOCALAPPDATA`,
    /// `LOCALAPPDATALOW`, `APPDATA`, `RECENT`, `USERPROFILE`, `PROGRAMDATA`, `WINDIR`,
    /// `SYSTEMROOT`.
    pub fn parse(name: &str) -> Option<Self> {
        const TABLE: &[(&str, RootToken)] = &[
            ("TEMP", RootToken::Temp),
            ("TMP", RootToken::Temp),
            ("LOCALAPPDATA", RootToken::LocalAppData),
            ("LOCALAPPDATALOW", RootToken::LocalAppDataLow),
            ("APPDATA", RootToken::AppData),
            ("RECENT", RootToken::Recent),
            ("USERPROFILE", RootToken::UserProfile),
            ("PROGRAMDATA", RootToken::ProgramData),
            ("WINDIR", RootToken::WinDir),
            ("SYSTEMROOT", RootToken::WinDir),
        ];
        TABLE
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, t)| t)
    }

    /// The canonical spelling used by [`RootSpec::to_canonical_string`] (without `%`).
    pub fn canonical_name(self) -> &'static str {
        match self {
            RootToken::Temp => "TEMP",
            RootToken::LocalAppData => "LOCALAPPDATA",
            RootToken::LocalAppDataLow => "LOCALAPPDATALOW",
            RootToken::AppData => "APPDATA",
            RootToken::Recent => "RECENT",
            RootToken::UserProfile => "USERPROFILE",
            RootToken::ProgramData => "PROGRAMDATA",
            RootToken::WinDir => "WINDIR",
        }
    }

    /// User-profile tokens are `User`; `ProgramData` and `WinDir` are `Machine`.
    pub fn scope(self) -> Scope {
        match self {
            RootToken::ProgramData | RootToken::WinDir => Scope::Machine,
            RootToken::Temp
            | RootToken::LocalAppData
            | RootToken::LocalAppDataLow
            | RootToken::AppData
            | RootToken::Recent
            | RootToken::UserProfile => Scope::User,
        }
    }

    /// The known folder the token resolves through (never the environment).
    pub fn known_folder(self) -> KnownFolderId {
        match self {
            RootToken::Temp => KnownFolderId::Temp,
            RootToken::LocalAppData => KnownFolderId::LocalAppData,
            RootToken::LocalAppDataLow => KnownFolderId::LocalAppDataLow,
            RootToken::AppData => KnownFolderId::RoamingAppData,
            RootToken::Recent => KnownFolderId::Recent,
            RootToken::UserProfile => KnownFolderId::Profile,
            RootToken::ProgramData => KnownFolderId::ProgramData,
            RootToken::WinDir => KnownFolderId::Windows,
        }
    }
}

/// A parsed, validated root template: a token plus template components (each passed
/// `check_template_component`). Built only by [`parse_root_spec`]/[`parse_root_spec_wide`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RootSpec {
    pub token: RootToken,
    pub components: Vec<Box<[u16]>>,
}

impl RootSpec {
    /// `Temp` token; or `WinDir` + `[Temp | SystemTemp]`; or `LocalAppData` + `[Temp]`.
    #[allow(clippy::unimplemented)] // M1-STUB (B)
    pub fn is_temp_like(&self) -> bool {
        unimplemented!()
    }

    /// `"%TEMP%\a\b"`. Round-trips: `parse_root_spec(&s.to_canonical_string()) == Ok(s)`.
    #[allow(clippy::unimplemented)] // M1-STUB (B)
    pub fn to_canonical_string(&self) -> String {
        unimplemented!()
    }
}

/// Parses a root spec from a string (UTF-8 front end of [`parse_root_spec_wide`]).
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn parse_root_spec(input: &str) -> Result<RootSpec, PathReject> {
    unimplemented!()
}

/// Parses a root spec from UTF-16 (core implementation; fuzz entry point). Algorithm: M1
/// design section 4.1. Never resolves `..`, never drops components, never expands the
/// environment. All literal absolute paths are rejected: roots come only from `%TOKEN%`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn parse_root_spec_wide(input: &[u16]) -> Result<RootSpec, PathReject> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_table() {
        assert_eq!(RootToken::parse("tmp"), Some(RootToken::Temp));
        assert_eq!(RootToken::parse("SystemRoot"), Some(RootToken::WinDir));
        assert_eq!(RootToken::parse("BEZEM_UNSET"), None);
        assert_eq!(
            RootToken::AppData.known_folder(),
            KnownFolderId::RoamingAppData
        );
        assert_eq!(RootToken::WinDir.scope(), Scope::Machine);
        assert_eq!(RootToken::Recent.scope(), Scope::User);
        for t in [
            RootToken::Temp,
            RootToken::LocalAppData,
            RootToken::LocalAppDataLow,
            RootToken::AppData,
            RootToken::Recent,
            RootToken::UserProfile,
            RootToken::ProgramData,
            RootToken::WinDir,
        ] {
            assert_eq!(RootToken::parse(t.canonical_name()), Some(t));
            assert_eq!(t.known_folder().scope(), t.scope());
        }
    }
}
