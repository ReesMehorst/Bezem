// SPDX-License-Identifier: MIT

//! Name-component checks: template components (strict) and listed names (lenient), reserved
//! device names and the 8.3 short-name detector.
//!
//! Ownership: work package **B**.

use super::PathReject;

/// Strict check for a component that comes from a root template (M1 design section 4.1 step 5):
/// rejects empty, `.`, `..`, `:`, `<>"|`, control chars, `*`/`?`, trailing dot or space, `~`
/// followed by an ASCII digit (8.3), reserved device names and components over 255 units.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn check_template_component(c: &[u16]) -> Result<(), PathReject> {
    unimplemented!()
}

/// Lenient check for a name returned by a directory listing: rejects only empty, `.`, `..`,
/// `\`, `/`, `:`, NUL and names over 255 units. Listed names are never re-parsed.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (B)
pub fn check_listed_name(c: &[u16]) -> Result<(), PathReject> {
    unimplemented!()
}
