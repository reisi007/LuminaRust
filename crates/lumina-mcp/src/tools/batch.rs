//! `lumina_batch` — directory-wide render/export (expanded MCP scope, mirrors
//! `lumina batch` with documented limits).
//!
//! One call = one directory. Every collected image is rendered with the active
//! recipe of its sidecar through the SAME choke point as `lumina_save`
//! ([`crate::util::render_recipe`] → `render_frame`) and written atomically.
//! Documented divergences from the CLI batch (see
//! `feature/platform/mcp-server.md`, „Erweiterter MVP-Scope“): sequential
//! execution, no resume markers, no one-shot mask flags, no presets, and
//! sidecars are never written (a missing sidecar renders the in-memory
//! default recipe, not an implicit materialization).

use crate::error::McpError;
use crate::util::{
    collect_tree_files, encode_with_quality, get_str, parse_bounded_uint, parse_output_format,
    read_and_decode, render_recipe,
};
use crate::Server;
use lumina_core::ImageFileFormat;
use lumina_sidecar::SidecarDocument;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const NAME: &str = "lumina_batch";
pub const DESCRIPTION: &str = "Render every image in a directory (recursively) with its sidecar \
recipe and write atomically into an output directory — one call per directory. Mirrors \
`lumina batch` (sequential MVP: no resume markers; missing sidecars render as the unedited \
default recipe; sidecars are never written). Reports per-item results — branch on `status`. \
Refuses the whole run up front when two inputs map onto the same output file name.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "input": { "type": "string", "description": "Directory to collect images from (recursive)." },
            "output": { "type": "string", "description": "Directory for rendered outputs." },
            "format": {
                "type": "string",
                "enum": ["png", "jpeg", "webp"],
                "description": "Output format (default: png)."
            },
            "quality": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "JPEG/WebP quality, 1..=100 (default: 90)."
            },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy name or id rendered for each image (default: the standard copy)."
            }
        },
        "required": ["input", "output"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let input_str = get_str(args, "input")?;
    let output_str = get_str(args, "output")?;
    let input_dir = Path::new(input_str);
    let output_dir = PathBuf::from(output_str);

    if !input_dir.is_dir() {
        return Err(McpError::FileNotFound(format!(
            "`{input_str}` is not a directory"
        )));
    }

    // Validate arguments before touching the filesystem (fail fast).
    let format_str = args
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("png");
    let format = parse_output_format(format_str)?;
    let quality = parse_bounded_uint(args, "quality", 1, 100)?.unwrap_or(90) as u8;
    let requested_copy = args
        .get("virtual_copy")
        .and_then(|value| value.as_str())
        .map(str::to_owned);

    // Deterministic collection (sorted, cycle-safe walk).
    let mut inputs = Vec::new();
    collect_tree_files(input_dir, &mut inputs, &|path| {
        crate::util::has_batch_image_extension(path)
    })?;

    // REVIEW-CLI-BATCHCOLLIDE-1 parity: name-based targets inside ONE flat
    // output directory can collide (`a/x.arw` and `b/x.png` both write
    // `x.png`). Refuse the whole run before anything is created or written.
    reject_duplicate_batch_targets(&inputs, format.default_extension())?;

    std::fs::create_dir_all(&output_dir)
        .map_err(|error| McpError::Encode(format!("could not create `{output_str}`: {error}")))?;

    let mut results: Vec<Value> = Vec::with_capacity(inputs.len());
    let mut failed = 0usize;
    for source in &inputs {
        match batch_one(
            source,
            &output_dir,
            format,
            quality,
            requested_copy.as_deref(),
        ) {
            Ok(output_path) => results.push(json!({
                "input": source.to_string_lossy(),
                "status": "ok",
                "output": output_path.to_string_lossy(),
            })),
            Err(error) => {
                failed += 1;
                results.push(json!({
                    "input": source.to_string_lossy(),
                    "status": "failed",
                    "error_name": error.name(),
                    "error": error.message(),
                }));
            }
        }
    }

    let succeeded = results.len() - failed;
    Ok(json!({
        "status": if failed == 0 { "ok" } else { "failed" },
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }))
}

/// Renders one input into the flat output directory.
///
/// Never writes sidecars: an existing sidecar is validated against the
/// current contents (loud failure on mismatch); a missing one yields the
/// in-memory standard document (default copy, no adjustments).
fn batch_one(
    source: &Path,
    output_dir: &Path,
    format: ImageFileFormat,
    quality: u8,
    virtual_copy: Option<&str>,
) -> Result<PathBuf, McpError> {
    let name = source
        .file_name()
        .ok_or_else(|| McpError::InvalidParams("input has no file name".into()))?
        .to_string_lossy()
        .into_owned();
    // Construction guarantees extension/format agreement, so no separate
    // extension gate is needed here.
    let target = output_dir
        .join(&name)
        .with_extension(format.default_extension());

    let (_bytes, frame, raw_metadata) = read_and_decode(source)?;
    let identity =
        crate::util::build_source_identity(source, &_bytes, &frame, raw_metadata.as_ref())?;
    let sidecar_path = lumina_sidecar::sidecar_path_for(source);
    let document = if sidecar_path.exists() {
        let (document, _revision, _status) =
            crate::tools::load::open_existing_sidecar(&sidecar_path, &identity)?;
        document
    } else {
        SidecarDocument::new(identity, crate::tools::load::PIPELINE_VERSION)
    };

    let copy = match virtual_copy {
        Some(requested) => document
            .virtual_copies
            .iter()
            .find(|copy| copy.name == requested || copy.id == requested)
            .ok_or_else(|| McpError::UnknownCopy(requested.to_string()))?,
        None => document
            .virtual_copies
            .iter()
            .find(|copy| copy.is_default)
            .or_else(|| document.virtual_copies.first())
            .ok_or(McpError::NoImageLoaded)?,
    };
    let white_balance = raw_metadata.as_ref().map(|meta| meta.camera_white_balance);
    let rendered = render_recipe(&frame, &copy.recipe, white_balance)?;
    let bytes = encode_with_quality(&rendered, format, quality)?;

    // Non-destructive guard + atomic write: the target must never resolve onto
    // the source or one of its Lumina bundle files.
    crate::util::write_output_guarded(source, &target, &bytes)?;
    Ok(target)
}

/// REVIEW-CLI-BATCHCOLLIDE-1 parity: distinct inputs may map onto the same
/// flat target file name after extension normalization. List the colliding
/// pair and refuse before any output exists.
fn reject_duplicate_batch_targets(inputs: &[PathBuf], extension: &str) -> Result<(), McpError> {
    let mut seen: std::collections::BTreeMap<String, PathBuf> = std::collections::BTreeMap::new();
    for input in inputs {
        let name = input
            .file_name()
            .ok_or_else(|| McpError::InvalidParams("input has no file name".into()))?;
        let target = PathBuf::from(name)
            .with_extension(extension)
            .to_string_lossy()
            .into_owned();
        match seen.get(&target) {
            Some(first) if first != input => {
                return Err(McpError::InvalidParams(format!(
                    "batch output collision: `{}` and `{}` both write `{}` into the output directory; refusing to silently overwrite",
                    first.display(),
                    input.display(),
                    target
                )));
            }
            _ => {
                seen.insert(target, input.clone());
            }
        }
    }
    Ok(())
}
