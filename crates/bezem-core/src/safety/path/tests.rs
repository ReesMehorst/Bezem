// SPDX-License-Identifier: MIT

use super::*;
use std::collections::HashSet;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn p(s: &str) -> WidePath {
    WidePath::parse_extended(&w(s)).unwrap()
}

fn c(s: &str) -> PathComponent {
    PathComponent::new(s).unwrap()
}

#[test]
fn component_accepts_literal_names() {
    for name in [
        "Temp",
        "a b",
        "Windows.",
        "Windows ",
        "...",
        "con",
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

#[test]
fn component_rejects_adversarial_names() {
    use PathError::*;
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

#[test]
fn parse_rejects_adversarial_paths() {
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
    assert!(WidePath::drive_root('1').is_err());
    assert!(WidePath::drive_root('é').is_err());
}

#[test]
fn push_enforces_total_length_and_leaves_path_unchanged() {
    let name = c(&"x".repeat(MAX_COMPONENT_UNITS));
    let mut x = WidePath::drive_root('C').unwrap();
    while x.push(name.clone()).is_ok() {}
    assert!(x.to_wide().len() <= MAX_PATH_UNITS);
    let before = x.clone();
    assert_eq!(x.push(name).unwrap_err(), PathError::PathTooLong);
    assert_eq!(x.components().len(), before.components().len());
    let mut fill = x.clone();
    while fill.push(c("y")).is_ok() {}
    // Filled up to the limit: a further "\y" (two units) no longer fits, the limit itself does.
    let len = fill.to_wide().len();
    assert!(len <= MAX_PATH_UNITS && len + 2 > MAX_PATH_UNITS, "{len}");
}

#[test]
fn parent_and_drive_root() {
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
fn hash_agrees_with_equality() {
    let set: HashSet<WidePath> = [p(r"\\?\C:\Windows\System32")].into_iter().collect();
    assert!(set.contains(&p(r"\\?\c:\WINDOWS\system32")));
    assert!(!set.contains(&p(r"\\?\C:\Windows")));
}

#[test]
fn containment_is_component_wise_not_string_prefix() {
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

#[test]
fn strip_prefix_returns_the_relative_components() {
    let root = p(r"\\?\C:\Users\Ann\AppData\Local\Temp");
    let item = p(r"\\?\c:\users\ann\appdata\local\temp\Sub\file.tmp");
    let rel = item.strip_prefix(&root).unwrap();
    assert_eq!(rel, &[c("Sub"), c("file.tmp")]);
    assert_eq!(root.join(rel).unwrap(), item);
    assert_eq!(root.strip_prefix(&root).unwrap(), &[] as &[PathComponent]);
    assert_eq!(root.strip_prefix(&item), None);
}
