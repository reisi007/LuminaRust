//! Non-destructive output guard (F-101-F1 bulk tools; since Review R2 also
//! `lumina_save`).
//!
//! Extracted from `util.rs` by MCP-MASK-APPLY so the mask-resolution wiring
//! in the render choke point does not grow the `util.rs` baseline. The guard
//! refuses any write whose target resolves onto the source image or one of
//! its Lumina bundle files (`<source>.lumina.json`, `<source>.lumina.zdata`),
//! so a render/export can never clobber the original or its sidecar.

use crate::error::McpError;
use std::fs;
use std::path::{Path, PathBuf};

/// Refuses when `target` resolves onto the source image or one of its Lumina
/// bundle files. Two identity checks are combined (defense in depth):
///
/// * **Candidate path equality** — canonical aliases including
///   not-yet-existing targets and symlinks (canonicalization follows them),
///   resolved against the canonical parent for missing paths. This mirrors
///   the CLI's `reject_protected_output`.
/// * **`(dev, inode)` identity** (Unix) — catches hard links between distinct
///   directory entries, which canonicalization cannot see.
///
/// Called before any mutation by `lumina_save`, `lumina_dust_removal` and
/// every bulk write.
pub fn reject_protected_target(source: &Path, target: &Path) -> Result<(), McpError> {
    let target_resolved = resolve_candidate(target).map_err(|error| {
        McpError::Encode(format!(
            "could not resolve output path `{}`: {error}",
            target.display()
        ))
    })?;
    let protected: Vec<(&str, PathBuf)> = vec![
        ("source image", source.to_path_buf()),
        ("sidecar", lumina_sidecar::sidecar_path_for(source)),
        (
            "mask/source-action bundle",
            lumina_sidecar::zdata_path_for(source),
        ),
    ];
    for (kind, path) in protected {
        // Both sides use the candidate convention (existing paths are
        // canonicalized; missing ones resolve against their canonical parent),
        // mirroring the CLI's `reject_protected_output`. A plain
        // canonicalize-the-first-argument comparison would fail on the
        // not-yet-existing sidecar/zdata candidates.
        let path_resolved = resolve_candidate(&path).map_err(|error| {
            McpError::Encode(format!("could not resolve `{}`: {error}", path.display()))
        })?;
        if path_resolved == target_resolved {
            return Err(McpError::Encode(format!(
                "output `{}` would overwrite the {kind} `{}`; refusing (non-destructive guarantee)",
                target.display(),
                path.display()
            )));
        }
        // Hard-link alias: the same underlying file under a different
        // directory entry. A rename over the alias would not touch the
        // protected file's own entry, but writing through it must still be
        // refused loudly so a future non-rename write path can never
        // silently clobber the bundle (REVIEW R2-MCP-02 defense in depth).
        #[cfg(unix)]
        if paths_are_same_file(&path, target).unwrap_or(false) {
            return Err(McpError::Encode(format!(
                "output `{}` is a hard-link alias of the {kind} `{}`; \
                 refusing (non-destructive guarantee)",
                target.display(),
                path.display()
            )));
        }
    }
    Ok(())
}

/// Unix: true when both paths refer to the same underlying file via
/// `(dev, inode)` identity — catches hard links between distinct paths.
#[cfg(unix)]
fn paths_are_same_file(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if !(a.exists() && b.exists()) {
        return Ok(false);
    }
    let (meta_a, meta_b) = (fs::metadata(a)?, fs::metadata(b)?);
    Ok(meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino())
}

/// Non-unix fallback: no portable inode identity exists; only candidate path
/// equality (checked separately above) applies.
#[cfg(not(unix))]
fn paths_are_same_file(_a: &Path, _b: &Path) -> std::io::Result<bool> {
    Ok(false)
}

/// Resolves `path` to a comparable identity (CLI `resolve_candidate` parity):
/// existing paths are canonicalized, missing ones are resolved against their
/// canonical parent directory.
fn resolve_candidate(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        fs::canonicalize(path)
    } else {
        let parent = fs::canonicalize(path.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(parent.join(path.file_name().unwrap_or_default()))
    }
}
