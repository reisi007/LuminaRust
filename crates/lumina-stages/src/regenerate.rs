//! `regenerate` — explicit per-module regeneration of the 1.0 derivable
//! AI/analysis values (`lumina regenerate` / `lumina_regenerate`) —
//! MCP-PARITY-B, GUI-GEN-GRANULAR-10 (F-100).
//!
//! Path-based (one call = one path, no `image_id`) like the existing bulk
//! tools. The three modules are handled in dependency order and each one is
//! **independent**: a single-module call never touches another module's
//! persisted state, the collective call only touches stale/missing values, and
//! nothing is recomputed without an explicit request.
//!
//! # The artefact gate
//!
//! `op="matching"` is the module that *loads an artefact*: it re-derives
//! `matched_exposure` from an actual render of the current recipe, with the
//! persisted mask planes and the recipe's source actions resolved exactly as a
//! real render resolves them. Two consequences, both loud:
//!
//! * an active generative role whose artefact cannot be produced or resolved
//!   makes that render fail (the core stage reports
//!   `generative_artifact.*.missing`), so the module aborts and the sidecar is
//!   not written — the tool can never report a derived `matched_exposure` for a
//!   frame the renderer refused to produce;
//! * a corrupt `.lumina.zdata` bundle and an unresolvable source action are
//!   reported, not skipped.
//!
//! `op="masks"` is a **refresh request**, not an inference: it marks the
//! selected source masks `Pending` and (for an explicit call) arms the
//! copy-wide one-shot flag. It never writes a stub matte as a valid artefact —
//! the `.lumina.zdata` mask persistence is the documented F-082 open item.
//!
//! # The render port
//!
//! The `matching` module needs a render, and the CLI's render entry
//! ([`lumina-cli`'s `render_standard_with_generative`]) carries its optional GPU
//! routing, which this crate deliberately does not link. So the shared code owns
//! the *decision and the mutation* and takes the render as a port
//! ([`MatchingRender`]). The CPU oracle ([`CpuRender`]) is the default and is
//! what the MCP server uses; the CLI passes its own entry so its behaviour —
//! including GPU routing — is unchanged. In a build without the `gpu` feature
//! the two are the same call into `lumina_core::render_frame`, which is the
//! configuration the byte-identity test measures.

use crate::auto_tone::{
    apply_auto_tone_result, auto_tone_input_fingerprint, auto_tone_is_fresh, PersistedAutoTone,
};
use crate::decode::{decode_input, source_identity};
use crate::error::StageError;
use crate::generative_artifact::CorrectorSource;
use crate::pipeline::{
    load_persisted_mask_planes, resolve_source_actions, sanitize_camera_white_balance,
};
use crate::report::{BulkReport, BulkRun, Persist};
use log::info;
use lumina_core::{
    match_total_exposure_masked, MaskContext, MaskPlane, MaskPolicy, RenderContext, RenderOutput,
};
use lumina_sidecar::{
    artifact_status, load_sidecar, save_sidecar, sidecar_path_for, zdata_path_for, ArtifactStatus,
    MaskDefinition, MaskOperation, MaskStatus, SidecarDocument, SourceIdentity,
};
use serde_json::{json, Value};
use std::path::Path;

/// The one regenerable module of the F-100 list (`masks`, `auto-tone`,
/// `matching`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegenerateModule {
    /// AI mask inference. Regeneration is an explicit refresh request
    /// (status `Pending`) consumed by the next render; the `.lumina.zdata`
    /// artifact persistence is the documented F-082 open item, so no stub
    /// matte is ever persisted as a valid artifact.
    Masks,
    /// Auto-Tone: the six sliders plus the AUTO-TONE-2 mirrors and the
    /// analysis fingerprint.
    AutoTone,
    /// F-008 Exposure Matching (the persisted `matched_exposure`).
    Matching,
}

impl RegenerateModule {
    /// The `--module` value and the `modules[].module` report string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Masks => "masks",
            Self::AutoTone => "auto-tone",
            Self::Matching => "matching",
        }
    }

    /// Parses one `--module` value. An unknown word is a loud rejection.
    pub fn parse(value: &str) -> Result<Self, StageError> {
        match value {
            "masks" => Ok(Self::Masks),
            "auto-tone" => Ok(Self::AutoTone),
            "matching" => Ok(Self::Matching),
            other => Err(StageError::Message(format!(
                "invalid --module `{other}`: expected one of masks|auto-tone|matching"
            ))),
        }
    }

    /// The three modules in dependency order.
    pub const ALL: [Self; 3] = [Self::Masks, Self::AutoTone, Self::Matching];
}

/// The render entry the `matching` module needs.
///
/// The shared crate owns the [`RenderContext`] it hands over; only the *route*
/// (CPU oracle vs the caller's accelerator) is the caller's choice. Both
/// implementations must be handed the identical context, which is why the
/// context is built here.
pub trait MatchingRender {
    /// Renders `frame` with `context` and returns the output. An error aborts
    /// the module loudly; there is no "render could not happen" fallback.
    fn render(
        &self,
        frame: &lumina_core::ImageFrame,
        context: &RenderContext<'_>,
    ) -> Result<RenderOutput, StageError>;
}

/// The default port: the CPU oracle. `lumina_core::render_frame` is the
/// reference pipeline (and, in a build without the `gpu` feature, literally the
/// same call the CLI makes).
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuRender;

impl MatchingRender for CpuRender {
    fn render(
        &self,
        frame: &lumina_core::ImageFrame,
        context: &RenderContext<'_>,
    ) -> Result<RenderOutput, StageError> {
        Ok(lumina_core::render_frame(frame, context)?)
    }
}

/// Transport-neutral `regenerate` request. Every field maps 1:1 onto one CLI
/// flag.
#[derive(Debug, Clone, Default)]
pub struct RegenerateRequest {
    /// Source image path; the sidecar lives next to it.
    pub input: String,
    /// Requested virtual-copy id (`None` = the first copy, as in the CLI).
    pub virtual_copy: Option<String>,
    /// Modules to regenerate. Empty = the collective default (every stale or
    /// missing module).
    pub modules: Vec<RegenerateModule>,
    /// Target luminance of the tone analysis and the exposure matching
    /// (`0..=1`; the CLI default is 0.5).
    pub target_luminance: f64,
}

/// What one `regenerate` call produced: the report plus the mutated document
/// for a caller that persists it itself.
pub type RegenerateRun = BulkRun;

/// Runs one `regenerate` call.
///
/// The sidecar is written **only** when something actually changed, so an
/// idempotent collective run leaves the file byte-identical.
///
/// `lensfun` is the caller-resolved corrector for this source: the corrector
/// needs the optional native `lensfun` capability that `lumina-stages`
/// deliberately does not link, so the *caller* resolves it and hands it in — the
/// same port slice A used for the geometry Lensfun report. `None` is a real "no
/// corrector applies" answer, never a guess.
pub fn run<S: CorrectorSource, R: MatchingRender + ?Sized>(
    request: &RegenerateRequest,
    correctors: &mut S,
    renderer: &R,
    persist: Persist,
) -> Result<RegenerateRun, StageError> {
    if !request.target_luminance.is_finite() || !(0.0..=1.0).contains(&request.target_luminance) {
        return Err(StageError::Message(
            "invalid target-luminance: must be finite and in 0..=1".into(),
        ));
    }
    let input = Path::new(&request.input);
    let bytes = std::fs::read(input).map_err(|error| StageError::io(input, error))?;
    let (frame, raw_metadata) = decode_input(input, &bytes)?;
    let mut corrector_owner = S::Owner::default();
    let lensfun = correctors.build(raw_metadata.as_ref(), &mut corrector_owner);
    let wb = raw_metadata.as_ref().and_then(|m| {
        let sanitized = sanitize_camera_white_balance(m.camera_white_balance);
        if sanitized.is_none() {
            eprintln!(
                "lumina: warning: As-Shot white balance invalid {:?} for `{}` — dropping to None (recipe WB remains, image renders)",
                m.camera_white_balance,
                request.input
            );
        }
        sanitized
    });
    let sidecar_path = sidecar_path_for(input);
    // Current source identity, reused for a freshly created sidecar and for
    // the mask stale pre-filter (N1).
    let current_identity = source_identity(input, &bytes, &frame, raw_metadata.as_ref())?;
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            SidecarDocument::new(current_identity.clone(), "raster-mvp-1")
        }
        Err(error) => return Err(error.into()),
    };
    let copy_index = match request.virtual_copy.as_deref() {
        Some(id) => document
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| StageError::Message(format!("unknown virtual copy `{id}`")))?,
        None => 0,
    };
    let copy_id = document.virtual_copies[copy_index].id.clone();
    // `all` = collective default (no `--module`); otherwise only the named
    // modules run, and those are forced (the explicit user request).
    let collective = request.modules.is_empty();
    let selected = |module: RegenerateModule| request.modules.contains(&module);
    let wants = |module: RegenerateModule| collective || selected(module);
    let forced = |module: RegenerateModule| selected(module);
    let mut changed = false;
    let mut report: Vec<Value> = Vec::new();
    let mut actions: Vec<String> = Vec::new();

    // ---- Module `masks` ----
    if wants(RegenerateModule::Masks) {
        let root = input.parent().unwrap_or_else(|| Path::new("."));
        let mask_ids: Vec<String> = document.virtual_copies[copy_index]
            .mask_library
            .iter()
            // N2: deterministic range prompts carry no artifact and are always
            // reproducible from pixels — they are excluded even by the forced
            // `--module masks` path (a refresh request for them is meaningless).
            .filter(|mask| {
                matches!(mask.operation, MaskOperation::Source)
                    && !lumina_core::range_masks::is_range_prompt(mask.prompt.as_ref())
            })
            .filter(|mask| {
                forced(RegenerateModule::Masks)
                    || mask_needs_regeneration(root, &current_identity, mask)
            })
            .map(|mask| mask.id.clone())
            .collect();
        if mask_ids.is_empty() {
            report.push(json!({
                "module": RegenerateModule::Masks.as_str(),
                "action": "skipped",
                "reason": if forced(RegenerateModule::Masks) { "no source masks" } else { "fresh" },
            }));
        } else {
            let mut masks_changed = false;
            for mask in document.virtual_copies[copy_index].mask_library.iter_mut() {
                if mask_ids.contains(&mask.id) && mask.status != MaskStatus::Pending {
                    // An explicit, persisted refresh request. The actual
                    // re-inference is consumed by the next render; the zdata
                    // artifact persistence is the documented F-082 open item,
                    // so no stub matte is ever written as a valid artifact.
                    mask.status = MaskStatus::Pending;
                    masks_changed = true;
                }
            }
            // M2: an explicit `--module masks` additionally arms the copy-wide
            // ONE-SHOT refresh in the recipe so the next render's decision layer
            // re-infers the complete module with `refresh == true` and does NOT
            // report the deliberately requested work as an implicit
            // re-inference. `process_selected` consumes and removes the flag
            // after the successful render (REVIEW-CLI-MASKFLAG-1).
            //
            // M2b: the collective default deliberately does NOT arm it. The
            // copy-wide flag overrides the persisted-valid fastpath for *every*
            // reachable source mask, so arming it here would also re-infer fresh
            // `Valid` masks — contradicting "nur veraltete oder fehlende" and
            // the GUI (`regenerate_stale` marks only stale masks). The `Pending`
            // markers set above are the per-mask refresh request; the decision
            // layer treats a persisted `Pending` marker as explicitly requested
            // and stays quiet for it.
            if forced(RegenerateModule::Masks) {
                let options = &mut document.virtual_copies[copy_index].recipe.options;
                if options.get("update_masks").map(String::as_str) != Some("true") {
                    options.insert("update_masks".into(), "true".into());
                    masks_changed = true;
                }
            }
            // An already-outstanding `Pending` request is idempotent: the
            // sidecar is not rewritten when nothing transitions.
            changed |= masks_changed;
            actions.push(format!("masks:{}", mask_ids.len()));
            report.push(json!({
                "module": RegenerateModule::Masks.as_str(),
                "action": "requested",
                "reason": if forced(RegenerateModule::Masks) { "explicit" } else { "stale-or-missing" },
                "masks": mask_ids,
            }));
        }
    }

    // ---- Module `auto-tone` ----
    if wants(RegenerateModule::AutoTone) {
        let current = &document.virtual_copies[copy_index].recipe.auto_features;
        // Fresh = enabled AND the *full* AUTO-TONE-2 contract is persisted:
        // all six sliders, all six `auto_features` mirrors and the analysis
        // fingerprint. AUTO-TONE-CLI-6: the predicate is the shared
        // `auto_tone_is_fresh`, which `process --auto-tone` satisfies as well —
        // a recipe written by `process` is therefore fresh and is NOT
        // overwritten here. An incomplete contract (the historic two-slider
        // `exposure`/`contrast` subset, a missing mirror or slider, or a
        // non-matching fingerprint) is stale and regenerated.
        let fingerprint = auto_tone_input_fingerprint(&frame, request.target_luminance);
        let fresh = auto_tone_is_fresh(&document.virtual_copies[copy_index].recipe, &fingerprint);
        if fresh && !forced(RegenerateModule::AutoTone) {
            report.push(json!({
                "module": RegenerateModule::AutoTone.as_str(),
                "action": "skipped",
                "reason": "fresh",
            }));
        } else if !current.enable_auto_tone && !forced(RegenerateModule::AutoTone) {
            // Deliberately disabled by the user: the collective default never
            // turns it on behind their back.
            report.push(json!({
                "module": RegenerateModule::AutoTone.as_str(),
                "action": "skipped",
                "reason": "not-enabled",
            }));
        } else {
            let mut recipe = document.virtual_copies[copy_index].recipe.clone();
            // Always recompute: an explicit regeneration must derive the values
            // from the frame again, never reproduce a persisted one.
            apply_auto_tone_result(
                &mut recipe,
                &frame,
                request.target_luminance,
                PersistedAutoTone::AlwaysRecompute,
                None,
            )?;
            document.virtual_copies[copy_index].recipe = recipe;
            changed = true;
            actions.push("auto-tone:generated".into());
            report.push(json!({
                "module": RegenerateModule::AutoTone.as_str(),
                "action": "generated",
                "reason": if forced(RegenerateModule::AutoTone) { "explicit" } else { "stale-or-missing" },
            }));
        }
    }

    // ---- Module `matching` ----
    if wants(RegenerateModule::Matching) {
        let current = &document.virtual_copies[copy_index].recipe.auto_features;
        let fresh = current.match_total_exposure && current.matched_exposure.is_some();
        if fresh && !forced(RegenerateModule::Matching) {
            report.push(json!({
                "module": RegenerateModule::Matching.as_str(),
                "action": "skipped",
                "reason": "fresh",
            }));
        } else if !current.match_total_exposure && !forced(RegenerateModule::Matching) {
            report.push(json!({
                "module": RegenerateModule::Matching.as_str(),
                "action": "skipped",
                "reason": "not-enabled",
            }));
        } else {
            let mut recipe = document.virtual_copies[copy_index].recipe.clone();
            let zdata_path = zdata_path_for(input);
            let mut ignored_warnings = Vec::new();
            // Deliberately no mask decision layer here: regenerating the
            // matching value must not re-infer masks (that is the `masks`
            // module). All persisted planes are handed to the render, but
            // `MaskContext`/`render_frame` evaluates only `MaskStatus::Valid`
            // definitions, so stale/missing masks are skipped (with a
            // `MaskPolicy::Warn` note) instead of being recomputed.
            let loaded_planes =
                load_persisted_mask_planes(&document, &zdata_path, &mut ignored_warnings);
            let source_actions = resolve_source_actions(&recipe, &zdata_path)?;
            let copies = document.virtual_copies.clone();
            let context = RenderContext {
                recipe: &recipe,
                camera_white_balance: wb,
                source_actions: &source_actions,
                masks: Some(MaskContext {
                    copies: &copies,
                    active_copy_id: &copy_id,
                    planes: loaded_planes,
                    policy: MaskPolicy::Warn,
                    source_roi: None,
                }),
                depth: None,
                lensfun,
            };
            // The artefact gate: a render that cannot be produced (an active
            // generative role with no resolvable artefact, an invalid recipe,
            // a corrupt bundle) aborts here and nothing is written.
            let output = renderer.render(&frame, &context)?;
            let mask_planes: Vec<MaskPlane> = output
                .mask_layers
                .iter()
                .map(|layer| layer.plane.clone())
                .collect();
            let matching =
                match_total_exposure_masked(&output.frame, request.target_luminance, &mask_planes)?;
            recipe.auto_features.match_total_exposure = true;
            recipe.auto_features.target_luminance = request.target_luminance;
            recipe.auto_features.matched_exposure = Some(matching);
            let total_exposure = (recipe.adjustments.get("exposure").copied().unwrap_or(0.0)
                + matching)
                .clamp(-10.0, 10.0);
            recipe.adjustments.insert("exposure".into(), total_exposure);
            document.virtual_copies[copy_index].recipe = recipe;
            changed = true;
            actions.push("matching:generated".into());
            report.push(json!({
                "module": RegenerateModule::Matching.as_str(),
                "action": "generated",
                "reason": if forced(RegenerateModule::Matching) { "explicit" } else { "stale-or-missing" },
                "matched_exposure": matching,
            }));
        }
    }

    if changed {
        document.validate()?;
    }
    let text = report
        .iter()
        .map(|entry| {
            format!(
                "{}={}",
                entry["module"].as_str().unwrap_or("?"),
                entry["action"].as_str().unwrap_or("?")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    info!(
        "regenerate: copy `{copy_id}` {} [{text}]",
        if changed { "updated" } else { "unchanged" }
    );
    let payload = json!({
        "command": "regenerate",
        "input": request.input,
        "virtual_copy": copy_id,
        "status": if changed { "updated" } else { "unchanged" },
        "modules": report,
    });
    let deferred = (changed && persist == Persist::Deferred).then(|| document.clone());
    if changed && persist == Persist::Immediately {
        save_sidecar(&sidecar_path, &document)?;
    }
    Ok(BulkRun {
        report: BulkReport::new(
            "regenerate",
            actions,
            Some(payload),
            vec![format!("regenerated: {text}")],
            changed,
        ),
        sidecar_path,
        document: deferred,
    })
}

/// GUI-GEN-GRANULAR-10 (F-100): does this source mask need a fresh artifact?
///
/// Conservative pre-filter for the *collective* default only; an explicit
/// `--module masks` forces the refresh regardless (range prompts excluded by
/// the caller, see `regenerate`). It checks the cheap, CLI-owned parts of the
/// decision-layer validity rule (status, source content hash, artifact
/// availability). The **authoritative** identity check stays in
/// `lumina-core::mask_loader` — decode context, model identity and plane
/// dimensions are deliberately *not* duplicated here, because that module is
/// the single owner of the inference contract. A fresh-looking value that is
/// actually stale on those dimensions is still caught (and reported loudly) by
/// the decision layer at render time.
fn mask_needs_regeneration(root: &Path, current: &SourceIdentity, mask: &MaskDefinition) -> bool {
    if !matches!(mask.status, MaskStatus::Valid) {
        return true;
    }
    if mask.source_fingerprint.content_hash != current.content_hash {
        return true;
    }
    match mask.artifact.as_ref() {
        None => true,
        Some(artifact) => artifact_status(root, artifact) != ArtifactStatus::Available,
    }
}
