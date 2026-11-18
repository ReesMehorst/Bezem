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
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn parse(s: &str) -> Result<Self, GlobError> {
        unimplemented!()
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
    #[allow(dead_code)] // M1-STUB (B): read by is_match_rel
    set: globset::GlobSet,
}

impl GlobMatcher {
    /// Builds each pattern with `case_insensitive(true)`, `literal_separator(true)`,
    /// `backslash_escape(false)`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn new(patterns: &[GlobPattern]) -> Result<Self, GlobError> {
        unimplemented!()
    }

    /// Matches a relative, `/`-joined UTF-16 path (converted lossily for matching).
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn is_match_rel(&self, rel_slash: &[u16]) -> bool {
        unimplemented!()
    }
}
