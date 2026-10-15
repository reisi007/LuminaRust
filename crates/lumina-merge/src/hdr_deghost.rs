//! Slice B (deghosting): the HDR ghost mask plus the HDR merge pass that
//! applies it.
//!
//! - [`HdrDeghostMode`] is the one frontend-facing mode enum (`off` |
//!   a threshold in `(0.05, 2.0]`); [`deghost_mask`] is the pure per-pixel
//!   ghost decision, [`merge_hdr_weighted_deghost`] is the HDR merge pass
//!   that applies it, and [`align_and_merge_hdr`] is the bundle-side pass
//!   (align → merge → recipe value).
//! - Extracted from `merge.rs` / `bundle.rs` (file-size ratchet); no
//!   behaviour lives here twice and the hat-weight mean itself is unchanged:
//!   with `deghost == None` the arithmetic is bit-identical to the pre-slice
//!   merge, which is what keeps existing bundles and committed goldens valid.
//!
//! # Documented limits (what the mask does NOT detect)
//!
//! Loud, because a mask that silently over- or under-blocks is worse than one
//! with named holes:
//!
//! - Motion **within** the threshold tolerance (slow objects, soft shadows).
//! - Motion that is **identical in every frame**: the reference frame (frame
//!   0, `inputs[0]`) carries the ghost itself, and no reference comparison can
//!   find that. Choosing the reference stays the caller's responsibility.
//! - Sub-pixel motion below the bilinear resampling (`sample_bilinear`): an
//!   interpolated mix pixel can stay under the threshold.
//! - Saturated pixels (hat weight 0, `hdr_hat_weight`): no frame contributes
//!   there, so deghosting changes nothing and the fallback rule behaves as
//!   before.
//! - The mask is a *relative* test, so it is scale-free by construction: a
//!   globally mis-exposed frame is the exposure resolution's job
//!   (`merge unsupported:`); the mask only reacts to *local* radiance
//!   disagreement.

use lumina_core::merge_geom::{clamp_linear, hdr_hat_weight, sample_bilinear};
use lumina_sidecar::{
    HdrDeghost, MergeExposure, MergeMode, MergeTransform, DEGHOST_MAX_THRESHOLD,
    DEGHOST_MIN_THRESHOLD,
};

use crate::bundle::{map_merge_error, MergePass, MergeRunError};
use crate::merge::relative_exposure;
use crate::{estimate_hdr_translation, LinearImage, MergeError};

/// Radiance floor of the relative deviation (`SOLL B.2`, suggestion `1e-3`):
/// the denominator of the deviation, so a black reference cannot divide by
/// zero. Documented, never guessed per pixel.
pub const DEGHOST_EPS: f64 = 1e-3;

/// Slice B: HDR deghosting mode (one enum, two spellings). `Off` is the
/// default on every frontend (no behaviour change); the only variant in 1.0
/// is a threshold in `(DEGHOST_MIN_THRESHOLD, DEGHOST_MAX_THRESHOLD]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum HdrDeghostMode {
    #[default]
    Off,
    Threshold(f64),
}

impl HdrDeghostMode {
    /// Parses the shared CLI spelling (`off` | `<threshold-float>`).
    /// Unknown values are a loud `Err` with the accepted values and the
    /// documented range — the CLI maps it onto its usage error, the GUI onto
    /// its own string. One message for both spellings of the field
    /// (`--deghost` flag / MCP `deghost` key).
    ///
    /// The range check is deliberately here *and* in `recipe.validate()`: a
    /// frontend refuses with its own class (CLI usage / MCP `InvalidParams`),
    /// and a persisted recipe is still refused loudly by the schema
    /// (`Invalid` → `merge failed:`). Neither clamps nor defaults.
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed == "off" {
            return Ok(HdrDeghostMode::Off);
        }
        let threshold: f64 = trimmed.parse().map_err(|_| {
            format!(
                "invalid `deghost` value `{value}`: expected `off` or a threshold in \
                 ({DEGHOST_MIN_THRESHOLD:.2}, {DEGHOST_MAX_THRESHOLD:.1}]"
            )
        })?;
        Self::threshold(threshold)
            .map_err(|reason| format!("invalid `deghost` value `{value}`: {reason}"))
    }

    /// The same rule for a frontend that already holds a number (the MCP wire
    /// form carries a JSON number, the CLI a text flag). Loud, never clamped.
    pub fn threshold(threshold: f64) -> Result<Self, String> {
        if !threshold.is_finite()
            || threshold <= DEGHOST_MIN_THRESHOLD
            || threshold > DEGHOST_MAX_THRESHOLD
        {
            return Err(format!(
                "threshold must be finite within ({DEGHOST_MIN_THRESHOLD:.2}, {DEGHOST_MAX_THRESHOLD:.1}], got {threshold} (never clamped)"
            ));
        }
        Ok(HdrDeghostMode::Threshold(threshold))
    }

    /// The persisted recipe value (`None` = off). The fallback policy is fixed
    /// in 1.0 but persisted explicitly — see [`HdrDeghost`].
    pub fn recipe_value(self) -> Option<HdrDeghost> {
        match self {
            HdrDeghostMode::Off => None,
            HdrDeghostMode::Threshold(threshold) => Some(HdrDeghost {
                threshold,
                reference_fallback: true,
            }),
        }
    }
}

/// Slice B: the HDR-only options gate (runs in `run_merge` before any decode
/// or file write, exactly like the `max_shift_px < 0` and the Slice-A gain
/// gates). Deghosting needs a bracket; a panorama set is stitched, not
/// bracketed, so asking for it there is a loud `merge failed:`.
pub(crate) fn check_deghost_mode(mode: MergeMode, deghost: HdrDeghostMode) -> Result<(), String> {
    if mode == MergeMode::Panorama && deghost != HdrDeghostMode::Off {
        return Err(
            "invalid --deghost: deghosting is HDR only (panorama frames are stitched, not bracketed)"
                .into(),
        );
    }
    Ok(())
}

/// The pure ghost decision for ONE output pixel (`SOLL B.2`, steps 2–3).
///
/// `radiances[i]` is frame `i`'s radiance **triple** at this pixel — already
/// shifted (via `shifts_px`) and normalised by its relative exposure — and
/// `radiances[0]` is the reference. A frame is a ghost here when the relative
/// deviation
///
/// ```text
/// |radiance_i[c] - radiance_0[c]| / max(radiance_0[c], DEGHOST_EPS)
/// ```
///
/// exceeds `threshold` in **at least one** channel `c`. Channels are not
/// averaged: a colour ghost (a red car in front of a grey wall) would
/// disappear in a luminance mean. The reference frame itself is never its own
/// ghost (its deviation is exactly 0), and an unmeasurable (non-finite)
/// deviation counts as a ghost — "cannot measure" is not "agrees".
///
/// Deterministic and total: no sort, no median, no randomness, one `f64`
/// comparison per channel.
#[must_use]
pub fn deghost_mask(radiances: &[[f64; 3]], threshold: f64) -> Vec<bool> {
    let reference = radiances.first().copied().unwrap_or([0.0; 3]);
    radiances
        .iter()
        .map(|radiance| {
            (0..3).any(|channel| {
                let deviation = (radiance[channel] - reference[channel]).abs()
                    / reference[channel].max(DEGHOST_EPS);
                !deviation.is_finite() || deviation > threshold
            })
        })
        .collect()
}

/// Weighted linear HDR merge of same-size frames, with the optional Slice-B
/// ghost mask.
///
/// With `deghost == None` this is the pre-slice merge, bit for bit: the same
/// per-frame sampling, the same `num/den` accumulation order, and the same
/// "all weights 0 → mean radiance" fallback.
///
/// With `deghost == Some(..)` a masked frame contributes hat weight 0 at that
/// pixel and is **also** kept out of the mean fallback. When every other frame
/// is masked, the weighted mean over the surviving reference frame is the
/// **reference frame's radiance** (`radiance_0`) — not the mean. That is the
/// whole point: the mean over ghosts is exactly the error the mask exists to
/// remove, and the reference is the only defined truth (`inputs[0]`,
/// "first = reference"). A fully clipped pixel keeps the documented mean
/// fallback over the frames still in play.
pub fn merge_hdr_weighted_deghost(
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    shifts_px: &[(f64, f64)],
    deghost: Option<&HdrDeghost>,
) -> Result<LinearImage, MergeError> {
    if frames.len() < 2 {
        return Err(MergeError::Invalid(format!(
            "hdr merge needs at least 2 frames, got {}",
            frames.len()
        )));
    }
    if frames.len() != exposures.len() || frames.len() != shifts_px.len() {
        return Err(MergeError::Invalid(format!(
            "hdr merge needs one exposure and shift per frame, got {} frames, {} exposures, {} shifts",
            frames.len(),
            exposures.len(),
            shifts_px.len()
        )));
    }
    let (w, h) = (frames[0].width(), frames[0].height());
    for (i, frame) in frames.iter().enumerate() {
        if frame.width() != w || frame.height() != h {
            return Err(MergeError::Invalid(format!(
                "hdr merge frame #{i} is {}x{}, expected {w}x{h} (no silent rescale)",
                frame.width(),
                frame.height()
            )));
        }
    }
    let rel: Vec<f64> = exposures
        .iter()
        .map(relative_exposure)
        .collect::<Result<_, _>>()?;
    // Validate shifts once, before touching pixels (fail fast, same contract).
    for (i, (dx, dy)) in shifts_px.iter().enumerate() {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(MergeError::Invalid(format!(
                "hdr shift #{i} must be finite, got ({dx}, {dy})"
            )));
        }
    }
    // Extract every channel plane exactly once, before the pixel loop
    // (O(frames * pixels)); the same pattern as `blend_panorama`. The
    // previous per-pixel `frame.channel_plane(c)` re-extracted a whole plane
    // for every pixel/channel/frame, so the merge scaled quadratically in the
    // pixel count (C1: 512x512x2 was ~54 s release, effectively unusable).
    let planes: Vec<[Vec<f32>; 3]> = frames
        .iter()
        .map(|frame| {
            [
                frame.channel_plane(0),
                frame.channel_plane(1),
                frame.channel_plane(2),
            ]
        })
        .collect();
    let mut out = vec![0.0f32; w as usize * h as usize * 3];
    // Reused per-pixel scratch, allocated once (no per-pixel allocation): one
    // resampled triple per frame, plus the radiance triple `deghost_mask`
    // wants. `None` deghosting keeps the exact pre-slice arithmetic.
    let mut pixels: Vec<[f64; 3]> = vec![[0.0f64; 3]; planes.len()];
    let mut radiances: Vec<[f64; 3]> = vec![[0.0f64; 3]; planes.len()];
    for y in 0..h {
        for x in 0..w {
            for (i, frame_planes) in planes.iter().enumerate() {
                let (dx, dy) = shifts_px[i];
                for (c, plane) in frame_planes.iter().enumerate() {
                    let pixel = sample_bilinear(plane, w, h, x as f64 - dx, y as f64 - dy) as f64;
                    pixels[i][c] = pixel;
                    radiances[i][c] = pixel / rel[i];
                }
            }
            let mask = deghost.map(|settings| deghost_mask(&radiances, settings.threshold));
            for c in 0..3 {
                let mut num = 0.0f64;
                let mut den = 0.0f64;
                let mut rad_sum = 0.0f64;
                let mut contributing = 0usize;
                for (i, _) in planes.iter().enumerate() {
                    if mask.as_ref().is_some_and(|mask| mask[i]) {
                        // Ghost at this pixel: hat weight 0, and it must not
                        // leak into the mean fallback either.
                        continue;
                    }
                    let radiance = radiances[i][c];
                    let weight = hdr_hat_weight(pixels[i][c] as f32) as f64;
                    num += weight * radiance;
                    den += weight;
                    rad_sum += radiance;
                    contributing += 1;
                }
                let value = if den > 0.0 {
                    num / den
                } else if contributing > 0 {
                    // All contributing hat weights are 0 (clipped inputs): the
                    // mean radiance over the frames still in play, as before —
                    // never NaN. When the mask left only the reference frame,
                    // that mean IS the reference radiance, which is the
                    // documented fallback for a fully masked pixel.
                    rad_sum / contributing as f64
                } else {
                    // Defensive only: the reference frame is never its own
                    // ghost, so a well-formed mask always leaves it standing.
                    radiances[0][c]
                };
                out[(y * w * 3 + x * 3) as usize + c] = clamp_linear(value as f32);
            }
        }
    }
    LinearImage::new(w, h, out).map_err(|e| MergeError::Invalid(e.to_string()))
}

/// The HDR merge pass: align (translation-only shifts), then merge — with the
/// Slice-B mask when a threshold is set, exactly as before otherwise. Moved
/// from `bundle.rs` (`align_and_merge`, HDR arm) unchanged apart from the
/// deghosting parameter.
pub(crate) fn align_and_merge_hdr(
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    max_shift_px: i32,
    deghost: HdrDeghostMode,
) -> Result<MergePass, MergeRunError> {
    let mut shifts = Vec::with_capacity(frames.len());
    let mut transforms = Vec::new();
    let mut residual_px: f64 = 0.0;
    let mut residual_warning = false;
    shifts.push((0.0, 0.0));
    for (index, frame) in frames.iter().enumerate().skip(1) {
        let shift =
            estimate_hdr_translation(&frames[0], frame, max_shift_px).map_err(map_merge_error)?;
        shifts.push((shift.dx, shift.dy));
        transforms.push(MergeTransform {
            source_index: index,
            matrix_3x3: [1.0, 0.0, shift.dx, 0.0, 1.0, shift.dy, 0.0, 0.0, 1.0],
        });
        residual_px = residual_px.max(shift.residual_px);
        residual_warning |= shift.status.residual_flag();
    }
    let merged =
        merge_hdr_weighted_deghost(frames, exposures, &shifts, deghost.recipe_value().as_ref())
            .map_err(map_merge_error)?;
    Ok(MergePass {
        merged,
        transforms,
        residual_px,
        residual_warning,
        gain_compensation: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exposure(time: f64) -> MergeExposure {
        MergeExposure {
            exposure_time_s: time,
            iso: 100,
            f_number: 8.0,
        }
    }

    /// The B-T1 geometry: a 16×16 frame.
    const W: u32 = 16;

    fn solid(value: f32) -> LinearImage {
        LinearImage::solid(W, W, [value, value, value])
    }

    /// Paints a `size × size` block of `value` at `(x0, y0)` (the moved
    /// object of the B-T1 fixture).
    fn paint_block(frame: &mut LinearImage, x0: u32, y0: u32, size: u32, value: f32) {
        let mut pixels = frame.pixels().to_vec();
        for y in y0..y0 + size {
            for x in x0..x0 + size {
                let index = ((y * W + x) * 3) as usize;
                pixels[index..index + 3].copy_from_slice(&[value, value, value]);
            }
        }
        *frame = LinearImage::new(W, W, pixels).unwrap();
    }

    /// The B-T1 bracket: three frames of the same radiance 16.0
    /// (`0.01/0.0125/0.015 s` × rel ratios 1/1.25/1.5), the middle one
    /// carrying a 6×6 bright block (radiance 35.2 there).
    fn b_t1_frames() -> (Vec<LinearImage>, Vec<MergeExposure>) {
        let mut ghost = solid(0.3125);
        paint_block(&mut ghost, 4, 4, 6, 0.6875);
        (
            vec![solid(0.25), ghost, solid(0.375)],
            vec![exposure(0.01), exposure(0.0125), exposure(0.015)],
        )
    }

    fn deghost(threshold: f64) -> HdrDeghost {
        HdrDeghost {
            threshold,
            reference_fallback: true,
        }
    }

    /// B-T1 — a synthetic ghost in exactly one frame: the block pixels fall
    /// back to the reference frame's radiance (±1e-6), every other pixel keeps
    /// the hat-weight mean of the agreeing frames. A stub without a mask
    /// averages the ghost in (hat mean 22.4) and fails.
    #[test]
    fn b_t1_synthetic_ghost_falls_back_to_the_reference() {
        let (frames, exposures) = b_t1_frames();
        let shifts = [(0.0, 0.0); 3];
        let out =
            merge_hdr_weighted_deghost(&frames, &exposures, &shifts, Some(&deghost(0.25))).unwrap();
        // Without the mask the block lands on the ghost mean.
        let plain = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).unwrap();
        let block = out.pixels()[((4 * W + 4) * 3) as usize];
        let plain_block = plain.pixels()[((4 * W + 4) * 3) as usize];
        assert!(
            (f64::from(block) - 16.0).abs() < 1e-6,
            "ghost pixel must be the reference radiance 16.0, got {block}"
        );
        assert!(
            (f64::from(plain_block) - 16.0).abs() > 1e-3,
            "the unmasked merge must average the ghost in, got {plain_block}"
        );
        // Every other pixel keeps the mean of the agreeing frames.
        for (index, pixel) in out.pixels().chunks(3).enumerate() {
            let (x, y) = ((index % W as usize) as u32, (index / W as usize) as u32);
            if (4..10).contains(&x) && (4..10).contains(&y) {
                continue;
            }
            assert!(
                (f64::from(pixel[0]) - 16.0).abs() < 1e-4,
                "clean pixel ({x},{y}) must stay at 16.0, got {}",
                pixel[0]
            );
        }
    }

    /// B-T2 — threshold sensitivity: the same 10 % deviation is left alone at
    /// 0.25 and caught at 0.06. Two thresholds, two results (a stub with a
    /// fixed threshold cannot pass both).
    #[test]
    fn b_t2_threshold_sensitivity() {
        let frames = vec![solid(0.25), solid(0.275)]; // exactly 10 % apart
        let exposures = vec![exposure(0.01), exposure(0.01)];
        let shifts = [(0.0, 0.0); 2];
        let untouched = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).unwrap();
        let wide =
            merge_hdr_weighted_deghost(&frames, &exposures, &shifts, Some(&deghost(0.25))).unwrap();
        assert_eq!(
            untouched.pixels(),
            wide.pixels(),
            "a 10 % deviation is below 0.25: the merge must be untouched"
        );
        let tight =
            merge_hdr_weighted_deghost(&frames, &exposures, &shifts, Some(&deghost(0.06))).unwrap();
        assert_ne!(
            tight.pixels(),
            wide.pixels(),
            "a 10 % deviation is above 0.06: the mask must intervene"
        );
    }

    /// B-T3 — the all-masked fallback: when every non-reference frame is a
    /// ghost, the pixel equals the reference radiance, not the mean. A stub
    /// that keeps the mean over ghosts fails here.
    #[test]
    fn b_t3_all_masked_falls_back_to_the_reference() {
        let frames = vec![solid(0.25), solid(0.9)];
        let exposures = vec![exposure(0.01), exposure(0.01)];
        let shifts = [(0.0, 0.0); 2];
        let out =
            merge_hdr_weighted_deghost(&frames, &exposures, &shifts, Some(&deghost(0.25))).unwrap();
        let plain = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).unwrap();
        for pixel in out.pixels().chunks(3) {
            assert!(
                (f64::from(pixel[0]) - 16.0).abs() < 1e-6,
                "all-masked pixel must be the reference radiance, got {}",
                pixel[0]
            );
        }
        assert!(
            (f64::from(plain.pixels()[0]) - 16.0).abs() > 1e-3,
            "the plain merge would average the ghost in, got {}",
            plain.pixels()[0]
        );
    }

    /// B-T4 — negative documentation: with the maximum threshold 2.0 even the
    /// B-T1 ghost stays signal, so the merge is bit-identical to the plain one
    /// (no over-blocking).
    #[test]
    fn b_t4_below_threshold_is_bit_identical() {
        let (frames, exposures) = b_t1_frames();
        let shifts = [(0.0, 0.0); 3];
        let plain = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).unwrap();
        let tolerant =
            merge_hdr_weighted_deghost(&frames, &exposures, &shifts, Some(&deghost(2.0))).unwrap();
        assert_eq!(
            plain.pixels(),
            tolerant.pixels(),
            "a deviation below the threshold must not change a single byte"
        );
    }

    /// The pure mask contract: the reference is never its own ghost, one loud
    /// channel is enough, and an unmeasurable deviation counts as a ghost.
    #[test]
    fn deghost_mask_contract() {
        assert!(!deghost_mask(&[[0.5, 0.5, 0.5]], 0.05)[0]);
        assert!(
            deghost_mask(&[[0.5, 0.5, 0.5], [0.5, 0.5, 0.5]], 0.05)
                .iter()
                .all(|ghost| !ghost),
            "identical frames are never ghosts"
        );
        assert!(
            deghost_mask(&[[0.5, 0.5, 0.5], [0.5, 0.5, 0.7]], 0.05)[1],
            "one channel is enough (no luminance averaging)"
        );
        assert!(
            !deghost_mask(&[[0.5, 0.5, 0.5], [0.5, 0.5, 0.7]], 0.5)[1],
            "and the threshold decides which"
        );
        assert!(
            deghost_mask(&[[0.5, 0.5, 0.5], [f64::NAN, 0.5, 0.5]], 2.0)[1],
            "an unmeasurable deviation is a ghost"
        );
        // A black reference is floored by DEGHOST_EPS: a bright frame is a
        // ghost there, a black one is not.
        assert!(deghost_mask(&[[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]], 0.25)[1]);
        assert!(!deghost_mask(&[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], 0.25)[1]);
    }

    /// The frontend spelling: `off` and every in-range number parse, the range
    /// is refused loudly at the boundary (never clamped), and the options gate
    /// is HDR only.
    #[test]
    fn deghost_mode_parses_and_refuses_out_of_range() {
        assert_eq!(HdrDeghostMode::parse("off"), Ok(HdrDeghostMode::Off));
        assert_eq!(
            HdrDeghostMode::parse(" 0.25 "),
            Ok(HdrDeghostMode::Threshold(0.25))
        );
        assert_eq!(
            HdrDeghostMode::parse("2.0"),
            Ok(HdrDeghostMode::Threshold(2.0)),
            "the upper bound is inclusive"
        );
        for value in ["0.05", "2.1", "0", "-1", "abc", ""] {
            assert!(
                HdrDeghostMode::parse(value).is_err(),
                "`{value}` must be refused loudly"
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.04, 2.1] {
            assert!(
                HdrDeghostMode::threshold(value).is_err(),
                "{value} must be refused, never defaulted or clamped"
            );
        }
        assert_eq!(HdrDeghostMode::Off.recipe_value(), None);
        assert_eq!(
            HdrDeghostMode::Threshold(0.25).recipe_value(),
            Some(HdrDeghost {
                threshold: 0.25,
                reference_fallback: true
            })
        );
        assert_eq!(
            check_deghost_mode(MergeMode::Hdr, HdrDeghostMode::Threshold(0.25)),
            Ok(())
        );
        assert!(
            check_deghost_mode(MergeMode::Panorama, HdrDeghostMode::Threshold(0.25)).is_err(),
            "panorama is stitched, not bracketed"
        );
        assert_eq!(
            check_deghost_mode(MergeMode::Panorama, HdrDeghostMode::Off),
            Ok(())
        );
    }

    /// Deterministic across runs. The pre-slice identity is proven bit-for-
    /// bit in `tests/hdr_deghost_plain_identity.rs` (verbatim reference).
    #[test]
    fn none_path_is_deterministic_across_runs() {
        let (frames, exposures) = b_t1_frames();
        let shifts = [(0.0, 0.0); 3];
        let a = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).unwrap();
        let b = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).unwrap();
        assert_eq!(a.pixels(), b.pixels());
    }
}
