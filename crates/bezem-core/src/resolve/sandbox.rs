// SPDX-License-Identifier: MIT

//! `KnownFolders::sandbox`: a fake known-folder layout under a test directory (M1 design
//! section 4.3). Only compiled with `cfg(test)` or the `test-support` feature, which shipped
//! crates must never enable.
//!
//! Ownership: work package **D**.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use super::KnownFolders;
use crate::path::WidePath;

impl KnownFolders {
    /// Every known folder placed under `root`: `Users\u` (Profile), `Users\u\AppData\Local`,
    /// `...\Local\Temp`, `...\LocalLow`, `Users\u\AppData\Roaming`, `...\Microsoft\Windows\Recent`,
    /// `Users\u\{Desktop,Documents,Downloads,Pictures,Music,Videos,OneDrive}`, `ProgramData`,
    /// `Windows`, `Program Files`, `Program Files (x86)`. Paths only: nothing is created.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (D)
    pub fn sandbox(root: &WidePath) -> Self {
        unimplemented!()
    }
}
