// SPDX-License-Identifier: MIT

//! Validated path types: [`PathComponent`] and [`WidePath`] (ARCHITECTURE.md sections 7.1, 8.3, 8.4).
//!
//! Pure logic over UTF-16 code units, so it builds and is tested on every platform.
//!
//! - A [`WidePath`] is always an absolute extended-length drive path, `\\?\X:\a\b`. It is built by
//!   pushing validated components onto a drive root. The only parser, [`WidePath::parse_extended`],
//!   is for API output (`GetFinalPathNameByHandleW`) and accepts that one form, nothing else: no
//!   `\\?\UNC\`, `\\.\`, `\??\`, `\\?\GLOBALROOT`, `\\?\Volume{..}`, relative or Win32 `C:\` paths.
//! - Names are stored as UTF-16 code units, so names that are not valid Unicode (unpaired
//!   surrogates) round-trip exactly and are never altered by a lossy conversion.
//!
//! # Comparison and case folding
//!
//! `==`, [`Hash`], [`WidePath::starts_with`] and friends are **ordinal and case-insensitive**, with
//! the semantics of `CompareStringOrdinal(.., bIgnoreCase = TRUE)` that section 8.3 prescribes and
//! that NTFS uses through its upcase table: each UTF-16 code unit is uppercased on its own, with no
//! locale and no normalization. The mapping used here is:
//!
//! - ASCII `a-z` to `A-Z`;
//! - any other BMP unit to its Unicode *simple* uppercase mapping when that is a single non-ASCII
//!   BMP character; otherwise the unit is unchanged. So `ß` is not `SS` or `ẞ`, Turkish `İ` is not
//!   `i`, and dotless `ı` / long `ſ` do not fold onto ASCII `I` / `S` (Windows does not fold them);
//! - surrogate units are unchanged, so characters outside the BMP never fold.
//!
//! This equality is **lexical**: an ordinal per-unit uppercase using the Unicode tables of the
//! pinned Rust toolchain, not a statement about what Windows considers the same file. It may fold
//! pairs that an older NTFS `$UpCase` table keeps apart (for example U+1C80 and U+0412, or Georgian
//! Mtavruli U+1C90 and Mkhedruli U+10D0), and it ignores per-directory case sensitivity. Over-folding
//! *loosens* base containment (section 8.3 step 7) and the final-path tripwire (section 8.4 step 8)
//! and *tightens* protected-path checks (section 8.3 step 8). It is therefore never the sole
//! containment guard: handle-relative opens below a verified root and file identity (volume serial
//! and file ID) are. In practice the compared paths are `FILE_NAME_NORMALIZED` final paths from the
//! same volume, where one directory yields identical code units. A test that cross-checks this
//! folding against `CompareStringOrdinal` on Windows is TODO for the `win32` layer (it needs FFI,
//! which may only live there). Globs use Unicode simple case folding instead (section 7.3).

use std::fmt;
use std::hash::{Hash, Hasher};

/// Longest name NTFS accepts, in UTF-16 code units.
pub const MAX_COMPONENT_UNITS: usize = 255;
/// Longest extended-length path, in UTF-16 code units, leaving room for the NUL terminator.
pub const MAX_PATH_UNITS: usize = 32_766;

const PREFIX: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
const SEP: u16 = b'\\' as u16;

/// Why a path or component was rejected. Every rejection is final: there is no repair or fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// Empty component (also covers `\\` and a trailing `\`).
    EmptyComponent,
    /// `.` or `..`.
    DotComponent,
    /// A separator, `:`, NUL, other control character or Win32 wildcard (`* ? < > " |`) in a name.
    ForbiddenUnit(u16),
    /// Component longer than [`MAX_COMPONENT_UNITS`].
    ComponentTooLong,
    /// Path longer than [`MAX_PATH_UNITS`].
    PathTooLong,
    /// Not of the form `\\?\X:\...` with an ASCII drive letter.
    NotExtendedDrivePath,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyComponent => f.write_str("empty path component"),
            Self::DotComponent => f.write_str("`.` or `..` path component"),
            Self::ForbiddenUnit(u) => write!(f, "forbidden character U+{u:04X} in path component"),
            Self::ComponentTooLong => f.write_str("path component too long"),
            Self::PathTooLong => f.write_str("path too long"),
            Self::NotExtendedDrivePath => f.write_str(r"not an extended-length \\?\X:\ path"),
        }
    }
}

impl std::error::Error for PathError {}

/// Uppercases one UTF-16 code unit as described in the module documentation.
fn upcase(u: u16) -> u16 {
    if u < 0x80 {
        return u16::from((u as u8).to_ascii_uppercase());
    }
    // Greek iota-subscript letters: Rust exposes only their multi-character full mapping, the
    // simple mapping is the matching capital eight code points up (or the listed exceptions).
    match u {
        0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7 => return u + 8,
        0x1FB3 => return 0x1FBC,
        0x1FC3 => return 0x1FCC,
        0x1FF3 => return 0x1FFC,
        _ => {}
    }
    let Some(c) = char::from_u32(u32::from(u)) else {
        return u; // surrogate half
    };
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(single), None) => u16::try_from(u32::from(single))
            .ok()
            .filter(|&v| v >= 0x80)
            .unwrap_or(u),
        _ => u,
    }
}

/// One validated path component (a single file or directory name).
///
/// Never empty, `.` or `..`; contains no `\`, `/`, `:`, NUL, control character (U+0001..U+001F) or
/// Win32 wildcard (`* ? < > " |`); at most [`MAX_COMPONENT_UNITS`] code units. Trailing dots and
/// spaces and reserved device names (`con`) are kept literally: they are only special to Win32 path
/// parsing, which extended-length and handle-relative opens bypass (section 8.1).
#[derive(Clone)]
pub struct PathComponent(Box<[u16]>);

impl PathComponent {
    /// Validates a name given as UTF-16 code units (as returned by directory enumeration).
    pub fn from_wide(units: &[u16]) -> Result<Self, PathError> {
        match units {
            [] => return Err(PathError::EmptyComponent),
            [0x2E] | [0x2E, 0x2E] => return Err(PathError::DotComponent),
            _ => {}
        }
        if units.len() > MAX_COMPONENT_UNITS {
            return Err(PathError::ComponentTooLong);
        }
        let forbidden = |u: u16| u < 0x20 || br#"\/:*?<>"|"#.iter().any(|&b| u == u16::from(b));
        match units.iter().copied().find(|&u| forbidden(u)) {
            Some(u) => Err(PathError::ForbiddenUnit(u)),
            None => Ok(Self(units.into())),
        }
    }

    /// Validates a name given as a Rust string.
    pub fn new(name: &str) -> Result<Self, PathError> {
        Self::from_wide(&name.encode_utf16().collect::<Vec<_>>())
    }

    /// The name as UTF-16 code units, without terminator.
    pub fn as_wide(&self) -> &[u16] {
        &self.0
    }
}

impl PartialEq for PathComponent {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(&a, &b)| upcase(a) == upcase(b))
    }
}

impl Eq for PathComponent {}

impl Hash for PathComponent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.0.len());
        self.0.iter().for_each(|&u| state.write_u16(upcase(u)));
    }
}

impl fmt::Display for PathComponent {
    /// Lossy (unpaired surrogates become U+FFFD). For display only, never for comparison.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        char::decode_utf16(self.0.iter().copied())
            .try_for_each(|c| fmt::Write::write_char(f, c.unwrap_or(char::REPLACEMENT_CHARACTER)))
    }
}

impl fmt::Debug for PathComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string())
    }
}

/// An absolute extended-length drive path, `\\?\X:\` followed by zero or more components.
///
/// Untrusted by itself: it says nothing about what is on disk. Equality and hashing are ordinal and
/// case-insensitive (see the module documentation).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WidePath {
    /// Uppercase ASCII drive letter.
    drive: u8,
    components: Vec<PathComponent>,
}

impl WidePath {
    /// The drive root `\\?\X:\`. `letter` must be an ASCII letter; it is stored uppercase.
    pub fn drive_root(letter: char) -> Result<Self, PathError> {
        if !letter.is_ascii_alphabetic() {
            return Err(PathError::NotExtendedDrivePath);
        }
        Ok(Self {
            drive: letter.to_ascii_uppercase() as u8,
            components: Vec::new(),
        })
    }

    /// Parses API output of exactly the form `\\?\X:\` or `\\?\X:\name(\name)*`.
    ///
    /// No trailing separator (except on the root), no repeated separator, `/` is not a separator,
    /// every name is validated as a [`PathComponent`].
    pub fn parse_extended(units: &[u16]) -> Result<Self, PathError> {
        let (drive, rest) = match units {
            [p0, p1, p2, p3, d, colon, sep, rest @ ..]
                if [*p0, *p1, *p2, *p3] == PREFIX && *colon == u16::from(b':') && *sep == SEP =>
            {
                (*d, rest)
            }
            _ => return Err(PathError::NotExtendedDrivePath),
        };
        let letter = char::from_u32(u32::from(drive)).ok_or(PathError::NotExtendedDrivePath)?;
        let mut path = Self::drive_root(letter)?;
        if !rest.is_empty() {
            for name in rest.split(|&u| u == SEP) {
                path.push(PathComponent::from_wide(name)?)?;
            }
        }
        Ok(path)
    }

    /// Appends one component. On error the path is left unchanged.
    pub fn push(&mut self, component: PathComponent) -> Result<(), PathError> {
        if self.wide_len() + usize::from(!self.components.is_empty()) + component.0.len()
            > MAX_PATH_UNITS
        {
            return Err(PathError::PathTooLong);
        }
        self.components.push(component);
        Ok(())
    }

    /// Returns a new path with `components` appended in order.
    pub fn join<'a, I>(&self, components: I) -> Result<Self, PathError>
    where
        I: IntoIterator<Item = &'a PathComponent>,
    {
        let mut path = self.clone();
        for c in components {
            path.push(c.clone())?;
        }
        Ok(path)
    }

    /// The uppercase drive letter.
    pub fn drive(&self) -> char {
        char::from(self.drive)
    }

    /// The components after the drive root.
    pub fn components(&self) -> &[PathComponent] {
        &self.components
    }

    /// `true` for `\\?\X:\` itself.
    pub fn is_drive_root(&self) -> bool {
        self.components.is_empty()
    }

    /// The parent directory, or `None` for a drive root.
    pub fn parent(&self) -> Option<Self> {
        let (_, init) = self.components.split_last()?;
        Some(Self {
            drive: self.drive,
            components: init.to_vec(),
        })
    }

    /// `true` if `base` is this path or one of its ancestors. Component-wise, never a string prefix:
    /// `\\?\C:\Windows` does not start `\\?\C:\WindowsApps`.
    pub fn starts_with(&self, base: &Self) -> bool {
        self.strip_prefix(base).is_some()
    }

    /// `true` if this path lies strictly below `base` (`starts_with` and not equal).
    pub fn is_descendant_of(&self, base: &Self) -> bool {
        self.strip_prefix(base).is_some_and(|rest| !rest.is_empty())
    }

    /// The components below `base`, or `None` if `base` is not this path or an ancestor of it.
    pub fn strip_prefix(&self, base: &Self) -> Option<&[PathComponent]> {
        if self.drive != base.drive {
            return None;
        }
        let rest = self.components.strip_prefix(base.components.as_slice())?;
        Some(rest)
    }

    /// The path as UTF-16, `\\?\X:\a\b`, without NUL terminator.
    pub fn to_wide(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.wide_len());
        out.extend_from_slice(&PREFIX);
        out.extend_from_slice(&[u16::from(self.drive), u16::from(b':'), SEP]);
        for (i, c) in self.components.iter().enumerate() {
            if i > 0 {
                out.push(SEP);
            }
            out.extend_from_slice(&c.0);
        }
        out
    }

    fn wide_len(&self) -> usize {
        let names: usize = self.components.iter().map(|c| c.0.len()).sum();
        7 + names + self.components.len().saturating_sub(1)
    }
}

impl fmt::Display for WidePath {
    /// Lossy, for display only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, r"\\?\{}:\", self.drive())?;
        for (i, c) in self.components.iter().enumerate() {
            if i > 0 {
                f.write_str("\\")?;
            }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for WidePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string())
    }
}

#[cfg(test)]
mod tests;
