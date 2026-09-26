//! The generative-canvas **artefact** helpers of `lumina generative`
//! (GEN-ONNX-1 Welle 1) — moved here by MCP-PARITY-B.
//!
//! Everything that produces, persists, resolves or reports a
//! `generative_canvas` artefact lives in this module so that the CLI and the
//! MCP server cannot disagree about *which* artefact a recipe needs, what its
//! identity is, and what a missing/stale/corrupt one means.
//!
//! # The model/artefact gate
//!
//! A generative role is only "there" if its artefact is there.
//! [`resolve_generative_role`] and [`generative_role_status`] are the two
//! halves of that gate:
//!
//! * a role whose recipe link is missing (deliberate `--remove`) is `missing`
//!   and **never** silently re-adopted from a still-present bundle record;
//! * a `Stale`/`Missing`/`Corrupt` link is reported by name and the render
//!   aborts — "no silent fallback" is part of the error text, not a comment;
//! * a link that is current but whose bundle record cannot be read is loud too,
//!   because rendering "as if not generated" is exactly the failure mode the
//!   whole artefact chain exists to prevent.
//!
//! So there is no state in which a call reports success while quietly rendering
//! an unexpanded or unfilled frame.

use crate::error::StageError;
use lumina_core::{
    has_transparent_pixels, render_frame_with_generative, GenerativeCanvasArtifact,
    GenerativeCanvasInput, GenerativeRole as CoreGenerativeRole, ImageFrame, LensfunCorrectorRef,
    RenderContext, SourceActionArtifact,
};
use lumina_onnx::fixture_manifest;
use lumina_onnx::generative::{
    produce_canvas, GenerativeCanvasOutput, GenerativeModelSource,
    GenerativeRole as OnnxGenerativeRole,
};
use lumina_sidecar::{
    generative_artifact_status, load_zdata, save_generative_canvas, Crop, EditRecipe,
    GenerativeArtifactRef, GenerativeArtifactStatus, GenerativeCanvas,
    GenerativeCanvasArtifact as SidecarGenerativeCanvas, GenerativeEdit, Geometry,
};
use serde_json::{json, Value};
use std::path::Path;

/// Bridges the `lumina-core` role to the `lumina-onnx` role so the producer
/// (here) and the render-time identity both speak of the same role.
pub fn onnx_generative_role(role: CoreGenerativeRole) -> OnnxGenerativeRole {
    match role {
        CoreGenerativeRole::Expand => OnnxGenerativeRole::Expand,
        CoreGenerativeRole::AutoFillTransparent => OnnxGenerativeRole::AutoFillTransparent,
    }
}

/// The prompt/model identity of a persisted generative edit.
///
/// `model_hash` is the pinned hash of the fixture model the CLI produces with
/// (`fixture_manifest(role)`), so producer (render-time verification) and
/// consumer (the ONNX producer) derive the exact same identity. `prompt` is the
/// persisted prompt; `negative_prompt` is the extras-backed additive field
/// (`GenerativeEdit::negative_prompt()`, `None` when unset) — `None` is
/// identity and matches the producer, which derives the same value.
pub fn generative_identity(
    role: CoreGenerativeRole,
    edit: &GenerativeEdit,
) -> lumina_core::GenerativeIdentity {
    lumina_core::GenerativeIdentity {
        model_hash: fixture_manifest(onnx_generative_role(role)).model_hash,
        prompt: edit.prompt.clone().unwrap_or_default(),
        negative_prompt: edit.negative_prompt().map(str::to_owned),
    }
}

/// Deterministic bundle record id for a generative identity digest.
pub fn generative_record_id(identity_digest: &str) -> String {
    let prefix = identity_digest.get(..16).unwrap_or(identity_digest);
    format!("generative_canvas:{prefix}")
}

/// Parse the CLI canvas spec `WxH+X+Y` / `WxH-X-Y`.
pub fn parse_generative_canvas(spec: &str) -> Result<GenerativeCanvas, StageError> {
    let invalid = || {
        StageError::Message(format!(
            "invalid --canvas `{spec}`: expected WxH+X+Y (offsets may be negative)"
        ))
    };
    let spec = spec.trim();
    let x_pos = spec.find('x').ok_or_else(invalid)?;
    let width: u32 = spec[..x_pos].trim().parse().map_err(|_| invalid())?;
    let rest = &spec[x_pos + 1..];
    let sign_pos = rest.find(['+', '-']).unwrap_or(rest.len());
    let height: u32 = rest[..sign_pos].trim().parse().map_err(|_| invalid())?;
    let offsets = &rest[sign_pos..];
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0usize;
    for (index, byte) in offsets.bytes().enumerate() {
        if index > 0 && (byte == b'+' || byte == b'-') {
            parts.push(&offsets[start..index]);
            start = index;
        }
    }
    if !offsets.is_empty() {
        parts.push(&offsets[start..]);
    }
    let (source_offset_x, source_offset_y) = match parts.as_slice() {
        [x, y] => (
            x.parse::<i32>().map_err(|_| invalid())?,
            y.parse::<i32>().map_err(|_| invalid())?,
        ),
        _ => return Err(invalid()),
    };
    let canvas = GenerativeCanvas {
        output_width: width,
        output_height: height,
        source_offset_x,
        source_offset_y,
        extras: Default::default(),
    };
    canvas
        .validate()
        .map_err(|error| StageError::Message(format!("invalid --canvas `{spec}`: {error}")))?;
    Ok(canvas)
}

/// Stage the frame exactly up to the generative stage, returning the frame
/// entering the auto-fill role (`after_lens`) and the frame entering the expand
/// role (`after_perspective`). Delegates to the core helper so producer and
/// consumer agree on the operation identity and no pipeline logic is
/// duplicated.
pub fn stage_generative_input(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<(ImageFrame, ImageFrame), StageError> {
    Ok(lumina_core::generative_input_frames(
        frame,
        recipe,
        camera_white_balance,
        source_actions,
        lensfun,
    )?)
}

/// Produce one role's composited canvas with the deterministic fixture model.
///
/// The persisted edit may carry both roles; `produce_canvas` reads a single
/// role from the flags, so a role-scoped copy is passed — the other role's flag
/// never silently changes which canvas is produced.
pub fn produce_generative_role_canvas(
    input_frame: &ImageFrame,
    edit: &GenerativeEdit,
    role: CoreGenerativeRole,
) -> Result<GenerativeCanvasOutput, StageError> {
    let mut scoped = edit.clone();
    match role {
        CoreGenerativeRole::Expand => {
            scoped.expand_beyond_image = Some(true);
            scoped.auto_fill_transparent = Some(false);
        }
        CoreGenerativeRole::AutoFillTransparent => {
            scoped.expand_beyond_image = Some(false);
            scoped.auto_fill_transparent = Some(true);
        }
    }
    let model = GenerativeModelSource::Fixture(onnx_generative_role(role));
    produce_canvas(input_frame, &scoped, &model)
        .map_err(|error| StageError::Message(format!("generative {role:?} canvas failed: {error}")))
}

/// Persist one produced canvas into the sidecar `.lumina.zdata` bundle and
/// return its portable recipe link, its `--json` role object and its human
/// summary. The record id is the deterministic identity id
/// ([`generative_record_id`]) — the same convention the GUI producer uses, so
/// GUI and CLI address the same record.
pub fn persist_generative_role_canvas(
    zdata_path: &Path,
    relative_path: &str,
    force: bool,
    output: GenerativeCanvasOutput,
) -> Result<(GenerativeArtifactRef, Value, String), StageError> {
    let identity = output.identity_digest;
    let role = output.role;
    let model_name = output.manifest.model_name.clone();
    let model_hash = output.manifest.model_hash.clone();
    let record = SidecarGenerativeCanvas {
        id: generative_record_id(&identity),
        width: output.width,
        height: output.height,
        pixels: output.pixels,
    };
    save_generative_canvas(zdata_path, record.clone(), force).map_err(|error| {
        StageError::Message(format!(
            "could not write generative canvas bundle `{}`: {error}",
            zdata_path.display()
        ))
    })?;
    let link =
        GenerativeArtifactRef::from_generative_canvas(&record, relative_path, identity.clone());
    let role_json = json!({
        "role": format!("{role:?}"),
        "width": record.width,
        "height": record.height,
        "record": record.id,
        "identity": identity,
        "model": model_name,
        "model_hash": model_hash,
    });
    let summary = format!(
        "role={role:?} {}x{} record={} identity={identity}",
        record.width, record.height, record.id
    );
    Ok((link, role_json, summary))
}

/// The frame entering the expand role when the auto-fill role produced a
/// canvas: the auto-filled frame with `Lens → auto-fill → Perspective` applied,
/// mirroring the render order `Lens → auto-fill → Perspective → expand → Crop`
/// (GEN-ONNX-1 double-role record). Without an applied auto-fill canvas the
/// caller uses `after_perspective` directly.
///
/// The CLI must **not** call `lumina-core`'s `#[cfg(feature = "lensfun")]`
/// `ImageFrame::apply_perspective_stage` directly: the CLI's own `lensfun`
/// feature can be off while `lumina-core`'s is on (e.g. `cargo check
/// --workspace`, where the GUI's default `lensfun` unifies the core feature),
/// which would select the wrong arity. The frame is therefore extracted through
/// the shared, feature-uniform core render pipeline: supplying the auto-fill
/// artifact makes `composite_auto_fill` replace the lens result with the
/// artifact frame, and the perspective stage runs on it exactly as the real
/// render does. A full-frame crop and `expand = false` keep the remaining
/// stages identity, so the CLI still owns no image math.
pub fn generative_expand_input(
    source: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    auto_fill: &GenerativeCanvasArtifact,
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<ImageFrame, StageError> {
    let mut staged = recipe.clone();
    // Neutralize crop/rotation/mirroring and lens blur; they run after the
    // expand stage in the real pipeline and must not alter the extracted frame.
    staged.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    staged.lens_blur = None;
    if let Some(edit) = staged.generative_edit.as_mut() {
        // Only the auto-fill substitution runs; the expand stage is the caller's
        // next step. `expand = false` requires `canvas == None`
        // (`validate_generative_edit`), and the canvas is unused by the
        // perspective stage.
        edit.expand_beyond_image = Some(false);
        edit.canvas = None;
    }
    let output = render_frame_with_generative(
        source,
        &RenderContext {
            recipe: &staged,
            camera_white_balance,
            source_actions,
            masks: None,
            lensfun,
            depth: None,
        },
        GenerativeCanvasInput {
            auto_fill: Some(auto_fill),
            expand: None,
        },
    )?;
    Ok(output.frame)
}

/// Resolve one active generative role's canvas.
///
/// The persisted record is addressed by the deterministic identity record id
/// ([`generative_record_id`]), so the *unlinked* second role of a double-role
/// record is resolvable by construction: a matching record proves the exact
/// identity (role, seed, canvas, prompt, model and input are all in the digest,
/// and the zdata load verifies the BLAKE3 checksum). When no matching record
/// exists, `link` — the recipe link, pre-scoped by the caller to the role it
/// belongs to — yields the loud `missing`/`stale`/`corrupt` diagnosis. There is
/// no silent fallback to an unexpanded render.
///
/// `owns_link` marks the role the single recipe link belongs to. Its link must
/// be present: an explicit `--remove` unlink is a deliberate state and must not
/// be silently reconstructed from the still-present bundle record.
pub fn resolve_generative_role(
    bundle_root: &Path,
    zdata_path: &Path,
    link: Option<&GenerativeArtifactRef>,
    owns_link: bool,
    role: CoreGenerativeRole,
    digest: &str,
) -> Result<GenerativeCanvasArtifact, StageError> {
    if owns_link && link.is_none() {
        return Err(StageError::Message(format!(
            "generative {role:?} is active but no `generative_canvas` artifact is linked; run \
             `lumina generative --generate --input <file>` (no silent fallback)"
        )));
    }
    let record_id = generative_record_id(digest);
    if zdata_path.exists() {
        if let Ok(container) = load_zdata(zdata_path) {
            if let Ok(record) = container.generative_canvas(&record_id) {
                let frame = ImageFrame::new(record.width, record.height, record.pixels)
                    .map_err(|error| StageError::Message(error.to_string()))?;
                return Ok(GenerativeCanvasArtifact::new(role, frame));
            }
        }
    }
    // The role-scoped recipe link is the canonical diagnosis: a
    // `Stale`/`Missing`/`Corrupt` link stays loud; a link that is current but
    // whose record is unreadable must never render "as if not generated".
    if let Some(link) = link {
        let status = generative_artifact_status(bundle_root, link, digest);
        if status != GenerativeArtifactStatus::Available {
            return Err(StageError::Message(format!(
                "generative canvas `{}` is {status:?} for identity {digest}; refusing to render \
                 (run `lumina generative --generate` to rebuild it) — no silent fallback",
                link.id
            )));
        }
        return Err(StageError::Message(format!(
            "generative {role:?} link `{}` is current but its bundle record `{record_id}` is \
             unreadable; refusing to render (no silent fallback)",
            link.id
        )));
    }
    Err(StageError::Message(format!(
        "generative {role:?} is active but no `generative_canvas` artifact is available for \
         identity {digest}; run `lumina generative --generate --input <file>` (no silent fallback)"
    )))
}

/// Non-fatal per-role status used by `--status`: `(status, resolved frame)`.
///
/// Mirrors [`resolve_generative_role`] without aborting, so a double-role
/// record reports every active role; the frame is returned when the auto-fill
/// canvas resolved (the expand identity is derived from it).
pub fn generative_role_status(
    bundle_root: &Path,
    zdata_path: &Path,
    link: Option<&GenerativeArtifactRef>,
    owns_link: bool,
    digest: &str,
) -> (String, Option<ImageFrame>) {
    // The link-owning role with an explicitly removed link is `missing` — the
    // record must not be re-adopted silently (`--remove`).
    if owns_link && link.is_none() {
        return ("missing".to_owned(), None);
    }
    let record_id = generative_record_id(digest);
    if zdata_path.exists() {
        if let Ok(container) = load_zdata(zdata_path) {
            if let Ok(record) = container.generative_canvas(&record_id) {
                if let Ok(frame) = ImageFrame::new(record.width, record.height, record.pixels) {
                    return ("available".to_owned(), Some(frame));
                }
            }
        }
    }
    if let Some(link) = link {
        let status = format!(
            "{:?}",
            generative_artifact_status(bundle_root, link, digest)
        )
        .to_lowercase();
        return (status, None);
    }
    ("missing".to_owned(), None)
}

/// Whether the auto-fill role is *required* for this input: active in the edit
/// **and** there really are transparent pixels after the lens stage. The
/// normative caller convention is "no transparent pixels → identity, no
/// artefact required", so the requirement and the activity are different
/// questions.
pub fn auto_fill_required(after_lens: &ImageFrame, edit: &GenerativeEdit) -> bool {
    edit.auto_fill_transparent.unwrap_or(false) && has_transparent_pixels(after_lens)
}

/// A recipe canvas that is present, for callers that already established
/// `expand_beyond_image`.
pub fn required_canvas(edit: &GenerativeEdit) -> Result<&GenerativeCanvas, StageError> {
    edit.canvas.as_ref().ok_or_else(|| {
        StageError::Message("expand_beyond_image requires a `canvas` (output_* + offsets)".into())
    })
}

/// A caller-supplied Lensfun corrector source.
///
/// The corrector borrows its database, and building it needs the optional native
/// `lensfun` capability that `lumina-stages` deliberately does not link. So the
/// *caller* resolves it — the same port slice A used for the geometry Lensfun
/// report — and hands the result in. Splitting the owner from the reference
/// keeps that possible **without** making the caller decode the source twice:
/// [`CorrectorSource::build`] is called with the metadata the shared code
/// already decoded, and it stores the corrector in the caller's own storage.
///
/// `None` is a real "no corrector applies" answer, never a guess, and never a
/// silently substituted correction.
pub trait CorrectorSource {
    /// Storage that keeps alive whatever the corrector borrows from. `Default`
    /// because it is created before the corrector exists.
    type Owner: Default;

    /// Builds the corrector for the decoded `metadata` and stores it in `owner`.
    fn build<'o>(
        &mut self,
        metadata: Option<&lumina_raw::RawMetadata>,
        owner: &'o mut Self::Owner,
    ) -> Option<LensfunCorrectorRef<'o>>;
}

/// The default corrector source: **no** corrector.
///
/// Correct for every build that does not link the native `lensfun` capability
/// (the CLI's default, and always for `lumina-mcp`), and for a source whose EXIF
/// matches no non-identity profile. It never guesses a correction.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCorrector;

impl CorrectorSource for NoCorrector {
    type Owner = ();

    fn build<'o>(
        &mut self,
        _metadata: Option<&lumina_raw::RawMetadata>,
        _owner: &'o mut (),
    ) -> Option<LensfunCorrectorRef<'o>> {
        None
    }
}
