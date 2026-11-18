// SPDX-License-Identifier: MIT

//! [`OwnedHandle`]: an owned kernel handle that is closed exactly once, on drop.

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};

#[cfg(windows)]
/// An open kernel object handle (file, directory, process, pipe, ...) owned by this value.
///
/// Invariants, established by [`OwnedHandle::from_raw`] and relied on by every wrapper:
/// - `raw` is a real user-mode handle: not null, not `INVALID_HANDLE_VALUE`, and not a
///   pseudo-handle such as `GetCurrentProcess()` (all of those are `<= 0` as an `isize`).
/// - This value is the only owner of `raw`, so `CloseHandle` runs exactly once, in `Drop`.
/// - `raw` never leaves `crate::win32`; other modules only pass `&OwnedHandle` to wrappers.
#[derive(Debug)]
pub struct OwnedHandle {
    raw: HANDLE,
}

#[cfg(windows)]
impl OwnedHandle {
    /// Takes ownership of `raw`. Returns `None` (and does not close anything) for null,
    /// `INVALID_HANDLE_VALUE` and pseudo-handles, so a failed open can never be owned.
    ///
    /// # Safety
    ///
    /// If `raw` is a real handle, it must have been returned by a successful
    /// create/open/duplicate call in this process, must be closable with `CloseHandle`
    /// (not a `FindFirstFile`, registry or socket handle), must **not** have been opened for
    /// overlapped I/O (no `FILE_FLAG_OVERLAPPED`; NT opens use `FILE_SYNCHRONOUS_IO_NONALERT`),
    /// which the `Sync` impl relies on, and the caller must not use or close it again:
    /// ownership moves into the returned value.
    #[cfg_attr(not(test), expect(dead_code, reason = "first caller arrives with W3"))]
    pub(in crate::win32) unsafe fn from_raw(raw: HANDLE) -> Option<Self> {
        // User-mode handle values are positive; 0, -1 (INVALID_HANDLE_VALUE and
        // GetCurrentProcess) and the other small negative pseudo-handles are rejected.
        if (raw.0.addr() as isize) <= 0 {
            None
        } else {
            Some(Self { raw })
        }
    }

    /// The raw handle, for passing to an API inside `crate::win32` only. The value stays
    /// owned by `self`: callers must not close it or keep it beyond the borrow.
    pub(in crate::win32) fn as_raw(&self) -> HANDLE {
        self.raw
    }
}

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: by the type invariant `raw` is a valid handle owned only by `self`, and
        // `drop` runs at most once, so this is the single CloseHandle for it and nothing can
        // use the value afterwards.
        let closed = unsafe { CloseHandle(self.as_raw()) };
        // A failed close cannot be recovered from in `drop`, and panicking here is not
        // allowed (ARCHITECTURE.md section 13.2). The handle is ours, so it cannot fail in
        // practice; ignore the result.
        drop(closed);
    }
}

#[cfg(windows)]
// SAFETY: a kernel handle is a process-wide value with no thread affinity (unlike, say, a
// window or a COM apartment pointer), so it may be moved to and closed on any thread.
unsafe impl Send for OwnedHandle {}

#[cfg(windows)]
// SAFETY: `&OwnedHandle` allows only passing the handle value to system calls (through the
// win32 wrappers), and no wrapper performs overlapped I/O, so no kernel operation borrows Rust
// memory beyond the call that started it. Concurrent synchronous calls on one handle are
// serialized by the kernel. Same reasoning as `std::os::windows::io::OwnedHandle`.
unsafe impl Sync for OwnedHandle {}

#[cfg(all(windows, test))]
mod tests {
    use super::*;

    #[test]
    fn rejects_null_invalid_and_pseudo_handles() {
        for v in [0isize, -1, -2, -6] {
            // SAFETY: none of these values is a real handle, so `from_raw` returns `None`
            // and takes ownership of nothing.
            let h = unsafe {
                OwnedHandle::from_raw(HANDLE(std::ptr::without_provenance_mut(v as usize)))
            };
            assert!(h.is_none(), "{v} accepted");
        }
    }

    #[test]
    fn closes_a_real_handle_on_drop() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::IntoRawHandle;
        use windows::Win32::Foundation::GetHandleInformation;

        let path = std::env::temp_dir().join(format!("bezem-handle-{}.tmp", std::process::id()));
        // Share mode 0: nobody else can open the file while this handle is alive.
        let exclusive = || {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .share_mode(0)
                .open(&path)
        };
        let raw = HANDLE(exclusive().unwrap().into_raw_handle());
        // SAFETY: `raw` was just released by `File::into_raw_handle`, so we own it and
        // nothing else will close it.
        let owned = unsafe { OwnedHandle::from_raw(raw) }.unwrap();

        let mut flags = 0u32;
        // SAFETY: `owned` keeps the handle open for this call, and `flags` is a live, writable
        // u32 for its whole duration.
        let info = unsafe { GetHandleInformation(owned.as_raw(), &mut flags) };
        assert!(info.is_ok(), "handle not valid while owned: {info:?}");
        assert!(
            exclusive().is_err(),
            "exclusive handle did not block a second open"
        );

        drop(owned);
        // Deterministic proof of the close: the exclusive open succeeds again.
        drop(exclusive().unwrap());
        std::fs::remove_file(&path).unwrap();
    }
}

/// Placeholder on non-Windows targets: it has no values, so no handle can exist there and
/// every wrapper returns an `unsupported_platform` error before reaching one.
#[cfg(not(windows))]
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "uninhabited by design: never constructed off Windows"
)]
pub struct OwnedHandle {
    _never: std::convert::Infallible,
}
