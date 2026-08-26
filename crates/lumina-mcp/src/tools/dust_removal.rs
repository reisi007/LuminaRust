//! `lumina_dust_removal` — persist a repair region as a recipe source action
//! (expanded MCP scope; wraps the existing F-042-N1 CLI `dust-removal`).
//!
//! Mirrors the reviewed CLI ordering (REVIEW-CLI-N2): the sidecar and the
//! target virtual copy are resolved and the region/replacement definitions are
//! FULLY validated BEFORE anything is appended to the `.lumina.zdata` bundle,
//! so a rejected call leaves no orphaned artifact bytes. The recipe stores
//! only a RELATIVE bundle reference, never an absolute path. The original
//! image is never modified.

use crate::error::McpError;
use crate::util::{get_str, read_and_decode};
use crate::Server;
use lumina_core::{ImageFrame, MaskPlane, RenderContext};
use lumina_sidecar::{
    append_repair_region, load_sidecar, load_zdata, save_sidecar, sidecar_path_for, zdata_path_for,
    RepairRegionArtifact, SourceActionArtifactRef, SourceActionKind, SourceActionSpec,
    SOURCE_ACTION_VERSION,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const NAME: &str = "lumina_dust_removal";
pub const DESCRIPTION: &str = "Record a dust-removal (or AI-replacement) repair region for a \
source image: appends the region artifact to the .lumina.zdata bundle and references it as a \
source action in a virtual copy's recipe. Non-destructive: the original is never modified. \
Region pixels >= 32768 are replaced by the replacement image at source resolution. Optional \
render_out verifies the effect headlessly.";

/// Repair-region definition consumed by this tool (same schema as the CLI's
/// `RepairRegionInput`). `region_values` are little-endian `u16`
/// (0..=u16::MAX); pixels `>= 32768` are replaced by the corresponding
/// `replacement_path` RGBA8 pixel. Region and replacement MUST share the
/// source frame's dimensions.
#[derive(Debug, Deserialize)]
struct RepairRegionInput {
    id: String,
    #[serde(default)]
    kind: Option<String>,
    region_width: u32,
    region_height: u32,
    region_values: Vec<u16>,
    replacement_path: PathBuf,
}

impl RepairRegionInput {
    /// Maps the agent-friendly kind names onto the persisted enum. Accepted:
    /// `"dust-removal"` (default), `"ai-replacement"`, plus the serde-native
    /// lowercase forms (`dustremoval`, `aireplacement`).
    fn kind(&self) -> Result<SourceActionKind, McpError> {
        match self.kind.as_deref() {
            None | Some("dust-removal") | Some("dustremoval") => Ok(SourceActionKind::DustRemoval),
            Some("ai-replacement") | Some("aireplacement") => Ok(SourceActionKind::AiReplacement),
            Some(other) => Err(McpError::InvalidParams(format!(
                "repair_region.kind `{other}` is not one of `dust-removal`, `ai-replacement`"
            ))),
        }
    }
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "input": { "type": "string", "description": "Path to the source image (sidecar must exist; run lumina_import first)." },
            "repair_region": {
                "type": "object",
                "description": "Repair-region definition.",
                "properties": {
                    "id": { "type": "string", "description": "Stable artifact id inside the bundle." },
                    "kind": {
                        "type": "string",
                        "enum": ["dust-removal", "ai-replacement"],
                        "description": "Source-action kind (default: dust-removal)."
                    },
                    "region_width": { "type": "integer", "minimum": 1, "description": "Region width; must equal the source frame width." },
                    "region_height": { "type": "integer", "minimum": 1, "description": "Region height; must equal the source frame height." },
                    "region_values": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0, "maximum": 65535 },
                        "description": "Row-major u16 mask values (width*height); values >= 32768 mark replaced pixels."
                    },
                    "replacement_path": { "type": "string", "description": "Path to an RGBA8-replaceable image matching the region dimensions." }
                },
                "required": ["id", "region_width", "region_height", "region_values", "replacement_path"]
            },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy id or name receiving the source action (default: the first copy)."
            },
            "render_out": {
                "type": "string",
                "description": "Optional path to render the frame with the action applied for verification. Must not resolve onto the source or its bundles."
            }
        },
        "required": ["input", "repair_region"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let input_str = get_str(args, "input")?;
    let input = Path::new(input_str);
    if !input.exists() {
        return Err(McpError::FileNotFound(input_str.to_string()));
    }

    // Optional verification render target is guarded BEFORE any mutation so a
    // protected output can never be reached by a partially applied run.
    let render_out = args
        .get("render_out")
        .and_then(|value| value.as_str())
        .map(PathBuf::from);
    if let Some(output) = &render_out {
        crate::util::reject_protected_target(input, output)?;
    }

    let (_bytes, frame, _raw) = read_and_decode(input)?;

    let definition: RepairRegionInput = serde_json::from_value(
        args.get("repair_region")
            .cloned()
            .ok_or_else(|| McpError::InvalidParams("missing `repair_region` object".into()))?,
    )
    .map_err(|error| McpError::InvalidParams(format!("invalid repair_region: {error}")))?;
    let kind = definition.kind()?;

    // Replacement must decode and match the declared region dimensions.
    let replacement_bytes = std::fs::read(&definition.replacement_path).map_err(|error| {
        McpError::FileNotFound(format!("{:?}: {error}", definition.replacement_path))
    })?;
    let replacement_frame = ImageFrame::decode(&replacement_bytes).map_err(|error| {
        McpError::Decode(format!("could not decode replacement image: {error}"))
    })?;
    if replacement_frame.width != definition.region_width
        || replacement_frame.height != definition.region_height
    {
        return Err(McpError::InvalidParams(format!(
            "replacement image {}x{} does not match region {}x{}",
            replacement_frame.width,
            replacement_frame.height,
            definition.region_width,
            definition.region_height
        )));
    }
    let region = RepairRegionArtifact {
        id: definition.id.clone(),
        width: definition.region_width,
        height: definition.region_height,
        region: definition.region_values.clone(),
        replacement: replacement_frame.pixels.clone(),
    };
    region
        .validate()
        .map_err(|error| McpError::InvalidParams(format!("invalid repair region: {error}")))?;
    // Source actions apply at source resolution (identical to the CLI).
    if region.width != frame.width || region.height != frame.height {
        return Err(McpError::InvalidParams(format!(
            "repair region {}x{} does not match source frame {}x{}; source actions apply at source resolution",
            region.width, region.height, frame.width, frame.height
        )));
    }

    // REVIEW-CLI-N2 parity: validate the sidecar and resolve the target copy
    // BEFORE anything is appended to the `.lumina.zdata` bundle — appending
    // first would leave orphaned artifact bytes behind on failure. A missing
    // sidecar is a loud error pointing at lumina_import/lumina_load.
    let sidecar_path = sidecar_path_for(input);
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(McpError::Sidecar(format!(
                "no sidecar for `{input_str}`; run lumina_import first"
            )));
        }
        Err(error) => return Err(McpError::Sidecar(format!("{error}"))),
    };
    let copy_index = match args.get("virtual_copy").and_then(|value| value.as_str()) {
        Some(requested) => document
            .virtual_copies
            .iter()
            .position(|copy| copy.id == requested || copy.name == requested)
            .ok_or_else(|| McpError::UnknownCopy(requested.to_string()))?,
        None => 0,
    };

    // Persist the artifact bytes into the portable bundle next to the source;
    // the recipe stores only the RELATIVE file name.
    let zdata_path = zdata_path_for(input);
    let relative_path = zdata_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repair.zdata")
        .to_string();
    let checksum = region.checksum();
    append_repair_region(&zdata_path, region).map_err(|error| {
        McpError::Sidecar(format!("could not write repair-region bundle: {error}"))
    })?;

    let spec = SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind,
        artifact: SourceActionArtifactRef {
            id: definition.id.clone(),
            relative_path: relative_path.clone(),
            checksum: checksum.clone(),
        },
    };
    document.virtual_copies[copy_index]
        .recipe
        .source_actions
        .push(spec);
    document
        .validate()
        .map_err(|error| McpError::Sidecar(format!("{error}")))?;
    save_sidecar(&sidecar_path, &document)
        .map_err(|error| McpError::Sidecar(format!("{error}")))?;

    // Optional headless verification render with the action applied.
    if let Some(output) = &render_out {
        let source_actions =
            resolve_source_actions(&document.virtual_copies[copy_index].recipe, &zdata_path)?;
        // Same manual-model context as the CLI's verification render: no
        // camera WB, no masks, no Lensfun — the point is to make the recorded
        // source action visible end-to-end.
        let rendered = lumina_core::render_frame(
            &frame,
            &RenderContext {
                recipe: &document.virtual_copies[copy_index].recipe,
                camera_white_balance: None,
                source_actions: &source_actions,
                masks: None,
                lensfun: None,
            },
        )
        .map_err(crate::error::map_core_error)?;
        let format = output_format_for(output)?;
        let bytes = rendered
            .frame
            .encode(format)
            .map_err(crate::error::map_core_error)?;
        crate::util::write_output_guarded(input, output, &bytes)?;
    }

    Ok(json!({
        "ok": true,
        "input": input_str,
        "virtual_copy": document.virtual_copies[copy_index].id,
        "artifact_id": definition.id,
        "bundle": relative_path,
        "checksum": checksum,
    }))
}

/// Resolves the recipe's persisted source actions from the `.lumina.zdata`
/// bundle. A missing bundle/artifact or a checksum mismatch against the recipe
/// reference is a loud error — no silent fallback (parity with the CLI).
fn resolve_source_actions(
    recipe: &lumina_sidecar::EditRecipe,
    zdata_path: &Path,
) -> Result<Vec<lumina_core::SourceActionArtifact>, McpError> {
    if recipe.source_actions.is_empty() {
        return Ok(Vec::new());
    }
    let container = load_zdata(zdata_path).map_err(|error| {
        McpError::Render(format!(
            "could not read source-action bundle `{}`: {error}",
            zdata_path.display()
        ))
    })?;
    let mut artifacts = Vec::with_capacity(recipe.source_actions.len());
    for spec in &recipe.source_actions {
        let region = container
            .repair_region(&spec.artifact.id)
            .map_err(|error| {
                McpError::Render(format!(
                    "source action `{}` artifact missing from bundle: {error}",
                    spec.artifact.id
                ))
            })?;
        if region.checksum() != spec.artifact.checksum {
            return Err(McpError::Render(format!(
                "source action `{}` checksum mismatch: recipe and bundle disagree (stale or corrupted artifact)",
                spec.artifact.id
            )));
        }
        let plane =
            MaskPlane::new(region.width, region.height, region.region).map_err(|error| {
                McpError::Render(format!(
                    "source action `{}` has an invalid region plane: {error}",
                    spec.artifact.id
                ))
            })?;
        let replacement = ImageFrame::new(region.width, region.height, region.replacement)
            .map_err(|error| {
                McpError::Render(format!(
                    "source action `{}` has an invalid replacement image: {error}",
                    spec.artifact.id
                ))
            })?;
        artifacts.push(lumina_core::SourceActionArtifact {
            region: plane,
            replacement,
        });
    }
    Ok(artifacts)
}

fn output_format_for(path: &Path) -> Result<lumina_core::ImageFileFormat, McpError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    lumina_core::ImageFileFormat::from_extension(&extension).ok_or_else(|| {
        McpError::UnsupportedFormat(format!(
            "unsupported render_out extension `.{extension}`; use png, jpg/jpeg or webp"
        ))
    })
}
