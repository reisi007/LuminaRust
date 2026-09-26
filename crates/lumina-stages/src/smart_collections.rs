//! `smart-collections` — evaluate a portable smart-collection catalogue against
//! the sidecars under a path (`lumina smart-collections` /
//! `lumina_smart_collections`) — MCP-PARITY-B.
//!
//! Path-based (one call = one path, no `image_id`) like the existing bulk
//! tools. **Read-only by contract**: the command evaluates rules and reports
//! which sidecar matches which collection id; it never writes a sidecar, so
//! both transports leave the tree byte-identical.
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs` (`fn
//! smart_collections`). The per-item isolation is unchanged: an unreadable or
//! corrupt sidecar, and a rule whose version the document does not speak, are
//! recorded per item and make the run **partial** — never a silently empty
//! success.

use crate::error::StageError;
use crate::paths::{collect_target_sidecars, load_smart_catalog};
use crate::report::{BulkReport, BulkRun};
use log::info;
use lumina_sidecar::load_sidecar;
use serde_json::{json, Value};
use std::path::Path;

/// Transport-neutral `smart-collections` request. Every field maps 1:1 onto one
/// CLI flag.
#[derive(Debug, Clone, Default)]
pub struct SmartCollectionsRequest {
    /// A sidecar file, an image, or a directory to scan recursively.
    pub input: String,
    /// The portable catalogue file
    /// (`{"format":"lumina-smart-catalog","version":1,"collections":[...]}`).
    pub catalog: String,
}

/// How many sidecars could not be evaluated. The caller turns a non-zero count
/// into the CLI's partial-failure exit / the MCP tool's error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialFailures {
    pub failed: usize,
}

impl PartialFailures {
    /// True when at least one sidecar failed.
    pub fn is_partial(self) -> bool {
        self.failed != 0
    }
}

/// Runs one `smart-collections` call: load and validate the catalogue, collect
/// the target sidecars deterministically, then evaluate every rule against
/// every virtual copy of every document.
///
/// The report is always complete — the caller decides what a partial run means
/// for its transport, so the CLI's exit code 3 and the MCP tool's error both
/// come from the same measurement.
pub fn run(request: &SmartCollectionsRequest) -> Result<(BulkRun, PartialFailures), StageError> {
    let defs = load_smart_catalog(Path::new(&request.catalog))?;
    let targets = collect_target_sidecars(Path::new(&request.input))?;
    if targets.is_empty() {
        return Err(StageError::Message(format!(
            "no sidecars found under `{}`",
            request.input
        )));
    }
    let mut matched_files = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items: Vec<Value> = Vec::with_capacity(targets.len());
    for sidecar in &targets {
        let path = sidecar.display().to_string();
        match load_sidecar(sidecar) {
            Err(error) => {
                let message = format!("{path}: {error}");
                eprintln!("error: smart-collections: {message}");
                info!("smart-collections: `{}` failed", sidecar.display());
                failures.push(message);
                items.push(json!({"sidecar": path, "status":"failed"}));
            }
            Ok(document) => {
                let mut matched: Vec<String> = Vec::new();
                let mut item_failed: Option<String> = None;
                for def in &defs {
                    match def.matches_any_copy(&document) {
                        Ok(true) => matched.push(def.id.clone()),
                        Ok(false) => {}
                        Err(error) => {
                            item_failed = Some(format!("{path}: {error}"));
                            break;
                        }
                    }
                }
                match item_failed {
                    Some(message) => {
                        eprintln!("error: smart-collections: {message}");
                        info!("smart-collections: `{}` failed", sidecar.display());
                        failures.push(message);
                        items.push(json!({"sidecar": path, "status":"failed"}));
                    }
                    None => {
                        if !matched.is_empty() {
                            matched_files += 1;
                        }
                        info!(
                            "smart-collections: `{}` matches {} collection(s)",
                            sidecar.display(),
                            matched.len()
                        );
                        items.push(json!({"sidecar": path, "status":"ok", "matches": matched}));
                    }
                }
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "smart-collections: {} of {} sidecar(s) match, {failed} failed",
        matched_files,
        targets.len()
    );
    let payload = json!({
        "command": "smart-collections",
        "input": request.input,
        "catalog": request.catalog,
        "matched_files": matched_files,
        "sidecars": targets.len(),
        "failed": failed,
        "errors": failures,
        "items": items,
        "status": if failed == 0 { "ok" } else { "partial" },
    });
    info!("{text}");
    let report = BulkReport::new(
        "smart-collections",
        Vec::new(),
        Some(payload),
        vec![text],
        false,
    );
    Ok((
        BulkRun {
            report,
            sidecar_path: targets[0].clone(),
            document: None,
        },
        PartialFailures { failed },
    ))
}
