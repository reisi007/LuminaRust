//! Slice B (deghosting): the recipe-side deghosting value, its loud
//! validation and its tests (B-T5).
//!
//! Extracted from `merge_recipe.rs` in the same move that added the field
//! (file-size ratchet); the schema, the validation and the digest behaviour
//! are the slice's own and are documented on [`HdrDeghost`]:
//! `deghost` lives in `MergeAlignment`, so it stays part of `merge_digest`,
//! and `None` still serialises to absent (no digest drift for pre-deghost
//! bundles — the same mechanism Slice A uses for `gain_compensation`).
//!
//! What lives here and what does not: this module owns the *value* and its
//! validation. The frontend spelling (`off` | a threshold number) and the
//! HDR-only options gate are `lumina_merge::hdr_deghost::HdrDeghostMode`,
//! the pixel-side mask is `lumina_merge::hdr_deghost::deghost_mask`. One
//! range, one place: the constants below are the single source the other
//! two read.

use serde::{Deserialize, Serialize};

use crate::merge_recipe::invalid;
use crate::{MergeMode, SidecarError};

/// Slice B: lower bound of the deghost threshold (exclusive). Below this a
/// relative radiance deviation is indistinguishable from sensor noise on a
/// dark bracket, and the mask would pierce holes into the merge.
pub const DEGHOST_MIN_THRESHOLD: f64 = 0.05;
/// Slice B: upper bound of the deghost threshold (inclusive). Above this
/// only a grossly mis-exposed frame is caught, which is the exposure
/// resolution's job, not the mask's.
pub const DEGHOST_MAX_THRESHOLD: f64 = 2.0;
/// Slice B: the documented opt-in default (25 % relative radiance
/// deviation). Not applied implicitly — a run without `deghost` keeps the
/// plain hat-weight mean (`None`), the value only shapes the recipe when a
/// frontend asks for it.
pub const DEGHOST_DEFAULT_THRESHOLD: f64 = 0.25;

/// Slice B: HDR ghost-mask parameters of one merge run.
///
/// `threshold` is the *relative* radiance deviation (per channel, against
/// the reference frame 0) above which a frame counts as a ghost at that
/// pixel and contributes hat weight 0. `reference_fallback` is the fallback
/// policy for a pixel where no frame survives: 1.0 implements exactly one
/// policy (use the reference frame's radiance, never the mean) and persists
/// it, so a future policy fails loudly at validation instead of silently
/// changing what an old recipe means.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HdrDeghost {
    pub threshold: f64,
    pub reference_fallback: bool,
}

/// Slice B: loud validation of the persisted deghosting (see
/// [`HdrDeghost`]). Every deviation is `Invalid` — the threshold is never
/// clamped into range, never defaulted and never reinterpreted, and an
/// unknown fallback policy is refused instead of being treated as "the
/// current one".
pub(crate) fn validate_hdr_deghost(
    deghost: Option<&HdrDeghost>,
    source_count: usize,
    mode: MergeMode,
) -> Result<(), SidecarError> {
    let Some(deghost) = deghost else {
        return Ok(());
    };
    if mode != MergeMode::Hdr {
        return invalid(
            "merge deghosting is HDR only (panorama frames are stitched, not bracketed)",
        );
    }
    if source_count < 2 {
        return invalid(format!(
            "merge deghosting needs at least 2 sources, got {source_count}"
        ));
    }
    if !deghost.threshold.is_finite()
        || deghost.threshold <= DEGHOST_MIN_THRESHOLD
        || deghost.threshold > DEGHOST_MAX_THRESHOLD
    {
        return invalid(format!(
            "merge deghost threshold must be finite within ({DEGHOST_MIN_THRESHOLD}, {DEGHOST_MAX_THRESHOLD}], got {}",
            deghost.threshold
        ));
    }
    if !deghost.reference_fallback {
        return invalid(
            "merge deghost reference_fallback must be true (the reference-frame fallback is the \
             only implemented policy; an unknown one is refused, never reinterpreted)"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge_gain::test_fixtures::sample_recipe;
    use crate::MergeRecipe;

    /// B-T5 — recipe roundtrip with deghosting is byte-stable, the digest
    /// moves with the threshold, and old JSON (without the key) still parses
    /// to `None`.
    #[test]
    fn deghost_roundtrip_digest_and_compat() {
        let mut recipe = sample_recipe();
        recipe.alignment.deghost = Some(HdrDeghost {
            threshold: DEGHOST_DEFAULT_THRESHOLD,
            reference_fallback: true,
        });
        recipe.validate().expect("deghost recipe valid");
        // Byte-stable roundtrip (the `merge_recipe.rs` pattern).
        let json = recipe.to_json().expect("encodes");
        assert!(
            json.contains("\"deghost\""),
            "a present deghost must serialise: {json}"
        );
        let parsed = MergeRecipe::from_json(&json).expect("parses + validates");
        assert_eq!(parsed, recipe);
        assert_eq!(parsed.to_json().expect("re-encodes"), json);
        // Digest sensitivity: same sources, another threshold = another
        // identity (two runs with 0.25 vs 0.5 produce different pixels from
        // the same bytes — the cache must not answer with the wrong one).
        let base = recipe.digest();
        let mut other = recipe.clone();
        other.alignment.deghost = Some(HdrDeghost {
            threshold: 0.5,
            reference_fallback: true,
        });
        assert_ne!(other.digest(), base, "threshold change alters digest");
        // Absent deghost adds no key to the digest input: the alignment
        // JSON exactly as `merge_digest` serialises it must not carry
        // `deghost`. This assertion is the mutation-killer for removing
        // `skip_serializing_if` — a `"deghost": null` key would drift the
        // digest against every pre-slice pin (T21 included).
        let mut absent = recipe.clone();
        absent.alignment.deghost = None;
        let alignment_json = serde_json::to_value(&absent.alignment).expect("alignment serialises");
        assert!(
            alignment_json.get("deghost").is_none(),
            "None must add no key to the digest input, got: {alignment_json}"
        );
        // Old JSON (key missing entirely) parses to None (B.9 compat).
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        value
            .get_mut("alignment")
            .expect("alignment present")
            .as_object_mut()
            .expect("alignment object")
            .remove("deghost");
        let old = MergeRecipe::from_json(&serde_json::to_string(&value).expect("re-encodes"))
            .expect("old sidecars parse");
        assert_eq!(old.alignment.deghost, None);
    }

    /// B-T5 (matrix) — every row is `Invalid`, never padded, clamped or
    /// reinterpreted; the boundary values 0.05-exclusive and 2.0-inclusive
    /// behave exactly as documented.
    #[test]
    fn deghost_validation_matrix() {
        fn with_deghost(recipe: &mut MergeRecipe, deghost: HdrDeghost) {
            recipe.alignment.deghost = Some(deghost);
        }
        fn good() -> HdrDeghost {
            HdrDeghost {
                threshold: DEGHOST_DEFAULT_THRESHOLD,
                reference_fallback: true,
            }
        }
        type Mutate = Box<dyn Fn(&mut MergeRecipe)>;
        let cases: Vec<(&str, Mutate)> = vec![
            (
                "threshold 0",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = 0.0;
                    with_deghost(r, d);
                }),
            ),
            (
                "threshold negative",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = -0.25;
                    with_deghost(r, d);
                }),
            ),
            (
                "threshold NaN",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = f64::NAN;
                    with_deghost(r, d);
                }),
            ),
            (
                "threshold +Inf",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = f64::INFINITY;
                    with_deghost(r, d);
                }),
            ),
            (
                "threshold at the excluded lower bound (0.05)",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = DEGHOST_MIN_THRESHOLD;
                    with_deghost(r, d);
                }),
            ),
            (
                "threshold below the range (0.04)",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = 0.04;
                    with_deghost(r, d);
                }),
            ),
            (
                "threshold above the range (2.1)",
                Box::new(|r| {
                    let mut d = good();
                    d.threshold = DEGHOST_MAX_THRESHOLD + 0.1;
                    with_deghost(r, d);
                }),
            ),
            (
                "reference_fallback false",
                Box::new(|r| {
                    let mut d = good();
                    d.reference_fallback = false;
                    with_deghost(r, d);
                }),
            ),
            (
                "deghost on panorama mode",
                Box::new(|r| {
                    r.mode = MergeMode::Panorama;
                    r.alignment.method = crate::MergeAlignmentMethod::PanoCylindricalHomography;
                    r.alignment.projection = crate::MergeProjection::Cylindrical;
                    with_deghost(r, good());
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
        // Boundary values: just above the minimum and exactly the maximum.
        for threshold in [DEGHOST_MIN_THRESHOLD + 1e-9, DEGHOST_MAX_THRESHOLD] {
            let mut recipe = sample_recipe();
            with_deghost(
                &mut recipe,
                HdrDeghost {
                    threshold,
                    reference_fallback: true,
                },
            );
            recipe.validate().expect("boundary threshold valid");
        }
    }

    /// The digest must move when the threshold moves — and an *absent*
    /// threshold must not move it at all (the two claims are one pair).
    #[test]
    fn deghost_is_digest_relevant_only_when_present() {
        let mut recipe = sample_recipe();
        // The claim is about the serialised digest input, so check the key
        // itself: a `None` deghost must not appear in the alignment JSON
        // (comparing two fresh fixtures against each other would check
        // nothing).
        let alignment_json = serde_json::to_value(&recipe.alignment).expect("alignment serialises");
        assert!(
            alignment_json.get("deghost").is_none(),
            "the fixture carries no deghost key, got: {alignment_json}"
        );
        recipe.alignment.deghost = Some(HdrDeghost {
            threshold: DEGHOST_DEFAULT_THRESHOLD,
            reference_fallback: true,
        });
        assert_ne!(recipe.digest(), sample_recipe().digest());
    }
}
