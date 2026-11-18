// SPDX-License-Identifier: MIT

//! `nt_open_relative`: `NtCreateFile` relative to a parent directory handle
//! (`OBJECT_ATTRIBUTES.RootDirectory`), `FILE_OPEN`, `OBJ_CASE_INSENSITIVE`.
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::error::NtError;

/// Opens `name` (one component, at most 255 units) relative to `parent` with the given access,
/// share and create options. `NTSTATUS < 0` => `NtError { api: "NtCreateFile", status }`.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn nt_open_relative(
    parent: Option<&OwnedHandle>,
    name: &[u16],
    access: u32,
    share: u32,
    options: u32,
) -> Result<OwnedHandle, NtError> {
    unimplemented!()
}
