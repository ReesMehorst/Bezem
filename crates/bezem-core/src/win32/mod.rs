// SPDX-License-Identifier: MIT

//! Thin, safe wrappers over Windows APIs. This is the **only** module in Bezem that may contain
//! `unsafe` code (ARCHITECTURE.md sections 2.1 and 15); a CI job fails on `unsafe` anywhere else.
//!
//! Rules for every item in this module:
//! - Callers never see raw handles or pointers. Handles are [`handle::OwnedHandle`] values that close
//!   on drop.
//! - Every wrapper documents its invariants, and every `unsafe` block has a `// SAFETY:` comment.
//! - Failures are returned as [`error::Win32Error`] or [`error::NtError`] (API name plus raw code); nothing
//!   panics on an API failure.
//! - Fail closed: when a wrapper cannot establish a fact, it returns an error and the caller
//!   skips the item (ARCHITECTURE.md section 8.2, invariant 5).
//! - On non-Windows targets every wrapper compiles and returns an `unsupported_platform` error,
//!   so pure-logic code builds and tests on Linux.
//! - `#[allow(unsafe_code)]` may appear only on the `mod win32;` declaration in `lib.rs`. This
//!   CI check (attribute lines only, so docs may mention it) must print exactly that one line:
//!   `git grep -nE '^[[:space:]]*#!?\[.*(allow|expect|warn)\(.*unsafe_code' -- '*.rs'`
//!
//! The signatures for workstreams W2 to W6 are frozen; `README.md` in this directory lists them.
//! Implementations replace stub bodies without changing a signature.
#![warn(missing_docs)]
#![deny(clippy::multiple_unsafe_ops_per_block)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub(crate) mod error;
pub(crate) mod handle;
pub(crate) mod known_folders;
