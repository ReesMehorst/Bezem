// SPDX-License-Identifier: MIT

//! [`LogWriter`]: the on-disk JSON Lines log.
//!
//! Ownership: work package **C**. File name `clean-<YYYY-MM-DDTHH-MM-SSZ>-<8 hex run id>.jsonl`,
//! created with `create_new(true)`; the header record is written and flushed first. Flushed
//! every 256 records, on `finish()` and (best effort) on drop. A crash leaves a truncated log
//! in which every line is valid JSON.

use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{LogRecord, LogSink, RunHeader};
use crate::error::LogError;

/// Records between two flushes.
pub const FLUSH_EVERY: u32 = 256;

/// Writes the deletion log to a file.
#[allow(dead_code)] // M1-STUB (C)
pub struct LogWriter {
    out: BufWriter<File>,
    path: PathBuf,
    since_flush: u32,
    clock: Box<dyn Fn() -> SystemTime + Send>,
}

impl fmt::Debug for LogWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogWriter")
            .field("path", &self.path)
            .field("since_flush", &self.since_flush)
            .finish_non_exhaustive()
    }
}

impl LogWriter {
    /// Creates a new log file in `dir` and writes the `run` header.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
    pub fn create(
        dir: &Path,
        run: &RunHeader,
        clock: Box<dyn Fn() -> SystemTime + Send>,
    ) -> Result<LogWriter, LogError> {
        unimplemented!()
    }

    /// The log file's path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
impl LogSink for LogWriter {
    fn record(&mut self, r: &LogRecord) -> Result<(), LogError> {
        unimplemented!()
    }
    fn finish(&mut self) -> Result<(), LogError> {
        unimplemented!()
    }
}
