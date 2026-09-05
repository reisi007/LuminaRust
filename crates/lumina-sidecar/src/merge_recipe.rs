//! LRPAR-G13-MERGE-15 / MERGE-SCHEMA-1: versioned merge-recipe schema.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (Rezept-Stufen-Vorschlag,
//! Persistenz, Normative Invarianten) plus Konkretisierung
//! „MERGE-SCHEMA-1" ebenda. A merge recipe is the declarative, versioned
//! provenance record of an HDR/panorama merge: source set + alignment +
//! mode. It lives in the merge-DNG's own sidecar bundle (sidecar-first);
//! the source sidecars are never mutated.
//!
//! Scope: schema + validation + digest only. No DNG writer, no alignment
//! computation, no CLI/GUI (follow-up waves MERGE-CORE-1 … MERGE-GOLDEN-1).
//!
//! Failure policy (Agents.md): every deviation is rejected loudly — never
//! silently clipped, defaulted, or reinterpreted. Pre-MVP schema decision:
//! `merge_version` is pinned to 1; foreign versions are rejected, there is
//! no silent migration.

use serde::{Deserialize, Serialize};

use crate::{validate_metadata_timestamp, SidecarError};

/// Current merge-recipe schema version. Foreign versions are rejected
/// loudly (pre-MVP: no back-compat obligation, but always versioned).
pub const MERGE_RECIPE_VERSION: u32 = 1;

/// Minimum number of sources: a merge needs at least a pair (exposure
/// bracket or panorama overlap pair).
pub const MIN_MERGE_SOURCES: usize = 2;
/// Upper bound against hostile/degenerate documents; panorama chains in the
/// 1.5 scope (single chain, cylindrical) stay far below this.
pub const MAX_MERGE_SOURCES: usize = 256;
/// Upper bound for the alignment residual: 100k px is absurd for any real
/// sensor and only caps degenerate input.
pub const MAX_MERGE_RESIDUAL_PX: f64 = 100_000.0;
/// Upper bound for the feather-blend width in the overlap.
pub const MAX_MERGE_BLEND_WIDTH_PX: u32 = 65_536;
/// Upper bound for a single exposure time (24 h); above is degenerate input.
pub const MAX_MERGE_EXPOSURE_TIME_S: f64 = 86_400.0;
/// Upper bound for ISO (EXIF integer range with headroom).
pub const MAX_MERGE_ISO: u32 = 10_000_000;
/// Upper bound for the f-number; above is degenerate input.
pub const MAX_MERGE_F_NUMBER: f64 = 256.0;
/// 1.5 scope: the only written DNG flavour is linear 16-bit non-mosaic.
pub const MERGE_OUTPUT_BITS: u16 = 16;

/// Checksum prefix shared with the AI-mask source-hash contract
/// (`blake3:<64 lowercase hex>`, BLAKE3-256 of the source bytes).
pub const MERGE_HASH_PREFIX: &str = "blake3:";
/// Hex length of a BLAKE3-256 digest.
pub const MERGE_HASH_HEX_LEN: usize = 64;

/// The merge path. Persisted lowercase (`hdr` | `panorama`); unknown modes
/// fail deserialization loudly instead of being guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeMode {
    Hdr,
    Panorama,
}

/// The 1.5 alignment scope: HDR is translation-only, panorama is a chained
/// cylindrical homography. Unknown methods are rejected, never reinterpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeAlignmentMethod {
    HdrTranslate,
    PanoCylindricalHomography,
}

/// Projection applied before blending. 1.5 scope: HDR performs no
/// projection, panorama is cylindrical only (no spherical/fisheye).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeProjection {
    None,
    Cylindrical,
}

/// Merge artefact status. `Stale`/`Missing`/`Unsupported` are visible states
/// (CLI/GUI surface them); there is no silent re-generation as the only
/// option and no silent single-image fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeStatus {
    Ok,
    Stale,
    Missing,
    Unsupported,
}

/// Decode identity of one merge source (decoder contract + orientation, see
/// the normative JSON sketch in the decision document).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeDecodeContext {
    pub decoder: String,
    pub decode_version: String,
    pub orientation: u8,
}

/// EXIF exposure values the pre-blend exposure compensation is computed
/// from exclusively. Missing EXIF exposure makes an HDR merge `unsupported`
/// downstream — it is never guessed from pixels (decision document).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeExposure {
    pub exposure_time_s: f64,
    pub iso: u32,
    pub f_number: f64,
}

/// One merge input. `path` is relative to the merge sidecar bundle;
/// absolute paths are forbidden so a moved bundle stays valid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeSource {
    pub path: String,
    pub content_hash: String,
    pub decode_context: MergeDecodeContext,
    pub exposure: MergeExposure,
}

/// Alignment transform of one source: row-major 3x3 matrix mapping the
/// source into the merge reference frame. The reference source (index 0)
/// conventionally carries no entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeTransform {
    pub source_index: usize,
    pub matrix_3x3: [f64; 9],
}

/// Reproducible alignment parameters: part of the merge identity and hence
/// of the digest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeAlignment {
    pub method: MergeAlignmentMethod,
    pub transforms: Vec<MergeTransform>,
    pub residual_px: f64,
    pub projection: MergeProjection,
    pub blend_width_px: u32,
}

/// Merge output descriptor. 1.5 scope: linear 16-bit non-mosaic DNG only;
/// anything else is rejected loudly (no silent TIFF/PNG-as-DNG substitute).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeOutput {
    pub file: String,
    pub bits: u16,
    pub mosaic: bool,
}

/// Declarative, versioned HDR/panorama merge recipe (sidecar-first
/// provenance of the merge DNG).
///
/// Note on the decision document's `"type": "merge"` envelope key: it is
/// deliberately *not* a field here. The envelope discriminator belongs to
/// the embedding sidecar document and is decided in MERGE-DNG-1; this type
/// is the recipe payload itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRecipe {
    pub merge_version: u32,
    pub mode: MergeMode,
    pub sources: Vec<MergeSource>,
    pub alignment: MergeAlignment,
    pub output: MergeOutput,
    pub created_at: String,
    pub status: MergeStatus,
    pub error: Option<String>,
}

impl MergeRecipe {
    /// Loud validation: unknown modes/methods and infinite/NaN/out-of-range
    /// values are rejected, never clipped or defaulted.
    pub fn validate(&self) -> Result<(), SidecarError> {
        validate_merge_recipe(self)
    }

    /// Deterministic digest over source set + alignment + mode (plus schema
    /// version) for stale detection. Metadata (`output`, `created_at`,
    /// `status`, `error`) is excluded: it describes the artefact, not the
    /// merge identity.
    pub fn digest(&self) -> String {
        merge_digest(self)
    }

    /// Canonical JSON encoding (key-sorted, no whitespace). Parsing it back
    /// with [`MergeRecipe::from_json`] reproduces the exact bytes.
    pub fn to_json(&self) -> Result<String, SidecarError> {
        serde_json::to_string(self)
            .map_err(|e| SidecarError::Json(format!("cannot encode merge recipe: {e}")))
    }

    /// Parses *and* validates: malformed JSON and schema violations are both
    /// loud errors.
    pub fn from_json(json: &str) -> Result<Self, SidecarError> {
        let recipe: Self = serde_json::from_str(json)
            .map_err(|e| SidecarError::Json(format!("invalid merge recipe JSON: {e}")))?;
        recipe.validate()?;
        Ok(recipe)
    }
}

/// Loud validation of a merge recipe (see [`MergeRecipe::validate`]).
pub fn validate_merge_recipe(recipe: &MergeRecipe) -> Result<(), SidecarError> {
    if recipe.merge_version != MERGE_RECIPE_VERSION {
        return invalid(format!(
            "unsupported merge_version {} (expected {MERGE_RECIPE_VERSION})",
            recipe.merge_version
        ));
    }
    if recipe.sources.len() < MIN_MERGE_SOURCES {
        return invalid(format!(
            "merge needs at least {MIN_MERGE_SOURCES} sources, got {}",
            recipe.sources.len()
        ));
    }
    if recipe.sources.len() > MAX_MERGE_SOURCES {
        return invalid(format!(
            "merge exceeds maximum of {MAX_MERGE_SOURCES} sources, got {}",
            recipe.sources.len()
        ));
    }
    for (index, source) in recipe.sources.iter().enumerate() {
        validate_merge_source(source)
            .map_err(|e| SidecarError::Invalid(format!("merge source #{index}: {e}")))?;
    }
    validate_merge_alignment(&recipe.alignment)
        .map_err(|e| SidecarError::Invalid(format!("merge alignment: {e}")))?;
    validate_mode_alignment(recipe.mode, &recipe.alignment)?;
    validate_merge_transforms(&recipe.alignment.transforms, recipe.sources.len())?;
    validate_merge_output(&recipe.output)?;
    validate_metadata_timestamp(&recipe.created_at)
        .map_err(|e| SidecarError::Invalid(format!("merge created_at invalid: {e}")))?;
    validate_merge_status(recipe.status, recipe.error.as_deref())?;
    Ok(())
}

/// Deterministic digest over source set + alignment + mode (plus schema
/// version) for stale detection: any change there must visibly invalidate
/// the merge DNG (`stale`), never trigger a silent re-generation.
/// The digest input is canonical JSON (key-sorted object); the digest is
/// rendered as `blake3:<hex>`.
pub fn merge_digest(recipe: &MergeRecipe) -> String {
    let canonical = serde_json::json!({
        "merge_version": recipe.merge_version,
        "mode": recipe.mode,
        "sources": recipe.sources,
        "alignment": recipe.alignment,
    });
    // `serde_json::Map` is key-sorted (no `preserve_order` feature), so this
    // encoding is canonical for a fixed schema.
    let bytes = serde_json::to_string(&canonical).unwrap_or_default();
    format!(
        "{MERGE_HASH_PREFIX}{}",
        blake3::hash(bytes.as_bytes()).to_hex()
    )
}

fn invalid<T>(message: impl Into<String>) -> Result<T, SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}

fn validate_merge_source(source: &MergeSource) -> Result<(), SidecarError> {
    validate_merge_relative_path("path", &source.path)?;
    validate_merge_content_hash(&source.content_hash)?;
    validate_merge_decode_context(&source.decode_context)?;
    validate_merge_exposure(&source.exposure)?;
    Ok(())
}

fn validate_merge_relative_path(field: &str, value: &str) -> Result<(), SidecarError> {
    // Same portable-bundle contract as sidecar artifact paths: no absolute
    // paths, no backslashes/drives, no empty/dot/dot-dot segments, so a
    // moved bundle (sources + merge DNG + sidecars) stays valid.
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || value.starts_with("//")
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return invalid(format!(
            "merge {field} must be a safe portable relative path"
        ));
    }
    Ok(())
}

fn validate_merge_content_hash(value: &str) -> Result<(), SidecarError> {
    const HINT: &str = "merge content_hash must be `blake3:<64 lowercase hex>`";
    let Some(hex) = value.strip_prefix(MERGE_HASH_PREFIX) else {
        return invalid(HINT);
    };
    if hex.len() != MERGE_HASH_HEX_LEN
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return invalid(HINT);
    }
    Ok(())
}

fn validate_merge_decode_context(context: &MergeDecodeContext) -> Result<(), SidecarError> {
    if context.decoder.trim().is_empty() {
        return invalid("merge decode_context.decoder must not be empty");
    }
    if context.decode_version.trim().is_empty() {
        return invalid("merge decode_context.decode_version must not be empty");
    }
    if !(1..=8).contains(&context.orientation) {
        return invalid(format!(
            "merge decode_context.orientation must be 1..=8, got {}",
            context.orientation
        ));
    }
    Ok(())
}

fn validate_merge_exposure(exposure: &MergeExposure) -> Result<(), SidecarError> {
    if !exposure.exposure_time_s.is_finite()
        || exposure.exposure_time_s <= 0.0
        || exposure.exposure_time_s > MAX_MERGE_EXPOSURE_TIME_S
    {
        return invalid(format!(
            "merge exposure_time_s must be finite within (0, {MAX_MERGE_EXPOSURE_TIME_S}], got {}",
            exposure.exposure_time_s
        ));
    }
    if exposure.iso == 0 || exposure.iso > MAX_MERGE_ISO {
        return invalid(format!(
            "merge iso must be within 1..={MAX_MERGE_ISO}, got {}",
            exposure.iso
        ));
    }
    if !exposure.f_number.is_finite()
        || exposure.f_number <= 0.0
        || exposure.f_number > MAX_MERGE_F_NUMBER
    {
        return invalid(format!(
            "merge f_number must be finite within (0, {MAX_MERGE_F_NUMBER}], got {}",
            exposure.f_number
        ));
    }
    Ok(())
}

fn validate_merge_alignment(alignment: &MergeAlignment) -> Result<(), SidecarError> {
    if !alignment.residual_px.is_finite()
        || alignment.residual_px < 0.0
        || alignment.residual_px > MAX_MERGE_RESIDUAL_PX
    {
        return invalid(format!(
            "merge residual_px must be finite within 0..={MAX_MERGE_RESIDUAL_PX}, got {}",
            alignment.residual_px
        ));
    }
    if alignment.blend_width_px > MAX_MERGE_BLEND_WIDTH_PX {
        return invalid(format!(
            "merge blend_width_px must be within 0..={MAX_MERGE_BLEND_WIDTH_PX}, got {}",
            alignment.blend_width_px
        ));
    }
    for (index, transform) in alignment.transforms.iter().enumerate() {
        if transform.matrix_3x3.iter().any(|v| !v.is_finite()) {
            return invalid(format!(
                "merge transform #{index} matrix_3x3 must be finite"
            ));
        }
    }
    Ok(())
}

/// 1.5 scope pairing: HDR is translation-only without projection, panorama
/// is the chained cylindrical homography. Anything else is rejected loudly
/// (no silent reinterpretation of a foreign pipeline).
fn validate_mode_alignment(
    mode: MergeMode,
    alignment: &MergeAlignment,
) -> Result<(), SidecarError> {
    let (expected_method, expected_projection) = match mode {
        MergeMode::Hdr => (MergeAlignmentMethod::HdrTranslate, MergeProjection::None),
        MergeMode::Panorama => (
            MergeAlignmentMethod::PanoCylindricalHomography,
            MergeProjection::Cylindrical,
        ),
    };
    if alignment.method != expected_method {
        return invalid(format!(
            "merge mode `{mode:?}` requires method `{expected_method:?}`, got `{:?}`",
            alignment.method
        ));
    }
    if alignment.projection != expected_projection {
        return invalid(format!(
            "merge mode `{mode:?}` requires projection `{expected_projection:?}`, got `{:?}`",
            alignment.projection
        ));
    }
    Ok(())
}

fn validate_merge_transforms(
    transforms: &[MergeTransform],
    source_count: usize,
) -> Result<(), SidecarError> {
    if transforms.len() > source_count {
        return invalid(format!(
            "merge has more transforms ({}) than sources ({source_count})",
            transforms.len()
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for (index, transform) in transforms.iter().enumerate() {
        if transform.source_index >= source_count {
            return invalid(format!(
                "merge transform #{index} references source {} of {source_count}",
                transform.source_index
            ));
        }
        if !seen.insert(transform.source_index) {
            return invalid(format!(
                "duplicate merge transform for source {}",
                transform.source_index
            ));
        }
    }
    Ok(())
}

fn validate_merge_output(output: &MergeOutput) -> Result<(), SidecarError> {
    validate_merge_relative_path("output file", &output.file)?;
    if output.bits != MERGE_OUTPUT_BITS {
        return invalid(format!(
            "merge output bits must be {}, got {}",
            MERGE_OUTPUT_BITS, output.bits
        ));
    }
    if output.mosaic {
        return invalid("merge output must be linear (mosaic = false)");
    }
    Ok(())
}

/// `ok` carries no error text (a contradiction is rejected); every other
/// status may carry an optional, non-blank error text.
fn validate_merge_status(status: MergeStatus, error: Option<&str>) -> Result<(), SidecarError> {
    match (status, error) {
        (MergeStatus::Ok, Some(_)) => invalid("merge status `ok` must not carry an error text"),
        (_, Some(text)) if text.trim().is_empty() => {
            invalid("merge error text must not be empty or whitespace-only")
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_hash(fill: u8) -> String {
        format!("{MERGE_HASH_PREFIX}{}", hex::fill(fill))
    }

    /// Local hex filler without a new dependency: repeat one byte value.
    mod hex {
        pub fn fill(byte: u8) -> String {
            let digit = format!("{byte:02x}");
            digit.repeat(32)
        }
    }

    fn sample_source(path: &str, hash_fill: u8, exposure_time_s: f64) -> MergeSource {
        MergeSource {
            path: path.into(),
            content_hash: hex_hash(hash_fill),
            decode_context: MergeDecodeContext {
                decoder: "libraw".into(),
                decode_version: "0.22.2+luminaabi1".into(),
                orientation: 1,
            },
            exposure: MergeExposure {
                exposure_time_s,
                iso: 100,
                f_number: 8.0,
            },
        }
    }

    fn sample_recipe() -> MergeRecipe {
        MergeRecipe {
            merge_version: MERGE_RECIPE_VERSION,
            mode: MergeMode::Hdr,
            sources: vec![
                sample_source("IMG_0001.ARW", 0xab, 0.01),
                sample_source("IMG_0002.ARW", 0xcd, 0.04),
            ],
            alignment: MergeAlignment {
                method: MergeAlignmentMethod::HdrTranslate,
                transforms: vec![MergeTransform {
                    source_index: 1,
                    matrix_3x3: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                }],
                residual_px: 0.4,
                projection: MergeProjection::None,
                blend_width_px: 64,
            },
            output: MergeOutput {
                file: "IMG_0001-HDR.dng".into(),
                bits: MERGE_OUTPUT_BITS,
                mosaic: false,
            },
            created_at: "2026-09-05T00:00:00Z".into(),
            status: MergeStatus::Ok,
            error: None,
        }
    }

    fn sample_panorama_recipe() -> MergeRecipe {
        let mut recipe = sample_recipe();
        recipe.mode = MergeMode::Panorama;
        recipe.alignment.method = MergeAlignmentMethod::PanoCylindricalHomography;
        recipe.alignment.projection = MergeProjection::Cylindrical;
        recipe.output.file = "IMG_0001-Pano.dng".into();
        recipe
    }

    #[test]
    fn valid_hdr_and_panorama_recipes_pass() {
        sample_recipe().validate().expect("hdr fixture valid");
        sample_panorama_recipe()
            .validate()
            .expect("panorama fixture valid");
    }

    #[test]
    fn json_roundtrip_is_byte_stable() {
        let recipe = sample_recipe();
        let json = recipe.to_json().expect("encodes");
        let parsed = MergeRecipe::from_json(&json).expect("parses + validates");
        assert_eq!(parsed, recipe);
        assert_eq!(
            parsed.to_json().expect("re-encodes"),
            json,
            "JSON -> type -> JSON must be byte-stable"
        );
    }

    #[test]
    fn panorama_roundtrip_is_byte_stable() {
        let recipe = sample_panorama_recipe();
        let json = recipe.to_json().expect("encodes");
        let parsed = MergeRecipe::from_json(&json).expect("parses + validates");
        assert_eq!(parsed, recipe);
        assert_eq!(parsed.to_json().expect("re-encodes"), json);
    }

    #[test]
    fn digest_is_deterministic_and_prefixed() {
        let recipe = sample_recipe();
        let first = recipe.digest();
        let second = sample_recipe().digest();
        assert_eq!(first, second, "same recipe -> same digest");
        assert!(first.starts_with(MERGE_HASH_PREFIX), "blake3: contract");
        assert_eq!(
            first.len(),
            MERGE_HASH_PREFIX.len() + MERGE_HASH_HEX_LEN,
            "blake3:<64 hex>"
        );
    }

    #[test]
    fn digest_changes_on_identity_change() {
        let base = sample_recipe().digest();
        // Mode change.
        let mut changed = sample_recipe();
        changed.mode = MergeMode::Panorama;
        changed.alignment.method = MergeAlignmentMethod::PanoCylindricalHomography;
        changed.alignment.projection = MergeProjection::Cylindrical;
        assert_ne!(changed.digest(), base, "mode change alters digest");
        // Source-set change.
        let mut changed = sample_recipe();
        changed.sources[0].content_hash = hex_hash(0x11);
        assert_ne!(changed.digest(), base, "source change alters digest");
        // Alignment change.
        let mut changed = sample_recipe();
        changed.alignment.residual_px = 0.5;
        assert_ne!(changed.digest(), base, "alignment change alters digest");
        let mut changed = sample_recipe();
        changed.alignment.transforms.clear();
        assert_ne!(changed.digest(), base, "transform change alters digest");
    }

    #[test]
    fn digest_ignores_artefact_metadata() {
        let base = sample_recipe().digest();
        for mutate in [
            |r: &mut MergeRecipe| r.status = MergeStatus::Stale,
            |r: &mut MergeRecipe| {
                r.status = MergeStatus::Missing;
                r.error = Some("dng gone".into());
            },
            |r: &mut MergeRecipe| r.created_at = "2026-09-06T00:00:00Z".into(),
            |r: &mut MergeRecipe| r.output.file = "other-HDR.dng".into(),
        ] {
            let mut changed = sample_recipe();
            mutate(&mut changed);
            assert_eq!(
                changed.digest(),
                base,
                "status/error/timestamp/output are not merge identity"
            );
        }
    }

    #[test]
    fn foreign_merge_version_rejected_without_migration() {
        for version in [0, 2, u32::MAX] {
            let mut recipe = sample_recipe();
            recipe.merge_version = version;
            assert!(
                recipe.validate().is_err(),
                "merge_version {version} must be rejected (pre-MVP, no back-compat)"
            );
        }
    }

    /// Validation matrix: every row must fail loudly (never clip/default).
    #[test]
    fn validation_matrix_rejects_every_error_path() {
        type Mutate = Box<dyn Fn(&mut MergeRecipe)>;
        let cases: Vec<(&str, Mutate)> = vec![
            ("no sources", Box::new(|r| r.sources.clear())),
            ("single source", Box::new(|r| r.sources.truncate(1))),
            (
                "absolute source path",
                Box::new(|r| r.sources[0].path = "/abs/IMG_0001.ARW".into()),
            ),
            (
                "backslash source path",
                Box::new(|r| r.sources[0].path = "dir\\IMG_0001.ARW".into()),
            ),
            (
                "dotdot source path",
                Box::new(|r| r.sources[0].path = "../IMG_0001.ARW".into()),
            ),
            (
                "drive source path",
                Box::new(|r| r.sources[0].path = "C:/IMG_0001.ARW".into()),
            ),
            ("empty source path", Box::new(|r| r.sources[0].path.clear())),
            (
                "hash without prefix",
                Box::new(|r| r.sources[0].content_hash = "ab".repeat(32)),
            ),
            (
                "short hash",
                Box::new(|r| r.sources[0].content_hash = "blake3:ab".into()),
            ),
            (
                "uppercase hash",
                Box::new(|r| r.sources[0].content_hash = format!("blake3:{}", "AB".repeat(32))),
            ),
            (
                "non-hex hash",
                Box::new(|r| {
                    r.sources[0].content_hash = format!("blake3:{}", "zz".repeat(32));
                }),
            ),
            (
                "empty decoder",
                Box::new(|r| r.sources[0].decode_context.decoder = "  ".into()),
            ),
            (
                "empty decode version",
                Box::new(|r| r.sources[0].decode_context.decode_version.clear()),
            ),
            (
                "orientation 0",
                Box::new(|r| r.sources[0].decode_context.orientation = 0),
            ),
            (
                "orientation 9",
                Box::new(|r| r.sources[0].decode_context.orientation = 9),
            ),
            (
                "zero exposure time",
                Box::new(|r| r.sources[0].exposure.exposure_time_s = 0.0),
            ),
            (
                "negative exposure time",
                Box::new(|r| r.sources[0].exposure.exposure_time_s = -0.01),
            ),
            (
                "NaN exposure time",
                Box::new(|r| r.sources[0].exposure.exposure_time_s = f64::NAN),
            ),
            (
                "infinite exposure time",
                Box::new(|r| r.sources[0].exposure.exposure_time_s = f64::INFINITY),
            ),
            (
                "oversized exposure time",
                Box::new(|r| {
                    r.sources[0].exposure.exposure_time_s = MAX_MERGE_EXPOSURE_TIME_S + 1.0;
                }),
            ),
            ("zero iso", Box::new(|r| r.sources[0].exposure.iso = 0)),
            (
                "oversized iso",
                Box::new(|r| r.sources[0].exposure.iso = MAX_MERGE_ISO + 1),
            ),
            (
                "zero f-number",
                Box::new(|r| r.sources[0].exposure.f_number = 0.0),
            ),
            (
                "NaN f-number",
                Box::new(|r| r.sources[0].exposure.f_number = f64::NAN),
            ),
            (
                "oversized f-number",
                Box::new(|r| r.sources[0].exposure.f_number = MAX_MERGE_F_NUMBER + 1.0),
            ),
            (
                "negative residual",
                Box::new(|r| r.alignment.residual_px = -0.1),
            ),
            (
                "NaN residual",
                Box::new(|r| r.alignment.residual_px = f64::NAN),
            ),
            (
                "infinite residual",
                Box::new(|r| r.alignment.residual_px = f64::INFINITY),
            ),
            (
                "oversized residual",
                Box::new(|r| r.alignment.residual_px = MAX_MERGE_RESIDUAL_PX + 1.0),
            ),
            (
                "oversized blend width",
                Box::new(|r| r.alignment.blend_width_px = MAX_MERGE_BLEND_WIDTH_PX + 1),
            ),
            (
                "NaN matrix element",
                Box::new(|r| r.alignment.transforms[0].matrix_3x3[2] = f64::NAN),
            ),
            (
                "infinite matrix element",
                Box::new(|r| r.alignment.transforms[0].matrix_3x3[0] = f64::NEG_INFINITY),
            ),
            (
                "transform out of range",
                Box::new(|r| r.alignment.transforms[0].source_index = 2),
            ),
            (
                "duplicate transform",
                Box::new(|r| {
                    r.alignment.transforms.push(MergeTransform {
                        source_index: 1,
                        matrix_3x3: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    });
                }),
            ),
            (
                "more transforms than sources",
                Box::new(|r| {
                    for i in 0..3 {
                        r.alignment.transforms.push(MergeTransform {
                            source_index: i % 2,
                            matrix_3x3: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                        });
                    }
                }),
            ),
            (
                "hdr with pano method",
                Box::new(|r| {
                    r.alignment.method = MergeAlignmentMethod::PanoCylindricalHomography;
                    r.alignment.projection = MergeProjection::Cylindrical;
                }),
            ),
            (
                "hdr with cylindrical projection",
                Box::new(|r| r.alignment.projection = MergeProjection::Cylindrical),
            ),
            (
                "pano with translate method",
                Box::new(|r| {
                    r.mode = MergeMode::Panorama;
                    r.alignment.projection = MergeProjection::Cylindrical;
                }),
            ),
            (
                "pano without projection",
                Box::new(|r| {
                    r.mode = MergeMode::Panorama;
                    r.alignment.method = MergeAlignmentMethod::PanoCylindricalHomography;
                }),
            ),
            (
                "absolute output file",
                Box::new(|r| r.output.file = "/abs/out-HDR.dng".into()),
            ),
            ("bits 8", Box::new(|r| r.output.bits = 8)),
            ("bits 32", Box::new(|r| r.output.bits = 32)),
            ("mosaic output", Box::new(|r| r.output.mosaic = true)),
            (
                "ok with error text",
                Box::new(|r| r.error = Some("stale?".into())),
            ),
            (
                "blank error text",
                Box::new(|r| {
                    r.status = MergeStatus::Stale;
                    r.error = Some("   ".into());
                }),
            ),
            (
                "bad timestamp",
                Box::new(|r| r.created_at = "not-a-timestamp".into()),
            ),
            (
                "non-utc timestamp",
                Box::new(|r| r.created_at = "2026-09-05T00:00:00".into()),
            ),
        ];
        for (name, mutate) in cases {
            let mut recipe = sample_recipe();
            mutate(&mut recipe);
            assert!(
                recipe.validate().is_err(),
                "case `{name}` must be rejected, not clipped or defaulted"
            );
        }
    }

    #[test]
    fn non_ok_status_without_error_is_allowed() {
        for status in [
            MergeStatus::Stale,
            MergeStatus::Missing,
            MergeStatus::Unsupported,
        ] {
            let mut recipe = sample_recipe();
            recipe.status = status;
            recipe.validate().expect("error text is optional");
            recipe.error = Some("source changed".into());
            recipe.validate().expect("non-blank error text allowed");
        }
    }

    #[test]
    fn unknown_mode_method_projection_status_rejected_at_parse() {
        let base = sample_recipe().to_json().expect("encodes");
        for (name, key, value) in [
            ("mode", "mode", "focus-stack"),
            ("method", "method", "feature_match"),
            ("projection", "projection", "spherical"),
            ("status", "status", "done"),
        ] {
            let tampered = inject_alignment_or_root(&base, key, value);
            let err = MergeRecipe::from_json(&tampered).expect_err(&format!("{name} rejected"));
            assert!(
                matches!(err, SidecarError::Json(_)),
                "unknown {name} is a loud parse error, got: {err}"
            );
        }
    }

    /// Replaces the value of a top-level (`mode`, `status`) or alignment
    /// (`method`, `projection`) string key in canonical recipe JSON.
    fn inject_alignment_or_root(base: &str, key: &str, value: &str) -> String {
        let mut parsed: serde_json::Value = serde_json::from_str(base).expect("valid json");
        let target = if parsed.get(key).is_some() {
            &mut parsed
        } else {
            parsed
                .get_mut("alignment")
                .expect("alignment object present")
        };
        target[key] = serde_json::Value::String(value.into());
        serde_json::to_string(&target).expect("re-encodes")
    }

    #[test]
    fn unknown_fields_rejected_at_parse() {
        let mut parsed: serde_json::Value =
            serde_json::from_str(&sample_recipe().to_json().expect("encodes")).expect("valid json");
        parsed["future_field"] = serde_json::Value::from(1);
        let err = MergeRecipe::from_json(&serde_json::to_string(&parsed).expect("re-encodes"))
            .expect_err("unknown field rejected");
        assert!(matches!(err, SidecarError::Json(_)), "got: {err}");
    }

    #[test]
    fn malformed_json_is_a_loud_json_error() {
        let err = MergeRecipe::from_json("{not json").expect_err("malformed rejected");
        assert!(matches!(err, SidecarError::Json(_)), "got: {err}");
    }

    #[test]
    fn from_json_validates_semantics_not_just_syntax() {
        let mut parsed: serde_json::Value =
            serde_json::from_str(&sample_recipe().to_json().expect("encodes")).expect("valid json");
        parsed["sources"][0]["path"] = serde_json::Value::from("/abs/IMG_0001.ARW");
        let err = MergeRecipe::from_json(&serde_json::to_string(&parsed).expect("re-encodes"))
            .expect_err("absolute path rejected");
        assert!(matches!(err, SidecarError::Invalid(_)), "got: {err}");
    }
}
