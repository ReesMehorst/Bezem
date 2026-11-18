// SPDX-License-Identifier: MIT

//! Shared value types used across the crate (M1 design section 2.1).
//!
//! Ownership: Phase 0 contracts; work package **B** owns further `FileTime` behaviour.
//! `Origin`, `ExeName`, `ProfileRef`, `Action*`, `Category`, `Risk` and `DataClass` are M2+
//! types and are intentionally not defined here.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A Windows file time: 100 ns ticks since 1601-01-01 UTC (`FILETIME` / `LARGE_INTEGER`).
///
/// Negative raw values clamp to 0. Comparisons in the safety layer happen in this domain
/// (`FileTime::from_system_time(now)`), never by converting a file time back to `SystemTime`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct FileTime(pub u64);

impl FileTime {
    /// Ticks per second (100 ns resolution).
    pub const TICKS_PER_SEC: u64 = 10_000_000;
    /// Seconds between 1601-01-01 and 1970-01-01.
    pub const UNIX_EPOCH_DIFF_SECS: u64 = 11_644_473_600;

    const UNIX_EPOCH_TICKS: u64 = Self::UNIX_EPOCH_DIFF_SECS * Self::TICKS_PER_SEC;

    /// Converts a `SystemTime`, saturating at 0 (before 1601) and `u64::MAX`.
    pub fn from_system_time(t: SystemTime) -> FileTime {
        match t.duration_since(UNIX_EPOCH) {
            Ok(after) => FileTime(Self::UNIX_EPOCH_TICKS.saturating_add(duration_to_ticks(after))),
            Err(before) => FileTime(
                Self::UNIX_EPOCH_TICKS.saturating_sub(duration_to_ticks(before.duration())),
            ),
        }
    }

    /// Converts back to a `SystemTime`. Values above `i64::MAX` ticks are clamped first so the
    /// result is representable on every supported platform.
    pub fn to_system_time(self) -> SystemTime {
        let ticks = self.0.min(i64::MAX as u64);
        if ticks >= Self::UNIX_EPOCH_TICKS {
            let d = ticks_to_duration(ticks - Self::UNIX_EPOCH_TICKS);
            UNIX_EPOCH.checked_add(d).unwrap_or(UNIX_EPOCH)
        } else {
            let d = ticks_to_duration(Self::UNIX_EPOCH_TICKS - ticks);
            UNIX_EPOCH.checked_sub(d).unwrap_or(UNIX_EPOCH)
        }
    }

    /// From a raw signed `LARGE_INTEGER` value; negatives clamp to 0.
    pub fn from_i64(raw: i64) -> FileTime {
        FileTime(u64::try_from(raw).unwrap_or(0))
    }

    /// The later of two times.
    pub fn max(self, o: FileTime) -> FileTime {
        if o.0 > self.0 {
            o
        } else {
            self
        }
    }

    /// A duration in 100 ns ticks, saturating at `u64::MAX` (sub-tick remainders truncate).
    pub fn duration_to_ticks(d: Duration) -> u64 {
        duration_to_ticks(d)
    }
}

fn duration_to_ticks(d: Duration) -> u64 {
    d.as_secs()
        .saturating_mul(FileTime::TICKS_PER_SEC)
        .saturating_add(u64::from(d.subsec_nanos() / 100))
}

fn ticks_to_duration(ticks: u64) -> Duration {
    let secs = ticks / FileTime::TICKS_PER_SEC;
    // Remainder is < 10_000_000, so `* 100` fits in u32 (< 1e9).
    let nanos = u32::try_from((ticks % FileTime::TICKS_PER_SEC) * 100).unwrap_or(0);
    Duration::new(secs, nanos)
}

/// The identity of a file: volume serial plus the 128-bit file id (`FILE_ID_INFO`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileIdentity {
    pub volume_serial: u64,
    pub file_id: [u8; 16],
}

/// A random 128-bit scan id. `Display` is 32 lower-case hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScanId(pub u128);

impl ScanId {
    /// A fresh random id from the OS RNG (`getrandom`).
    ///
    /// The id only needs to be unique, not secret: if the OS RNG fails, the id falls back to a
    /// value built from the current time and a process-wide counter instead of failing.
    pub fn random() -> ScanId {
        let mut buf = [0u8; 16];
        if getrandom::fill(&mut buf).is_ok() {
            return ScanId(u128::from_le_bytes(buf));
        }
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        ScanId((t << 32) ^ u128::from(n) ^ (u128::from(std::process::id()) << 96))
    }
}

impl fmt::Display for ScanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

impl serde::Serialize for ScanId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// A cleaner id such as `windows.user_temp`: one or more of `[a-z0-9._-]`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct CleanerId(Box<str>);

/// Returned by [`CleanerId::new`] for an empty id or a character outside `[a-z0-9._-]`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid cleaner id {0:?}: expected one or more of [a-z0-9._-]")]
pub struct InvalidCleanerId(pub String);

impl CleanerId {
    /// Validates and wraps an id.
    pub fn new(s: &str) -> Result<CleanerId, InvalidCleanerId> {
        let ok = !s.is_empty()
            && s.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-'
            });
        if ok {
            Ok(CleanerId(s.into()))
        } else {
            Err(InvalidCleanerId(s.to_owned()))
        }
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CleanerId {
    type Error = InvalidCleanerId;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        CleanerId::new(&s)
    }
}

impl From<CleanerId> for String {
    fn from(id: CleanerId) -> String {
        id.0.into()
    }
}

impl fmt::Display for CleanerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a root lives in the user's profile or is machine-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    User,
    Machine,
}

/// The privilege the current process runs with. M1 only ever runs `Unelevated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Privilege {
    Unelevated,
    ElevatedWorker,
    ElevatedInteractive,
}

/// Why an item was not deleted (ARCHITECTURE A:265-281, exact variants; wire names snake_case).
/// `Ord` exists so the type can key a `BTreeMap`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Sharing violation or lock conflict.
    Locked,
    AccessDenied,
    /// Any reparse point (junction, symlink, mount point, cloud tag): never traversed or deleted.
    ReparsePoint,
    /// `NumberOfLinks > 1`.
    HardLinked,
    /// `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN` or `OFFLINE`.
    CloudPlaceholder,
    /// `FILE_ATTRIBUTE_SYSTEM` and the target does not set `include_system`.
    SystemFile,
    /// Read-only and the target does not set `delete_readonly` (or the FS cannot ignore it).
    ReadOnly,
    /// Younger than the effective minimum age, or timestamped in the future.
    TooNew,
    /// Matched an exclude glob, or lies beyond `max_depth`.
    Excluded,
    /// Identity, size or write time differs from the plan.
    ChangedSinceScan,
    /// Already gone (not an error).
    Gone,
    /// Final-path check failed: the target was aborted.
    OutsideRoot,
    DeletePending,
    /// M3 (Recycle Bin mode); never produced in M1.
    RecycleUnavailable,
    /// Any other OS failure; the code is kept in the log record.
    FsError,
}

/// The outcome of the safety verdict for one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Eligible,
    Skip(SkipReason),
}

/// How files are removed. Recycle Bin mode is M3 and intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeleteMode {
    Permanent,
}

/// File/directory/byte totals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Totals {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub bytes_on_disk: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_unix_epoch_round_trip() {
        let ft = FileTime::from_system_time(UNIX_EPOCH);
        assert_eq!(
            ft.0,
            FileTime::UNIX_EPOCH_DIFF_SECS * FileTime::TICKS_PER_SEC
        );
        assert_eq!(ft.to_system_time(), UNIX_EPOCH);
        let t = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_700);
        assert_eq!(FileTime::from_system_time(t).to_system_time(), t);
    }

    #[test]
    fn filetime_clamps() {
        assert_eq!(FileTime::from_i64(-5), FileTime(0));
        assert_eq!(FileTime::from_i64(42), FileTime(42));
        assert_eq!(FileTime(3).max(FileTime(7)), FileTime(7));
        let before_1601 = UNIX_EPOCH - Duration::from_secs(FileTime::UNIX_EPOCH_DIFF_SECS + 10);
        assert_eq!(FileTime::from_system_time(before_1601), FileTime(0));
    }

    #[test]
    fn scan_id_display_is_32_hex() {
        assert_eq!(ScanId(0xab).to_string(), format!("{:0>32}", "ab"));
        assert_eq!(ScanId::random().to_string().len(), 32);
    }

    #[test]
    fn cleaner_id_validation() {
        assert!(CleanerId::new("windows.user_temp").is_ok());
        assert!(CleanerId::new("a-b_c.9").is_ok());
        assert!(CleanerId::new("").is_err());
        assert!(CleanerId::new("Windows").is_err());
        assert!(CleanerId::new("a/b").is_err());
        let id: Result<CleanerId, _> = serde_json::from_str("\"BAD\"");
        assert!(id.is_err());
    }

    #[test]
    fn skip_reason_wire_names() {
        let s = serde_json::to_string(&SkipReason::ChangedSinceScan).unwrap();
        assert_eq!(s, "\"changed_since_scan\"");
    }
}
