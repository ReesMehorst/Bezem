// SPDX-License-Identifier: MIT

//! Include/exclude glob patterns (M1 design section 2.4; ARCHITECTURE A:749-769).
//!
//! Ownership: work package **B**. Globs only narrow what is eligible; deletion safety never
//! depends on a glob.

/// A validated glob pattern, relative to the target root with `/` as separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobPattern {
    source: String,
    normalized: String,
}

/// Why a glob pattern was refused.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum GlobError {
    #[error("empty pattern")]
    Empty,
    #[error("absolute pattern (leading '/' or drive)")]
    Absolute,
    #[error("'..' component in pattern")]
    ParentComponent,
    #[error("'**' must be a whole component")]
    DoubleStarPartial,
    #[error("invalid character {0:?} in pattern")]
    InvalidChar(char),
    #[error("glob compile error: {0}")]
    Compile(String),
}

impl GlobPattern {
    /// `\` is rewritten to `/`; rejects a leading `/`, a drive `X:`, any `..` component, any
    /// `:`, NUL and the empty pattern; `**` only as a whole component.
    ///
    /// Empty components (`a//b`, trailing `/`) are rejected as [`GlobError::Empty`]; the pattern
    /// is also compiled once so syntax errors surface here as [`GlobError::Compile`].
    pub fn parse(s: &str) -> Result<Self, GlobError> {
        if s.is_empty() {
            return Err(GlobError::Empty);
        }
        if s.contains('\0') {
            return Err(GlobError::InvalidChar('\0'));
        }
        let normalized = s.replace('\\', "/");
        if normalized.starts_with('/') {
            return Err(GlobError::Absolute);
        }
        let mut chars = normalized.chars();
        if let (Some(d), Some(':')) = (chars.next(), chars.next()) {
            if d.is_ascii_alphabetic() {
                return Err(GlobError::Absolute);
            }
        }
        if normalized.contains(':') {
            return Err(GlobError::InvalidChar(':'));
        }
        for comp in normalized.split('/') {
            if comp.is_empty() {
                return Err(GlobError::Empty);
            }
            if comp == ".." {
                return Err(GlobError::ParentComponent);
            }
            if comp.contains("**") && comp != "**" {
                return Err(GlobError::DoubleStarPartial);
            }
        }
        compile(&normalized)?;
        Ok(GlobPattern {
            source: s.to_owned(),
            normalized,
        })
    }

    /// The pattern as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The pattern after `\` -> `/` normalization.
    pub fn normalized(&self) -> &str {
        &self.normalized
    }
}

/// A compiled set of patterns.
#[derive(Debug, Clone)]
pub struct GlobMatcher {
    set: globset::GlobSet,
}

/// Compiles one normalized pattern with the fixed options (ARCHITECTURE 7.3).
///
/// `globset`'s case-insensitivity is ASCII-only, so both the pattern and every matched path
/// are first passed through [`fold_simple`] (one-to-one Unicode lowercase); together this gives
/// Unicode simple case folding as ARCHITECTURE 7.3 specifies. Known `globset` limitation: `?`
/// does not match a single non-ASCII character (it under-matches, which only narrows).
fn compile(normalized: &str) -> Result<globset::Glob, GlobError> {
    globset::GlobBuilder::new(&fold_simple(normalized))
        .case_insensitive(true)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map_err(|e| GlobError::Compile(e.to_string()))
}

impl GlobMatcher {
    /// Builds each pattern with `case_insensitive(true)`, `literal_separator(true)`,
    /// `backslash_escape(false)`.
    /// An empty pattern list matches nothing.
    pub fn new(patterns: &[GlobPattern]) -> Result<Self, GlobError> {
        let mut b = globset::GlobSetBuilder::new();
        for p in patterns {
            b.add(compile(&p.normalized)?);
        }
        let set = b.build().map_err(|e| GlobError::Compile(e.to_string()))?;
        Ok(GlobMatcher { set })
    }

    /// `true` when the matcher holds no patterns.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Matches a relative, `/`-joined UTF-16 path (converted lossily for matching).
    ///
    /// Lone surrogates become U+FFFD for matching; globs only narrow what is eligible, so a
    /// lossy match can never make an item deletable that the verdict would refuse.
    pub fn is_match_rel(&self, rel_slash: &[u16]) -> bool {
        if self.set.is_empty() {
            return false;
        }
        self.set
            .is_match(fold_simple(&String::from_utf16_lossy(rel_slash)))
    }
}

/// Lower-cases every character whose lowercase is a single character (so lengths and `?`
/// semantics are preserved); others are kept as they are.
fn fold_simple(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut low = c.to_lowercase();
            match (low.next(), low.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn matcher(pats: &[&str]) -> GlobMatcher {
        let pats: Vec<GlobPattern> = pats
            .iter()
            .map(|p| GlobPattern::parse(p).unwrap())
            .collect();
        GlobMatcher::new(&pats).unwrap()
    }

    /// Q:31-42 glob escape attempts plus the parse rules.
    #[test]
    fn parse_table() {
        use GlobError as E;
        let bad: &[(&str, GlobError)] = &[
            ("", E::Empty),
            ("a//b", E::Empty),
            ("a/", E::Empty),
            (r"..\*", E::ParentComponent),
            ("../x", E::ParentComponent),
            ("a/../../b", E::ParentComponent),
            ("a/..", E::ParentComponent),
            ("a**b", E::DoubleStarPartial),
            ("**.tmp", E::DoubleStarPartial),
            ("a/x**", E::DoubleStarPartial),
            (r"C:\*", E::Absolute),
            ("c:x", E::Absolute),
            ("/x", E::Absolute),
            (r"\x", E::Absolute),
            (r"\\server\share", E::Absolute),
            ("a:b", E::Absolute),
            ("ab:c", E::InvalidChar(':')),
            ("file.txt::$DATA", E::InvalidChar(':')),
            ("a\0", E::InvalidChar('\0')),
        ];
        for (p, want) in bad {
            assert_eq!(GlobPattern::parse(p), Err(want.clone()), "{p:?}");
        }
        assert!(matches!(GlobPattern::parse("[abc"), Err(E::Compile(_))));
        for ok in [
            "*.lock",
            "**",
            "**/x",
            "a/**",
            "a/**/b",
            "[!a]*.{log,tmp}",
            "...",
            "./x",
        ] {
            assert!(GlobPattern::parse(ok).is_ok(), "{ok:?}");
        }
        let p = GlobPattern::parse(r"cache\*.tmp").unwrap();
        assert_eq!(p.source(), r"cache\*.tmp");
        assert_eq!(p.normalized(), "cache/*.tmp");
    }

    #[test]
    fn matching_semantics() {
        let m = matcher(&["*.lock"]);
        assert!(m.is_match_rel(&w("a.lock")));
        assert!(m.is_match_rel(&w("A.LOCK")));
        // `*` never crosses a separator.
        assert!(!m.is_match_rel(&w("dir/a.lock")));
        let m = matcher(&["**/*.lock"]);
        assert!(m.is_match_rel(&w("a.lock")));
        assert!(m.is_match_rel(&w("x/y/a.lock")));
        let m = matcher(&["**"]);
        assert!(m.is_match_rel(&w("x/y/z")));
        // `\` is a separator in patterns, never an escape.
        let m = matcher(&[r"cache\*"]);
        assert!(m.is_match_rel(&w("cache/x")));
        assert!(!m.is_match_rel(&w("cachex")));
        let m = matcher(&["[*]"]);
        assert!(m.is_match_rel(&w("*")));
        assert!(!m.is_match_rel(&w("a")));
        // Unicode case-insensitivity.
        let m = matcher(&["ÄÖ*"]);
        assert!(m.is_match_rel(&w("äö.txt")));
        assert!(matcher(&["äö*"]).is_match_rel(&w("ÄÖ.TXT")));
        assert!(matcher(&["*"]).is_match_rel(&w("İ")));
        assert!(matcher(&["\u{212A}*"]).is_match_rel(&w("kelvin")));
        // Brace and class sets.
        let m = matcher(&["*.{log,tmp}", "[!a]x"]);
        assert!(m.is_match_rel(&w("q.TMP")));
        assert!(m.is_match_rel(&w("bx")));
        assert!(!m.is_match_rel(&w("ax")));
        // Lone surrogates are matched lossily and never panic.
        let mut v = w("a");
        v.push(0xD800);
        assert!(matcher(&["a*"]).is_match_rel(&v));
        // The empty matcher matches nothing.
        let empty = GlobMatcher::new(&[]).unwrap();
        assert!(empty.is_empty());
        assert!(!empty.is_match_rel(&w("x")));
        assert!(!matcher(&["x"]).is_empty());
    }
}
