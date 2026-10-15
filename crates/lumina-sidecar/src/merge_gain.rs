//! Slice A (gain compensation): the recipe-side gain value, its loud
//! validation and its tests (A-T5/A-T6).
//!
//! Extracted from `merge_recipe.rs` (file-size ratchet); the schema,
//! the validation and the digest behaviour are unchanged by the move:
//! `gain_compensation` lives in `MergeAlignment`, so it stays part of
//! `merge_digest`, and `None` still serialises to absent (no digest
//! drift for pre-gain bundles).

use serde::{Deserialize, Serialize};

use crate::merge_recipe::invalid;
use crate::{MergeMode, SidecarError};

/// Slice A (gain compensation): upper bound for one panorama gain factor
/// (≈ ±3 EV). A larger measured gain almost always signals wrong overlap
/// (mis-alignment), not real exposure difference — it is rejected loudly
/// (`Unsupported` in the estimation, `Invalid` in persisted recipes), never
/// clamped.
pub const MAX_PANO_GAIN: f64 = 8.0;
/// Slice A (gain compensation): minimum geometric overlap in pixels the gain
/// statistic may stand on. Below this the estimation refuses loudly
/// (`Unsupported`), and a persisted recipe below it is `Invalid`.
pub const A_MIN_OVERLAP_STAT_PX: u64 = 1024;

/// Slice A: the gain-estimation method of a panorama merge. The only 1.0
/// variant is the overlap median (median of the per-pixel reference/frame
/// luminance ratio over the full geometric overlap vs. frame 0). Unknown
/// methods fail deserialization loudly instead of being guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanoGainMethod {
    OverlapMedian,
}

/// Slice A: panorama gain compensation, estimated from pixels and persisted
/// as its own recipe value (never mixed into the EXIF provenance
/// `MergeExposure` (see `merge_recipe`)). `gains[i]` multiplies frame `i` before the feather
/// combination; `gains[0]` is 1.0 by reference definition.
/// `min_overlap_px` is the smallest pairwise overlap pixel count the
/// statistic stands on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanoGainCompensation {
    pub method: PanoGainMethod,
    pub gains: Vec<f64>,
    pub min_overlap_px: u64,
}

/// Slice A: loud validation of the persisted gain compensation (see
/// [`PanoGainCompensation`]). Every deviation is `Invalid` — gains are never
/// silently padded, truncated, clamped or reinterpreted.
pub(crate) fn validate_pano_gain_compensation(
    gain: Option<&PanoGainCompensation>,
    source_count: usize,
    mode: MergeMode,
) -> Result<(), SidecarError> {
    let Some(gain) = gain else { return Ok(()) };
    if mode != MergeMode::Panorama {
        return invalid("merge gain compensation is panorama only");
    }
    if gain.gains.len() != source_count {
        return invalid(format!(
            "merge gain_compensation needs one gain per source, got {} gains for {source_count} sources",
            gain.gains.len()
        ));
    }
    if gain.gains[0] != 1.0 {
        return invalid(format!(
            "merge gain_compensation gains[0] must be 1.0 (reference definition), got {}",
            gain.gains[0]
        ));
    }
    for (index, value) in gain.gains.iter().enumerate() {
        if !value.is_finite() || *value <= 0.0 {
            return invalid(format!(
                "merge gain_compensation gains[#{index}] must be finite and > 0, got {value}"
            ));
        }
        if *value > MAX_PANO_GAIN {
            return invalid(format!(
                "merge gain_compensation gains[#{index}] {value} exceeds {MAX_PANO_GAIN} (never clamped)"
            ));
        }
    }
    if gain.min_overlap_px < A_MIN_OVERLAP_STAT_PX {
        return invalid(format!(
            "merge gain_compensation min_overlap_px {} is below {A_MIN_OVERLAP_STAT_PX}",
            gain.min_overlap_px
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_fixtures {
    //! Shared recipe fixtures (also used by `merge_recipe` tests).
    use crate::merge_recipe::{
        MergeAlignment, MergeAlignmentMethod, MergeDecodeContext, MergeExposure, MergeOutput,
        MergeProjection, MergeRecipe, MergeSource, MergeStatus, MergeTransform, MERGE_HASH_PREFIX,
        MERGE_OUTPUT_BITS, MERGE_RECIPE_VERSION,
    };
    use crate::MergeMode;
    /// Source-hash filler: one repeated byte, prefixed like a real hash.
    pub(crate) fn hex_hash(fill: u8) -> String {
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

    pub(crate) fn sample_recipe() -> MergeRecipe {
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
                gain_compensation: None,
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

    pub(crate) fn sample_panorama_recipe() -> MergeRecipe {
        let mut recipe = sample_recipe();
        recipe.mode = MergeMode::Panorama;
        recipe.alignment.method = MergeAlignmentMethod::PanoCylindricalHomography;
        recipe.alignment.projection = MergeProjection::Cylindrical;
        recipe.output.file = "IMG_0001-Pano.dng".into();
        recipe
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::{sample_panorama_recipe, sample_recipe};
    use super::*;
    use crate::{MergeAlignmentMethod, MergeMode, MergeProjection, MergeRecipe};
    /// A-T5: recipe roundtrip with gain is byte-stable, the digest moves with
    /// the gain, and old JSON (without the key) still parses to `None`.
    #[test]
    fn gain_compensation_roundtrip_digest_and_compat() {
        let mut recipe = sample_panorama_recipe();
        recipe.alignment.gain_compensation = Some(PanoGainCompensation {
            method: PanoGainMethod::OverlapMedian,
            gains: vec![1.0, 0.5],
            min_overlap_px: A_MIN_OVERLAP_STAT_PX,
        });
        recipe.validate().expect("gain recipe valid");
        // Byte-stable roundtrip (pattern `:543-554`).
        let json = recipe.to_json().expect("encodes");
        assert!(
            json.contains("gain_compensation"),
            "a present gain must serialise: {json}"
        );
        let parsed = MergeRecipe::from_json(&json).expect("parses + validates");
        assert_eq!(parsed, recipe);
        assert_eq!(parsed.to_json().expect("re-encodes"), json);
        // Digest sensitivity (pattern `:579-599`): same sources, other gains
        // = other identity.
        let base = recipe.digest();
        let mut changed = recipe.clone();
        changed.alignment.gain_compensation = Some(PanoGainCompensation {
            method: PanoGainMethod::OverlapMedian,
            gains: vec![1.0, 0.6],
            min_overlap_px: A_MIN_OVERLAP_STAT_PX,
        });
        assert_ne!(changed.digest(), base, "gain change alters digest");
        // Absent gain digests like no gain at all (None adds no key).
        let mut absent = recipe.clone();
        absent.alignment.gain_compensation = None;
        let mut pre_gain_shape = sample_panorama_recipe();
        pre_gain_shape.alignment.gain_compensation = None;
        assert_eq!(
            absent.digest(),
            pre_gain_shape.digest(),
            "None must not drift the digest"
        );
        // Old JSON (key missing entirely) parses to None (A.9 compat).
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        value
            .get_mut("alignment")
            .expect("alignment present")
            .as_object_mut()
            .expect("alignment object")
            .remove("gain_compensation");
        let old = MergeRecipe::from_json(&serde_json::to_string(&value).expect("re-encodes"))
            .expect("old sidecars parse");
        assert_eq!(old.alignment.gain_compensation, None);
        // Unknown method values fail loudly at parse (never guessed).
        let mut tampered: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        tampered["alignment"]["gain_compensation"]["method"] =
            serde_json::Value::from("exif-ratio");
        let err = MergeRecipe::from_json(&serde_json::to_string(&tampered).expect("re-encodes"))
            .expect_err("unknown gain method rejected");
        assert!(matches!(err, SidecarError::Json(_)), "got: {err}");
    }

    /// A-T6: gain validation matrix — every row is `Invalid`, never
    /// padded/truncated/clamped.
    #[test]
    fn gain_compensation_validation_matrix() {
        fn with_gain(recipe: &mut MergeRecipe, gain: PanoGainCompensation) {
            recipe.mode = MergeMode::Panorama;
            recipe.alignment.method = MergeAlignmentMethod::PanoCylindricalHomography;
            recipe.alignment.projection = MergeProjection::Cylindrical;
            recipe.alignment.gain_compensation = Some(gain);
        }
        fn good() -> PanoGainCompensation {
            PanoGainCompensation {
                method: PanoGainMethod::OverlapMedian,
                gains: vec![1.0, 0.5],
                min_overlap_px: A_MIN_OVERLAP_STAT_PX,
            }
        }
        // (name, gain-mutator or mode override)
        type Mutate = Box<dyn Fn(&mut MergeRecipe)>;
        let cases: Vec<(&str, Mutate)> = vec![
            (
                "len mismatch (1 gain, 2 sources)",
                Box::new(|r| {
                    let mut g = good();
                    g.gains.truncate(1);
                    with_gain(r, g);
                }),
            ),
            (
                "len mismatch (3 gains, 2 sources)",
                Box::new(|r| {
                    let mut g = good();
                    g.gains.push(0.5);
                    with_gain(r, g);
                }),
            ),
            (
                "gains[0] != 1.0",
                Box::new(|r| {
                    let mut g = good();
                    g.gains[0] = 0.5;
                    with_gain(r, g);
                }),
            ),
            (
                "gain == 0",
                Box::new(|r| {
                    let mut g = good();
                    g.gains[1] = 0.0;
                    with_gain(r, g);
                }),
            ),
            (
                "gain < 0",
                Box::new(|r| {
                    let mut g = good();
                    g.gains[1] = -0.5;
                    with_gain(r, g);
                }),
            ),
            (
                "gain NaN",
                Box::new(|r| {
                    let mut g = good();
                    g.gains[1] = f64::NAN;
                    with_gain(r, g);
                }),
            ),
            (
                "gain +Inf",
                Box::new(|r| {
                    let mut g = good();
                    g.gains[1] = f64::INFINITY;
                    with_gain(r, g);
                }),
            ),
            (
                "gain above MAX_PANO_GAIN",
                Box::new(|r| {
                    let mut g = good();
                    g.gains[1] = MAX_PANO_GAIN + 0.5;
                    with_gain(r, g);
                }),
            ),
            (
                "min_overlap_px below minimum",
                Box::new(|r| {
                    let mut g = good();
                    g.min_overlap_px = A_MIN_OVERLAP_STAT_PX - 1;
                    with_gain(r, g);
                }),
            ),
            (
                "gain on HDR mode",
                Box::new(|r| {
                    // sample_recipe() is HDR; keep it, only attach a gain.
                    r.alignment.gain_compensation = Some(good());
                }),
            ),
        ];
        for (name, mutate) in cases {
            let mut recipe = sample_recipe();
            mutate(&mut recipe);
            let err = recipe.validate().expect_err(&format!(
                "case `{name}` must be rejected, not padded or clamped"
            ));
            assert!(
                matches!(err, SidecarError::Invalid(_)),
                "case `{name}` must be Invalid, got: {err}"
            );
        }
        // Boundary values are accepted: exactly MAX and exactly the minimum.
        let mut recipe = sample_panorama_recipe();
        with_gain(
            &mut recipe,
            PanoGainCompensation {
                method: PanoGainMethod::OverlapMedian,
                gains: vec![1.0, MAX_PANO_GAIN],
                min_overlap_px: A_MIN_OVERLAP_STAT_PX,
            },
        );
        recipe.validate().expect("boundary values valid");
    }
}
