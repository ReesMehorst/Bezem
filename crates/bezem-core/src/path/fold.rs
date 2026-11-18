// SPDX-License-Identifier: MIT

//! Case-folding comparisons on UTF-16 paths.
//!
//! Ownership: work package **B**.
//!
//! - [`Cmp::Strict`]: ASCII-only case fold, everything else exact. Fail-safe because it can only
//!   under-match; OS final paths carry the on-disk casing. Used for allowlist containment.
//! - [`Cmp::Loose`]: full Unicode fold both ways (`char::to_uppercase().flat_map(to_lowercase)`)
//!   plus U+0130/U+0131 -> `i`, U+212A -> `k`, U+017F -> `s`, `ß` -> `ss`. Over-matches, so it is
//!   used ONLY for protected/forbidden checks, where it can only reject more.

/// Which case-folding rule a comparison uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Strict,
    Loose,
}

/// Whole-string equality under `c`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn eq_wide(a: &[u16], b: &[u16], c: Cmp) -> bool {
    unimplemented!()
}

/// `path` equals `prefix` or continues with `\` right after it (component-boundary prefix),
/// under `c`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn starts_with_at_boundary(path: &[u16], prefix: &[u16], c: Cmp) -> bool {
    unimplemented!()
}
