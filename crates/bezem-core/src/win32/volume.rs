// SPDX-License-Identifier: MIT

//! Local-fixed-volume check: `NtQueryVolumeInformationFile(FileFsDeviceInformation)`
//! (`FILE_REMOTE_DEVICE` => Remote) and `GetDriveTypeW`.
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::error::NtError;
use crate::fs::VolumeKind;

/// `FILE_REMOTE_DEVICE` (defined locally).
pub(crate) const FILE_REMOTE_DEVICE: u32 = 0x10;

/// The kind of volume `h` (whose final path is `final_path`) lives on.
#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
pub(crate) fn volume_kind(h: &OwnedHandle, final_path: &[u16]) -> Result<VolumeKind, NtError> {
    unimplemented!()
}
