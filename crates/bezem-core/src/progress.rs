// SPDX-License-Identifier: MIT

//! Progress events and cancellation (ARCHITECTURE A:402-437; M1 design section 2.12).
//!
//! Ownership: work package **C**; `CancelToken` and `ThrottledCounters` are implemented in
//! Phase 0. Callbacks never block. Counter events are coalesced to at most 10 Hz.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::log::LogRecord;
use crate::types::{CleanerId, SkipReason, Totals};

/// Receives progress events. Implemented for any `Fn(&ProgressEvent) + Send + Sync`.
pub trait Progress: Send + Sync {
    fn on_event(&self, e: &ProgressEvent);
}

impl<F: Fn(&ProgressEvent) + Send + Sync> Progress for F {
    fn on_event(&self, e: &ProgressEvent) {
        self(e)
    }
}

/// Ignores every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn on_event(&self, _e: &ProgressEvent) {}
}

/// Run phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Phase {
    Resolve,
    Scan,
    Clean,
}

/// At most this many `Skipped` events per cleaner; after that only counts.
pub const MAX_SKIPPED_EVENTS_PER_CLEANER: u64 = 100;

/// A progress event. `WaitingForApps` and `ActionFinished` are M2+.
#[derive(Debug, Clone, serde::Serialize)]
pub enum ProgressEvent {
    PhaseStarted {
        phase: Phase,
    },
    CleanerStarted {
        id: CleanerId,
    },
    Scanned {
        id: CleanerId,
        files: u64,
        bytes: u64,
    },
    Deleted {
        id: CleanerId,
        files: u64,
        bytes: u64,
    },
    Skipped {
        id: CleanerId,
        path: String,
        reason: SkipReason,
    },
    CleanerFinished {
        id: CleanerId,
        totals: Totals,
        skipped: u64,
    },
    LogRecord(LogRecord),
}

/// Cooperative cancellation. Clones share the flag. The walker checks it before each
/// directory batch, the deleter before each item.
#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation (idempotent).
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// `Err(Cancelled)` once cancellation was requested.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Returned by [`CancelToken::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Minimum interval between two counter emissions.
pub const EMIT_INTERVAL_MS: u64 = 100;

/// Lock-free file/byte counters that tell the caller when to emit (at most every
/// [`EMIT_INTERVAL_MS`]). No ticker thread: the caller passes a monotonic `now_ms`.
#[derive(Debug, Default)]
pub struct ThrottledCounters {
    files: AtomicU64,
    bytes: AtomicU64,
    last_emit_ms: AtomicU64,
}

impl ThrottledCounters {
    /// Counters at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds to the counters. Returns the running totals when at least [`EMIT_INTERVAL_MS`]
    /// passed since the last emission and this caller won the race to emit.
    pub fn add(&self, files: u64, bytes: u64, now_ms: u64) -> Option<(u64, u64)> {
        let f = self
            .files
            .fetch_add(files, Ordering::Relaxed)
            .saturating_add(files);
        let b = self
            .bytes
            .fetch_add(bytes, Ordering::Relaxed)
            .saturating_add(bytes);
        let last = self.last_emit_ms.load(Ordering::Relaxed);
        if now_ms.saturating_sub(last) >= EMIT_INTERVAL_MS
            && self
                .last_emit_ms
                .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            Some((f, b))
        } else {
            None
        }
    }

    /// The final totals, to emit once at the end.
    pub fn flush(&self) -> (u64, u64) {
        (
            self.files.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_token_is_shared() {
        let t = CancelToken::new();
        let c = t.clone();
        assert!(t.check().is_ok());
        c.cancel();
        assert!(t.is_cancelled());
        assert_eq!(t.check(), Err(Cancelled));
    }

    #[test]
    fn counters_throttle() {
        let c = ThrottledCounters::new();
        assert_eq!(c.add(1, 10, 100), Some((1, 10)));
        assert_eq!(c.add(1, 10, 150), None);
        assert_eq!(c.add(1, 10, 200), Some((3, 30)));
        assert_eq!(c.flush(), (3, 30));
    }

    #[test]
    fn closures_are_progress() {
        let p = |_e: &ProgressEvent| {};
        p.on_event(&ProgressEvent::PhaseStarted { phase: Phase::Scan });
        NoProgress.on_event(&ProgressEvent::PhaseStarted {
            phase: Phase::Clean,
        });
    }
}
