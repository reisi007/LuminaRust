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
//!   deterministic v2 `input_spec_digest` over model, tile, preprocessing and
//!   versioned core blend/distance-to-edge assembly contracts,
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

mod input_spec;
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

pub use input_spec::{
    canonical_input_spec_text, input_spec_digest, DenoiseBlendIdentity, DenoiseInputSpecContract,
    DenoiseInputSpecParameters, DenoiseOutputIdentity, DenoiseTileAssemblyIdentity,
    DENOISE_ALPHA_POLICY, DENOISE_BLEND_ALGORITHM, DENOISE_BLEND_ALGORITHM_VERSION,
    DENOISE_BLEND_FORMULA, DENOISE_BLEND_ROUNDING, DENOISE_DEFAULT_FIXTURE_MODEL_HASH,
    DENOISE_DEFAULT_INPUT_SPEC_DIGEST, DENOISE_DETAIL_SCALE_UNITS, DENOISE_DETAIL_WINDOW,
    DENOISE_INPUT_SPEC_ALGORITHM, DENOISE_LUMINANCE_STANDARD, DENOISE_OUTPUT_ENCODING,
    DENOISE_OUTPUT_LAYOUT, DENOISE_OUTPUT_RANGE, DENOISE_OUTPUT_SHAPE,
    DENOISE_TILE_ASSEMBLY_ALGORITHM, DENOISE_TILE_ASSEMBLY_ALGORITHM_VERSION,
    DENOISE_TILE_ASSEMBLY_MATH, DENOISE_TILE_ASSEMBLY_ROUNDING, DENOISE_TILE_ASSEMBLY_WEIGHT,
    DENOISE_TILE_COVERAGE, DENOISE_WORKING_SPACE, DENOISE_ZERO_STRENGTH_POLICY,
};

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

/// Tile geometry of a tiled KI-Denoise run; size/overlap and core blend are part of the v2 `input_spec_digest` (F-096a).
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
}

/// Build the planned KI-Denoise descriptor: additive `denoise`, full-resolution
/// RGB identity preprocessing and `pending-integration` weights (F-078 gate).
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

/// Whether `manifest` carries a real (non-placeholder) `model_hash`; the planned
/// descriptor stays `pending-integration` (F-078 gate).
#[must_use]
pub fn denoise_model_hash_is_pinned(manifest: &ModelManifest) -> bool {
    manifest.model_hash != PENDING_INTEGRATION_HASH
}

/// KI-Denoise model suite: descriptor plus tile geometry defining the input contract.
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

    /// SHA-256 identity (`sha256:<64 lowercase hex>`) of the model input,
    /// tile/preprocessing/rescaling and versioned core blend/assembly contract.
    /// The v2 tag makes every behavior change visibly stale; v1 is not reused.
    #[must_use]
    pub fn input_spec_digest(&self) -> String {
        let model_input_digest = self.model.input.identity_digest();
        let parameters = DenoiseInputSpecParameters {
            model_input_digest: &model_input_digest,
            input_tensor_name: &self.model.input.tensor_name,
            output_tensor_name: &self.model.output_tensor_name,
            tile_width: self.tiles.tile_width,
            tile_height: self.tiles.tile_height,
            overlap: self.tiles.overlap,
            preprocessing_name: DENOISE_PREPROCESSING_NAME,
            preprocessing_version: DENOISE_PREPROCESSING_VERSION,
            rescaling_method: DENOISE_RESCALING_METHOD,
        };
        input_spec_digest(&parameters, &DenoiseInputSpecContract::default())
    }

    /// Map the suite identity onto the sidecar [`ModelIdentity`] used by the
    /// §6 stale-detection comparison, carrying the **denoise** input-spec
    /// digest (model/tile/core behavior included) under [`INPUT_SPEC_DIGEST_KEY`].
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

/// Digest of the documented default denoise input contract.
///
/// This helper computes the contract from the same [`DenoiseModelSuite`]
/// producer used by the ONNX path; it does not select, load, or promote a
/// model.  GUI and other recipe builders can use it instead of maintaining a
/// second hard-coded digest.
#[must_use]
pub fn default_denoise_input_spec_digest() -> String {
    DenoiseModelSuite::planned().input_spec_digest()
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
        if image.width() == 0 || image.height() == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width(),
                actual_height: image.height(),
            });
        }
        Ok(stub_denoise_rgb(
            image.pixels(),
            image.width(),
            image.height(),
        ))
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
    if frame.width() == 0 || frame.height() == 0 {
        return Err(OnnxError::InvalidDimensions {
            expected_width: suite.tiles.tile_width,
            expected_height: suite.tiles.tile_height,
            actual_width: frame.width(),
            actual_height: frame.height(),
        });
    }

    // Row-major RGB8 tiles covering the whole frame.
    let mut tile_pixels: Vec<Vec<u8>> = Vec::new();
    let mut y = 0u32;
    while y < frame.height() {
        let tile_height = suite.tiles.tile_height.min(frame.height() - y);
        let mut x = 0u32;
        while x < frame.width() {
            let tile_width = suite.tiles.tile_width.min(frame.width() - x);
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
        while y < frame.height() {
            let tile_height = suite.tiles.tile_height.min(frame.height() - y);
            let mut x = 0u32;
            while x < frame.width() {
                let tile_width = suite.tiles.tile_width.min(frame.width() - x);
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

    let artifact = assemble_denoise_tiles(frame.width(), frame.height(), &tiles)
        .map_err(|error| OnnxError::InvalidDenoiseData(error.to_string()))?;
    if artifact.width != frame.width() || artifact.height != frame.height() {
        return Err(OnnxError::InvalidDenoiseData(format!(
            "assembled denoise artifact is {}x{}, expected {}x{}",
            artifact.width,
            artifact.height,
            frame.width(),
            frame.height()
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
    let frame_width = frame.width() as usize;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        for column in 0..width as usize {
            let src = ((y as usize + row) * frame_width + (x as usize + column)) * 4;
            let dst = (row * width as usize + column) * 4;
            pixels[dst..dst + 3].copy_from_slice(&frame.pixels()[src..src + 3]);
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
mod tests;
