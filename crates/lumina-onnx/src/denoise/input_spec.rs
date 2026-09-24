//! Versioned F-096a input-spec identity for the KI-Denoise producer.
//!
//! The digest covers the model input contract *and* the core algorithms that
//! turn model output into a render-visible artifact.  Keeping this text in a
//! small module makes the identity auditable without growing the producer
//! module, and makes an algorithm change an explicit v2 contract change rather
//! than an accidental reinterpretation of the old v1 digest.

use crate::hash::compute_sha256_hex;
use lumina_sidecar::DENOISE_SHA256_PREFIX;

/// Canonical schema tag for the complete denoise input specification.
///
/// v2 is intentional: v1 did not name the core blend or tile-assembly
/// algorithms.  Persisted artifacts carrying a v1 digest therefore become
/// stale instead of silently being interpreted as the new contract.
pub const DENOISE_INPUT_SPEC_ALGORITHM: &str = "lumina-denoise-input-spec-v2";
/// SHA-256 pin for the default 512×512/32-overlap v2 contract.  This is a
/// deterministic test/documentation pin, not a model or weight pin.
pub const DENOISE_DEFAULT_INPUT_SPEC_DIGEST: &str =
    "sha256:e02314484356c026f5f4f64d4823450a450a833945a163f9d9abe07e024cda07";
/// SHA-256 pin for the deterministic default fixture specification that uses
/// the v2 input contract. This is not a weight-file hash.
pub const DENOISE_DEFAULT_FIXTURE_MODEL_HASH: &str =
    "sha256:0a5917d19b0e786042e493eb967bb02ca024c51a5711703d6357815c067feca0";

/// Name of the versioned core blend algorithm used after inference.
pub const DENOISE_BLEND_ALGORITHM: &str = "lumina-denoise-core-blend";
/// Version of [`DENOISE_BLEND_ALGORITHM`].
pub const DENOISE_BLEND_ALGORITHM_VERSION: &str = "1";
/// Canonical blend formula (the user-controlled strength/detail values are
/// recipe data, not fixed algorithm constants).
pub const DENOISE_BLEND_FORMULA: &str = "strength*(1-preserve_detail*detail)";
/// Detail measurement used by the core blend.
pub const DENOISE_DETAIL_WINDOW: &str = "3x3-box-mean-clamped";
/// Detail scale in 8-bit luminance units; mirrors the core F-096a constant.
pub const DENOISE_DETAIL_SCALE_UNITS: u32 = 32;
/// Luminance weights used by the detail measurement.
pub const DENOISE_LUMINANCE_STANDARD: &str = "rec709-0.2126-0.7152-0.0722";
/// Rounding/clamping used after the blend interpolation.
pub const DENOISE_BLEND_ROUNDING: &str = "round-half-away-from-zero-clamp-0-255";
/// Alpha behavior of the blend.
pub const DENOISE_ALPHA_POLICY: &str = "preserve";
/// Strict identity behavior for a zero-strength stage.
pub const DENOISE_ZERO_STRENGTH_POLICY: &str = "strict-identity-no-mutation";

/// Name of the versioned distance-to-edge tile assembly algorithm.
pub const DENOISE_TILE_ASSEMBLY_ALGORITHM: &str = "lumina-denoise-distance-to-edge-assembly";
/// Version of [`DENOISE_TILE_ASSEMBLY_ALGORITHM`].
pub const DENOISE_TILE_ASSEMBLY_ALGORITHM_VERSION: &str = "1";
/// Per-tile weight rule for the assembly algorithm.
pub const DENOISE_TILE_ASSEMBLY_WEIGHT: &str = "min-distance-to-edge-plus-1";
/// Normalization rule for the assembly algorithm.
pub const DENOISE_TILE_ASSEMBLY_MATH: &str = "normalized-weighted-mean";
/// Rounding/clamping used after tile assembly.
pub const DENOISE_TILE_ASSEMBLY_ROUNDING: &str = "round-half-away-from-zero-clamp-0-255";
/// Coverage policy for tiled production.
pub const DENOISE_TILE_COVERAGE: &str = "complete-required";

/// Working color/pixel space of the core stage.
pub const DENOISE_WORKING_SPACE: &str = "srgb-rgba8";
/// Output pixel encoding.
pub const DENOISE_OUTPUT_ENCODING: &str = "rgb8";
/// Output byte order.
pub const DENOISE_OUTPUT_LAYOUT: &str = "row-major";
/// Canonical model-output tensor shape.
pub const DENOISE_OUTPUT_SHAPE: &str = "nchw-[1,3,H,W]";
/// Canonical model-output value range and conversion.
pub const DENOISE_OUTPUT_RANGE: &str = "unit-0-1-to-rgb8-round-clamp";

/// Versioned identity of the core blend algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenoiseBlendIdentity {
    pub algorithm: &'static str,
    pub version: &'static str,
    pub formula: &'static str,
    pub detail_window: &'static str,
    pub detail_scale: u32,
    pub luminance_standard: &'static str,
    pub rounding: &'static str,
    pub alpha_policy: &'static str,
    pub zero_strength_policy: &'static str,
}

/// Versioned identity of the distance-to-edge tile assembly algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenoiseTileAssemblyIdentity {
    pub algorithm: &'static str,
    pub version: &'static str,
    pub weight: &'static str,
    pub math: &'static str,
    pub rounding: &'static str,
    pub coverage: &'static str,
}

/// Versioned identity of the denoise output/persistence contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenoiseOutputIdentity {
    pub working_space: &'static str,
    pub encoding: &'static str,
    pub layout: &'static str,
    pub shape: &'static str,
    pub value_range: &'static str,
    pub payload_encoding_version: u32,
}

/// All behavior-bearing F-096a values appended to the model input identity.
///
/// The fields are public so contract tests can make a deliberate alternate
/// identity without editing the producer's large module.  Production uses
/// [`Self::default`]; changing one of these values is a contract change and
/// must be accompanied by a deliberate schema-tag review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenoiseInputSpecContract {
    pub blend: DenoiseBlendIdentity,
    pub assembly: DenoiseTileAssemblyIdentity,
    pub output: DenoiseOutputIdentity,
}

/// Model/tile/preprocessing values supplied by a suite for canonicalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenoiseInputSpecParameters<'a> {
    pub model_input_digest: &'a str,
    pub input_tensor_name: &'a str,
    pub output_tensor_name: &'a str,
    pub tile_width: u32,
    pub tile_height: u32,
    pub overlap: u32,
    pub preprocessing_name: &'a str,
    pub preprocessing_version: &'a str,
    pub rescaling_method: &'a str,
}

impl Default for DenoiseInputSpecContract {
    fn default() -> Self {
        Self {
            blend: DenoiseBlendIdentity {
                algorithm: DENOISE_BLEND_ALGORITHM,
                version: DENOISE_BLEND_ALGORITHM_VERSION,
                formula: DENOISE_BLEND_FORMULA,
                detail_window: DENOISE_DETAIL_WINDOW,
                detail_scale: DENOISE_DETAIL_SCALE_UNITS,
                luminance_standard: DENOISE_LUMINANCE_STANDARD,
                rounding: DENOISE_BLEND_ROUNDING,
                alpha_policy: DENOISE_ALPHA_POLICY,
                zero_strength_policy: DENOISE_ZERO_STRENGTH_POLICY,
            },
            assembly: DenoiseTileAssemblyIdentity {
                algorithm: DENOISE_TILE_ASSEMBLY_ALGORITHM,
                version: DENOISE_TILE_ASSEMBLY_ALGORITHM_VERSION,
                weight: DENOISE_TILE_ASSEMBLY_WEIGHT,
                math: DENOISE_TILE_ASSEMBLY_MATH,
                rounding: DENOISE_TILE_ASSEMBLY_ROUNDING,
                coverage: DENOISE_TILE_COVERAGE,
            },
            output: DenoiseOutputIdentity {
                working_space: DENOISE_WORKING_SPACE,
                encoding: DENOISE_OUTPUT_ENCODING,
                layout: DENOISE_OUTPUT_LAYOUT,
                shape: DENOISE_OUTPUT_SHAPE,
                value_range: DENOISE_OUTPUT_RANGE,
                payload_encoding_version: lumina_core::DENOISE_RGB_ENCODING_VERSION,
            },
        }
    }
}

/// Build the fixed-order, human-auditable canonical text before hashing.
#[must_use]
pub fn canonical_input_spec_text(
    parameters: &DenoiseInputSpecParameters<'_>,
    contract: &DenoiseInputSpecContract,
) -> String {
    format!(
        "{DENOISE_INPUT_SPEC_ALGORITHM}|model={}|input_tensor_name={}|output_tensor_name={}|tile={}x{}|overlap={}|preprocessing={}:{}|rescaling={}|working_space={}|blend_algorithm={}:{}|blend_formula={}|detail_window={}|detail_scale={}|luminance_standard={}|blend_rounding={}|alpha_policy={}|zero_strength_policy={}|assembly_algorithm={}:{}|tile_weight={}|assembly_math={}|assembly_rounding={}|tile_coverage={}|output_encoding={}|output_layout={}|output_shape={}|output_range={}|payload_encoding_version={}",
        parameters.model_input_digest,
        parameters.input_tensor_name,
        parameters.output_tensor_name,
        parameters.tile_width,
        parameters.tile_height,
        parameters.overlap,
        parameters.preprocessing_name,
        parameters.preprocessing_version,
        parameters.rescaling_method,
        contract.output.working_space,
        contract.blend.algorithm,
        contract.blend.version,
        contract.blend.formula,
        contract.blend.detail_window,
        contract.blend.detail_scale,
        contract.blend.luminance_standard,
        contract.blend.rounding,
        contract.blend.alpha_policy,
        contract.blend.zero_strength_policy,
        contract.assembly.algorithm,
        contract.assembly.version,
        contract.assembly.weight,
        contract.assembly.math,
        contract.assembly.rounding,
        contract.assembly.coverage,
        contract.output.encoding,
        contract.output.layout,
        contract.output.shape,
        contract.output.value_range,
        contract.output.payload_encoding_version,
    )
}

/// Hash the canonical text into the persisted `sha256:<hex>` identity.
#[must_use]
pub fn input_spec_digest(
    parameters: &DenoiseInputSpecParameters<'_>,
    contract: &DenoiseInputSpecContract,
) -> String {
    let text = canonical_input_spec_text(parameters, contract);
    let digest = compute_sha256_hex(text.as_bytes())
        .expect("hashing an in-memory denoise input-spec buffer cannot fail");
    format!("{DENOISE_SHA256_PREFIX}{digest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(contract: &DenoiseInputSpecContract) -> String {
        input_spec_digest(
            &DenoiseInputSpecParameters {
                model_input_digest: "sha256:model-input",
                input_tensor_name: "image",
                output_tensor_name: "output",
                tile_width: 512,
                tile_height: 512,
                overlap: 32,
                preprocessing_name: "denoise_identity_unit",
                preprocessing_version: "1",
                rescaling_method: "identity",
            },
            contract,
        )
    }

    #[test]
    fn v2_tag_and_behavior_components_are_explicit() {
        let text = canonical_input_spec_text(
            &DenoiseInputSpecParameters {
                model_input_digest: "sha256:model-input",
                input_tensor_name: "image",
                output_tensor_name: "output",
                tile_width: 512,
                tile_height: 512,
                overlap: 32,
                preprocessing_name: "denoise_identity_unit",
                preprocessing_version: "1",
                rescaling_method: "identity",
            },
            &DenoiseInputSpecContract::default(),
        );
        assert!(text.starts_with("lumina-denoise-input-spec-v2|"));
        assert!(text.contains("blend_algorithm=lumina-denoise-core-blend:1"));
        assert!(text.contains("assembly_algorithm=lumina-denoise-distance-to-edge-assembly:1"));
        assert!(text.contains("detail_scale=32"));
        assert!(text.contains("zero_strength_policy=strict-identity-no-mutation"));
        assert!(text.contains("output_encoding=rgb8"));
        assert!(text.contains("output_shape=nchw-[1,3,H,W]"));
        assert!(text.contains("output_range=unit-0-1-to-rgb8-round-clamp"));
        assert!(text.contains("payload_encoding_version=1"));
        assert_eq!(
            lumina_core::DENOISE_DETAIL_SCALE.to_bits(),
            32.0_f32.to_bits()
        );
        assert!(!text.contains("lumina-denoise-input-spec-v1"));
    }

    #[test]
    fn changing_blend_or_assembly_contract_changes_digest() {
        let base = DenoiseInputSpecContract::default();
        let baseline = digest(&base);

        let mut blend_changed = base;
        blend_changed.blend.version = "2";
        assert_ne!(digest(&blend_changed), baseline);

        let mut assembly_changed = base;
        assembly_changed.assembly.version = "2";
        assert_ne!(digest(&assembly_changed), baseline);
    }
}
