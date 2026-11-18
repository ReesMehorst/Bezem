// SPDX-License-Identifier: MIT

//! The read-only parallel scanner (custom handle-relative walker on `rayon`; `jwalk` and
//! `walkdir` are not used) and its plan types (M1 design sections 2.9 and 6, package C).
//!
//! Ownership: work package **C**. The public entry point is `pipeline::scan_target`; the
//! walker itself is crate-internal and added by C.

mod plan;

pub use plan::{DirIndex, DirNode, DirTable, PlanEntry, TargetPlan};

/// Largest number of scan threads when `threads` is `None`.
pub const MAX_DEFAULT_THREADS: usize = 8;

/// Walker parameters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkParams {
    /// `None` = `min(available_parallelism, 8)`.
    pub threads: Option<usize>,
}
