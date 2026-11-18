// SPDX-License-Identifier: MIT

//! [`OwnedHandle`]: an owned kernel handle, closed with `CloseHandle` on drop.
//!
//! Ownership: work package **A** (adds the `Drop` impl).

use windows::Win32::Foundation::HANDLE;

/// An owned kernel handle. Never exposed outside `win32`.
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
    /// The raw handle, for calls inside `win32` only.
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}
