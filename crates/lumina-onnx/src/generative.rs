//! Generative canvas production (GEN-ONNX-1 Welle 1, `feature/product/generative-expand.md`).
//!
//! This module is the native producer side of the generative stage: it turns a
//! [`lumina_sidecar::GenerativeEdit`] recipe plus the decoded frame into the
//! **full composited canvas** (`generative_canvas` artifact) and its complete
//! identity. The core render stage only *consumes* that artifact
//! (artifact compositing instead of the former heuristic BFS); all modelling,
//! capability gating and identity pinning live here, in the native crate.
//!
//! ## Model decision (Welle 1, see `feature/product/generative-expand.md`)
//!
//! No license-clean inpaint/outpaint weights are committed in the workspace and
//! the project doctrine forbids spontaneous downloads (Agents.md). Welle 1
//! therefore delivers:
//!
//! * a **deterministic fixture model** ([`fixture_manifest`]) whose
//!   `model_hash` is the real SHA-256 over a canonical, documented fixture
//!   specification (not the `pending-integration` placeholder), so the
//!   generative artifact identity is complete and verifiable; and
//! * a documented **real-model attachment interface**
//!   ([`GenerativeModelSource::Artifact`]) that hash-verifies a real `.onnx`
//!   artifact before any inference ([`Self::resolve_manifest`]).
//!
//! The *planned weight* descriptors (`inpaint_heal_manifest` /
//! `outpaint_expand_manifest`) keep the loud `pending-integration` placeholder:
//! the missing real model is reported as unavailable, never silently replaced
//! by the fixture.
//!
//! ## No silent fallback
//!
//! Every failure is an explicit [`OnnxError`]: an unavailable backend, a
//! missing/invalid canvas, a capability the model does not declare, or a
//! hash-mismatched artifact. There is no fallback to the BFS heuristic and no
//! "render as if nothing had been generated".

use std::path::{Path, PathBuf};

use lumina_core::generative::GenerativeRole as CoreRole;
use lumina_core::{GenerativeCacheKey, GenerativeIdentity, ImageFrame};
use lumina_sidecar::{GenerativeCanvas, GenerativeEdit};

use crate::hash::{compute_sha256_hex, PENDING_INTEGRATION_HASH};
use crate::inpaint::{InpaintRequest, StubInpaintBackend};
use crate::manifest::{inpaint_heal_manifest, outpaint_expand_manifest, ModelManifest};
use crate::outpaint::{OutpaintCanvas, OutpaintRequest, StubOutpaintBackend};
use crate::{ModelHashStatus, OnnxError};

/// Role of one generative canvas production run.
///
/// Mirrors the core render roles one-to-one and is deliberately **not**
/// inferred from the model name; the role is read from the persisted
/// `GenerativeEdit` flags (`effective_expand` / `auto_fill_transparent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerativeRole {
    /// `auto_fill_transparent`: fill transparent pixels after lens correction.
    AutoFillTransparent,
    /// `expand_beyond_image`: composite the frame into a larger canvas and fill
    /// the new border.
    Expand,
}

impl GenerativeRole {
    /// Select the role from a persisted [`GenerativeEdit`].
    ///
    /// `expand_beyond_image` wins over `auto_fill_transparent` for the single
    /// record MVP (the SOLL documents one record carrying both flags with the
    /// order `Lens → GenerativeEdit → Perspective → Crop`); the expand canvas
    /// is authoritative then. An edit with neither flag active needs no canvas
    /// and returns `None` (identity, no artifact).
    pub fn from_edit(edit: &GenerativeEdit) -> Option<Self> {
        if edit.effective_expand() {
            Some(Self::Expand)
        } else if edit.auto_fill_transparent.unwrap_or(false) {
            Some(Self::AutoFillTransparent)
        } else {
            None
        }
    }
}

/// Version tag of the deterministic fixture model algorithm.
///
/// Part of the fixture specification that is hashed into the real
/// `model_hash`; changing the algorithm requires a new version tag (and
/// therefore a new, explicit identity — never a silent re-interpretation).
pub const GENERATIVE_FIXTURE_ALGORITHM: &str = "lumina-generative-fixture-v1";

/// Canonical, versioned text form of a fixture model specification.
///
/// Encoded by hand (fixed field order, leading schema tag) so the digest has
/// no error path and is stable independent of any serializer. Every identity
/// component that influences the produced pixels participates: algorithm
/// version, model name/version, inference resolution, tensor contract and
/// normalization.
fn canonical_fixture_spec(manifest: &ModelManifest) -> String {
    format!(
        "{GENERATIVE_FIXTURE_ALGORITHM}|name={}|version={}|res={}x{}|tensor={}|format={:?}|mean={:?}|std={:?}",
        manifest.model_name,
        manifest.model_version,
        manifest.input.resolution.width,
        manifest.input.resolution.height,
        manifest.input.tensor_name,
        manifest.input.tensor_format,
        manifest.input.normalization.mean,
        manifest.input.normalization.std,
    )
}

/// Real SHA-256 identity of the deterministic fixture model.
///
/// This is **not** a weight-file hash (no weights exist); it is the content
/// hash of the exact, documented fixture specification that fully determines
/// the produced canvas. [`Self::verify`] recomputes it, so the pin is a real
/// check and not a placeholder. The real-weight path
/// ([`GenerativeModelSource::Artifact`]) uses the streaming SHA-256 over the
/// `.onnx` artifact bytes instead.
#[must_use]
pub fn fixture_model_hash(manifest: &ModelManifest) -> String {
    let digest = compute_sha256_hex(canonical_fixture_spec(manifest).as_bytes())
        .expect("hashing an in-memory buffer cannot fail");
    format!("sha256:{digest}")
}

/// The deterministic fixture manifest for `role`, hash-pinned to
/// [`fixture_model_hash`].
///
/// The descriptor itself comes from the planned descriptors
/// ([`outpaint_expand_manifest`] / [`inpaint_heal_manifest`]); only the
/// `model_hash` placeholder is replaced by the real fixture hash, so there is
/// exactly one capability/resolution contract. A role without a declared
/// capability is rejected by the backends (never guessed).
#[must_use]
pub fn fixture_manifest(role: GenerativeRole) -> ModelManifest {
    let mut manifest = match role {
        GenerativeRole::Expand => outpaint_expand_manifest(),
        GenerativeRole::AutoFillTransparent => inpaint_heal_manifest(),
    };
    manifest.model_hash = fixture_model_hash(&manifest);
    manifest
}

/// Whether `manifest` carries a real (non-placeholder) `model_hash`.
#[must_use]
pub fn manifest_hash_is_pinned(manifest: &ModelManifest) -> bool {
    manifest.model_hash != PENDING_INTEGRATION_HASH
}

/// Verify the fixture manifest's pin: recompute the spec digest and compare.
///
/// Returns [`ModelHashStatus::Verified`] only when the manifest actually
/// carries the recomputed fixture hash; a `pending-integration` manifest is
/// reported as [`ModelHashStatus::Pending`] (the documented pre-integration
/// state); anything else is a [`ModelHashStatus::Mismatch`]. Never a silent
/// success.
#[must_use]
pub fn verify_fixture_manifest(manifest: &ModelManifest) -> ModelHashStatus {
    if manifest.model_hash == PENDING_INTEGRATION_HASH {
        return ModelHashStatus::Pending;
    }
    let actual = {
        let mut canonical = manifest.clone();
        canonical.model_hash = String::new();
        fixture_model_hash(&canonical)
    };
    if actual == manifest.model_hash {
        ModelHashStatus::Verified
    } else {
        ModelHashStatus::Mismatch {
            expected: manifest.model_hash.clone(),
            actual,
        }
    }
}

/// Where the generative model comes from.
///
/// * [`Self::Fixture`] — the deterministic, hash-pinned fixture model used by
///   the CLI stub path and by every test. No weights, no network.
/// * [`Self::Artifact`] — the documented real-model attachment interface: a
///   `ModelManifest` plus the local `.onnx` artifact path. [`Self::resolve_manifest`]
///   stream-hashes the artifact and refuses a mismatching/stale/missing file
///   loudly.
#[derive(Debug, Clone, PartialEq)]
pub enum GenerativeModelSource {
    /// Deterministic hash-pinned fixture model (no weights, no network).
    Fixture(GenerativeRole),
    /// A real local ONNX artifact with a pinned manifest identity.
    Artifact {
        /// Manifest declaring the exact identity and I/O contract.
        ///
        /// Boxed because it is much larger than the fixture variant payload;
        /// this keeps the enum cheap to move.
        manifest: Box<ModelManifest>,
        /// Local `.onnx` artifact path (never downloaded, never guessed).
        path: PathBuf,
    },
}

impl GenerativeModelSource {
    /// The real-model attachment constructor.
    #[must_use]
    pub fn artifact(manifest: ModelManifest, path: impl Into<PathBuf>) -> Self {
        Self::Artifact {
            manifest: Box::new(manifest),
            path: path.into(),
        }
    }

    /// Resolve the manifest for this source, verifying the artifact identity.
    ///
    /// The returned manifest is always **hash-pinned and verified**: the
    /// fixture recomputes its own spec digest (`Verified`), and a real artifact
    /// must hash to its pinned value. The real-`Artifact` path **hard-refuses**
    /// a `pending-integration` manifest (`ModelHashStatus::Pending`): a real
    /// artifact without a pinned identity cannot be verified, and silently
    /// proceeding would be exactly the phantom-green the doctrine forbids. A
    /// mismatching artifact is refused as `ModelArtifactStale`; a missing one as
    /// `MissingModel`. The fixture is never a silent substitute.
    pub fn resolve_manifest(&self) -> Result<ModelManifest, OnnxError> {
        match self {
            Self::Fixture(role) => {
                let manifest = fixture_manifest(*role);
                match verify_fixture_manifest(&manifest) {
                    ModelHashStatus::Verified => Ok(manifest),
                    ModelHashStatus::Pending | ModelHashStatus::Mismatch { .. } => {
                        // The fixture pin is computed from this very spec, so
                        // this is a programming error, not a runtime state.
                        Err(OnnxError::InvalidManifest(
                            "deterministic fixture manifest failed its own hash pin".into(),
                        ))
                    }
                }
            }
            Self::Artifact { manifest, path } => {
                let status = crate::hash::verify_model_file(path, &manifest.model_hash)?;
                if matches!(status, ModelHashStatus::Pending) {
                    // A real artifact with the `pending-integration` placeholder
                    // has no pinned identity: refuse loudly instead of inferring
                    // with unverifiable weights.
                    return Err(OnnxError::UnsupportedModel {
                        name: manifest.model_name.clone(),
                        reason: "model manifest carries the `pending-integration` placeholder; \
                                 a real generative artifact requires a pinned `model_hash`"
                            .into(),
                    });
                }
                status.enforce_inference_allowed(&manifest.model_name)?;
                manifest.validate()?;
                Ok((**manifest).clone())
            }
        }
    }

    /// Capability gate: the resolved manifest must declare `capability`.
    fn resolve_with_capability(&self, role: GenerativeRole) -> Result<ModelManifest, OnnxError> {
        let manifest = self.resolve_manifest()?;
        let declared = match role {
            GenerativeRole::Expand => manifest.capabilities.outpaint,
            GenerativeRole::AutoFillTransparent => manifest.capabilities.inpaint_heal,
        };
        if !declared {
            let capability = match role {
                GenerativeRole::Expand => "outpaint",
                GenerativeRole::AutoFillTransparent => "inpaint_heal",
            };
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: format!("{capability} not declared"),
            });
        }
        Ok(manifest)
    }
}

/// Full composited canvas produced for one generative operation.
///
/// `pixels` is the complete RGBA8 canvas (`width * height * 4`, row-major),
/// including unchanged source pixels — exactly the `generative_canvas` record
/// payload. `identity_digest` is the core
/// [`GenerativeCacheKey::digest`] of the run, so a later render can prove the
/// persisted record still matches the current source/recipe/seed/canvas via
/// `lumina_sidecar::generative_artifact_status`.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerativeCanvasOutput {
    pub role: GenerativeRole,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Manifest (with real, pinned `model_hash`) used for this run.
    pub manifest: ModelManifest,
    /// `GenerativeCacheKey::digest()` — the complete operation identity.
    pub identity_digest: String,
    /// `seed` that participated in the identity (0 when the recipe has none).
    pub seed: u64,
}

impl GenerativeCanvasOutput {
    /// The canvas as an [`ImageFrame`], for the caller to persist/inspect.
    pub fn to_frame(&self) -> Result<ImageFrame, OnnxError> {
        ImageFrame::new(self.width, self.height, self.pixels.clone()).map_err(|error| {
            OnnxError::InferenceFailed {
                name: "generative-canvas".into(),
                reason: error.to_string(),
            }
        })
    }
}

/// Produce the full composited generative canvas for `edit` over `frame`.
///
/// The role is read from the persisted flags, the manifest is resolved through
/// `source` (capability- and hash-gated) and the deterministic backend runs.
/// The returned identity digest is the core cache-key digest of the *exact*
/// same `(frame, canvas, seed)` tuple the render stage will later verify
/// against, so production and consumption cannot drift.
pub fn produce_canvas(
    frame: &ImageFrame,
    edit: &GenerativeEdit,
    source: &GenerativeModelSource,
) -> Result<GenerativeCanvasOutput, OnnxError> {
    let role = GenerativeRole::from_edit(edit).ok_or_else(|| {
        OnnxError::InvalidManifest(
        "generative_edit has neither `expand_beyond_image` nor `auto_fill_transparent` active; \
         nothing to produce (no silent canvas)"
            .into(),
    )
    })?;
    let manifest = source.resolve_with_capability(role)?;
    let seed = edit.seed.unwrap_or(0);

    match role {
        GenerativeRole::Expand => {
            let canvas = edit.canvas.as_ref().ok_or_else(|| {
                OnnxError::InvalidManifest(
                    "expand_beyond_image requires `canvas` (output_* + source_offset)".into(),
                )
            })?;
            let backend = StubOutpaintBackend { available: true };
            let request = OutpaintRequest {
                prompt: edit.prompt.clone().unwrap_or_default(),
                negative_prompt: edit.negative_prompt().map(str::to_owned),
                seed,
                canvas: OutpaintCanvas {
                    output_width: canvas.output_width,
                    output_height: canvas.output_height,
                    source_offset_x: canvas.source_offset_x,
                    source_offset_y: canvas.source_offset_y,
                },
            };
            let pixels = backend.expand_with_manifest(
                &frame.pixels,
                frame.width,
                frame.height,
                &request,
                &manifest,
            )?;
            let identity_digest = GenerativeCacheKey::expand(
                frame,
                canvas,
                seed,
                &GenerativeIdentity::new(manifest.model_hash.clone(), request.prompt.clone())
                    .with_negative_prompt(request.negative_prompt.clone()),
            )
            .digest();
            Ok(GenerativeCanvasOutput {
                role,
                width: canvas.output_width,
                height: canvas.output_height,
                pixels,
                manifest,
                identity_digest,
                seed,
            })
        }
        GenerativeRole::AutoFillTransparent => {
            let mask = transparent_mask(frame);
            if !mask.iter().any(|&value| value != 0) {
                return Err(OnnxError::InferenceFailed {
                    name: manifest.model_name.clone(),
                    reason: "auto_fill requested for a frame without transparent pixels".into(),
                });
            }
            let backend = StubInpaintBackend { available: true };
            let request = InpaintRequest {
                prompt: edit.prompt.clone().unwrap_or_default(),
                negative_prompt: edit.negative_prompt().map(str::to_owned),
                seed,
                region: None,
            };
            let pixels = backend.heal(&frame.pixels, frame.width, frame.height, &mask, &request)?;
            let identity_digest = GenerativeCacheKey::auto_fill(
                frame,
                seed,
                &GenerativeIdentity::new(manifest.model_hash.clone(), request.prompt.clone())
                    .with_negative_prompt(request.negative_prompt.clone()),
            )
            .digest();
            Ok(GenerativeCanvasOutput {
                role,
                width: frame.width,
                height: frame.height,
                pixels,
                manifest,
                identity_digest,
                seed,
            })
        }
    }
}

/// The implicit transparent mask after lens correction (`mask[i] = 255` where
/// the pixel is transparent or pure black, matching
/// `lumina_core::has_transparent_pixels`).
#[must_use]
pub fn transparent_mask(frame: &ImageFrame) -> Vec<u8> {
    let mut mask = vec![0u8; (frame.width as usize) * (frame.height as usize)];
    for (index, px) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
        if px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0) {
            mask[index] = 255;
        }
    }
    mask
}

/// Verify a persisted generative link against a freshly produced identity.
///
/// Thin convenience over the sidecar decision layer so the CLI can gate
/// rendering on `Available` only — a `Stale`/`Missing`/`Corrupt` link is a
/// visible refusal, never a silent re-generation.
#[must_use]
pub fn link_is_current(
    bundle_root: &Path,
    link: &lumina_sidecar::GenerativeArtifactRef,
    current_identity: &str,
) -> lumina_sidecar::GenerativeArtifactStatus {
    lumina_sidecar::generative_artifact_status(bundle_root, link, current_identity)
}

/// Convert a sidecar [`GenerativeCanvas`] into the backend canvas geometry.
#[must_use]
pub fn outpaint_canvas_from_sidecar(canvas: &GenerativeCanvas) -> OutpaintCanvas {
    OutpaintCanvas {
        output_width: canvas.output_width,
        output_height: canvas.output_height,
        source_offset_x: canvas.source_offset_x,
        source_offset_y: canvas.source_offset_y,
    }
}

/// Map a core role onto the ONNX role (kept explicit so the mapping is
/// testable and cannot silently diverge).
#[must_use]
pub fn role_from_core(role: CoreRole) -> GenerativeRole {
    match role {
        CoreRole::AutoFillTransparent => GenerativeRole::AutoFillTransparent,
        CoreRole::Expand => GenerativeRole::Expand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_edit() -> GenerativeEdit {
        GenerativeEdit {
            version: 1,
            canvas: Some(GenerativeCanvas {
                output_width: 6,
                output_height: 5,
                source_offset_x: 1,
                source_offset_y: 1,
                extras: Default::default(),
            }),
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: Some(true),
            seed: Some(42),
            prompt: Some("extend the sky".into()),
            extras: Default::default(),
        }
    }

    fn frame() -> ImageFrame {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = ((i + 1) * 7 % 256) as u8;
            px[1] = ((i + 1) * 13 % 256) as u8;
            px[2] = ((i + 1) * 29 % 256) as u8;
            px[3] = 255;
        }
        ImageFrame::new(4, 4, pixels).unwrap()
    }

    #[test]
    fn fixture_hash_is_real_and_pinned_not_placeholder() {
        let manifest = fixture_manifest(GenerativeRole::Expand);
        assert_ne!(manifest.model_hash, PENDING_INTEGRATION_HASH);
        assert!(manifest_hash_is_pinned(&manifest));
        assert!(manifest.model_hash.starts_with("sha256:"));
        assert_eq!(manifest.model_hash.len(), "sha256:".len() + 64);
        assert_eq!(
            verify_fixture_manifest(&manifest),
            ModelHashStatus::Verified,
            "the fixture pin must recompute to itself"
        );
    }

    #[test]
    fn fixture_hash_is_deterministic_and_direction_sensitive() {
        let a = fixture_manifest(GenerativeRole::Expand);
        let b = fixture_manifest(GenerativeRole::Expand);
        assert_eq!(a.model_hash, b.model_hash);

        let mut changed = a.clone();
        changed.model_version = "1.0.1".into();
        // A different spec must produce a different pin (the recomputed digest
        // no longer matches the old one).
        assert!(matches!(
            verify_fixture_manifest(&changed),
            ModelHashStatus::Mismatch { .. }
        ));
        assert_ne!(fixture_model_hash(&changed), a.model_hash);
    }

    #[test]
    fn planned_weight_descriptor_stays_loudly_pending() {
        // The *real* planned descriptors keep the placeholder — the missing
        // model is never silently replaced by the fixture.
        let real = outpaint_expand_manifest();
        assert_eq!(real.model_hash, PENDING_INTEGRATION_HASH);
        assert!(!manifest_hash_is_pinned(&real));
        assert_eq!(verify_fixture_manifest(&real), ModelHashStatus::Pending);
    }

    #[test]
    fn role_selection_reads_flags_not_names() {
        assert_eq!(
            GenerativeRole::from_edit(&expand_edit()),
            Some(GenerativeRole::Expand)
        );
        let mut auto = expand_edit();
        auto.expand_beyond_image = None;
        auto.canvas = None;
        auto.auto_fill_transparent = Some(true);
        assert_eq!(
            GenerativeRole::from_edit(&auto),
            Some(GenerativeRole::AutoFillTransparent)
        );
        let mut idle = expand_edit();
        idle.expand_beyond_image = None;
        idle.auto_fill_transparent = None;
        assert_eq!(GenerativeRole::from_edit(&idle), None);
    }

    #[test]
    fn produce_expand_is_deterministic_and_matches_core_identity() {
        let frame = frame();
        let edit = expand_edit();
        let source = GenerativeModelSource::Fixture(GenerativeRole::Expand);
        let a = produce_canvas(&frame, &edit, &source).unwrap();
        let b = produce_canvas(&frame, &edit, &source).unwrap();
        assert_eq!(a, b, "same inputs must be byte-identical");
        assert_eq!((a.width, a.height), (6, 5));
        assert_eq!(a.pixels.len(), 6 * 5 * 4);

        // The producer identity digest MUST equal the core render-side cache
        // key digest for the same tuple (no drift between production and
        // consumption) — including the prompt/model identity.
        let expected = GenerativeCacheKey::expand(
            &frame,
            edit.canvas.as_ref().unwrap(),
            edit.seed.unwrap(),
            &GenerativeIdentity::new(
                fixture_manifest(GenerativeRole::Expand).model_hash,
                edit.prompt.clone().unwrap_or_default(),
            ),
        )
        .digest();
        assert_eq!(a.identity_digest, expected);
    }

    /// GEN-ONNX-1 BLOCKER fix: the produced identity is prompt- and
    /// model-sensitive (the fixture pixels depend on the prompt). A prompt or
    /// model change must flip the identity — otherwise a stale canvas would be
    /// served as `Available` at render time.
    #[test]
    fn produce_identity_changes_with_prompt_and_model() {
        let frame = frame();
        let source = GenerativeModelSource::Fixture(GenerativeRole::Expand);
        let mut edit_a = expand_edit();
        edit_a.prompt = Some("A".into());
        let out_a = produce_canvas(&frame, &edit_a, &source).unwrap();
        let mut edit_b = edit_a.clone();
        edit_b.prompt = Some("B".into());
        let out_b = produce_canvas(&frame, &edit_b, &source).unwrap();
        assert_ne!(
            out_a.identity_digest, out_b.identity_digest,
            "a prompt change MUST flip the produced identity"
        );
        assert_ne!(
            out_a.pixels, out_b.pixels,
            "the fixture canvas is prompt-dependent (drift must be detectable)"
        );

        // Negative prompt is equally identity-bearing (GEN-ONNX-1 Welle 2a).
        let mut edit_c = edit_a.clone();
        edit_c.set_negative_prompt(Some("blurry".into()));
        let out_c = produce_canvas(&frame, &edit_c, &source).unwrap();
        assert_ne!(
            out_a.identity_digest, out_c.identity_digest,
            "a negative-prompt change MUST flip the produced identity"
        );
        assert_ne!(
            out_a.pixels, out_c.pixels,
            "the fixture canvas is negative-prompt-dependent"
        );

        let canvas = edit_a.canvas.as_ref().unwrap();
        let seed = edit_a.seed.unwrap();
        let fixture = GenerativeIdentity::new(
            fixture_manifest(GenerativeRole::Expand).model_hash,
            "A".to_owned(),
        );
        let other_model = GenerativeIdentity::new("sha256:other-model", "A".to_owned());
        assert_ne!(
            GenerativeCacheKey::expand(&frame, canvas, seed, &fixture).digest(),
            GenerativeCacheKey::expand(&frame, canvas, seed, &other_model).digest(),
            "a model-hash change MUST flip the produced identity"
        );
    }

    #[test]
    fn produce_expand_preserves_source_block() {
        let frame = frame();
        let out = produce_canvas(
            &frame,
            &expand_edit(),
            &GenerativeModelSource::Fixture(GenerativeRole::Expand),
        )
        .unwrap();
        let out_w = out.width as usize;
        for y in 0..frame.height as usize {
            for x in 0..frame.width as usize {
                let src = (y * frame.width as usize + x) * 4;
                let dst = ((y + 1) * out_w + (x + 1)) * 4;
                assert_eq!(&out.pixels[dst..dst + 4], &frame.pixels[src..src + 4]);
            }
        }
    }

    #[test]
    fn produce_auto_fill_fills_transparent_pixels() {
        let mut pixels = vec![0u8; 3 * 3 * 4];
        let c = 4 * 4;
        pixels[c] = 100;
        pixels[c + 1] = 150;
        pixels[c + 2] = 200;
        pixels[c + 3] = 255;
        let frame = ImageFrame::new(3, 3, pixels).unwrap();
        let mut edit = expand_edit();
        edit.expand_beyond_image = None;
        edit.canvas = None;
        edit.auto_fill_transparent = Some(true);
        let out = produce_canvas(
            &frame,
            &edit,
            &GenerativeModelSource::Fixture(GenerativeRole::AutoFillTransparent),
        )
        .unwrap();
        assert_eq!((out.width, out.height), (3, 3));
        assert!(!lumina_core::has_transparent_pixels(
            &out.to_frame().unwrap()
        ));
        // Identity matches the core auto-fill key (prompt+model included).
        assert_eq!(
            out.identity_digest,
            GenerativeCacheKey::auto_fill(
                &frame,
                42,
                &GenerativeIdentity::new(
                    fixture_manifest(GenerativeRole::AutoFillTransparent).model_hash,
                    edit.prompt.clone().unwrap_or_default(),
                ),
            )
            .digest()
        );
    }

    #[test]
    fn auto_fill_without_transparency_is_loud_no_artifact() {
        let frame = frame(); // fully opaque
        let mut edit = expand_edit();
        edit.expand_beyond_image = None;
        edit.canvas = None;
        edit.auto_fill_transparent = Some(true);
        let err = produce_canvas(
            &frame,
            &edit,
            &GenerativeModelSource::Fixture(GenerativeRole::AutoFillTransparent),
        )
        .unwrap_err();
        assert!(
            matches!(err, OnnxError::InferenceFailed { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn expand_without_canvas_is_rejected_loudly() {
        let frame = frame();
        let mut edit = expand_edit();
        edit.canvas = None;
        let err = produce_canvas(
            &frame,
            &edit,
            &GenerativeModelSource::Fixture(GenerativeRole::Expand),
        )
        .unwrap_err();
        assert!(matches!(err, OnnxError::InvalidManifest(_)), "got {err:?}");
    }

    #[test]
    fn artifact_source_reports_missing_model_visibly() {
        let manifest = fixture_manifest(GenerativeRole::Expand);
        let source = GenerativeModelSource::artifact(
            manifest,
            Path::new("/nonexistent/lumina/generative.onnx"),
        );
        assert!(matches!(
            source.resolve_manifest(),
            Err(OnnxError::MissingModel { .. })
        ));
    }

    #[test]
    fn artifact_source_refuses_hash_mismatch_not_silent() {
        let dir = std::env::temp_dir().join(format!(
            "lumina-gen-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.onnx");
        std::fs::write(&path, b"not the pinned bytes").unwrap();
        let mut manifest = fixture_manifest(GenerativeRole::Expand);
        manifest.model_hash = format!("sha256:{}", "0".repeat(64));
        let source = GenerativeModelSource::artifact(manifest, &path);
        let err = source.resolve_manifest().unwrap_err();
        assert!(
            matches!(err, OnnxError::ModelArtifactStale { .. }),
            "hash mismatch must be a loud stale artifact, got {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GEN-ONNX-1 F1: the real-`Artifact` path hard-refuses a
    /// `pending-integration` manifest — no pinned identity means no inference.
    #[test]
    fn artifact_source_refuses_pending_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "lumina-gen-pending-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("model.onnx");
        std::fs::write(&path, b"unverifiable bytes").unwrap();
        // The planned weight descriptor keeps the placeholder…
        let manifest = outpaint_expand_manifest();
        assert_eq!(manifest.model_hash, PENDING_INTEGRATION_HASH);
        let source = GenerativeModelSource::artifact(manifest, &path);
        // …and the real-artifact path refuses it loudly instead of inferring.
        let err = source.resolve_manifest().unwrap_err();
        assert!(
            matches!(err, OnnxError::UnsupportedModel { .. }),
            "a pending manifest on the artifact path must be refused, got {err:?}"
        );
        assert!(
            err.to_string().contains("pending-integration"),
            "the refusal must name the placeholder, got {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capability_gate_rejects_wrong_role_manifest() {
        // The fixture outpaint manifest must be refused for the auto-fill role
        // and vice versa (a model declares exactly one generative capability).
        let frame = frame();
        let mut edit = expand_edit();
        edit.expand_beyond_image = None;
        edit.canvas = None;
        edit.auto_fill_transparent = Some(true);
        // Force the outpaint fixture manifest for an auto-fill role.
        let manifest = fixture_manifest(GenerativeRole::Expand);
        let source = GenerativeModelSource::Artifact {
            path: {
                // A tiny file whose hash matches a manifest pinned to its own
                // digest, so the hash gate passes and the capability gate runs.
                let dir = std::env::temp_dir().join(format!(
                    "lumina-gen-cap-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                ));
                std::fs::create_dir_all(&dir).unwrap();
                let path = dir.join("model.onnx");
                std::fs::write(&path, b"capability-gate").unwrap();
                path
            },
            manifest: Box::new(ModelManifest {
                model_hash: format!(
                    "sha256:{}",
                    compute_sha256_hex(&b"capability-gate"[..]).unwrap()
                ),
                ..manifest
            }),
        };
        let err = produce_canvas(&frame, &edit, &source).unwrap_err();
        assert!(
            matches!(err, OnnxError::UnsupportedModel { .. }),
            "a manifest without the role capability must be refused, got {err:?}"
        );
    }

    #[test]
    fn transparent_mask_matches_core_detection() {
        let mut pixels = vec![0u8; 2 * 2 * 4];
        pixels[3] = 255; // first pixel opaque black -> still "transparent"
        pixels[4] = 10;
        pixels[5] = 20;
        pixels[6] = 30;
        pixels[7] = 255; // second pixel opaque colour
        let frame = ImageFrame::new(2, 2, pixels).unwrap();
        let mask = transparent_mask(&frame);
        assert_eq!(mask[0], 255, "opaque black counts as transparent");
        assert_eq!(mask[1], 0);
        assert_eq!(mask[2], 255);
        assert_eq!(mask[3], 255);
    }

    #[test]
    fn core_role_mapping_is_total() {
        assert_eq!(role_from_core(CoreRole::Expand), GenerativeRole::Expand);
        assert_eq!(
            role_from_core(CoreRole::AutoFillTransparent),
            GenerativeRole::AutoFillTransparent
        );
    }
}
