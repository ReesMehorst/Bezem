// SPDX-License-Identifier: MIT

//! Safety layer (ARCHITECTURE.md section 8).
//!
//! `SafetyPolicy`, allowlisted bases, protected paths and root verification build on the pure path
//! types in [`crate::path`], re-exported here.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub use crate::path::{FileIdentity, PathComponent, PathError, WidePath};
