//! `relocate` — move one image together with its sidecar companions
//! (`lumina relocate` / `lumina_relocate`) — MCP-PARITY-B.
//!
//! Path-based (one call = one pair of paths, no `image_id`) like the existing
//! bulk tools.
//!
//! The **collision** and the **sidecar move** are the two things that must not
//! be left out, and both are here unchanged from
//! `crates/lumina-cli/src/main.rs` (`fn relocate`):
//!
//! * an existing target image aborts before anything moves, and so does an
//!   existing target *companion* (`.lumina.json` / `.lumina.zdata`) — without
//!   that second check a move could leave an orphaned recipe beside the old
//!   image while the new path has none, i.e. a path without a recipe;
//! * the companions are derived from the **target** image path and moved with
//!   it, so a rename keeps the recipe attached to the new name;
//! * a missing source, a non-directory target parent and a failed companion
//!   move are all loud, and the companion error names the step reached (the
//!   image may already sit at the target — no silent half state, no data loss
//!   by overwrite).

use crate::error::StageError;
use crate::paths::move_file_cross_volume;
use crate::report::{BulkReport, BulkRun};
use log::info;
use lumina_sidecar::{sidecar_path_for, zdata_path_for};
use serde_json::json;
use std::path::Path;

/// Transport-neutral `relocate` request. Every field maps 1:1 onto one CLI flag.
#[derive(Debug, Clone, Default)]
pub struct RelocateRequest {
    /// Source image to move (must exist).
    pub from: String,
    /// Destination image path (must not exist; parent must be a directory).
    pub to: String,
}

impl RelocateRequest {
    /// The two companion moves, derived from the **target** image path.
    fn companions(&self) -> [(std::path::PathBuf, std::path::PathBuf); 2] {
        [
            (
                sidecar_path_for(Path::new(&self.from)),
                sidecar_path_for(Path::new(&self.to)),
            ),
            (
                zdata_path_for(Path::new(&self.from)),
                zdata_path_for(Path::new(&self.to)),
            ),
        ]
    }
}

/// Runs one `relocate` call: validate the whole move, move the image, then move
/// every present companion, then report.
///
/// This command moves files; it never writes a sidecar, so it has no
/// compare-and-swap side (the moved recipe travels with the image, and the
/// pre-flight checks above are what keep it unique).
pub fn run(request: &RelocateRequest) -> Result<BulkRun, StageError> {
    let from = Path::new(&request.from);
    let to = Path::new(&request.to);
    if !from.is_file() {
        return Err(StageError::Message(format!(
            "relocate: source `{}` does not exist",
            request.from
        )));
    }
    if to.exists() {
        return Err(StageError::Message(format!(
            "relocate: target `{}` already exists; refusing to overwrite",
            request.to
        )));
    }
    if let Some(parent) = to.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        if !parent.is_dir() {
            return Err(StageError::Message(format!(
                "relocate: target parent `{}` is no directory",
                parent.display()
            )));
        }
    }
    let moves = request.companions();
    for (source, target) in &moves {
        if source.is_file() && target.exists() {
            return Err(StageError::Message(format!(
                "relocate: companion target `{}` already exists; refusing to overwrite",
                target.display()
            )));
        }
    }
    move_file_cross_volume(from, to).map_err(|error| StageError::io(from, error))?;
    info!("relocate: image `{}` -> `{}`", request.from, request.to);
    let mut actions = vec!["image".to_string()];
    for (source, target) in &moves {
        if source.is_file() {
            move_file_cross_volume(source, target).map_err(|error| {
                StageError::Message(format!(
                    "relocate: image moved to `{}` but companion `{}` failed: {error}",
                    request.to,
                    source.display()
                ))
            })?;
            info!(
                "relocate: companion `{}` -> `{}`",
                source.display(),
                target.display()
            );
            let kind = if source.to_string_lossy().ends_with(".lumina.json") {
                "sidecar"
            } else {
                "bundle"
            };
            actions.push(kind.to_string());
        }
    }
    let text = format!("relocated `{}` -> `{}`", request.from, request.to);
    let payload = json!({
        "command": "relocate",
        "from": request.from,
        "to": request.to,
        "status": "ok",
    });
    info!("{text}");
    Ok(BulkRun {
        report: BulkReport::new("relocate", actions, Some(payload), vec![text], true),
        sidecar_path: sidecar_path_for(to),
        document: None,
    })
}
