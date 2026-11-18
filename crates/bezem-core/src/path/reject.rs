// SPDX-License-Identifier: MIT

//! [`PathReject`]: why a path, root spec or name component was refused.
//!
//! Ownership: work package **B**.

/// Why a path, root spec or name component was refused. The first failing rule wins
/// (M1 design section 4.1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathReject {
    #[error("empty path")]
    Empty,
    #[error("root spec must start with a %TOKEN%")]
    NoLeadingToken,
    #[error("rooted path without a drive")]
    RootedNoDrive,
    #[error("literal absolute paths are not accepted as roots")]
    LiteralAbsolute,
    #[error("drive root")]
    DriveRoot,
    #[error("drive-relative path")]
    DriveRelative,
    #[error("device or namespace prefix")]
    DevicePrefix,
    #[error("UNC path")]
    Unc,
    #[error("unknown token %{0}%")]
    UnknownToken(String),
    #[error("'%' after the leading token")]
    EmbeddedPercent,
    #[error("'..' component")]
    ParentTraversal,
    #[error("'.' component")]
    CurrentDirComponent,
    #[error("empty component")]
    EmptyComponent,
    #[error("invalid character U+{0:04X}")]
    InvalidChar(u16),
    #[error("alternate data stream or ':' in a component")]
    AlternateStream,
    #[error("component ends with a dot or space")]
    TrailingDotOrSpace,
    #[error("8.3 short name")]
    ShortName,
    #[error("reserved device name")]
    ReservedDeviceName,
    #[error("wildcard in a path component")]
    Wildcard,
    #[error("component longer than 255 UTF-16 units")]
    ComponentTooLong,
    #[error("path too long")]
    PathTooLong,
    #[error("NUL character")]
    NulChar,
    #[error("final path is not of the form \\\\?\\X:\\...")]
    BadFinalPathForm,
    #[error("final path is a UNC path")]
    UncFinal,
    #[error("lone surrogate in a template")]
    LoneSurrogateInTemplate,
}
