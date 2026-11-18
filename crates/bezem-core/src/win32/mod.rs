// SPDX-License-Identifier: MIT

//! The reviewed Win32/NT layer: the ONLY module allowed to contain `unsafe` code
//! (`lib.rs` opts it in with `#[allow(unsafe_code)]`; CI `unsafe-audit` excludes this
//! directory). Windows only. M1 design section 3.
//!
//! Ownership: work package **A**. Rules: every `unsafe` block carries a `// SAFETY:` comment;
//! wrappers return `Win32Error`/`NtError` (or the seam's error types), never panic and never
//! expose raw handles. Wrappers report facts; all policy lives in `safety`.
//!
//! Phase 0: skeleton only. Every wrapper below is a signature stub.
// The stubs are not called yet; remove together with the M1-STUB markers.
#![allow(dead_code)] // M1-STUB (A)

mod dirent;
mod dispose;
mod finalpath;
mod fs_impl;
mod handle;
mod info;
mod known_folders;
mod nt;
mod open;
mod volume;

pub use fs_impl::{Win32Dir, Win32Fs, Win32Item};
pub(crate) use handle::OwnedHandle;
pub(crate) use known_folders::SystemKnownFolders;
