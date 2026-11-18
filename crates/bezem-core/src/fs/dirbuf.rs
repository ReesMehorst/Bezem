// SPDX-License-Identifier: MIT

//! Pure parsers for `GetFileInformationByHandleEx` directory buffers
//! (`FILE_ID_EXTD_DIR_INFO`, `FILE_ID_BOTH_DIR_INFO`, `FILE_FULL_DIR_INFO`), M1 design
//! section 3.3. Safe code with bounds checks; bad offsets are an error, never a panic.
//! `.` and `..` are filtered here.
//!
//! Ownership: work package **A**.
//!
//! # Layout
//!
//! The byte offsets in [`layout`] are the natural `#[repr(C)]` offsets of the SDK structs. They
//! are checked at compile time against `core::mem::offset_of!` on the `windows` crate structs
//! (see `win32/layout_check.rs`, built for every Windows target, including the Linux
//! `cargo check --target x86_64-pc-windows-msvc` gate). Note that `FILE_FULL_DIR_INFO.FileName`
//! is at offset **68** (directly after `EaSize`), not 72 as the first draft of the design said.
//!
//! # Robustness
//!
//! Every read is bounds-checked; every entry must fit the buffer entirely (header and name);
//! `NextEntryOffset` must be zero (last entry) or point past the end of the current entry's
//! name, so the walk strictly advances and cannot loop or overlap; name lengths must be even
//! and non-zero. Any violation stops the walk with a [`DirBufError`] (the directory is then
//! reported as failed, never partially trusted beyond the entries already delivered).

use std::ops::ControlFlow;

use super::attr;
use super::RawEntry;
use crate::types::FileTime;

/// A malformed directory buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DirBufError {
    #[error("entry at offset {offset} overruns the buffer")]
    Truncated { offset: usize },
    #[error("bad file name length at offset {offset}")]
    BadNameLength { offset: usize },
    #[error("bad next-entry offset at offset {offset}")]
    BadNextOffset { offset: usize },
}

/// Byte offsets inside one entry (relative to the entry start), per the SDK `#[repr(C)]`
/// layouts; verified against the `windows` crate structs at compile time on Windows targets.
pub mod layout {
    // Common prefix of all three structs.
    pub const NEXT_ENTRY_OFFSET: usize = 0;
    pub const CREATION_TIME: usize = 8;
    pub const LAST_ACCESS_TIME: usize = 16;
    pub const LAST_WRITE_TIME: usize = 24;
    pub const CHANGE_TIME: usize = 32;
    pub const END_OF_FILE: usize = 40;
    pub const ALLOCATION_SIZE: usize = 48;
    pub const FILE_ATTRIBUTES: usize = 56;
    pub const FILE_NAME_LENGTH: usize = 60;
    pub const EA_SIZE: usize = 64;

    // FILE_ID_EXTD_DIR_INFO.
    pub const EXTD_REPARSE_POINT_TAG: usize = 68;
    pub const EXTD_FILE_ID: usize = 72;
    pub const EXTD_FILE_NAME: usize = 88;

    // FILE_ID_BOTH_DIR_INFO.
    pub const BOTH_SHORT_NAME_LENGTH: usize = 68;
    pub const BOTH_SHORT_NAME: usize = 70;
    pub const BOTH_FILE_ID: usize = 96;
    pub const BOTH_FILE_NAME: usize = 104;

    // FILE_FULL_DIR_INFO.
    pub const FULL_FILE_NAME: usize = 68;
}

/// Where the id and reparse tag of a listing class live.
#[derive(Clone, Copy)]
enum Class {
    /// 128-bit id at `EXTD_FILE_ID`, tag at `EXTD_REPARSE_POINT_TAG`.
    Extd,
    /// 64-bit id at `BOTH_FILE_ID`; `EaSize` is the tag when the reparse bit is set.
    Both,
    /// No id; `EaSize` is the tag when the reparse bit is set.
    Full,
}

impl Class {
    fn name_offset(self) -> usize {
        match self {
            Class::Extd => layout::EXTD_FILE_NAME,
            Class::Both => layout::BOTH_FILE_NAME,
            Class::Full => layout::FULL_FILE_NAME,
        }
    }
}

/// Parses a `FILE_ID_EXTD_DIR_INFO` chain (128-bit ids).
pub fn parse_extd(
    buf: &[u8],
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    parse(buf, Class::Extd, out)
}

/// Parses a `FILE_ID_BOTH_DIR_INFO` chain (64-bit ids, zero-extended to 16 bytes).
pub fn parse_both(
    buf: &[u8],
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    parse(buf, Class::Both, out)
}

/// Parses a `FILE_FULL_DIR_INFO` chain (no ids: `file_id = None`).
pub fn parse_full(
    buf: &[u8],
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    parse(buf, Class::Full, out)
}

/// Reads `N` bytes at `at` from `entry`, or `None` if out of range.
fn bytes<const N: usize>(entry: &[u8], at: usize) -> Option<[u8; N]> {
    let end = at.checked_add(N)?;
    entry.get(at..end)?.try_into().ok()
}

fn u32_at(entry: &[u8], at: usize, offset: usize) -> Result<u32, DirBufError> {
    bytes::<4>(entry, at)
        .map(u32::from_le_bytes)
        .ok_or(DirBufError::Truncated { offset })
}

fn i64_at(entry: &[u8], at: usize, offset: usize) -> Result<i64, DirBufError> {
    bytes::<8>(entry, at)
        .map(i64::from_le_bytes)
        .ok_or(DirBufError::Truncated { offset })
}

/// A size field (`LARGE_INTEGER`); negative values (malformed) clamp to 0.
fn size_at(entry: &[u8], at: usize, offset: usize) -> Result<u64, DirBufError> {
    i64_at(entry, at, offset).map(|v| u64::try_from(v).unwrap_or(0))
}

fn is_dot_or_dotdot(name: &[u16]) -> bool {
    const DOT: u16 = b'.' as u16;
    name == [DOT] || name == [DOT, DOT]
}

fn parse(
    buf: &[u8],
    class: Class,
    out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
) -> Result<(), DirBufError> {
    let name_off = class.name_offset();
    // Reused across entries: the buffer is not guaranteed 2-aligned for a `&[u16]` view, and
    // reinterpreting would need code this module may not contain, so the name is copied.
    let mut name: Vec<u16> = Vec::with_capacity(256);
    let mut offset = 0usize;
    loop {
        let entry = buf.get(offset..).ok_or(DirBufError::Truncated { offset })?;
        if entry.len() < name_off {
            return Err(DirBufError::Truncated { offset });
        }

        let next = u32_at(entry, layout::NEXT_ENTRY_OFFSET, offset)? as usize;
        let name_len = u32_at(entry, layout::FILE_NAME_LENGTH, offset)? as usize;
        if name_len == 0 || name_len % 2 != 0 {
            return Err(DirBufError::BadNameLength { offset });
        }
        let name_end = name_off
            .checked_add(name_len)
            .ok_or(DirBufError::BadNameLength { offset })?;
        let name_bytes = entry
            .get(name_off..name_end)
            .ok_or(DirBufError::BadNameLength { offset })?;
        // The next entry must start after this one's name (no overlap, strictly forward) and
        // inside the buffer. Checked before delivering this entry so a corrupt chain stops at
        // the first bad link.
        let next_offset = if next == 0 {
            None
        } else {
            if next < name_end {
                return Err(DirBufError::BadNextOffset { offset });
            }
            let n = offset
                .checked_add(next)
                .filter(|&n| n < buf.len())
                .ok_or(DirBufError::BadNextOffset { offset })?;
            Some(n)
        };

        name.clear();
        name.extend(
            name_bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])),
        );

        if !is_dot_or_dotdot(&name) {
            let attributes = u32_at(entry, layout::FILE_ATTRIBUTES, offset)?;
            let (reparse_tag, file_id) = match class {
                Class::Extd => {
                    let tag = u32_at(entry, layout::EXTD_REPARSE_POINT_TAG, offset)?;
                    let id = bytes::<16>(entry, layout::EXTD_FILE_ID)
                        .ok_or(DirBufError::Truncated { offset })?;
                    (tag, Some(id))
                }
                Class::Both | Class::Full => {
                    // MS-FSCC 2.4.17/2.4.19: with FILE_ATTRIBUTE_REPARSE_POINT set, EaSize
                    // MUST contain the reparse tag.
                    let ea = u32_at(entry, layout::EA_SIZE, offset)?;
                    let tag = if attributes & attr::REPARSE_POINT != 0 {
                        ea
                    } else {
                        0
                    };
                    let id = match class {
                        Class::Both => {
                            let lo = bytes::<8>(entry, layout::BOTH_FILE_ID)
                                .ok_or(DirBufError::Truncated { offset })?;
                            let mut id = [0u8; 16];
                            id[..8].copy_from_slice(&lo);
                            Some(id)
                        }
                        _ => None,
                    };
                    (tag, id)
                }
            };
            let e = RawEntry {
                name: &name,
                attributes,
                reparse_tag,
                size: size_at(entry, layout::END_OF_FILE, offset)?,
                allocated: size_at(entry, layout::ALLOCATION_SIZE, offset)?,
                creation: FileTime::from_i64(i64_at(entry, layout::CREATION_TIME, offset)?),
                last_write: FileTime::from_i64(i64_at(entry, layout::LAST_WRITE_TIME, offset)?),
                change: FileTime::from_i64(i64_at(entry, layout::CHANGE_TIME, offset)?),
                file_id,
            };
            if out(e).is_break() {
                return Ok(());
            }
        }

        match next_offset {
            None => return Ok(()),
            Some(n) => offset = n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct E {
        name: Vec<u16>,
        attrs: u32,
        ea: u32,
        tag: u32,
        size: i64,
        alloc: i64,
        creation: i64,
        write: i64,
        change: i64,
        id: [u8; 16],
    }

    fn e(name: &str) -> E {
        E {
            name: name.encode_utf16().collect(),
            attrs: 0x20,
            ea: 0,
            tag: 0,
            size: 1234,
            alloc: 4096,
            creation: 100,
            write: 200,
            change: 300,
            id: core::array::from_fn(|i| i as u8 + 1),
        }
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum K {
        Extd,
        Both,
        Full,
    }

    fn name_off(k: K) -> usize {
        match k {
            K::Extd => layout::EXTD_FILE_NAME,
            K::Both => layout::BOTH_FILE_NAME,
            K::Full => layout::FULL_FILE_NAME,
        }
    }

    fn put_u32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_i64(b: &mut [u8], at: usize, v: i64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// One encoded entry, without padding.
    fn encode_one(k: K, x: &E) -> Vec<u8> {
        let no = name_off(k);
        let mut b = vec![0u8; no + x.name.len() * 2];
        put_i64(&mut b, layout::CREATION_TIME, x.creation);
        put_i64(&mut b, layout::LAST_ACCESS_TIME, 999);
        put_i64(&mut b, layout::LAST_WRITE_TIME, x.write);
        put_i64(&mut b, layout::CHANGE_TIME, x.change);
        put_i64(&mut b, layout::END_OF_FILE, x.size);
        put_i64(&mut b, layout::ALLOCATION_SIZE, x.alloc);
        put_u32(&mut b, layout::FILE_ATTRIBUTES, x.attrs);
        put_u32(&mut b, layout::FILE_NAME_LENGTH, (x.name.len() * 2) as u32);
        put_u32(&mut b, layout::EA_SIZE, x.ea);
        match k {
            K::Extd => {
                put_u32(&mut b, layout::EXTD_REPARSE_POINT_TAG, x.tag);
                b[layout::EXTD_FILE_ID..layout::EXTD_FILE_ID + 16].copy_from_slice(&x.id);
            }
            K::Both => {
                b[layout::BOTH_SHORT_NAME_LENGTH] = 4;
                for (i, u) in [b'S', b'H', 0, 0].iter().enumerate() {
                    b[layout::BOTH_SHORT_NAME + 2 * i] = *u;
                }
                b[layout::BOTH_FILE_ID..layout::BOTH_FILE_ID + 8].copy_from_slice(&x.id[..8]);
            }
            K::Full => {}
        }
        for (i, u) in x.name.iter().enumerate() {
            b[no + 2 * i..no + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        b
    }

    /// A chain with 8-byte aligned entries, as the OS produces it.
    fn encode(k: K, xs: &[E]) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, x) in xs.iter().enumerate() {
            let mut one = encode_one(k, x);
            if i + 1 < xs.len() {
                while one.len() % 8 != 0 {
                    one.push(0);
                }
                let len = one.len() as u32;
                put_u32(&mut one, layout::NEXT_ENTRY_OFFSET, len);
            }
            out.extend_from_slice(&one);
        }
        out
    }

    fn run(
        k: K,
        buf: &[u8],
        out: &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>,
    ) -> Result<(), DirBufError> {
        match k {
            K::Extd => parse_extd(buf, out),
            K::Both => parse_both(buf, out),
            K::Full => parse_full(buf, out),
        }
    }

    #[derive(Debug, PartialEq)]
    struct Owned {
        name: String,
        attributes: u32,
        reparse_tag: u32,
        size: u64,
        allocated: u64,
        creation: FileTime,
        last_write: FileTime,
        change: FileTime,
        file_id: Option<[u8; 16]>,
    }

    fn collect(k: K, buf: &[u8]) -> (Result<(), DirBufError>, Vec<Owned>) {
        let mut v = Vec::new();
        let r = run(k, buf, &mut |e| {
            v.push(Owned {
                name: String::from_utf16_lossy(e.name),
                attributes: e.attributes,
                reparse_tag: e.reparse_tag,
                size: e.size,
                allocated: e.allocated,
                creation: e.creation,
                last_write: e.last_write,
                change: e.change,
                file_id: e.file_id,
            });
            ControlFlow::Continue(())
        });
        (r, v)
    }

    const ALL: [K; 3] = [K::Extd, K::Both, K::Full];

    #[test]
    fn layout_is_self_consistent() {
        // Natural repr(C) placement of the SDK structs (also asserted against the windows
        // crate structs at compile time on Windows targets).
        assert_eq!(layout::EXTD_FILE_ID, layout::EXTD_REPARSE_POINT_TAG + 4);
        assert_eq!(layout::EXTD_FILE_NAME, layout::EXTD_FILE_ID + 16);
        assert_eq!(layout::BOTH_SHORT_NAME, layout::BOTH_SHORT_NAME_LENGTH + 2);
        assert_eq!(layout::BOTH_SHORT_NAME + 24, 94);
        assert_eq!(layout::BOTH_FILE_ID, 96); // 94 rounded up to the i64 alignment
        assert_eq!(layout::BOTH_FILE_NAME, layout::BOTH_FILE_ID + 8);
        assert_eq!(layout::FULL_FILE_NAME, layout::EA_SIZE + 4);
    }

    #[test]
    fn parses_all_fields_each_class() {
        for k in ALL {
            let mut a = e("alpha.tmp");
            a.size = 5;
            let b = e("b");
            let buf = encode(k, &[a.clone(), b]);
            let (r, v) = collect(k, &buf);
            assert_eq!(r, Ok(()), "{k:?}");
            assert_eq!(v.len(), 2, "{k:?}");
            assert_eq!(v[0].name, "alpha.tmp");
            assert_eq!(v[1].name, "b");
            assert_eq!(v[0].attributes, 0x20);
            assert_eq!(v[0].size, 5);
            assert_eq!(v[0].allocated, 4096);
            assert_eq!(v[0].creation, FileTime(100));
            assert_eq!(v[0].last_write, FileTime(200));
            assert_eq!(v[0].change, FileTime(300));
            assert_eq!(v[0].reparse_tag, 0);
            let want_id = match k {
                K::Extd => Some(a.id),
                K::Both => {
                    let mut id = [0u8; 16];
                    id[..8].copy_from_slice(&a.id[..8]);
                    Some(id)
                }
                K::Full => None,
            };
            assert_eq!(v[0].file_id, want_id, "{k:?}");
        }
    }

    #[test]
    fn filters_dot_and_dotdot_only() {
        for k in ALL {
            let buf = encode(k, &[e("."), e(".."), e("..."), e(".x"), e("x")]);
            let (r, v) = collect(k, &buf);
            assert_eq!(r, Ok(()));
            let names: Vec<_> = v.iter().map(|o| o.name.as_str()).collect();
            assert_eq!(names, ["...", ".x", "x"], "{k:?}");
        }
    }

    #[test]
    fn reparse_tag_sources() {
        // Extd: the dedicated field, independent of EaSize.
        let mut x = e("j");
        x.attrs = attr::REPARSE_POINT | attr::DIRECTORY;
        x.tag = 0xA000_0003;
        x.ea = 77;
        let (_, v) = collect(K::Extd, &encode(K::Extd, &[x.clone()]));
        assert_eq!(v[0].reparse_tag, 0xA000_0003);
        // Both/Full: EaSize when the reparse bit is set...
        x.ea = 0xA000_000C;
        for k in [K::Both, K::Full] {
            let (_, v) = collect(k, &encode(k, &[x.clone()]));
            assert_eq!(v[0].reparse_tag, 0xA000_000C, "{k:?}");
        }
        // ...and 0 (EaSize is an EA length) when it is not.
        let mut y = e("f");
        y.ea = 512;
        for k in [K::Both, K::Full] {
            let (_, v) = collect(k, &encode(k, &[y.clone()]));
            assert_eq!(v[0].reparse_tag, 0, "{k:?}");
        }
    }

    #[test]
    fn negative_times_and_sizes_clamp_to_zero() {
        for k in ALL {
            let mut x = e("n");
            x.size = -1;
            x.alloc = i64::MIN;
            x.write = -5;
            x.change = i64::MIN;
            x.creation = -1;
            let (r, v) = collect(k, &encode(k, &[x]));
            assert_eq!(r, Ok(()));
            assert_eq!(v[0].size, 0);
            assert_eq!(v[0].allocated, 0);
            assert_eq!(v[0].last_write, FileTime(0));
            assert_eq!(v[0].change, FileTime(0));
            assert_eq!(v[0].creation, FileTime(0));
        }
    }

    #[test]
    fn non_ascii_and_lone_surrogate_names_are_exact() {
        for k in ALL {
            let mut x = e("");
            x.name = vec![0x00E9, 0xD800, 0x0041, 0xDFFF, 0xD83D, 0xDE00];
            let buf = encode(k, &[x.clone()]);
            let mut got = Vec::new();
            let r = run(k, &buf, &mut |e| {
                got = e.name.to_vec();
                ControlFlow::Continue(())
            });
            assert_eq!(r, Ok(()));
            assert_eq!(got, x.name, "{k:?}");
        }
    }

    #[test]
    fn break_stops_early() {
        for k in ALL {
            let buf = encode(k, &[e("a"), e("b"), e("c")]);
            let mut seen = 0;
            let r = run(k, &buf, &mut |_| {
                seen += 1;
                ControlFlow::Break(())
            });
            assert_eq!(r, Ok(()));
            assert_eq!(seen, 1);
        }
    }

    #[test]
    fn unaligned_next_offset_is_accepted_when_in_bounds() {
        // Parsing is byte-wise; alignment is the OS's business, not a safety property.
        for k in ALL {
            let a = encode_one(k, &e("a"));
            let mut buf = a.clone();
            let len = buf.len() as u32;
            put_u32(&mut buf, 0, len); // not padded to 8
            buf.extend_from_slice(&encode_one(k, &e("bb")));
            let (r, v) = collect(k, &buf);
            assert_eq!(r, Ok(()), "{k:?}");
            assert_eq!(v.len(), 2);
        }
    }

    #[test]
    fn empty_and_short_buffers() {
        for k in ALL {
            assert_eq!(collect(k, &[]).0, Err(DirBufError::Truncated { offset: 0 }));
            for n in [1, 4, 8, name_off(k) - 1] {
                let buf = vec![0u8; n];
                assert_eq!(
                    collect(k, &buf).0,
                    Err(DirBufError::Truncated { offset: 0 }),
                    "{k:?} {n}"
                );
            }
        }
    }

    #[test]
    fn bad_name_lengths() {
        for k in ALL {
            let good = encode(k, &[e("abc")]);
            // zero, odd, too long by one unit, absurd.
            for bad in [0u32, 1, 5, 8, u32::MAX, u32::MAX - 1] {
                let mut b = good.clone();
                put_u32(&mut b, layout::FILE_NAME_LENGTH, bad);
                let (r, v) = collect(k, &b);
                assert_eq!(
                    r,
                    Err(DirBufError::BadNameLength { offset: 0 }),
                    "{k:?} {bad}"
                );
                assert!(v.is_empty());
            }
            // Name cut off by the end of the buffer.
            let mut b = good.clone();
            b.truncate(b.len() - 1);
            assert_eq!(
                collect(k, &b).0,
                Err(DirBufError::BadNameLength { offset: 0 })
            );
        }
    }

    #[test]
    fn bad_next_offsets() {
        for k in ALL {
            let two = encode(k, &[e("a"), e("b")]);
            let first_len = u32::from_le_bytes(two[0..4].try_into().unwrap());
            let no = name_off(k) as u32;
            // Self-loop, backwards/overlapping into own header and name, past the end,
            // exactly the end, absurd.
            for bad in [
                1u32,
                4,
                no,
                no + 1, // inside own name (name is 2 bytes long)
                two.len() as u32,
                two.len() as u32 + 8,
                u32::MAX,
            ] {
                let mut b = two.clone();
                put_u32(&mut b, 0, bad);
                let (r, v) = collect(k, &b);
                assert_eq!(
                    r,
                    Err(DirBufError::BadNextOffset { offset: 0 }),
                    "{k:?} {bad}"
                );
                assert!(v.is_empty(), "the entry with the bad link is not delivered");
            }
            // A valid first link to a truncated second entry: first is delivered, then error.
            let mut b = two.clone();
            b.truncate(first_len as usize + 10);
            let (r, v) = collect(k, &b);
            assert_eq!(
                r,
                Err(DirBufError::Truncated {
                    offset: first_len as usize
                })
            );
            assert_eq!(v.len(), 1);
        }
    }

    #[test]
    fn long_chain_terminates() {
        for k in ALL {
            let xs: Vec<E> = (0..500).map(|i| e(&format!("file{i}"))).collect();
            let (r, v) = collect(k, &encode(k, &xs));
            assert_eq!(r, Ok(()));
            assert_eq!(v.len(), 500);
        }
    }

    /// Tiny deterministic PRNG (xorshift) so the corpus is reproducible without dev-deps.
    fn rng(seed: &mut u64) -> u64 {
        let mut x = *seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *seed = x;
        x
    }

    #[test]
    fn random_garbage_never_panics_and_always_terminates() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        for round in 0..3000 {
            let len = (rng(&mut seed) % 600) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| rng(&mut seed) as u8).collect();
            // Bias some rounds toward plausible headers to reach deeper code paths.
            if round % 2 == 0 && len >= 64 {
                let nl = ((rng(&mut seed) % 40) * 2) as u32;
                put_u32(&mut buf, layout::FILE_NAME_LENGTH, nl);
                let nx = (rng(&mut seed) % 256) as u32;
                put_u32(&mut buf, 0, nx);
            }
            for k in ALL {
                let mut calls = 0usize;
                let _ = run(k, &buf, &mut |e| {
                    calls += 1;
                    assert!(!e.name.is_empty());
                    ControlFlow::Continue(())
                });
                // Every delivered entry consumed at least name_off + 2 bytes.
                assert!(calls <= len / (name_off(k) + 2) + 1);
            }
        }
    }

    #[test]
    fn mutated_valid_buffers_never_panic() {
        let mut seed = 42u64;
        for k in ALL {
            let base = encode(k, &[e("one"), e("two.tmp"), e("three"), e(".."), e("x")]);
            for _ in 0..3000 {
                let mut b = base.clone();
                for _ in 0..(1 + rng(&mut seed) % 4) {
                    let i = (rng(&mut seed) as usize) % b.len();
                    b[i] = rng(&mut seed) as u8;
                }
                if rng(&mut seed) % 4 == 0 {
                    let cut = (rng(&mut seed) as usize) % (b.len() + 1);
                    b.truncate(cut);
                }
                let _ = collect(k, &b);
            }
        }
    }
}
