//! MCP-PARITY-C: the shared core of the two path-based merge tools
//! `lumina_merge_hdr` and `lumina_merge_pano` — one implementation, two names.
//!
//! The tools mirror `lumina merge-hdr` / `lumina merge-pano`: they feed the
//! **same** orchestration ([`lumina_merge::bundle::run_merge`], the CLI knot
//! `crates/lumina-cli/src/merge.rs:92-106`) from the **same** bytes with the
//! **same** decode + exposure adapter ([`lumina_stages::merge`], Impl-A). There
//! is no second merge implementation and no second DNG writer, so an MCP call
//! and the equivalent CLI call cannot drift in digest, checksum, sidecar or
//! error class (SOLL: `feature/quality/mcp-parity-c-soll.md` §4.3, §6
//! Invarianten 1–2; the MCP-side parity tests are a later slice).
//!
//! # The handler/merge-domain split (§2)
//!
//! This module checks **JSON shape and type only** — a field present, a
//! string/array/integer/boolean as promised, `additionalProperties`. Every
//! violation of that is `InvalidParams`. Every **merge-domain** rule (source
//! count, list arity range, `output` form, exposure/recipe ranges) is left to
//! `run_merge` / `resolve_exposure` / `recipe.validate()` and surfaces as the
//! mapped [`MergeRunError`] class (§3): an MCP call never turns a CLI
//! `merge failed:` / `merge unsupported:` into an `InvalidParams`. The single
//! deliberate exception is the explicit-list arity, which the CLI also checks
//! itself *before* `run_merge` (and therefore reports as a tool-level message,
//! not as a `MergeRunError`); it maps onto `InvalidParams`.
//!
//! # The result payload (§5)
//!
//! Exactly the seven CLI `--json` fields — `command`, `output`, `sidecar`,
//! `digest`, `checksum`, `cached`, `status` — plus the optional `warning`
//! string when the alignment residual exceeded the documented threshold. No
//! `saved`/`action`/`text` envelope: `result_payload` invents those for
//! `BulkReport`s and would add fields the CLI JSON does not have.

use crate::error::McpError;
use crate::tools::stage_common::{optional_bool, optional_str, reject_unknown_fields};
use lumina_merge::bundle::{
    merge_command_name, run_merge, MergeRunError, MergeRunOptions, MergeRunOutcome,
    MergeSourceFrame,
};
use lumina_sidecar::MergeMode;
use lumina_stages::merge::{
    check_explicit_list_lengths, decode_source, resolve_exposure, MergeExplicitInputs,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Every argument key the two merge tools accept (`additionalProperties:
/// false`, enforced in the handler — a schema is only a description for the
/// calling model).
pub const ALLOWED: &[&str] = &[
    "inputs",
    "output",
    "exposure_times",
    "isos",
    "f_numbers",
    "max_shift_px",
    "blend_width_px",
    "force",
];

/// Alignment search radius default (the CLI's `#[arg(default_value_t = 16)]`).
const DEFAULT_MAX_SHIFT_PX: i32 = 16;
/// Feather-blend width default (the CLI's `#[arg(default_value_t = 64)]`).
const DEFAULT_BLEND_WIDTH_PX: u32 = 64;

/// Runs one merge job: parse the arguments (JSON shape only), validate the
/// shared rules, call the shared orchestration, map its error class, build the
/// §5 payload. `merge_hdr::run` / `merge_pano::run` are one-liners onto this.
pub fn run_mode(mode: MergeMode, args: &Value) -> Result<Value, McpError> {
    // Shape gate first (§2 Pkt. 5): unknown fields are a loud `InvalidParams`,
    // silently ignoring them would be the "silent no-op" the slice forbids.
    reject_unknown_fields(args, merge_command_name(mode), ALLOWED)?;

    // `inputs` is required and must be a string array (shape). Its COUNT is
    // merge domain: 0/1/257 sources fail in `validate_source_count` inside
    // `run_merge` with the CLI's own `merge failed:` class (§2 Pkt. 1), never
    // as a tool-level `InvalidParams`.
    let inputs = match args.get("inputs") {
        Some(value) => serde_json::from_value::<Vec<PathBuf>>(value.clone()).map_err(|error| {
            McpError::InvalidParams(format!(
                "`inputs` must be an array of path strings: {error}"
            ))
        })?,
        None => return Err(McpError::InvalidParams("missing `inputs`".into())),
    };
    let exposure_times = number_list::<f64>(args, "exposure_times", "numbers")?;
    let isos = number_list::<u32>(args, "isos", "integers")?;
    let f_numbers = number_list::<f64>(args, "f_numbers", "numbers")?;
    let explicit = MergeExplicitInputs {
        input: &inputs,
        exposure_times: &exposure_times,
        isos: &isos,
        f_numbers: &f_numbers,
    };
    // The one gestalt rule that stays handler-side (§2 Pkt. 2): each explicit
    // list is empty or carries exactly one value per input. The CLI checks the
    // same rule before `run_merge` and reports it as its own message, so MCP
    // maps it onto `InvalidParams` — the message text is the shared one.
    check_explicit_list_lengths(&explicit).map_err(McpError::InvalidParams)?;

    // `output` is optional: absent or `null` is the valid default target
    // (`<ref-stem>-HDR.dng` / `-Pano.dng` beside `inputs[0]`). Only a present
    // non-string is a type error; the FORM (`.dng`, file name, not a source)
    // is checked by `resolve_dng_path` inside `run_merge` (§2 Pkt. 3).
    let output = optional_str(args, "output")?.map(PathBuf::from);
    // Strict JSON types, no range clamp: a negative `max_shift_px` reaches the
    // library guard in `run_merge` (`merge failed:`) and an out-of-range
    // `blend_width_px` reaches `recipe.validate()` — exactly like the CLI
    // (§2 Pkt. 4).
    let max_shift_px = optional_i32(args, "max_shift_px")?.unwrap_or(DEFAULT_MAX_SHIFT_PX);
    let blend_width_px = optional_u32(args, "blend_width_px")?.unwrap_or(DEFAULT_BLEND_WIDTH_PX);
    let force = optional_bool(args, "force")?.unwrap_or(false);

    let options = MergeRunOptions {
        output,
        max_shift_px,
        blend_width_px,
        force,
        dng_decode_version: lumina_raw::libraw_decode_version(),
    };
    let resolve = |mode: MergeMode, index: usize, source: &MergeSourceFrame| {
        resolve_exposure(mode, index, &explicit, source)
    };
    let outcome =
        run_merge(mode, &inputs, &options, decode_source, resolve).map_err(map_merge_run_error)?;

    Ok(outcome_payload(mode, &outcome))
}

/// Maps a [`MergeRunError`] onto the MCP error model (§3 table).
///
/// The **full `Display` text** — `merge missing:` / `merge stale:` /
/// `merge unsupported:` / `merge failed:` plus the cause — is kept verbatim
/// inside the message (`MergeRunError::Display`,
/// `bundle.rs:267-271`), so a client can substring-test the canonical CLI
/// prefix, and every rejection stays a loud tool error (`isError: true`),
/// never a silent partial result.
fn map_merge_run_error(error: MergeRunError) -> McpError {
    match &error {
        MergeRunError::Missing(_) => McpError::FileNotFound(error.to_string()),
        MergeRunError::Stale(_) => McpError::MergeStale(error.to_string()),
        MergeRunError::Unsupported(_) => McpError::MergeUnsupported(error.to_string()),
        MergeRunError::Failed(_) => McpError::MergeFailed(error.to_string()),
    }
}

/// The tool result: exactly the CLI `--json` fields (§5) plus `warning`
/// (string, only when the residual exceeded the documented threshold). The
/// warning text is the CLI's own warning line (`merge.rs:108-116`); naming the
/// final field form stays with the owner (SOLL §13 Punkt 5).
fn outcome_payload(mode: MergeMode, outcome: &MergeRunOutcome) -> Value {
    let command = merge_command_name(mode);
    if outcome.residual_warning {
        log::warn!(
            "{command}: aligned with residual {:.2}px (above the documented shift threshold)",
            outcome.residual_px
        );
    } else {
        log::info!("{command}: aligned (residual {:.2}px)", outcome.residual_px);
    }
    let mut payload = json!({
        "command": command,
        "output": outcome.dng_path,
        "sidecar": outcome.sidecar_path,
        "digest": outcome.digest,
        "checksum": outcome.checksum,
        "cached": outcome.cached,
        "status": "ok",
    });
    if outcome.residual_warning {
        payload["warning"] = Value::String(format!(
            "{command} aligned with residual {:.2}px \
             (above the documented shift threshold); result kept, nothing cropped silently",
            outcome.residual_px
        ));
    }
    payload
}

/// Parses an optional JSON array of numbers into `Vec<T>`.
///
/// Stops at the JSON-type level: a non-array, a non-number element or (for
/// integer targets) a fractional value is `InvalidParams`. The values' domain
/// ranges are deliberately NOT checked here — they belong to the merge domain
/// (`resolve_exposure` → `merge unsupported:`, §2 footnote).
fn number_list<T: DeserializeOwned>(
    args: &Value,
    key: &str,
    kind: &str,
) -> Result<Vec<T>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => serde_json::from_value::<Vec<T>>(value.clone()).map_err(|error| {
            McpError::InvalidParams(format!("`{key}` must be an array of {kind}: {error}"))
        }),
    }
}

/// Parses an optional integer field, negatives included (`max_shift_px` is an
/// `i32` in the CLI). Only the JSON type is checked; a negative value is
/// passed through to `run_merge`'s library guard (§2 Pkt. 4).
fn optional_i32(args: &Value, key: &str) -> Result<Option<i32>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| {
                McpError::InvalidParams(format!("`{key}` must be an integer, got `{number}`"))
            }),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be an integer, got `{other}`"
        ))),
    }
}

/// Parses an optional unsigned integer field (`blend_width_px` is a `u32` in
/// the CLI). Only the JSON type is checked; the documented `0..=65536` range
/// is enforced by `recipe.validate()` inside `run_merge` (§2 Pkt. 4).
fn optional_u32(args: &Value, key: &str) -> Result<Option<u32>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| {
                McpError::InvalidParams(format!(
                    "`{key}` must be a non-negative integer, got `{number}`"
                ))
            }),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be a non-negative integer, got `{other}`"
        ))),
    }
}

/// The input schema both tools publish. The mode is the only variable: the
/// shared core builds one schema per mode, and the two tools differ in nothing
/// but their `NAME`/`DESCRIPTION` and the documented default target name.
///
/// The `minItems`/`maxItems`/`minimum`/`maximum` keywords are **description**
/// for the calling model: the handler does not enforce them and the merge
/// domain checks the same rules with its own error class (§2).
pub fn schema(mode: MergeMode) -> Value {
    let default_target = match mode {
        MergeMode::Hdr => "<reference-stem>-HDR.dng",
        MergeMode::Panorama => "<reference-stem>-Pano.dng",
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "inputs": {
                "type": "array",
                "items": { "type": "string" },
                "minItems": 2,
                "maxItems": 256,
                "description": "Source paths in merge order; inputs[0] is the reference. Absolute paths. 2..=256 sources: the count itself is enforced by the merge domain (`merge failed:`), not by this schema."
            },
            "output": {
                "type": ["string", "null"],
                "description": format!("Merge-DNG target. Absent or null = the default `{default_target}` next to inputs[0]. A present value must end in `.dng`, carry a file name and not name a source; that form is checked by the merge domain (`merge failed:`).")
            },
            "exposure_times": {
                "type": "array",
                "items": { "type": "number" },
                "description": "Per-source exposure times in seconds, one value per input (empty or exactly len(inputs)). Absent/empty = EXIF-or-fallback per source; HDR without any exposure source fails loudly as `merge unsupported:`, panorama stores it as provenance only. Each value must be finite within (0, 86400]."
            },
            "isos": {
                "type": "array",
                "items": { "type": "integer" },
                "description": "Per-source ISO values, one value per input (empty or exactly len(inputs)). Each value must be within 1..=10000000; out of range is `merge unsupported:` (HDR)."
            },
            "f_numbers": {
                "type": "array",
                "items": { "type": "number" },
                "description": "Per-source f-numbers, one value per input (empty or exactly len(inputs)). Each value must be finite within (0, 256]; out of range is `merge unsupported:` (HDR)."
            },
            "max_shift_px": {
                "type": "integer",
                "minimum": 0,
                "description": "Alignment search radius in pixels (default 16). No upper limit is documented; a negative value reaches the merge domain's library guard (`merge failed:`)."
            },
            "blend_width_px": {
                "type": "integer",
                "minimum": 0,
                "maximum": 65536,
                "description": "Feather-blend width in the panorama overlap (default 64). The 0..=65536 range is enforced by the merge domain (`merge failed:`), not by this schema."
            },
            "force": {
                "type": "boolean",
                "description": "Re-merge even when an existing bundle is stale or dangling (default false). Without it a stale bundle fails loudly as `merge stale:`."
            }
        },
        "required": ["inputs"]
    })
}
