// SPDX-License-Identifier: MIT

//! [`WidePath`]: the only absolute-path type security decisions use.
//!
//! Ownership: work package **B** (constructors, pushes and comparisons). The simple accessors
//! that rely only on the type invariant are implemented in Phase 0.

use super::component::{check_known_component, check_listed_name, check_template_component};
use super::{eq_wide, starts_with_at_boundary, Cmp, PathReject};

/// Length of the `\\?\X:\` prefix in UTF-16 units.
const PREFIX_LEN: usize = 7;
/// Offset of the drive letter (after `\\?\`).
const DRIVE_AT: usize = 4;
const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const DOT: u16 = b'.' as u16;
const QUESTION: u16 = b'?' as u16;
/// Longest accepted path in UTF-16 units (including the `\\?\` prefix).
const MAX_TOTAL: usize = 32_000;

fn is_ascii_letter(u: u16) -> bool {
    u8::try_from(u).is_ok_and(|b| b.is_ascii_alphabetic())
}

fn ascii_upper(u: u16) -> u16 {
    match u8::try_from(u) {
        Ok(b) => u16::from(b.to_ascii_uppercase()),
        Err(_) => u,
    }
}

fn is_sep(u: u16) -> bool {
    u == BACKSLASH || u == SLASH
}

/// `s` without the ASCII-case-insensitive `prefix`, if it starts with it.
fn strip_prefix_ci<'a>(s: &'a [u16], prefix: &str) -> Option<&'a [u16]> {
    let p: Vec<u16> = prefix.encode_utf16().collect();
    let head = s.get(..p.len())?;
    if eq_wide(head, &p, Cmp::Strict) {
        s.get(p.len()..)
    } else {
        None
    }
}

/// A validated absolute Windows path in UTF-16.
///
/// Invariant: `\\?\X:\` (ASCII letter, upper-cased) + components joined by `\`; no trailing
/// `\` except at the drive root; no empty, `.` or `..` components; no `:` after the drive, no
/// NUL; each component <= 255 units; total <= 32_000 units. Only the constructors below build
/// one, so every accessor may rely on the invariant.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct WidePath(Vec<u16>);

impl WidePath {
    /// Strict `\\?\X:\...` only (OS API output); rejects `\\?\UNC\`, `\\?\Volume{`,
    /// `GLOBALROOT`, `\\.\` and `\??\`.
    ///
    /// Components are checked like listed names (they carry on-disk names literally).
    pub fn from_final(raw: &[u16]) -> Result<Self, PathReject> {
        if raw.is_empty() {
            return Err(PathReject::Empty);
        }
        if raw.contains(&0) {
            return Err(PathReject::NulChar);
        }
        let Some(rest) = strip_prefix_ci(raw, r"\\?\") else {
            return Err(PathReject::BadFinalPathForm);
        };
        if strip_prefix_ci(rest, r"UNC\").is_some() {
            return Err(PathReject::UncFinal);
        }
        match rest {
            [d, c, s, body @ ..] if is_ascii_letter(*d) && *c == COLON && *s == BACKSLASH => {
                Self::build(*d, body, check_listed_name)
            }
            _ => Err(PathReject::BadFinalPathForm),
        }
    }

    /// A known-folder API value: `X:\...` or `\\?\X:\...`. Lenient on `~1` (the real `%TEMP%`
    /// is 8.3 on CI runners); rejects UNC, relative, empty, fewer than 4 units and `..`.
    ///
    /// One trailing `\` is accepted (`GetTempPath2W` style). Drive roots are rejected
    /// ([`PathReject::DriveRoot`]); forward slashes, wildcards and characters no file name can
    /// hold are rejected.
    pub fn from_known_value(raw: &[u16]) -> Result<Self, PathReject> {
        if raw.is_empty() {
            return Err(PathReject::Empty);
        }
        if raw.contains(&0) {
            return Err(PathReject::NulChar);
        }
        let (drive, after) = if let Some(rest) = strip_prefix_ci(raw, r"\\?\") {
            if strip_prefix_ci(rest, r"UNC\").is_some() {
                return Err(PathReject::Unc);
            }
            match rest {
                [d, c, s, after @ ..] if is_ascii_letter(*d) && *c == COLON && *s == BACKSLASH => {
                    (*d, after)
                }
                [d, c] if is_ascii_letter(*d) && *c == COLON => return Err(PathReject::DriveRoot),
                _ => return Err(PathReject::DevicePrefix),
            }
        } else {
            match raw {
                [BACKSLASH, BACKSLASH, d, BACKSLASH, ..] if *d == DOT || *d == QUESTION => {
                    return Err(PathReject::DevicePrefix)
                }
                [BACKSLASH, QUESTION, QUESTION, BACKSLASH, ..] => {
                    return Err(PathReject::DevicePrefix)
                }
                [s1, s2, ..] if is_sep(*s1) && is_sep(*s2) => return Err(PathReject::Unc),
                [s, ..] if is_sep(*s) => return Err(PathReject::RootedNoDrive),
                [d, c] if is_ascii_letter(*d) && *c == COLON => return Err(PathReject::DriveRoot),
                [d, c, s, after @ ..] if is_ascii_letter(*d) && *c == COLON && *s == BACKSLASH => {
                    (*d, after)
                }
                [d, c, SLASH, ..] if is_ascii_letter(*d) && *c == COLON => {
                    return Err(PathReject::InvalidChar(SLASH))
                }
                [d, c, ..] if is_ascii_letter(*d) && *c == COLON => {
                    return Err(PathReject::DriveRelative)
                }
                _ => return Err(PathReject::NoLeadingToken),
            }
        };
        // One trailing `\` is tolerated; what remains must name at least one component.
        let after = match after {
            [inner @ .., BACKSLASH] => inner,
            _ => after,
        };
        if after.is_empty() {
            return Err(PathReject::DriveRoot);
        }
        Self::build(drive, after, check_known_component)
    }

    /// Appends a template component, checked strictly (`check_template_component`).
    pub fn push_template_component(&mut self, c: &[u16]) -> Result<(), PathReject> {
        check_template_component(c)?;
        self.push_checked(c)
    }

    /// Appends a name returned by a directory listing, checked leniently (`check_listed_name`).
    pub fn push_listed_name(&mut self, c: &[u16]) -> Result<(), PathReject> {
        check_listed_name(c)?;
        self.push_checked(c)
    }

    fn push_checked(&mut self, c: &[u16]) -> Result<(), PathReject> {
        let sep = usize::from(!self.is_drive_root());
        if self.0.len() + sep + c.len() > MAX_TOTAL {
            return Err(PathReject::PathTooLong);
        }
        if sep == 1 {
            self.0.push(BACKSLASH);
        }
        self.0.extend_from_slice(c);
        Ok(())
    }

    /// `\\?\` + upper-cased `drive` + `:\` + `body` (components separated by `\`, no
    /// trailing `\`), each component through `check`.
    fn build(
        drive: u16,
        body: &[u16],
        check: fn(&[u16]) -> Result<(), PathReject>,
    ) -> Result<Self, PathReject> {
        let mut v: Vec<u16> = Vec::with_capacity(PREFIX_LEN + body.len());
        v.extend(r"\\?\".encode_utf16());
        v.push(ascii_upper(drive));
        v.push(COLON);
        v.push(BACKSLASH);
        if !body.is_empty() {
            for comp in body.split(|&u| u == BACKSLASH) {
                check(comp)?;
            }
            v.extend_from_slice(body);
        }
        if v.len() > MAX_TOTAL {
            return Err(PathReject::PathTooLong);
        }
        Ok(WidePath(v))
    }

    /// `\\?\X:\` with no components.
    pub fn is_drive_root(&self) -> bool {
        self.0.len() == PREFIX_LEN
    }

    /// The upper-case ASCII drive letter as a UTF-16 unit.
    pub fn drive_letter(&self) -> u16 {
        self.0.get(DRIVE_AT).copied().unwrap_or(0)
    }

    /// The components after the drive root, in order.
    pub fn components(&self) -> impl Iterator<Item = &[u16]> {
        let rest = self.0.get(PREFIX_LEN..).unwrap_or(&[]);
        rest.split(|&u| u == BACKSLASH).filter(|c| !c.is_empty())
    }

    /// Number of components below the drive root (0 for a drive root).
    pub fn depth(&self) -> usize {
        self.components().count()
    }

    /// The full path including the `\\?\` prefix, without a terminating NUL.
    pub fn as_wide(&self) -> &[u16] {
        &self.0
    }

    /// The full path including the `\\?\` prefix, NUL-terminated (for OS calls).
    pub fn to_wide_nul(&self) -> Vec<u16> {
        let mut v = Vec::with_capacity(self.0.len() + 1);
        v.extend_from_slice(&self.0);
        v.push(0);
        v
    }

    /// The path without the `\\?\` prefix (`X:\...`), as written to logs.
    pub fn without_prefix(&self) -> &[u16] {
        self.0.get(DRIVE_AT..).unwrap_or(&[])
    }

    /// [`Self::without_prefix`] as a `String`, lossy (lone surrogates become U+FFFD). For
    /// display only; never for decisions.
    pub fn display_string(&self) -> String {
        String::from_utf16_lossy(self.without_prefix())
    }

    /// The parent path, or `None` for a drive root.
    pub fn parent(&self) -> Option<WidePath> {
        if self.is_drive_root() {
            return None;
        }
        let last_sep = self.0.iter().rposition(|&u| u == BACKSLASH)?;
        // Keep the trailing `\` when the parent is the drive root.
        let end = if last_sep < PREFIX_LEN {
            PREFIX_LEN
        } else {
            last_sep
        };
        Some(WidePath(self.0.get(..end)?.to_vec()))
    }

    /// Exact equality except for ASCII case.
    pub fn eq_strict(&self, o: &WidePath) -> bool {
        eq_wide(&self.0, &o.0, Cmp::Strict)
    }

    /// `self == base` or `self` is below `base`, at a component boundary, under `cmp`.
    pub fn is_same_or_below(&self, base: &WidePath, cmp: Cmp) -> bool {
        starts_with_at_boundary(&self.0, &base.0, cmp)
    }

    /// `self` is below `base` (not equal), at a component boundary, under `cmp`.
    pub fn is_strictly_below(&self, base: &WidePath, cmp: Cmp) -> bool {
        self.is_same_or_below(base, cmp) && !eq_wide(&self.0, &base.0, cmp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> WidePath {
        // Test-only: bypasses validation; inputs below satisfy the invariant.
        WidePath(s.encode_utf16().collect())
    }

    #[test]
    fn accessors_rely_on_invariant() {
        let root = w(r"\\?\C:\");
        assert!(root.is_drive_root());
        assert_eq!(root.depth(), 0);
        assert_eq!(root.parent(), None);
        assert_eq!(root.drive_letter(), u16::from(b'C'));

        let p = w(r"\\?\C:\Users\u\Temp");
        assert!(!p.is_drive_root());
        assert_eq!(p.depth(), 3);
        assert_eq!(p.display_string(), r"C:\Users\u\Temp");
        assert_eq!(p.parent(), Some(w(r"\\?\C:\Users\u")));
        assert_eq!(w(r"\\?\C:\Users").parent(), Some(root));
        assert_eq!(p.to_wide_nul().last(), Some(&0));
        let comps: Vec<String> = p.components().map(String::from_utf16_lossy).collect();
        assert_eq!(comps, ["Users", "u", "Temp"]);
    }

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Checks the documented type invariant on any constructed value.
    pub(crate) fn assert_invariant(p: &WidePath) {
        let v = p.as_wide();
        assert!(v.len() >= PREFIX_LEN && v.len() <= MAX_TOTAL, "{v:?}");
        assert_eq!(&v[..4], &u(r"\\?\")[..]);
        assert!((u16::from(b'A')..=u16::from(b'Z')).contains(&v[4]));
        assert_eq!(v[5], COLON);
        assert_eq!(v[6], BACKSLASH);
        assert!(!v.contains(&0));
        if !p.is_drive_root() {
            assert_ne!(v.last(), Some(&BACKSLASH));
            for c in v[PREFIX_LEN..].split(|&x| x == BACKSLASH) {
                assert!(check_listed_name(c).is_ok(), "bad component {c:?}");
            }
        }
    }

    #[test]
    fn from_final_table() {
        use PathReject as R;
        let ok: &[(&str, &str)] = &[
            (r"\\?\C:\", r"\\?\C:\"),
            (r"\\?\c:\Users\u", r"\\?\C:\Users\u"),
            (
                r"\\?\C:\Users\u\AppData\Local\Temp",
                r"\\?\C:\Users\u\AppData\Local\Temp",
            ),
            // On-disk names are literal: reserved names, trailing dot/space, tildes are fine.
            (
                r"\\?\C:\x\CON\a.\b \PROGRA~1",
                r"\\?\C:\x\CON\a.\b \PROGRA~1",
            ),
        ];
        for &(raw, want) in ok {
            let p = WidePath::from_final(&u(raw)).unwrap();
            assert_eq!(p.as_wide(), &u(want)[..], "{raw:?}");
            assert_invariant(&p);
        }
        let bad: &[(&str, PathReject)] = &[
            ("", R::Empty),
            (r"C:\Windows", R::BadFinalPathForm),
            (r"\\?\UNC\server\share", R::UncFinal),
            (r"\\?\unc\server\share", R::UncFinal),
            (r"\\?\Volume{1234}\x", R::BadFinalPathForm),
            (
                r"\\?\GLOBALROOT\Device\HarddiskVolume1\x",
                R::BadFinalPathForm,
            ),
            (r"\\.\C:\x", R::BadFinalPathForm),
            (r"\??\C:\x", R::BadFinalPathForm),
            (r"\\?\C:", R::BadFinalPathForm),
            (r"\\?\C:x", R::BadFinalPathForm),
            (r"\\?\1:\x", R::BadFinalPathForm),
            (r"\\?\C:\x\", R::EmptyComponent),
            (r"\\?\C:\x\\y", R::EmptyComponent),
            (r"\\?\C:\x\..\y", R::ParentTraversal),
            (r"\\?\C:\x\.\y", R::CurrentDirComponent),
            (r"\\?\C:\x\a:b", R::AlternateStream),
            (r"\\?\C:\x/y", R::InvalidChar(SLASH)),
            ("\\\\?\\C:\\a\0", R::NulChar),
        ];
        for (raw, want) in bad {
            assert_eq!(WidePath::from_final(&u(raw)), Err(want.clone()), "{raw:?}");
        }
        let long_comp = format!(r"\\?\C:\{}", "a".repeat(256));
        assert_eq!(
            WidePath::from_final(&u(&long_comp)),
            Err(R::ComponentTooLong)
        );
        let long_path = format!(r"\\?\C:\{}", vec!["a".repeat(200); 170].join("\\"));
        assert_eq!(WidePath::from_final(&u(&long_path)), Err(R::PathTooLong));
        // Lone surrogates in on-disk names are carried literally.
        let mut raw = u(r"\\?\C:\x");
        raw.push(0xD800);
        assert!(WidePath::from_final(&raw).is_ok());
    }

    #[test]
    fn from_known_value_table() {
        use PathReject as R;
        let ok: &[(&str, &str)] = &[
            (r"C:\Users\u", r"\\?\C:\Users\u"),
            (r"c:\Users\u\", r"\\?\C:\Users\u"),
            (r"\\?\C:\Users\u", r"\\?\C:\Users\u"),
            (r"C:\x", r"\\?\C:\x"),
            // Lenient on 8.3: the real %TEMP% is short-form on CI runners.
            (
                r"C:\Users\RUNNER~1\AppData\Local\Temp",
                r"\\?\C:\Users\RUNNER~1\AppData\Local\Temp",
            ),
        ];
        for &(raw, want) in ok {
            let p = WidePath::from_known_value(&u(raw)).unwrap();
            assert_eq!(p.as_wide(), &u(want)[..], "{raw:?}");
            assert_invariant(&p);
        }
        let bad: &[(&str, PathReject)] = &[
            ("", R::Empty),
            ("C:", R::DriveRoot),
            (r"C:\", R::DriveRoot),
            (r"C:\\", R::DriveRoot),
            ("C:/", R::InvalidChar(SLASH)),
            (r"\\?\C:\", R::DriveRoot),
            (r"\\?\C:", R::DriveRoot),
            (r"\\?\UNC\s\x", R::Unc),
            (r"\\?\Volume{x}\y", R::DevicePrefix),
            (r"\\.\C:\x", R::DevicePrefix),
            (r"\\.\PhysicalDrive0", R::DevicePrefix),
            (r"\??\C:\x", R::DevicePrefix),
            (r"\\server\share", R::Unc),
            ("//server/share", R::Unc),
            (r"\Temp", R::RootedNoDrive),
            ("/Temp", R::RootedNoDrive),
            ("C:Temp", R::DriveRelative),
            ("Temp", R::NoLeadingToken),
            ("    ", R::NoLeadingToken),
            (r"C:\a\..\b", R::ParentTraversal),
            (r"C:\a\.", R::CurrentDirComponent),
            (r"C:\a\\b", R::EmptyComponent),
            (r"C:\a\b\\", R::EmptyComponent),
            (r"C:\a/b", R::InvalidChar(SLASH)),
            (r"C:\a*", R::Wildcard),
            (r"C:\a|b", R::InvalidChar(u16::from(b'|'))),
            (r"C:\a:b", R::AlternateStream),
            ("C:\\a\0", R::NulChar),
        ];
        for (raw, want) in bad {
            assert_eq!(
                WidePath::from_known_value(&u(raw)),
                Err(want.clone()),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn pushes() {
        let mut p = WidePath::from_known_value(&u(r"C:\Users\u")).unwrap();
        p.push_template_component(&u("AppData")).unwrap();
        assert_eq!(
            p.push_template_component(&u("..")),
            Err(PathReject::ParentTraversal)
        );
        assert_eq!(
            p.push_template_component(&u("a~1")),
            Err(PathReject::ShortName)
        );
        assert_eq!(
            p.push_template_component(&u(r"a\b")),
            Err(PathReject::InvalidChar(BACKSLASH))
        );
        p.push_listed_name(&u("CON")).unwrap();
        p.push_listed_name(&u("x.")).unwrap();
        assert_eq!(
            p.push_listed_name(&u("a:b")),
            Err(PathReject::AlternateStream)
        );
        assert_eq!(p.push_listed_name(&u("")), Err(PathReject::EmptyComponent));
        assert_eq!(p.display_string(), r"C:\Users\u\AppData\CON\x.");
        assert_invariant(&p);

        let mut root = WidePath::from_final(&u(r"\\?\D:\")).unwrap();
        root.push_listed_name(&u("x")).unwrap();
        assert_eq!(root.as_wide(), &u(r"\\?\D:\x")[..]);
        assert_eq!(root.drive_letter(), u16::from(b'D'));

        // Total length is capped.
        let mut big = WidePath::from_known_value(&u(r"C:\x")).unwrap();
        let comp = u(&"a".repeat(255));
        let mut res = Ok(());
        for _ in 0..200 {
            res = big.push_listed_name(&comp);
            if res.is_err() {
                break;
            }
        }
        assert_eq!(res, Err(PathReject::PathTooLong));
        assert!(big.as_wide().len() <= MAX_TOTAL);
        assert_invariant(&big);
    }

    #[test]
    fn comparisons() {
        let base = w(r"\\?\C:\Users\u\AppData\Local");
        let below = w(r"\\?\C:\Users\u\AppData\Local\Temp");
        let sibling = w(r"\\?\C:\Users\u\AppData\LocalLow");
        let upper = w(r"\\?\C:\USERS\U\APPDATA\LOCAL");
        assert!(base.eq_strict(&upper));
        assert!(!base.eq_strict(&below));
        assert!(below.is_same_or_below(&base, Cmp::Strict));
        assert!(below.is_strictly_below(&base, Cmp::Strict));
        assert!(base.is_same_or_below(&base, Cmp::Strict));
        assert!(!base.is_strictly_below(&base, Cmp::Strict));
        assert!(!upper.is_strictly_below(&base, Cmp::Strict));
        assert!(!sibling.is_same_or_below(&base, Cmp::Strict));
        assert!(!base.is_same_or_below(&below, Cmp::Strict));
        let drive = w(r"\\?\C:\");
        assert!(below.is_strictly_below(&drive, Cmp::Strict));
        assert!(!w(r"\\?\D:\x").is_same_or_below(&drive, Cmp::Strict));
        // Loose: İ folds to i only in loose mode.
        let win = w(r"\\?\C:\Windows");
        let tricky = w(r"\\?\C:\WİNDOWS");
        assert!(!tricky.eq_strict(&win));
        assert!(tricky.is_same_or_below(&win, Cmp::Loose));
        assert!(!tricky.is_strictly_below(&win, Cmp::Loose));
        assert!(w(r"\\?\C:\WİNDOWS\System32").is_strictly_below(&win, Cmp::Loose));
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        fn arb_raw() -> impl Strategy<Value = Vec<u16>> {
            let unit = prop_oneof![
                4 => Just(BACKSLASH),
                1 => Just(SLASH),
                1 => Just(COLON),
                2 => Just(DOT),
                1 => Just(QUESTION),
                1 => Just(u16::from(b'C')),
                1 => Just(u16::from(b'U')),
                1 => Just(u16::from(b'N')),
                3 => Just(u16::from(b'a')),
                1 => Just(u16::from(b'~')),
                1 => Just(u16::from(b'1')),
                1 => Just(0u16),
                1 => Just(0xD800u16),
                1 => any::<u16>(),
            ];
            let prefix = prop_oneof![
                Just(String::new()),
                Just(r"C:\".to_owned()),
                Just(r"\\?\C:\".to_owned()),
                Just(r"\\?\".to_owned()),
                Just(r"\\?\UNC\".to_owned()),
            ];
            (prefix, proptest::collection::vec(unit, 0..24)).prop_map(|(p, rest)| {
                let mut v: Vec<u16> = p.encode_utf16().collect();
                v.extend(rest);
                v
            })
        }

        proptest! {
            /// Constructors never panic and every value they return satisfies the invariant.
            #[test]
            fn constructors_uphold_invariant(raw in arb_raw()) {
                if let Ok(p) = WidePath::from_final(&raw) {
                    assert_invariant(&p);
                    // Re-parsing the stored form is the identity.
                    prop_assert_eq!(WidePath::from_final(p.as_wide()), Ok(p.clone()));
                    if let Some(parent) = p.parent() {
                        assert_invariant(&parent);
                        prop_assert!(p.is_strictly_below(&parent, Cmp::Strict));
                    }
                }
                if let Ok(p) = WidePath::from_known_value(&raw) {
                    assert_invariant(&p);
                    prop_assert!(!p.is_drive_root());
                    prop_assert_eq!(WidePath::from_known_value(p.as_wide()), Ok(p.clone()));
                }
            }
        }
    }
}
