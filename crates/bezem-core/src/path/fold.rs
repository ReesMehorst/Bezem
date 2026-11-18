// SPDX-License-Identifier: MIT

//! Case-folding comparisons on UTF-16 paths.
//!
//! Ownership: work package **B**.
//!
//! - [`Cmp::Strict`]: ASCII-only case fold, everything else exact. Fail-safe because it can only
//!   under-match; OS final paths carry the on-disk casing. Used for allowlist containment.
//! - [`Cmp::Loose`]: full Unicode fold both ways (`char::to_uppercase().flat_map(to_lowercase)`)
//!   plus U+0130/U+0131 -> `i`, U+212A -> `k`, U+017F -> `s`, `ß` -> `ss`. Over-matches, so it is
//!   used ONLY for protected/forbidden checks, where it can only reject more.
//!
//! Lone surrogates never fold: they compare equal only to the identical code unit.

const BACKSLASH: u16 = b'\\' as u16;
const BACKSLASH_FOLDED: u32 = b'\\' as u32;

/// Which case-folding rule a comparison uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Strict,
    Loose,
}

/// Whole-string equality under `c`.
pub fn eq_wide(a: &[u16], b: &[u16], c: Cmp) -> bool {
    match c {
        Cmp::Strict => a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| ascii_eq(x, y)),
        Cmp::Loose => loose_fold(a) == loose_fold(b),
    }
}

/// `path` equals `prefix` or continues with `\` right after it (component-boundary prefix),
/// under `c`. A `prefix` that itself ends with `\` (a drive root `\\?\X:\`) matches any `path`
/// that starts with it. An empty `prefix` matches nothing.
pub fn starts_with_at_boundary(path: &[u16], prefix: &[u16], c: Cmp) -> bool {
    if prefix.is_empty() {
        return false;
    }
    match c {
        Cmp::Strict => {
            let Some(head) = path.get(..prefix.len()) else {
                return false;
            };
            if !eq_wide(head, prefix, Cmp::Strict) {
                return false;
            }
            match path.get(prefix.len()) {
                None => true,
                Some(&next) => next == BACKSLASH || prefix.last() == Some(&BACKSLASH),
            }
        }
        Cmp::Loose => {
            let p = loose_fold(path);
            let q = loose_fold(prefix);
            let Some(head) = p.get(..q.len()) else {
                return false;
            };
            if head != q.as_slice() {
                return false;
            }
            match p.get(q.len()) {
                None => true,
                Some(&next) => next == BACKSLASH_FOLDED || q.last() == Some(&BACKSLASH_FOLDED),
            }
        }
    }
}

fn ascii_eq(x: u16, y: u16) -> bool {
    ascii_lower(x) == ascii_lower(y)
}

fn ascii_lower(u: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&u) {
        u + 32
    } else {
        u
    }
}

/// Folds UTF-16 into a sequence of `u32`: scalar values after the loose fold, and lone
/// surrogates as their raw unit (surrogate values are never scalar values, so they cannot
/// collide with a folded character).
fn loose_fold(s: &[u16]) -> Vec<u32> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while let Some(&u) = s.get(i) {
        let decoded = char::decode_utf16(s.get(i..).unwrap_or(&[]).iter().copied()).next();
        match decoded {
            Some(Ok(ch)) => {
                fold_char(ch, &mut out);
                i += ch.len_utf16();
            }
            _ => {
                out.push(u32::from(u));
                i += 1;
            }
        }
    }
    out
}

fn fold_char(ch: char, out: &mut Vec<u32>) {
    match ch {
        // Turkish dotted/dotless I, Kelvin sign, long s, sharp s (both cases).
        '\u{0130}' | '\u{0131}' => out.push(u32::from('i')),
        '\u{212A}' => out.push(u32::from('k')),
        '\u{017F}' => out.push(u32::from('s')),
        '\u{00DF}' | '\u{1E9E}' => {
            out.push(u32::from('s'));
            out.push(u32::from('s'));
        }
        _ => {
            for up in ch.to_uppercase() {
                for low in up.to_lowercase() {
                    out.push(u32::from(low));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn strict_is_ascii_only() {
        assert!(eq_wide(&w("C:\\Windows"), &w("c:\\WINDOWS"), Cmp::Strict));
        assert!(!eq_wide(&w("abc"), &w("abcd"), Cmp::Strict));
        assert!(!eq_wide(&w("é"), &w("É"), Cmp::Strict));
        assert!(eq_wide(&w("é"), &w("é"), Cmp::Strict));
        // '@' and '`' sit next to the letter ranges and must not fold.
        assert!(!eq_wide(&w("@"), &w("`"), Cmp::Strict));
        assert!(!eq_wide(&w("["), &w("{"), Cmp::Strict));
    }

    #[test]
    fn q_case_folding_table() {
        // (a, b, strict, loose)
        let cases: &[(&str, &str, bool, bool)] = &[
            ("İ", "i", false, true),
            ("ı", "I", false, true),
            ("ß", "SS", false, true),
            ("ß", "ss", false, true),
            ("ẞ", "ss", false, true),
            ("\u{212A}", "k", false, true),
            ("\u{212A}", "K", false, true),
            ("\u{017F}", "S", false, true),
            ("Straße", "STRASSE", false, true),
            ("C:\\WİNDOWS", "C:\\Windows", false, true),
            ("Ä", "ä", false, true),
            ("Ω", "ω", false, true),
            ("a", "b", false, false),
            ("ab", "a", false, false),
        ];
        for &(a, b, strict, loose) in cases {
            assert_eq!(
                eq_wide(&w(a), &w(b), Cmp::Strict),
                strict,
                "strict {a:?} {b:?}"
            );
            assert_eq!(
                eq_wide(&w(a), &w(b), Cmp::Loose),
                loose,
                "loose {a:?} {b:?}"
            );
        }
    }

    #[test]
    fn lone_surrogates_compare_exactly() {
        let a = vec![0xD800u16, u16::from(b'x')];
        let b = vec![0xD800u16, u16::from(b'X')];
        let c = vec![0xDC00u16, u16::from(b'x')];
        assert!(eq_wide(&a, &b, Cmp::Strict));
        assert!(eq_wide(&a, &b, Cmp::Loose));
        assert!(!eq_wide(&a, &c, Cmp::Loose));
        // A lone surrogate never equals the replacement character.
        assert!(!eq_wide(&[0xD800], &w("\u{FFFD}"), Cmp::Loose));
        // A valid pair decodes and folds as one character.
        assert!(eq_wide(&w("𐐀"), &w("𐐨"), Cmp::Loose));
    }

    #[test]
    fn boundary_prefix() {
        let cases: &[(&str, &str, bool, bool)] = &[
            (r"\\?\C:\Users\u", r"\\?\C:\Users", true, true),
            (r"\\?\C:\Users", r"\\?\C:\Users", true, true),
            (r"\\?\C:\Users2", r"\\?\C:\Users", false, false),
            (r"\\?\C:\Use", r"\\?\C:\Users", false, false),
            (r"\\?\C:\x", r"\\?\C:\", true, true),
            (r"\\?\C:\", r"\\?\C:\", true, true),
            (r"\\?\c:\users\U", r"\\?\C:\USERS", true, true),
            (r"\\?\C:\WİNDOWS\System32", r"\\?\C:\Windows", false, true),
            (r"\\?\C:\WİNDOWSX", r"\\?\C:\Windows", false, false),
            (r"\\?\C:\Straße\x", r"\\?\C:\STRASSE", false, true),
            (r"x", "", false, false),
        ];
        for &(p, q, strict, loose) in cases {
            assert_eq!(
                starts_with_at_boundary(&w(p), &w(q), Cmp::Strict),
                strict,
                "strict {p:?} {q:?}"
            );
            assert_eq!(
                starts_with_at_boundary(&w(p), &w(q), Cmp::Loose),
                loose,
                "loose {p:?} {q:?}"
            );
        }
    }

    fn arb_wide() -> impl Strategy<Value = Vec<u16>> {
        let unit = prop_oneof![
            Just(u16::from(b'\\')),
            Just(u16::from(b'a')),
            Just(u16::from(b'A')),
            Just(u16::from(b's')),
            Just(0x00DFu16),
            Just(0x0130u16),
            Just(0x212Au16),
            Just(0xD800u16),
            any::<u16>(),
        ];
        proptest::collection::vec(unit, 0..12)
    }

    proptest! {
        /// Loose only ever over-matches Strict (it is used solely to reject more).
        #[test]
        fn strict_implies_loose(a in arb_wide(), b in arb_wide()) {
            if eq_wide(&a, &b, Cmp::Strict) {
                prop_assert!(eq_wide(&a, &b, Cmp::Loose));
            }
            if starts_with_at_boundary(&a, &b, Cmp::Strict) {
                prop_assert!(starts_with_at_boundary(&a, &b, Cmp::Loose));
            }
        }

        #[test]
        fn reflexive(a in arb_wide()) {
            prop_assert!(eq_wide(&a, &a, Cmp::Strict));
            prop_assert!(eq_wide(&a, &a, Cmp::Loose));
            prop_assert_eq!(starts_with_at_boundary(&a, &a, Cmp::Strict), !a.is_empty());
        }
    }
}
