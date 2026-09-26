//! The transport-neutral result of one stage-editor call.
//!
//! Both transports render the *same* state, so the state has to be formatted
//! once. `report.payload` is the exact document the CLI prints for `--json` (and
//! the stage section the MCP tools return), `report.lines` is the exact
//! human-readable line list the CLI prints without `--json`, and
//! `report.summary` is the CLI's final one-line status. A caller therefore
//! cannot accidentally reformat a stage state for its own transport.

use lumina_sidecar::SidecarDocument;
use serde_json::Value;
use std::path::PathBuf;

/// Whether the editor persists the sidecar itself or hands the validated
/// document back to the caller.
///
/// Both paths end in the *same* `lumina_sidecar::save_sidecar_locked`, so the
/// bytes on disk are identical either way — this is a policy switch about WHO
/// writes, not about WHAT is written.
///
/// * [`Persist::Immediately`] — the CLI. The editor calls `save_sidecar`, which
///   is the pre-extraction behaviour byte for byte.
/// * [`Persist::Deferred`] — the MCP server. The editor returns the validated
///   document and the caller persists it under a compare-and-swap against the
///   revision `lumina_load` saw, exactly like `lumina_edit`. A sidecar that
///   changed on disk in between then surfaces as `SidecarConflict` (`-32010`)
///   instead of being silently overwritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persist {
    /// The editor writes the sidecar itself (the CLI path).
    Immediately,
    /// The editor returns the validated document; the caller writes it.
    Deferred,
}

/// One stage-editor result.
#[derive(Debug, Clone)]
pub struct StageReport {
    /// CLI subcommand name (`spot`, `lens-blur`, `geometry`, `upright`).
    pub command: &'static str,
    /// The resolved virtual-copy id the call acted on.
    pub copy_id: String,
    /// The applied actions, in application order (empty for read-only calls).
    pub actions: Vec<String>,
    /// The exact `--json` document / MCP tool payload.
    pub payload: Value,
    /// The exact human-readable lines (without `--json`).
    pub lines: Vec<String>,
    /// The exact final status line (without `--json`).
    pub summary: String,
    /// Whether the call was a write. A read-only call reports `false`.
    pub wrote: bool,
}

impl StageReport {
    /// Builds a report from its parts.
    pub fn new(
        command: &'static str,
        copy_id: impl Into<String>,
        actions: Vec<String>,
        payload: Value,
        lines: Vec<String>,
        summary: impl Into<String>,
        wrote: bool,
    ) -> Self {
        Self {
            command,
            copy_id: copy_id.into(),
            actions,
            payload,
            lines,
            summary: summary.into(),
            wrote,
        }
    }
}

/// What a stage editor returns: the formatted report plus, for a write, the
/// validated document and the sidecar path it belongs to.
#[derive(Debug, Clone)]
pub struct StageRun {
    /// The formatted, transport-neutral report.
    pub report: StageReport,
    /// The sidecar the call resolved. Always present, so a caller can re-check
    /// or (under [`Persist::Deferred`]) write it.
    pub sidecar_path: PathBuf,
    /// The mutated, validated document — `Some` only for a write under
    /// [`Persist::Deferred`]. `None` for a read (nothing was changed) and for a
    /// write under [`Persist::Immediately`] (the editor already saved it).
    pub document: Option<SidecarDocument>,
}
