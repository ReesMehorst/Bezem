// SPDX-License-Identifier: MIT

//! Catalog-free M1 entry points (decision 13; M1 design section 2.10): scan targets into a
//! `ScanOutcome`, seal it into a [`DeletePlan`], execute it. `Engine`/`Catalog` arrive in M2.
//!
//! Ownership: work package **C**.

use std::sync::Arc;
use std::time::SystemTime;

use crate::error::EngineError;
use crate::fs::FsBackend;
use crate::log::LogSink;
use crate::progress::{CancelToken, Progress};
use crate::report::{CleanSummary, ScanOutcome, TargetScan};
use crate::resolve::KnownFolders;
use crate::safety::SafetyPolicy;
use crate::target::Target;
use crate::types::{CleanerId, DeleteMode, ScanId, Scope};
use crate::walk::TargetPlan;

/// Everything a scan or clean needs besides its inputs.
#[derive(Debug)]
pub struct Context<'a, F: FsBackend> {
    pub fs: &'a F,
    pub policy: &'a SafetyPolicy,
    /// The reference time for age checks (fixed for the whole run).
    pub now: SystemTime,
    /// `None` = `min(available_parallelism, 8)`; tests use `Some(1)`.
    pub threads: Option<usize>,
}

/// One target to scan: (cleaner, scope, target).
#[derive(Debug, Clone)]
pub struct ScanInput {
    pub cleaner: CleanerId,
    pub scope: Scope,
    pub target: Arc<Target>,
}

/// A sealed delete plan: only `ScanOutcome::into_delete_plan` builds one.
#[derive(Debug)]
pub struct DeletePlan {
    scan_id: ScanId,
    mode: DeleteMode,
    #[allow(dead_code)] // M1-STUB (C): consumed by execute_plan
    targets: Vec<TargetPlan>,
}

impl DeletePlan {
    /// The scan the plan came from.
    pub fn scan_id(&self) -> ScanId {
        self.scan_id
    }
    /// The delete mode (always `Permanent` in M1).
    pub fn mode(&self) -> DeleteMode {
        self.mode
    }
}

impl ScanOutcome {
    /// Seals the plans for deletion.
    pub fn into_delete_plan(self, mode: DeleteMode) -> DeletePlan {
        DeletePlan {
            scan_id: self.report.scan_id,
            mode,
            targets: self.plans,
        }
    }
}

/// Resolves, verifies and scans one target. Never panics: resolve/root/target errors become
/// a `TargetStatus` and `None` plan.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
pub fn scan_target<F: FsBackend>(
    ctx: &Context<'_, F>,
    cleaner: &CleanerId,
    scope: Scope,
    target_index: usize,
    target: &Arc<Target>,
    known: &KnownFolders,
    progress: &dyn Progress,
    cancel: &CancelToken,
) -> (TargetScan, Option<TargetPlan>) {
    unimplemented!()
}

/// Scans every input (grouped per cleaner, in input order) into a report plus plans.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
pub fn scan_targets<F: FsBackend>(
    ctx: &Context<'_, F>,
    inputs: &[ScanInput],
    known: &KnownFolders,
    progress: &dyn Progress,
    cancel: &CancelToken,
) -> ScanOutcome {
    unimplemented!()
}

/// Executes a plan: re-verifies each root, deletes per file with re-checks, then the
/// empty-directory pass; every outcome is logged exactly once through `log`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
pub fn execute_plan<F: FsBackend>(
    ctx: &Context<'_, F>,
    plan: DeletePlan,
    log: &mut dyn LogSink,
    progress: &dyn Progress,
    cancel: &CancelToken,
) -> Result<CleanSummary, EngineError> {
    unimplemented!()
}
