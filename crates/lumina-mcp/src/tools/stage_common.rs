//! Shared plumbing for the four session-based recipe stage editors exposed as
//! MCP tools (`lumina_spot`, `lumina_lens_blur`, `lumina_geometry`,
//! `lumina_upright`) — MCP-PARITY-A.
//!
//! # Read and write are separate
//!
//! Every tool takes a required `op`. The read ops (`list`, and `detect` for
//! `spot`) never touch the sidecar; the write ops always name one explicit
//! field or one explicit operation. There is deliberately **no** op that writes
//! a whole stage object, so an agent can never overwrite fields it did not ask
//! to change.
//!
//! # One implementation, two transports
//!
//! The tools build a `lumina_stages::*Request` and call the same
//! `lumina_stages::*::run` that `lumina-cli` calls (see
//! `crates/lumina-cli/src/stages.rs`). They do not re-implement a single
//! mutation, validation rule or payload field. An MCP call therefore leaves
//! byte-identical sidecar bytes to the equivalent CLI call — the property
//! `crates/lumina-cli/tests/stage_parity.rs` proves for all four editors in
//! both the read and the write direction.
//!
//! # Session semantics
//!
//! The tools are session tools: `image_id` resolves the loaded image and the
//! sidecar is read fresh from disk on every call, so an external change is seen
//! rather than cached. A write is handed to [`Persist::Deferred`], so the
//! validated document is written by [`commit`] under a compare-and-swap against
//! the revision `lumina_load` saw — exactly the `lumina_edit` contract. A CAS
//! miss surfaces as `SidecarConflict` (`-32010`) instead of silently
//! overwriting. The session is updated exactly the way `lumina_edit` updates it
//! (`document` + `sidecar_revision`), which is the only session state these
//! tools touch. A **read** never enters [`commit`]'s write path: it has nothing
//! to roll back and never writes.

use crate::error::McpError;
use crate::session::ImageState;
use crate::Server;
use lumina_sidecar::{save_sidecar_if_unchanged, SidecarDocument, SidecarError};
use serde_json::Value;

/// Read ops. These never write the sidecar.
pub const READ_OPS: &[&str] = &["list"];

/// Rejects every argument key that is not in `allowed`.
///
/// The JSON schemas already set `"additionalProperties": false`, but a schema is
/// a *description* for the calling model: the enforcement has to happen in the
/// handler, or a client that ignores the schema could pass an unknown field and
/// get a silent success. Unknown fields are therefore a loud `InvalidParams`.
pub fn reject_unknown_fields(args: &Value, tool: &str, allowed: &[&str]) -> Result<(), McpError> {
    let Some(object) = args.as_object() else {
        return Err(McpError::InvalidParams(format!(
            "`{tool}` arguments must be an object"
        )));
    };
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(McpError::InvalidParams(format!(
                "unknown field `{key}` for `{tool}`; expected one of {}",
                allowed.join("|")
            )));
        }
    }
    Ok(())
}

/// Rejects an operation that the tool does not implement.
pub fn reject_unknown_op(tool: &str, op: &str, ops: &[&str]) -> Result<(), McpError> {
    if ops.contains(&op) {
        return Ok(());
    }
    Err(McpError::InvalidParams(format!(
        "unknown op `{op}` for `{tool}`; expected one of {}",
        ops.join("|")
    )))
}

/// Enforces that a write op carries the value it names.
///
/// Without this a `op="set_amount"` call with no `amount` would fall through to
/// the shared editor with no mutation set and come back as a successful *read* —
/// exactly the "silent no-op / empty result as success" the slice forbids. Every
/// write op therefore names its required value(s) and a missing one is a loud
/// `InvalidParams` before the shared editor is reached.
pub fn require_value(
    tool: &str,
    op: &str,
    // `requirements` holds `(name, is_missing)` pairs: only the names whose value
    // is absent for this op are reported, so one call describes one op.
    requirements: &[(&str, bool)],
) -> Result<(), McpError> {
    let missing: Vec<&str> = requirements
        .iter()
        .filter(|(_, is_missing)| *is_missing)
        .map(|(name, _)| *name)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(McpError::InvalidParams(format!(
        "{tool}: op `{op}` requires {}",
        missing
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(" and ")
    )))
}

/// Resolves the optional `virtual_copy` id. An unknown id is a loud
/// `UnknownCopy` from the shared editor, never a silent fallback to another
/// copy.
pub fn virtual_copy(args: &Value) -> Result<Option<String>, McpError> {
    match args.get("virtual_copy") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`virtual_copy` must be a string, got `{other}`"
        ))),
    }
}

/// Reads a required, non-empty string field.
pub fn required_str(args: &Value, key: &str) -> Result<String, McpError> {
    match args.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        Some(Value::String(_)) => Err(McpError::InvalidParams(format!(
            "`{key}` must not be empty"
        ))),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be a string, got `{other}`"
        ))),
        None => Err(McpError::InvalidParams(format!("missing `{key}`"))),
    }
}

/// Reads an optional string field (`null` = absent).
pub fn optional_str(args: &Value, key: &str) -> Result<Option<String>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be a string, got `{other}`"
        ))),
    }
}

fn number(args: &Value, key: &str) -> Result<Option<f64>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let value = number
                .as_f64()
                .ok_or_else(|| McpError::InvalidParams(format!("`{key}` is not a number")))?;
            if !value.is_finite() {
                return Err(McpError::InvalidParams(format!(
                    "`{key}` must be a finite number, got `{value}`"
                )));
            }
            Ok(Some(value))
        }
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be a number, got `{other}`"
        ))),
    }
}

/// Reads an optional finite `f32` field.
///
/// Out-of-range values are deliberately **not** clamped here: the shared stage
/// editor rejects them with the CLI's own message and range, so both transports
/// report the same thing for the same input.
pub fn optional_f32(args: &Value, key: &str) -> Result<Option<f32>, McpError> {
    Ok(number(args, key)?.map(|value| value as f32))
}

/// Reads an optional finite `f64` field.
pub fn optional_f64(args: &Value, key: &str) -> Result<Option<f64>, McpError> {
    number(args, key)
}

/// Reads an optional integral `usize` field in `0..=4096`.
pub fn optional_usize(args: &Value, key: &str) -> Result<Option<usize>, McpError> {
    match number(args, key)? {
        None => Ok(None),
        Some(value) if value < 0.0 || value.fract() != 0.0 || value > 4096.0 => {
            Err(McpError::InvalidParams(format!(
                "`{key}` must be an integer in 0..=4096, got `{value}`"
            )))
        }
        Some(value) => Ok(Some(value as usize)),
    }
}

/// Reads an optional non-negative integral `u64` field.
pub fn optional_u64(args: &Value, key: &str) -> Result<Option<u64>, McpError> {
    match number(args, key)? {
        None => Ok(None),
        Some(value) if value < 0.0 || value.fract() != 0.0 => Err(McpError::InvalidParams(
            format!("`{key}` must be a non-negative integer, got `{value}`"),
        )),
        Some(value) => Ok(Some(value as u64)),
    }
}

/// Reads an optional boolean field (`null` = absent).
pub fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, McpError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(other) => Err(McpError::InvalidParams(format!(
            "`{key}` must be a boolean, got `{other}`"
        ))),
    }
}

/// Maps a shared [`lumina_stages::StageError`] onto the MCP error model.
///
/// Loudness is preserved: every variant becomes a tool error with
/// `isError: true`. Nothing is downgraded to an empty result, and no rejection
/// is ever reported as a success with zero rows.
pub fn map_stage_error(tool: &str, error: lumina_stages::StageError) -> McpError {
    use lumina_stages::StageError;
    match error {
        StageError::Message(message) => McpError::InvalidParams(format!("{tool}: {message}")),
        StageError::Io { path, message } => {
            McpError::Sidecar(format!("{tool}: I/O error for `{path}`: {message}"))
        }
        StageError::Sidecar(error) => McpError::Sidecar(format!("{tool}: {error}")),
        // An out-of-range/inverted-range rule rejected by the sidecar validator
        // or by `lumina-core` keeps its structured `InvalidAdjustment` shape.
        StageError::Core(error) => match crate::error::map_core_error(error) {
            // An out-of-range / inverted-range rule keeps its structured shape.
            other @ McpError::InvalidAdjustment { .. } => other,
            other => McpError::Render(format!("{tool}: {}", other.message())),
        },
        StageError::Raw(error) => McpError::Decode(format!("{tool}: {error}")),
    }
}

/// Commits a [`lumina_stages::StageRun`] for a session tool.
///
/// A read (`document == None`) is a no-op and returns the current revision. A
/// write is persisted under the compare-and-swap the session loaded, and the
/// session is rebased onto the new revision — the same contract `lumina_edit`
/// implements. The bytes written are the ones the shared editor validated, via
/// the same `lumina_sidecar` atomic save the CLI uses, which is what makes an
/// MCP write byte-identical to the equivalent CLI write.
pub fn commit(
    state: &mut ImageState,
    run: lumina_stages::StageRun,
) -> Result<(lumina_stages::StageReport, String), McpError> {
    let report = run.report;
    let Some(document) = run.document else {
        return Ok((report, state.sidecar_revision.clone()));
    };
    let expected = state.sidecar_revision.clone();
    let revision = save_sidecar_if_unchanged(&run.sidecar_path, &document, Some(&expected))
        .map_err(map_sidecar_error)?;
    state.document = document;
    state.sidecar_revision = revision.clone();
    Ok((report, revision))
}

fn map_sidecar_error(error: SidecarError) -> McpError {
    match error {
        SidecarError::Conflict(path) => McpError::SidecarConflict(path),
        other => McpError::Sidecar(other.to_string()),
    }
}

/// The tool result: the shared stage payload (byte-identical to the CLI's
/// `--json` document) plus the MCP-specific envelope fields an agent needs —
/// `saved` (false for a read op), `action` (the applied actions) and
/// `revision` (the sidecar revision the session is now on).
pub fn result_payload(report: lumina_stages::StageReport, revision: &str) -> Value {
    let mut payload = report.payload;
    if let Some(object) = payload.as_object_mut() {
        object.insert("saved".into(), Value::Bool(report.wrote));
        object.insert("action".into(), Value::String(report.actions.join(",")));
        object.insert("revision".into(), Value::String(revision.to_owned()));
    }
    payload
}

/// Runs one shared stage editor for a session tool: resolve the session image,
/// let the editor do the work under [`Persist::Deferred`], then commit under
/// compare-and-swap. The single place every stage tool's execution goes through,
/// so all four share the session contract too.
pub fn execute<T>(
    server: &mut Server,
    tool: &str,
    image_id: &str,
    input: &std::path::Path,
    build: impl FnOnce(&str) -> T,
    run_editor: impl FnOnce(&T) -> Result<lumina_stages::StageRun, lumina_stages::StageError>,
) -> Result<Value, McpError> {
    let request = build(&input.display().to_string());
    let outcome = run_editor(&request).map_err(|error| map_stage_error(tool, error))?;
    let state = server.session.require_id_mut(image_id)?;
    let (report, revision) = commit(state, outcome)?;
    log::info!(
        "{tool}: copy={} wrote={} actions={:?} revision={revision}",
        report.copy_id,
        report.wrote,
        report.actions
    );
    Ok(result_payload(report, &revision))
}

/// Re-reads the persisted sidecar so a caller can compare a tool result against
/// what is actually on disk. Test helper (a path-based parity test can read the
/// file itself, but an in-process test needs this).
pub fn reload(path: &std::path::Path) -> Result<SidecarDocument, McpError> {
    lumina_sidecar::load_sidecar(path).map_err(|error| McpError::Sidecar(format!("{error}")))
}
