// SPDX-License-Identifier: MIT

//! Safety layer (ARCHITECTURE.md section 8).
//!
//! At M1/S1 this holds only the path types. `SafetyPolicy`, allowlisted bases, protected paths and
//! root verification build on them.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod path;

pub use path::{PathComponent, PathError, WidePath};
