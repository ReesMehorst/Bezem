// SPDX-License-Identifier: MIT

//! Root specifications: `%TOKEN%\comp\comp` templates and the [`RootToken`] table.
//!
//! Ownership: work package **B**. The parser algorithm is M1 design section 4.1; the
//! `RootSpec` it returns is the only input `resolve::resolve_root` accepts.

use super::{check_template_component, eq_wide, Cmp, PathReject};
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
    ///
    /// Matches on the leading components (so roots below those folders count too) and folds
    /// case loosely: a false positive only raises the age floor, so over-matching is the safe
    /// direction. `UserProfile` + `[AppData, Local, Temp]` is treated as temp-like as well.
    pub fn is_temp_like(&self) -> bool {
        let starts = |names: &[&str]| {
            names.len() <= self.components.len()
                && names.iter().zip(&self.components).all(|(n, c)| {
                    let n: Vec<u16> = n.encode_utf16().collect();
                    eq_wide(&n, c, Cmp::Loose)
                })
        };
        match self.token {
            RootToken::Temp => true,
            RootToken::WinDir => starts(&["Temp"]) || starts(&["SystemTemp"]),
            RootToken::LocalAppData => starts(&["Temp"]),
            RootToken::UserProfile => starts(&["AppData", "Local", "Temp"]),
            RootToken::LocalAppDataLow
            | RootToken::AppData
            | RootToken::Recent
            | RootToken::ProgramData => false,
        }
    }

    /// `"%TEMP%\a\b"`. Round-trips: `parse_root_spec(&s.to_canonical_string()) == Ok(s)`.
    pub fn to_canonical_string(&self) -> String {
        let mut s =
            String::with_capacity(16 + self.components.iter().map(|c| c.len() + 1).sum::<usize>());
        s.push('%');
        s.push_str(self.token.canonical_name());
        s.push('%');
        for c in &self.components {
            s.push('\\');
            // Components never hold lone surrogates (rejected by the parser), so this is lossless.
            s.push_str(&String::from_utf16_lossy(c));
        }
        s
    }
}

/// Longest accepted template tail (components joined by `\`), in UTF-16 units.
const MAX_TEMPLATE_TAIL: usize = 240;

const PERCENT: u16 = b'%' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const QUESTION: u16 = b'?' as u16;
const DOT: u16 = b'.' as u16;

/// Parses a root spec from a string (UTF-8 front end of [`parse_root_spec_wide`]).
pub fn parse_root_spec(input: &str) -> Result<RootSpec, PathReject> {
    let wide: Vec<u16> = input.encode_utf16().collect();
    parse_root_spec_wide(&wide)
}

/// Parses a root spec from UTF-16 (core implementation; fuzz entry point). Algorithm: M1
/// design section 4.1. Never resolves `..`, never drops components, never expands the
/// environment. All literal absolute paths are rejected: roots come only from `%TOKEN%`.
pub fn parse_root_spec_wide(input: &[u16]) -> Result<RootSpec, PathReject> {
    // Step 1: emptiness (after trimming whitespace), NUL, lone surrogates.
    if char::decode_utf16(input.iter().copied()).all(|r| r.is_ok_and(char::is_whitespace)) {
        return Err(PathReject::Empty);
    }
    if input.contains(&0) {
        return Err(PathReject::NulChar);
    }
    if char::decode_utf16(input.iter().copied()).any(|r| r.is_err()) {
        return Err(PathReject::LoneSurrogateInTemplate);
    }

    // Step 2: `/` -> `\` (no collapsing, no trimming).
    let s: Vec<u16> = input
        .iter()
        .map(|&u| if u == SLASH { BACKSLASH } else { u })
        .collect();

    // Step 3: anything not starting with `%` is rejected, classified for the message.
    if s.first() != Some(&PERCENT) {
        return Err(classify_literal(&s));
    }

    // Step 4: `%NAME%` must be closed and a known token; then only `\` or the end.
    let rest = s.get(1..).unwrap_or(&[]);
    let Some(close) = rest.iter().position(|&u| u == PERCENT) else {
        return Err(PathReject::UnknownToken(String::from_utf16_lossy(rest)));
    };
    let name = String::from_utf16_lossy(rest.get(..close).unwrap_or(&[]));
    let Some(token) = RootToken::parse(&name) else {
        return Err(PathReject::UnknownToken(name));
    };
    let tail = rest.get(close + 1..).unwrap_or(&[]);
    if tail.contains(&PERCENT) {
        return Err(PathReject::EmbeddedPercent);
    }
    let tail = match tail {
        [] => &[][..],
        [BACKSLASH, t @ ..] => t,
        _ => return Err(PathReject::NoLeadingToken),
    };

    // Step 5: split; one trailing `\` is dropped (`%TEMP%\` has no components, `%TEMP%\\`
    // has an empty one); every component checked strictly.
    let mut components = Vec::new();
    if !tail.is_empty() {
        let tail = match tail {
            [t @ .., BACKSLASH] => t,
            _ => tail,
        };
        for c in tail.split(|&u| u == BACKSLASH) {
            check_template_component(c)?;
            components.push(Box::<[u16]>::from(c));
        }
        if tail.len() > MAX_TEMPLATE_TAIL {
            return Err(PathReject::PathTooLong);
        }
    }
    Ok(RootSpec { token, components })
}

/// Step 3 of the algorithm: why a spec that does not start with `%` is rejected. `s` already
/// has `/` replaced by `\`.
fn classify_literal(s: &[u16]) -> PathReject {
    let is_letter = |u: u16| u8::try_from(u).is_ok_and(|b| b.is_ascii_alphabetic());
    match s {
        [BACKSLASH, BACKSLASH, m, BACKSLASH, ..] if *m == QUESTION || *m == DOT => {
            PathReject::DevicePrefix
        }
        [BACKSLASH, QUESTION, QUESTION, BACKSLASH, ..] => PathReject::DevicePrefix,
        [BACKSLASH, BACKSLASH, ..] => PathReject::Unc,
        [BACKSLASH, ..] => PathReject::RootedNoDrive,
        [d, COLON, rest @ ..] if is_letter(*d) => {
            if rest.iter().all(|&u| u == BACKSLASH) {
                PathReject::DriveRoot
            } else if rest.first() == Some(&BACKSLASH) {
                PathReject::LiteralAbsolute
            } else {
                PathReject::DriveRelative
            }
        }
        _ => PathReject::NoLeadingToken,
    }
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

    fn comps(spec: &RootSpec) -> Vec<String> {
        spec.components
            .iter()
            .map(|c| String::from_utf16_lossy(c))
            .collect()
    }

    /// Q:31-42 path cases (M1 design section 4.2), parser level.
    #[test]
    fn q_path_cases() {
        use PathReject as R;
        let long_tail = format!(r"%TEMP%\{}", vec!["a".repeat(60); 5].join("\\"));
        let cases: Vec<(&str, PathReject)> = vec![
            // Empty, whitespace-only, '.'.
            ("", R::Empty),
            ("  ", R::Empty),
            ("\t", R::Empty),
            (" \r\n\u{3000}", R::Empty),
            (".", R::NoLeadingToken),
            // Unset / unknown tokens are rejected, never expanded to "".
            (r"%BEZEM_UNSET%\foo", R::UnknownToken("BEZEM_UNSET".into())),
            (r"%UNSET%\x", R::UnknownToken("UNSET".into())),
            (r"%%\x", R::UnknownToken(String::new())),
            (r"%TEMP", R::UnknownToken("TEMP".into())),
            (r"%PATH%", R::UnknownToken("PATH".into())),
            // Drive roots in all spellings.
            ("C:", R::DriveRoot),
            (r"C:\", R::DriveRoot),
            ("C:/", R::DriveRoot),
            ("c:\\\\", R::DriveRoot),
            // Device / namespace prefixes.
            (r"\\?\C:\", R::DevicePrefix),
            (r"\\.\C:\", R::DevicePrefix),
            (r"\??\C:\", R::DevicePrefix),
            (r"\\.\PhysicalDrive0", R::DevicePrefix),
            (r"\\?\UNC\s\x", R::DevicePrefix),
            (r"\\?\GLOBALROOT\Device\x", R::DevicePrefix),
            (r"\\?\Volume{0}\x", R::DevicePrefix),
            ("//?/C:/", R::DevicePrefix),
            // UNC.
            (r"\\server\share", R::Unc),
            (r"\\server\share\x", R::Unc),
            ("//server/share", R::Unc),
            // Mapped drive / any literal absolute path.
            (r"T:\x", R::LiteralAbsolute),
            // Forbidden roots with spelling variants: all literal paths are refused.
            (r"C:\Windows", R::LiteralAbsolute),
            (r"c:\WINDOWS\", R::LiteralAbsolute),
            (r"C:\Windows\System32", R::LiteralAbsolute),
            (r"C:\Program Files", R::LiteralAbsolute),
            (r"C:\Program Files (x86)", R::LiteralAbsolute),
            (r"C:\Users\u", R::LiteralAbsolute),
            ("C:/Users/u", R::LiteralAbsolute),
            (r"C:\PROGRA~1", R::LiteralAbsolute),
            (r"C:\WINDOW~1", R::LiteralAbsolute),
            (r"C:\WİNDOWS", R::LiteralAbsolute),
            ("C:Windows", R::DriveRelative),
            (r"\Windows", R::RootedNoDrive),
            ("/Windows", R::RootedNoDrive),
            // Reserved device names and relative paths.
            ("CON", R::NoLeadingToken),
            ("NUL", R::NoLeadingToken),
            ("COM1", R::NoLeadingToken),
            (r"foo\bar", R::NoLeadingToken),
            (r"..\x", R::NoLeadingToken),
            (r" %TEMP%\x", R::NoLeadingToken),
            (r"%TEMP%x", R::NoLeadingToken),
            // Traversal and mixed separators.
            (r"%TEMP%\..\..", R::ParentTraversal),
            ("%TEMP%/../x", R::ParentTraversal),
            (r"%TEMP%\a\..\b", R::ParentTraversal),
            (r"%TEMP%\a/../b", R::ParentTraversal),
            (r"%TEMP%\.\x", R::CurrentDirComponent),
            (r"%TEMP%\\x", R::EmptyComponent),
            (r"%TEMP%\x\\", R::EmptyComponent),
            (r"%TEMP%\\", R::EmptyComponent),
            ("%TEMP%//", R::EmptyComponent),
            // 8.3 names.
            (r"%TEMP%\PROGRA~1", R::ShortName),
            (r"%TEMP%\a~1.tmp", R::ShortName),
            (r"%LOCALAPPDATA%\x\WINDOW~1\y", R::ShortName),
            // Reserved names.
            (r"%TEMP%\CON", R::ReservedDeviceName),
            (r"%TEMP%\aux.txt", R::ReservedDeviceName),
            (r"%TEMP%\x\com1", R::ReservedDeviceName),
            (r"%TEMP%\LPT¹", R::ReservedDeviceName),
            // Trailing dot or space (checked before reserved names).
            (r"%TEMP%\nul.", R::TrailingDotOrSpace),
            (r"%TEMP%\x.", R::TrailingDotOrSpace),
            (r"%TEMP%\x ", R::TrailingDotOrSpace),
            (r"%TEMP%\x \y", R::TrailingDotOrSpace),
            // Alternate data streams.
            (r"%TEMP%\file.txt:stream", R::AlternateStream),
            (r"%TEMP%\file.txt::$DATA", R::AlternateStream),
            (r"%TEMP%\C:\x", R::AlternateStream),
            // Wildcards, invalid chars, embedded tokens.
            (r"%TEMP%\*", R::Wildcard),
            (r"%TEMP%\a?b", R::Wildcard),
            (r"%TEMP%\a<b", R::InvalidChar(u16::from(b'<'))),
            (r"%TEMP%\a|b", R::InvalidChar(u16::from(b'|'))),
            ("%TEMP%\\a\u{1}", R::InvalidChar(1)),
            (r"%TEMP%\%TEMP%", R::EmbeddedPercent),
            (r"%TEMP%\100%", R::EmbeddedPercent),
            (r"%TEMP%%x", R::EmbeddedPercent),
            ("%TEMP%\\a\0", R::NulChar),
            (&long_tail, R::PathTooLong),
        ];
        for (input, want) in &cases {
            assert_eq!(parse_root_spec(input), Err(want.clone()), "{input:?}");
        }
        let too_long_comp = format!(r"%TEMP%\{}", "a".repeat(256));
        assert_eq!(parse_root_spec(&too_long_comp), Err(R::ComponentTooLong));
        assert_eq!(
            parse_root_spec_wide(&[u16::from(b'%'), 0xD800]),
            Err(R::LoneSurrogateInTemplate)
        );
    }

    #[test]
    fn accepted_specs() {
        let ok: &[(&str, RootToken, &[&str])] = &[
            ("%TEMP%", RootToken::Temp, &[]),
            (r"%TEMP%\", RootToken::Temp, &[]),
            ("%tmp%/", RootToken::Temp, &[]),
            ("%TEMP%/a\\b", RootToken::Temp, &["a", "b"]),
            (r"%TEMP%\a\b\", RootToken::Temp, &["a", "b"]),
            (
                r"%LocalAppData%\Microsoft\Edge\User Data",
                RootToken::LocalAppData,
                &["Microsoft", "Edge", "User Data"],
            ),
            (r"%SYSTEMROOT%\Temp", RootToken::WinDir, &["Temp"]),
            (
                r"%windir%\SoftwareDistribution\Download",
                RootToken::WinDir,
                &["SoftwareDistribution", "Download"],
            ),
            (r"%TEMP%\İ\ß\K", RootToken::Temp, &["İ", "ß", "K"]),
            (
                r"%TEMP%\a~b\.x\...x",
                RootToken::Temp,
                &["a~b", ".x", "...x"],
            ),
        ];
        for &(input, token, want) in ok {
            let spec = parse_root_spec(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
            assert_eq!(spec.token, token, "{input:?}");
            assert_eq!(comps(&spec), want, "{input:?}");
        }
        let max_tail = format!(r"%TEMP%\{}", vec!["a".repeat(59); 4].join("\\"));
        assert!(parse_root_spec(&max_tail).is_ok());
    }

    #[test]
    fn canonical_string() {
        let spec = parse_root_spec("%tmp%/a\\b").unwrap();
        assert_eq!(spec.to_canonical_string(), r"%TEMP%\a\b");
        assert_eq!(
            parse_root_spec("%SystemRoot%")
                .unwrap()
                .to_canonical_string(),
            "%WINDIR%"
        );
    }

    #[test]
    fn temp_like() {
        let cases: &[(&str, bool)] = &[
            ("%TEMP%", true),
            (r"%TMP%\x", true),
            (r"%WINDIR%\Temp", true),
            (r"%WINDIR%\temp\x", true),
            (r"%WINDIR%\SystemTemp", true),
            (r"%WINDIR%\Minidump", false),
            ("%WINDIR%", false),
            (r"%LOCALAPPDATA%\Temp", true),
            (r"%LOCALAPPDATA%\TEMP\sub", true),
            (r"%LOCALAPPDATA%\Microsoft", false),
            (r"%USERPROFILE%\AppData\Local\Temp", true),
            (r"%USERPROFILE%\AppData\Local", false),
            (r"%APPDATA%\Temp", false),
            (r"%PROGRAMDATA%\Temp", false),
            (r"%RECENT%", false),
            (r"%LOCALAPPDATALOW%\Temp", false),
        ];
        for &(input, want) in cases {
            assert_eq!(
                parse_root_spec(input).unwrap().is_temp_like(),
                want,
                "{input:?}"
            );
        }
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        const TOKENS: &[&str] = &[
            "%TEMP%",
            "%tmp%",
            "%LOCALAPPDATA%",
            "%AppData%",
            "%WINDIR%",
            "%SYSTEMROOT%",
            "%USERPROFILE%",
            "%UNSET%",
            "%%",
            "%",
        ];

        fn arb_input() -> impl Strategy<Value = String> {
            let piece = prop_oneof![
                3 => Just("\\".to_owned()),
                2 => Just("/".to_owned()),
                1 => Just(".".to_owned()),
                1 => Just("..".to_owned()),
                1 => Just(":".to_owned()),
                1 => Just("~1".to_owned()),
                1 => Just("%".to_owned()),
                1 => Just(" ".to_owned()),
                1 => Just("CON".to_owned()),
                1 => Just("*".to_owned()),
                4 => "[a-zA-Z0-9_ -]{1,6}",
                1 => any::<char>().prop_map(|c| c.to_string()),
            ];
            (
                prop::sample::select(TOKENS),
                proptest::collection::vec(piece, 0..10),
                any::<bool>(),
            )
                .prop_map(|(t, pieces, with_token)| {
                    let mut s = if with_token {
                        t.to_owned()
                    } else {
                        String::new()
                    };
                    s.extend(pieces);
                    s
                })
        }

        fn arb_component() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9_][a-zA-Z0-9_ .()-]{0,10}[a-zA-Z0-9_)]"
        }

        proptest! {
            /// Containment: an accepted spec is token-rooted and its components contain no
            /// `.`/`..`/empty components, separators, `:`, `%`, NUL or wildcards.
            #[test]
            fn containment(input in arb_input()) {
                if let Ok(spec) = parse_root_spec(&input) {
                    prop_assert!(input.starts_with('%'));
                    for c in &spec.components {
                        prop_assert!(!c.is_empty());
                        prop_assert!(c[..] != [u16::from(b'.')]);
                        prop_assert!(c[..] != [u16::from(b'.'), u16::from(b'.')]);
                        for &u in c.iter() {
                            prop_assert!(![b'\\', b'/', b':', b'%', 0, b'*', b'?']
                                .iter()
                                .any(|&b| u == u16::from(b)));
                        }
                        prop_assert!(check_template_component(c).is_ok());
                    }
                    // Round trip and idempotence of the canonical form.
                    let canon = spec.to_canonical_string();
                    let again = parse_root_spec(&canon);
                    prop_assert_eq!(again.as_ref(), Ok(&spec));
                    prop_assert_eq!(again.map(|s| s.to_canonical_string()), Ok(canon));
                }
            }

            /// Valid specs round-trip and normalization never drops or changes components.
            #[test]
            fn round_trip(
                token in prop::sample::select(TOKENS[..7].to_vec()),
                parts in proptest::collection::vec(arb_component(), 0..6),
                slash in any::<bool>(),
            ) {
                let sep = if slash { "/" } else { "\\" };
                let mut input = token.to_owned();
                for p in &parts {
                    input.push_str(sep);
                    input.push_str(p);
                }
                match parse_root_spec(&input) {
                    Ok(spec) => {
                        prop_assert_eq!(comps(&spec), parts.clone());
                        let canon = spec.to_canonical_string();
                        prop_assert_eq!(parse_root_spec(&canon), Ok(spec));
                    }
                    // Only the component rules may reject these generated inputs.
                    Err(e) => prop_assert!(matches!(
                        e,
                        PathReject::ReservedDeviceName
                            | PathReject::TrailingDotOrSpace
                            | PathReject::ShortName
                    ), "{e:?} for {input:?}"),
                }
            }

            /// The wide parser never panics on arbitrary UTF-16.
            #[test]
            fn wide_never_panics(v in proptest::collection::vec(any::<u16>(), 0..40)) {
                let _ = parse_root_spec_wide(&v);
            }
        }
    }
}
