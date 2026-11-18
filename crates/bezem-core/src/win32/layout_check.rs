// SPDX-License-Identifier: MIT

//! Compile-time checks that the hand-written byte offsets and constants used by the pure,
//! Linux-tested code (`fs::dirbuf`, `fs::status`, `fs::attr`) agree with the `windows` crate
//! definitions. These are `const` assertions, so they are evaluated by
//! `cargo check --target x86_64-pc-windows-msvc` on Linux as well as on Windows CI.
//!
//! Ownership: work package **A**.

use core::mem::{offset_of, size_of};

use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT,
    FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_DELETE_PENDING, ERROR_DIR_NOT_EMPTY, ERROR_FILE_NOT_FOUND,
    ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER,
    ERROR_LOCK_VIOLATION, ERROR_NOT_SUPPORTED, ERROR_NO_MORE_FILES, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, ERROR_USER_MAPPED_FILE, STATUS_ACCESS_DENIED, STATUS_CANNOT_DELETE,
    STATUS_DELETE_PENDING, STATUS_DIRECTORY_NOT_EMPTY, STATUS_FILE_IS_A_DIRECTORY,
    STATUS_FILE_LOCK_CONFLICT, STATUS_LOCK_NOT_GRANTED, STATUS_NOT_A_DIRECTORY,
    STATUS_NO_SUCH_FILE, STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_INVALID,
    STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND, STATUS_OBJECT_PATH_SYNTAX_BAD,
    STATUS_PRIVILEGE_NOT_HELD, STATUS_SHARING_VIOLATION, STATUS_USER_MAPPED_FILE,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_OFFLINE,
    FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_FULL_DIR_INFO, FILE_ID_BOTH_DIR_INFO,
    FILE_ID_EXTD_DIR_INFO,
};

use crate::fs::dirbuf::layout as l;
use crate::fs::{attr, status as st};

macro_rules! common_prefix {
    ($t:ty) => {
        assert!(offset_of!($t, NextEntryOffset) == l::NEXT_ENTRY_OFFSET);
        assert!(offset_of!($t, CreationTime) == l::CREATION_TIME);
        assert!(offset_of!($t, LastAccessTime) == l::LAST_ACCESS_TIME);
        assert!(offset_of!($t, LastWriteTime) == l::LAST_WRITE_TIME);
        assert!(offset_of!($t, ChangeTime) == l::CHANGE_TIME);
        assert!(offset_of!($t, EndOfFile) == l::END_OF_FILE);
        assert!(offset_of!($t, AllocationSize) == l::ALLOCATION_SIZE);
        assert!(offset_of!($t, FileAttributes) == l::FILE_ATTRIBUTES);
        assert!(offset_of!($t, FileNameLength) == l::FILE_NAME_LENGTH);
        assert!(offset_of!($t, EaSize) == l::EA_SIZE);
    };
}

// fs::dirbuf offsets (M1 design section 3.3, with FULL's FileName corrected to 68).
const _: () = {
    common_prefix!(FILE_ID_EXTD_DIR_INFO);
    assert!(offset_of!(FILE_ID_EXTD_DIR_INFO, ReparsePointTag) == l::EXTD_REPARSE_POINT_TAG);
    assert!(offset_of!(FILE_ID_EXTD_DIR_INFO, FileId) == l::EXTD_FILE_ID);
    assert!(offset_of!(FILE_ID_EXTD_DIR_INFO, FileName) == l::EXTD_FILE_NAME);

    common_prefix!(FILE_ID_BOTH_DIR_INFO);
    assert!(offset_of!(FILE_ID_BOTH_DIR_INFO, ShortNameLength) == l::BOTH_SHORT_NAME_LENGTH);
    assert!(offset_of!(FILE_ID_BOTH_DIR_INFO, ShortName) == l::BOTH_SHORT_NAME);
    assert!(offset_of!(FILE_ID_BOTH_DIR_INFO, FileId) == l::BOTH_FILE_ID);
    assert!(offset_of!(FILE_ID_BOTH_DIR_INFO, FileName) == l::BOTH_FILE_NAME);

    common_prefix!(FILE_FULL_DIR_INFO);
    assert!(offset_of!(FILE_FULL_DIR_INFO, FileName) == l::FULL_FILE_NAME);

    // The 128-bit id is a plain byte array (alignment 1) and the 64-bit one an i64.
    assert!(size_of::<windows::Win32::Storage::FileSystem::FILE_ID_128>() == 16);
};

// fs::attr constants.
const _: () = {
    assert!(FILE_ATTRIBUTE_READONLY.0 == attr::READONLY);
    assert!(FILE_ATTRIBUTE_HIDDEN.0 == attr::HIDDEN);
    assert!(FILE_ATTRIBUTE_SYSTEM.0 == attr::SYSTEM);
    assert!(FILE_ATTRIBUTE_DIRECTORY.0 == attr::DIRECTORY);
    assert!(FILE_ATTRIBUTE_REPARSE_POINT.0 == attr::REPARSE_POINT);
    assert!(FILE_ATTRIBUTE_OFFLINE.0 == attr::OFFLINE);
    assert!(FILE_ATTRIBUTE_RECALL_ON_OPEN.0 == attr::RECALL_ON_OPEN);
    assert!(FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0 == attr::RECALL_ON_DATA_ACCESS);
};

// fs::status constants.
const _: () = {
    assert!(STATUS_NO_SUCH_FILE.0 == st::STATUS_NO_SUCH_FILE);
    assert!(STATUS_ACCESS_DENIED.0 == st::STATUS_ACCESS_DENIED);
    assert!(STATUS_OBJECT_NAME_INVALID.0 == st::STATUS_OBJECT_NAME_INVALID);
    assert!(STATUS_OBJECT_NAME_NOT_FOUND.0 == st::STATUS_OBJECT_NAME_NOT_FOUND);
    assert!(STATUS_OBJECT_NAME_COLLISION.0 == st::STATUS_OBJECT_NAME_COLLISION);
    assert!(STATUS_OBJECT_PATH_NOT_FOUND.0 == st::STATUS_OBJECT_PATH_NOT_FOUND);
    assert!(STATUS_OBJECT_PATH_SYNTAX_BAD.0 == st::STATUS_OBJECT_PATH_SYNTAX_BAD);
    assert!(STATUS_SHARING_VIOLATION.0 == st::STATUS_SHARING_VIOLATION);
    assert!(STATUS_FILE_LOCK_CONFLICT.0 == st::STATUS_FILE_LOCK_CONFLICT);
    assert!(STATUS_LOCK_NOT_GRANTED.0 == st::STATUS_LOCK_NOT_GRANTED);
    assert!(STATUS_DELETE_PENDING.0 == st::STATUS_DELETE_PENDING);
    assert!(STATUS_PRIVILEGE_NOT_HELD.0 == st::STATUS_PRIVILEGE_NOT_HELD);
    assert!(STATUS_FILE_IS_A_DIRECTORY.0 == st::STATUS_FILE_IS_A_DIRECTORY);
    assert!(STATUS_DIRECTORY_NOT_EMPTY.0 == st::STATUS_DIRECTORY_NOT_EMPTY);
    assert!(STATUS_NOT_A_DIRECTORY.0 == st::STATUS_NOT_A_DIRECTORY);
    assert!(STATUS_CANNOT_DELETE.0 == st::STATUS_CANNOT_DELETE);
    assert!(STATUS_USER_MAPPED_FILE.0 == st::STATUS_USER_MAPPED_FILE);

    assert!(ERROR_INVALID_FUNCTION.0 == st::ERROR_INVALID_FUNCTION);
    assert!(ERROR_FILE_NOT_FOUND.0 == st::ERROR_FILE_NOT_FOUND);
    assert!(ERROR_PATH_NOT_FOUND.0 == st::ERROR_PATH_NOT_FOUND);
    assert!(ERROR_ACCESS_DENIED.0 == st::ERROR_ACCESS_DENIED);
    assert!(ERROR_NO_MORE_FILES.0 == st::ERROR_NO_MORE_FILES);
    assert!(ERROR_SHARING_VIOLATION.0 == st::ERROR_SHARING_VIOLATION);
    assert!(ERROR_LOCK_VIOLATION.0 == st::ERROR_LOCK_VIOLATION);
    assert!(ERROR_NOT_SUPPORTED.0 == st::ERROR_NOT_SUPPORTED);
    assert!(ERROR_INVALID_PARAMETER.0 == st::ERROR_INVALID_PARAMETER);
    assert!(ERROR_INSUFFICIENT_BUFFER.0 == st::ERROR_INSUFFICIENT_BUFFER);
    assert!(ERROR_DIR_NOT_EMPTY.0 == st::ERROR_DIR_NOT_EMPTY);
    assert!(ERROR_DELETE_PENDING.0 == st::ERROR_DELETE_PENDING);
    assert!(ERROR_USER_MAPPED_FILE.0 == st::ERROR_USER_MAPPED_FILE);
};

// NtCreateFile option/disposition values quoted in the M1 design section 3.1.
const _: () = {
    assert!(FILE_OPEN.0 == 1);
    assert!(FILE_DIRECTORY_FILE.0 == 0x1);
    assert!(FILE_SYNCHRONOUS_IO_NONALERT.0 == 0x20);
    assert!(FILE_NON_DIRECTORY_FILE.0 == 0x40);
    assert!(FILE_OPEN_FOR_BACKUP_INTENT.0 == 0x4000);
    assert!(FILE_OPEN_REPARSE_POINT.0 == 0x20_0000);
};
