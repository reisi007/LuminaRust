//! Model manifest and capabilities (F-080).
//!
//! [`ModelManifest`] is the serializable identity + I/O contract of an ONNX
//! model; [`ModelCapabilities`] enumerates what a model can do. At least one
//! capability must be set, and unknown fields are rejected, so a manifest can
//! never silently claim a capability it does not declare.

use crate::OnnxError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod types;

pub use types::{ChannelLayout, InputNormalization, Resolution, TensorFormat};

/// Input tensor specification for an ONNX model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInputSpec {
    /// Inference (preprocessing) resolution.
    pub resolution: Resolution,
    /// Channel layout of the RGB input.
    pub channel_layout: ChannelLayout,
    /// Name of the input tensor in the ONNX graph.
    pub tensor_name: String,
    /// Tensor memory layout expected by the model.
    pub tensor_format: TensorFormat,
    /// Input normalization applied before the tensor is filled.
    #[serde(default = "InputNormalization::imagenet")]
    pub normalization: InputNormalization,
}

/// Model capabilities (F-080).
///
/// `subject_segmentation` is the *base* capability (automatic subject
/// segmentation, e.g. BiRefNet) with no prompts. The box/point/mask/class/
/// instance flags are the interactive/advanced capabilities enumerated in
/// F-080; `inpaint_heal` (SPOT-REMOVE-1) and `outpaint` (GEN-EXPAND-1) are the
/// generative capabilities; `face_detect`/`face_embed` are the face-pipeline
/// capabilities (LRPAR-G12-FACE-20). **At least one capability must be set** —
/// see [`ModelCapabilities::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    /// Automatic subject segmentation (no prompts required).
    pub subject_segmentation: bool,
    /// Accepts a bounding-box prompt.
    pub box_prompt: bool,
    /// Accepts point (click) prompts.
    pub point_prompt: bool,
    /// Accepts a mask prompt.
    pub mask_prompt: bool,
    /// Produces a class label / detection.
    pub class_detection: bool,
    /// Produces per-instance segmentations.
    pub instance_segmentation: bool,
    /// Spot-heal generative inpaint (SPOT-REMOVE-01, local ONNX).
    #[serde(default)]
    pub inpaint_heal: bool,
    /// Generative canvas expansion / outpainting (GEN-EXPAND-1, local ONNX).
    ///
    /// Declares that the model can extend the image beyond its original
    /// surface (`canvas.output_* > source_*`, ">100 %"). Checked by the
    /// outpaint stub backend before inference; a model without this flag is
    /// rejected visibly (no silent substitution by another model).
    #[serde(default)]
    pub outpaint: bool,
    /// Face detection (LRPAR-G12-FACE-20 / FACE-20-S2, local ONNX).
    ///
    /// Declares that the model turns an RGB input into detected face boxes +
    /// scores + landmarks. Required by
    /// [`crate::face::FaceDetectionInference`] backends; a manifest without
    /// this flag is refused visibly (never substituted by another model).
    /// Additive `#[serde(default)]` field: manifests written before it existed
    /// keep parsing unchanged.
    #[serde(default)]
    pub face_detect: bool,
    /// Face embedding (LRPAR-G12-FACE-20 / FACE-20-S2, local ONNX).
    ///
    /// Declares that the model turns an aligned face crop into a normalized
    /// identity vector. Required by [`crate::face::FaceEmbeddingInference`]
    /// backends. Additive `#[serde(default)]` field: manifests written before
    /// it existed keep parsing unchanged.
    #[serde(default)]
    pub face_embed: bool,
    /// KI-Denoise RGB-to-RGB (LRPAR-G14-DENOISE-IMPL-20, local ONNX).
    ///
    /// Declares that the model turns an RGB raster into a denoised RGB raster
    /// of identical geometry. Required by
    /// [`crate::denoise::DenoiseInference`] backends; a manifest without this
    /// flag is refused visibly (never substituted by another model).
    /// Additive `#[serde(default)]` field: manifests written before it existed
    /// keep parsing unchanged.
    #[serde(default)]
    pub denoise: bool,
}

impl ModelCapabilities {
    /// Whether any capability is declared.
    pub fn any(&self) -> bool {
        self.subject_segmentation
            || self.box_prompt
            || self.point_prompt
            || self.mask_prompt
            || self.class_detection
            || self.instance_segmentation
            || self.inpaint_heal
            || self.outpaint
            || self.face_detect
            || self.face_embed
            || self.denoise
    }

    /// Validate that at least one capability is set.
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.validate_for("model")
    }

    pub(crate) fn validate_for(&self, name: &str) -> Result<(), OnnxError> {
        if !self.any() {
            return Err(OnnxError::UnsupportedModel {
                name: name.to_owned(),
                reason: "no model capabilities declared (at least one of subject_segmentation, \
                     box_prompt, point_prompt, mask_prompt, class_detection, \
                     instance_segmentation, inpaint_heal, outpaint, face_detect, face_embed, \
                     denoise must be true)"
                    .into(),
            });
        }
        Ok(())
    }
}

/// Unchecked deserialization form. Deserializing into this struct rejects
/// unknown fields; [`TryFrom`] then enforces the full manifest invariants so a
/// manifest with no capabilities set, an empty hash/license, a zero
/// resolution, or empty tensor names is rejected at construction time.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelManifestUnchecked {
    model_name: String,
    model_version: String,
    model_hash: String,
    license: String,
    input: ModelInputSpec,
    /// Name of the primary (matte) output tensor. Defaults to `output`, the
    /// documented single-output contract of the v1 subject models, so
    /// manifests written before the field existed keep parsing.
    #[serde(default = "default_output_tensor_name")]
    output_tensor_name: String,
    capabilities: ModelCapabilities,
}

/// Default primary output tensor name (`#[serde(default)]` for manifests
/// written before the field existed).
fn default_output_tensor_name() -> String {
    "output".to_owned()
}

/// Serializable ONNX model identity + I/O contract.
///
/// The model hash is documented as the identity artifact (F-080 / ai-masks
/// mask identity). It is intentionally a plain string here; the mapping to the
/// sidecar `ModelIdentity` happens in F-048.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelManifest {
    /// Human/model registry name, e.g. `BiRefNet`.
    pub model_name: String,
    /// Model version (semver-ish string).
    pub model_version: String,
    /// Content/artifact hash identifying the exact weights. The documented
    /// algorithm is SHA-256 over the artifact bytes; the placeholder
    /// [`crate::hash::PENDING_INTEGRATION_HASH`] marks the pre-integration
    /// state in which no pinned identity exists yet.
    pub model_hash: String,
    /// Model license, e.g. `Apache-2.0`.
    pub license: String,
    /// Input tensor specification.
    pub input: ModelInputSpec,
    /// Name of the primary (matte) output tensor in the ONNX graph.
    pub output_tensor_name: String,
    /// Declared model capabilities.
    pub capabilities: ModelCapabilities,
}

/// Canonical, versioned text form of an input spec, fed into the identity
/// digest (R2-ONNX-01).
///
/// Encoded by hand — field order and separators fixed, leading schema tag —
/// so the digest has **no error path** and stays stable independent of any
/// serializer's float formatting. Every component of the spec is included:
/// inference resolution, channel layout, tensor name, tensor format and both
/// normalization vectors. Two specs yield the same text iff they are
/// semantically equal.
fn canonical_input_spec_text(spec: &ModelInputSpec) -> String {
    format!(
        "lumina-input-spec-v1|res={}x{}|layout={:?}|tensor_name={}|tensor_format={:?}|mean={:?}|std={:?}",
        spec.resolution.width,
        spec.resolution.height,
        spec.channel_layout,
        spec.tensor_name,
        spec.tensor_format,
        spec.normalization.mean,
        spec.normalization.std,
    )
}

impl ModelInputSpec {
    /// Deterministic SHA-256 identity digest over this input spec
    /// (R2-ONNX-01 / ai-masks.md: "Vorverarbeitung und Inferenzauflösung"
    /// sind Identitätsbestandteile).
    ///
    /// The returned value has the form `sha256:<64 lowercase hex chars>`. Any
    /// change to resolution, layouts, tensor names or the input normalization
    /// flips the digest even when `model_name`/`model_version`/`model_hash`
    /// stay untouched — exactly the stale-detection hole R2-ONNX-01 closes.
    pub(crate) fn identity_digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(canonical_input_spec_text(self).as_bytes());
        format!("sha256:{}", crate::hash::to_hex(&hasher.finalize()))
    }
}

/// Extra key under which the `ModelInputSpec::identity_digest` (deterministic
/// SHA-256 over the input spec) is persisted in
/// the sidecar [`lumina_sidecar::ModelIdentity`] extras (see
/// [`ModelManifest::to_model_identity`]).
pub const INPUT_SPEC_DIGEST_KEY: &str = "input_spec_digest";

impl ModelManifest {
    /// Validate the full manifest invariant set (REVIEW-ONNX-N2):
    ///
    /// * at least one capability is declared;
    /// * `model_hash` and `license` are non-empty;
    /// * the inference resolution is non-zero on both axes;
    /// * input and output tensor names are non-empty;
    /// * the input normalization is finite with strictly positive `std`
    ///   (a zero/NaN `std` would silently poison preprocessing).
    ///
    /// Violations surface as [`OnnxError::InvalidManifest`] with all reasons
    /// joined; the capability invariant keeps its established
    /// [`OnnxError::UnsupportedModel`] error.
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.capabilities.validate_for(&self.model_name)?;
        validate_fields(
            &self.model_name,
            &self.model_hash,
            &self.license,
            &self.input,
            &self.output_tensor_name,
        )
    }

    /// Map this manifest's identity onto the sidecar
    /// [`lumina_sidecar::ModelIdentity`] used by
    /// the mask-loading decision layer (F-048 / F-051) for stale-detection.
    ///
    /// Beyond name/version/hash the persisted identity carries a
    /// deterministic digest over the full [`ModelInputSpec`] (inference
    /// resolution, channel layout, tensor names, tensor format, input
    /// normalization) under [`INPUT_SPEC_DIGEST_KEY`]. ai-masks.md lists
    /// "Vorverarbeitung und Inferenzauflösung" explicitly as mask-identity
    /// components; without the digest a normalization/resolution change would
    /// keep cached masks `valid` despite changed inference semantics
    /// (R2-ONNX-01).
    ///
    /// # Sidecar impact (documented decision per AGENTS.md)
    ///
    /// * The digest is an **additive optional** extras entry. Sidecars written
    ///   before this change keep parsing unchanged (`extras` is
    ///   `#[serde(default)]` and skipped when empty); no schema-version bump
    ///   and no migration are required.
    /// * Existing persisted masks do **not** change validity through this
    ///   change: the decision-layer comparison (`lumina-core`
    ///   `model_identity_matches`) currently compares name/version/hash only.
    ///   That comparison MUST start honoring [`INPUT_SPEC_DIGEST_KEY`] before
    ///   real weights land in F-048 — otherwise the stale-detection hole this
    ///   digest enables stays closed only on the producer side (follow-up,
    ///   outside this crate).
    /// * Face descriptors (YuNet/SFace, S6 2026-09-20) now carry hash-pinned
    ///   valid identities; all other shipped descriptors still carry the
    ///   [`crate::hash::PENDING_INTEGRATION_HASH`] placeholder.
    pub fn to_model_identity(&self) -> lumina_sidecar::ModelIdentity {
        let mut extras = BTreeMap::new();
        extras.insert(
            INPUT_SPEC_DIGEST_KEY.to_owned(),
            serde_json::Value::String(self.input.identity_digest()),
        );
        lumina_sidecar::ModelIdentity {
            name: self.model_name.clone(),
            version: self.model_version.clone(),
            hash: self.model_hash.clone(),
            extras,
        }
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, OnnxError> {
        serde_json::to_string(self).map_err(|e| OnnxError::InvalidManifest(e.to_string()))
    }

    /// Deserialize from JSON, rejecting unknown fields and enforcing the
    /// manifest invariants (see [`ModelManifest::validate`]).
    pub fn from_json(json: &str) -> Result<Self, OnnxError> {
        let unchecked: ModelManifestUnchecked =
            serde_json::from_str(json).map_err(|e| OnnxError::InvalidManifest(e.to_string()))?;
        unchecked.try_into()
    }
}

/// Shared field invariants enforced by [`ModelManifest::validate`] and the
/// `TryFrom<ModelManifestUnchecked>` construction path.
fn validate_fields(
    model_name: &str,
    model_hash: &str,
    license: &str,
    input: &ModelInputSpec,
    output_tensor_name: &str,
) -> Result<(), OnnxError> {
    let mut problems: Vec<String> = Vec::new();
    if model_hash.trim().is_empty() {
        problems.push("model_hash must not be empty".to_owned());
    }
    if license.trim().is_empty() {
        problems.push("license must not be empty".to_owned());
    }
    if input.resolution.width == 0 || input.resolution.height == 0 {
        problems.push(format!(
            "input resolution must be non-zero, got {}x{}",
            input.resolution.width, input.resolution.height
        ));
    }
    if input.tensor_name.trim().is_empty() {
        problems.push("input.tensor_name must not be empty".to_owned());
    }
    if output_tensor_name.trim().is_empty() {
        problems.push("output_tensor_name must not be empty".to_owned());
    }
    for (axis, value) in input.normalization.mean.iter().enumerate() {
        if !value.is_finite() {
            problems.push(format!("normalization.mean[{axis}] must be finite"));
        }
    }
    for (axis, value) in input.normalization.std.iter().enumerate() {
        if !value.is_finite() || *value <= 0.0 {
            problems.push(format!(
                "normalization.std[{axis}] must be finite and > 0, got {value}"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(OnnxError::InvalidManifest(format!(
            "model `{model_name}` manifest is invalid: {}",
            problems.join("; ")
        )))
    }
}

impl TryFrom<ModelManifestUnchecked> for ModelManifest {
    type Error = OnnxError;

    fn try_from(raw: ModelManifestUnchecked) -> Result<Self, Self::Error> {
        raw.capabilities.validate_for(&raw.model_name)?;
        validate_fields(
            &raw.model_name,
            &raw.model_hash,
            &raw.license,
            &raw.input,
            &raw.output_tensor_name,
        )?;
        Ok(Self {
            model_name: raw.model_name,
            model_version: raw.model_version,
            model_hash: raw.model_hash,
            license: raw.license,
            input: raw.input,
            output_tensor_name: raw.output_tensor_name,
            capabilities: raw.capabilities,
        })
    }
}

/// Inference resolution width for BiRefNet.
pub const BIREFNET_INFERENCE_WIDTH: u32 = 1024;
/// Inference resolution height for BiRefNet.
pub const BIREFNET_INFERENCE_HEIGHT: u32 = 1024;

/// Inference resolution width for the SAM 2.1 `hiera_*` model family.
pub const SAM2_INFERENCE_WIDTH: u32 = 1024;
/// Inference resolution height for the SAM 2.1 `hiera_*` model family.
pub const SAM2_INFERENCE_HEIGHT: u32 = 1024;

/// SAM 2.1 release version string used for every `sam2.1_hiera_*` descriptor.
///
/// The family is the Meta "SAM 2.1" release (checkpoint family `092824`,
/// Apache-2.0). `model_hash` stays `pending-integration` until real,
/// hash-pinned ONNX fixtures are committed (no spontaneous downloads — see
/// `AGENTS.md`).
pub const SAM2_RELEASE_VERSION: &str = "2.1.0";

/// SAM 2.1 model-family variants (F-082).
///
/// The adapter selects one of these at runtime via [`select_variant`] /
/// [`DeviceProfile`] so the same code path serves the whole `hiera_*` family.
/// The exact `model_name` is persisted in the mask identity, keeping re-runs
/// reproducible regardless of which device class ran the original inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sam2Variant {
    /// `sam2.1_hiera_tiny` — lowest CPU load, fastest encoder (~38.9 M params).
    Tiny,
    /// `sam2.1_hiera_small` — small quality bump (~46.0 M params).
    Small,
    /// `sam2.1_hiera_base_plus` — Meta's standard balanced variant (~80.8 M).
    BasePlus,
    /// `sam2.1_hiera_large` — highest quality, high-end only (~224.4 M).
    Large,
}

impl Sam2Variant {
    /// All four variants, in ascending cost/quality order.
    pub const ALL: [Sam2Variant; 4] = [
        Sam2Variant::Tiny,
        Sam2Variant::Small,
        Sam2Variant::BasePlus,
        Sam2Variant::Large,
    ];

    /// Exact `model_name` persisted in the mask identity.
    pub fn model_name(self) -> &'static str {
        match self {
            Sam2Variant::Tiny => "sam2.1_hiera_tiny",
            Sam2Variant::Small => "sam2.1_hiera_small",
            Sam2Variant::BasePlus => "sam2.1_hiera_base_plus",
            Sam2Variant::Large => "sam2.1_hiera_large",
        }
    }

    /// Build the [`ModelManifest`] descriptor for this variant.
    pub fn manifest(self) -> ModelManifest {
        sam2_1_manifest(self)
    }
}

/// BiRefNet descriptor: the first automatic subject model.
///
/// - Automatic subject segmentation from a single RGB input to an alpha matte.
/// - No prompts: only `subject_segmentation` is true.
/// - Documented inference resolution 1024×1024 (square resize).
/// - License `MIT` (BiRefNet by Zheng et al., arXiv:2401.03407 — verified
///   2026-08-20 via GitHub `LICENSE` = MIT, Copyright (c) 2024 ZhengPeng, and the
///   HF model card `ZhengPeng7/BiRefNet` (`license: mit`); no download performed
///   in this iteration). `model_hash` is a placeholder (`pending-integration`)
///   until real weights are provided in F-048.
pub fn birefnet_manifest() -> ModelManifest {
    ModelManifest {
        model_name: "BiRefNet".into(),
        model_version: "1.0.0".into(),
        model_hash: crate::hash::PENDING_INTEGRATION_HASH.into(),
        license: "MIT".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: BIREFNET_INFERENCE_WIDTH,
                height: BIREFNET_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "input".into(),
            tensor_format: TensorFormat::Nchw,
            // BiRefNet preprocesses with ImageNet mean/std.
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: "output".into(),
        capabilities: ModelCapabilities {
            subject_segmentation: true,
            box_prompt: false,
            point_prompt: false,
            mask_prompt: false,
            class_detection: false,
            instance_segmentation: false,
            inpaint_heal: false,
            outpaint: false,
            face_detect: false,
            face_embed: false,
            denoise: false,
        },
    }
}

pub fn inpaint_heal_manifest() -> ModelManifest {
    ModelManifest {
        model_name: "inpaint-heal-xl".into(),
        model_version: "1.0.0".into(),
        model_hash: crate::hash::PENDING_INTEGRATION_HASH.into(),
        license: "Apache-2.0".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: 512,
                height: 512,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "image".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: "output".into(),
        capabilities: ModelCapabilities {
            subject_segmentation: false,
            box_prompt: false,
            point_prompt: false,
            mask_prompt: false,
            class_detection: false,
            instance_segmentation: false,
            inpaint_heal: true,
            outpaint: false,
            face_detect: false,
            face_embed: false,
            denoise: false,
        },
    }
}

/// Inference resolution width for the generative outpaint model.
pub const OUTPAINT_INFERENCE_WIDTH: u32 = 1024;
/// Inference resolution height for the generative outpaint model.
pub const OUTPAINT_INFERENCE_HEIGHT: u32 = 1024;

/// Outpaint descriptor: generative canvas expansion beyond the original
/// surface (GEN-EXPAND-1, `GenerativeEdit` expand role).
///
/// - Declares only the `outpaint` capability (separately testable per
///   `feature/product/generative-expand.md`; a model may declare just one of
///   `inpaint_heal` / `outpaint`).
/// - Documented inference resolution 1024x1024 (square resize, same contract
///   as `GenerativeEdit.inference_resolution`).
/// - License `Apache-2.0` is a **placeholder pre-integration declaration**,
///   mirroring `inpaint_heal_manifest`: no weights are committed, no download
///   is performed, and `model_hash` is `pending-integration` until real
///   weights are hash-pinned in a follow-up (F-078,
///   `feature/quality/fixtures-licensing.md`). Many state-of-the-art
///   inpaint/outpaint weights are non-commercial — the license MUST be
///   verified against the actual weight source before pinning (same
///   AGPL-trap caution as the `ultralytics` note in
///   `feature/quality/fixtures-licensing.md` section 5). Tests never touch
///   the network; they run against the deterministic stub backend only.
pub fn outpaint_expand_manifest() -> ModelManifest {
    ModelManifest {
        model_name: "inpaint-outpaint-xl".into(),
        model_version: "1.0.0".into(),
        model_hash: crate::hash::PENDING_INTEGRATION_HASH.into(),
        license: "Apache-2.0".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: OUTPAINT_INFERENCE_WIDTH,
                height: OUTPAINT_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "image".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: "output".into(),
        capabilities: ModelCapabilities {
            subject_segmentation: false,
            box_prompt: false,
            point_prompt: false,
            mask_prompt: false,
            class_detection: false,
            instance_segmentation: false,
            inpaint_heal: false,
            outpaint: true,
            face_detect: false,
            face_embed: false,
            denoise: false,
        },
    }
}

/// SAM 2.1 descriptor for a single variant.
///
/// Every variant is an interactive segmentation model: it declares
/// `box_prompt`, `point_prompt` and `mask_prompt` (the three new interactive
/// capabilities) and **not** `subject_segmentation` (SAM 2 is prompted, not
/// automatic). All four share the documented 1024x1024 RGB NCHW encoder
/// contract with the image tensor named `images`.
///
/// `model_hash` is `pending-integration` (same placeholder format as
/// BiRefNet) until real, hash-pinned ONNX fixtures are committed. License is
/// `Apache-2.0` (facebookresearch/sam2, verified — no download in this
/// iteration).
pub fn sam2_1_manifest(variant: Sam2Variant) -> ModelManifest {
    ModelManifest {
        model_name: variant.model_name().into(),
        model_version: SAM2_RELEASE_VERSION.into(),
        model_hash: crate::hash::PENDING_INTEGRATION_HASH.into(),
        license: "Apache-2.0".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: SAM2_INFERENCE_WIDTH,
                height: SAM2_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "images".into(),
            tensor_format: TensorFormat::Nchw,
            // The SAM 2.1 image encoder uses ImageNet normalization.
            normalization: InputNormalization::IMAGENET,
        },
        // Primary decoder output per the documented F-082 prompt contract
        // (`masks` on the original resolution + `iou_predictions` +
        // `low_res_masks`).
        output_tensor_name: "masks".into(),
        capabilities: ModelCapabilities {
            subject_segmentation: false,
            box_prompt: true,
            point_prompt: true,
            mask_prompt: true,
            class_detection: false,
            instance_segmentation: false,
            inpaint_heal: false,
            outpaint: false,
            face_detect: false,
            face_embed: false,
            denoise: false,
        },
    }
}

/// All four SAM 2.1 variant descriptors, in ascending cost/quality order.
pub fn sam2_1_manifests() -> Vec<ModelManifest> {
    Sam2Variant::ALL.iter().map(|v| v.manifest()).collect()
}

/// Device capability profile used to pick a SAM 2.1 family variant at runtime
/// (F-082 dynamic variant selection).
///
/// The selected variant is **not** part of the mask identity; the identity
/// persists the exact `model_name`/`model_hash` of the variant actually used,
/// so re-runs are reproducible regardless of device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Logical CPU cores reported by the host.
    pub cores: u32,
    /// Explicit variant override; wins over the core-count heuristic.
    pub r#override: Option<Sam2Variant>,
}

impl DeviceProfile {
    /// Build a profile from an explicit core count and optional override.
    pub fn new(cores: u32, r#override: Option<Sam2Variant>) -> Self {
        Self { cores, r#override }
    }

    /// Profile from a core count only (no override).
    pub fn with_cores(cores: u32) -> Self {
        Self {
            cores,
            r#override: None,
        }
    }

    /// Profile with an explicit variant override (core count ignored).
    pub fn with_override(variant: Sam2Variant) -> Self {
        Self {
            cores: 0,
            r#override: Some(variant),
        }
    }

    /// Detect the host profile via [`std::thread::available_parallelism`]
    /// (no extra dependencies). On failure (cannot query the platform) it
    /// returns a conservative `cores = 0` profile, which maps to `tiny`.
    pub fn detect() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(0);
        Self {
            cores,
            r#override: None,
        }
    }

    /// Select the SAM 2.1 variant for this profile (see [`select_variant`]).
    pub fn select_variant(&self) -> Sam2Variant {
        select_variant(self)
    }
}

/// Select the SAM 2.1 variant for a [`DeviceProfile`] using the documented,
/// deterministic thresholds (F-082 SOLL, start values, later benchmark-tuned):
///
/// | cores            | variant    |
/// | ---------------- | ---------- |
/// | `< 4`            | `tiny`     |
/// | `4 ..= 7`        | `small`    |
/// | `8 ..= 15`       | `base_plus`|
/// | `>= 16`          | `large`    |
///
/// An explicit `override` always wins, independent of `cores`.
pub fn select_variant(profile: &DeviceProfile) -> Sam2Variant {
    if let Some(variant) = profile.r#override {
        return variant;
    }
    match profile.cores {
        0..=3 => Sam2Variant::Tiny,
        4..=7 => Sam2Variant::Small,
        8..=15 => Sam2Variant::BasePlus,
        _ => Sam2Variant::Large,
    }
}

#[cfg(test)]
mod tests;
