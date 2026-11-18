// SPDX-License-Identifier: MIT

//! `impl FsBackend for Win32Fs` (flags and status mapping per M1 design section 3.1).
//!
//! Ownership: work package **A**.

use super::OwnedHandle;
use crate::fs::{
    DispositionError, DispositionKind, DispositionOpts, FsBackend, FsError, IdQuality, ItemKind,
    ItemQuery, OpenRootError, RawEntry, RootOpenMode, RootProbe,
};
use crate::path::WidePath;
use std::ops::ControlFlow;

/// The real Windows backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32Fs {
    /// Adds `FILE_OPEN_FOR_BACKUP_INTENT` to directory opens. Always false in M1.
    backup_intent: bool,
}

/// An open directory handle (no `FILE_SHARE_DELETE`). Opaque: the raw handle never leaves
/// `win32`.
#[derive(Debug)]
pub struct Win32Dir(OwnedHandle);

/// An item opened for delete. Opaque: the raw handle never leaves `win32`.
#[derive(Debug)]
pub struct Win32Item(OwnedHandle);

#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (A)
impl FsBackend for Win32Fs {
    type Dir = Win32Dir;
    type Item = Win32Item;

    fn open_root(
        &self,
        path: &WidePath,
        mode: RootOpenMode,
    ) -> Result<RootProbe<Self::Dir>, OpenRootError> {
        unimplemented!()
    }

    fn open_subdir(&self, parent: &Self::Dir, name: &[u16]) -> Result<Self::Dir, FsError> {
        unimplemented!()
    }

    fn read_dir(
        &self,
        dir: &Self::Dir,
        sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<IdQuality, FsError> {
        unimplemented!()
    }

    fn open_item(
        &self,
        parent: &Self::Dir,
        name: &[u16],
        kind: ItemKind,
    ) -> Result<Self::Item, FsError> {
        unimplemented!()
    }

    fn query_item(&self, item: &Self::Item) -> Result<ItemQuery, FsError> {
        unimplemented!()
    }

    fn final_path(&self, item: &Self::Item) -> Result<Vec<u16>, FsError> {
        unimplemented!()
    }

    fn set_disposition(
        &self,
        item: &Self::Item,
        o: DispositionOpts,
    ) -> Result<DispositionKind, DispositionError> {
        unimplemented!()
    }
}
