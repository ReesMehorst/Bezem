// SPDX-License-Identifier: MIT

//! Per-thread LRU cache of open parent-directory handles for the deleter.
//!
//! Ownership: work package **C**. Cached directory handles lack `FILE_SHARE_DELETE`, so the
//! empty-directory pass must call `evict_subtree` before reopening a directory with `DELETE`.

use std::sync::Arc;

use crate::walk::{DirIndex, DirTable};

/// Default number of cached handles per worker.
#[allow(dead_code)] // M1-STUB (C)
pub(crate) const DEFAULT_CAPACITY: usize = 64;

/// LRU of `DirIndex -> open directory handle`.
#[derive(Debug)]
#[allow(dead_code)] // M1-STUB (C)
pub(crate) struct DirCache<D> {
    capacity: usize,
    /// Most recently used last.
    entries: Vec<(DirIndex, Arc<D>)>,
}

#[allow(dead_code)] // M1-STUB (C)
impl<D> DirCache<D> {
    /// An empty cache holding at most `capacity` handles.
    pub(crate) fn new(capacity: usize) -> Self {
        DirCache {
            capacity,
            entries: Vec::new(),
        }
    }

    /// Drops the cached handles of `dir` and every directory below it.
    #[allow(clippy::unimplemented, unused_variables)] // M1-STUB (C)
    pub(crate) fn evict_subtree(&mut self, dir: DirIndex, table: &DirTable) {
        unimplemented!()
    }
}

impl<D> Default for DirCache<D> {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}
