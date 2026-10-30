// SPDX-License-Identifier: MIT

//! Scan and clean results (ARCHITECTURE.md sections 2.1 and 3.3).
//!
//! Only [`SkipReason`] exists so far, because the win32 signature freeze maps NTSTATUS values
//! onto it. `ScanReport`, `DeletePlan` and `CleanSummary` arrive with the engine workstream; the
//! serde derives arrive with the `serde` dependency.

/// Why an item was not deleted (ARCHITECTURE.md section 3.2). A skip is never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// Sharing violation: another process has the file open for write or delete.
    Locked,
    /// The OS denied access.
    AccessDenied,
    /// The item is a reparse point (any tag); never traversed or deleted.
    ReparsePoint,
    /// `NumberOfLinks > 1`.
    HardLinked,
    /// `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN` or `OFFLINE`.
    CloudPlaceholder,
    /// `FILE_ATTRIBUTE_SYSTEM` and the target does not include system files.
    SystemFile,
    /// Read-only and the target does not delete read-only files (or the FS cannot).
    ReadOnly,
    /// Younger than the target's `min_age`.
    TooNew,
    /// Matched an exclude glob (user or definition).
    Excluded,
    /// Identity, size or write time differs from the plan.
    ChangedSinceScan,
    /// Already deleted by someone else (not an error).
    Gone,
    /// The final-path check failed; the whole target is aborted.
    OutsideRoot,
    /// The file already has a delete pending.
    DeletePending,
    /// Recycle Bin mode, and the shell would have deleted permanently.
    RecycleUnavailable,
    /// An enumerated name is not a valid `PathComponent` (for example longer than 255 units or
    /// containing a wildcard), so it can never be opened. A skip, never `OutsideRoot`.
    UnsupportedName,
    /// Anything else; the Win32/NT code is kept in the log record.
    FsError,
}
