// SPDX-License-Identifier: MIT

//! Scan plan types: interned directory table and per-file plan entries (ARCHITECTURE
//! A:321-352). Plans are sealed: only the walker builds them and only the deleter consumes
//! them.
//!
//! Ownership: work package **C**.

use std::sync::Arc;

use crate::fs::IdQuality;
use crate::resolve::ResolvedRoot;
use crate::target::Target;
use crate::types::{CleanerId, FileIdentity, FileTime};

const SLASH: u16 = b'/' as u16;

/// Index into a [`DirTable`]; `DirIndex(0)` is the root.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DirIndex(pub u32);

impl DirIndex {
    /// The root directory.
    pub const ROOT: DirIndex = DirIndex(0);
}

/// One interned directory. The root has `parent: None` and an empty name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirNode {
    pub parent: Option<DirIndex>,
    pub name: Box<[u16]>,
    pub identity: FileIdentity,
    /// The directory itself passed the verdict and may be removed if left empty.
    pub eligible_empty_candidate: bool,
}

/// Directory paths interned once, as parent pointers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirTable {
    nodes: Vec<DirNode>,
}

impl DirTable {
    /// A table holding only the root (index 0).
    pub fn with_root(root_identity: FileIdentity) -> Self {
        DirTable {
            nodes: vec![DirNode {
                parent: None,
                name: Box::new([]),
                identity: root_identity,
                eligible_empty_candidate: false,
            }],
        }
    }

    /// Appends a node and returns its index (`None` if the table is full).
    pub fn push(&mut self, node: DirNode) -> Option<DirIndex> {
        let i = u32::try_from(self.nodes.len()).ok()?;
        self.nodes.push(node);
        Some(DirIndex(i))
    }

    /// The node at `i`.
    pub fn get(&self, i: DirIndex) -> Option<&DirNode> {
        self.nodes.get(usize::try_from(i.0).ok()?)
    }

    /// Number of nodes (root included).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` when the table has no nodes (not even a root).
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Component names from the root (exclusive) down to `i` (inclusive).
    pub fn rel_components(&self, i: DirIndex) -> Vec<&[u16]> {
        let mut out = Vec::new();
        let mut cur = Some(i);
        // Bounded by the table size, so a corrupt parent cycle cannot loop forever.
        for _ in 0..self.nodes.len() {
            let Some(node) = cur.and_then(|c| self.get(c)) else {
                break;
            };
            if node.parent.is_none() {
                break;
            }
            out.push(&*node.name);
            cur = node.parent;
        }
        out.reverse();
        out
    }

    /// Depth of `i` below the root (root = 0).
    pub fn depth(&self, i: DirIndex) -> u16 {
        u16::try_from(self.rel_components(i).len()).unwrap_or(u16::MAX)
    }

    /// Writes the `/`-joined relative path of `i` (plus `leaf`, if any) into `out` (cleared
    /// first).
    pub fn rel_slash(&self, i: DirIndex, leaf: Option<&[u16]>, out: &mut Vec<u16>) {
        out.clear();
        for c in self.rel_components(i).into_iter().chain(leaf) {
            if !out.is_empty() {
                out.push(SLASH);
            }
            out.extend_from_slice(c);
        }
    }
}

/// One file the scan found eligible (about 72 bytes plus the name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub dir: DirIndex,
    pub name: Box<[u16]>,
    pub identity: FileIdentity,
    pub size: u64,
    pub last_write: FileTime,
}

/// The sealed plan for one target.
#[derive(Debug)]
#[allow(dead_code)] // M1-STUB (C): fields are read by the deleter
pub struct TargetPlan {
    pub(crate) cleaner: CleanerId,
    pub(crate) root: ResolvedRoot,
    pub(crate) target: Arc<Target>,
    pub(crate) root_identity: FileIdentity,
    pub(crate) id_quality: IdQuality,
    pub(crate) dirs: DirTable,
    pub(crate) files: Vec<PlanEntry>,
    /// Post-order (children before parents).
    pub(crate) empty_dir_candidates: Vec<DirIndex>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Box<[u16]> {
        s.encode_utf16().collect()
    }

    #[test]
    fn rel_paths() {
        let id = FileIdentity {
            volume_serial: 1,
            file_id: [0; 16],
        };
        let mut t = DirTable::with_root(id);
        let node = |parent, name| DirNode {
            parent: Some(parent),
            name: w(name),
            identity: id,
            eligible_empty_candidate: false,
        };
        let a = t.push(node(DirIndex::ROOT, "a")).unwrap();
        let b = t.push(node(a, "b")).unwrap();
        assert_eq!(t.depth(DirIndex::ROOT), 0);
        assert_eq!(t.depth(b), 2);
        let mut out = Vec::new();
        t.rel_slash(b, Some(&w("f.txt")), &mut out);
        assert_eq!(String::from_utf16_lossy(&out), "a/b/f.txt");
        t.rel_slash(DirIndex::ROOT, Some(&w("f.txt")), &mut out);
        assert_eq!(String::from_utf16_lossy(&out), "f.txt");
        t.rel_slash(a, None, &mut out);
        assert_eq!(String::from_utf16_lossy(&out), "a");
    }
}
