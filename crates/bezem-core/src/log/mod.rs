// SPDX-License-Identifier: MIT

//! The per-run JSON Lines deletion log (F-LOG-01; ARCHITECTURE A:1192-1214; decision 7).
//!
//! Ownership: work package **C**. The library never runs without a log: callers pass a
//! `&mut dyn LogSink`; tests use [`MemSink`]/[`NullSink`], production uses [`LogWriter`]. The
//! scanner writes nothing; only a delete run creates a log.

mod record;
mod timefmt;
mod writer;

pub use record::{encode_path, LogRecord, RunHeader, Stamped};
pub use timefmt::{file_stamp, rfc3339_utc};
pub use writer::{LogWriter, FLUSH_EVERY};

use crate::error::LogError;

/// Destination for log records. Each outcome is recorded exactly once.
pub trait LogSink: Send {
    fn record(&mut self, r: &LogRecord) -> Result<(), LogError>;
    /// Flushes; called once at the end of the run.
    fn finish(&mut self) -> Result<(), LogError>;
}

/// Discards every record.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullSink;

impl LogSink for NullSink {
    fn record(&mut self, _r: &LogRecord) -> Result<(), LogError> {
        Ok(())
    }
    fn finish(&mut self) -> Result<(), LogError> {
        Ok(())
    }
}

/// Keeps every record in memory (tests).
#[derive(Debug, Clone, Default)]
pub struct MemSink(pub Vec<LogRecord>);

impl LogSink for MemSink {
    fn record(&mut self, r: &LogRecord) -> Result<(), LogError> {
        self.0.push(r.clone());
        Ok(())
    }
    fn finish(&mut self) -> Result<(), LogError> {
        Ok(())
    }
}
