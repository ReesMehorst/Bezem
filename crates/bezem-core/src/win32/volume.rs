// SPDX-License-Identifier: MIT

//! Local-fixed-volume check: `NtQueryVolumeInformationFile(FileFsDeviceInformation)`
//! (`FILE_REMOTE_DEVICE` => Remote) and `GetDriveTypeW`.
//!
//! Ownership: work package **A**.

use core::ffi::c_void;
use core::mem::size_of;

use windows::core::PCWSTR;
use windows::Wdk::Storage::FileSystem::{FileFsDeviceInformation, NtQueryVolumeInformationFile};
use windows::Wdk::System::SystemServices::{FILE_FS_DEVICE_INFORMATION, FILE_REMOTE_DEVICE};
use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::Win32::System::IO::IO_STATUS_BLOCK;

use super::OwnedHandle;
use crate::error::NtError;
use crate::fs::VolumeKind;

// `GetDriveTypeW` results (winbase.h). Defined locally: the crate only exports them from a
// much larger feature (`Win32_System_WindowsProgramming`).
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;

/// The drive letter of a `\\?\X:\...` (or `\\?\X:`) path, or `None` for any other form.
fn drive_letter(final_path: &[u16]) -> Option<u16> {
    let prefix: [u16; 4] = [b'\\'.into(), b'\\'.into(), b'?'.into(), b'\\'.into()];
    let rest = final_path.strip_prefix(&prefix[..])?;
    let (&letter, rest) = rest.split_first()?;
    let is_letter = u8::try_from(letter).is_ok_and(|b| b.is_ascii_alphabetic());
    let colon_then_sep = match rest {
        [c] => *c == u16::from(b':'),
        [c, s, ..] => *c == u16::from(b':') && *s == u16::from(b'\\'),
        _ => false,
    };
    (is_letter && colon_then_sep).then_some(letter)
}

/// The kind of volume `h` (whose final path is `final_path`) lives on.
///
/// `Remote` if the device reports `FILE_REMOTE_DEVICE`; otherwise the `GetDriveTypeW` class of
/// the final path's drive letter. A final path without a drive letter is `Other` (the policy
/// rejects that form anyway).
pub(crate) fn volume_kind(h: &OwnedHandle, final_path: &[u16]) -> Result<VolumeKind, NtError> {
    let mut info = FILE_FS_DEVICE_INFORMATION::default();
    let mut iosb = IO_STATUS_BLOCK::default();
    // SAFETY: `h` is an open handle (OwnedHandle invariant, kept alive by the borrow); `iosb`
    // and `info` are live, writable locals; `info` is the documented output struct of
    // FileFsDeviceInformation (two u32s, valid for any bit pattern) and the length passed is
    // its exact size (8 bytes, cannot truncate).
    let st = unsafe {
        NtQueryVolumeInformationFile(
            h.raw(),
            &mut iosb,
            (&mut info as *mut FILE_FS_DEVICE_INFORMATION).cast::<c_void>(),
            size_of::<FILE_FS_DEVICE_INFORMATION>() as u32,
            FileFsDeviceInformation,
        )
    };
    if st.0 < 0 {
        return Err(NtError {
            api: "NtQueryVolumeInformationFile",
            status: st.0,
        });
    }
    if info.Characteristics & FILE_REMOTE_DEVICE != 0 {
        return Ok(VolumeKind::Remote);
    }

    let Some(letter) = drive_letter(final_path) else {
        return Ok(VolumeKind::Other);
    };
    let root: [u16; 4] = [letter, b':'.into(), b'\\'.into(), 0];
    // SAFETY: `root` is a live, NUL-terminated UTF-16 string ("X:\") that the API only reads.
    let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    Ok(match kind {
        DRIVE_FIXED => VolumeKind::LocalFixed,
        DRIVE_REMOVABLE => VolumeKind::Removable,
        DRIVE_REMOTE => VolumeKind::Remote,
        _ => VolumeKind::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::drive_letter;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn drive_letters() {
        assert_eq!(drive_letter(&w(r"\\?\C:\Users\x")), Some(u16::from(b'C')));
        assert_eq!(drive_letter(&w(r"\\?\d:\")), Some(u16::from(b'd')));
        assert_eq!(drive_letter(&w(r"\\?\E:")), Some(u16::from(b'E')));
        for bad in [
            r"\\?\UNC\server\share",
            r"\\?\Volume{0}\x",
            r"C:\x",
            r"\\?\1:\x",
            r"\\?\CC:\x",
            r"\\?\C:x",
            r"\\?\",
            "",
        ] {
            assert_eq!(drive_letter(&w(bad)), None, "{bad}");
        }
    }
}
