// SPDX-License-Identifier: MIT

//! UTC timestamp formatting for the log (hand-rolled civil-from-days; the `time` crate is not
//! used because its recent releases exceed the 1.85 MSRV).
//!
//! Ownership: work package **C**.

use std::time::SystemTime;

/// `2026-10-28T14:03:22.123Z` (RFC 3339, UTC).
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
pub fn rfc3339_utc(t: SystemTime) -> String {
    unimplemented!()
}

/// `2026-10-28T14-03-22Z`, for log file names.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
pub fn file_stamp(t: SystemTime) -> String {
    unimplemented!()
}
