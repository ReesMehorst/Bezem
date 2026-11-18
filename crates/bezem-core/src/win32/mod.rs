// SPDX-License-Identifier: MIT

//! The reviewed Win32/NT layer: the ONLY module allowed to contain `unsafe` code
//! (`lib.rs` opts it in with `#[allow(unsafe_code)]`; CI `unsafe-audit` excludes this
//! directory). Windows only. M1 design section 3.
//!
//! Ownership: work package **A**. Rules: every `unsafe` block carries a `// SAFETY:` comment;
//! wrappers return `Win32Error`/`NtError` (or the seam's error types), never panic and never
//! expose raw handles. Wrappers report facts; all policy lives in `safety`.
//!
//! Runtime behavior (junctions, sharing violations, POSIX delete, listing fallbacks) can only
//! be confirmed on Windows CI; on Linux this module is verified with
//! `cargo check/clippy --target x86_64-pc-windows-msvc` (which also evaluates the
//! compile-time layout assertions in `layout_check`).

mod dirent;
mod dispose;
mod finalpath;
mod fs_impl;
mod handle;
mod info;
// Only reachable through `KnownFolders::from_system`, which package B still stubs.
#[allow(dead_code)] // M1-STUB (B): remove when KnownFolders::from_system uses it
mod known_folders;
mod layout_check;
mod nt;
mod open;
#[cfg(test)]
mod tests;
mod volume;

pub use fs_impl::{Win32Dir, Win32Fs, Win32Item};
pub(crate) use handle::OwnedHandle;
#[allow(unused_imports)] // M1-STUB (B): used by KnownFolders::from_system
pub(crate) use known_folders::SystemKnownFolders;

use windows::Win32::Foundation::GetLastError;

/// `GetLastError()` as a plain code. Call immediately after the failing API.
fn last_error() -> u32 {
    // SAFETY: GetLastError only reads the calling thread's last-error value; no preconditions.
    unsafe { GetLastError() }.0
}

/// The Win32 code inside a `windows::core::Error` produced by a failing BOOL API
/// (`HRESULT_FROM_WIN32(GetLastError())`, i.e. `0x8007xxxx`). Other HRESULTs are returned
/// unchanged (as their u32 bit pattern) so nothing is lost for the log.
fn win32_code(e: &windows::core::Error) -> u32 {
    let hr = e.code().0 as u32;
    if hr & 0xFFFF_0000 == 0x8007_0000 {
        hr & 0xFFFF
    } else {
        hr
    }
}
