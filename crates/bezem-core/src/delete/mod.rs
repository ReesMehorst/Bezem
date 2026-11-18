// SPDX-License-Identifier: MIT

//! The handle-based deleter (ARCHITECTURE A:904-978; M1 design section 2.11).
//!
//! Ownership: work package **C**. `delete_entry` follows A:910-972 exactly: dircache open
//! chain -> `open_item(File)` -> `query_item` -> verdict-equivalent checks on
//! `ItemFacts::from_query` + `check_against_plan` + exclude re-check -> final-path compare
//! (`Cmp::Strict`; mismatch = `FinalPathMismatch` violation, target aborted) ->
//! `set_disposition`. The crate-internal function signatures below are provisional and may be
//! changed by C.

mod dircache;

pub(crate) use dircache::DirCache;

use crate::error::ErrorCode;
use crate::fs::FsBackend;
use crate::path::WidePath;
use crate::pipeline::Context;
use crate::safety::{SafetyViolation, VerifiedRoot};
use crate::types::SkipReason;
use crate::walk::{DirIndex, PlanEntry, TargetPlan};

/// The outcome of deleting one plan entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted { bytes: u64, final_path: WidePath },
    Skipped { reason: SkipReason, code: ErrorCode },
    Violation(SafetyViolation),
}

/// Deletes one file of `plan` through a handle opened relative to its parent.
#[allow(dead_code)] // M1-STUB (C)
#[allow(clippy::unimplemented, unused_variables, clippy::ptr_arg)] // M1-STUB (C)
pub(crate) fn delete_entry<F: FsBackend>(
    ctx: &Context<'_, F>,
    root: &VerifiedRoot<F::Dir>,
    plan: &TargetPlan,
    entry: &PlanEntry,
    cache: &mut DirCache<F::Dir>,
    scratch: &mut Vec<u16>,
) -> DeleteOutcome {
    unimplemented!()
}

/// Removes the plan's empty-directory candidates (post-order; never the root). Returns the
/// number removed and the skipped directories. `NotEmpty` is left silently.
#[allow(dead_code)] // M1-STUB (C)
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
pub(crate) fn remove_empty_dirs<F: FsBackend>(
    ctx: &Context<'_, F>,
    root: &VerifiedRoot<F::Dir>,
    plan: &TargetPlan,
    cache: &mut DirCache<F::Dir>,
) -> (u64, Vec<(DirIndex, SkipReason, ErrorCode)>) {
    unimplemented!()
}
