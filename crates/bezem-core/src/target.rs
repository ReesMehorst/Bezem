// SPDX-License-Identifier: MIT

//! [`Target`]: one root plus its selection rules, constructed in code for M1 (TOML is M2).
//!
//! Ownership: work package **B**.

use std::time::Duration;

use crate::glob::{GlobError, GlobMatcher};
use crate::path::RootSpec;

/// Include/exclude matching for one target. `include` applies to files only; `exclude`
/// applies to both, and a matching directory prunes its subtree.
#[derive(Debug, Clone)]
pub struct TargetMatcher {
    #[allow(dead_code)] // M1-STUB (B): read by selects_file
    include: Option<GlobMatcher>,
    #[allow(dead_code)] // M1-STUB (B): read by excludes
    exclude: GlobMatcher,
}

impl TargetMatcher {
    /// `true` when there are no include patterns or one matches `rel` (`/`-joined, relative).
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn selects_file(&self, rel: &[u16]) -> bool {
        unimplemented!()
    }

    /// `true` when an exclude pattern matches `rel`.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
    pub fn excludes(&self, rel: &[u16]) -> bool {
        unimplemented!()
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
#[allow(dead_code)] // M1-STUB (B): fields are read by build()
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
    #[allow(clippy::unimplemented)] // M1-STUB (B)
    pub fn build(self) -> Result<Target, TargetBuildError> {
        unimplemented!()
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
