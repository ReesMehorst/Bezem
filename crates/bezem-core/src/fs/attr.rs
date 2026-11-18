// SPDX-License-Identifier: MIT

//! `FILE_ATTRIBUTE_*` constants and helpers (values from the Windows SDK `winnt.h`).
//!
//! Ownership: Phase 0 (stable contract).

pub const READONLY: u32 = 0x1;
pub const HIDDEN: u32 = 0x2;
pub const SYSTEM: u32 = 0x4;
pub const DIRECTORY: u32 = 0x10;
pub const REPARSE_POINT: u32 = 0x400;
pub const OFFLINE: u32 = 0x1000;
pub const RECALL_ON_OPEN: u32 = 0x4_0000;
pub const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// Cloud placeholder: `OFFLINE`, `RECALL_ON_OPEN` or `RECALL_ON_DATA_ACCESS`. Such items are
/// never hydrated or deleted.
pub fn is_cloud(attrs: u32) -> bool {
    attrs & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// Reparse point by attribute bit OR a non-zero reparse tag (either one is enough).
pub fn is_reparse(attrs: u32, reparse_tag: u32) -> bool {
    attrs & REPARSE_POINT != 0 || reparse_tag != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert!(is_cloud(RECALL_ON_DATA_ACCESS));
        assert!(is_cloud(OFFLINE | READONLY));
        assert!(!is_cloud(READONLY | HIDDEN | SYSTEM));
        assert!(is_reparse(REPARSE_POINT, 0));
        assert!(is_reparse(0, 0xA000_0003));
        assert!(!is_reparse(DIRECTORY, 0));
    }
}
