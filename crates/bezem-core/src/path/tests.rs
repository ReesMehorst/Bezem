// SPDX-License-Identifier: MIT

//! Tests named `inv1_` / `inv5_` cover ARCHITECTURE.md section 8.2 invariant 1 (nothing outside a
//! verified root: containment is component-wise) and invariant 5 (a failed check rejects, with no
//! repair or fallback).

use super::*;
use std::collections::HashSet;
use PathError::*;

#[test]
fn file_identity_compares_both_fields() {
    let a = FileIdentity {
        volume_serial: 7,
        file_id: [1; 16],
    };
    let b = a; // Copy
    assert_eq!(a, b);
    assert_ne!(
        a,
        FileIdentity {
            volume_serial: 8,
            ..a
        }
    );
    assert_ne!(
        a,
        FileIdentity {
            file_id: [2; 16],
            ..a
        }
    );
    let set: HashSet<FileIdentity> = [a].into_iter().collect();
    assert!(set.contains(&b));
    assert!(!format!("{a:?}").is_empty());
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn p(s: &str) -> WidePath {
    WidePath::parse_extended(&w(s)).unwrap()
}

fn c(s: &str) -> PathComponent {
    PathComponent::new(s).unwrap()
}

/// A path on `C:` whose `to_wide()` is exactly `n` units long (`n >= 8`).
fn path_of_len(n: usize) -> WidePath {
    let mut x = WidePath::drive_root('C').unwrap();
    loop {
        let sep = usize::from(!x.components().is_empty());
        let avail = n - x.to_wide().len() - sep;
        if avail <= MAX_COMPONENT_UNITS {
            x.push(c(&"x".repeat(avail))).unwrap();
            return x;
        }
        // Leave at least one unit for the next name after its separator.
        x.push(c(&"x".repeat(MAX_COMPONENT_UNITS.min(avail - 2))))
            .unwrap();
    }
}

#[test]
fn component_accepts_literal_names() {
    // Trailing dots/spaces, a lone space and device names are only special to Win32 path parsing.
    // They are safe solely because Bezem uses them under `\\?\` or handle-relative opens, which
    // never normalize them (ARCHITECTURE.md 8.1); Recycle mode skips them (8.5).
    for name in [
        "Temp",
        "a b",
        "Windows.",
        "Windows ",
        "...",
        ". ",
        ".. ",
        " ",
        "con",
        "CON",
        "NUL",
        "COM1",
        "aux.txt",
        "$I30",
        "Ünïcödé",
    ] {
        assert_eq!(c(name).to_string(), name);
    }
    assert!(PathComponent::new(&"x".repeat(MAX_COMPONENT_UNITS)).is_ok());
    // Unpaired surrogates are legal NTFS names and must round-trip unchanged.
    let odd = [0x61, 0xD800, 0x62];
    assert_eq!(PathComponent::from_wide(&odd).unwrap().as_wide(), &odd);
}

/// Invariant 5
#[test]
fn inv5_component_rejects_adversarial_names() {
    let cases: &[(&str, PathError)] = &[
        ("", EmptyComponent),
        (".", DotComponent),
        ("..", DotComponent),
        (r"a\b", ForbiddenUnit(u16::from(b'\\'))),
        ("a/b", ForbiddenUnit(u16::from(b'/'))),
        ("/", ForbiddenUnit(u16::from(b'/'))),
        ("file.txt:stream", ForbiddenUnit(u16::from(b':'))),
        ("file.txt::$DATA", ForbiddenUnit(u16::from(b':'))),
        ("C:", ForbiddenUnit(u16::from(b':'))),
        ("a\0b", ForbiddenUnit(0)),
        ("\0", ForbiddenUnit(0)),
        ("a\u{1}", ForbiddenUnit(1)),
        ("tab\t", ForbiddenUnit(9)),
        ("*", ForbiddenUnit(u16::from(b'*'))),
        ("a?b", ForbiddenUnit(u16::from(b'?'))),
        ("<", ForbiddenUnit(u16::from(b'<'))),
        (">", ForbiddenUnit(u16::from(b'>'))),
        ("\"", ForbiddenUnit(u16::from(b'"'))),
        ("|", ForbiddenUnit(u16::from(b'|'))),
        (r"..\..", ForbiddenUnit(u16::from(b'\\'))),
    ];
    for (name, err) in cases {
        assert_eq!(PathComponent::new(name).unwrap_err(), *err, "{name:?}");
    }
    let long = "x".repeat(MAX_COMPONENT_UNITS + 1);
    assert_eq!(PathComponent::new(&long).unwrap_err(), ComponentTooLong);
}

/// Invariant 5
#[test]
fn inv5_forbidden_unit_boundaries() {
    // The control range ends at U+001F; U+0020 (space) and U+007F (DEL) are outside the rule and
    // accepted literally, like any other name NTFS can store.
    assert_eq!(
        PathComponent::from_wide(&[0x1F]).unwrap_err(),
        ForbiddenUnit(0x1F)
    );
    assert_eq!(
        PathComponent::new("a\u{1F}b").unwrap_err(),
        ForbiddenUnit(0x1F)
    );
    for ok in [" ", "  ", "a b", "\u{7F}", "a\u{7F}"] {
        assert!(PathComponent::new(ok).is_ok(), "{ok:?}");
    }
}

#[test]
fn parse_accepts_only_extended_drive_form() {
    assert_eq!(p(r"\\?\C:\"), WidePath::drive_root('C').unwrap());
    let x = p(r"\\?\c:\Users\Ann\AppData\Local\Temp");
    assert_eq!(x.drive(), 'C');
    assert_eq!(x.components().len(), 5);
    assert_eq!(x.to_string(), r"\\?\C:\Users\Ann\AppData\Local\Temp");
    assert_eq!(x.to_wide(), w(r"\\?\C:\Users\Ann\AppData\Local\Temp"));
    assert_eq!(WidePath::parse_extended(&x.to_wide()).unwrap(), x);
}

/// Invariant 5
#[test]
fn inv5_parse_rejects_adversarial_paths() {
    for s in [
        "",
        " ",
        ".",
        "C:",
        r"C:\",
        "C:/",
        r"C:\Windows",
        r"\Temp",
        r"%BEZEM_UNSET%\foo",
        r"\\?\",
        r"\\?\C:",
        r"\\?\C:Windows",
        r"\\?\1:\",
        r"\\?\CC:\",
        r"\\?\é:\",
        r"\\?\Ｃ:\",
        r"\\.\C:\",
        r"\\.\PhysicalDrive0",
        r"\??\C:\",
        r"\\?\UNC\server\share",
        r"\\server\share\x",
        r"\\?\GLOBALROOT\Device\HarddiskVolume1\x",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\x",
        "//?/C:/Windows",
        r"\\?\C:/Windows",
        r"\\?\C:\Windows\",
        r"\\?\C:\\Windows",
        r"\\?\C:\a\\b",
        r"\\?\C:\Temp\..\Windows",
        r"\\?\C:\Temp\..\..",
        r"\\?\C:\.\Windows",
        r"\\?\C:\Temp/../Windows",
        r"\\?\C:\a\file.txt:stream",
        "\\\\?\\C:\\a\0",
        "\\\\?\\C:\\Temp\0\\Windows",
    ] {
        assert!(
            WidePath::parse_extended(&w(s)).is_err(),
            "{s:?} must be rejected"
        );
    }
}

#[test]
fn push_builds_the_same_path_as_parse() {
    let mut x = WidePath::drive_root('c').unwrap();
    for name in ["Windows", "Temp"] {
        x.push(c(name)).unwrap();
    }
    assert_eq!(x, p(r"\\?\C:\Windows\Temp"));
    assert_eq!(x.to_wide(), w(r"\\?\C:\Windows\Temp"));
    let y = WidePath::drive_root('C')
        .unwrap()
        .join(&[c("Windows"), c("Temp")])
        .unwrap();
    assert_eq!(x, y);
    for bad in ['1', 'é', '\u{FF23}' /* fullwidth C */, ':'] {
        assert_eq!(WidePath::drive_root(bad).unwrap_err(), NotExtendedDrivePath);
    }
}

/// Invariant 5
#[test]
fn inv5_push_enforces_total_length_and_leaves_path_unchanged() {
    let name = c(&"x".repeat(MAX_COMPONENT_UNITS));
    let mut x = WidePath::drive_root('C').unwrap();
    while x.push(name.clone()).is_ok() {}
    assert!(x.to_wide().len() <= MAX_PATH_UNITS);
    let before = x.clone();
    assert_eq!(x.push(name).unwrap_err(), PathTooLong);
    assert!(x == before && x.to_wide() == before.to_wide());
}

/// Invariant 5
#[test]
fn inv5_length_limit_is_exact_for_push_parse_and_join() {
    let max = path_of_len(MAX_PATH_UNITS);
    assert_eq!(max.to_wide().len(), MAX_PATH_UNITS);
    assert_eq!(WidePath::parse_extended(&max.to_wide()).unwrap(), max);

    let mut exact = path_of_len(MAX_PATH_UNITS - 2);
    exact.push(c("y")).unwrap(); // "\y": exactly MAX_PATH_UNITS
    assert_eq!(exact.to_wide().len(), MAX_PATH_UNITS);

    let mut over = path_of_len(MAX_PATH_UNITS - 1);
    let before = over.clone();
    assert_eq!(over.push(c("y")).unwrap_err(), PathTooLong); // would be MAX_PATH_UNITS + 1
    assert!(over == before && over.to_wide() == before.to_wide());

    let mut wide = before.to_wide();
    wide.extend(w(r"\y"));
    assert_eq!(wide.len(), MAX_PATH_UNITS + 1);
    assert_eq!(WidePath::parse_extended(&wide).unwrap_err(), PathTooLong);

    // `join` failing partway: "\a" still fits, "\bc" does not; the original is untouched.
    let base = path_of_len(MAX_PATH_UNITS - 3);
    let before = base.clone();
    assert_eq!(base.join(&[c("a"), c("bc")]).unwrap_err(), PathTooLong);
    assert!(base == before && base.to_wide() == before.to_wide());
}

/// Invariant 1
#[test]
fn inv1_parent_and_drive_root() {
    let x = p(r"\\?\C:\Windows\Temp");
    assert!(!x.is_drive_root());
    assert_eq!(x.parent().unwrap(), p(r"\\?\C:\Windows"));
    assert!(p(r"\\?\C:\").is_drive_root());
    assert_eq!(p(r"\\?\C:\").parent(), None);
}

#[test]
fn equality_is_ordinal_case_insensitive() {
    assert_eq!(p(r"\\?\C:\WINDOWS\System32"), p(r"\\?\c:\windows\system32"));
    assert_eq!(c("Ärger"), c("äRGER"));
    assert_eq!(c("σ"), c("Σ"));
    assert_eq!(c("ᾀ"), c("ᾈ")); // simple mapping of iota-subscript letters
    assert_eq!(c("ÿ"), c("Ÿ"));
    // Known lexical over-folds vs. older NTFS upcase tables (documented in the module docs).
    assert_eq!(c("\u{10D0}"), c("\u{1C90}")); // Georgian Mkhedruli / Mtavruli
    assert_eq!(c("\u{1C80}"), c("\u{0412}")); // Cyrillic rounded ve / VE
                                              // No linguistic or multi-character folding, no non-ASCII onto ASCII.
    for (a, b) in [
        ("İ", "i"),
        ("İ", "I"),
        ("ı", "I"),
        ("ı", "i"),
        ("ß", "SS"),
        ("ß", "ss"),
        ("ß", "ẞ"),
        ("ﬀ", "FF"),
        ("ſ", "S"),
        ("\u{212A}", "K"), // Kelvin sign
        ("𐐨", "𐐀"),        // outside the BMP: surrogate units never fold
        ("e\u{301}", "é"), // no normalization
    ] {
        assert_ne!(c(a), c(b), "{a:?} vs {b:?}");
    }
    // Lookalikes and short names never alias a real name by string comparison.
    assert_ne!(p(r"\\?\C:\Windows."), p(r"\\?\C:\Windows"));
    assert_ne!(p(r"\\?\C:\Windows "), p(r"\\?\C:\Windows"));
    assert_ne!(p(r"\\?\C:\PROGRA~1"), p(r"\\?\C:\Program Files"));
    assert_ne!(p(r"\\?\C:\Windows"), p(r"\\?\D:\Windows"));
}

#[test]
fn lone_surrogates_round_trip_and_stay_distinct() {
    let mut wide = w(r"\\?\C:\a");
    wide.push(0xD800);
    wide.extend(w(r"\b"));
    wide.push(0xDC00);
    let x = WidePath::parse_extended(&wide).unwrap();
    assert_eq!(x.to_wide(), wide);
    assert_eq!(x.to_string(), "\\\\?\\C:\\a\u{FFFD}\\b\u{FFFD}");
    let hi = PathComponent::from_wide(&[0xD800]).unwrap();
    let lo = PathComponent::from_wide(&[0xDC00]).unwrap();
    assert_ne!(hi, lo);
    assert_eq!(hi.to_string(), "\u{FFFD}");
}

#[test]
fn hash_agrees_with_equality() {
    let set: HashSet<WidePath> = [p(r"\\?\C:\Windows\System32")].into_iter().collect();
    assert!(set.contains(&p(r"\\?\c:\WINDOWS\system32")));
    assert!(!set.contains(&p(r"\\?\C:\Windows")));
    let names: HashSet<PathComponent> = [c("Ärger")].into_iter().collect();
    assert!(names.contains(&c("äRGER")));
    assert!(!names.contains(&c("Arger")));
}

/// Invariant 1
#[test]
fn inv1_containment_is_component_wise_not_string_prefix() {
    let windows = p(r"\\?\C:\Windows");
    assert!(p(r"\\?\C:\WINDOWS\System32").starts_with(&windows));
    assert!(p(r"\\?\C:\Windows\System32").is_descendant_of(&windows));
    assert!(windows.starts_with(&windows));
    assert!(!windows.is_descendant_of(&windows));
    for other in [
        r"\\?\C:\WindowsApps\x",
        r"\\?\C:\Windows.old\System32",
        r"\\?\C:\Windows \x",
        r"\\?\C:\Window",
        r"\\?\D:\Windows\System32",
        r"\\?\C:\",
    ] {
        assert!(!p(other).starts_with(&windows), "{other:?}");
        assert!(!p(other).is_descendant_of(&windows), "{other:?}");
    }
    let ann = p(r"\\?\C:\Users\Ann");
    assert!(!p(r"\\?\C:\Users\Ann.Old\AppData").starts_with(&ann));
    assert!(!p(r"\\?\C:\Users\Anna").starts_with(&ann));
    // Every path on a drive is below its root; a root is not below itself.
    let root = p(r"\\?\C:\");
    assert!(windows.is_descendant_of(&root));
    assert!(!root.is_descendant_of(&root));
    assert!(!p(r"\\?\D:\").starts_with(&root));
}

/// Invariant 1
#[test]
fn inv1_strip_prefix_returns_the_relative_components() {
    let root = p(r"\\?\C:\Users\Ann\AppData\Local\Temp");
    let item = p(r"\\?\c:\users\ann\appdata\local\temp\Sub\file.tmp");
    let rel = item.strip_prefix(&root).unwrap();
    assert_eq!(rel, &[c("Sub"), c("file.tmp")]);
    assert_eq!(root.join(rel).unwrap(), item);
    assert_eq!(root.strip_prefix(&root).unwrap(), &[] as &[PathComponent]);
    assert_eq!(root.strip_prefix(&item), None);
}

#[test]
fn upcase_is_idempotent_ascii_closed_and_keeps_surrogates() {
    let surrogate = |u: u16| (0xD800..=0xDFFF).contains(&u);
    for u in 0..=u16::MAX {
        let up = upcase(u);
        assert_eq!(upcase(up), up, "not idempotent at U+{u:04X}");
        assert_eq!(u < 0x80, up < 0x80, "U+{u:04X} -> U+{up:04X} crosses ASCII");
        assert_eq!(surrogate(u), surrogate(up), "U+{u:04X} -> U+{up:04X}");
        if surrogate(u) {
            assert_eq!(up, u);
        }
    }
}

#[test]
fn errors_and_debug_render() {
    for e in [
        EmptyComponent,
        DotComponent,
        ForbiddenUnit(0x3A),
        ComponentTooLong,
        PathTooLong,
        NotExtendedDrivePath,
    ] {
        let err: &dyn std::error::Error = &e;
        assert!(!err.to_string().is_empty());
        assert!(err.source().is_none());
        assert!(!format!("{e:?}").is_empty());
    }
    assert_eq!(
        ForbiddenUnit(0x3A).to_string(),
        "forbidden character U+003A in path component"
    );
    assert_eq!(format!("{:?}", c("Temp")), r#""Temp""#);
    assert_eq!(format!("{:?}", p(r"\\?\C:\Temp")), r#""\\\\?\\C:\\Temp""#);
}
