//! `upright` — the LRPAR-G06-UPRIGHT-15 automatic-upright stage editor
//! (`lumina upright` / `lumina_upright`).
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs` (`fn upright`,
//! `upright_mut`, `upright_list`). The analysis is classic, model-free and
//! deterministic (`lumina_core::analyze_upright`), and `--analyze` binds the
//! result to the current source identity through the shared
//! `upright_input_fingerprint`, so a changed source is reported as **stale**,
//! never silently recomputed. Every mutation validates loudly and appends
//! exactly one history entry.
//!
//! `upright --enable` without a persisted analysis is a loud rejection ("no
//! persisted upright analysis; run `upright --analyze` first") and writes
//! nothing — the same contract the MCP tool enforces.

use crate::copy::{copy_mut, copy_ref, resolve_copy};
use crate::decode::{read_and_decode, source_identity, timestamp};
use crate::error::StageError;
use crate::report::{Persist, StageReport, StageRun};
use log::info;
use lumina_core::{analyze_upright, upright_analysis, upright_input_fingerprint};
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, HistoryEntry, SidecarDocument, Upright,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Transport-neutral `upright` request. Every field maps 1:1 onto one CLI flag.
#[derive(Debug, Clone, Default)]
pub struct UprightRequest {
    /// Source image path; the sidecar lives next to it.
    pub input: String,
    /// Requested virtual-copy id (`None` = default copy).
    pub virtual_copy: Option<String>,
    /// List the persisted upright stage and its freshness (read-only).
    pub list: bool,
    /// Run the deterministic `upright-lines-v1` analysis and persist it.
    pub analyze: bool,
    /// Apply the persisted analysis as the effective F-099 perspective.
    pub enable: bool,
    /// Stop applying the persisted analysis; the manual perspective returns.
    pub disable: bool,
    /// Remove the whole upright stage (analysis included).
    pub clear: bool,
}

impl UprightRequest {
    /// True when the request can change the sidecar.
    pub fn wants_mutation(&self) -> bool {
        self.analyze || self.enable || self.disable || self.clear
    }
}

/// Runs one `upright` call: conflict matrix, load, mutate, validate, one history
/// entry, persist, report.
pub fn run(request: &UprightRequest, persist: Persist) -> Result<StageRun, StageError> {
    if request.enable && request.disable {
        return Err(StageError::Message(
            "--enable and --disable are mutually exclusive".into(),
        ));
    }
    if request.analyze && request.clear {
        return Err(StageError::Message(
            "--analyze and --clear are mutually exclusive".into(),
        ));
    }
    let wants_mutation = request.wants_mutation();
    if request.list && wants_mutation {
        return Err(StageError::Message(
            "--list is read-only; pass no mutation flags with it".into(),
        ));
    }
    let path = sidecar_path_for(Path::new(&request.input));
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(StageError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                request.input
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_copy(&document, request.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();

    if request.clear {
        copy_mut(&mut document, &copy_id)?.recipe.upright = None;
        info!("upright: cleared stage on copy `{copy_id}`");
        actions.push("upright:clear".into());
    } else {
        if request.analyze {
            let (bytes, frame, raw) = read_and_decode(Path::new(&request.input))?;
            let identity =
                source_identity(Path::new(&request.input), &bytes, &frame, raw.as_ref())?;
            let fingerprint = upright_input_fingerprint(
                &identity.content_hash,
                frame.width,
                frame.height,
                identity.orientation,
            );
            let suggestion = analyze_upright(&frame);
            info!(
                "upright: analyzed copy `{copy_id}` ({} line pixels, confidence {:.3}, \
                 vertical {:.3}, horizontal {:.3}, rotation {:.3})",
                suggestion.line_count,
                suggestion.confidence,
                suggestion.vertical,
                suggestion.horizontal,
                suggestion.rotation
            );
            // `--analyze` applies the fresh suggestion; `--disable` in the same
            // call keeps it persisted but inactive.
            let enabled = !request.disable;
            copy_mut(&mut document, &copy_id)?.recipe.upright = Some(Upright {
                version: 1,
                enabled,
                analysis: Some(upright_analysis(suggestion, fingerprint)),
            });
            actions.push("upright:analyze".into());
            actions.push(format!(
                "upright:{}",
                if enabled { "enable" } else { "disable" }
            ));
        } else if request.enable || request.disable {
            let enabled = request.enable;
            let upright = stage_mut(&mut document, &copy_id)?;
            if upright.analysis.is_none() {
                return Err(StageError::Message(
                    "no persisted upright analysis; run `upright --analyze` first".into(),
                ));
            }
            upright.enabled = enabled;
            info!(
                "upright: {} on copy `{copy_id}`",
                if enabled { "enabled" } else { "disabled" }
            );
            actions.push(format!(
                "upright:{}",
                if enabled { "enable" } else { "disable" }
            ));
        }
    }

    if wants_mutation {
        document.validate()?;
        let copy = copy_mut(&mut document, &copy_id)?;
        let final_recipe = copy.recipe.clone();
        let mut id = format!("upright-{}", timestamp());
        let mut suffix = 0u32;
        while copy.history.iter().any(|entry| entry.id == id) {
            suffix += 1;
            id = format!("upright-{}-{suffix}", timestamp());
        }
        let mut extras = BTreeMap::new();
        extras.insert("step".into(), Value::String("upright".into()));
        extras.insert("actions".into(), Value::String(actions.join(",")));
        copy.history.push(HistoryEntry {
            id,
            recipe: final_recipe,
            recorded_at: Some(timestamp()),
            extras,
        });
        if persist == Persist::Immediately {
            save_sidecar(&path, &document)?;
        }
    }
    let report = list(request, &document, &copy_id, &actions, wants_mutation)?;
    let document = (wants_mutation && persist == Persist::Deferred).then_some(document);
    Ok(StageRun {
        report,
        sidecar_path: path,
        document,
    })
}

/// Mutable access to one virtual copy's upright stage, creating an empty
/// (disabled, no analysis) stage when none exists.
pub fn stage_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Upright, StageError> {
    let copy = copy_mut(document, copy_id)?;
    Ok(copy.recipe.upright.get_or_insert(Upright {
        version: 1,
        enabled: false,
        analysis: None,
    }))
}

/// Read-only upright status: persisted stage plus `fresh`/`stale` vs. the
/// current source fingerprint. A stale analysis is reported, never silently
/// recomputed (SOLL: Identität/Veraltung).
fn list(
    request: &UprightRequest,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
    wrote: bool,
) -> Result<StageReport, StageError> {
    let copy = copy_ref(document, copy_id)?;
    let stage = copy.recipe.upright.as_ref();
    // Current fingerprint from the sidecar source identity geometry; a change
    // to the source bytes flips to `stale` loudly.
    let current_content_hash = match fs::read(&request.input) {
        Ok(bytes) => format!("blake3:{}", blake3::hash(&bytes).to_hex()),
        Err(_) => document.source.content_hash.clone(),
    };
    let current = upright_input_fingerprint(
        &current_content_hash,
        document.source.geometry_fingerprint.width,
        document.source.geometry_fingerprint.height,
        document.source.orientation,
    );
    let status = match stage.and_then(|stage| stage.analysis.as_ref()) {
        None => "none",
        Some(analysis) if analysis.fingerprint.input_fingerprint == current => "fresh",
        Some(_) => "stale",
    };
    let payload = json!({
        "command": "upright",
        "input": request.input,
        "copy": copy_id,
        "enabled": stage.map(|stage| stage.enabled),
        "status": status,
        "upright": stage,
        "actions": actions,
    });
    let mut lines: Vec<String> = Vec::new();
    match stage {
        None => lines.push("  upright: none".into()),
        Some(stage) => {
            lines.push(format!(
                "  upright: enabled={} status={status}",
                stage.enabled
            ));
            if let Some(analysis) = &stage.analysis {
                lines.push(format!(
                    "    analysis: {} v{} vertical={} horizontal={} rotation={} \
                     lines={} confidence={}",
                    analysis.fingerprint.algorithm,
                    analysis.fingerprint.version,
                    analysis.vertical,
                    analysis.horizontal,
                    analysis.rotation,
                    analysis.line_count,
                    analysis.confidence
                ));
                lines.push(format!(
                    "    fingerprint: {}",
                    analysis.fingerprint.input_fingerprint
                ));
            }
        }
    }
    let summary = if actions.is_empty() {
        "upright status listed".to_string()
    } else {
        "upright updated".to_string()
    };
    Ok(StageReport::new(
        "upright",
        copy_id,
        actions.to_vec(),
        payload,
        lines,
        summary,
        wrote,
    ))
}
