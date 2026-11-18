// SPDX-License-Identifier: MIT

//! `MemFs`: an in-memory [`FsBackend`] with fault and TOCTOU hooks (M1 design section 3.4).
//! Only compiled with `cfg(test)` or the `test-support` feature.
//!
//! Ownership: work package **D** (Phase 1 deliverable needed by B and C). The internals are
//! placeholders; the public builder API below is the contract B and C write tests against.
//!
//! Paths given to the builder are absolute Windows paths as `&str` (`C:\a\b`, `\\?\` optional).
//! Semantics to implement: open directory handles pin their node (replace/remove through the
//! test API returns a sharing violation, mirroring no `FILE_SHARE_DELETE`); case-insensitive
//! (ASCII) name lookup; `open_subdir`/`open_item` on a junction or symlink report a reparse
//! point and never follow; ancestors of a root ARE followed (relocated-profile tests);
//! `final_path` is computed from the real parent chain.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use std::collections::BTreeMap;
use std::fmt;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use super::{
    DispositionError, DispositionKind, DispositionOpts, FsBackend, FsError, IdQuality, ItemKind,
    ItemQuery, OpenRootError, RawEntry, RootOpenMode, RootProbe, VolumeKind,
};
use crate::path::WidePath;
use crate::types::FileTime;

/// Attributes and times of a file added with [`MemFs::add_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSpec {
    pub size: u64,
    pub attrs: u32,
    pub last_write: FileTime,
    pub change: FileTime,
    /// Hard-link count (1 = not hard-linked).
    pub links: u32,
    /// `None` = auto-assign a unique id.
    pub id: Option<[u8; 16]>,
}

impl Default for FileSpec {
    fn default() -> Self {
        FileSpec {
            size: 0,
            attrs: 0,
            last_write: FileTime(0),
            change: FileTime(0),
            links: 1,
            id: None,
        }
    }
}

/// The operation about to run, passed to the hook with the node's display path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsOp {
    OpenRoot(String),
    OpenSubdir(String),
    ReadDir(String),
    OpenItem(String),
    Query(String),
    FinalPath(String),
    Dispose(String),
}

/// Hook called BEFORE each backend operation (without holding the tree lock), for TOCTOU
/// injection such as swapping a scanned file for a junction after N operations.
pub type MemHook = Box<dyn Fn(&FsOp, &MemFs) + Send + Sync>;

/// Placeholder for the in-memory tree (D replaces this).
#[derive(Debug, Default)]
struct MemTree {}

/// An in-memory file system. Cheap to clone (shared state).
#[derive(Clone, Default)]
#[allow(dead_code)] // M1-STUB (D)
pub struct MemFs {
    inner: Arc<Mutex<MemTree>>,
    hook: Arc<Mutex<Option<MemHook>>>,
}

impl fmt::Debug for MemFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemFs").finish_non_exhaustive()
    }
}

/// An open directory in [`MemFs`].
#[derive(Debug)]
pub struct MemDir {}

/// An item opened for delete in [`MemFs`].
#[derive(Debug)]
pub struct MemItem {}

#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (D)
impl MemFs {
    /// An empty file system.
    pub fn new() -> MemFs {
        MemFs::default()
    }
    /// Creates a directory (and missing ancestors).
    pub fn add_dir(&self, path: &str) {
        unimplemented!()
    }
    /// Creates a file (and missing ancestor directories).
    pub fn add_file(&self, path: &str, spec: FileSpec) {
        unimplemented!()
    }
    /// Creates a directory junction at `path` pointing at `target`.
    pub fn add_junction(&self, path: &str, target: &str) {
        unimplemented!()
    }
    /// Creates a file symbolic link at `path` pointing at `target`.
    pub fn add_symlink_file(&self, path: &str, target: &str) {
        unimplemented!()
    }
    /// While set, `open_item` on `path` fails with a sharing violation (`Locked`).
    pub fn set_locked(&self, path: &str, locked: bool) {
        unimplemented!()
    }
    /// Opens of `path` fail with access denied.
    pub fn set_acl_denied(&self, path: &str) {
        unimplemented!()
    }
    /// Paths starting with `path_prefix` report `kind` from `open_root`.
    pub fn set_volume(&self, path_prefix: &str, kind: VolumeKind) {
        unimplemented!()
    }
    /// `final_path` of `path` returns `raw` instead of the computed path (tripwire tests).
    pub fn set_final_path_override(&self, path: &str, raw: Vec<u16>) {
        unimplemented!()
    }
    /// Forces `DispositionKind::Legacy` (no POSIX delete; read-only files then fail).
    pub fn set_no_posix_delete(&self, on: bool) {
        unimplemented!()
    }
    /// Caps the listing id quality (simulates FAT-type volumes).
    pub fn set_no_extd_ids(&self, quality: IdQuality) {
        unimplemented!()
    }
    /// Installs the TOCTOU hook.
    pub fn set_hook(&self, hook: MemHook) {
        unimplemented!()
    }
    /// Path -> (kind, size) for every node, for before/after manifests.
    pub fn snapshot(&self) -> BTreeMap<String, (ItemKind, u64)> {
        unimplemented!()
    }
    /// Whether `path` exists (no following of the final component).
    pub fn exists(&self, path: &str) -> bool {
        unimplemented!()
    }
}

#[allow(clippy::unimplemented, unused_variables)] // M1-STUB (D)
impl FsBackend for MemFs {
    type Dir = MemDir;
    type Item = MemItem;

    fn open_root(
        &self,
        path: &WidePath,
        mode: RootOpenMode,
    ) -> Result<RootProbe<MemDir>, OpenRootError> {
        unimplemented!()
    }

    fn open_subdir(&self, parent: &MemDir, name: &[u16]) -> Result<MemDir, FsError> {
        unimplemented!()
    }

    fn read_dir(
        &self,
        dir: &MemDir,
        sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<IdQuality, FsError> {
        unimplemented!()
    }

    fn open_item(&self, parent: &MemDir, name: &[u16], kind: ItemKind) -> Result<MemItem, FsError> {
        unimplemented!()
    }

    fn query_item(&self, item: &MemItem) -> Result<ItemQuery, FsError> {
        unimplemented!()
    }

    fn final_path(&self, item: &MemItem) -> Result<Vec<u16>, FsError> {
        unimplemented!()
    }

    fn set_disposition(
        &self,
        item: &MemItem,
        o: DispositionOpts,
    ) -> Result<DispositionKind, DispositionError> {
        unimplemented!()
    }
}
