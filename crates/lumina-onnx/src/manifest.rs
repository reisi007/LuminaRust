//! Model manifest and capabilities (F-080).
//!
//! [`ModelManifest`] is the serializable identity + I/O contract of an ONNX
//! model; [`ModelCapabilities`] enumerates what a model can do. At least one
//! capability must be set, and unknown fields are rejected, so a manifest can
//! never silently claim a capability it does not declare.

use crate::OnnxError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Channel layout of the model input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelLayout {
    /// 3-channel RGB (no alpha).
    Rgb,
}

/// Memory layout of the model input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorFormat {
    /// Batch, channels, height, width.
    Nchw,
    /// Batch, height, width, channels.
    Nhwc,
}

/// Model input resolution in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Per-channel input normalization for the ONNX graph (REVIEW-ONNX-PREPROC-1).
///
/// Pixel value `v` (u8) is mapped to `(v / 255 - mean[c]) / std[c]` before it
/// is written into the input tensor. The normalization is part of the model
/// I/O contract and therefore lives in the manifest — backends must read it
/// from there instead of hardcoding a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputNormalization {
    /// Per-channel mean in RGB order, on the `[0, 1]` scale.
    pub mean: [f32; 3],
    /// Per-channel standard deviation in RGB order, on the `[0, 1]` scale.
    pub std: [f32; 3],
}

impl InputNormalization {
    /// ImageNet mean/std — the documented preprocessing of BiRefNet and the
    /// SAM 2.1 image encoder. This is also the serde default so manifests
    /// written before the field existed keep parsing with the correct target
    /// semantics (the previous `[0, 1]`-only behavior was reviewed as wrong,
    /// not as a compatibility requirement).
    pub const IMAGENET: InputNormalization = InputNormalization {
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
    };

    /// Default used by `#[serde(default)]`: ImageNet normalization.
    pub fn imagenet() -> Self {
        Self::IMAGENET
    }
}

impl Default for InputNormalization {
    fn default() -> Self {
        Self::IMAGENET
    }
}

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
/// segmentation, e.g. BiRefNet) with no prompts. The remaining five flags are
/// the interactive/advanced capabilities enumerated in F-080. **At least one
/// capability must be set** — see [`ModelCapabilities::validate`].
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
                     instance_segmentation must be true)"
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

/// Extra key under which [`ModelInputSpec::identity_digest`] is persisted in
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

    /// Map this manifest's identity onto the sidecar [`ModelIdentity`] used by
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
    /// # Sidecar impact (documented decision per Agents.md)
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
    /// * All shipped descriptors still carry the
    ///   [`crate::hash::PENDING_INTEGRATION_HASH`] placeholder, i.e. no
    ///   hash-pinned valid identities exist yet that could be invalidated by
    ///   the new extras entry.
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
/// `Agents.md`).
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
mod tests {
    use super::*;

    #[test]
    fn birefnet_has_only_subject_capability() {
        let m = birefnet_manifest();
        assert!(m.capabilities.subject_segmentation);
        assert!(!m.capabilities.box_prompt);
        assert!(!m.capabilities.point_prompt);
        assert!(!m.capabilities.mask_prompt);
        assert!(!m.capabilities.class_detection);
        assert!(!m.capabilities.instance_segmentation);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn manifest_json_roundtrip_preserves_identity() {
        let m = birefnet_manifest();
        let json = m.to_json().unwrap();
        let back = ModelManifest::from_json(&json).unwrap();
        assert_eq!(m, back);
    }

    // R2-ONNX-01 — the persisted identity must carry a deterministic digest
    // over the ModelInputSpec (ai-masks.md: preprocessing + inference
    // resolution are identity components), not an empty extras map.
    #[test]
    fn model_identity_extras_carry_deterministic_input_spec_digest() {
        let identity = birefnet_manifest().to_model_identity();
        let digest = identity
            .extras
            .get(INPUT_SPEC_DIGEST_KEY)
            .unwrap_or_else(|| panic!("identity extras must carry {INPUT_SPEC_DIGEST_KEY}"));
        let serde_json::Value::String(digest) = digest else {
            panic!("digest extra must be a string, got {digest:?}");
        };
        let Some(hex) = digest.strip_prefix("sha256:") else {
            panic!("digest must be sha256-prefixed, got {digest}");
        };
        assert_eq!(hex.len(), 64, "sha256 hex length, got {hex}");
        assert!(
            hex.bytes().all(|b| b.is_ascii_hexdigit()),
            "lowercase-hex digest expected, got {hex}"
        );
        // Deterministic across calls and instances.
        assert_eq!(
            identity.extras,
            birefnet_manifest().to_model_identity().extras,
            "the same spec must always produce the same digest"
        );

        // The SAM 2.1 family shares resolution/layout but differs in name —
        // its digest must still be well-formed and stable.
        let sam = sam2_1_manifest(Sam2Variant::Tiny).to_model_identity();
        assert!(sam.extras.contains_key(INPUT_SPEC_DIGEST_KEY));
    }

    /// Direction test (analogous to the F-082-FOLLOWUP-HASH hash-direction
    /// tests): changing **only** the input normalization must change the
    /// persisted mask identity — this is exactly the stale-detection signal
    /// that was previously bypassable.
    #[test]
    fn identity_changes_when_normalization_changes() {
        let base = birefnet_manifest();
        let mut changed = birefnet_manifest();
        changed.input.normalization.mean[0] += 0.001;

        let a = base.to_model_identity();
        let b = changed.to_model_identity();

        // Name/version/hash stay untouched — only the spec digest may differ.
        assert_eq!(a.name, b.name);
        assert_eq!(a.version, b.version);
        assert_eq!(a.hash, b.hash);
        assert_ne!(
            a.extras.get(INPUT_SPEC_DIGEST_KEY),
            b.extras.get(INPUT_SPEC_DIGEST_KEY),
            "a normalization change MUST flip the persisted input-spec digest"
        );
    }

    /// Direction test: changing **only** the inference resolution must also
    /// flip the persisted identity digest.
    #[test]
    fn identity_changes_when_resolution_changes() {
        let base = birefnet_manifest();
        let mut changed = birefnet_manifest();
        changed.input.resolution = Resolution {
            width: 512,
            height: 512,
        };

        let a = base.to_model_identity();
        let b = changed.to_model_identity();
        assert_eq!((a.name, a.version, a.hash), (b.name, b.version, b.hash));
        assert_ne!(
            a.extras.get(INPUT_SPEC_DIGEST_KEY),
            b.extras.get(INPUT_SPEC_DIGEST_KEY),
            "a resolution change MUST flip the persisted input-spec digest"
        );
    }

    /// Any other spec component participates too: tensor format and layout
    /// are part of the inference contract.
    #[test]
    fn identity_changes_when_tensor_format_or_layout_change() {
        let base = birefnet_manifest().to_model_identity();

        let mut nhwc = birefnet_manifest();
        nhwc.input.tensor_format = TensorFormat::Nhwc;
        assert_ne!(
            base.extras.get(INPUT_SPEC_DIGEST_KEY),
            nhwc.to_model_identity().extras.get(INPUT_SPEC_DIGEST_KEY),
        );

        let mut other_tensor = birefnet_manifest();
        other_tensor.input.tensor_name = "pixels".into();
        assert_ne!(
            base.extras.get(INPUT_SPEC_DIGEST_KEY),
            other_tensor
                .to_model_identity()
                .extras
                .get(INPUT_SPEC_DIGEST_KEY),
        );
    }

    #[test]
    fn manifest_rejects_unknown_field() {
        let mut value = serde_json::to_value(birefnet_manifest()).unwrap();
        value["unknown_top_level"] = serde_json::json!(1);
        let err = ModelManifest::from_json(&value.to_string()).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected unknown-field error, got {err}"
        );
    }

    #[test]
    fn no_capabilities_set_is_rejected() {
        let json = serde_json::json!({
            "model_name": "X",
            "model_version": "1",
            "model_hash": "h",
            "license": "MIT",
            "input": {
                "resolution": {"width": 4, "height": 4},
                "channel_layout": "rgb",
                "tensor_name": "input",
                "tensor_format": "nchw"
            },
            "capabilities": {
                "subject_segmentation": false,
                "box_prompt": false,
                "point_prompt": false,
                "mask_prompt": false,
                "class_detection": false,
                "instance_segmentation": false
            }
        })
        .to_string();
        let err = ModelManifest::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OnnxError::UnsupportedModel { .. }),
            "expected UnsupportedModel, got {err:?}"
        );
    }

    #[test]
    fn capabilities_reject_unknown_field() {
        let json = serde_json::json!({
            "subject_segmentation": true,
            "box_prompt": false,
            "point_prompt": false,
            "mask_prompt": false,
            "class_detection": false,
            "instance_segmentation": false,
            "future_capability": true
        })
        .to_string();
        let err = serde_json::from_str::<ModelCapabilities>(&json).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected unknown-field error, got {err}"
        );
    }

    #[test]
    fn capabilities_validate_directly() {
        assert!(ModelCapabilities::default().validate().is_err());
        let ok = ModelCapabilities {
            subject_segmentation: true,
            ..Default::default()
        };
        assert!(ok.validate().is_ok());
    }

    // F-083 #2 — every SAM 2.1 variant descriptor is well-formed.
    #[test]
    fn sam2_1_manifests_yield_four_valid_variants() {
        let manifests = sam2_1_manifests();
        assert_eq!(manifests.len(), 4);
        let names: Vec<&str> = manifests.iter().map(|m| m.model_name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "sam2.1_hiera_tiny",
                "sam2.1_hiera_small",
                "sam2.1_hiera_base_plus",
                "sam2.1_hiera_large",
            ]
        );
        for m in &manifests {
            // Three interactive capabilities declared.
            assert!(m.capabilities.box_prompt);
            assert!(m.capabilities.point_prompt);
            assert!(m.capabilities.mask_prompt);
            // Not automatic subject segmentation.
            assert!(!m.capabilities.subject_segmentation);
            // Valid input spec: 1024² RGB NCHW, encoder tensor named `images`.
            assert_eq!(m.input.resolution.width, SAM2_INFERENCE_WIDTH);
            assert_eq!(m.input.resolution.height, SAM2_INFERENCE_HEIGHT);
            assert_eq!(m.input.channel_layout, ChannelLayout::Rgb);
            assert_eq!(m.input.tensor_format, TensorFormat::Nchw);
            assert_eq!(m.input.tensor_name, "images");
            // model_hash uses the same placeholder format as BiRefNet.
            assert_eq!(m.model_hash, "pending-integration");
            assert_eq!(m.license, "Apache-2.0");
            assert!(m.validate().is_ok());
        }
    }

    #[test]
    fn sam2_1_manifest_matches_variant_name() {
        for v in Sam2Variant::ALL {
            let m = sam2_1_manifest(v);
            assert_eq!(m.model_name, v.model_name());
            assert_eq!(m.model_version, SAM2_RELEASE_VERSION);
        }
    }

    // F-083 #3 — deterministic variant selection with exact thresholds.
    #[test]
    fn select_variant_boundaries() {
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(3)),
            Sam2Variant::Tiny
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(4)),
            Sam2Variant::Small
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(7)),
            Sam2Variant::Small
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(8)),
            Sam2Variant::BasePlus
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(15)),
            Sam2Variant::BasePlus
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(16)),
            Sam2Variant::Large
        );
        // Above the top threshold still maps to large.
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(128)),
            Sam2Variant::Large
        );
    }

    #[test]
    fn select_variant_override_wins() {
        // Override beats the core-count heuristic at every threshold.
        assert_eq!(
            select_variant(&DeviceProfile::with_override(Sam2Variant::Large)),
            Sam2Variant::Large
        );
        assert_eq!(
            select_variant(&DeviceProfile::new(3, Some(Sam2Variant::Large))),
            Sam2Variant::Large,
            "override must win even on a tiny-core profile"
        );
        assert_eq!(
            select_variant(&DeviceProfile::new(128, Some(Sam2Variant::Tiny))),
            Sam2Variant::Tiny,
            "override must win even on a high-core profile"
        );
    }

    #[test]
    fn select_variant_zero_cores_is_tiny_fallback() {
        // The conservative fallback (cores == 0, as returned when
        // available_parallelism cannot be queried) maps to tiny.
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(0)),
            Sam2Variant::Tiny
        );
    }

    #[test]
    fn detect_returns_a_valid_variant() {
        // detect() must not panic and must yield one of the four variants.
        let profile = DeviceProfile::detect();
        let variant = profile.select_variant();
        assert!(Sam2Variant::ALL.contains(&variant));
        // The method and free function agree.
        assert_eq!(profile.select_variant(), select_variant(&profile));
    }

    // REVIEW-ONNX-N2 — non-empty hash/license, valid resolutions, non-empty
    // tensor names and a sane normalization are enforced by validate().
    #[test]
    fn validate_rejects_empty_hash_and_license() {
        let mut m = birefnet_manifest();
        m.model_hash = "  ".to_owned();
        let err = m.validate().unwrap_err();
        assert!(
            err.to_string().contains("model_hash must not be empty"),
            "got {err}"
        );

        let mut m = birefnet_manifest();
        m.license = String::new();
        let err = m.validate().unwrap_err();
        assert!(
            err.to_string().contains("license must not be empty"),
            "got {err}"
        );
    }

    #[test]
    fn validate_rejects_zero_resolution() {
        let mut m = birefnet_manifest();
        m.input.resolution = Resolution {
            width: 0,
            height: 32,
        };
        let err = m.validate().unwrap_err();
        assert!(err.to_string().contains("must be non-zero"), "got {err}");
    }

    #[test]
    fn validate_rejects_empty_tensor_names() {
        let mut m = birefnet_manifest();
        m.input.tensor_name = String::new();
        assert!(m.validate().is_err());

        let mut m = birefnet_manifest();
        m.output_tensor_name = " ".to_owned();
        assert!(m
            .validate()
            .unwrap_err()
            .to_string()
            .contains("output_tensor_name must not be empty"));
    }

    #[test]
    fn validate_rejects_degenerate_normalization() {
        let mut m = sam2_1_manifest(Sam2Variant::Tiny);
        m.input.normalization.std = [0.229, 0.0, 0.225];
        let err = m.validate().unwrap_err();
        assert!(
            err.to_string().contains("normalization.std[1]"),
            "zero std must be rejected, got {err}"
        );

        let mut m = sam2_1_manifest(Sam2Variant::Tiny);
        m.input.normalization.mean[2] = f32::NAN;
        assert!(m
            .validate()
            .unwrap_err()
            .to_string()
            .contains("normalization.mean[2] must be finite"));
    }

    #[test]
    fn from_json_enforces_field_invariants_too() {
        let json = serde_json::json!({
            "model_name": "X",
            "model_version": "1",
            "model_hash": "",
            "license": "MIT",
            "input": {
                "resolution": {"width": 4, "height": 4},
                "channel_layout": "rgb",
                "tensor_name": "input",
                "tensor_format": "nchw"
            },
            "capabilities": {"subject_segmentation": true}
        })
        .to_string();
        let err = ModelManifest::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OnnxError::InvalidManifest(_)),
            "empty hash via JSON must be rejected, got {err:?}"
        );
    }

    /// Manifests written before `normalization`/`output_tensor_name` existed
    /// must keep parsing: normalization defaults to ImageNet, output name to
    /// `output` (documented single-output v1 contract).
    #[test]
    fn from_json_defaults_new_fields_for_pre_existing_manifests() {
        let json = serde_json::json!({
            "model_name": "X",
            "model_version": "1",
            "model_hash": "h",
            "license": "MIT",
            "input": {
                "resolution": {"width": 4, "height": 4},
                "channel_layout": "rgb",
                "tensor_name": "input",
                "tensor_format": "nchw"
            },
            "capabilities": {
                "subject_segmentation": true,
                "box_prompt": false,
                "point_prompt": false,
                "mask_prompt": false,
                "class_detection": false,
                "instance_segmentation": false
            }
        })
        .to_string();
        let m = ModelManifest::from_json(&json).unwrap();
        assert_eq!(m.output_tensor_name, "output");
        assert_eq!(m.input.normalization, InputNormalization::IMAGENET);
    }

    /// Explicitly serialized manifests round-trip the new fields verbatim.
    #[test]
    fn json_roundtrip_preserves_normalization_and_output_name() {
        let mut m = birefnet_manifest();
        m.input.normalization = InputNormalization {
            mean: [0.5, 0.5, 0.5],
            std: [0.25, 0.25, 0.25],
        };
        m.output_tensor_name = "matte".into();
        let back = ModelManifest::from_json(&m.to_json().unwrap()).unwrap();
        assert_eq!(m, back);
    }
}
