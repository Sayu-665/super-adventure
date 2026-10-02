//! Errors returned when a pack cannot be opened at all.
//!
//! Everything that can go wrong *inside* a pack (bad properties, missing includes,
//! ambiguous options, ...) is reported as [`sb_core::Diagnostics`] instead.

use std::path::PathBuf;

/// A fatal error opening a shader pack.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// The path could not be read.
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The file is not a readable zip archive.
    #[error("cannot read zip archive {path}: {message}")]
    Zip { path: PathBuf, message: String },
    /// The directory or archive contains no `shaders/` root and does not look like one.
    #[error("{path} is not a shader pack: no `shaders/` directory found")]
    NotAShaderPack { path: PathBuf },
}
