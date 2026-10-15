//! `lumina_matrix` — the recipe matrix as an MCP tool (MCP-PARITY-C).
//!
//! Mirrors `lumina matrix`: the call goes through the shared runner
//! ([`lumina_stages::matrix_run::run`], the CLI knot
//! `crates/lumina-cli/src/matrix.rs`) over a [`MatrixRequest`], so the report,
//! the golden/export bytes and the error texts are transport-independent.
//! Path-based: one call is one matrix run over `recipe_set`, no `image_id`,
//! no session state.
//!
//! # Handler/domain split (§2)
//!
//! The handler checks **JSON shape and type only**; every matrix-domain rule
//! (set validation, recipe filter, sample existence, tolerance,
//! `comparison_width == 0`, `require_gpu` combinations) is left to the runner
//! and surfaces as [`McpError::Matrix`] with the CLI text verbatim (§7).
//! A red run (`failed > 0`) is a tool error that still carries the full
//! report in `data.report` (SOLL §7.1).
//!
//! # Payload (§3)
//!
//! Exactly the CLI `--json` fields — no `saved`/`action`/`text`/`warnings[]`.

use crate::error::McpError;
use crate::tools::stage_common::{optional_bool, optional_str, reject_unknown_fields};
use crate::Server;
use lumina_stages::{matrix_run, MatrixRender, MatrixRequest};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const NAME: &str = "lumina_matrix";
pub const DESCRIPTION: &str = "Run the recipe matrix (`lumina matrix` twin). Path-based: \
`recipe_set` names the versioned recipe-set file (absent/null = the env chain \
`LUMINA_MATRIX_RECIPE_SET` -> `LUMINA_MATRIX_DIR` -> ancestor walk), one call is one \
matrix run. Verifies every sample x recipe pair against its golden PNG within the \
declared tolerance, or writes the baselines with `update_goldens: true` (CPU-pinned). \
`recipes` selects recipe ids (absent/null/empty = all, in set order); `comparison_width` \
overrides the set width; `require_gpu` proves the declared `expected_route` against the \
route the render path actually used. Returns the CLI's own JSON fields (`command`, \
`mode`, `recipe_set`, `golden_dir`, `comparison_width`, `pairs_total`, `passed`, \
`failed`, `duration_ms`, `pairs`). Argument shape errors are `InvalidParams`; every \
matrix-domain rejection keeps the CLI's own text (`MatrixError`).";

/// Every argument key the tool accepts (`additionalProperties: false`,
/// enforced in the handler — a schema is only a description for the calling
/// model).
pub const ALLOWED: &[&str] = &[
    "recipe_set",
    "golden_dir",
    "work_dir",
    "recipes",
    "comparison_width",
    "update_goldens",
    "require_gpu",
];

/// The MCP render port: the MCP-owned choke point
/// ([`crate::util::render_matrix_routed`]), never the CLI's
/// `render_standard_routed` (it is `pub(crate)` there and baseline-locked).
struct McpMatrixRender;

impl MatrixRender for McpMatrixRender {
    fn render(
        &self,
        frame: &lumina_core::ImageFrame,
        recipe: &lumina_sidecar::EditRecipe,
        context: &lumina_core::RenderContext<'_>,
    ) -> Result<(lumina_core::RenderOutput, lumina_stages::MatrixRoute), lumina_stages::StageError>
    {
        crate::util::render_matrix_routed(frame, recipe, context)
    }

    fn has_gpu_backend(&self) -> bool {
        // Compile-time capability, mirroring the CLI port: a build without
        // the `gpu` feature can never take the GPU route, so the runner's
        // `--require-gpu` pre-check fails loudly instead of reporting every
        // recipe as an unexpected CPU route.
        cfg!(feature = "gpu")
    }
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "recipe_set": {
                "type": ["string", "null"],
                "description": "Versioned recipe-set file. Absent or null = the env chain `LUMINA_MATRIX_RECIPE_SET` -> `LUMINA_MATRIX_DIR` -> ancestor walk; existence is checked by the matrix domain (`MatrixError`), not here."
            },
            "golden_dir": {
                "type": ["string", "null"],
                "description": "Golden directory. Absent or null = `<recipe-set-dir>/golden`; created only in baseline mode."
            },
            "work_dir": {
                "type": ["string", "null"],
                "description": "Directory for full-resolution exports and diff images. Absent or null = a fresh temporary directory, removed afterwards (like the CLI)."
            },
            "recipes": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Only run these recipe ids. Absent, null or empty = all recipes of the set, in set order; an unknown id aborts loudly in the matrix domain (`MatrixError`)."
            },
            "comparison_width": {
                "type": "integer",
                "minimum": 1,
                "description": "Override the comparison width from the recipe set. Absent or null = the set value. `minimum` is descriptive: 0 is a loud matrix-domain error (`MatrixError`), not `InvalidParams`."
            },
            "update_goldens": {
                "type": "boolean",
                "description": "Baseline mode: write/refresh the goldens instead of verifying (CPU-pinned, platform-neutral; default false). Cannot be combined with `require_gpu`."
            },
            "require_gpu": {
                "type": "boolean",
                "description": "GPU-parity proof: fail every pair whose declared `expected_route` does not match the route the render path actually used (default false)."
            }
        },
        "required": []
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    // Shape gate first (§2 Pkt. 5): unknown fields are a loud `InvalidParams`
    // before any file access.
    reject_unknown_fields(args, NAME, ALLOWED)?;

    // Absent or `null` = the runner default in each case; only a present
    // non-string is a type error (§2 Pkt. 2). Existence stays domain.
    let recipe_set = optional_str(args, "recipe_set")?.map(PathBuf::from);
    let golden_dir = optional_str(args, "golden_dir")?.map(PathBuf::from);
    let work_dir = optional_str(args, "work_dir")?.map(PathBuf::from);
    // Absent or `null` = all recipes; a non-array or a non-string element is
    // `InvalidParams`, while unknown ids stay domain (§2 Pkt. 1).
    let recipes = string_list(args, "recipes")?;
    // Strict JSON integer (§2 Pkt. 3): `0` passes through to the domain.
    let comparison_width = optional_width(args, "comparison_width")?;
    // The flag *combination* stays domain (§2 Pkt. 4).
    let update_goldens = optional_bool(args, "update_goldens")?.unwrap_or(false);
    let require_gpu = optional_bool(args, "require_gpu")?.unwrap_or(false);

    let request = MatrixRequest {
        recipe_set,
        golden_dir,
        work_dir,
        recipes,
        comparison_width,
        update_goldens,
        require_gpu,
    };
    match matrix_run::run(&request, &McpMatrixRender) {
        Ok(report) => match report.summary_error {
            None => {
                log::info!(
                    "matrix: {} passed, 0 failed ({} ms)",
                    report.payload["passed"],
                    report.payload["duration_ms"]
                );
                Ok(report.payload)
            }
            // A red run is an error WITH the report (SOLL §7.1).
            Some(summary) => Err(McpError::Matrix {
                message: summary,
                report: Some(report.payload),
            }),
        },
        // Every pre-report abort keeps the CLI text verbatim (§7).
        Err(error) => Err(McpError::Matrix {
            message: error.to_string(),
            report: None,
        }),
    }
}

/// Parses an optional string array (`null` = absent). Stops at the JSON-type
/// level; the values' domain (known recipe ids) belongs to the matrix runner.
fn string_list(args: &Value, key: &str) -> Result<Vec<String>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => serde_json::from_value::<Vec<String>>(value.clone()).map_err(|error| {
            McpError::InvalidParams(format!("`{key}` must be an array of strings: {error}"))
        }),
    }
}

/// Parses an optional comparison width strictly: only a JSON integer that fits
/// `u32` passes — `1.5`/`"384"`/`-1` are `InvalidParams`. `Some(0)` is valid
/// here and fails loudly in the matrix domain (§2 Pkt. 3).
fn optional_width(args: &Value, key: &str) -> Result<Option<u32>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| {
                McpError::InvalidParams(format!("`{key}` must be an integer, got `{number}`"))
            }),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be an integer, got `{other}`"
        ))),
    }
}
