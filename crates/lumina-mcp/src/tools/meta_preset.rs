//! `lumina_apply_meta_preset` — apply one S4 meta-preset to many images
//! (path-based per-path report).

use crate::error::McpError;
use crate::tools::meta_common::{expected_revision, map_sidecar_error, require_sidecar_for};
use crate::util::get_str;
use crate::Server;
use lumina_sidecar::{
    load_meta_preset_file, now_rfc3339_utc, render_meta_preset, resolve_meta_preset_path,
    save_sidecar_if_unchanged,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const NAME: &str = "lumina_apply_meta_preset";
pub const DESCRIPTION: &str = "Apply an IPTC meta-preset (*.lumina-meta-preset.json, static or \
dynamic) to several images by path. Dynamic presets require every placeholder variable via \
`vars` (missing/unknown variables and limit violations abort the whole call loudly, nothing is \
written). Per-path report with updated/unchanged/failed; idempotent re-application reports \
unchanged.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "paths": {
                "type": "array",
                "description": "Source image paths to apply the preset to.",
                "items": { "type": "string" },
                "minItems": 1
            },
            "preset": {
                "type": "string",
                "description": "Preset file path or display name (resolved like the CLI)."
            },
            "vars": {
                "type": "object",
                "description": "Placeholder variables for dynamic presets (all required).",
                "additionalProperties": { "type": "string" }
            }
        },
        "required": ["paths", "preset"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let paths = parse_paths(args)?;
    let preset_spec = get_str(args, "preset")?;
    let vars = parse_vars(args)?;

    // Resolve + validate + render upfront (SOLL §5): any deviation aborts the
    // whole call with exit-loud tool error before a single target is touched.
    let resolved = resolve_meta_preset_path(preset_spec, None).map_err(|error| {
        McpError::InvalidParams(format!(
            "cannot resolve meta preset `{preset_spec}`: {error}"
        ))
    })?;
    let preset = load_meta_preset_file(&resolved).map_err(|error| {
        McpError::InvalidParams(format!(
            "cannot load meta preset `{}`: {error}",
            resolved.display()
        ))
    })?;
    let display = resolved.display().to_string();
    let rendered = render_meta_preset(&preset, &vars, &display).map_err(|error| {
        McpError::InvalidParams(format!(
            "cannot render meta preset `{}`: {error}",
            resolved.display()
        ))
    })?;
    let origin = format!("preset:{}", preset.name);
    log::info!(
        "lumina_apply_meta_preset `{}` to {} target(s) ({} field(s))",
        preset.name,
        paths.len(),
        rendered.len()
    );

    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(paths.len());
    for target in &paths {
        match apply_preset_to_target(Path::new(target), &rendered, &origin) {
            Ok(true) => {
                log::info!(
                    "lumina_apply_meta_preset: `{target}` updated (preset `{}`)",
                    preset.name
                );
                updated_count += 1;
                items.push(json!({"path": target, "status": "updated"}));
            }
            Ok(false) => {
                log::info!(
                    "lumina_apply_meta_preset: `{target}` unchanged (preset `{}` already applied)",
                    preset.name
                );
                unchanged_count += 1;
                items.push(json!({"path": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{target}: {}", error.message());
                log::info!(
                    "lumina_apply_meta_preset: `{target}` failed ({})",
                    error.name()
                );
                failures.push(message.clone());
                items.push(json!({
                    "path": target,
                    "status": "failed",
                    "error_name": error.name(),
                    "error": message,
                }));
            }
        }
    }
    let failed = failures.len();
    log::info!(
        "lumina_apply_meta_preset: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (preset `{}`)",
        preset.name
    );
    Ok(json!({
        "preset": display,
        "name": preset.name,
        "updated": updated_count,
        "unchanged": unchanged_count,
        "failed": failed,
        "errors": failures,
        "items": items,
        "status": if failed == 0 { "ok" } else { "partial" },
    }))
}

fn parse_paths(args: &Value) -> Result<Vec<String>, McpError> {
    let array = args.get("paths").and_then(Value::as_array).ok_or_else(|| {
        McpError::InvalidParams("missing `paths` array (at least one source image path)".into())
    })?;
    if array.is_empty() {
        return Err(McpError::InvalidParams(
            "`paths` must contain at least one source image path".into(),
        ));
    }
    let mut paths = Vec::with_capacity(array.len());
    for entry in array {
        let path = entry.as_str().ok_or_else(|| {
            McpError::InvalidParams(format!("`paths` entries must be strings (got `{entry}`)"))
        })?;
        paths.push(path.to_string());
    }
    Ok(paths)
}

fn parse_vars(args: &Value) -> Result<BTreeMap<String, String>, McpError> {
    let Some(value) = args.get("vars").filter(|value| !value.is_null()) else {
        return Ok(BTreeMap::new());
    };
    let object = value.as_object().ok_or_else(|| {
        McpError::InvalidParams("`vars` must be an object of placeholder → string value".into())
    })?;
    let mut vars = BTreeMap::new();
    for (key, entry) in object {
        let text = entry.as_str().ok_or_else(|| {
            McpError::InvalidParams(format!(
                "variable `{key}` must have a string value (got `{entry}`)"
            ))
        })?;
        vars.insert(key.clone(), text.to_string());
    }
    Ok(vars)
}

/// Applies the rendered preset to one target per CAS + atomic save: `Ok(true)`
/// on update, `Ok(false)` for idempotent no-ops. A missing sidecar or a CAS
/// conflict fails only its own item — the series never aborts.
fn apply_preset_to_target(
    target: &Path,
    resolved: &BTreeMap<String, String>,
    origin: &str,
) -> Result<bool, McpError> {
    if !target.exists() {
        return Err(McpError::FileNotFound(target.display().to_string()));
    }
    let (sidecar_path, document) = require_sidecar_for(target)?;
    let expected = expected_revision(&document)?;
    let mut candidate = document.clone();
    if !candidate
        .apply_metadata_draft(resolved, origin, &now_rfc3339_utc())
        .map_err(map_sidecar_error)?
    {
        return Ok(false);
    }
    save_sidecar_if_unchanged(&sidecar_path, &candidate, Some(&expected))
        .map_err(map_sidecar_error)?;
    Ok(true)
}
