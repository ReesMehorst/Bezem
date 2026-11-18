// SPDX-License-Identifier: MIT

//! Pure, Linux-testable path parsing and validation (M1 design sections 2.3 and 4.1-4.2).
//!
//! Ownership: work package **B**. Nothing in this module touches the file system or reads
//! environment variables. Security decisions compare [`WidePath`]s built from OS final paths,
//! never user- or definition-supplied strings.

mod component;
mod final_path;
mod fold;
mod reject;
mod spec;
mod wide;

pub use component::{check_listed_name, check_template_component};
pub use final_path::parse_final;
pub use fold::{eq_wide, starts_with_at_boundary, Cmp};
pub use reject::PathReject;
pub use spec::{parse_root_spec, parse_root_spec_wide, RootSpec, RootToken};
pub use wide::WidePath;
