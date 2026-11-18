// SPDX-License-Identifier: MIT

//! [`WidePath`]: the only absolute-path type security decisions use.
//!
//! Ownership: work package **B** (constructors, pushes and comparisons). The simple accessors
//! that rely only on the type invariant are implemented in Phase 0.

use super::{Cmp, PathReject};

/// Length of the `\\?\X:\` prefix in UTF-16 units.
const PREFIX_LEN: usize = 7;
/// Offset of the drive letter (after `\\?\`).
const DRIVE_AT: usize = 4;
const BACKSLASH: u16 = b'\\' as u16;

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
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn from_final(raw: &[u16]) -> Result<Self, PathReject> {
        unimplemented!()
    }

    /// A known-folder API value: `X:\...` or `\\?\X:\...`. Lenient on `~1` (the real `%TEMP%`
    /// is 8.3 on CI runners); rejects UNC, relative, empty, fewer than 4 units and `..`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn from_known_value(raw: &[u16]) -> Result<Self, PathReject> {
        unimplemented!()
    }

    /// Appends a template component, checked strictly (`check_template_component`).
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn push_template_component(&mut self, c: &[u16]) -> Result<(), PathReject> {
        unimplemented!()
    }

    /// Appends a name returned by a directory listing, checked leniently (`check_listed_name`).
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn push_listed_name(&mut self, c: &[u16]) -> Result<(), PathReject> {
        unimplemented!()
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
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn eq_strict(&self, o: &WidePath) -> bool {
        unimplemented!()
    }

    /// `self == base` or `self` is below `base`, at a component boundary, under `cmp`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn is_same_or_below(&self, base: &WidePath, cmp: Cmp) -> bool {
        unimplemented!()
    }

    /// `self` is below `base` (not equal), at a component boundary, under `cmp`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn is_strictly_below(&self, base: &WidePath, cmp: Cmp) -> bool {
        unimplemented!()
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
}
