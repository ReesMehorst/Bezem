// SPDX-License-Identifier: MIT

//! The per-item verdict and the delete-time plan comparison (M1 design section 2.8).
//!
//! Ownership: work package **B**.
//!
//! Verdict order (first hit wins, deterministic): ReparsePoint (attribute bit OR tag != 0) >
//! CloudPlaceholder > Excluded (`matcher.excludes(rel)`) > SystemFile (SYSTEM &&
//! !include_system) > ReadOnly (READONLY && !delete_readonly) > HardLinked (files,
//! link_count > 1) > DeletePending > TooNew. TooNew: `newest = max(last_write, change)`;
//! `newest > now` => TooNew; `now - newest < effective_min_age` => TooNew; exactly equal to
//! the threshold => Eligible. Include globs are NOT part of the verdict.
//! `check_against_plan` order: identity (only if `IdQuality != None`), then size, then
//! last_write; each => ChangedSinceScan.

use std::time::SystemTime;

use super::SafetyPolicy;
use crate::fs::{attr, IdQuality, ItemKind, ItemQuery, RawEntry};
use crate::target::Target;
use crate::types::{FileTime, SkipReason, Verdict};
use crate::walk::PlanEntry;

/// The facts the verdict looks at, from a listing ([`ItemFacts::from_raw`]) or from an open
/// handle ([`ItemFacts::from_query`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemFacts<'a> {
    pub kind: ItemKind,
    /// `/`-joined path relative to the root, long names.
    pub rel_path: &'a [u16],
    pub attributes: u32,
    pub reparse_tag: u32,
    pub size: u64,
    pub last_write: FileTime,
    pub change_time: FileTime,
    /// `None` during the scan (listings carry no link count).
    pub link_count: Option<u32>,
    /// `None` during the scan.
    pub delete_pending: Option<bool>,
}

impl<'a> ItemFacts<'a> {
    /// Facts from a directory listing entry.
    pub fn from_raw(e: &RawEntry<'_>, rel: &'a [u16], kind: ItemKind) -> Self {
        ItemFacts {
            kind,
            rel_path: rel,
            attributes: e.attributes,
            reparse_tag: e.reparse_tag,
            size: e.size,
            last_write: e.last_write,
            change_time: e.change,
            link_count: None,
            delete_pending: None,
        }
    }

    /// Facts read from an open item handle.
    pub fn from_query(q: &ItemQuery, rel: &'a [u16]) -> Self {
        ItemFacts {
            kind: if q.is_directory {
                ItemKind::Dir
            } else {
                ItemKind::File
            },
            rel_path: rel,
            attributes: q.attributes,
            reparse_tag: q.reparse_tag,
            size: q.size,
            last_write: q.last_write,
            change_time: q.change,
            link_count: Some(q.link_count),
            delete_pending: Some(q.delete_pending),
        }
    }
}

impl SafetyPolicy {
    /// The verdict for one item (order in the module docs).
    ///
    /// Fail-closed extras: a file link count other than 1 (`0` included) is `HardLinked`; an
    /// item whose newest timestamp is 0 (unknown) is `TooNew`.
    pub fn verdict(&self, facts: &ItemFacts<'_>, target: &Target, now: SystemTime) -> Verdict {
        use SkipReason as R;
        let skip = if attr::is_reparse(facts.attributes, facts.reparse_tag) {
            Some(R::ReparsePoint)
        } else if attr::is_cloud(facts.attributes) {
            Some(R::CloudPlaceholder)
        } else if target.matcher.excludes(facts.rel_path) {
            Some(R::Excluded)
        } else if facts.attributes & attr::SYSTEM != 0 && !target.include_system {
            Some(R::SystemFile)
        } else if facts.attributes & attr::READONLY != 0 && !target.delete_readonly {
            Some(R::ReadOnly)
        } else if facts.kind == ItemKind::File && facts.link_count.is_some_and(|n| n != 1) {
            Some(R::HardLinked)
        } else if facts.delete_pending == Some(true) {
            Some(R::DeletePending)
        } else if self.too_new(facts, target, now) {
            Some(R::TooNew)
        } else {
            None
        };
        match skip {
            Some(r) => Verdict::Skip(r),
            None => Verdict::Eligible,
        }
    }

    /// `newest = max(last_write, change)`; future (clock skew) or unknown (0) => too new;
    /// `now - newest < effective_min_age` => too new; exactly equal => old enough.
    fn too_new(&self, facts: &ItemFacts<'_>, target: &Target, now: SystemTime) -> bool {
        let newest = facts.last_write.max(facts.change_time);
        let now = FileTime::from_system_time(now);
        if newest == FileTime(0) || newest > now {
            return true;
        }
        let age_ticks = now.0 - newest.0;
        age_ticks < FileTime::duration_to_ticks(self.effective_min_age(target))
    }

    /// Compares delete-time facts with the plan; `Some(ChangedSinceScan)` on any difference.
    /// A directory where the plan has a file is a change too.
    pub fn check_against_plan(
        &self,
        q: &ItemQuery,
        e: &PlanEntry,
        id_quality: IdQuality,
    ) -> Option<SkipReason> {
        let changed = (id_quality != IdQuality::None && q.identity != e.identity)
            || q.size != e.size
            || q.last_write != e.last_write
            || q.is_directory;
        changed.then_some(SkipReason::ChangedSinceScan)
    }
}
