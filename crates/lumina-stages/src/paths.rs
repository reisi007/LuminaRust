//! Path, directory-walk and catalogue helpers shared by the path-based
//! commands — MCP-PARITY-B.
//!
//! These are the *unchanged* helpers of `crates/lumina-cli/src/main.rs`
//! (`require_sidecar`, `collect_target_sidecars`, `collect_sidecars`,
//! `collect_tree_files` / `collect_tree_files_inner`, `move_file_cross_volume`
//! and `load_smart_catalog`), moved verbatim so that `lumina-cli` **and**
//! `lumina-mcp` resolve the same target list, walk the same tree with the same
//! cycle guard, refuse the same overwrites and read the same catalogue.
//!
//! Nothing here is a decision: it is the I/O substrate the two path-based
//! commands (`collections`, `relocate`) and the read-only `smart-collections`
//! stand on. Only the error type changed (`StageError` instead of `CliError`);
//! `lumina-cli` converts it transparently, so its stderr text and exit codes
//! are unchanged.

use crate::error::StageError;
use lumina_sidecar::{
    load_sidecar, sidecar_path_for, validate_smart_collection_def, SidecarDocument,
    SmartCollectionDef, SMART_COLLECTION_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Requires the sidecar of `input`, failing loudly when none exists instead
/// of silently operating on default contents.
pub fn require_sidecar(input: &Path) -> Result<(PathBuf, SidecarDocument), StageError> {
    let path = sidecar_path_for(input);
    match load_sidecar(&path) {
        Ok(document) => Ok((path, document)),
        Err(lumina_sidecar::SidecarError::Missing(_)) => Err(StageError::Message(format!(
            "no sidecar for `{}`; run `import` first",
            input.display()
        ))),
        Err(error) => Err(error.into()),
    }
}

/// Resolves `input` of the multi-sidecar commands to the sidecar files to
/// process, in deterministic (sorted) order: a `*.lumina.json` file is used
/// directly, any other file maps to its sidecar path, and a directory is
/// scanned recursively (symlink-/loop-safe, same walk as `reindex`).
pub fn collect_target_sidecars(input: &Path) -> Result<Vec<PathBuf>, StageError> {
    if input.is_file() {
        if input.to_string_lossy().ends_with(".lumina.json") {
            return Ok(vec![input.to_path_buf()]);
        }
        return Ok(vec![sidecar_path_for(input)]);
    }
    let mut files = Vec::new();
    collect_sidecars(input, &mut files)?;
    files.sort();
    Ok(files)
}

/// Shared recursive directory walk behind `collect_images` and
/// `collect_sidecars` (REVIEW-CLI-N5 / REVIEW-CLI-FOLLOWUP-1): the visited set
/// holds canonical directory identities so filesystem cycles (symlink loops,
/// bind mounts) terminate instead of overflowing the stack, directory symlinks
/// are never followed and every directory level is walked in deterministic
/// (sorted) order.
pub fn collect_tree_files<F>(
    path: &Path,
    output: &mut Vec<PathBuf>,
    keep: F,
) -> Result<(), StageError>
where
    F: Fn(&Path) -> bool,
{
    let mut visited = BTreeSet::new();
    collect_tree_files_inner(path, output, &mut visited, &keep)
}

fn collect_tree_files_inner<F>(
    path: &Path,
    output: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
    keep: &F,
) -> Result<(), StageError>
where
    F: Fn(&Path) -> bool,
{
    let identity = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(identity) {
        return Ok(());
    }
    let mut entries: Vec<std::fs::DirEntry> = fs::read_dir(path)
        .map_err(|error| StageError::io(path, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| StageError::io(path, error))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let p = entry.path();
        // `entry.file_type()` never follows symlinks: a symlinked directory is
        // never recursed into, which removes symlink loops by construction.
        if entry
            .file_type()
            .map_err(|error| StageError::io(&p, error))?
            .is_dir()
        {
            collect_tree_files_inner(&p, output, visited, keep)?;
        } else if keep(&p) && p.is_file() {
            output.push(p);
        }
    }
    Ok(())
}

/// Collects every `*.lumina.json` under `path` (REVIEW-CLI-FOLLOWUP-1): the
/// same symlink-/loop-safe walk as `collect_images`, so a scan cannot cycle
/// through directory symlinks. Regular-file-only collection also keeps
/// dangling or special (FIFO) entries out of the scan.
pub fn collect_sidecars(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), StageError> {
    collect_tree_files(path, output, |p| {
        p.to_string_lossy().ends_with(".lumina.json")
    })
}

/// Move one file to `target`, tolerating a cross-filesystem move: `rename`
/// fails with `CrossesDevices` (EXDEV) when source and target live on
/// different volumes, so fall back to copy + remove. Loud on error; a failed
/// source removal cleans up the copied target again (the source still exists,
/// so no data is lost).
pub fn move_file_cross_volume(source: &Path, target: &Path) -> std::io::Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(source, target)?;
            if let Err(remove_error) = fs::remove_file(source) {
                let _ = fs::remove_file(target);
                return Err(remove_error);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Portable smart-collection catalog file (G-15 META-MVP, Slice 2). The file
/// holds versioned rule data only — never absolute paths — and is validated
/// with the same rules as the sidecar slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartCatalogFile {
    pub format: String,
    pub version: u8,
    pub collections: Vec<SmartCollectionDef>,
}

/// Loads and validates a smart-collection catalog file. Every deviation
/// (unreadable file, invalid JSON, wrong format/version marker, invalid
/// definition) is a loud error; there is no silent fallback to an empty
/// catalog.
pub fn load_smart_catalog(path: &Path) -> Result<Vec<SmartCollectionDef>, StageError> {
    let json = fs::read_to_string(path).map_err(|error| StageError::io(path, error))?;
    let catalog: SmartCatalogFile = serde_json::from_str(&json).map_err(|error| {
        StageError::Message(format!(
            "invalid smart-collection catalog `{}`: {error}",
            path.display()
        ))
    })?;
    if catalog.format != "lumina-smart-catalog" {
        return Err(StageError::Message(format!(
            "invalid smart-collection catalog `{}`: expected format \"lumina-smart-catalog\", got \"{}\"",
            path.display(),
            catalog.format
        )));
    }
    if catalog.version != SMART_COLLECTION_VERSION {
        return Err(StageError::Message(format!(
            "invalid smart-collection catalog `{}`: unsupported version {}, expected {SMART_COLLECTION_VERSION}",
            path.display(),
            catalog.version
        )));
    }
    for def in &catalog.collections {
        validate_smart_collection_def(def).map_err(|error| {
            StageError::Message(format!(
                "invalid smart-collection definition `{}` in catalog `{}`: {error}",
                def.id,
                path.display()
            ))
        })?;
    }
    Ok(catalog.collections)
}
