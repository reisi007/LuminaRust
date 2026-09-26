//! Render-context substrate shared by the artefact commands `generative` and
//! `regenerate` — MCP-PARITY-B.
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs`
//! (`sanitize_camera_white_balance`, `zdata_mask_tile_id`,
//! `load_persisted_mask_planes`, `resolve_source_actions`). They decide what a
//! render context contains — the sanitised As-Shot white balance, the persisted
//! mask planes and the source-action artifacts — and every one of them is
//! load-bearing for the bytes `regenerate --module matching` derives. A second
//! copy in the MCP layer would therefore be a silent divergence of the derived
//! `matched_exposure`, which is exactly what the byte-identity requirement
//! forbids.

use crate::error::StageError;
use lumina_core::{ImageFrame, MaskPlane, SourceActionArtifact};
use lumina_sidecar::{
    load_validated_source_action_bundle, load_zdata, EditRecipe, MaskOperation, SidecarDocument,
};
use std::collections::BTreeMap;
use std::path::Path;

/// Build a Lensfun lens corrector from decoded RAW metadata (EXIF) for use as
/// `RenderContext.lensfun`.
///
/// Returns `Some(wb)` for a usable As-Shot white balance, `None` if any gain is
/// NaN/infinite or non-positive. Mirrors the lumina-gui load/background-decode
/// sanitisation (R2-WB) so CLI and GUI degrade identically instead of aborting
/// the render on a corrupt CR3 `cam_mul`.
pub fn sanitize_camera_white_balance(wb: [f32; 4]) -> Option<[f32; 4]> {
    if wb.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        None
    } else {
        Some(wb)
    }
}

/// Composite zdata record id for a persisted mask plane (REVIEW-CLI-N1).
///
/// Tiles inside the `.lumina.zdata` bundle are stored under the composite
/// record id `<copy_id>/<mask_id>` so two virtual copies may carry same-named
/// masks (`subject`) without silently sharing one matte. Field order and the
/// `/` separator are normative: `lumina-gui` must adopt this exact convention
/// when it reads/writes mask tiles.
pub fn zdata_mask_tile_id(copy_id: &str, mask_id: &str) -> String {
    format!("{copy_id}/{mask_id}")
}

/// Loads every persisted source-mask plane from the optional `.lumina.zdata`
/// bundle, keyed by `(copy_id, mask_id)` (REVIEW-CLI-N1). A MISSING bundle
/// yields an empty map without any warning (nothing was persisted); missing
/// per-key tiles are decided by the F-051 decision layer in lumina-core
/// (cache, re-inference or a loud error — never a silent fallback).
///
/// R2-CLI-05: a bundle that EXISTS but cannot be read (truncated, malformed,
/// unsupported version, checksum mismatch) is no longer treated silently like
/// a missing bundle — that masked data loss as an ordinary "missing mask"
/// situation. The load failure surfaces as an explicit
/// "unreadable or corrupt" warning on stderr AND in `warnings_out` (the same
/// channel the render's mask warnings travel through). Per-tile lookups after
/// a clean load need no extra corruption handling: `load_zdata` already
/// verifies every record checksum up front (REVIEW-SIDECAR-ZDATA-1), so a
/// surviving tile miss is plain absence.
pub fn load_persisted_mask_planes(
    document: &SidecarDocument,
    zdata_path: &Path,
    warnings_out: &mut Vec<String>,
) -> BTreeMap<(String, String), MaskPlane> {
    let mut planes: BTreeMap<(String, String), MaskPlane> = BTreeMap::new();
    if !zdata_path.exists() {
        return planes;
    }
    let container = match load_zdata(zdata_path) {
        Ok(container) => container,
        Err(error) => {
            let warning = format!(
                "mask/source-action bundle `{}` is unreadable or corrupt ({error}); persisted mask planes are treated as missing and will be re-decided by the mask layer",
                zdata_path.display()
            );
            eprintln!("warning: {warning}");
            warnings_out.push(warning);
            return planes;
        }
    };
    for copy in &document.virtual_copies {
        for mask in copy
            .mask_library
            .iter()
            .filter(|m| matches!(m.operation, MaskOperation::Source))
        {
            let Ok(tile) = container.tile(&zdata_mask_tile_id(&copy.id, &mask.id), 0, 0) else {
                continue;
            };
            if let Ok(plane) = MaskPlane::new(tile.width, tile.height, tile.values) {
                planes.insert((copy.id.clone(), mask.id.clone()), plane);
            }
        }
    }
    planes
}

/// Resolves the recipe's source-action artifacts out of the `.lumina.zdata`
/// bundle, verifying every region checksum against the persisted reference.
///
/// A missing record, a checksum mismatch (stale or corrupted artifact) or an
/// invalid plane/replacement is a loud error: the render that would have used
/// it must not silently continue with the unrepaired source.
pub fn resolve_source_actions(
    recipe: &EditRecipe,
    zdata_path: &Path,
) -> Result<Vec<SourceActionArtifact>, StageError> {
    if recipe.source_actions.is_empty() {
        return Ok(Vec::new());
    }
    let container =
        load_validated_source_action_bundle(recipe, zdata_path).map_err(StageError::Message)?;
    let mut artifacts = Vec::with_capacity(recipe.source_actions.len());
    for spec in &recipe.source_actions {
        let region = container
            .repair_region(&spec.artifact.id)
            .map_err(|error| {
                StageError::Message(format!(
                    "source action `{}` artifact missing from bundle: {error}",
                    spec.artifact.id
                ))
            })?;
        if region.checksum() != spec.artifact.checksum {
            return Err(StageError::Message(format!(
                "source action `{}` checksum mismatch: recipe and bundle disagree (stale or corrupted artifact)",
                spec.artifact.id
            )));
        }
        let mask_plane =
            MaskPlane::new(region.width, region.height, region.region).map_err(|error| {
                StageError::Message(format!(
                    "source action `{}` has an invalid region plane: {error}",
                    spec.artifact.id
                ))
            })?;
        let replacement = ImageFrame::new(region.width, region.height, region.replacement)
            .map_err(|error| {
                StageError::Message(format!(
                    "source action `{}` has an invalid replacement image: {error}",
                    spec.artifact.id
                ))
            })?;
        artifacts.push(SourceActionArtifact {
            region: mask_plane,
            replacement,
        });
    }
    Ok(artifacts)
}
