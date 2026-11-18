// SPDX-License-Identifier: MIT

//! Bezem core: scanning, safety checks and the deletion engine (no UI).
//!
//! M1 "safe core". The module tree and the owning work package of each module are listed in
//! the M1 design (section 1 and section 6):
//!
//! | Module | Contents | Owner |
//! |---|---|---|
//! | [`types`] | shared value types (`FileTime`, `SkipReason`, ...) | Phase 0 / B (`FileTime`) |
//! | [`error`] | `Win32Error`, `NtError`, `ErrorCode`, tier errors | Phase 0 / A (name tables) |
//! | [`path`] | pure path parsing and validation (`WidePath`, `RootSpec`) | B |
//! | [`glob`] | include/exclude glob patterns | B |
//! | [`target`] | `Target` and its builder | B |
//! | [`resolve`] | known-folder resolution | B (A: system source) |
//! | [`fs`] | the `FsBackend` seam, status tables, directory buffer parsers, `MemFs` | Phase 0 / A / D |
//! | [`safety`] | allowlist, protected paths, `verify_root`, `verdict` | B |
//! | [`walk`] | the read-only parallel scanner and its plan types | C |
//! | [`delete`] | the handle-based deleter | C |
//! | [`log`] | the per-run JSON Lines deletion log | C |
//! | [`report`], [`progress`], [`pipeline`] | scan output, progress events, catalog-free entry points | C |
//! | `win32` (private, Windows only) | the only place allowed to call the OS directly | A |
//!
//! The crate denies `unsafe_code`; the private `win32` module is the single reviewed opt-in
//! (ARCHITECTURE.md section 15; enforced by the CI audit job).
#![deny(unsafe_code)]

pub mod delete;
pub mod error;
pub mod fs;
pub mod glob;
pub mod log;
pub mod path;
pub mod pipeline;
pub mod progress;
pub mod report;
pub mod resolve;
pub mod safety;
pub mod target;
pub mod types;
pub mod walk;

#[cfg(windows)]
#[allow(unsafe_code)]
mod win32;

#[cfg(windows)]
pub use win32::{Win32Dir, Win32Fs, Win32Item};
