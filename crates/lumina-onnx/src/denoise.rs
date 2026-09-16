//! LRPAR-G14-DENOISE-IMPL-20 (ONNX slice): native KI-Denoise producer.
//!
//! SOLL: `feature/architecture/pipeline.md` §F-096a and the decision
//! `feature/decisions/LRPAR-G14-DENOISE-20.md` (§3.1 fixture path, §4
//! ONNX classification, §5 persistence, §6 status model).
//!
//! This module is the **producer** side of the optional
//! `recipe.adjustments.denoise_ai` stage. It owns
//!
//! * the additive, separate `denoise` capability descriptor
//!   ([`denoise_manifest`]) — RGB-to-RGB, full-resolution tiled inference,
//!   identity preprocessing, and
//! * the [`DenoiseModelSuite`] (descriptor + [`DenoiseTileSpec`]) with a
//!   deterministic `input_spec_digest` producer that covers inference
//!   resolution, tile size/overlap and preprocessing (a change invalidates
//!   persisted artifacts visibly),
//! * the deterministic, tests-only [`StubDenoiseBackend`] and the
//!   [`DenoiseInference`] surface (the real ORT path lives behind `onnx-rt` in
//!   [`ort`]),
//! * the tiled producer [`produce_denoise_artifact`]: overlapping tiles are
//!   assembled through the core `assemble_denoise_tiles` contract (seam-free by
//!   construction), the artifact checksum is the core/canonical RGB8 digest,
//!   and the reproducible producer provenance is persisted via
//!   `lumina_core::set_denoise_producer_provenance`,
//! * the fixture path (§3.1, GEN-ONNX-1 style): a deterministic, hash-pinned
//!   fixture suite ([`fixture_denoise_suite`]) whose `model_hash` is a real
//!   SHA-256 over the canonical fixture spec — no weights, no network.
//!
//! ## No silent fallback / `pending-integration`
//!
//! The **planned** descriptor carries
//! [`crate::hash::PENDING_INTEGRATION_HASH`] and can never report `Verified`.
//! Producing with it is a loud [`OnnxError::ModelUnavailable`]: the missing
//! real model is surfaced as the §6 status `unavailable`, never silently
//! replaced by the deterministic stub. `strength == 0`/`enabled == false`/field
//! `None` stay identity and need no model at all (the core stage owns that).
//!
//! All failures are explicit [`OnnxError`]s; nothing is clamped, repaired or
//! guessed.

#[cfg(feature = "onnx-rt")]
pub mod ort;

use std::collections::BTreeMap;

use lumina_core::{
    assemble_denoise_tiles, resolve_denoise_status, set_denoise_producer_provenance,
    DenoiseIdentity, DenoiseRgbArtifact, DenoiseStageStatus, DenoiseTile,
};
use lumina_sidecar::{
    DenoiseAi, DenoiseArtifactKind, DenoiseArtifactRef, DenoiseModelIdentity, ModelIdentity,
    DENOISE_AI_VERSION, DENOISE_PENDING_MODEL_HASH, DENOISE_SHA256_PREFIX,
};

use crate::hash::{compute_sha256_hex, PENDING_INTEGRATION_HASH};
use crate::manifest::{
    ChannelLayout, InputNormalization, ModelCapabilities, ModelInputSpec, ModelManifest,
    Resolution as ModelResolution, TensorFormat, INPUT_SPEC_DIGEST_KEY,
};
use crate::OnnxError;

/// Planned KI-Denoise model name (RGB-to-RGB denoiser, F-078 candidate class
/// only — the concrete checkpoint is not fixed yet, see the decision §3/§4).
pub const DENOISE_MODEL_NAME: &str = "denoise-rgb-v1";
/// Planned model version tag.
pub const DENOISE_MODEL_VERSION: &str = "1";
/// Pre-integration licence **declaration** (mirrors the existing planned
/// generative descriptors): the concrete weight grant is verified at F-078
/// integration time, never here.
pub const DENOISE_MODEL_LICENSE: &str = "Apache-2.0";

/// Default tiled-inference tile width (decision §4: full-resolution tiled
/// inference, e.g. 512×512 with overlap). Part of the `input_spec_digest`.
pub const DENOISE_TILE_WIDTH: u32 = 512;
/// Default tiled-inference tile height. See [`DENOISE_TILE_WIDTH`].
pub const DENOISE_TILE_HEIGHT: u32 = 512;
/// Default tile overlap in pixels (seam-free feather via the core assembly).
pub const DENOISE_TILE_OVERLAP: u32 = 32;
/// Declared canonical inference resolution width (== default tile width; the
/// denoiser is fully-convolutional, so edge tiles may be smaller).
pub const DENOISE_INFERENCE_WIDTH: u32 = DENOISE_TILE_WIDTH;
/// Declared canonical inference resolution height. See
/// [`DENOISE_INFERENCE_WIDTH`].
pub const DENOISE_INFERENCE_HEIGHT: u32 = DENOISE_TILE_HEIGHT;

/// Name of the KI-Denoise preprocessing contract (identity `[0, 1]` scaling).
pub const DENOISE_PREPROCESSING_NAME: &str = "denoise_identity_unit";
/// Version of the KI-Denoise preprocessing contract.
pub const DENOISE_PREPROCESSING_VERSION: &str = "1";
/// Documented rescaling method: the output geometry equals the input geometry
/// (no up/downsampling — unlike the mask resample).
pub const DENOISE_RESCALING_METHOD: &str = "identity";

/// Version tag of the deterministic fixture-model algorithm (GEN-ONNX-1 style,
/// decision §3.1). Changing the algorithm requires a new tag and therefore a
/// new, explicit identity — never a silent re-interpretation.
pub const DENOISE_FIXTURE_ALGORITHM: &str = "lumina-denoise-fixture-v1";
/// Leading schema tag of the denoise `input_spec_digest` text.
pub const DENOISE_INPUT_SPEC_ALGORITHM: &str = "lumina-denoise-input-spec-v1";

/// Tile geometry of a tiled KI-Denoise run.
///
/// Tile size, overlap and the (core) blend are part of the `input_spec_digest`,
/// so changing any of them invalidates persisted `denoise_rgb` artifacts
/// visibly (`feature/architecture/pipeline.md` §F-096a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenoiseTileSpec {
    /// Tile width in pixels (`> 0`).
    pub tile_width: u32,
    /// Tile height in pixels (`> 0`).
    pub tile_height: u32,
    /// Overlap in pixels; must be strictly smaller than both tile axes so the
    /// stride stays `>= 1` (otherwise the tiling would not advance).
    pub overlap: u32,
}

impl Default for DenoiseTileSpec {
    fn default() -> Self {
        Self {
            tile_width: DENOISE_TILE_WIDTH,
            tile_height: DENOISE_TILE_HEIGHT,
            overlap: DENOISE_TILE_OVERLAP,
        }
    }
}

impl DenoiseTileSpec {
    /// Build a tile spec, validating it loudly.
    pub fn new(tile_width: u32, tile_height: u32, overlap: u32) -> Result<Self, OnnxError> {
        let spec = Self {
            tile_width,
            tile_height,
            overlap,
        };
        spec.validate()?;
        Ok(spec)
    }

    /// Loud validation: non-zero tile axes and an overlap strictly smaller than
    /// both (so the per-axis stride is at least one pixel).
    pub fn validate(&self) -> Result<(), OnnxError> {
        if self.tile_width == 0 || self.tile_height == 0 {
            return Err(OnnxError::InvalidDenoiseData(format!(
                "denoise tile dimensions must be non-zero, got {}x{}",
                self.tile_width, self.tile_height
            )));
        }
        if self.overlap >= self.tile_width || self.overlap >= self.tile_height {
            return Err(OnnxError::InvalidDenoiseData(format!(
                "denoise tile overlap ({}) must be smaller than both tile axes ({}x{})",
                self.overlap, self.tile_width, self.tile_height
            )));
        }
        Ok(())
    }

    /// Horizontal advance between tile origins (`>= 1`).
    #[must_use]
    pub fn stride_x(&self) -> u32 {
        self.tile_width - self.overlap
    }

    /// Vertical advance between tile origins (`>= 1`).
    #[must_use]
    pub fn stride_y(&self) -> u32 {
        self.tile_height - self.overlap
    }

    fn canonical_text(&self) -> String {
        format!(
            "tile={}x{}|overlap={}",
            self.tile_width, self.tile_height, self.overlap
        )
    }
}

/// Build the planned KI-Denoise descriptor: declares **only** the additive
/// `denoise` capability, full-resolution RGB(RGB) identity preprocessing and
/// `pending-integration` weights (F-078 gate, decision §3.1).
#[must_use]
pub fn denoise_manifest() -> ModelManifest {
    ModelManifest {
        model_name: DENOISE_MODEL_NAME.into(),
        model_version: DENOISE_MODEL_VERSION.into(),
        model_hash: PENDING_INTEGRATION_HASH.into(),
        license: DENOISE_MODEL_LICENSE.into(),
        input: ModelInputSpec {
            resolution: ModelResolution {
                width: DENOISE_INFERENCE_WIDTH,
                height: DENOISE_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "image".into(),
            tensor_format: TensorFormat::Nchw,
            // Identity: denoisers are not ImageNet-normalized (decision §4),
            // and the preprocessing is part of the input-spec identity.
            normalization: InputNormalization::IDENTITY,
        },
        output_tensor_name: "output".into(),
        capabilities: ModelCapabilities {
            denoise: true,
            ..Default::default()
        },
    }
}

/// Whether `manifest` carries a real (non-placeholder) `model_hash`.
///
/// The planned descriptor stays `pending-integration` and can never report
/// `Verified` (decision §3.1); the deterministic fixture suite is pinned with a
/// real digest.
#[must_use]
pub fn denoise_model_hash_is_pinned(manifest: &ModelManifest) -> bool {
    manifest.model_hash != PENDING_INTEGRATION_HASH
}

/// KI-Denoise model suite: the descriptor plus the tile geometry that fully
/// determines the inference input contract.
#[derive(Debug, Clone, PartialEq)]
pub struct DenoiseModelSuite {
    /// Model descriptor (must declare `denoise`).
    pub model: ModelManifest,
    /// Tiled-inference geometry (part of the identity).
    pub tiles: DenoiseTileSpec,
}

impl DenoiseModelSuite {
    /// Build a suite from an explicit descriptor + tile spec, validating the
    /// capability and I/O contract loudly (no silent capability guessing).
    pub fn new(model: ModelManifest, tiles: DenoiseTileSpec) -> Result<Self, OnnxError> {
        let suite = Self { model, tiles };
        suite.validate()?;
        Ok(suite)
    }

    /// The planned descriptor (pending weights) with the default tile spec.
    #[must_use]
    pub fn planned() -> Self {
        Self {
            model: denoise_manifest(),
            tiles: DenoiseTileSpec::default(),
        }
    }

    /// Validate the descriptor, the tile spec and the denoise I/O contract:
    /// RGB/NCHW, identity preprocessing and a declared inference resolution
    /// that equals the tile size (so the digest describes one geometry).
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.model.validate()?;
        if !self.model.capabilities.denoise {
            return Err(OnnxError::UnsupportedModel {
                name: self.model.model_name.clone(),
                reason: "denoise not declared".into(),
            });
        }
        self.tiles.validate()?;
        if self.model.input.channel_layout != ChannelLayout::Rgb
            || self.model.input.tensor_format != TensorFormat::Nchw
        {
            return Err(OnnxError::UnsupportedModel {
                name: self.model.model_name.clone(),
                reason: "denoise models must declare an RGB NCHW input".into(),
            });
        }
        if self.model.input.normalization != InputNormalization::IDENTITY {
            return Err(OnnxError::UnsupportedModel {
                name: self.model.model_name.clone(),
                reason: "denoise models must declare identity preprocessing (mean 0, std 1)".into(),
            });
        }
        if self.model.input.resolution.width != self.tiles.tile_width
            || self.model.input.resolution.height != self.tiles.tile_height
        {
            return Err(OnnxError::InvalidDenoiseData(format!(
                "denoise inference resolution {}x{} must equal the tile size {}x{}",
                self.model.input.resolution.width,
                self.model.input.resolution.height,
                self.tiles.tile_width,
                self.tiles.tile_height
            )));
        }
        Ok(())
    }

    /// Deterministic SHA-256 identity digest of the complete denoise inference
    /// input contract (`sha256:<64 lowercase hex>`): the model input spec
    /// (inference resolution, channel layout, tensor names, tensor format,
    /// normalization) **plus** tile size/overlap and the preview/rescaling
    /// contract. Any change flips the digest and invalidates persisted
    /// artifacts visibly.
    #[must_use]
    pub fn input_spec_digest(&self) -> String {
        let digest = compute_sha256_hex(canonical_input_spec_text(self).as_bytes())
            .expect("hashing an in-memory buffer cannot fail");
        format!("{DENOISE_SHA256_PREFIX}{digest}")
    }

    /// Map the suite identity onto the sidecar [`ModelIdentity`] used by the
    /// §6 stale-detection comparison, carrying the **denoise** input-spec
    /// digest (tile geometry included) under [`INPUT_SPEC_DIGEST_KEY`].
    #[must_use]
    pub fn to_model_identity(&self) -> ModelIdentity {
        let mut identity = self.model.to_model_identity();
        identity.extras.insert(
            INPUT_SPEC_DIGEST_KEY.to_owned(),
            serde_json::Value::String(self.input_spec_digest()),
        );
        identity
    }
}

/// Canonical, versioned text form of the denoise input contract.
///
/// Encoded by hand (fixed field order, leading schema tag) so the digest has no
/// error path and is stable independent of any serializer. It embeds the
/// manifest's own deterministic input-spec digest (which hashes resolution,
/// layout, tensor names, format and normalization) and adds the tile geometry,
/// preprocessing contract and rescaling method.
fn canonical_input_spec_text(suite: &DenoiseModelSuite) -> String {
    format!(
        "{DENOISE_INPUT_SPEC_ALGORITHM}|model={}|{}|preprocessing={}:{}|rescaling={}",
        suite.model.input.identity_digest(),
        suite.tiles.canonical_text(),
        DENOISE_PREPROCESSING_NAME,
        DENOISE_PREPROCESSING_VERSION,
        DENOISE_RESCALING_METHOD,
    )
}

/// Canonical, versioned text form of the deterministic fixture specification.
fn canonical_fixture_spec(suite: &DenoiseModelSuite) -> String {
    format!(
        "{DENOISE_FIXTURE_ALGORITHM}|name={}|version={}|input_spec={}",
        suite.model.model_name,
        suite.model.model_version,
        suite.input_spec_digest(),
    )
}

/// Real SHA-256 identity of the deterministic denoise fixture model
/// (GEN-ONNX-1 style, decision §3.1).
///
/// This is **not** a weight-file hash (no weights exist); it is the content
/// hash of the exact, documented fixture specification that fully determines
/// the inference contract. [`verify_fixture_denoise_suite`] recomputes it, so
/// the pin is a real check and not a placeholder.
#[must_use]
pub fn fixture_denoise_model_hash(suite: &DenoiseModelSuite) -> String {
    let digest = compute_sha256_hex(canonical_fixture_spec(suite).as_bytes())
        .expect("hashing an in-memory buffer cannot fail");
    format!("{DENOISE_SHA256_PREFIX}{digest}")
}

/// The planned suite with the deterministic fixture pin (a real digest, not
/// `pending-integration`). Tests and the stub path use this; the planned
/// descriptor itself keeps the placeholder.
#[must_use]
pub fn fixture_denoise_suite() -> DenoiseModelSuite {
    let planned = DenoiseModelSuite::planned();
    let mut model = planned.model.clone();
    model.model_hash = fixture_denoise_model_hash(&planned);
    DenoiseModelSuite {
        model,
        tiles: planned.tiles,
    }
}

/// Verify the fixture suite's pin: recompute the spec digest and compare.
///
/// Returns [`ModelHashStatus::Verified`] only when the suite actually carries
/// the recomputed fixture hash; a `pending-integration` suite is reported as
/// [`ModelHashStatus::Pending`]; anything else is a
/// [`ModelHashStatus::Mismatch`]. Never a silent success.
#[must_use]
pub fn verify_fixture_denoise_suite(suite: &DenoiseModelSuite) -> crate::hash::ModelHashStatus {
    use crate::hash::ModelHashStatus;
    if suite.model.model_hash == PENDING_INTEGRATION_HASH {
        return ModelHashStatus::Pending;
    }
    let expected = {
        let mut canonical = suite.clone();
        canonical.model.model_hash = String::new();
        fixture_denoise_model_hash(&canonical)
    };
    if expected == suite.model.model_hash {
        ModelHashStatus::Verified
    } else {
        ModelHashStatus::Mismatch {
            expected: suite.model.model_hash.clone(),
            actual: expected,
        }
    }
}

/// Refuse a KI-Denoise suite whose weights are not pinned.
///
/// The §3.1 decision makes the missing real model a visible state: a
/// `pending-integration` descriptor is reportable as `unavailable` and must
/// never drive inference implicitly.
pub fn require_pinned_denoise_suite(suite: &DenoiseModelSuite) -> Result<(), OnnxError> {
    if !denoise_model_hash_is_pinned(&suite.model) {
        return Err(OnnxError::ModelUnavailable {
            name: suite.model.model_name.clone(),
        });
    }
    Ok(())
}

/// Build the reproducible producer identity for an artifact produced by
/// `suite` over `source_content_hash`/`decode_fingerprint`.
///
/// This is exactly the identity [`lumina_core::set_denoise_producer_provenance`]
/// persists in `DenoiseAi.extras`, and what
/// [`lumina_core::resolve_denoise_status`] compares field by field for
/// stale-detection.
#[must_use]
pub fn denoise_producer_identity(
    suite: &DenoiseModelSuite,
    source_content_hash: impl Into<String>,
    decode_fingerprint: impl Into<String>,
    artifact_checksum: impl Into<String>,
) -> DenoiseIdentity {
    DenoiseIdentity {
        source_content_hash: source_content_hash.into(),
        decode_fingerprint: decode_fingerprint.into(),
        model_name: suite.model.model_name.clone(),
        model_version: suite.model.model_version.clone(),
        model_hash: suite.model.model_hash.clone(),
        input_spec_digest: suite.input_spec_digest(),
        artifact_checksum: artifact_checksum.into(),
    }
}

/// Resolve the visible §6 status of a persisted `denoise_ai` stage.
///
/// Thin, documented delegation to the core decision layer (single source of
/// truth for the status model) so CLI/GUI callers use one entry point:
/// `pending-integration` → [`DenoiseStageStatus::Unavailable`], a changed
/// model/input-spec/provenance → `Stale`, absent artifact → `Missing`, checksum
/// mismatch → `Corrupt`, else `Ready`.
#[must_use]
pub fn denoise_stage_status(
    denoise: &DenoiseAi,
    current: &DenoiseIdentity,
    recorded: &DenoiseIdentity,
    artifact_present: bool,
) -> DenoiseStageStatus {
    resolve_denoise_status(denoise, current, recorded, artifact_present)
}

/// KI-Denoise inference surface: one RGB tile in, one RGB8 tile out.
///
/// Implementations must be deterministic for identical input and must never
/// silently substitute a different model. The output has exactly
/// `image.width * image.height * 3` bytes (row-major RGB, no alpha).
pub trait DenoiseInference {
    /// The model manifest this backend was built from.
    fn manifest(&self) -> &ModelManifest;

    /// Whether the model artifact/weights required for inference are present.
    fn is_available(&self) -> bool {
        true
    }

    /// Denoise one RGB tile, returning row-major RGB8 of the same geometry.
    fn denoise(&self, image: &lumina_core::ImageFrame) -> Result<Vec<u8>, OnnxError>;
}

/// Deterministic, weight-free KI-Denoise stub (`LRPAR-G14-DENOISE-IMPL-20`,
/// tests-only).
///
/// The default transform is a documented, integer 3×3 box mean per channel — a
/// real (if simple) low-pass that is fully deterministic and platform-
/// independent. It is explicitly **not** a production fallback: a caller that
/// needs real inference must obtain the `onnx-rt` engine, and a stub reporting
/// itself unavailable refuses inference with [`OnnxError::ModelUnavailable`].
#[derive(Debug)]
pub struct StubDenoiseBackend {
    manifest: ModelManifest,
    available: bool,
}

impl StubDenoiseBackend {
    /// Build a stub from a validated manifest that declares `denoise`.
    pub fn new(manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        if !manifest.capabilities.denoise {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "denoise not declared".into(),
            });
        }
        Ok(Self {
            manifest,
            available: true,
        })
    }

    /// Override the reported availability (simulates a missing installation).
    #[must_use]
    pub fn with_availability(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Whether this backend can currently perform inference.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.available
    }
}

impl DenoiseInference for StubDenoiseBackend {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn denoise(&self, image: &lumina_core::ImageFrame) -> Result<Vec<u8>, OnnxError> {
        if !self.available {
            return Err(OnnxError::ModelUnavailable {
                name: self.manifest.model_name.clone(),
            });
        }
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        Ok(stub_denoise_rgb(&image.pixels, image.width, image.height))
    }
}

/// Deterministic integer 3×3 box mean over the RGB channels of a row-major
/// RGBA8 frame, returning row-major RGB8 (`width * height * 3`). The window is
/// clamped to the frame; the mean is `round(sum / count)` via integer
/// arithmetic, so it is byte-identical on every platform.
fn stub_denoise_rgb(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for dy in -1i32..=1 {
                let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                for dx in -1i32..=1 {
                    let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let src = (yy * w + xx) * 4;
                    sum[0] += u32::from(pixels[src]);
                    sum[1] += u32::from(pixels[src + 1]);
                    sum[2] += u32::from(pixels[src + 2]);
                    count += 1;
                }
            }
            let dst = (y * w + x) * 3;
            for channel in 0..3 {
                out[dst + channel] = ((sum[channel] + count / 2) / count) as u8;
            }
        }
    }
    out
}

/// Convert one model output tensor into a row-major RGB8 tile.
///
/// The canonical denoise output contract is NCHW `[1, 3, H, W]` with values in
/// `[0, 1]`; a different shape, a value-count mismatch or a non-finite value is
/// a loud [`OnnxError::InferenceFailed`], never a silent reshape/clamp. Values
/// are clamped to `[0, 1]` and rounded to 8-bit.
pub fn decode_denoise_rgb(
    model_name: &str,
    shape: &[usize],
    data: &[f32],
) -> Result<Vec<u8>, OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    let (height, width) = match shape {
        [1, 3, height, width] => (*height, *width),
        _ => {
            return Err(fail(format!(
                "unexpected denoise output shape {shape:?}: expected [1, 3, H, W]"
            )))
        }
    };
    if height == 0 || width == 0 {
        return Err(fail(format!(
            "denoise output spatial dimensions must be non-zero, got {height}x{width}"
        )));
    }
    let plane = height * width;
    if data.len() != plane * 3 {
        return Err(fail(format!(
            "denoise output holds {} values for shape {height}x{width}, expected {}",
            data.len(),
            plane * 3
        )));
    }
    let mut out = vec![0u8; plane * 3];
    for index in 0..plane {
        for channel in 0..3 {
            let value = data[channel * plane + index];
            if !value.is_finite() {
                return Err(fail(format!(
                    "denoise output holds a non-finite value at channel {channel}, pixel {index}"
                )));
            }
            out[index * 3 + channel] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Ok(out)
}

/// One KI-Denoise production request.
#[derive(Debug, Clone)]
pub struct DenoiseRequest<'a> {
    /// Persisted blend strength `0..=1` (core applies the actual blend).
    pub strength: f32,
    /// Persisted edge/detail protection `0..=1`.
    pub preserve_detail: f32,
    /// Live source content hash (for the producer provenance).
    pub source_content_hash: &'a str,
    /// Live decode fingerprint (for the producer provenance).
    pub decode_fingerprint: &'a str,
    /// Portable relative path of the `.lumina.zdata` bundle holding the result.
    pub artifact_relative_path: &'a str,
}

/// A produced denoise artifact plus the complete, ready-to-persist recipe
/// stage.
#[derive(Debug, Clone, PartialEq)]
pub struct DenoiseProduced {
    /// Full-frame RGB8 artifact (canonical checksum, identical to the zdata
    /// record checksum).
    pub artifact: DenoiseRgbArtifact,
    /// The `denoise_ai` stage with the artifact reference and the persisted
    /// producer provenance.
    pub recipe: DenoiseAi,
}

/// Produce the full-frame KI-Denoise artifact with tiled inference and assemble
/// it seam-free through the core `assemble_denoise_tiles` contract.
///
/// * The suite must be pinned ([`require_pinned_denoise_suite`]); a
///   `pending-integration` descriptor is a loud `ModelUnavailable`.
/// * The backend manifest must equal the suite descriptor (no silent
///   model/suite drift) and must be available.
/// * Overlapping tiles advance by `tile - overlap`; each tile is denoised by
///   the backend and the artifact is the weighted (distance-to-edge) assembly,
///   so constant tiles reconstruct exactly and no seam can appear.
/// * The recipe carries the artifact reference (checksum = canonical core
///   digest) and the reproducible producer provenance, so
///   [`denoise_stage_status`] can later prove `Ready`/`Stale`.
pub fn produce_denoise_artifact(
    frame: &lumina_core::ImageFrame,
    suite: &DenoiseModelSuite,
    backend: &dyn DenoiseInference,
    request: &DenoiseRequest<'_>,
) -> Result<DenoiseProduced, OnnxError> {
    suite.validate()?;
    require_pinned_denoise_suite(suite)?;
    if backend.manifest() != &suite.model {
        return Err(OnnxError::UnsupportedModel {
            name: suite.model.model_name.clone(),
            reason: "backend manifest does not match the denoise suite descriptor".into(),
        });
    }
    if !backend.is_available() {
        return Err(OnnxError::ModelUnavailable {
            name: suite.model.model_name.clone(),
        });
    }
    for (field, value) in [
        ("strength", request.strength),
        ("preserve_detail", request.preserve_detail),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(OnnxError::InvalidDenoiseData(format!(
                "denoise {field} must be finite within 0..=1, got {value}"
            )));
        }
    }
    if frame.width == 0 || frame.height == 0 {
        return Err(OnnxError::InvalidDimensions {
            expected_width: suite.tiles.tile_width,
            expected_height: suite.tiles.tile_height,
            actual_width: frame.width,
            actual_height: frame.height,
        });
    }

    // Row-major RGB8 tiles covering the whole frame.
    let mut tile_pixels: Vec<Vec<u8>> = Vec::new();
    let mut y = 0u32;
    while y < frame.height {
        let tile_height = suite.tiles.tile_height.min(frame.height - y);
        let mut x = 0u32;
        while x < frame.width {
            let tile_width = suite.tiles.tile_width.min(frame.width - x);
            let tile = tile_frame(frame, x, y, tile_width, tile_height);
            let produced = backend.denoise(&tile)?;
            let expected = tile_width as usize * tile_height as usize * 3;
            if produced.len() != expected {
                return Err(OnnxError::InvalidDenoiseData(format!(
                    "denoise backend produced {} bytes for a {}x{} tile, expected {expected}",
                    produced.len(),
                    tile_width,
                    tile_height
                )));
            }
            tile_pixels.push(produced);
            x += suite.tiles.stride_x();
        }
        y += suite.tiles.stride_y();
    }

    let tiles: Vec<DenoiseTile<'_>> = {
        let mut descriptors = Vec::with_capacity(tile_pixels.len());
        let mut index = 0usize;
        let mut y = 0u32;
        while y < frame.height {
            let tile_height = suite.tiles.tile_height.min(frame.height - y);
            let mut x = 0u32;
            while x < frame.width {
                let tile_width = suite.tiles.tile_width.min(frame.width - x);
                descriptors.push(DenoiseTile {
                    x,
                    y,
                    width: tile_width,
                    height: tile_height,
                    pixels: &tile_pixels[index],
                });
                index += 1;
                x += suite.tiles.stride_x();
            }
            y += suite.tiles.stride_y();
        }
        descriptors
    };

    let artifact = assemble_denoise_tiles(frame.width, frame.height, &tiles)
        .map_err(|error| OnnxError::InvalidDenoiseData(error.to_string()))?;
    if artifact.width != frame.width || artifact.height != frame.height {
        return Err(OnnxError::InvalidDenoiseData(format!(
            "assembled denoise artifact is {}x{}, expected {}x{}",
            artifact.width, artifact.height, frame.width, frame.height
        )));
    }

    let identity = denoise_producer_identity(
        suite,
        request.source_content_hash,
        request.decode_fingerprint,
        artifact.checksum(),
    );
    let mut recipe = DenoiseAi {
        version: DENOISE_AI_VERSION,
        enabled: true,
        model: DenoiseModelIdentity {
            name: suite.model.model_name.clone(),
            version: suite.model.model_version.clone(),
            model_hash: suite.model.model_hash.clone(),
            extras: BTreeMap::new(),
        },
        input_spec_digest: suite.input_spec_digest(),
        strength: request.strength,
        preserve_detail: request.preserve_detail,
        artifact: Some(DenoiseArtifactRef {
            kind: DenoiseArtifactKind::DenoiseRgb,
            relative_path: request.artifact_relative_path.to_owned(),
            format: "lumina-zdata".into(),
            checksum: artifact.checksum(),
            width: artifact.width,
            height: artifact.height,
            channels: "rgb8".into(),
            data_version: lumina_core::DENOISE_RGB_ENCODING_VERSION.to_string(),
            extras: BTreeMap::new(),
        }),
        extras: BTreeMap::new(),
    };
    set_denoise_producer_provenance(&mut recipe, &identity);

    Ok(DenoiseProduced { artifact, recipe })
}

/// Copy one RGB tile out of a row-major RGBA8 frame into a standalone frame
/// (alpha is set to `255`; the denoise stage never consumes alpha).
fn tile_frame(
    frame: &lumina_core::ImageFrame,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> lumina_core::ImageFrame {
    let frame_width = frame.width as usize;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        for column in 0..width as usize {
            let src = ((y as usize + row) * frame_width + (x as usize + column)) * 4;
            let dst = (row * width as usize + column) * 4;
            pixels[dst..dst + 3].copy_from_slice(&frame.pixels[src..src + 3]);
            pixels[dst + 3] = 255;
        }
    }
    lumina_core::ImageFrame::new(width, height, pixels)
        .expect("tile dimensions are inside the validated frame")
}

/// The `pending-integration` marker re-exported for callers that classify the
/// §6 status themselves (`denoise.model.model_hash == DENOISE_PENDING_MODEL_HASH`).
pub const DENOISE_PENDING_HASH: &str = DENOISE_PENDING_MODEL_HASH;

/// A resolvable real KI-Denoise engine — or the explicit statement that this
/// build cannot provide one.
///
/// Mirrors [`crate::resolve::OnnxEngine`] / [`crate::FaceOnnxEngine`] so a
/// CLI/core caller can obtain the real denoiser without a silent fallback.
pub enum DenoiseOnnxEngine {
    /// Real, artifact-verified ONNX Runtime denoiser. Only exists when the
    /// `onnx-rt` feature is compiled in.
    #[cfg(feature = "onnx-rt")]
    OnnxRuntime(Box<ort::OrtDenoiser>),
    /// The `onnx-rt` capability is not compiled into this build. A deliberate,
    /// visible state — never a silent stub fallback.
    RuntimeDisabled,
}

impl std::fmt::Debug for DenoiseOnnxEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "onnx-rt")]
            DenoiseOnnxEngine::OnnxRuntime(_) => f.write_str("OnnxRuntime(<real denoiser>)"),
            DenoiseOnnxEngine::RuntimeDisabled => f.write_str("RuntimeDisabled"),
        }
    }
}

/// Attempt to load the real KI-Denoise engine for `manifest`.
///
/// # Contract (no silent fallback)
///
/// | situation | result |
/// | --- | --- |
/// | manifest invalid or lacks `denoise` | `Err(UnsupportedModel / InvalidManifest)` |
/// | `pending-integration` (no pinned identity) | `Err(ModelUnavailable)` — the visible §6 `unavailable` state |
/// | `onnx-rt` **off**, pinned manifest | `Ok(RuntimeDisabled)` (the caller decides explicitly) |
/// | `onnx-rt` **on**, artifact missing/unreadable | `Err(MissingModel)` |
/// | `onnx-rt` **on**, digest ≠ pinned hash | loads, then refuses inference with `ModelArtifactStale` |
/// | `onnx-rt` **on**, declared tensor name absent | `Err(InferenceFailed)` |
#[cfg_attr(not(feature = "onnx-rt"), allow(unused_variables))]
pub fn try_load_denoise_engine(
    model_path: &std::path::Path,
    manifest: ModelManifest,
) -> Result<DenoiseOnnxEngine, OnnxError> {
    manifest.validate()?;
    if !manifest.capabilities.denoise {
        return Err(OnnxError::UnsupportedModel {
            name: manifest.model_name.clone(),
            reason: "denoise not declared".into(),
        });
    }
    if manifest.model_hash == PENDING_INTEGRATION_HASH {
        return Err(OnnxError::ModelUnavailable {
            name: manifest.model_name,
        });
    }
    #[cfg(feature = "onnx-rt")]
    {
        let backend = ort::OrtDenoiser::new(model_path, manifest)?;
        Ok(DenoiseOnnxEngine::OnnxRuntime(Box::new(backend)))
    }
    #[cfg(not(feature = "onnx-rt"))]
    {
        Ok(DenoiseOnnxEngine::RuntimeDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::ModelHashStatus;
    use crate::{InputNormalization as Normalization, ModelCapabilities as Capabilities};

    fn tiny_manifest() -> ModelManifest {
        ModelManifest {
            model_name: "tiny-denoise".into(),
            model_version: "1".into(),
            model_hash: PENDING_INTEGRATION_HASH.into(),
            license: "MIT".into(),
            input: ModelInputSpec {
                resolution: ModelResolution {
                    width: 4,
                    height: 4,
                },
                channel_layout: ChannelLayout::Rgb,
                tensor_name: "image".into(),
                tensor_format: TensorFormat::Nchw,
                normalization: Normalization::IDENTITY,
            },
            output_tensor_name: "output".into(),
            capabilities: Capabilities {
                denoise: true,
                ..Default::default()
            },
        }
    }

    fn tiny_suite() -> DenoiseModelSuite {
        DenoiseModelSuite::new(tiny_manifest(), DenoiseTileSpec::new(4, 4, 1).unwrap()).unwrap()
    }

    /// A `tile`×`tile` suite with the deterministic fixture pin (real hash).
    fn pinned_suite(tile: u32, overlap: u32) -> DenoiseModelSuite {
        let mut model = tiny_manifest();
        model.input.resolution = ModelResolution {
            width: tile,
            height: tile,
        };
        let planned =
            DenoiseModelSuite::new(model, DenoiseTileSpec::new(tile, tile, overlap).unwrap())
                .unwrap();
        let mut pinned = planned.clone();
        pinned.model.model_hash = fixture_denoise_model_hash(&planned);
        DenoiseModelSuite::new(pinned.model, pinned.tiles).unwrap()
    }

    #[test]
    fn planned_descriptor_declares_only_denoise_and_stays_pending() {
        let manifest = denoise_manifest();
        assert!(manifest.capabilities.denoise);
        assert!(!manifest.capabilities.subject_segmentation);
        assert!(!manifest.capabilities.face_detect);
        assert!(!manifest.capabilities.face_embed);
        assert!(!manifest.capabilities.inpaint_heal);
        assert!(!manifest.capabilities.outpaint);
        assert_eq!(manifest.model_hash, PENDING_INTEGRATION_HASH);
        assert!(!denoise_model_hash_is_pinned(&manifest));
        assert!(manifest.validate().is_ok());
        assert_eq!(manifest.input.normalization, InputNormalization::IDENTITY);
        assert!(matches!(
            verify_fixture_denoise_suite(&DenoiseModelSuite::planned()),
            ModelHashStatus::Pending
        ));
    }

    #[test]
    fn fixture_suite_is_really_pinned_and_direction_sensitive() {
        let fixture = fixture_denoise_suite();
        assert!(denoise_model_hash_is_pinned(&fixture.model));
        assert!(fixture.model.model_hash.starts_with("sha256:"));
        assert_eq!(fixture.model.model_hash.len(), "sha256:".len() + 64);
        assert_eq!(
            verify_fixture_denoise_suite(&fixture),
            ModelHashStatus::Verified
        );
        assert!(fixture.validate().is_ok());

        // Deterministic across calls.
        assert_eq!(
            fixture.model.model_hash,
            fixture_denoise_suite().model.model_hash
        );

        // A changed spec flips the pin (the recomputed digest no longer
        // matches the stored one)…
        let mut changed = fixture.clone();
        changed.model.model_version = "2".into();
        assert!(matches!(
            verify_fixture_denoise_suite(&changed),
            ModelHashStatus::Mismatch { .. }
        ));
        assert_ne!(
            fixture_denoise_model_hash(&changed),
            fixture.model.model_hash
        );
    }

    #[test]
    fn input_spec_digest_is_deterministic_and_covers_tiles_and_model() {
        let suite = tiny_suite();
        let digest = suite.input_spec_digest();
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), "sha256:".len() + 64);
        assert_eq!(digest, tiny_suite().input_spec_digest());

        // Tile geometry is identity-bearing.
        let mut tile_changed = suite.clone();
        tile_changed.tiles = DenoiseTileSpec::new(4, 4, 2).unwrap();
        assert_ne!(digest, tile_changed.input_spec_digest());

        // Inference resolution (part of the manifest input spec) is too.
        let mut resolution_changed = suite.clone();
        resolution_changed.model.input.resolution = ModelResolution {
            width: 4,
            height: 4,
        };
        resolution_changed.model.input.tensor_name = "pixels".into();
        assert_ne!(digest, resolution_changed.input_spec_digest());

        // The persisted model identity carries the denoise digest (not the
        // plain model input digest).
        let identity = suite.to_model_identity();
        let stored = identity
            .extras
            .get(INPUT_SPEC_DIGEST_KEY)
            .expect("identity must carry the input-spec digest");
        assert_eq!(stored, &serde_json::Value::String(digest));
    }

    #[test]
    fn suite_rejects_wrong_capability_and_contract() {
        let mut wrong_capability = tiny_manifest();
        wrong_capability.capabilities = Capabilities {
            face_detect: true,
            ..Default::default()
        };
        assert!(matches!(
            DenoiseModelSuite::new(wrong_capability, DenoiseTileSpec::default()),
            Err(OnnxError::UnsupportedModel { .. })
        ));

        let mut wrong_norm = tiny_manifest();
        wrong_norm.input.normalization = InputNormalization::IMAGENET;
        assert!(matches!(
            DenoiseModelSuite::new(wrong_norm, DenoiseTileSpec::new(4, 4, 0).unwrap()),
            Err(OnnxError::UnsupportedModel { .. })
        ));

        let mut wrong_resolution = tiny_manifest();
        wrong_resolution.input.resolution = ModelResolution {
            width: 8,
            height: 8,
        };
        assert!(matches!(
            DenoiseModelSuite::new(wrong_resolution, DenoiseTileSpec::new(4, 4, 1).unwrap()),
            Err(OnnxError::InvalidDenoiseData(_))
        ));
    }

    #[test]
    fn tile_spec_validation_is_loud() {
        assert!(DenoiseTileSpec::new(0, 4, 0).is_err());
        assert!(DenoiseTileSpec::new(4, 0, 0).is_err());
        assert!(DenoiseTileSpec::new(4, 4, 4).is_err());
        assert!(DenoiseTileSpec::new(4, 4, 5).is_err());
        let ok = DenoiseTileSpec::new(4, 4, 1).unwrap();
        assert_eq!(ok.stride_x(), 3);
        assert_eq!(ok.stride_y(), 3);
    }

    #[test]
    fn decode_denoise_rgb_converts_nchw_and_is_loud() {
        // [1, 3, 2, 1]: R plane [0.0, 1.0], G plane [0.5, 0.5], B plane [0, 0].
        let out = decode_denoise_rgb("M", &[1, 3, 2, 1], &[0.0, 1.0, 0.5, 0.5, 0.0, 0.0]).unwrap();
        assert_eq!(out, vec![0, 128, 0, 255, 128, 0]);

        // Wrong rank/channels, wrong value count, non-finite → loud.
        assert!(decode_denoise_rgb("M", &[1, 1, 2, 2], &[0.0; 4]).is_err());
        assert!(decode_denoise_rgb("M", &[1, 3, 2, 2], &[0.0; 4]).is_err());
        assert!(
            decode_denoise_rgb("M", &[1, 3, 1, 2], &[0.0, f32::NAN, 0.0, 0.0, 0.0, 0.0]).is_err()
        );
    }

    #[test]
    fn stub_is_deterministic_and_available_gate_is_loud() {
        let stub = StubDenoiseBackend::new(tiny_manifest()).unwrap();
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = (i * 11 % 256) as u8;
            px[1] = (i * 17 % 256) as u8;
            px[2] = (i * 23 % 256) as u8;
            px[3] = 255;
        }
        let frame = lumina_core::ImageFrame::new(4, 4, pixels).unwrap();
        let a = stub.denoise(&frame).unwrap();
        let b = stub.denoise(&frame).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 4 * 4 * 3);

        let unavailable = StubDenoiseBackend::new(tiny_manifest())
            .unwrap()
            .with_availability(false);
        assert!(matches!(
            unavailable.denoise(&frame),
            Err(OnnxError::ModelUnavailable { .. })
        ));
        assert!(matches!(
            stub.denoise(&lumina_core::ImageFrame::new(0, 0, vec![]).unwrap()),
            Err(OnnxError::InvalidDimensions { .. })
        ));

        // A manifest without the capability is refused loudly.
        let mut wrong = tiny_manifest();
        wrong.capabilities = Capabilities {
            subject_segmentation: true,
            ..Default::default()
        };
        assert!(matches!(
            StubDenoiseBackend::new(wrong),
            Err(OnnxError::UnsupportedModel { .. })
        ));
    }

    #[test]
    fn producer_pending_manifest_is_refused_loudly() {
        let stub = StubDenoiseBackend::new(tiny_manifest()).unwrap();
        let frame = lumina_core::ImageFrame::new(4, 4, vec![128u8; 4 * 4 * 4]).unwrap();
        let request = DenoiseRequest {
            strength: 0.5,
            preserve_detail: 0.0,
            source_content_hash: "blake3:src",
            decode_fingerprint: "libraw:1",
            artifact_relative_path: "IMG.lumina.zdata",
        };
        let err = produce_denoise_artifact(&frame, &tiny_suite(), &stub, &request).unwrap_err();
        assert!(
            matches!(err, OnnxError::ModelUnavailable { .. }),
            "a pending-integration suite must be refused, got {err:?}"
        );
    }

    #[test]
    fn producer_is_seamless_for_constant_tiles_and_deterministic() {
        // A small, fixture-pinned suite (4x4 tile, 1px overlap).
        let small = pinned_suite(4, 1);
        let stub = StubDenoiseBackend::new(small.model.clone()).unwrap();

        // Constant frame: box mean is constant → no seam.
        let mut constant = vec![0u8; 6 * 5 * 4];
        for px in constant.as_chunks_mut::<4>().0.iter_mut() {
            px[0] = 40;
            px[1] = 50;
            px[2] = 60;
            px[3] = 255;
        }
        let frame = lumina_core::ImageFrame::new(6, 5, constant).unwrap();
        let request = DenoiseRequest {
            strength: 0.5,
            preserve_detail: 0.0,
            source_content_hash: "blake3:src",
            decode_fingerprint: "libraw:1",
            artifact_relative_path: "IMG.lumina.zdata",
        };
        let a = produce_denoise_artifact(&frame, &small, &stub, &request).unwrap();
        let b = produce_denoise_artifact(&frame, &small, &stub, &request).unwrap();
        assert_eq!(a, b, "same inputs must be byte-identical");
        assert_eq!((a.artifact.width, a.artifact.height), (6, 5));
        assert!(
            a.artifact
                .pixels
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| *p == [40, 50, 60]),
            "constant tiles must assemble a constant, seam-free frame"
        );
        // The recipe references exactly the produced artifact.
        assert_eq!(
            a.recipe.artifact.as_ref().unwrap().checksum,
            a.artifact.checksum()
        );
        assert_eq!(
            a.recipe.model.model_hash, small.model.model_hash,
            "the recipe pins the producing model"
        );
    }

    #[test]
    fn engine_resolver_refuses_pending_and_reports_capability_absence() {
        // The pending placeholder is a loud `unavailable`, in every build —
        // never a silent `RuntimeDisabled` or a stub.
        assert!(matches!(
            try_load_denoise_engine(
                std::path::Path::new("/nonexistent/denoise.onnx"),
                denoise_manifest()
            ),
            Err(OnnxError::ModelUnavailable { .. })
        ));
        let mut wrong = tiny_manifest();
        wrong.capabilities = Capabilities {
            subject_segmentation: true,
            ..Default::default()
        };
        assert!(matches!(
            try_load_denoise_engine(std::path::Path::new("/nonexistent/denoise.onnx"), wrong),
            Err(OnnxError::UnsupportedModel { .. })
        ));

        // A pinned manifest without the `onnx-rt` feature yields the explicit
        // capability statement (the ORT path itself is covered in
        // `tests/denoise_ort.rs`).
        #[cfg(not(feature = "onnx-rt"))]
        {
            let pinned = pinned_suite(4, 1);
            let engine = try_load_denoise_engine(
                std::path::Path::new("/nonexistent/denoise.onnx"),
                pinned.model,
            )
            .expect("feature-off load must succeed with the explicit flag");
            assert!(
                matches!(engine, DenoiseOnnxEngine::RuntimeDisabled),
                "expected the explicit capability statement, got {engine:?}"
            );
        }
    }

    #[test]
    fn producer_records_provenance_and_status_resolution_is_visible() {
        use lumina_core::denoise_producer_provenance;
        use lumina_core::DenoiseStageStatus;

        let suite = pinned_suite(4, 1);
        let stub = StubDenoiseBackend::new(suite.model.clone()).unwrap();
        let frame = lumina_core::ImageFrame::new(4, 4, [10u8, 20, 30, 255].repeat(16)).unwrap();
        let request = DenoiseRequest {
            strength: 0.5,
            preserve_detail: 0.0,
            source_content_hash: "blake3:src",
            decode_fingerprint: "libraw:1",
            artifact_relative_path: "IMG.lumina.zdata",
        };
        let produced = produce_denoise_artifact(&frame, &suite, &stub, &request).unwrap();

        let recorded = denoise_producer_provenance(&produced.recipe)
            .expect("the producer must persist its provenance");
        let current = denoise_producer_identity(
            &suite,
            "blake3:src",
            "libraw:1",
            produced.artifact.checksum(),
        );
        assert_eq!(recorded, current);
        assert_eq!(
            denoise_stage_status(&produced.recipe, &current, &recorded, true),
            DenoiseStageStatus::Ready
        );

        // Model change → stale.
        let mut changed = current.clone();
        changed.model_version = "9".into();
        assert_eq!(
            denoise_stage_status(&produced.recipe, &changed, &recorded, true),
            DenoiseStageStatus::Stale
        );
        // Producer-provenance drift → stale.
        let mut drift = recorded.clone();
        drift.input_spec_digest = format!("{DENOISE_SHA256_PREFIX}{}", "00".repeat(32));
        assert_eq!(
            denoise_stage_status(&produced.recipe, &current, &drift, true),
            DenoiseStageStatus::Stale
        );
        // Absent artifact → missing; checksum mismatch → corrupt.
        assert_eq!(
            denoise_stage_status(&produced.recipe, &current, &recorded, false),
            DenoiseStageStatus::Missing
        );
        let mut corrupt = current.clone();
        corrupt.artifact_checksum = "00".repeat(32);
        assert_eq!(
            denoise_stage_status(&produced.recipe, &corrupt, &recorded, true),
            DenoiseStageStatus::Corrupt
        );
        // pending-integration → unavailable even with a perfect artifact.
        let mut pending = produced.recipe.clone();
        pending.model.model_hash = DENOISE_PENDING_MODEL_HASH.into();
        assert_eq!(
            denoise_stage_status(&pending, &current, &recorded, true),
            DenoiseStageStatus::Unavailable
        );
    }
}
