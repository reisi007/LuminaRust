//! The read-only `--status` half of `lumina generative` — split out of
//! `generative.rs` by MCP-PARITY-B so neither file grows past the 500-line
//! ratchet (no baseline entry may be added for a new file).
//!
//! `status` is the diagnosis side of the model/artefact gate: it names the
//! per-role status (`available`, `not-required`, `stale`, `missing`, `corrupt`)
//! derived from the same digest the producer wrote, and it **fails** when a role
//! is not satisfied — the CLI's exit code 1 with a message ending in
//! "no silent fallback". There is no state in which an active-but-unproduced
//! role is reported as a success.

use crate::error::StageError;
use crate::generative::REPORTED_MODEL;
use crate::generative_artifact::{
    auto_fill_required, generative_expand_input, generative_identity, generative_record_id,
    generative_role_status, required_canvas, stage_generative_input,
};
use crate::report::BulkReport;
use lumina_core::{
    GenerativeCacheKey, GenerativeCanvasArtifact, GenerativeRole as CoreGenerativeRole, ImageFrame,
    LensfunCorrectorRef, SourceActionArtifact,
};
use lumina_sidecar::{EditRecipe, GenerativeEdit};
use serde_json::{json, Value};
use std::path::Path;

/// Builds the `--status` document. A role that is not satisfied is reported by
/// name and then fails, so the caller can turn that into the CLI exit code /
/// the MCP tool error.
#[allow(clippy::too_many_arguments)]
pub fn status_report(
    edit: &GenerativeEdit,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    bundle_root: &Path,
    zdata_path: &Path,
    seed: u64,
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<BulkReport, StageError> {
    let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
    let expand_active = edit.effective_expand();
    if !auto_fill_active && !expand_active {
        return Ok(BulkReport::new(
            "generative",
            Vec::new(),
            Some(json!({ "status": "inactive" })),
            vec!["generative: inactive (no role active)".into()],
            false,
        ));
    }
    let (after_lens, after_perspective) =
        stage_generative_input(frame, recipe, camera_white_balance, source_actions, lensfun)?;
    let link = edit.artifact.as_ref();
    let link_role = if expand_active {
        Some(CoreGenerativeRole::Expand)
    } else {
        Some(CoreGenerativeRole::AutoFillTransparent)
    };
    let role_link = |role: CoreGenerativeRole| link.filter(|_| link_role == Some(role));
    // (role, identity digest, status, record id)
    let mut roles: Vec<(&'static str, Option<String>, String, Option<String>)> = Vec::new();
    let mut auto_fill_frame: Option<ImageFrame> = None;
    let auto_fill_is_required = auto_fill_required(&after_lens, edit);
    if auto_fill_active {
        if !auto_fill_is_required {
            roles.push(("AutoFillTransparent", None, "not-required".to_owned(), None));
        } else {
            let identity = generative_identity(CoreGenerativeRole::AutoFillTransparent, edit);
            let digest = GenerativeCacheKey::auto_fill(&after_lens, seed, &identity).digest();
            let owns_link = link_role == Some(CoreGenerativeRole::AutoFillTransparent);
            let (status, resolved) = generative_role_status(
                bundle_root,
                zdata_path,
                role_link(CoreGenerativeRole::AutoFillTransparent),
                owns_link,
                &digest,
            );
            auto_fill_frame = resolved;
            roles.push((
                "AutoFillTransparent",
                Some(digest.clone()),
                status,
                Some(generative_record_id(&digest)),
            ));
        }
    }
    if expand_active {
        if auto_fill_is_required && auto_fill_frame.is_none() {
            // The expand identity embeds the auto-filled pixels; without the
            // auto-fill canvas its status cannot be verified.
            roles.push(("Expand", None, "missing".to_owned(), None));
        } else {
            let canvas = required_canvas(edit).map_err(|_| {
                StageError::Message("expand_beyond_image requires a `canvas`".into())
            })?;
            let expand_input = match auto_fill_frame.as_ref() {
                Some(filled) => {
                    let artifact = GenerativeCanvasArtifact::new(
                        CoreGenerativeRole::AutoFillTransparent,
                        filled.clone(),
                    );
                    generative_expand_input(
                        frame,
                        recipe,
                        camera_white_balance,
                        source_actions,
                        &artifact,
                        lensfun,
                    )?
                }
                None => after_perspective.clone(),
            };
            let identity = generative_identity(CoreGenerativeRole::Expand, edit);
            let digest =
                GenerativeCacheKey::expand(&expand_input, canvas, seed, &identity).digest();
            let (status, _) = generative_role_status(
                bundle_root,
                zdata_path,
                role_link(CoreGenerativeRole::Expand),
                link_role == Some(CoreGenerativeRole::Expand),
                &digest,
            );
            roles.push((
                "Expand",
                Some(digest.clone()),
                status,
                Some(generative_record_id(&digest)),
            ));
        }
    }
    let ok = |status: &str| status == "available" || status == "not-required";
    let failed: Vec<String> = roles
        .iter()
        .filter(|(_, _, status, _)| !ok(status))
        .map(|(role, _, status, _)| format!("{role}={status}"))
        .collect();
    let combined = if failed.is_empty() {
        if roles.iter().any(|(_, _, status, _)| status == "available") {
            "available".to_owned()
        } else {
            "not-required".to_owned()
        }
    } else {
        failed.join(", ")
    };
    let (payload, line) = if let [role] = roles.as_slice() {
        let (role, identity, status, record) = role;
        (
            json!({
                "status": status,
                "role": role,
                "identity": identity,
                "record": record,
                "model": REPORTED_MODEL,
            }),
            format!("generative: status={status} role={role}"),
        )
    } else {
        let role_json: Vec<Value> = roles
            .iter()
            .map(|(role, identity, status, record)| {
                json!({
                    "role": role,
                    "status": status,
                    "identity": identity,
                    "record": record,
                })
            })
            .collect();
        let detail = roles
            .iter()
            .map(|(role, _, status, _)| format!("{role}:{status}"))
            .collect::<Vec<_>>()
            .join(", ");
        (
            json!({
                "status": combined,
                "roles": role_json,
                "model": REPORTED_MODEL,
            }),
            format!("generative: status={combined} roles={detail}"),
        )
    };
    if !failed.is_empty() {
        return Err(StageError::Message(format!(
            "generative canvas is `{combined}` (no silent fallback; run `lumina generative --generate`)"
        )));
    }
    Ok(BulkReport::new(
        "generative",
        Vec::new(),
        Some(payload),
        vec![line],
        false,
    ))
}
