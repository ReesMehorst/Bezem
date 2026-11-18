// SPDX-License-Identifier: MIT

//! Scan output (ARCHITECTURE A:289-319; M1 design section 2.10).
//!
//! Ownership: work package **C**. `ScanReport` is serializable; the sealed plans live beside
//! it in `ScanOutcome` and are never serialized (design deviation 10).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::types::{CleanerId, ScanId, SkipReason, Totals};
use crate::walk::TargetPlan;

/// Scan result for one target.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetScan {
    /// Display path (without `\\?\`), lossy.
    pub display: String,
    pub status: TargetStatus,
    pub totals: Totals,
    pub skipped: BTreeMap<SkipReason, u64>,
}

/// Status of one target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TargetStatus {
    Ready,
    NothingToClean,
    Missing,
    Unavailable { reason: String },
    Failed { error: String },
    SafetyViolation { detail: String },
}

/// One of the largest eligible files, for previews.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SampleEntry {
    pub path: String,
    pub bytes: u64,
}

/// Status of one cleaner. `NeedsAdmin` is reported without error; M1 never elevates.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CleanerStatus {
    Ready,
    NothingToClean,
    NeedsAdmin,
    Blocked { reason: String },
    Unavailable { reason: String },
    Failed { error: String },
}

/// Scan result for one cleaner.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CleanerScan {
    pub id: CleanerId,
    pub status: CleanerStatus,
    pub totals: Totals,
    pub targets: Vec<TargetScan>,
    /// Largest-20 samples.
    pub largest: Vec<SampleEntry>,
    pub skipped: BTreeMap<SkipReason, u64>,
}

/// Current `ScanReport::format`.
pub const SCAN_REPORT_FORMAT: u32 = 1;

/// The whole scan result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanReport {
    /// [`SCAN_REPORT_FORMAT`].
    pub format: u32,
    pub scan_id: ScanId,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub cleaners: Vec<CleanerScan>,
}

/// The report plus the sealed per-target plans. Turn it into a `DeletePlan` with
/// `ScanOutcome::into_delete_plan` (in `pipeline`).
#[derive(Debug)]
pub struct ScanOutcome {
    pub report: ScanReport,
    pub(crate) plans: Vec<TargetPlan>,
}

/// Result of `pipeline::execute_plan`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CleanSummary {
    pub files: u64,
    pub bytes: u64,
    pub skipped: BTreeMap<SkipReason, u64>,
    pub cancelled: bool,
    pub safety_violations: u32,
    pub duration: Duration,
    pub log_path: Option<PathBuf>,
}
