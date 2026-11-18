// SPDX-License-Identifier: MIT

//! `MemFs`: an in-memory [`FsBackend`] with fault and TOCTOU hooks (M1 design section 3.4).
//! Only compiled with `cfg(test)` or the `test-support` feature.
//!
//! Ownership: work package **D**.
//!
//! `MemFs` mimics the NTFS behaviours the safety and delete engines depend on:
//!
//! - Names are looked up case-insensitively (ASCII fold); listings return the stored casing,
//!   sorted by folded name like NTFS.
//! - `open_subdir` on a junction or symlink returns `Err(ReparsePoint)`. `open_item` on one
//!   succeeds (like `FILE_OPEN_REPARSE_POINT`) and `query_item` reports the reparse attribute
//!   and tag. Nothing below the root is ever followed. Ancestors of a path ARE followed
//!   (relocated profiles), by `open_root` and by every path-taking test API.
//! - Open directory handles pin their node (no `FILE_SHARE_DELETE`): removing, replacing or
//!   renaming a pinned directory (or any ancestor of a pinned node) through the test API fails
//!   with a sharing violation. An item handle (opened with DELETE access) is exclusive.
//! - Status codes come from [`super::status`] constants, so the status mapping is exercised.
//! - The hook runs BEFORE each backend operation, without the tree lock held, so it may mutate
//!   the tree (TOCTOU injection). The builder and `try_*` API never fire the hook.
//!
//! Builder paths are absolute Windows paths (`C:\a\b`, `\\?\` optional, `/` accepted). Misuse
//! of the builder panics with a clear message; the `try_*` methods return errors instead.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::attr;
use super::status::{
    ERROR_ACCESS_DENIED, ERROR_USER_MAPPED_FILE, STATUS_ACCESS_DENIED, STATUS_DELETE_PENDING,
    STATUS_FILE_IS_A_DIRECTORY, STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_COLLISION,
    STATUS_OBJECT_NAME_NOT_FOUND, STATUS_SHARING_VIOLATION,
};
use super::{
    check_open_name, DispositionError, DispositionKind, DispositionOpts, FsBackend, FsError,
    IdQuality, ItemKind, ItemQuery, OpenRootError, RawEntry, RootOpenMode, RootProbe, VolumeKind,
};
use crate::error::ErrorCode;
use crate::path::WidePath;
use crate::types::{FileIdentity, FileTime, SkipReason};

/// `IO_REPARSE_TAG_MOUNT_POINT`: the tag reported for junctions.
pub const TAG_MOUNT_POINT: u32 = 0xA000_0003;
/// `IO_REPARSE_TAG_SYMLINK`: the tag reported for symbolic links.
pub const TAG_SYMLINK: u32 = 0xA000_000C;

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

/// The operation about to run, passed to the hook with the node's display path
/// (`C:\a\b`, no `\\?\`). For `OpenSubdir`/`OpenItem` this is the parent path plus the
/// requested name, whether or not it exists yet.
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

/// What a node is, as reported by [`MemFs::snapshot_detailed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Dir,
    File,
    Junction { target: String },
    SymlinkFile { target: String },
}

/// Everything observable about a node, for byte-for-byte before/after manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSummary {
    pub node_type: NodeType,
    pub size: u64,
    pub attrs: u32,
    pub last_write: FileTime,
    pub change: FileTime,
    pub links: u32,
    pub id: [u8; 16],
}

// ---------------------------------------------------------------------------------------
// Errors (all built from the real status constants)
// ---------------------------------------------------------------------------------------

fn nt(reason: SkipReason, status: i32) -> FsError {
    FsError::new(reason, ErrorCode::Nt(status))
}
fn e_gone() -> FsError {
    nt(SkipReason::Gone, STATUS_OBJECT_NAME_NOT_FOUND)
}
fn e_sharing() -> FsError {
    nt(SkipReason::Locked, STATUS_SHARING_VIOLATION)
}
fn e_denied() -> FsError {
    nt(SkipReason::AccessDenied, STATUS_ACCESS_DENIED)
}
fn e_pending() -> FsError {
    nt(SkipReason::DeletePending, STATUS_DELETE_PENDING)
}
fn e_not_dir() -> FsError {
    nt(SkipReason::ChangedSinceScan, STATUS_NOT_A_DIRECTORY)
}
fn e_is_dir() -> FsError {
    nt(SkipReason::ChangedSinceScan, STATUS_FILE_IS_A_DIRECTORY)
}
fn e_collision() -> FsError {
    nt(SkipReason::FsError, STATUS_OBJECT_NAME_COLLISION)
}
fn e_reparse() -> FsError {
    FsError::new(SkipReason::ReparsePoint, ErrorCode::None)
}
fn e_readonly() -> FsError {
    FsError::new(SkipReason::ReadOnly, ErrorCode::Win32(ERROR_ACCESS_DENIED))
}
fn e_mapped() -> FsError {
    FsError::new(SkipReason::Locked, ErrorCode::Win32(ERROR_USER_MAPPED_FILE))
}
fn e_loop() -> FsError {
    FsError::new(SkipReason::FsError, ErrorCode::Sim("reparse_loop"))
}

// ---------------------------------------------------------------------------------------
// Wide-string helpers
// ---------------------------------------------------------------------------------------

const BS: u16 = b'\\' as u16;
const MAX_FOLLOWS: u32 = 32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn fold(u: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&u) {
        u + 32
    } else {
        u
    }
}

fn folded(name: &[u16]) -> Vec<u16> {
    name.iter().map(|&u| fold(u)).collect()
}

fn name_eq(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| fold(x) == fold(y))
}

fn is_sep(u: u16) -> bool {
    u == BS || u == u16::from(b'/')
}

/// Parsed absolute path: upper-case drive letter and components.
type Abs = (u16, Vec<Vec<u16>>);

/// `X:\a\b`, optionally prefixed by `\\?\`. `.`/`..` components are rejected.
fn parse_abs(raw: &[u16]) -> Option<Abs> {
    let prefix = wide(r"\\?\");
    let s = raw.strip_prefix(prefix.as_slice()).unwrap_or(raw);
    let (&d, rest) = s.split_first()?;
    let (&colon, rest) = rest.split_first()?;
    let letter = u8::try_from(d).ok().filter(u8::is_ascii_alphabetic)?;
    if colon != u16::from(b':') {
        return None;
    }
    if rest.first().is_some_and(|&u| !is_sep(u)) {
        return None;
    }
    let mut comps = Vec::new();
    for c in rest.split(|&u| is_sep(u)).filter(|c| !c.is_empty()) {
        if c == [u16::from(b'.')] || c == [u16::from(b'.'), u16::from(b'.')] {
            return None;
        }
        comps.push(c.to_vec());
    }
    Some((u16::from(letter.to_ascii_uppercase()), comps))
}

fn parse_abs_or_panic(raw: &[u16]) -> Abs {
    parse_abs(raw).unwrap_or_else(|| {
        panic!(
            "MemFs: not an absolute drive path: {:?}",
            String::from_utf16_lossy(raw)
        )
    })
}

fn abs_display(drive: u16, comps: &[Vec<u16>]) -> Vec<u16> {
    let mut out = vec![drive, u16::from(b':'), BS];
    for (i, c) in comps.iter().enumerate() {
        if i > 0 {
            out.push(BS);
        }
        out.extend_from_slice(c);
    }
    out
}

fn s(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

// ---------------------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct LinkTarget {
    drive: u16,
    comps: Vec<Vec<u16>>,
}

#[derive(Debug, Clone)]
enum Data {
    Dir { children: Vec<u64> },
    File,
    Junction(LinkTarget),
    Symlink(LinkTarget),
}

#[derive(Debug, Clone)]
struct Node {
    name: Vec<u16>,
    parent: Option<u64>,
    drive: u16,
    data: Data,
    attrs: u32,
    size: u64,
    last_write: FileTime,
    change: FileTime,
    links: u32,
    file_id: [u8; 16],
    locked: bool,
    acl_denied: bool,
    mapped: bool,
    final_override: Option<Vec<u16>>,
    /// Open directory handles (no SHARE_DELETE).
    pin: u32,
    /// An item handle (DELETE access, exclusive) is open.
    item_open: bool,
    /// Legacy disposition set: removed when the item handle closes.
    delete_pending: bool,
    /// Detached from the namespace.
    deleted: bool,
    /// Display path at the moment of detaching.
    last_path: Vec<u16>,
}

impl Node {
    fn is_dir_type(&self) -> bool {
        matches!(self.data, Data::Dir { .. } | Data::Junction(_))
    }
    fn reparse_tag(&self) -> u32 {
        match self.data {
            Data::Junction(_) => TAG_MOUNT_POINT,
            Data::Symlink(_) => TAG_SYMLINK,
            _ => 0,
        }
    }
    fn attributes(&self) -> u32 {
        let mut a = self.attrs;
        match self.data {
            Data::Dir { .. } => a |= attr::DIRECTORY,
            Data::File => a &= !attr::DIRECTORY,
            Data::Junction(_) => a |= attr::DIRECTORY | attr::REPARSE_POINT,
            Data::Symlink(_) => {
                a &= !attr::DIRECTORY;
                a |= attr::REPARSE_POINT;
            }
        }
        a
    }
    fn is_reparse(&self) -> bool {
        attr::is_reparse(self.attributes(), self.reparse_tag())
    }
    fn size_reported(&self) -> u64 {
        if matches!(self.data, Data::File) {
            self.size
        } else {
            0
        }
    }
    fn allocated(&self) -> u64 {
        self.size_reported().div_ceil(4096) * 4096
    }
}

#[derive(Debug)]
enum Fail {
    NotFound,
    Loop,
}

#[derive(Debug)]
struct MemTree {
    nodes: HashMap<u64, Node>,
    next_id: u64,
    drives: BTreeMap<u16, u64>,
    volumes: Vec<(Abs, VolumeKind)>,
    no_posix_delete: bool,
    id_quality: IdQuality,
}

impl Default for MemTree {
    fn default() -> Self {
        MemTree {
            nodes: HashMap::new(),
            next_id: 1,
            drives: BTreeMap::new(),
            volumes: Vec::new(),
            no_posix_delete: false,
            id_quality: IdQuality::Full128,
        }
    }
}

fn id_bytes(n: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&n.to_le_bytes());
    b
}

impl MemTree {
    fn node(&self, id: u64) -> &Node {
        self.nodes.get(&id).expect("MemFs: dangling node id")
    }
    fn node_mut(&mut self, id: u64) -> &mut Node {
        self.nodes.get_mut(&id).expect("MemFs: dangling node id")
    }

    fn alloc(&mut self, name: Vec<u16>, parent: Option<u64>, drive: u16, data: Data) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            Node {
                name,
                parent,
                drive,
                data,
                attrs: 0,
                size: 0,
                last_write: FileTime(0),
                change: FileTime(0),
                links: 1,
                file_id: id_bytes(id),
                locked: false,
                acl_denied: false,
                mapped: false,
                final_override: None,
                pin: 0,
                item_open: false,
                delete_pending: false,
                deleted: false,
                last_path: Vec::new(),
            },
        );
        id
    }

    fn root_of(&mut self, drive: u16) -> u64 {
        if let Some(&r) = self.drives.get(&drive) {
            return r;
        }
        let r = self.alloc(
            vec![drive, u16::from(b':')],
            None,
            drive,
            Data::Dir {
                children: Vec::new(),
            },
        );
        self.drives.insert(drive, r);
        r
    }

    fn find_child(&self, dir: u64, name: &[u16]) -> Option<u64> {
        match &self.node(dir).data {
            Data::Dir { children } => children
                .iter()
                .copied()
                .find(|&c| name_eq(&self.node(c).name, name)),
            _ => None,
        }
    }

    fn link_child(&mut self, parent: u64, child: u64) {
        if let Data::Dir { children } = &mut self.node_mut(parent).data {
            children.push(child);
        } else {
            panic!("MemFs: parent is not a directory");
        }
    }

    /// Display path of a node (`C:\a\b`); the stored path once detached.
    fn node_path(&self, id: u64) -> Vec<u16> {
        let n = self.node(id);
        if n.deleted {
            return n.last_path.clone();
        }
        let mut names: Vec<Vec<u16>> = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let nn = self.node(c);
            if nn.parent.is_some() {
                names.push(nn.name.clone());
            }
            cur = nn.parent;
        }
        names.reverse();
        abs_display(n.drive, &names)
    }

    /// Resolves `drive` + `comps`, following every junction/symlink on the way (the final
    /// component only when `follow_final`).
    fn lookup(&self, drive: u16, comps: &[Vec<u16>], follow_final: bool) -> Result<u64, Fail> {
        let mut follows = 0;
        let mut work: VecDeque<Vec<u16>> = comps.iter().cloned().collect();
        let mut cur = *self.drives.get(&drive).ok_or(Fail::NotFound)?;
        while let Some(c) = work.pop_front() {
            if !matches!(self.node(cur).data, Data::Dir { .. }) {
                return Err(Fail::NotFound);
            }
            let ch = self.find_child(cur, &c).ok_or(Fail::NotFound)?;
            let last = work.is_empty();
            match &self.node(ch).data {
                Data::Junction(t) | Data::Symlink(t) if !last || follow_final => {
                    follows += 1;
                    if follows > MAX_FOLLOWS {
                        return Err(Fail::Loop);
                    }
                    for tc in t.comps.iter().rev() {
                        work.push_front(tc.clone());
                    }
                    cur = *self.drives.get(&t.drive).ok_or(Fail::NotFound)?;
                }
                _ => cur = ch,
            }
        }
        Ok(cur)
    }

    /// `mkdir -p`, following junctions on the way. Panics on conflicts (builder misuse).
    fn ensure_dir_chain(&mut self, drive: u16, comps: &[Vec<u16>]) -> u64 {
        let mut cur = self.root_of(drive);
        for c in comps {
            match self.find_child(cur, c) {
                Some(ch) => match &self.node(ch).data {
                    Data::Dir { .. } => cur = ch,
                    Data::Junction(t) => {
                        let t = t.clone();
                        cur = self
                            .lookup(t.drive, &t.comps, true)
                            .unwrap_or_else(|_| panic!("MemFs: dangling junction in builder path"));
                        assert!(
                            matches!(self.node(cur).data, Data::Dir { .. }),
                            "MemFs: junction target is not a directory"
                        );
                    }
                    _ => panic!("MemFs: {:?} is not a directory", s(c)),
                },
                None => {
                    let n = self.alloc(
                        c.clone(),
                        Some(cur),
                        drive,
                        Data::Dir {
                            children: Vec::new(),
                        },
                    );
                    self.link_child(cur, n);
                    cur = n;
                }
            }
        }
        cur
    }

    fn subtree(&self, id: u64) -> Vec<u64> {
        let mut out = vec![id];
        let mut i = 0;
        while let Some(&cur) = out.get(i) {
            i += 1;
            if let Data::Dir { children } = &self.node(cur).data {
                out.extend(children.iter().copied());
            }
        }
        out
    }

    fn subtree_pinned(&self, id: u64) -> bool {
        self.subtree(id).iter().any(|&n| {
            let n = self.node(n);
            n.pin > 0 || n.item_open
        })
    }

    /// Removes `id` (and everything below) from the namespace. Handles keep the nodes.
    fn detach(&mut self, id: u64) {
        let ids = self.subtree(id);
        for &n in &ids {
            let p = self.node_path(n);
            self.node_mut(n).last_path = p;
        }
        if let Some(parent) = self.node(id).parent {
            if let Data::Dir { children } = &mut self.node_mut(parent).data {
                children.retain(|&c| c != id);
            }
        }
        for n in ids {
            self.node_mut(n).deleted = true;
        }
    }

    fn volume_kind(&self, drive: u16, comps: &[Vec<u16>]) -> VolumeKind {
        let mut best: Option<(usize, VolumeKind)> = None;
        for ((d, pc), kind) in &self.volumes {
            let hit = *d == drive
                && pc.len() <= comps.len()
                && pc.iter().zip(comps).all(|(a, b)| name_eq(a, b));
            if hit && best.is_none_or(|(len, _)| pc.len() >= len) {
                best = Some((pc.len(), *kind));
            }
        }
        best.map_or(VolumeKind::LocalFixed, |(_, k)| k)
    }

    fn serial(drive: u16) -> u64 {
        0x5EED_0000 + u64::from(drive)
    }

    /// Creates `data` as a child of `parent`, replacing an existing entry of that name.
    /// A pinned existing entry => sharing violation.
    fn put(&mut self, parent: u64, name: &[u16], data: Data) -> Result<u64, FsError> {
        let drive = self.node(parent).drive;
        if let Some(old) = self.find_child(parent, name) {
            if self.subtree_pinned(old) {
                return Err(e_sharing());
            }
            self.detach(old);
        }
        let id = self.alloc(name.to_vec(), Some(parent), drive, data);
        self.link_child(parent, id);
        Ok(id)
    }
}

fn apply_spec(n: &mut Node, spec: FileSpec) {
    n.attrs = spec.attrs & !attr::DIRECTORY;
    n.size = spec.size;
    n.last_write = spec.last_write;
    n.change = spec.change;
    n.links = spec.links;
    if let Some(id) = spec.id {
        n.file_id = id;
    }
}

fn target_of(path: &str) -> LinkTarget {
    let (drive, comps) = parse_abs_or_panic(&wide(path));
    LinkTarget { drive, comps }
}

fn lock_tree(m: &Mutex<MemTree>) -> MutexGuard<'_, MemTree> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An in-memory file system. Cheap to clone (shared state).
#[derive(Clone, Default)]
pub struct MemFs {
    inner: Arc<Mutex<MemTree>>,
    hook: Arc<Mutex<Option<Arc<MemHook>>>>,
}

impl fmt::Debug for MemFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemFs").finish_non_exhaustive()
    }
}

/// An open directory in [`MemFs`]. Pins its node until dropped.
#[derive(Debug)]
pub struct MemDir {
    tree: Arc<Mutex<MemTree>>,
    node: u64,
}

impl Drop for MemDir {
    fn drop(&mut self) {
        let mut t = lock_tree(&self.tree);
        let n = t.node_mut(self.node);
        n.pin = n.pin.saturating_sub(1);
    }
}

/// An item opened for delete in [`MemFs`]. Exclusive (DELETE access, `FILE_SHARE_READ` only)
/// until dropped; a legacy disposition takes effect when the handle drops.
#[derive(Debug)]
pub struct MemItem {
    tree: Arc<Mutex<MemTree>>,
    node: u64,
}

impl Drop for MemItem {
    fn drop(&mut self) {
        let mut t = lock_tree(&self.tree);
        let (pending, deleted, non_empty) = {
            let n = t.node_mut(self.node);
            n.item_open = false;
            let non_empty = matches!(&n.data, Data::Dir { children } if !children.is_empty());
            (n.delete_pending, n.deleted, non_empty)
        };
        if pending && !deleted && !non_empty {
            t.detach(self.node);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Builder / test API
// ---------------------------------------------------------------------------------------

impl MemFs {
    /// An empty file system.
    pub fn new() -> MemFs {
        MemFs::default()
    }

    fn tree(&self) -> MutexGuard<'_, MemTree> {
        lock_tree(&self.inner)
    }

    fn fire(&self, op: FsOp) {
        let h = self
            .hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(h) = h {
            h(&op, self);
        }
    }

    /// Node at `path` (ancestors followed, final component not); panics when missing.
    fn expect_node(t: &MemTree, path: &str) -> u64 {
        let (d, c) = parse_abs_or_panic(&wide(path));
        t.lookup(d, &c, false)
            .unwrap_or_else(|_| panic!("MemFs: no such path {path:?}"))
    }

    /// Creates a directory (and missing ancestors). Existing directories are left alone; an
    /// existing non-directory is replaced.
    pub fn add_dir(&self, path: &str) {
        self.add_dir_wide(&wide(path));
    }

    /// [`Self::add_dir`] with a UTF-16 path (names that are not valid UTF-8/UTF-16 pairs).
    pub fn add_dir_wide(&self, path: &[u16]) {
        let (d, c) = parse_abs_or_panic(path);
        let mut t = self.tree();
        let Some((name, parents)) = c.split_last() else {
            t.root_of(d);
            return;
        };
        let p = t.ensure_dir_chain(d, parents);
        if let Some(ex) = t.find_child(p, name) {
            if matches!(t.node(ex).data, Data::Dir { .. }) {
                return;
            }
        }
        t.put(
            p,
            name,
            Data::Dir {
                children: Vec::new(),
            },
        )
        .unwrap_or_else(|e| panic!("MemFs::add_dir: {e:?}"));
    }

    /// Creates a file (and missing ancestor directories), replacing any existing entry (a new
    /// file id is assigned unless `spec.id` is set).
    pub fn add_file(&self, path: &str, spec: FileSpec) {
        self.add_file_wide(&wide(path), spec);
    }

    /// [`Self::add_file`] with a UTF-16 path (lone surrogates, odd names).
    pub fn add_file_wide(&self, path: &[u16], spec: FileSpec) {
        let (d, c) = parse_abs_or_panic(path);
        let (name, parents) = c
            .split_last()
            .expect("MemFs::add_file: path is a drive root");
        let mut t = self.tree();
        let p = t.ensure_dir_chain(d, parents);
        let id = t
            .put(p, name, Data::File)
            .unwrap_or_else(|e| panic!("MemFs::add_file: {e:?}"));
        apply_spec(t.node_mut(id), spec);
    }

    /// Creates a directory junction at `path` pointing at `target` (an absolute path; may be
    /// dangling).
    pub fn add_junction(&self, path: &str, target: &str) {
        self.add_link(path, Data::Junction(target_of(target)));
    }

    /// Creates a file symbolic link at `path` pointing at `target`.
    pub fn add_symlink_file(&self, path: &str, target: &str) {
        self.add_link(path, Data::Symlink(target_of(target)));
    }

    fn add_link(&self, path: &str, data: Data) {
        let (d, c) = parse_abs_or_panic(&wide(path));
        let (name, parents) = c.split_last().expect("MemFs: link path is a drive root");
        let mut t = self.tree();
        let p = t.ensure_dir_chain(d, parents);
        t.put(p, name, data)
            .unwrap_or_else(|e| panic!("MemFs::add_link: {e:?}"));
    }

    /// Changes size and last-write time of an existing file in place (id and attributes are
    /// kept; the change time follows the write time).
    pub fn modify_file(&self, path: &str, size: u64, last_write: FileTime) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        let n = t.node_mut(id);
        assert!(
            matches!(n.data, Data::File),
            "MemFs::modify_file: not a file"
        );
        n.size = size;
        n.last_write = last_write;
        n.change = last_write;
    }

    /// Replaces the user attribute bits of the node (`HIDDEN`, `SYSTEM`, `READONLY`, cloud
    /// bits, ...). `DIRECTORY`/`REPARSE_POINT` for real links come from the node type.
    pub fn set_attrs(&self, path: &str, attrs: u32) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        t.node_mut(id).attrs = attrs;
    }

    /// Sets last-write and change times of any node (directories default to time 0).
    pub fn set_times(&self, path: &str, last_write: FileTime, change: FileTime) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        let n = t.node_mut(id);
        n.last_write = last_write;
        n.change = change;
    }

    /// While set, `open_item` on `path` fails with a sharing violation (`Locked`).
    pub fn set_locked(&self, path: &str, locked: bool) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        t.node_mut(id).locked = locked;
    }

    /// Opens of `path` fail with access denied (listing the parent still shows it).
    pub fn set_acl_denied(&self, path: &str) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        t.node_mut(id).acl_denied = true;
    }

    /// Removes the access-denied flag again.
    pub fn clear_acl_denied(&self, path: &str) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        t.node_mut(id).acl_denied = false;
    }

    /// While set, the file is mapped as an image: `set_disposition` fails with
    /// `ERROR_USER_MAPPED_FILE` (`Locked`) even with POSIX semantics.
    pub fn set_image_mapped(&self, path: &str, mapped: bool) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        t.node_mut(id).mapped = mapped;
    }

    /// Paths starting with `path_prefix` (`D:\`, `C:\Mounts\net`) report `kind` from
    /// `open_root`; the longest matching prefix wins; the default is `LocalFixed`.
    pub fn set_volume(&self, path_prefix: &str, kind: VolumeKind) {
        let abs = parse_abs_or_panic(&wide(path_prefix));
        let mut t = self.tree();
        t.volumes.retain(|(p, _)| {
            !(p.0 == abs.0
                && p.1.len() == abs.1.len()
                && p.1.iter().zip(&abs.1).all(|(a, b)| name_eq(a, b)))
        });
        t.volumes.push((abs, kind));
    }

    /// `final_path` of `path` returns `raw` instead of the computed path (tripwire tests).
    pub fn set_final_path_override(&self, path: &str, raw: Vec<u16>) {
        let mut t = self.tree();
        let id = Self::expect_node(&t, path);
        t.node_mut(id).final_override = Some(raw);
    }

    /// Forces `DispositionKind::Legacy` (no POSIX delete; read-only files then fail).
    pub fn set_no_posix_delete(&self, on: bool) {
        self.tree().no_posix_delete = on;
    }

    /// Caps the listing id quality (simulates FAT-type volumes). Default `Full128`.
    pub fn set_no_extd_ids(&self, quality: IdQuality) {
        self.tree().id_quality = quality;
    }

    /// Installs the TOCTOU hook (replacing any previous one).
    pub fn set_hook(&self, hook: MemHook) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(hook));
    }

    /// Removes the hook.
    pub fn clear_hook(&self) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Removes `path` and everything below it, as an outside actor would. Sharing violation
    /// when the node or anything below it is pinned by an open handle.
    pub fn try_remove(&self, path: &str) -> Result<(), FsError> {
        let (d, c) = parse_abs_or_panic(&wide(path));
        let mut t = self.tree();
        if c.is_empty() {
            return Err(e_denied());
        }
        let id = t.lookup(d, &c, false).map_err(|_| e_gone())?;
        if t.subtree_pinned(id) {
            return Err(e_sharing());
        }
        t.detach(id);
        Ok(())
    }

    /// Moves `from` to `to` on the same drive (ids are kept). Sharing violation when pinned;
    /// name collision when `to` exists or lies below `from`.
    pub fn try_rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        let (d, c) = parse_abs_or_panic(&wide(from));
        let (d2, c2) = parse_abs_or_panic(&wide(to));
        let mut t = self.tree();
        if c.is_empty() || c2.is_empty() || d != d2 {
            return Err(e_collision());
        }
        let id = t.lookup(d, &c, false).map_err(|_| e_gone())?;
        let (new_name, parents) = c2.split_last().ok_or_else(e_collision)?;
        let np = t.lookup(d2, parents, true).map_err(|_| e_gone())?;
        if !matches!(t.node(np).data, Data::Dir { .. }) {
            return Err(e_gone());
        }
        if t.find_child(np, new_name).is_some() || t.subtree(id).contains(&np) {
            return Err(e_collision());
        }
        if t.subtree_pinned(id) {
            return Err(e_sharing());
        }
        if let Some(old_parent) = t.node(id).parent {
            if let Data::Dir { children } = &mut t.node_mut(old_parent).data {
                children.retain(|&x| x != id);
            }
        }
        t.link_child(np, id);
        let n = t.node_mut(id);
        n.parent = Some(np);
        n.name = new_name.clone();
        Ok(())
    }

    fn replace(&self, path: &str, data: Data, spec: Option<FileSpec>) -> Result<(), FsError> {
        let (d, c) = parse_abs_or_panic(&wide(path));
        let mut t = self.tree();
        if c.is_empty() {
            return Err(e_denied());
        }
        let id = t.lookup(d, &c, false).map_err(|_| e_gone())?;
        let (parent, name) = {
            let n = t.node(id);
            (n.parent.ok_or_else(e_denied)?, n.name.clone())
        };
        let new = t.put(parent, &name, data)?;
        if let Some(sp) = spec {
            apply_spec(t.node_mut(new), sp);
        }
        Ok(())
    }

    /// Swaps the existing node at `path` for a junction to `target` (fails with a sharing
    /// violation while the node is pinned).
    pub fn try_replace_with_junction(&self, path: &str, target: &str) -> Result<(), FsError> {
        self.replace(path, Data::Junction(target_of(target)), None)
    }

    /// Swaps the existing node at `path` for a file symlink to `target`.
    pub fn try_replace_with_symlink_file(&self, path: &str, target: &str) -> Result<(), FsError> {
        self.replace(path, Data::Symlink(target_of(target)), None)
    }

    /// Swaps the existing node at `path` for a new file (new id).
    pub fn try_replace_with_file(&self, path: &str, spec: FileSpec) -> Result<(), FsError> {
        self.replace(path, Data::File, Some(spec))
    }

    /// Swaps the existing node at `path` for a new empty directory (new id).
    pub fn try_replace_with_dir(&self, path: &str) -> Result<(), FsError> {
        self.replace(
            path,
            Data::Dir {
                children: Vec::new(),
            },
            None,
        )
    }

    /// Path -> (kind, size) for every node in the namespace (junctions count as `Dir`,
    /// file symlinks as `File`, both size 0; they are never descended into).
    pub fn snapshot(&self) -> BTreeMap<String, (ItemKind, u64)> {
        let t = self.tree();
        let mut out = BTreeMap::new();
        for id in Self::all_nodes(&t) {
            let n = t.node(id);
            let kind = if n.is_dir_type() {
                ItemKind::Dir
            } else {
                ItemKind::File
            };
            out.insert(s(&t.node_path(id)), (kind, n.size_reported()));
        }
        out
    }

    /// Everything observable about every node, keyed by display path.
    pub fn snapshot_detailed(&self) -> BTreeMap<String, NodeSummary> {
        let t = self.tree();
        let mut out = BTreeMap::new();
        for id in Self::all_nodes(&t) {
            let n = t.node(id);
            let tgt = |l: &LinkTarget| s(&abs_display(l.drive, &l.comps));
            let node_type = match &n.data {
                Data::Dir { .. } => NodeType::Dir,
                Data::File => NodeType::File,
                Data::Junction(l) => NodeType::Junction { target: tgt(l) },
                Data::Symlink(l) => NodeType::SymlinkFile { target: tgt(l) },
            };
            out.insert(
                s(&t.node_path(id)),
                NodeSummary {
                    node_type,
                    size: n.size_reported(),
                    attrs: n.attributes(),
                    last_write: n.last_write,
                    change: n.change,
                    links: n.links,
                    id: n.file_id,
                },
            );
        }
        out
    }

    fn all_nodes(t: &MemTree) -> Vec<u64> {
        let mut out = Vec::new();
        for &root in t.drives.values() {
            out.extend(t.subtree(root));
        }
        out
    }

    /// Whether `path` exists (ancestors followed, the final component not).
    pub fn exists(&self, path: &str) -> bool {
        let Some((d, c)) = parse_abs(&wide(path)) else {
            return false;
        };
        self.tree().lookup(d, &c, false).is_ok()
    }

    /// Whether an open handle currently pins `path` (directory handle or item handle).
    pub fn is_pinned(&self, path: &str) -> bool {
        let t = self.tree();
        let id = Self::expect_node(&t, path);
        let n = t.node(id);
        n.pin > 0 || n.item_open
    }

    /// Like [`FsBackend::open_root`] with a `&str` path (`WidePath` construction is not needed
    /// in tests that only exercise the file system).
    pub fn open_root_str(
        &self,
        path: &str,
        mode: RootOpenMode,
    ) -> Result<RootProbe<MemDir>, OpenRootError> {
        self.open_root_wide(&wide(path), mode)
    }

    /// Like [`FsBackend::open_root`] with a raw UTF-16 path (`\\?\` optional).
    pub fn open_root_wide(
        &self,
        raw: &[u16],
        mode: RootOpenMode,
    ) -> Result<RootProbe<MemDir>, OpenRootError> {
        let shown = parse_abs(raw).map_or_else(|| raw.to_vec(), |(d, c)| abs_display(d, &c));
        self.fire(FsOp::OpenRoot(s(&shown)));
        let (drive, comps) = parse_abs(raw).ok_or(OpenRootError::NotFound)?;
        let mut t = self.tree();
        let follow = mode == RootOpenMode::CanonicalizeFollow;
        let id = match t.lookup(drive, &comps, follow) {
            Ok(id) => id,
            Err(Fail::NotFound) => return Err(OpenRootError::NotFound),
            Err(Fail::Loop) => return Err(OpenRootError::Fs(e_loop())),
        };
        let n = t.node(id);
        if n.acl_denied {
            return Err(OpenRootError::Fs(e_denied()));
        }
        if n.item_open {
            return Err(OpenRootError::Fs(e_sharing()));
        }
        let path = t.node_path(id);
        let (rd, rc) = parse_abs(&path).expect("MemFs: node path is absolute");
        let mut final_path = wide(r"\\?\");
        final_path.extend_from_slice(&path);
        let probe = RootProbe {
            dir: MemDir {
                tree: Arc::clone(&self.inner),
                node: id,
            },
            final_path,
            identity: FileIdentity {
                volume_serial: MemTree::serial(n.drive),
                file_id: n.file_id,
            },
            is_directory: n.is_dir_type(),
            is_reparse: n.is_reparse(),
            volume: t.volume_kind(rd, &rc),
        };
        t.node_mut(id).pin += 1;
        Ok(probe)
    }

    fn child_display(&self, parent: u64, name: &[u16]) -> String {
        let t = self.tree();
        let mut p = t.node_path(parent);
        if p.last() != Some(&BS) {
            p.push(BS);
        }
        p.extend_from_slice(name);
        s(&p)
    }

    fn item_display(&self, node: u64) -> String {
        s(&self.tree().node_path(node))
    }
}

impl FsBackend for MemFs {
    type Dir = MemDir;
    type Item = MemItem;

    fn open_root(
        &self,
        path: &WidePath,
        mode: RootOpenMode,
    ) -> Result<RootProbe<MemDir>, OpenRootError> {
        self.open_root_wide(path.as_wide(), mode)
    }

    fn open_subdir(&self, parent: &MemDir, name: &[u16]) -> Result<MemDir, FsError> {
        check_open_name(name)?;
        self.fire(FsOp::OpenSubdir(self.child_display(parent.node, name)));
        let mut t = self.tree();
        if t.node(parent.node).deleted {
            return Err(e_pending());
        }
        let ch = t.find_child(parent.node, name).ok_or_else(e_gone)?;
        let n = t.node(ch);
        if !n.is_dir_type() && !n.is_reparse() {
            return Err(e_not_dir());
        }
        if n.acl_denied {
            return Err(e_denied());
        }
        if n.delete_pending {
            return Err(e_pending());
        }
        if n.item_open {
            return Err(e_sharing());
        }
        if n.is_reparse() {
            return Err(e_reparse());
        }
        t.node_mut(ch).pin += 1;
        Ok(MemDir {
            tree: Arc::clone(&self.inner),
            node: ch,
        })
    }

    fn read_dir(
        &self,
        dir: &MemDir,
        sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<IdQuality, FsError> {
        self.fire(FsOp::ReadDir(self.item_display(dir.node)));
        struct Row {
            name: Vec<u16>,
            attributes: u32,
            reparse_tag: u32,
            size: u64,
            allocated: u64,
            last_write: FileTime,
            change: FileTime,
            file_id: [u8; 16],
        }
        let (rows, quality) = {
            let t = self.tree();
            let n = t.node(dir.node);
            if n.deleted {
                return Err(e_pending());
            }
            if n.is_reparse() {
                return Err(e_reparse());
            }
            let Data::Dir { children } = &n.data else {
                return Err(e_not_dir());
            };
            let mut rows: Vec<Row> = children
                .iter()
                .map(|&c| {
                    let cn = t.node(c);
                    Row {
                        name: cn.name.clone(),
                        attributes: cn.attributes(),
                        reparse_tag: cn.reparse_tag(),
                        size: cn.size_reported(),
                        allocated: cn.allocated(),
                        last_write: cn.last_write,
                        change: cn.change,
                        file_id: cn.file_id,
                    }
                })
                .collect();
            rows.sort_by_key(|r| folded(&r.name));
            (rows, t.id_quality)
        };
        for r in &rows {
            let file_id = match quality {
                IdQuality::Full128 => Some(r.file_id),
                IdQuality::Id64 => {
                    let mut id = r.file_id;
                    id[8..].fill(0);
                    Some(id)
                }
                IdQuality::None => None,
            };
            let entry = RawEntry {
                name: &r.name,
                attributes: r.attributes,
                reparse_tag: r.reparse_tag,
                size: r.size,
                allocated: r.allocated,
                creation: r.last_write,
                last_write: r.last_write,
                change: r.change,
                file_id,
            };
            if sink(&entry).is_break() {
                break;
            }
        }
        Ok(quality)
    }

    fn open_item(&self, parent: &MemDir, name: &[u16], kind: ItemKind) -> Result<MemItem, FsError> {
        check_open_name(name)?;
        self.fire(FsOp::OpenItem(self.child_display(parent.node, name)));
        let mut t = self.tree();
        if t.node(parent.node).deleted {
            return Err(e_pending());
        }
        let ch = t.find_child(parent.node, name).ok_or_else(e_gone)?;
        let n = t.node(ch);
        match kind {
            ItemKind::File if n.is_dir_type() => return Err(e_is_dir()),
            ItemKind::Dir if !n.is_dir_type() => return Err(e_not_dir()),
            _ => {}
        }
        if n.delete_pending {
            return Err(e_pending());
        }
        if n.acl_denied {
            return Err(e_denied());
        }
        if n.locked || n.item_open || n.pin > 0 {
            return Err(e_sharing());
        }
        t.node_mut(ch).item_open = true;
        Ok(MemItem {
            tree: Arc::clone(&self.inner),
            node: ch,
        })
    }

    fn query_item(&self, item: &MemItem) -> Result<ItemQuery, FsError> {
        self.fire(FsOp::Query(self.item_display(item.node)));
        let t = self.tree();
        let n = t.node(item.node);
        Ok(ItemQuery {
            attributes: n.attributes(),
            reparse_tag: n.reparse_tag(),
            identity: FileIdentity {
                volume_serial: MemTree::serial(n.drive),
                file_id: n.file_id,
            },
            link_count: n.links,
            delete_pending: n.delete_pending || n.deleted,
            is_directory: n.is_dir_type(),
            size: n.size_reported(),
            allocated: n.allocated(),
            creation: n.last_write,
            last_write: n.last_write,
            change: n.change,
        })
    }

    fn final_path(&self, item: &MemItem) -> Result<Vec<u16>, FsError> {
        self.fire(FsOp::FinalPath(self.item_display(item.node)));
        let t = self.tree();
        let n = t.node(item.node);
        if let Some(raw) = &n.final_override {
            return Ok(raw.clone());
        }
        let mut out = wide(r"\\?\");
        out.extend_from_slice(&t.node_path(item.node));
        Ok(out)
    }

    fn set_disposition(
        &self,
        item: &MemItem,
        o: DispositionOpts,
    ) -> Result<DispositionKind, DispositionError> {
        self.fire(FsOp::Dispose(self.item_display(item.node)));
        let mut t = self.tree();
        let legacy = t.no_posix_delete;
        let n = t.node(item.node);
        if n.deleted || n.delete_pending {
            return Err(DispositionError::Fs(e_pending()));
        }
        if let Data::Dir { children } = &n.data {
            if !children.is_empty() {
                return Err(DispositionError::NotEmpty);
            }
        }
        let readonly = n.attrs & attr::READONLY != 0 && !n.is_dir_type();
        if readonly && (legacy || !o.ignore_readonly) {
            return Err(DispositionError::Fs(e_readonly()));
        }
        if n.mapped {
            return Err(DispositionError::Fs(e_mapped()));
        }
        if legacy {
            t.node_mut(item.node).delete_pending = true;
            Ok(DispositionKind::Legacy)
        } else {
            t.detach(item.node);
            Ok(DispositionKind::Posix)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::fs::status::{
        ERROR_ACCESS_DENIED, ERROR_USER_MAPPED_FILE, STATUS_ACCESS_DENIED, STATUS_DELETE_PENDING,
        STATUS_FILE_IS_A_DIRECTORY, STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_NOT_FOUND,
        STATUS_SHARING_VIOLATION,
    };

    const NO_FOLLOW: RootOpenMode = RootOpenMode::VerifyNoFollow;
    const FOLLOW: RootOpenMode = RootOpenMode::CanonicalizeFollow;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }
    fn file(size: u64) -> FileSpec {
        FileSpec {
            size,
            ..FileSpec::default()
        }
    }
    fn root(fs: &MemFs, p: &str) -> MemDir {
        fs.open_root_str(p, NO_FOLLOW).unwrap().dir
    }
    fn names(fs: &MemFs, d: &MemDir) -> Vec<String> {
        let mut v = Vec::new();
        fs.read_dir(d, &mut |e| {
            v.push(String::from_utf16_lossy(e.name));
            ControlFlow::Continue(())
        })
        .unwrap();
        v
    }
    fn item(fs: &MemFs, d: &MemDir, name: &str) -> MemItem {
        fs.open_item(d, &w(name), ItemKind::File).unwrap()
    }
    fn posix() -> DispositionOpts {
        DispositionOpts {
            ignore_readonly: false,
        }
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn memfs_is_send_and_sync_and_handles_are_send() {
        assert_send_sync::<MemFs>();
        assert_send_sync::<MemDir>();
        fn is_send<T: Send>() {}
        is_send::<MemItem>();
    }

    #[test]
    fn name_lookup_is_case_insensitive_and_listing_keeps_stored_casing() {
        let fs = MemFs::new();
        fs.add_file(r"C:\Users\U\Temp\Cache.TMP", file(5));
        let d = root(&fs, r"c:\USERS\u");
        let sub = fs.open_subdir(&d, &w("tEmP")).unwrap();
        assert_eq!(names(&fs, &sub), ["Cache.TMP"]);
        let it = fs.open_item(&sub, &w("cache.tmp"), ItemKind::File).unwrap();
        assert_eq!(fs.query_item(&it).unwrap().size, 5);
        assert!(fs.exists(r"C:\users\u\TEMP\CACHE.tmp"));
        // The fold is ASCII only: a non-ASCII case difference does not match.
        fs.add_file("C:\\Users\\U\\Temp\\\u{e4}.txt", file(1));
        assert_eq!(
            fs.open_item(&sub, &w("\u{c4}.txt"), ItemKind::File)
                .unwrap_err()
                .reason,
            SkipReason::Gone
        );
    }

    #[test]
    fn listing_is_sorted_by_folded_name_and_never_has_dot_entries() {
        let fs = MemFs::new();
        for n in ["b", "A", "c", "B2"] {
            fs.add_file(&format!(r"C:\d\{n}"), file(1));
        }
        let d = root(&fs, r"C:\d");
        let l = names(&fs, &d);
        assert_eq!(l, ["A", "b", "B2", "c"]);
        assert!(!l.iter().any(|n| n == "." || n == ".."));
        let rd = root(&fs, r"C:\");
        assert_eq!(names(&fs, &rd), ["d"]);
    }

    #[test]
    fn read_dir_sink_break_stops_listing() {
        let fs = MemFs::new();
        for n in ["a", "b", "c"] {
            fs.add_file(&format!(r"C:\d\{n}"), file(1));
        }
        let d = root(&fs, r"C:\d");
        let mut seen = 0;
        fs.read_dir(&d, &mut |_| {
            seen += 1;
            ControlFlow::Break(())
        })
        .unwrap();
        assert_eq!(seen, 1);
    }

    #[test]
    fn junction_and_symlink_are_reparse_results_never_followed() {
        let fs = MemFs::new();
        fs.add_file(r"C:\canary\secret.txt", file(7));
        fs.add_dir(r"C:\root");
        fs.add_junction(r"C:\root\evil", r"C:\canary");
        fs.add_symlink_file(r"C:\root\link.txt", r"C:\canary\secret.txt");
        let d = root(&fs, r"C:\root");

        // open_subdir: reparse error, junction and symlink alike.
        for n in ["evil", "link.txt"] {
            let e = fs.open_subdir(&d, &w(n)).unwrap_err();
            assert_eq!(e.reason, SkipReason::ReparsePoint, "{n}");
        }
        // The listing reports them as reparse points with tags, and does not list the target.
        let mut seen = Vec::new();
        fs.read_dir(&d, &mut |e| {
            seen.push((
                String::from_utf16_lossy(e.name),
                e.attributes,
                e.reparse_tag,
            ));
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(
            seen,
            [
                (
                    "evil".to_owned(),
                    attr::DIRECTORY | attr::REPARSE_POINT,
                    TAG_MOUNT_POINT
                ),
                ("link.txt".to_owned(), attr::REPARSE_POINT, TAG_SYMLINK),
            ]
        );
        // open_item succeeds on the link itself (FILE_OPEN_REPARSE_POINT) and reports it.
        let j = fs.open_item(&d, &w("evil"), ItemKind::Dir).unwrap();
        let q = fs.query_item(&j).unwrap();
        assert_eq!(q.reparse_tag, TAG_MOUNT_POINT);
        assert!(q.attributes & attr::REPARSE_POINT != 0 && q.is_directory);
        let l = item(&fs, &d, "link.txt");
        let q = fs.query_item(&l).unwrap();
        assert_eq!((q.reparse_tag, q.size), (TAG_SYMLINK, 0));
        // final_path of the link is the link's own path, not the target's.
        assert_eq!(
            String::from_utf16_lossy(&fs.final_path(&j).unwrap()),
            r"\\?\C:\root\evil"
        );
        // Disposing the junction removes the link only; the target tree is untouched.
        drop(j);
        let j = fs.open_item(&d, &w("evil"), ItemKind::Dir).unwrap();
        assert_eq!(
            fs.set_disposition(&j, posix()).unwrap(),
            DispositionKind::Posix
        );
        assert!(fs.exists(r"C:\canary\secret.txt"));
        assert!(!fs.exists(r"C:\root\evil"));
    }

    #[test]
    fn item_kind_mismatch_reports_changed_since_scan_with_real_status() {
        let fs = MemFs::new();
        fs.add_dir(r"C:\r\sub");
        fs.add_file(r"C:\r\f", file(1));
        let d = root(&fs, r"C:\r");
        let e = fs.open_item(&d, &w("sub"), ItemKind::File).unwrap_err();
        assert_eq!(
            e,
            FsError::new(
                SkipReason::ChangedSinceScan,
                ErrorCode::Nt(STATUS_FILE_IS_A_DIRECTORY)
            )
        );
        let e = fs.open_item(&d, &w("f"), ItemKind::Dir).unwrap_err();
        assert_eq!(e.code, ErrorCode::Nt(STATUS_NOT_A_DIRECTORY));
        let e = fs.open_subdir(&d, &w("f")).unwrap_err();
        assert_eq!(e.code, ErrorCode::Nt(STATUS_NOT_A_DIRECTORY));
        let e = fs.open_subdir(&d, &w("nope")).unwrap_err();
        assert_eq!(
            e,
            FsError::new(
                SkipReason::Gone,
                ErrorCode::Nt(STATUS_OBJECT_NAME_NOT_FOUND)
            )
        );
    }

    #[test]
    fn ancestors_of_the_root_are_followed_but_the_root_itself_is_not() {
        let fs = MemFs::new();
        fs.add_file(r"D:\real\profile\Temp\x.tmp", file(3));
        fs.add_junction(r"C:\Users\u", r"D:\real\profile"); // relocated profile
                                                            // Ancestor junction followed.
        let p = fs.open_root_str(r"C:\Users\u\Temp", NO_FOLLOW).unwrap();
        assert!(p.is_directory && !p.is_reparse);
        assert_eq!(
            String::from_utf16_lossy(&p.final_path),
            r"\\?\D:\real\profile\Temp",
            "final path comes from the real parent chain"
        );
        assert_eq!(names(&fs, &p.dir), ["x.tmp"]);
        // The final component is a junction: not followed in VerifyNoFollow ...
        let p = fs.open_root_str(r"C:\Users\u", NO_FOLLOW).unwrap();
        assert!(p.is_reparse && p.is_directory);
        assert_eq!(String::from_utf16_lossy(&p.final_path), r"\\?\C:\Users\u");
        assert_eq!(
            fs.read_dir(&p.dir, &mut |_| ControlFlow::Continue(()))
                .unwrap_err()
                .reason,
            SkipReason::ReparsePoint
        );
        // ... and followed when canonicalizing bases.
        let p = fs.open_root_str(r"C:\Users\u", FOLLOW).unwrap();
        assert!(!p.is_reparse);
        assert_eq!(
            String::from_utf16_lossy(&p.final_path),
            r"\\?\D:\real\profile"
        );
        assert_eq!(p.identity.volume_serial, MemTree::serial(u16::from(b'D')));
    }

    #[test]
    fn open_root_missing_dangling_loop_and_bad_paths() {
        let fs = MemFs::new();
        fs.add_dir(r"C:\a");
        fs.add_junction(r"C:\dangling", r"C:\nowhere");
        fs.add_junction(r"C:\loop1", r"C:\loop2");
        fs.add_junction(r"C:\loop2", r"C:\loop1");
        assert_eq!(
            fs.open_root_str(r"C:\missing", NO_FOLLOW).unwrap_err(),
            OpenRootError::NotFound
        );
        assert_eq!(
            fs.open_root_str(r"Z:\", NO_FOLLOW).unwrap_err(),
            OpenRootError::NotFound
        );
        assert_eq!(
            fs.open_root_str(r"C:\dangling", FOLLOW).unwrap_err(),
            OpenRootError::NotFound
        );
        assert!(
            fs.open_root_str(r"C:\dangling", NO_FOLLOW)
                .unwrap()
                .is_reparse
        );
        match fs.open_root_str(r"C:\loop1", FOLLOW).unwrap_err() {
            OpenRootError::Fs(e) => assert_eq!(e.code, ErrorCode::Sim("reparse_loop")),
            other => panic!("expected loop error, got {other:?}"),
        }
        for bad in ["", "relative\\x", r"\\server\share", r"C:rel", r"C:\a\..\b"] {
            assert_eq!(
                fs.open_root_str(bad, NO_FOLLOW).unwrap_err(),
                OpenRootError::NotFound,
                "{bad:?}"
            );
        }
        // `\\?\` prefix accepted.
        assert!(fs.open_root_str(r"\\?\C:\a", NO_FOLLOW).is_ok());
    }

    #[test]
    fn open_dir_handles_pin_their_node() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\sub\deep\f.txt", file(1));
        let d = root(&fs, r"C:\r");
        let sub = fs.open_subdir(&d, &w("sub")).unwrap();
        assert!(fs.is_pinned(r"C:\r\sub"));

        let sharing = FsError::new(SkipReason::Locked, ErrorCode::Nt(STATUS_SHARING_VIOLATION));
        assert_eq!(fs.try_remove(r"C:\r\sub").unwrap_err(), sharing);
        assert_eq!(
            fs.try_replace_with_junction(r"C:\r\sub", r"C:\elsewhere")
                .unwrap_err(),
            sharing
        );
        assert_eq!(fs.try_replace_with_dir(r"C:\r\sub").unwrap_err(), sharing);
        assert_eq!(
            fs.try_rename(r"C:\r\sub", r"C:\r\moved").unwrap_err(),
            sharing
        );
        // An ancestor of a pinned node is pinned too (removal would take the pinned dir).
        assert_eq!(fs.try_remove(r"C:\r\sub").unwrap_err(), sharing);
        assert_eq!(fs.try_remove(r"C:\r").unwrap_err(), sharing);
        // Nothing changed.
        assert!(fs.exists(r"C:\r\sub\deep\f.txt"));
        // Siblings of a pinned directory are free.
        fs.add_file(r"C:\r\other.txt", file(1));
        fs.try_remove(r"C:\r\other.txt").unwrap();

        drop(sub);
        assert!(!fs.is_pinned(r"C:\r\sub"));
        fs.try_replace_with_junction(r"C:\r\sub", r"C:\elsewhere")
            .unwrap();
        // After the swap the engine sees a reparse point.
        assert_eq!(
            fs.open_subdir(&d, &w("sub")).unwrap_err().reason,
            SkipReason::ReparsePoint
        );
    }

    #[test]
    fn a_pinned_descendant_blocks_removal_of_the_ancestor() {
        let fs = MemFs::new();
        fs.add_dir(r"C:\r\a\b");
        let d = root(&fs, r"C:\r");
        let a = fs.open_subdir(&d, &w("a")).unwrap();
        let b = fs.open_subdir(&a, &w("b")).unwrap();
        drop(a);
        assert!(fs.try_remove(r"C:\r\a").is_err());
        drop(b);
        fs.try_remove(r"C:\r\a").unwrap();
        assert!(!fs.exists(r"C:\r\a\b"));
    }

    #[test]
    fn rename_keeps_identity_and_rejects_collisions_and_cycles() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\a\f", file(1));
        fs.add_dir(r"C:\r\b");
        let before = fs.snapshot_detailed()[r"C:\r\a"].id;
        fs.try_rename(r"C:\r\a", r"C:\r\b\a2").unwrap();
        assert!(fs.exists(r"C:\r\b\a2\f") && !fs.exists(r"C:\r\a"));
        assert_eq!(fs.snapshot_detailed()[r"C:\r\b\a2"].id, before);
        assert_eq!(
            fs.try_rename(r"C:\r\b", r"C:\r\b\a2\inside")
                .unwrap_err()
                .code,
            ErrorCode::Nt(STATUS_OBJECT_NAME_COLLISION)
        );
        fs.add_dir(r"C:\r\c");
        assert!(fs.try_rename(r"C:\r\c", r"C:\R\B").is_err());
        assert!(fs.try_rename(r"C:\r\c", r"D:\c").is_err());
        assert_eq!(
            fs.try_rename(r"C:\r\zz", r"C:\r\yy").unwrap_err().reason,
            SkipReason::Gone
        );
    }

    #[test]
    fn final_path_is_computed_from_the_real_chain_and_can_be_overridden() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\Sub Dir\File.TXT", file(1));
        let d = root(&fs, r"C:\r");
        let sub = fs.open_subdir(&d, &w("sub dir")).unwrap();
        let it = item(&fs, &sub, "file.txt");
        // On-disk casing, not the requested casing.
        assert_eq!(
            String::from_utf16_lossy(&fs.final_path(&it).unwrap()),
            r"\\?\C:\r\Sub Dir\File.TXT"
        );
        // A rename of an ancestor is reflected (the chain is real, not cached).
        drop((it, sub));
        fs.try_rename(r"C:\r\Sub Dir", r"C:\r\Moved").unwrap();
        let sub = fs.open_subdir(&d, &w("moved")).unwrap();
        let it = item(&fs, &sub, "file.txt");
        assert_eq!(
            String::from_utf16_lossy(&fs.final_path(&it).unwrap()),
            r"\\?\C:\r\Moved\File.TXT"
        );
        drop(it);
        fs.set_final_path_override(r"C:\r\Moved\File.TXT", w(r"\\?\C:\Windows\System32\evil"));
        let it = item(&fs, &sub, "file.txt");
        assert_eq!(
            String::from_utf16_lossy(&fs.final_path(&it).unwrap()),
            r"\\?\C:\Windows\System32\evil"
        );
    }

    #[test]
    fn locked_file_reports_sharing_violation_and_unlocks() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\held.dat", file(1));
        let d = root(&fs, r"C:\r");
        fs.set_locked(r"C:\r\held.dat", true);
        let e = fs
            .open_item(&d, &w("held.dat"), ItemKind::File)
            .unwrap_err();
        assert_eq!(
            e,
            FsError::new(SkipReason::Locked, ErrorCode::Nt(STATUS_SHARING_VIOLATION))
        );
        // Listing still shows it.
        assert_eq!(names(&fs, &d), ["held.dat"]);
        fs.set_locked(r"C:\r\held.dat", false);
        assert!(fs.open_item(&d, &w("held.dat"), ItemKind::File).is_ok());
    }

    #[test]
    fn acl_denied_blocks_every_open_but_not_the_listing() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\secret\f", file(1));
        fs.add_file(r"C:\r\locked.txt", file(1));
        let d = root(&fs, r"C:\r");
        fs.set_acl_denied(r"C:\r\secret");
        fs.set_acl_denied(r"C:\r\locked.txt");
        let denied = FsError::new(
            SkipReason::AccessDenied,
            ErrorCode::Nt(STATUS_ACCESS_DENIED),
        );
        assert_eq!(fs.open_subdir(&d, &w("secret")).unwrap_err(), denied);
        assert_eq!(
            fs.open_item(&d, &w("secret"), ItemKind::Dir).unwrap_err(),
            denied
        );
        assert_eq!(
            fs.open_item(&d, &w("locked.txt"), ItemKind::File)
                .unwrap_err(),
            denied
        );
        assert_eq!(
            fs.open_root_str(r"C:\r\secret", NO_FOLLOW).unwrap_err(),
            OpenRootError::Fs(denied)
        );
        assert_eq!(names(&fs, &d), ["locked.txt", "secret"]);
        fs.clear_acl_denied(r"C:\r\locked.txt");
        assert!(fs.open_item(&d, &w("locked.txt"), ItemKind::File).is_ok());
    }

    #[test]
    fn item_handles_are_exclusive_and_dir_items_conflict_with_open_dir_handles() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\f", file(1));
        fs.add_dir(r"C:\r\sub");
        let d = root(&fs, r"C:\r");
        let first = item(&fs, &d, "f");
        assert_eq!(
            fs.open_item(&d, &w("f"), ItemKind::File)
                .unwrap_err()
                .reason,
            SkipReason::Locked
        );
        drop(first);
        assert!(fs.open_item(&d, &w("f"), ItemKind::File).is_ok());

        // Our own cached directory handle (no SHARE_DELETE) blocks opening it for DELETE.
        let cached = fs.open_subdir(&d, &w("sub")).unwrap();
        assert_eq!(
            fs.open_item(&d, &w("sub"), ItemKind::Dir)
                .unwrap_err()
                .reason,
            SkipReason::Locked
        );
        drop(cached);
        let dir_item = fs.open_item(&d, &w("sub"), ItemKind::Dir).unwrap();
        // ... and a DELETE-access item blocks new directory handles on the same dir.
        assert_eq!(
            fs.open_subdir(&d, &w("sub")).unwrap_err().reason,
            SkipReason::Locked
        );
        drop(dir_item);
        assert!(fs.open_subdir(&d, &w("sub")).is_ok());
    }

    #[test]
    fn posix_delete_removes_immediately_and_the_name_is_reusable() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\f", file(9));
        let d = root(&fs, r"C:\r");
        let it = item(&fs, &d, "f");
        assert_eq!(
            fs.set_disposition(&it, posix()).unwrap(),
            DispositionKind::Posix
        );
        assert!(!fs.exists(r"C:\r\f"));
        assert!(names(&fs, &d).is_empty());
        // The open handle still answers, as delete-pending, with its last path.
        let q = fs.query_item(&it).unwrap();
        assert!(q.delete_pending);
        assert_eq!(
            String::from_utf16_lossy(&fs.final_path(&it).unwrap()),
            r"\\?\C:\r\f"
        );
        // A second dispose is a delete-pending error.
        match fs.set_disposition(&it, posix()).unwrap_err() {
            DispositionError::Fs(e) => assert_eq!(e.code, ErrorCode::Nt(STATUS_DELETE_PENDING)),
            DispositionError::NotEmpty => panic!("wrong error"),
        }
        // POSIX semantics: the name can be recreated while the old handle is still open.
        fs.add_file(r"C:\r\f", file(1));
        assert!(fs.exists(r"C:\r\f"));
        drop(it);
        assert!(
            fs.exists(r"C:\r\f"),
            "closing the old handle must not touch the new file"
        );
    }

    #[test]
    fn legacy_delete_takes_effect_on_close_and_blocks_new_opens_meanwhile() {
        let fs = MemFs::new();
        fs.set_no_posix_delete(true);
        fs.add_file(r"C:\r\f", file(9));
        let d = root(&fs, r"C:\r");
        let it = item(&fs, &d, "f");
        assert_eq!(
            fs.set_disposition(&it, posix()).unwrap(),
            DispositionKind::Legacy
        );
        assert!(
            fs.exists(r"C:\r\f"),
            "legacy delete waits for the handle close"
        );
        assert!(fs.query_item(&it).unwrap().delete_pending);
        assert_eq!(
            fs.open_item(&d, &w("f"), ItemKind::File)
                .unwrap_err()
                .reason,
            SkipReason::DeletePending,
            "a delete-pending file cannot be opened again"
        );
        drop(it);
        assert!(!fs.exists(r"C:\r\f"));
    }

    #[test]
    fn read_only_files_posix_needs_ignore_flag_legacy_never_works() {
        let fs = MemFs::new();
        let ro = FileSpec {
            attrs: attr::READONLY,
            ..file(1)
        };
        fs.add_file(r"C:\r\ro.txt", ro);
        let d = root(&fs, r"C:\r");
        let it = item(&fs, &d, "ro.txt");
        let e = fs.set_disposition(&it, posix()).unwrap_err();
        assert_eq!(
            e,
            DispositionError::Fs(FsError::new(
                SkipReason::ReadOnly,
                ErrorCode::Win32(ERROR_ACCESS_DENIED)
            ))
        );
        assert!(fs.exists(r"C:\r\ro.txt"));
        // Attributes are never cleared by a failed (or successful) attempt.
        assert_eq!(
            fs.query_item(&it).unwrap().attributes & attr::READONLY,
            attr::READONLY
        );
        assert_eq!(
            fs.set_disposition(
                &it,
                DispositionOpts {
                    ignore_readonly: true
                }
            )
            .unwrap(),
            DispositionKind::Posix
        );
        assert!(!fs.exists(r"C:\r\ro.txt"));
        drop(it);

        // Legacy fallback cannot ignore read-only, even when asked.
        fs.set_no_posix_delete(true);
        fs.add_file(r"C:\r\ro2.txt", ro);
        let it = item(&fs, &d, "ro2.txt");
        let e = fs
            .set_disposition(
                &it,
                DispositionOpts {
                    ignore_readonly: true,
                },
            )
            .unwrap_err();
        assert!(matches!(e, DispositionError::Fs(f) if f.reason == SkipReason::ReadOnly));
        drop(it);
        assert!(fs.exists(r"C:\r\ro2.txt"));
    }

    #[test]
    fn mapped_image_is_locked_even_with_posix_delete() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\app.dll", file(1));
        let d = root(&fs, r"C:\r");
        fs.set_image_mapped(r"C:\r\app.dll", true);
        let it = item(&fs, &d, "app.dll");
        assert_eq!(
            fs.set_disposition(&it, posix()).unwrap_err(),
            DispositionError::Fs(FsError::new(
                SkipReason::Locked,
                ErrorCode::Win32(ERROR_USER_MAPPED_FILE)
            ))
        );
        assert!(fs.exists(r"C:\r\app.dll"));
    }

    #[test]
    fn directory_delete_is_not_empty_until_empty() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\sub\f", file(1));
        let d = root(&fs, r"C:\r");
        let it = fs.open_item(&d, &w("sub"), ItemKind::Dir).unwrap();
        assert_eq!(
            fs.set_disposition(&it, posix()).unwrap_err(),
            DispositionError::NotEmpty
        );
        assert!(fs.exists(r"C:\r\sub\f"));
        drop(it);
        fs.try_remove(r"C:\r\sub\f").unwrap();
        let it = fs.open_item(&d, &w("sub"), ItemKind::Dir).unwrap();
        assert_eq!(
            fs.set_disposition(&it, posix()).unwrap(),
            DispositionKind::Posix
        );
        assert!(!fs.exists(r"C:\r\sub"));
        // Read-only directories are deletable (the bit is ignored for directories).
        fs.add_dir(r"C:\r\rod");
        fs.set_attrs(r"C:\r\rod", attr::READONLY);
        let it = fs.open_item(&d, &w("rod"), ItemKind::Dir).unwrap();
        assert!(fs.set_disposition(&it, posix()).is_ok());
    }

    #[test]
    fn legacy_directory_delete_checks_emptiness_too() {
        let fs = MemFs::new();
        fs.set_no_posix_delete(true);
        fs.add_file(r"C:\r\sub\f", file(1));
        fs.add_dir(r"C:\r\empty");
        let d = root(&fs, r"C:\r");
        let it = fs.open_item(&d, &w("sub"), ItemKind::Dir).unwrap();
        assert_eq!(
            fs.set_disposition(&it, posix()).unwrap_err(),
            DispositionError::NotEmpty
        );
        let e = fs.open_item(&d, &w("empty"), ItemKind::Dir).unwrap();
        assert_eq!(
            fs.set_disposition(&e, posix()).unwrap(),
            DispositionKind::Legacy
        );
        drop(e);
        assert!(!fs.exists(r"C:\r\empty") && fs.exists(r"C:\r\sub\f"));
    }

    #[test]
    fn listing_id_quality_downgrades() {
        let fs = MemFs::new();
        let wide_id = {
            let mut id = [0u8; 16];
            id[0] = 1;
            id[12] = 0xEE;
            id
        };
        fs.add_file(
            r"C:\r\f",
            FileSpec {
                id: Some(wide_id),
                ..file(1)
            },
        );
        let d = root(&fs, r"C:\r");
        let ids = |fs: &MemFs| {
            let mut got = Vec::new();
            let q = fs
                .read_dir(&d, &mut |e| {
                    got.push(e.file_id);
                    ControlFlow::Continue(())
                })
                .unwrap();
            (q, got)
        };
        assert_eq!(ids(&fs), (IdQuality::Full128, vec![Some(wide_id)]));
        fs.set_no_extd_ids(IdQuality::Id64);
        let mut low = wide_id;
        low[8..].fill(0);
        assert_eq!(ids(&fs), (IdQuality::Id64, vec![Some(low)]));
        fs.set_no_extd_ids(IdQuality::None);
        assert_eq!(ids(&fs), (IdQuality::None, vec![None]));
        // The handle-based query still has the full identity.
        assert_eq!(
            fs.query_item(&item(&fs, &d, "f")).unwrap().identity.file_id,
            wide_id
        );
    }

    #[test]
    fn auto_ids_are_unique_and_stable_per_node() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\a", file(1));
        fs.add_file(r"C:\r\b", file(1));
        let d = root(&fs, r"C:\r");
        let ia = fs.query_item(&item(&fs, &d, "a")).unwrap().identity;
        let ib = fs.query_item(&item(&fs, &d, "b")).unwrap().identity;
        assert_ne!(ia, ib);
        assert_eq!(ia, fs.query_item(&item(&fs, &d, "a")).unwrap().identity);
        // Replacing a file (delete + create) gives a new identity; modify keeps it.
        fs.modify_file(r"C:\r\a", 50, FileTime(77));
        let q = fs.query_item(&item(&fs, &d, "a")).unwrap();
        assert_eq!(
            (q.identity, q.size, q.last_write, q.change),
            (ia, 50, FileTime(77), FileTime(77))
        );
        fs.add_file(r"C:\r\a", file(1));
        assert_ne!(ia, fs.query_item(&item(&fs, &d, "a")).unwrap().identity);
    }

    #[test]
    fn volume_kinds_use_the_longest_matching_prefix() {
        let fs = MemFs::new();
        fs.add_dir(r"C:\a\b");
        fs.add_dir(r"D:\x");
        fs.add_dir(r"T:\x");
        let kind = |p: &str| fs.open_root_str(p, NO_FOLLOW).unwrap().volume;
        assert_eq!(kind(r"C:\a\b"), VolumeKind::LocalFixed);
        fs.set_volume(r"T:\", VolumeKind::Remote);
        fs.set_volume(r"D:", VolumeKind::Removable);
        fs.set_volume(r"C:\a", VolumeKind::Other);
        assert_eq!(kind(r"T:\x"), VolumeKind::Remote);
        assert_eq!(kind(r"d:\X"), VolumeKind::Removable);
        assert_eq!(kind(r"C:\a\b"), VolumeKind::Other);
        assert_eq!(kind(r"C:\"), VolumeKind::LocalFixed);
        fs.set_volume(r"C:\a\b", VolumeKind::Remote);
        fs.set_volume(r"C:\A", VolumeKind::LocalFixed); // replaces the C:\a entry
        assert_eq!(kind(r"C:\a\b"), VolumeKind::Remote);
        assert_eq!(kind(r"C:\a"), VolumeKind::LocalFixed);
        // A volume mounted through a junction reports the kind of the real location.
        fs.add_junction(r"C:\mnt", r"T:\x");
        assert_eq!(
            fs.open_root_str(r"C:\mnt", FOLLOW).unwrap().volume,
            VolumeKind::Remote
        );
    }

    #[test]
    fn query_reports_attributes_times_links_and_sizes() {
        let fs = MemFs::new();
        fs.add_file(
            r"C:\r\f.bin",
            FileSpec {
                size: 5000,
                attrs: attr::HIDDEN | attr::SYSTEM,
                last_write: FileTime(100),
                change: FileTime(200),
                links: 3,
                id: None,
            },
        );
        fs.add_dir(r"C:\r\d");
        fs.set_times(r"C:\r\d", FileTime(5), FileTime(6));
        let d = root(&fs, r"C:\r");
        let q = fs.query_item(&item(&fs, &d, "f.bin")).unwrap();
        assert_eq!(q.attributes, attr::HIDDEN | attr::SYSTEM);
        assert_eq!((q.size, q.allocated, q.link_count), (5000, 8192, 3));
        assert_eq!((q.last_write, q.change), (FileTime(100), FileTime(200)));
        assert!(!q.is_directory && !q.delete_pending && q.reparse_tag == 0);
        let q = fs
            .query_item(&fs.open_item(&d, &w("d"), ItemKind::Dir).unwrap())
            .unwrap();
        assert!(q.is_directory && q.attributes & attr::DIRECTORY != 0);
        assert_eq!(
            (q.size, q.last_write, q.change),
            (0, FileTime(5), FileTime(6))
        );
        // The listing agrees with the handle query.
        let mut got = Vec::new();
        fs.read_dir(&d, &mut |e| {
            got.push((
                String::from_utf16_lossy(e.name),
                e.attributes,
                e.size,
                e.allocated,
                e.last_write,
                e.change,
            ));
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(
            got,
            [
                (
                    "d".to_owned(),
                    attr::DIRECTORY,
                    0,
                    0,
                    FileTime(5),
                    FileTime(6)
                ),
                (
                    "f.bin".to_owned(),
                    attr::HIDDEN | attr::SYSTEM,
                    5000,
                    8192,
                    FileTime(100),
                    FileTime(200)
                ),
            ]
        );
    }

    #[test]
    fn cloud_and_reparse_attribute_bits_pass_through_for_plain_files() {
        let fs = MemFs::new();
        fs.add_file(
            r"C:\r\cloud.txt",
            FileSpec {
                attrs: attr::RECALL_ON_DATA_ACCESS,
                ..file(1)
            },
        );
        fs.add_file(
            r"C:\r\odd.bin",
            FileSpec {
                attrs: attr::REPARSE_POINT,
                ..file(1)
            },
        );
        let d = root(&fs, r"C:\r");
        let q = fs.query_item(&item(&fs, &d, "cloud.txt")).unwrap();
        assert!(attr::is_cloud(q.attributes));
        let q = fs.query_item(&item(&fs, &d, "odd.bin")).unwrap();
        assert!(attr::is_reparse(q.attributes, q.reparse_tag) && q.reparse_tag == 0);
    }

    #[test]
    fn bad_names_are_refused_before_the_hook_runs() {
        let fs = MemFs::new();
        fs.add_dir(r"C:\r\sub");
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        fs.set_hook(Box::new(move |_, _| {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        let d = root(&fs, r"C:\r");
        assert_eq!(calls.load(Ordering::SeqCst), 1); // OpenRoot only
        for bad in ["", ".", "..", "sub\\x", "sub/x", "a:b", "x\0y"] {
            let e = fs.open_subdir(&d, &w(bad)).unwrap_err();
            assert_eq!(
                e,
                FsError::new(SkipReason::FsError, ErrorCode::Sim("bad_name"))
            );
            assert!(fs.open_item(&d, &w(bad), ItemKind::File).is_err());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn hook_runs_before_each_op_with_paths_and_sees_the_pre_op_state() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\sub\f.txt", file(1));
        let log: Arc<Mutex<Vec<(FsOp, bool)>>> = Arc::default();
        let l = Arc::clone(&log);
        fs.set_hook(Box::new(move |op, fs| {
            let present = match op {
                FsOp::OpenItem(p) | FsOp::Dispose(p) | FsOp::Query(p) | FsOp::FinalPath(p) => {
                    fs.exists(p)
                }
                _ => true,
            };
            l.lock().unwrap().push((op.clone(), present));
        }));
        let d = root(&fs, r"c:\R");
        let sub = fs.open_subdir(&d, &w("SUB")).unwrap();
        fs.read_dir(&sub, &mut |_| ControlFlow::Continue(()))
            .unwrap();
        let it = fs.open_item(&sub, &w("f.txt"), ItemKind::File).unwrap();
        fs.query_item(&it).unwrap();
        fs.final_path(&it).unwrap();
        fs.set_disposition(&it, posix()).unwrap();
        // A missing name still fires, with exists == false.
        fs.open_item(&sub, &w("ghost"), ItemKind::File).unwrap_err();
        let got = log.lock().unwrap().clone();
        let s = |x: &str| x.to_owned();
        assert_eq!(
            got,
            [
                (FsOp::OpenRoot(s(r"C:\R")), true),
                (FsOp::OpenSubdir(s(r"C:\r\SUB")), true),
                (FsOp::ReadDir(s(r"C:\r\sub")), true),
                (FsOp::OpenItem(s(r"C:\r\sub\f.txt")), true),
                (FsOp::Query(s(r"C:\r\sub\f.txt")), true),
                (FsOp::FinalPath(s(r"C:\r\sub\f.txt")), true),
                (FsOp::Dispose(s(r"C:\r\sub\f.txt")), true),
                (FsOp::OpenItem(s(r"C:\r\sub\ghost")), false),
            ]
        );
    }

    #[test]
    fn hook_can_swap_a_scanned_directory_for_a_junction_between_ops() {
        // Scan lists `victim` as a plain directory; before the delete pass opens it, an
        // attacker replaces it with a junction to the canary.
        let fs = MemFs::new();
        fs.add_file(r"C:\canary\keep.txt", file(10));
        fs.add_file(r"C:\r\victim\old.tmp", file(1));
        let d = root(&fs, r"C:\r");
        let mut seen_dir = false;
        fs.read_dir(&d, &mut |e| {
            seen_dir = e.attributes & attr::DIRECTORY != 0;
            ControlFlow::Continue(())
        })
        .unwrap();
        assert!(seen_dir);
        fs.set_hook(Box::new(|op, fs| {
            if matches!(op, FsOp::OpenSubdir(p) if p.eq_ignore_ascii_case(r"C:\r\victim")) {
                fs.try_replace_with_junction(r"C:\r\victim", r"C:\canary")
                    .unwrap();
            }
        }));
        let e = fs.open_subdir(&d, &w("victim")).unwrap_err();
        assert_eq!(e.reason, SkipReason::ReparsePoint);
        assert!(fs.exists(r"C:\canary\keep.txt"));
    }

    #[test]
    fn hook_can_lock_a_file_mid_run_and_swap_a_file_for_another() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\a.tmp", file(1));
        fs.add_file(r"C:\r\b.tmp", file(1));
        let d = root(&fs, r"C:\r");
        let a_id = fs.query_item(&item(&fs, &d, "a.tmp")).unwrap().identity;
        fs.set_hook(Box::new(|op, fs| match op {
            FsOp::OpenItem(p) if p.ends_with("a.tmp") => fs.set_locked(r"C:\r\a.tmp", true),
            FsOp::OpenItem(p) if p.ends_with("b.tmp") => {
                fs.try_replace_with_file(r"C:\r\b.tmp", file(999)).unwrap();
            }
            _ => {}
        }));
        assert_eq!(
            fs.open_item(&d, &w("a.tmp"), ItemKind::File)
                .unwrap_err()
                .reason,
            SkipReason::Locked
        );
        // The swap lands before the open: the opened file is the new one (size 999, new id).
        let b = fs.open_item(&d, &w("b.tmp"), ItemKind::File).unwrap();
        let q = fs.query_item(&b).unwrap();
        assert_eq!(q.size, 999);
        assert_ne!(q.identity, a_id);
    }

    #[test]
    fn hook_mutation_cannot_deadlock_and_clear_hook_works() {
        let fs = MemFs::new();
        fs.add_dir(r"C:\r");
        let n = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&n);
        fs.set_hook(Box::new(move |_, fs| {
            c.fetch_add(1, Ordering::SeqCst);
            fs.add_file(r"C:\r\spawned.txt", FileSpec::default());
            let _ = fs.snapshot();
        }));
        let d = root(&fs, r"C:\r");
        assert_eq!(names(&fs, &d), ["spawned.txt"]);
        fs.clear_hook();
        let before = n.load(Ordering::SeqCst);
        names(&fs, &d);
        assert_eq!(n.load(Ordering::SeqCst), before);
    }

    #[test]
    fn snapshot_and_exists_support_before_after_manifests() {
        let fs = MemFs::new();
        fs.add_file(r"C:\sentinel\a.txt", file(3));
        fs.add_file(r"C:\sentinel\sub\b.txt", file(4));
        fs.add_file(r"C:\r\x.tmp", file(5));
        fs.add_junction(r"C:\r\evil", r"C:\sentinel");
        fs.add_symlink_file(r"C:\r\l", r"C:\sentinel\a.txt");
        let snap = fs.snapshot();
        assert_eq!(snap[r"C:\"], (ItemKind::Dir, 0));
        assert_eq!(snap[r"C:\sentinel\a.txt"], (ItemKind::File, 3));
        assert_eq!(snap[r"C:\r\evil"], (ItemKind::Dir, 0));
        assert_eq!(snap[r"C:\r\l"], (ItemKind::File, 0));
        assert!(
            !snap.keys().any(|k| k.starts_with(r"C:\r\evil\")),
            "junctions are never descended into"
        );
        assert!(fs.exists(r"C:\r\evil"), "a link exists as itself ...");
        assert!(
            fs.exists(r"C:\r\evil\sub\b.txt"),
            "... and ancestors are followed"
        );
        assert!(!fs.exists(r"C:\r\evil\nope"));
        assert!(!fs.exists("not a path"));

        let before = fs.snapshot_detailed();
        let sentinel = |m: &BTreeMap<String, NodeSummary>| {
            m.iter()
                .filter(|(k, _)| k.starts_with(r"C:\sentinel"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        };
        fs.try_remove(r"C:\r\x.tmp").unwrap();
        let after = fs.snapshot_detailed();
        assert_eq!(sentinel(&before), sentinel(&after));
        assert!(before.contains_key(r"C:\r\x.tmp") && !after.contains_key(r"C:\r\x.tmp"));
        assert_eq!(
            after[r"C:\r\evil"].node_type,
            NodeType::Junction {
                target: r"C:\sentinel".to_owned()
            }
        );
        // modify_file shows up in the detailed manifest (size and time), not only the paths.
        fs.modify_file(r"C:\sentinel\a.txt", 3, FileTime(1));
        assert_ne!(sentinel(&after), sentinel(&fs.snapshot_detailed()));
    }

    #[test]
    fn builder_follows_intermediate_junctions_like_windows() {
        let fs = MemFs::new();
        fs.add_dir(r"D:\real");
        fs.add_junction(r"C:\link", r"D:\real");
        fs.add_file(r"C:\link\new.txt", file(2));
        assert!(fs.exists(r"D:\real\new.txt"));
        // The snapshot sees the file once, under its real path.
        let snap = fs.snapshot();
        assert!(snap.contains_key(r"D:\real\new.txt"));
        assert!(!snap.contains_key(r"C:\link\new.txt"));
    }

    #[test]
    fn unicode_emoji_long_and_lone_surrogate_names_round_trip() {
        let fs = MemFs::new();
        let long_dir = "d".repeat(200);
        let deep = format!(r"C:\r\{long_dir}\{long_dir}\leaf.tmp");
        assert!(deep.len() > 260);
        fs.add_file(&deep, file(1));
        fs.add_file("C:\\r\\\u{1F4A9}.tmp", file(2));
        fs.add_file("C:\\r\\na\u{ef}ve \u{65e5}\u{672c}.txt", file(3));
        fs.add_file("C:\\r\\CON", file(4));
        let mut lone = w(r"C:\r\bad");
        lone.push(0xD800);
        fs.add_file_wide(&lone, file(5));
        let d = root(&fs, r"C:\r");

        let mut listed: Vec<Vec<u16>> = Vec::new();
        fs.read_dir(&d, &mut |e| {
            listed.push(e.name.to_vec());
            ControlFlow::Continue(())
        })
        .unwrap();
        assert!(listed.contains(&w("\u{1F4A9}.tmp")));
        assert!(listed.contains(&w("na\u{ef}ve \u{65e5}\u{672c}.txt")));
        assert!(
            listed.contains(&lone[5..].to_vec()),
            "lone surrogate listed verbatim"
        );
        // The long path is reachable with relative opens only.
        let a = fs.open_subdir(&d, &w(&long_dir)).unwrap();
        let b = fs.open_subdir(&a, &w(&long_dir)).unwrap();
        let it = item(&fs, &b, "leaf.tmp");
        assert_eq!(
            String::from_utf16_lossy(&fs.final_path(&it).unwrap()),
            format!(r"\\?\{deep}")
        );
        // Open by the exact lone-surrogate name works.
        let name = lone[5..].to_vec();
        let it = fs.open_item(&d, &name, ItemKind::File).unwrap();
        assert_eq!(fs.query_item(&it).unwrap().size, 5);
        // Reserved device names are ordinary names here (policy lives above the seam).
        assert!(fs.open_item(&d, &w("con"), ItemKind::File).is_ok());
    }

    #[test]
    fn hard_link_count_is_reported() {
        let fs = MemFs::new();
        fs.add_file(
            r"C:\r\f",
            FileSpec {
                links: 2,
                ..file(1)
            },
        );
        let d = root(&fs, r"C:\r");
        let it = item(&fs, &d, "f");
        assert_eq!(fs.query_item(&it).unwrap().link_count, 2);
    }

    #[test]
    fn an_open_item_handle_pins_its_node_against_the_test_api() {
        // The real file system also refuses to rename or delete a file we hold open with
        // DELETE access and no SHARE_DELETE.
        let fs = MemFs::new();
        fs.add_file(r"C:\r\f", file(1));
        let d = root(&fs, r"C:\r");
        let it = item(&fs, &d, "f");
        assert!(fs.is_pinned(r"C:\r\f"));
        assert_eq!(
            fs.try_remove(r"C:\r\f").unwrap_err().reason,
            SkipReason::Locked
        );
        assert!(fs.try_replace_with_file(r"C:\r\f", file(2)).is_err());
        drop(it);
        assert!(!fs.is_pinned(r"C:\r\f"));
        fs.try_remove(r"C:\r\f").unwrap();
    }

    #[test]
    fn read_dir_of_a_file_or_stale_handle_errors() {
        let fs = MemFs::new();
        fs.add_file(r"C:\r\f", file(1));
        let p = fs.open_root_str(r"C:\r\f", NO_FOLLOW).unwrap();
        assert!(!p.is_directory);
        assert_eq!(
            fs.read_dir(&p.dir, &mut |_| ControlFlow::Continue(()))
                .unwrap_err()
                .code,
            ErrorCode::Nt(STATUS_NOT_A_DIRECTORY)
        );
    }

    #[test]
    fn clones_share_state() {
        let fs = MemFs::new();
        let other = fs.clone();
        other.add_file(r"C:\x", file(1));
        assert!(fs.exists(r"C:\x"));
    }

    #[test]
    fn concurrent_opens_and_listings_are_safe() {
        let fs = MemFs::new();
        for i in 0..50 {
            fs.add_file(&format!(r"C:\r\f{i}.tmp"), file(i));
        }
        std::thread::scope(|sc| {
            for _ in 0..4 {
                sc.spawn(|| {
                    let d = root(&fs, r"C:\r");
                    for _ in 0..20 {
                        assert_eq!(names(&fs, &d).len(), 50);
                    }
                });
            }
        });
        assert_eq!(fs.snapshot().len(), 52);
    }

    #[test]
    fn builder_rejects_non_absolute_paths() {
        let r = std::panic::catch_unwind(|| MemFs::new().add_dir(r"relative\x"));
        assert!(r.is_err());
        let r = std::panic::catch_unwind(|| MemFs::new().add_dir(r"C:\a\..\b"));
        assert!(r.is_err());
    }
}
