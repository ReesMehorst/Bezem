// SPDX-License-Identifier: MIT

//! Bezem core: scanning, safety checks and the deletion engine (no UI).
//!
//! See `docs/ARCHITECTURE.md` section 2.1 for the planned modules.
//!
//! `unsafe` is denied here rather than forbidden, so the future `win32` module can
//! opt back in with `#[allow(unsafe_code)]` (ARCHITECTURE.md section 15).
#![deny(unsafe_code)]

pub mod safety;
