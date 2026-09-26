//! `lumina_trigger_export` — render one image by path and export it, with an
//! opt-in JPEG metadata bake-in (LRPAR-G15-IPTC-S7).
//!
//! Wraps the same choke point as `lumina_save` (`render_recipe` → encode →
//! `write_output_guarded`) without touching the session. `write_metadata`
//! with a non-JPEG format is a loud [`McpError::InvalidParams`] (SOLL §7:
//! bake-in is JPEG-only); without the flag the export is exactly today's
//! behavior (no metadata, no silent assumptions).

use crate::error::McpError;
use crate::tools::load::{open_existing_sidecar, PIPELINE_VERSION};
use crate::tools::meta_common::draft_to_iptc;
use crate::util::{
    build_source_identity, encode_with_quality, get_str, parse_bounded_uint, parse_output_format,
    read_and_decode, render_recipe, validate_output_extension, write_output_guarded,
};
use crate::Server;
use lumina_core::ImageFileFormat;
use lumina_sidecar::SidecarDocument;
use serde_json::{json, Value};
use std::path::Path;

pub const NAME: &str = "lumina_trigger_export";
pub const DESCRIPTION: &str = "Render one image by path and export it (same choke point as \
lumina_save, without touching the session). `write_metadata: true` splices the sidecar draft \
(+ keywords) as IPTC IIM + XMP into the export — JPEG only; any other format with \
`write_metadata` is rejected loudly (InvalidParams). Without the flag the export carries no \
metadata.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path to the source image." },
            "output_path": { "type": "string", "description": "Destination path for the export." },
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
                "description": "Virtual copy name or id (default: the standard copy)."
            },
            "write_metadata": {
                "type": "boolean",
                "description": "Splice the IPTC draft into the export (JPEG only, default: false)."
            }
        },
        "required": ["path", "output_path"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let path_str = get_str(args, "path")?;
    let source = Path::new(path_str);
    let output_str = args
        .get("output_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing `output_path`".into()))?;
    let output = Path::new(output_str);

    // Fail fast before any render: format/extension agreement, strict quality
    // bounds, and the JPEG-only gate for the metadata bake-in (SOLL §7).
    let format_str = args
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("png");
    let format = parse_output_format(format_str)?;
    let quality = parse_bounded_uint(args, "quality", 1, 100)?.unwrap_or(90) as u8;
    validate_output_extension(output, format)?;
    let write_metadata = match args.get("write_metadata").filter(|value| !value.is_null()) {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| {
            McpError::InvalidParams(format!(
                "`write_metadata` must be a boolean (got `{value}`)"
            ))
        })?,
    };
    if write_metadata && format != ImageFileFormat::Jpeg {
        let name = match format {
            ImageFileFormat::Png => "png",
            ImageFileFormat::Jpeg => "jpeg",
            ImageFileFormat::WebP => "webp",
        };
        return Err(McpError::InvalidParams(format!(
            "`write_metadata` is only supported for JPEG exports; refusing {name} output `{output_str}` (bake-in is JPEG-only)"
        )));
    }

    let (bytes, frame, raw_metadata) = read_and_decode(source)?;
    let identity = build_source_identity(source, &bytes, &frame, raw_metadata.as_ref())?;
    let sidecar_path = lumina_sidecar::sidecar_path_for(source);
    let document = if sidecar_path.exists() {
        let (document, _revision, _status) = open_existing_sidecar(&sidecar_path, &identity)?;
        document
    } else {
        // CLI export parity: a missing sidecar renders the in-memory default
        // recipe — the export never materializes a sidecar by itself.
        SidecarDocument::new(identity, PIPELINE_VERSION)
    };

    let requested = args.get("virtual_copy").and_then(|value| value.as_str());
    let copy = match requested {
        Some(name) => document
            .virtual_copies
            .iter()
            .find(|copy| copy.name == name || copy.id == name)
            .ok_or_else(|| McpError::UnknownCopy(name.to_string()))?,
        None => document
            .virtual_copies
            .iter()
            .find(|copy| copy.is_default)
            .or_else(|| document.virtual_copies.first())
            .ok_or(McpError::NoImageLoaded)?,
    };
    let white_balance = raw_metadata.as_ref().map(|meta| meta.camera_white_balance);
    // MCP-MASK-APPLY: apply the persisted mask planes (policy `warn`) exactly
    // like the CLI export, so MCP and CLI render byte-identical output.
    let zdata_path = lumina_sidecar::zdata_path_for(source);
    let masks = crate::masks::render_mask_context(&document, &copy.id, &zdata_path);
    let rendered = render_recipe(&frame, &copy.recipe, white_balance, Some(&masks))?;
    let mut encoded = encode_with_quality(&rendered, format, quality)?;

    // Opt-in bake-in (S6 `bake_metadata_into_jpeg` parity, post-encode splice;
    // pixel bytes verbatim). An empty draft keeps the export plain with a loud
    // warning (`empty`, never a silent no-op); an IIM-limit violation fails
    // the export loudly before anything is written.
    let metadata_written = if write_metadata {
        Some(bake_metadata_into_jpeg(
            &document,
            &mut encoded,
            output_str,
        )?)
    } else {
        None
    };

    write_output_guarded(source, output, &encoded)?;

    let mut payload = json!({
        "ok": true,
        "path": output_str,
        "bytes_written": encoded.len() as u64,
        "format": format.default_extension(),
    });
    if let Some(written) = metadata_written {
        payload["metadata_written"] = written;
    }
    log::info!(
        "lumina_trigger_export for `{path_str}` → `{output_str}` (write_metadata: {write_metadata})"
    );
    Ok(payload)
}

fn bake_metadata_into_jpeg(
    document: &SidecarDocument,
    encoded: &mut Vec<u8>,
    output_str: &str,
) -> Result<Value, McpError> {
    let meta = draft_to_iptc(document);
    if meta.is_empty() {
        log::warn!(
            "no IPTC draft or keywords for `{output_str}`; exporting without embedded metadata (metadata_written: empty)"
        );
        return Ok(json!({"iim": false, "xmp": false, "status": "empty"}));
    }
    let spliced = lumina_iptc::embed_metadata(encoded, &meta).map_err(|error| {
        McpError::Encode(format!(
            "metadata bake-in for `{output_str}` failed: {error}"
        ))
    })?;
    *encoded = spliced;
    log::info!("export with IPTC metadata for `{output_str}` (metadata_written: written)");
    Ok(json!({"iim": true, "xmp": true, "status": "written"}))
}
