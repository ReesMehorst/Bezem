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
pub mod report;
pub mod safety;

// Private (ARCHITECTURE.md section 2.1): raw handles and wrappers never leave the crate. Each
// not-yet-used win32 item carries its own `cfg_attr(not(test), expect(dead_code))`, which turns
// into an error as soon as that item gets a caller. Unit-test builds see a different set of
// used items, so dead code is only allowed there.
#[allow(unsafe_code)]
#[cfg_attr(test, allow(dead_code))]
mod win32;

pub use win32::error::{Error as OsError, HresultError, NtError, Win32Error};
