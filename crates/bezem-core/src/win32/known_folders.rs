// SPDX-License-Identifier: MIT

//! [`SystemKnownFolders`]: known-folder values from the OS. `Temp` via `GetTempPath2W`
//! (resolved at runtime, `GetTempPathW` fallback; computed once). Others via
//! `SHGetKnownFolderPath(KF_FLAG_DEFAULT)`, freed with `CoTaskMemFree` in a drop guard.
//!
//! Ownership: work package **A**.

use crate::resolve::{KnownFolderError, KnownFolderId, KnownFolderSource};

/// The production [`KnownFolderSource`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemKnownFolders;

#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
impl KnownFolderSource for SystemKnownFolders {
    fn get(&self, id: KnownFolderId) -> Result<Vec<u16>, KnownFolderError> {
        unimplemented!()
    }
}
