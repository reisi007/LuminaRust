//! `collections` — the source-level collection memberships of one sidecar
//! (`lumina collections` / `lumina_collections`) — MCP-PARITY-B.
//!
//! Path-based, one call = one path (no `image_id`), exactly like the existing
//! bulk tools (`lumina_import`, `lumina_batch`, `lumina_reindex`,
//! `lumina_dust_removal`): it runs beside the single-image session and never
//! touches session state.
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs` (`fn collections`,
//! `split_collection_assignment`, `format_collections_text`). The write path is
//! unchanged: the same [`BatchOp`] application as the CLI, the same
//! `document.validate()` gate, the same atomic `save_sidecar`, the same
//! idempotent no-op that does not rewrite a single byte, and the same
//! `collections for \`…\` rejected: …` error channel.

use crate::error::StageError;
use crate::paths::require_sidecar;
use crate::report::{BulkReport, BulkRun, Persist};
use log::info;
use lumina_sidecar::{apply_batch_op, save_sidecar, BatchOp, CollectionMembership};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Transport-neutral `collections` request. Every field maps 1:1 onto one CLI
/// flag.
#[derive(Debug, Clone, Default)]
pub struct CollectionsRequest {
    /// Source image path; the sidecar lives next to it.
    pub input: String,
    /// Memberships to add/rename as `id=name` (repeatable, in order).
    pub add_to: Vec<String>,
    /// Membership ids to remove (repeatable, in order after `add_to`).
    pub remove_from: Vec<String>,
}

impl CollectionsRequest {
    /// True when the request can change the sidecar. A request with no
    /// membership operation is the read-only listing, like the CLI without
    /// `--add-to`/`--remove-from`.
    pub fn wants_mutation(&self) -> bool {
        !self.add_to.is_empty() || !self.remove_from.is_empty()
    }
}

/// Splits an `add_to` value at the first `=` into `(id, name)`.
pub fn split_collection_assignment(value: &str) -> Result<(String, String), StageError> {
    value.split_once('=').map_or_else(
        || {
            Err(StageError::Message(format!(
                "invalid collection assignment `{value}`: expected `id=name`"
            )))
        },
        |(id, name)| Ok((id.to_string(), name.to_string())),
    )
}

/// Runs one `collections` call: load, apply the membership operations in order,
/// validate, persist once, report. With no membership operation the call is
/// read-only and changes no byte.
pub fn run(request: &CollectionsRequest, persist: Persist) -> Result<BulkRun, StageError> {
    let input = Path::new(&request.input);
    let (path, mut document) = require_sidecar(input)?;
    let original_bytes = fs::read(input).map_err(|error| StageError::io(input, error))?;
    let mut ops = Vec::with_capacity(request.add_to.len() + request.remove_from.len());
    for assignment in &request.add_to {
        let (id, name) = split_collection_assignment(assignment)?;
        ops.push(BatchOp::AddToCollection { id, name });
    }
    for id in &request.remove_from {
        ops.push(BatchOp::RemoveFromCollection { id: id.clone() });
    }
    let mut changed = false;
    for op in &ops {
        changed |= apply_batch_op(&mut document, op).map_err(|error| {
            StageError::Message(format!(
                "collections for `{}` rejected: {error}",
                request.input
            ))
        })?;
    }
    if changed {
        document.validate()?;
        if persist == Persist::Immediately {
            save_sidecar(&path, &document)?;
        }
        info!(
            "collections for `{}` updated ({} operation(s), {} membership(s))",
            request.input,
            ops.len(),
            document.collections.len()
        );
    } else if ops.is_empty() {
        info!("collections for `{}` listed", request.input);
    } else {
        info!(
            "collections for `{}` unchanged (idempotent no-op)",
            request.input
        );
    }
    // The pre-extraction invariant: an idempotent no-op must not have touched a
    // single byte. Kept as a debug assertion exactly as before.
    debug_assert_eq!(
        fs::read(input).map_err(|error| StageError::io(input, error))?,
        original_bytes
    );
    let memberships: Vec<CollectionMembership> = document.collections.clone();
    let payload = json!({
        "command": "collections",
        "input": request.input,
        "collections": memberships,
        "changed": changed,
        "status": "ok",
    });
    let report = BulkReport::new(
        "collections",
        ops.iter().map(op_name).collect(),
        Some(payload),
        vec![format_collections_text(&memberships)],
        changed,
    );
    Ok(BulkRun {
        report,
        sidecar_path: path,
        document: (changed && persist == Persist::Deferred).then_some(document),
    })
}

/// Names one applied membership operation for the MCP envelope. The CLI has no
/// such field, so this is additive and never changes the `--json` document.
fn op_name(op: &BatchOp) -> String {
    match op {
        BatchOp::AddToCollection { id, .. } => format!("add-to:{id}"),
        BatchOp::RemoveFromCollection { id } => format!("remove-from:{id}"),
        other => format!("{other:?}"),
    }
}

/// The CLI's human-readable membership line.
pub fn format_collections_text(memberships: &[CollectionMembership]) -> String {
    if memberships.is_empty() {
        "collections: (none)".into()
    } else {
        let entries = memberships
            .iter()
            .map(|m| format!("{} ({})", m.name, m.id))
            .collect::<Vec<_>>();
        format!("collections: {}", entries.join(", "))
    }
}
