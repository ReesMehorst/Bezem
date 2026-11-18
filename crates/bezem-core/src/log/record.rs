// SPDX-License-Identifier: MIT

//! Log record types and path encoding.
//!
//! Ownership: work package **C**. On disk every line is `{"t":"<RFC 3339 UTC>", ...record}`
//! via [`Stamped`]. Paths are written without `\\?\`; lone surrogates become U+FFFD in `path`
//! and the lossless UTF-16LE hex goes to `path_hex`.

use crate::types::SkipReason;

/// The first record of every log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RunHeader {
    pub run_id: String,
    pub version: &'static str,
    /// `"permanent"` in M1.
    pub mode: &'static str,
    pub privilege: String,
    pub scan_id: String,
}

/// One log record. The `action` record type is M2+.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogRecord {
    Run(RunHeader),
    Deleted {
        cleaner: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path_hex: Option<String>,
        bytes: u64,
        /// `"permanent"` in M1.
        how: &'static str,
    },
    Skipped {
        cleaner: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path_hex: Option<String>,
        reason: SkipReason,
        code: Option<String>,
    },
    CleanerDone {
        cleaner: String,
        files: u64,
        bytes: u64,
        skipped: u64,
    },
    /// Error level; beyond A:1199 (design deviation 9).
    SafetyViolation {
        cleaner: String,
        kind: String,
        path: String,
        detail: String,
    },
    Summary {
        files: u64,
        bytes: u64,
        skipped: u64,
        cancelled: bool,
        safety_violations: u32,
        duration_ms: u64,
    },
}

/// A record with its timestamp, as written on disk.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Stamped<'a> {
    pub t: &'a str,
    #[serde(flatten)]
    pub rec: &'a LogRecord,
}

/// Encodes a UTF-16 path for the log: `(path, path_hex)`. `path_hex` is `Some` (UTF-16LE,
/// lower-case hex) only when the input is not valid UTF-16.
pub fn encode_path(wide: &[u16]) -> (String, Option<String>) {
    match String::from_utf16(wide) {
        Ok(s) => (s, None),
        Err(_) => {
            let mut hex = String::with_capacity(wide.len() * 4);
            for b in wide.iter().flat_map(|u| u.to_le_bytes()) {
                hex.push_str(&format!("{b:02x}"));
            }
            (String::from_utf16_lossy(wide), Some(hex))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_encoding() {
        let ok: Vec<u16> = "C:\\a".encode_utf16().collect();
        assert_eq!(encode_path(&ok), ("C:\\a".to_owned(), None));
        let lone = [u16::from(b'a'), 0xD800];
        let (p, hex) = encode_path(&lone);
        assert_eq!(p, "a\u{FFFD}");
        assert_eq!(hex.as_deref(), Some("610000d8"));
    }

    #[test]
    fn stamped_shape() {
        let rec = LogRecord::Deleted {
            cleaner: "windows.user_temp".into(),
            path: "C:\\x.tmp".into(),
            path_hex: None,
            bytes: 3,
            how: "permanent",
        };
        let s = serde_json::to_string(&Stamped { t: "T", rec: &rec }).unwrap();
        assert_eq!(
            s,
            r#"{"t":"T","type":"deleted","cleaner":"windows.user_temp","path":"C:\\x.tmp","bytes":3,"how":"permanent"}"#
        );
    }
}
