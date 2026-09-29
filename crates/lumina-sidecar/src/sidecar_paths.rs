//! Single source of the on-disk file suffixes of the sidecar bundle
//! (SIDECAR-SUFFIX-CONST).
//!
//! The JSON sidecar and the binary `zdata` bundle are named
//! `<original>.lumina.json` and `<original>.lumina.zdata`. Both suffixes are
//! defined here **once** so every builder and validator derives from one place;
//! a later rename — a product decision, not made here (NAMING-F1) — is a single
//! edit instead of a search across the tree. `meta_preset.rs` follows the same
//! pattern for the third, meta-preset format.

use std::path::{Path, PathBuf};

/// Authoritative file suffix of the JSON sidecar next to an original
/// (`<original>.lumina.json`).
pub const SIDECAR_FILE_SUFFIX: &str = ".lumina.json";

/// Authoritative file suffix of the binary sidecar bundle next to an original
/// (`<original>.lumina.zdata`). Mirrors [`SIDECAR_FILE_SUFFIX`].
pub const ZDATA_FILE_SUFFIX: &str = ".lumina.zdata";

/// Returns the sidecar path immediately next to `source`.
pub fn sidecar_path_for(source: &Path) -> PathBuf {
    let filename = source
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    source.with_file_name(format!("{filename}{SIDECAR_FILE_SUFFIX}"))
}
