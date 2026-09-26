//! Shared plumbing for the five **path-based** MCP tools of MCP-PARITY-B:
//! `lumina_collections`, `lumina_smart_collections`, `lumina_relocate`,
//! `lumina_generative`, `lumina_regenerate`.
//!
//! # Path-based, like the existing bulk tools
//!
//! These take a `path` (or `from`/`to`), not the session's `image_id`: they work
//! on the **library**, not on the loaded image, and they run beside a loaded
//! image without touching session state — the established `lumina_import` /
//! `lumina_batch` / `lumina_reindex` / `lumina_dust_removal` pattern. One call is
//! one path.
//!
//! # The write path is the CLI's
//!
//! They call the same `lumina_stages::*::run` the CLI calls with
//! [`lumina_stages::Persist::Immediately`], so the bytes on disk come from the
//! same `lumina_sidecar::save_sidecar` in both transports — that is the
//! *structural* half of the byte-identity claim, and
//! `crates/lumina-cli/tests/library_parity.rs` measures it over the real
//! `lumina-cli` binary.
//!
//! **Documented divergence:** these tools do **not** participate in the session's
//! compare-and-swap (`lumina_edit`, `lumina_update_metadata_draft`), because
//! they are not session tools and a caller may address a path that was never
//! loaded. A concurrent external change is therefore overwritten rather than
//! reported as `SidecarConflict` — the same behaviour as the CLI and as
//! `lumina_dust_removal`. A caller that needs the CAS contract uses
//! `lumina_edit` on a loaded image.

use crate::error::McpError;
use crate::tools::stage_common::map_stage_error;
use lumina_stages::report::BulkReport;
use lumina_stages::Persist;
use serde_json::Value;

/// The persist mode the path-based tools use: the shared code writes, exactly as
/// the CLI does. See the module docs for the documented CAS divergence.
pub const PATH_PERSIST: Persist = Persist::Immediately;

/// The tool result: the shared command payload (byte-identical to the CLI's
/// `--json` document) plus the MCP-specific envelope fields an agent needs —
/// `saved` (false for a read), `action` (the applied actions) and `status`
/// (the command's own status word).
///
/// A command that prints no `--json` document on its path (`generative
/// op="remove"`) still returns the envelope, so an agent always learns *what*
/// happened instead of receiving an empty object.
pub fn result_payload(report: BulkReport) -> Value {
    let mut payload = report
        .payload
        .unwrap_or_else(|| Value::Object(Default::default()));
    if let Some(object) = payload.as_object_mut() {
        object
            .entry("status")
            .or_insert_with(|| Value::String(if report.wrote { "ok" } else { "unchanged" }.into()));
    }
    let mut envelope = match payload {
        Value::Object(object) => object,
        other => {
            let mut object = serde_json::Map::new();
            object.insert("result".into(), other);
            object
        }
    };
    envelope.insert("saved".into(), Value::Bool(report.wrote));
    envelope.insert("action".into(), Value::String(report.actions.join(",")));
    if !report.lines.is_empty() {
        envelope.insert("text".into(), Value::String(report.lines.join("\n")));
    }
    Value::Object(envelope)
}

/// Maps a shared [`lumina_stages::StageError`] onto the MCP error model, keeping
/// the loudness: every variant becomes a tool error with `isError: true`, and a
/// rejection is never reported as a successful empty result.
pub fn map_error(tool: &str, error: lumina_stages::StageError) -> McpError {
    map_stage_error(tool, error)
}
