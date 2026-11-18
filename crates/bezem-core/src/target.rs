// SPDX-License-Identifier: MIT

//! [`Target`]: one root plus its selection rules, constructed in code for M1 (TOML is M2).
//!
//! Ownership: work package **B**.

use std::time::Duration;

use crate::glob::{GlobError, GlobMatcher, GlobPattern};
use crate::path::RootSpec;

/// Include/exclude matching for one target. `include` applies to files only; `exclude`
/// applies to both, and a matching directory prunes its subtree.
#[derive(Debug, Clone)]
pub struct TargetMatcher {
    include: Option<GlobMatcher>,
    /// Every exclude pattern `p` is compiled as both `p` and `p/**`, so anything below an
    /// excluded directory is excluded too (also at delete time, where only the file's own
    /// relative path is re-checked).
    exclude: GlobMatcher,
}

impl TargetMatcher {
    /// `true` when there are no include patterns or one matches `rel` (`/`-joined, relative).
    pub fn selects_file(&self, rel: &[u16]) -> bool {
        match &self.include {
            None => true,
            Some(m) => m.is_match_rel(rel),
        }
    }

    /// `true` when an exclude pattern matches `rel`.
    pub fn excludes(&self, rel: &[u16]) -> bool {
        self.exclude.is_match_rel(rel)
    }

    /// Builds the matcher from raw include/exclude pattern strings.
    fn build(include: &[String], exclude: &[String]) -> Result<Self, GlobError> {
        let include = if include.is_empty() {
            None
        } else {
            let pats = include
                .iter()
                .map(|p| GlobPattern::parse(p))
                .collect::<Result<Vec<_>, _>>()?;
            Some(GlobMatcher::new(&pats)?)
        };
        let mut ex = Vec::with_capacity(exclude.len() * 2);
        for p in exclude {
            let pat = GlobPattern::parse(p)?;
            let below = GlobPattern::parse(&format!("{}/**", pat.normalized()))?;
            ex.push(pat);
            ex.push(below);
        }
        Ok(TargetMatcher {
            include,
            exclude: GlobMatcher::new(&ex)?,
        })
    }
}

/// One root and its rules.
#[derive(Debug, Clone)]
pub struct Target {
    pub spec: RootSpec,
    pub matcher: TargetMatcher,
    pub recursive: bool,
    pub min_age: Duration,
    pub remove_empty_dirs: bool,
    pub include_system: bool,
    pub delete_readonly: bool,
    pub optional: bool,
    pub max_depth: u16,
    /// `spec.is_temp_like()`, set by the builder only.
    pub temp_root: bool,
}

/// Builder for [`Target`]. Defaults: `min_age` 24 h, `recursive` true, `max_depth` 64, every
/// other flag false, no include/exclude patterns.
#[derive(Debug, Clone)]
pub struct TargetBuilder {
    spec: RootSpec,
    min_age_hours: u32,
    include: Vec<String>,
    exclude: Vec<String>,
    recursive: bool,
    max_depth: u16,
    remove_empty_dirs: bool,
    include_system: bool,
    delete_readonly: bool,
    optional: bool,
}

/// Default `min_age_hours`.
pub const DEFAULT_MIN_AGE_HOURS: u32 = 24;
/// Largest accepted `min_age_hours` (one year).
pub const MAX_MIN_AGE_HOURS: u32 = 8760;
/// Default `max_depth`.
pub const DEFAULT_MAX_DEPTH: u16 = 64;

impl Target {
    /// Starts a builder with the defaults above.
    pub fn builder(spec: RootSpec) -> TargetBuilder {
        TargetBuilder {
            spec,
            min_age_hours: DEFAULT_MIN_AGE_HOURS,
            include: Vec::new(),
            exclude: Vec::new(),
            recursive: true,
            max_depth: DEFAULT_MAX_DEPTH,
            remove_empty_dirs: false,
            include_system: false,
            delete_readonly: false,
            optional: false,
        }
    }
}

impl TargetBuilder {
    /// Minimum age in hours (0..=8760; temp roots >= 24).
    pub fn min_age_hours(mut self, hours: u32) -> Self {
        self.min_age_hours = hours;
        self
    }

    /// Adds include patterns (appends to earlier calls).
    pub fn include(mut self, patterns: &[&str]) -> Self {
        self.include
            .extend(patterns.iter().map(|p| (*p).to_owned()));
        self
    }

    /// Adds exclude patterns (appends to earlier calls).
    pub fn exclude(mut self, patterns: &[&str]) -> Self {
        self.exclude
            .extend(patterns.iter().map(|p| (*p).to_owned()));
        self
    }

    /// `false` means direct children only.
    pub fn recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }

    /// Directories deeper than this are not entered (counted as `Excluded`).
    pub fn max_depth(mut self, depth: u16) -> Self {
        self.max_depth = depth;
        self
    }

    /// Remove directories left empty after the target's files (never the root).
    pub fn remove_empty_dirs(mut self, on: bool) -> Self {
        self.remove_empty_dirs = on;
        self
    }

    /// Allow `FILE_ATTRIBUTE_SYSTEM` items.
    pub fn include_system(mut self, on: bool) -> Self {
        self.include_system = on;
        self
    }

    /// Delete read-only files with `FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE` (decision 1:
    /// default false; attributes are never cleared).
    pub fn delete_readonly(mut self, on: bool) -> Self {
        self.delete_readonly = on;
        self
    }

    /// A missing root is `Missing` (not `Unavailable`).
    pub fn optional(mut self, on: bool) -> Self {
        self.optional = on;
        self
    }

    /// Validates and builds. Rejects `min_age_hours > 8760`, temp roots below 24 h and bad
    /// globs.
    pub fn build(self) -> Result<Target, TargetBuildError> {
        if self.min_age_hours > MAX_MIN_AGE_HOURS {
            return Err(TargetBuildError::AgeTooLarge);
        }
        let temp_root = self.spec.is_temp_like();
        if temp_root && self.min_age_hours < DEFAULT_MIN_AGE_HOURS {
            return Err(TargetBuildError::TempAgeBelowFloor {
                min_hours: DEFAULT_MIN_AGE_HOURS,
            });
        }
        let matcher = TargetMatcher::build(&self.include, &self.exclude)?;
        Ok(Target {
            spec: self.spec,
            matcher,
            recursive: self.recursive,
            min_age: Duration::from_secs(u64::from(self.min_age_hours) * 3600),
            remove_empty_dirs: self.remove_empty_dirs,
            include_system: self.include_system,
            delete_readonly: self.delete_readonly,
            optional: self.optional,
            max_depth: self.max_depth,
            temp_root,
        })
    }
}

/// Why [`TargetBuilder::build`] failed.
#[derive(Debug, thiserror::Error)]
pub enum TargetBuildError {
    #[error("min_age_hours above {MAX_MIN_AGE_HOURS}")]
    AgeTooLarge,
    #[error("temp roots need min_age_hours >= {min_hours}")]
    TempAgeBelowFloor { min_hours: u32 },
    #[error("glob: {0}")]
    Glob(#[from] GlobError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::parse_root_spec;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn spec(s: &str) -> RootSpec {
        parse_root_spec(s).unwrap()
    }

    #[test]
    fn defaults() {
        let t = Target::builder(spec(r"%LOCALAPPDATA%\x")).build().unwrap();
        assert_eq!(t.min_age, Duration::from_secs(24 * 3600));
        assert!(t.recursive);
        assert_eq!(t.max_depth, DEFAULT_MAX_DEPTH);
        assert!(!t.remove_empty_dirs);
        assert!(!t.include_system);
        assert!(
            !t.delete_readonly,
            "decision 1: delete_readonly defaults to false"
        );
        assert!(!t.optional);
        assert!(!t.temp_root);
        assert!(t.matcher.selects_file(&w("anything")));
        assert!(!t.matcher.excludes(&w("anything")));
    }

    #[test]
    fn setters() {
        let t = Target::builder(spec(r"%LOCALAPPDATA%\x"))
            .min_age_hours(0)
            .recursive(false)
            .max_depth(3)
            .remove_empty_dirs(true)
            .include_system(true)
            .delete_readonly(true)
            .optional(true)
            .include(&["*.tmp"])
            .include(&["*.log"])
            .exclude(&["keep"])
            .build()
            .unwrap();
        assert_eq!(t.min_age, Duration::ZERO);
        assert!(!t.recursive);
        assert_eq!(t.max_depth, 3);
        assert!(t.remove_empty_dirs && t.include_system && t.delete_readonly && t.optional);
        assert!(t.matcher.selects_file(&w("a.tmp")));
        assert!(t.matcher.selects_file(&w("a.LOG")));
        assert!(!t.matcher.selects_file(&w("a.txt")));
        // An excluded directory excludes everything below it, too.
        assert!(t.matcher.excludes(&w("keep")));
        assert!(t.matcher.excludes(&w("keep/a.tmp")));
        assert!(t.matcher.excludes(&w("KEEP/x/y")));
        assert!(!t.matcher.excludes(&w("keeper")));
        assert!(!t.matcher.excludes(&w("x/keep")));
    }

    #[test]
    fn age_rules() {
        let b = |s: &str, h: u32| Target::builder(spec(s)).min_age_hours(h).build();
        assert!(matches!(
            b(r"%LOCALAPPDATA%\x", 8761),
            Err(TargetBuildError::AgeTooLarge)
        ));
        assert!(b(r"%LOCALAPPDATA%\x", 8760).is_ok());
        assert!(b(r"%LOCALAPPDATA%\x", 0).is_ok());
        for temp in [
            "%TEMP%",
            r"%WINDIR%\Temp",
            r"%LOCALAPPDATA%\Temp",
            r"%WINDIR%\SystemTemp",
        ] {
            assert!(
                matches!(
                    b(temp, 23),
                    Err(TargetBuildError::TempAgeBelowFloor { min_hours: 24 })
                ),
                "{temp}"
            );
            let t = b(temp, 24).unwrap();
            assert!(t.temp_root);
        }
        assert!(matches!(
            b("%TEMP%", 9000),
            Err(TargetBuildError::AgeTooLarge)
        ));
    }

    #[test]
    fn bad_globs() {
        let r = Target::builder(spec("%TEMP%")).include(&[r"..\*"]).build();
        assert!(matches!(
            r,
            Err(TargetBuildError::Glob(GlobError::ParentComponent))
        ));
        let r = Target::builder(spec("%TEMP%")).exclude(&["/x"]).build();
        assert!(matches!(
            r,
            Err(TargetBuildError::Glob(GlobError::Absolute))
        ));
        let r = Target::builder(spec("%TEMP%")).exclude(&["a**"]).build();
        assert!(matches!(
            r,
            Err(TargetBuildError::Glob(GlobError::DoubleStarPartial))
        ));
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        fn arb_pattern() -> impl Strategy<Value = String> {
            let comp = prop_oneof![
                Just("*".to_owned()),
                Just("**".to_owned()),
                Just("*.tmp".to_owned()),
                Just("?".to_owned()),
                Just("[ab]*".to_owned()),
                "[a-c]{1,2}",
            ];
            proptest::collection::vec(comp, 1..4).prop_map(|v| v.join("/"))
        }

        fn arb_rel() -> impl Strategy<Value = String> {
            let comp = prop_oneof![
                Just("a".to_owned()),
                Just("b.tmp".to_owned()),
                Just("ab".to_owned()),
                Just("c".to_owned()),
                Just("x.TMP".to_owned()),
            ];
            proptest::collection::vec(comp, 1..5).prop_map(|v| v.join("/"))
        }

        proptest! {
            /// Adding exclude patterns never un-excludes anything (globs only narrow), and
            /// anything below an excluded path is excluded too.
            #[test]
            fn exclude_monotone(
                a in proptest::collection::vec(arb_pattern(), 0..3),
                extra in proptest::collection::vec(arb_pattern(), 0..3),
                rels in proptest::collection::vec(arb_rel(), 1..8),
            ) {
                let a_refs: Vec<&str> = a.iter().map(String::as_str).collect();
                let all: Vec<&str> = a.iter().chain(&extra).map(String::as_str).collect();
                let small = Target::builder(spec("%TEMP%")).exclude(&a_refs).build().unwrap();
                let big = Target::builder(spec("%TEMP%")).exclude(&all).build().unwrap();
                for rel in &rels {
                    let rel = w(rel);
                    if small.matcher.excludes(&rel) {
                        prop_assert!(big.matcher.excludes(&rel));
                    }
                    if big.matcher.excludes(&rel) {
                        let mut child = rel.clone();
                        child.extend(w("/z"));
                        prop_assert!(big.matcher.excludes(&child));
                    }
                }
            }

            /// More include patterns only widen the include selection; no include list
            /// selects everything.
            #[test]
            fn include_monotone(
                a in proptest::collection::vec(arb_pattern(), 1..3),
                extra in proptest::collection::vec(arb_pattern(), 0..3),
                rel in arb_rel(),
            ) {
                let a_refs: Vec<&str> = a.iter().map(String::as_str).collect();
                let all: Vec<&str> = a.iter().chain(&extra).map(String::as_str).collect();
                let small = Target::builder(spec("%TEMP%")).include(&a_refs).build().unwrap();
                let big = Target::builder(spec("%TEMP%")).include(&all).build().unwrap();
                let none = Target::builder(spec("%TEMP%")).build().unwrap();
                let rel = w(&rel);
                if small.matcher.selects_file(&rel) {
                    prop_assert!(big.matcher.selects_file(&rel));
                }
                prop_assert!(none.matcher.selects_file(&rel));
            }
        }
    }
}
