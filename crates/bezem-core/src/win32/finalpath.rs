// SPDX-License-Identifier: MIT

//! `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)`, growing the buffer.
//!
//! Ownership: work package **A**.

use windows::Win32::Storage::FileSystem::{
    GetFinalPathNameByHandleW, FILE_NAME_NORMALIZED, GETFINALPATHNAMEBYHANDLE_FLAGS,
    VOLUME_NAME_DOS,
};

use super::{last_error, OwnedHandle};
use crate::error::Win32Error;
use crate::fs::status::ERROR_INSUFFICIENT_BUFFER;

const API: &str = "GetFinalPathNameByHandleW";

/// `FILE_NAME_NORMALIZED | VOLUME_NAME_DOS` (both 0: long names, `\\?\X:\` form).
const FLAGS: GETFINALPATHNAMEBYHANDLE_FLAGS =
    GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0);

/// First attempt: enough for almost every real path.
const INITIAL_UNITS: usize = 512;
/// Upper bound for a buffer (UNICODE_STRING limit of 32,767 units plus slack). A larger
/// "required size" is not a real path; fail instead of allocating.
const MAX_UNITS: usize = 32_768 + 16;
/// The path can change between calls (rename of an ancestor); retry a few times only.
const ATTEMPTS: usize = 4;

/// The raw final path (no NUL). Validation happens in `path::parse_final`.
pub(crate) fn final_path(h: &OwnedHandle) -> Result<Vec<u16>, Win32Error> {
    let mut buf = vec![0u16; INITIAL_UNITS];
    for _ in 0..ATTEMPTS {
        // SAFETY: `h` is an open handle (OwnedHandle invariant, kept alive by the borrow).
        // `buf` is a live, writable slice; the wrapper passes its pointer and exact length
        // (<= MAX_UNITS, so the crate's u32 conversion cannot fail) and the API writes at most
        // that many units.
        let n = unsafe { GetFinalPathNameByHandleW(h.raw(), &mut buf, FLAGS) } as usize;
        if n == 0 {
            return Err(Win32Error {
                api: API,
                code: last_error(),
            });
        }
        if n < buf.len() {
            // Success: `n` units written, excluding the terminating NUL.
            buf.truncate(n);
            return Ok(buf);
        }
        // Too small: `n` is the required size including the NUL.
        if n > MAX_UNITS {
            break;
        }
        buf.resize(n, 0);
    }
    Err(Win32Error {
        api: API,
        code: ERROR_INSUFFICIENT_BUFFER,
    })
}
