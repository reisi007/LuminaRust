//! `process`: the render-and-persist entry point including the mask-inference
//! engine resolution and the generative artifact resolution.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, process slice). `process_selected` itself
//! stays in the crate root because the structural Auto-Tone guard in
//! `tests/auto_tone_writer.rs` pins its shared-writer call to `src/main.rs`.
//! Pure move: bodies are byte-identical; only the `pub(super)` tokens and this
//! header are new.

use super::*;

pub(super) fn process_selected(
    args: ProcessArgs,
    quality: u8,
    virtual_copy: Option<&str>,
    policy: MaskPolicy,
    mask_warnings_out: &mut Vec<String>,
) -> Result<Option<MetadataWritten>, CliError> {
    super::process_selected_with_report(args, quality, virtual_copy, policy, mask_warnings_out).map(
        |(metadata_written, _rendered)| {
            #[cfg(feature = "gpu")]
            let _discarded_gpu_report = _rendered.gpu_report;
            metadata_written
        },
    )
}

pub(super) fn process(args: ProcessArgs) -> Result<(), CliError> {
    // `process` has no explicit quality flag; it uses the shared default (90),
    // which is identical to the historical `frame.encode(format)` output.
    // LRPAR-G15-IPTC-S6: `--write-metadata` rides on `args` into
    // `process_selected` (bake-in outcome lands in the sidecar record).
    process_selected(args, 90, None, MaskPolicy::Warn, &mut Vec::new())?;
    Ok(())
}

/// Resolve the subject-mask inference engine the CLI wires into the F-048 /
/// F-051 mask-loading decision layer (F-082-FOLLOWUP, real ORT path).
///
/// # Contract (no silent fallback)
///
/// The CLI **requests** the real ONNX engine only when the current run can
/// actually require re-inference (`needs_inference`: the active copy carries
/// reachable mask layers — the same reachability the F-048/F-051 decision
/// layer uses, so a `--update-masks` refresh on a mask-less copy requests
/// nothing). Without mask work no model is needed, so no engine is requested
/// and there is nothing to fail on.
///
/// | Build | `needs_inference` | Result |
/// | --- | --- | --- |
/// | `onnx-rt` off (default) | any | `Some(StubBackend)` — the documented default wiring |
/// | `onnx-rt` on | true, `LUMINA_MODEL_PATH` set, artifact loadable and identity-verified | `Some(OnnxRuntime)` — the real engine |
/// | `onnx-rt` on | true, `LUMINA_MODEL_PATH` unset | hard `CliError` |
/// | `onnx-rt` on | true, artifact missing / stale / mismatched / wrong tensor names | hard `CliError` carrying the resolver's `OnnxError` |
/// | `onnx-rt` on | false | `Ok(None)` — no engine is requested |
///
/// The deterministic [`StubBackend`] is only ever wired in default builds:
/// under `onnx-rt` a requested real engine is **never** downgraded to the stub
/// (AGENTS.md: „Fehlende oder inkompatible Artefakte werden sichtbar als
/// veraltet oder nicht verfügbar gemeldet"; ai-masks.md F-082-FOLLOWUP).
#[cfg(not(feature = "onnx-rt"))]
pub(super) fn resolve_mask_inference_engine(
    _needs_inference: bool,
) -> Result<Option<Box<dyn MaskInference>>, CliError> {
    // Default build: the deterministic, dependency-free stub backend is the
    // documented default wiring (unchanged behavior).
    match StubBackend::new(birefnet_manifest()) {
        Ok(stub) => Ok(Some(Box::new(stub))),
        Err(_) => Ok(None),
    }
}
#[cfg(feature = "onnx-rt")]
pub(super) fn resolve_mask_inference_engine(
    needs_inference: bool,
) -> Result<Option<Box<dyn MaskInference>>, CliError> {
    if !needs_inference {
        return Ok(None);
    }
    let model_path = std::env::var_os("LUMINA_MODEL_PATH").ok_or_else(|| {
        CliError::Message(
            "`onnx-rt` is compiled into this build and the run requires mask \
             re-inference, but `LUMINA_MODEL_PATH` is not set; the real ONNX engine \
             is unavailable and the deterministic stub is never a silent substitute \
             (F-082-FOLLOWUP)"
                .into(),
        )
    })?;
    resolve_onnx_engine_from_path(Path::new(&model_path))
}

/// Resolve the real ONNX engine from an explicit artifact path (F-082-FOLLOWUP).
///
/// Path-parameterized so the onnx-rt wiring semantics are testable without
/// touching the process-global `LUMINA_MODEL_PATH` (and thereby racing other
/// render tests). The BiRefNet [`ModelManifest`](lumina_onnx::ModelManifest) is
/// the identity/IO contract; the resolver reports `RuntimeDisabled` (feature
/// off — impossible here), `OnnxRuntime` (artifact verified) or a hard
/// [`OnnxError`](lumina_onnx::OnnxError): a missing/stale/mismatched artifact is
/// a loud error, never a silent stub.
#[cfg(feature = "onnx-rt")]
pub(super) fn resolve_onnx_engine_from_path(
    model_path: &Path,
) -> Result<Option<Box<dyn MaskInference>>, CliError> {
    match try_load_onnx_engine(model_path, &birefnet_manifest()) {
        Ok(OnnxEngine::OnnxRuntime(engine)) => Ok(Some(engine)),
        Ok(OnnxEngine::RuntimeDisabled) => unreachable!(
            "`try_load_onnx_engine` must return `OnnxRuntime` when `onnx-rt` is compiled in"
        ),
        Err(error) => Err(CliError::Message(format!(
            "the real ONNX engine could not be loaded from `{}`: {error} \
             (no silent fallback to the deterministic stub)",
            model_path.display()
        ))),
    }
}

/// Owned pair of caller-supplied generative canvas artifacts.
#[derive(Default)]
pub(crate) struct GenerativeArtifacts {
    pub(crate) auto_fill: Option<GenerativeCanvasArtifact>,
    pub(crate) expand: Option<GenerativeCanvasArtifact>,
}

impl GenerativeArtifacts {
    pub(crate) fn input(&self) -> GenerativeCanvasInput<'_> {
        GenerativeCanvasInput {
            auto_fill: self.auto_fill.as_ref(),
            expand: self.expand.as_ref(),
        }
    }
}

/// Resolve the render-time generative canvas artifacts for `recipe`.
///
/// GEN-ONNX-1 (double-role record): a single `GenerativeEdit` may carry both
/// `auto_fill_transparent` and `expand_beyond_image` (the SOLL order is
/// `Lens → auto-fill → Perspective → expand → Crop`). Both roles are resolved;
/// the expand identity is derived from the auto-filled frame with the
/// perspective stage applied, exactly as the producer built it. A
/// missing/invalid/absent artifact for an active role aborts the render — there
/// is no silent fallback.
pub(super) fn resolve_generative_artifacts(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    zdata_path: &Path,
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<GenerativeArtifacts, CliError> {
    let Some(edit) = recipe.generative_edit.as_ref() else {
        return Ok(GenerativeArtifacts::default());
    };
    let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
    let expand_active = edit.effective_expand();
    if !auto_fill_active && !expand_active {
        return Ok(GenerativeArtifacts::default());
    }
    let seed = edit.seed.unwrap_or(0);
    let (after_lens, after_perspective) =
        stage_generative_input(frame, recipe, camera_white_balance, source_actions, lensfun)?;
    let bundle_root = zdata_path.parent().unwrap_or_else(|| Path::new("."));
    let link = edit.artifact.as_ref();
    // The single recipe link belongs to the canvas-defining role the producer
    // persisted: expand when it is active, otherwise auto-fill (the double-role
    // producer links the expand canvas). It is scoped per role so a
    // `stale`/`missing`/`corrupt` diagnosis is never reported for the wrong
    // role.
    let link_role = if expand_active {
        Some(CoreGenerativeRole::Expand)
    } else if auto_fill_active {
        Some(CoreGenerativeRole::AutoFillTransparent)
    } else {
        None
    };
    let role_link = |role: CoreGenerativeRole| link.filter(|_| link_role == Some(role));
    let mut artifacts = GenerativeArtifacts::default();
    // Auto-fill first: its composited frame is the input of the expand role
    // (`Lens → auto-fill → Perspective → expand`). Auto-fill without
    // transparent pixels after lens needs no artifact (normative caller
    // convention, `auto_fill = None` is the identity).
    if auto_fill_active && has_transparent_pixels(&after_lens) {
        let identity = generative_identity(CoreGenerativeRole::AutoFillTransparent, edit);
        let digest = GenerativeCacheKey::auto_fill(&after_lens, seed, &identity).digest();
        artifacts.auto_fill = Some(resolve_generative_role(
            bundle_root,
            zdata_path,
            role_link(CoreGenerativeRole::AutoFillTransparent),
            link_role == Some(CoreGenerativeRole::AutoFillTransparent),
            CoreGenerativeRole::AutoFillTransparent,
            &digest,
        )?);
    }
    if expand_active {
        let canvas = edit.canvas.as_ref().ok_or_else(|| {
            CliError::Message("expand_beyond_image requires a `canvas` (output_* + offsets)".into())
        })?;
        // Mirror the render order `Lens → auto-fill → Perspective → expand` so
        // the expand identity matches the canvas the producer built (the expand
        // canvas is authoritative and must embed the auto-filled pixels).
        let expand_input = match artifacts.auto_fill.as_ref() {
            Some(auto_fill) => generative_expand_input(
                frame,
                recipe,
                camera_white_balance,
                source_actions,
                auto_fill,
                lensfun,
            )?,
            None => after_perspective.clone(),
        };
        let identity = generative_identity(CoreGenerativeRole::Expand, edit);
        let digest = GenerativeCacheKey::expand(&expand_input, canvas, seed, &identity).digest();
        artifacts.expand = Some(resolve_generative_role(
            bundle_root,
            zdata_path,
            role_link(CoreGenerativeRole::Expand),
            link_role == Some(CoreGenerativeRole::Expand),
            CoreGenerativeRole::Expand,
            &digest,
        )?);
    }
    Ok(artifacts)
}
