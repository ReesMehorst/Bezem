// SPDX-License-Identifier: MIT

//! [`OwnedHandle`]: an owned kernel handle, closed with `CloseHandle` on drop.
//!
//! Ownership: work package **A**.

use windows::Win32::Foundation::{CloseHandle, HANDLE};

/// An owned kernel handle. Never exposed outside `win32`.
///
/// Invariant: `self.0` is a valid, open handle that this value exclusively owns (it is closed
/// exactly once, in `Drop`). It is never null and never `INVALID_HANDLE_VALUE`.
#[derive(Debug)]
pub(crate) struct OwnedHandle(HANDLE);

// SAFETY: a kernel handle value is a process-wide table index, not a pointer into memory owned
// by one thread; the kernel synchronizes every operation on it, so moving it to another
// thread is sound. `OwnedHandle` gives out no interior mutability.
unsafe impl Send for OwnedHandle {}
// SAFETY: as above; all operations through `&OwnedHandle` are kernel calls that are safe to
// issue concurrently from several threads on the same handle.
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    /// Takes ownership of `h`. Returns `None` (and closes nothing) for null or
    /// `INVALID_HANDLE_VALUE`, so a failed open can never be wrapped.
    ///
    /// # Safety
    ///
    /// `h` must be either null/`INVALID_HANDLE_VALUE`, or an open handle that the caller owns
    /// and does not close or wrap a second time: ownership moves into the returned value.
    pub(crate) unsafe fn from_raw(h: HANDLE) -> Option<OwnedHandle> {
        // `is_invalid` is true for both null and `INVALID_HANDLE_VALUE` (-1).
        if h.is_invalid() {
            None
        } else {
            Some(OwnedHandle(h))
        }
    }

    /// The raw handle, for calls inside `win32` only. Valid while `self` is alive.
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: by the type invariant `self.0` is an open handle exclusively owned by this
        // value, and `drop` runs at most once, so it is closed exactly once and no copy of the
        // raw value outlives `self` (`raw()` borrows). A close failure cannot be acted on and
        // must not panic, so the result is ignored.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
