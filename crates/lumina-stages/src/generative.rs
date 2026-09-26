//! `generative` — produce / report / remove the persisted `generative_canvas`
//! artefact of one virtual copy (`lumina generative` / `lumina_generative`) —
//! MCP-PARITY-B, GEN-ONNX-1 Welle 1.
//!
//! Path-based (one call = one path, no `image_id`) like the existing bulk
//! tools, with the CLI's own `--json` document and human line reproduced
//! verbatim through [`BulkReport`].
//!
//! # The model/artefact gate
//!
//! This command *is* the producer side of that gate, so it has several distinct
//! ways to refuse instead of faking a result:
//!
//! * `generate` with **no active role** (neither `expand` nor `auto_fill`) — a
//!   model call that would produce nothing is a loud error, not a successful
//!   no-op.
//! * `generate` where the role cannot produce a canvas: `auto_fill` with no
//!   transparent pixels after the lens stage, or `expand` without a `canvas`.
//!   No record, no link, no sidecar byte.
//! * `status` on a role whose artefact is not `available`/`not-required` — the
//!   command reports the named status and then **fails**, exactly like the CLI
//!   (whose message ends in "no silent fallback"). There is no code path in
//!   which an active-but-unproduced role is reported as a success.
//!
//! Nothing in this module ever invents an artefact, an identity digest or a
//! status: the model is the same `lumina_onnx` fixture the CLI uses, the record
//! id is the same deterministic digest, and the link is the same portable
//! [`GenerativeArtifactRef`]. The caller supplies the (optional) Lensfun
//! corrector, which needs the native capability this crate deliberately does
//! not link — the same port slice A used for the geometry Lensfun report.

use crate::decode::{decode_input, source_identity};
use crate::error::StageError;
use crate::generative_artifact::{
    auto_fill_required, generative_expand_input, parse_generative_canvas,
    persist_generative_role_canvas, produce_generative_role_canvas, stage_generative_input,
    CorrectorSource,
};
use crate::pipeline::{resolve_source_actions, sanitize_camera_white_balance};
use crate::report::{BulkReport, BulkRun, Persist};
use log::info;
use lumina_core::{
    GenerativeCanvasArtifact, GenerativeRole as CoreGenerativeRole, ImageFrame,
    LensfunCorrectorRef, SourceActionArtifact,
};
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, zdata_path_for, EditRecipe,
    GenerativeArtifactRef, GenerativeEdit, SidecarDocument,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The model identity the fixture producer reports, kept as a literal so the
/// `--status` document keeps naming the model it always named.
pub const REPORTED_MODEL: &str = "inpaint-outpaint-xl";

/// Transport-neutral `generative` request. Every field maps 1:1 onto one CLI
/// flag.
#[derive(Debug, Clone, Default)]
pub struct GenerativeRequest {
    /// Source image path; the sidecar and the `.lumina.zdata` bundle live next
    /// to it.
    pub input: String,
    /// Requested virtual-copy id (`None` = the first copy, as in the CLI).
    pub virtual_copy: Option<String>,
    /// Report the persisted artefact status (read-only).
    pub status: bool,
    /// Produce and persist the composited canvas artefact.
    pub generate: bool,
    /// Explicit regeneration: replace an existing record for the same identity.
    pub force: bool,
    /// Remove the persisted artefact link from the recipe (the bundle record
    /// and the original image are left untouched).
    pub remove: bool,
    /// Prompt (identity-bearing, roundtrip-stable; may be empty).
    pub prompt: Option<String>,
    /// Negative prompt (identity-bearing; additive schema field).
    pub negative_prompt: Option<String>,
    /// Deterministic seed (identity-bearing).
    pub seed: Option<u64>,
    /// `auto_fill_transparent`: fill transparent pixels after lens correction.
    pub auto_fill: bool,
    /// `expand_beyond_image`: enlarge the canvas (requires `canvas`).
    pub expand: bool,
    /// Target canvas `WxH+X+Y` (offsets may be negative).
    pub canvas: Option<String>,
    /// `keep_generative_content` crop decision.
    pub keep: Option<bool>,
}

/// Runs one `generative` call.
///
/// A missing sidecar is materialised in memory (exactly like the CLI); only the
/// `remove` and `generate` paths persist, through `document.validate()` and the
/// atomic `save_sidecar`.
///
/// `correctors` is the caller's Lensfun corrector source. It is asked **after**
/// the decode, with the metadata this function already read, so a caller with
/// the native capability enabled resolves the corrector exactly once — the
/// pre-extraction call order, with no second decode. `NoCorrector` is the
/// default and never guesses a correction.
pub fn run<S: CorrectorSource>(
    request: &GenerativeRequest,
    correctors: &mut S,
    persist: Persist,
) -> Result<BulkRun, StageError> {
    let input = Path::new(&request.input);
    let bytes = std::fs::read(input).map_err(|error| StageError::io(input, error))?;
    let (frame, raw_metadata) = decode_input(input, &bytes)?;
    let mut corrector_owner = S::Owner::default();
    let lensfun = correctors.build(raw_metadata.as_ref(), &mut corrector_owner);
    let wb = raw_metadata
        .as_ref()
        .and_then(|metadata| sanitize_camera_white_balance(metadata.camera_white_balance));
    let sidecar_path = sidecar_path_for(input);
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => SidecarDocument::new(
            source_identity(input, &bytes, &frame, raw_metadata.as_ref())?,
            "raster-mvp-1",
        ),
        Err(error) => return Err(error.into()),
    };
    let current_identity = source_identity(input, &bytes, &frame, raw_metadata.as_ref())?;
    if document.source.content_hash != current_identity.content_hash {
        return Err(StageError::Message(format!(
            "source changed since sidecar was written: `{}`",
            request.input
        )));
    }
    let copy_index = match request.virtual_copy.as_deref() {
        Some(id) => document
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| StageError::Message(format!("unknown virtual copy `{id}`")))?,
        None => 0,
    };
    let mut recipe = document.virtual_copies[copy_index].recipe.clone();
    let zdata_path = zdata_path_for(input);
    let bundle_root = zdata_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if request.remove {
        if let Some(edit) = recipe.generative_edit.as_mut() {
            edit.artifact = None;
        }
        document.virtual_copies[copy_index].recipe = recipe;
        info!("generative: artifact link removed (bundle record kept)");
        // The CLI's `--remove` persists unconditionally, so the MCP path does
        // too: the recipe is the artefact link, and a caller that asked to
        // remove it must not be told "nothing happened".
        document.validate()?;
        persist_and_report(
            document,
            sidecar_path,
            persist,
            BulkReport::new("generative", vec!["remove".into()], None, Vec::new(), true),
        )
    } else {
        let edit = merged_edit(request, &recipe)?;
        let seed = edit.seed.unwrap_or(0);
        let source_actions = resolve_source_actions(&recipe, &zdata_path)?;

        if request.status {
            // `--status` is read-only and fails when a role is not satisfied.
            return Ok(BulkRun {
                report: crate::generative_status::status_report(
                    &edit,
                    &frame,
                    &recipe,
                    wb,
                    &source_actions,
                    &bundle_root,
                    &zdata_path,
                    seed,
                    lensfun,
                )?,
                sidecar_path,
                document: None,
            });
        }
        if !request.generate {
            return Err(StageError::Message(
                "specify one of --status, --generate or --remove".into(),
            ));
        }
        generate(
            request,
            &mut document,
            copy_index,
            &mut recipe,
            &edit,
            &frame,
            wb,
            &source_actions,
            lensfun,
            &zdata_path,
            persist,
        )
    }
}

/// Merges the caller overrides into the persisted generative edit (additive),
/// rejecting an unsupported schema version loudly.
fn merged_edit(
    request: &GenerativeRequest,
    recipe: &EditRecipe,
) -> Result<GenerativeEdit, StageError> {
    let mut edit = recipe.generative_edit.clone().unwrap_or(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: None,
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    if edit.version != 1 {
        return Err(StageError::Message(format!(
            "unsupported generative_edit version {}; explicit migration required",
            edit.version
        )));
    }
    if let Some(prompt) = request.prompt.clone() {
        edit.prompt = Some(prompt);
    }
    if let Some(negative_prompt) = request.negative_prompt.clone() {
        edit.set_negative_prompt(Some(negative_prompt));
    }
    if let Some(seed) = request.seed {
        edit.seed = Some(seed);
    }
    if request.auto_fill {
        edit.auto_fill_transparent = Some(true);
    }
    if request.expand {
        let spec = request
            .canvas
            .as_deref()
            .ok_or_else(|| StageError::Message("--expand requires --canvas WxH+X+Y".into()))?;
        edit.expand_beyond_image = Some(true);
        edit.canvas = Some(parse_generative_canvas(spec)?);
    } else if let Some(spec) = request.canvas.as_deref() {
        edit.canvas = Some(parse_generative_canvas(spec)?);
    }
    if let Some(keep) = request.keep {
        edit.keep_generative_content = Some(keep);
    }
    Ok(edit)
}

/// Produces the canvas(es), links the canvas-defining one and persists the
/// recipe.
#[allow(clippy::too_many_arguments)]
fn generate(
    request: &GenerativeRequest,
    document: &mut SidecarDocument,
    copy_index: usize,
    recipe: &mut EditRecipe,
    edit: &GenerativeEdit,
    frame: &ImageFrame,
    wb: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    lensfun: Option<LensfunCorrectorRef<'_>>,
    zdata_path: &Path,
    persist: Persist,
) -> Result<BulkRun, StageError> {
    let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
    let expand_active = edit.effective_expand();
    if !auto_fill_active && !expand_active {
        return Err(StageError::Message(
            "no generative role active: pass --expand or --auto-fill".into(),
        ));
    }
    let (after_lens, after_perspective) =
        stage_generative_input(frame, recipe, wb, source_actions, lensfun)?;
    let relative_path = zdata_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "generative.zdata".into());

    // SOLL order `Lens → auto-fill → Perspective → expand`: the auto-fill
    // canvas is produced first; the expand canvas is then built from the
    // auto-filled frame with the perspective stage applied, so the composited
    // auto-fill pixels are authoritative in the expand canvas (GEN-ONNX-1
    // double-role record).
    let mut auto_fill_link: Option<GenerativeArtifactRef> = None;
    let mut expand_link: Option<GenerativeArtifactRef> = None;
    let mut auto_fill_frame: Option<ImageFrame> = None;
    let mut json_roles: Vec<Value> = Vec::new();
    let mut summaries: Vec<String> = Vec::new();
    if auto_fill_active {
        if !auto_fill_required(&after_lens, edit) {
            // Normative caller convention: no transparent pixels after lens →
            // identity, no artefact required (never a silent synthetic fill).
            info!(
                "generative: auto_fill active but no transparent pixels after lens; \
                 no auto-fill canvas produced (identity)"
            );
        } else {
            let output = produce_generative_role_canvas(
                &after_lens,
                edit,
                CoreGenerativeRole::AutoFillTransparent,
            )?;
            auto_fill_frame = Some(
                output
                    .to_frame()
                    .map_err(|error| StageError::Message(error.to_string()))?,
            );
            let (link, role_json, summary) =
                persist_generative_role_canvas(zdata_path, &relative_path, request.force, output)?;
            auto_fill_link = Some(link);
            json_roles.push(role_json);
            summaries.push(summary);
        }
    }
    if expand_active {
        // Mirror the render order `Lens → auto-fill → Perspective → expand` so
        // the produced expand canvas embeds the auto-filled pixels (and its
        // identity matches what the render-time resolver recomputes). The
        // pending double-role edit must be visible in the staged recipe — the
        // persisted recipe is only written below.
        let mut expand_recipe = recipe.clone();
        expand_recipe.generative_edit = Some(edit.clone());
        let expand_input = match auto_fill_frame.as_ref() {
            Some(filled) => {
                let artifact = GenerativeCanvasArtifact::new(
                    CoreGenerativeRole::AutoFillTransparent,
                    filled.clone(),
                );
                generative_expand_input(
                    frame,
                    &expand_recipe,
                    wb,
                    source_actions,
                    &artifact,
                    lensfun,
                )?
            }
            None => after_perspective.clone(),
        };
        let output =
            produce_generative_role_canvas(&expand_input, edit, CoreGenerativeRole::Expand)?;
        let (link, role_json, summary) =
            persist_generative_role_canvas(zdata_path, &relative_path, request.force, output)?;
        expand_link = Some(link);
        json_roles.push(role_json);
        summaries.push(summary);
    }
    if auto_fill_link.is_none() && expand_link.is_none() {
        return Err(StageError::Message(
            "--auto-fill: no transparent pixels after lens correction; nothing to generate \
             (no artifact written, no silent synthetic fill)"
                .into(),
        ));
    }
    // The single recipe link is the canvas-defining expand role when both are
    // active (SOLL: `Lens → GenerativeEdit → Perspective → Crop`); the
    // auto-fill record stays addressable by its deterministic identity id.
    let mut edit = edit.clone();
    edit.artifact = expand_link.or(auto_fill_link);
    recipe.generative_edit = Some(edit);
    document.virtual_copies[copy_index].recipe = recipe.clone();
    let summary = format!("generative canvas written: {}", summaries.join("; "));
    info!("generative: {summary}");
    // Preserve the documented single-role payload shape; the double-role
    // document keeps the full role list.
    let payload = if let [only] = json_roles.as_slice() {
        json!({
            "status": "generated",
            "role": only["role"],
            "width": only["width"],
            "height": only["height"],
            "record": only["record"],
            "identity": only["identity"],
            "model": only["model"],
            "model_hash": only["model_hash"],
        })
    } else {
        json!({"status": "generated", "roles": json_roles})
    };
    document.validate()?;
    persist_and_report(
        document.clone(),
        sidecar_path_for(Path::new(&request.input)),
        persist,
        BulkReport::new(
            "generative",
            vec!["generate".into()],
            Some(payload),
            vec![summary],
            true,
        ),
    )
}

/// Persists (unless the caller persists) and wraps the report.
fn persist_and_report(
    document: SidecarDocument,
    sidecar_path: PathBuf,
    persist: Persist,
    report: BulkReport,
) -> Result<BulkRun, StageError> {
    let deferred = (persist == Persist::Deferred).then(|| document.clone());
    if persist == Persist::Immediately {
        save_sidecar(&sidecar_path, &document)?;
    }
    Ok(BulkRun {
        report,
        sidecar_path,
        document: deferred,
    })
}
