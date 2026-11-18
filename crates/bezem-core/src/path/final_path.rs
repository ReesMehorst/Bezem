// SPDX-License-Identifier: MIT

//! Validation of `GetFinalPathNameByHandleW` output.
//!
//! Ownership: work package **B**.

use super::{PathReject, WidePath};

/// Parses an OS final path. Accepts only `\\?\X:\...` (strict); rejects `\\?\UNC\`
/// ([`PathReject::UncFinal`]), `\\?\Volume{`, `GLOBALROOT`, `\\.\`, `\??\` and anything else
/// ([`PathReject::BadFinalPathForm`]). Same as [`WidePath::from_final`].
pub fn parse_final(raw: &[u16]) -> Result<WidePath, PathReject> {
    WidePath::from_final(raw)
}
