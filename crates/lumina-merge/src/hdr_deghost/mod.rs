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

mod align;
mod mask;
mod merge;
mod mode;

pub(crate) use align::align_and_merge_hdr;
pub use mask::{deghost_mask, DEGHOST_EPS};
pub use merge::merge_hdr_weighted_deghost;
pub(crate) use mode::check_deghost_mode;
pub use mode::HdrDeghostMode;

#[cfg(test)]
mod tests {
    use super::{check_deghost_mode, deghost_mask, merge_hdr_weighted_deghost, HdrDeghostMode};
    use crate::LinearImage;
    use lumina_sidecar::{HdrDeghost, MergeExposure, MergeMode};

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
