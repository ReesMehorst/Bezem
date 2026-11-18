// SPDX-License-Identifier: MIT

//! `read_dir`: directory listing via `GetFileInformationByHandleEx` with a 64 KiB 8-aligned
//! buffer; `FileIdExtdDirectory*` with fallbacks to `FileIdBothDirectory*` and
//! `FileFullDirectory*`. Buffers are parsed by `fs::dirbuf` (safe code).
//!
//! Ownership: work package **A**.

use core::ffi::c_void;
use std::ops::ControlFlow;

use windows::Win32::Storage::FileSystem::{
    FileFullDirectoryInfo, FileFullDirectoryRestartInfo, FileIdBothDirectoryInfo,
    FileIdBothDirectoryRestartInfo, FileIdExtdDirectoryInfo, FileIdExtdDirectoryRestartInfo,
    GetFileInformationByHandleEx, FILE_INFO_BY_HANDLE_CLASS,
};

use super::{win32_code, OwnedHandle};
use crate::error::Win32Error;
use crate::fs::dirbuf::{self, DirBufError};
use crate::fs::status::{self, ERROR_NO_MORE_FILES};
use crate::fs::{IdQuality, RawEntry};

const API: &str = "GetFileInformationByHandleEx";

/// Buffer size in bytes (A:1147-1154: 64 KiB batches).
const BUF_BYTES: usize = 64 * 1024;

type Parser = fn(&[u8], &mut dyn FnMut(RawEntry<'_>) -> ControlFlow<()>) -> Result<(), DirBufError>;

/// One listing class: (restart class for the first call, continue class, quality, parser).
struct Listing {
    restart: FILE_INFO_BY_HANDLE_CLASS,
    next: FILE_INFO_BY_HANDLE_CLASS,
    quality: IdQuality,
    parse: Parser,
}

const LISTINGS: [Listing; 3] = [
    Listing {
        restart: FileIdExtdDirectoryRestartInfo,
        next: FileIdExtdDirectoryInfo,
        quality: IdQuality::Full128,
        parse: dirbuf::parse_extd,
    },
    Listing {
        restart: FileIdBothDirectoryRestartInfo,
        next: FileIdBothDirectoryInfo,
        quality: IdQuality::Id64,
        parse: dirbuf::parse_both,
    },
    Listing {
        restart: FileFullDirectoryRestartInfo,
        next: FileFullDirectoryInfo,
        quality: IdQuality::None,
        parse: dirbuf::parse_full,
    },
];

/// Why a listing failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadDirError {
    /// The OS call failed.
    Api(Win32Error),
    /// The OS returned a buffer that does not parse (never trusted beyond the entries already
    /// delivered).
    Malformed(DirBufError),
}

/// A 64 KiB buffer with 8-byte alignment (the entries contain `LARGE_INTEGER`s and the OS
/// requires the buffer to be aligned for them).
struct DirBuf(Vec<u64>);

impl DirBuf {
    fn new() -> DirBuf {
        DirBuf(vec![0u64; BUF_BYTES / 8])
    }

    /// One `GetFileInformationByHandleEx` call into the buffer; `Err` is the Win32 code.
    fn fill(&mut self, dir: &OwnedHandle, class: FILE_INFO_BY_HANDLE_CLASS) -> Result<(), u32> {
        // Zeroed so that nothing from an earlier batch can be misread as part of this one.
        self.0.fill(0);
        // SAFETY: `dir` is an open directory handle (OwnedHandle invariant, kept alive by the
        // borrow) opened for synchronous I/O with FILE_LIST_DIRECTORY. The pointer and length
        // describe exactly the live, writable, 8-aligned `Vec<u64>` allocation (BUF_BYTES,
        // which fits u32), and the API writes at most that many bytes. `class` is one of the
        // directory classes whose output is a byte chain parsed by `dirbuf` (no Rust type is
        // ever materialized from it).
        unsafe {
            GetFileInformationByHandleEx(
                dir.raw(),
                class,
                self.0.as_mut_ptr().cast::<c_void>(),
                BUF_BYTES as u32,
            )
        }
        .map_err(|e| win32_code(&e))
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: the pointer comes from a live `Vec<u64>` of `len` elements, i.e.
        // `len * 8` initialised bytes (u64 has no padding, every byte is initialised, and `u8`
        // has alignment 1 and accepts every value). The returned slice borrows `self`, so the
        // allocation cannot be mutated or freed while it is alive.
        unsafe { core::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), self.0.len() * 8) }
    }
}

/// Lists `dir`; returns the id quality of the class that worked.
///
/// The first call uses the restart class of `FileIdExtdDirectory*`. If that class is not
/// supported (`ERROR_INVALID_PARAMETER`, `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_FUNCTION`), the
/// next class is tried: `FileIdBothDirectory*` (64-bit ids) then `FileFullDirectory*` (no
/// ids). `ERROR_NO_MORE_FILES` ends the listing. The caller serializes listings of one handle
/// (the enumeration cursor lives in the file object).
pub(crate) fn read_dir(
    dir: &OwnedHandle,
    sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
) -> Result<IdQuality, ReadDirError> {
    let mut buf = DirBuf::new();
    let api_err = |code| ReadDirError::Api(Win32Error { api: API, code });

    for (i, listing) in LISTINGS.iter().enumerate() {
        match buf.fill(dir, listing.restart) {
            Ok(()) => return drain(dir, listing, &mut buf, sink),
            // Empty directory (a volume root has no `.`/`..`).
            Err(ERROR_NO_MORE_FILES) => return Ok(listing.quality),
            Err(code)
                if status::is_disposition_unsupported_win32(code) && i + 1 < LISTINGS.len() =>
            {
                continue
            }
            Err(code) => return Err(api_err(code)),
        }
    }
    // Unreachable: the last listing returns from every arm. Fail closed anyway.
    Err(api_err(status::ERROR_NOT_SUPPORTED))
}

/// Delivers the batch already in `buf`, then keeps calling the continue class until
/// `ERROR_NO_MORE_FILES` or the sink breaks.
fn drain(
    dir: &OwnedHandle,
    listing: &Listing,
    buf: &mut DirBuf,
    sink: &mut dyn FnMut(&RawEntry<'_>) -> ControlFlow<()>,
) -> Result<IdQuality, ReadDirError> {
    loop {
        let mut stop = false;
        (listing.parse)(buf.bytes(), &mut |e| {
            let flow = sink(&e);
            stop = flow.is_break();
            flow
        })
        .map_err(ReadDirError::Malformed)?;
        if stop {
            return Ok(listing.quality);
        }
        match buf.fill(dir, listing.next) {
            Ok(()) => {}
            Err(ERROR_NO_MORE_FILES) => return Ok(listing.quality),
            Err(code) => return Err(ReadDirError::Api(Win32Error { api: API, code })),
        }
    }
}
