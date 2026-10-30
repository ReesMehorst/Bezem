// SPDX-License-Identifier: MIT

//! Bezem core: scanning, safety checks and the deletion engine (no UI).
//!
//! See `docs/ARCHITECTURE.md` section 2.1 for the planned modules.
//!
//! `unsafe` is denied here rather than forbidden, so the private `win32` module can opt back in
//! with `#[allow(unsafe_code)]` (ARCHITECTURE.md sections 2.1 and 15). That declaration below is
//! the only place the allow may appear.
#![deny(unsafe_code)]

pub mod path;
pub mod safety;

// Private (ARCHITECTURE.md section 2.1): raw handles and wrappers never leave the crate. The
// `dead_code` expectation covers the frozen stubs until their consumers land; it turns into a
// warning, and must be removed, once every item is used.
#[allow(unsafe_code)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "win32 API waits for its M1 consumers")
)]
#[cfg_attr(test, allow(dead_code))]
mod win32;

pub use win32::error::{Error as OsError, HresultError, NtError, Win32Error};
