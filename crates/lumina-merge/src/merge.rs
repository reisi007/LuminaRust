//! Merge: weighted linear HDR + feather-blended panorama.
//!
//! - HDR: per-pixel radiance mean weighted by the hat function over EXIF
//!   derived relative exposure (`exposure_time_s * iso / f_number^2`).
//!   Output is linear scene radiance (may exceed 1, never clipped above;
//!   negatives from float error are clamped to 0). Monotone in every input.
//! - Panorama: side-by-side placement by integer-rounded translation with
//!   feather blend across `blend_width_px` in the overlap (documented
//!   width; 0 = hard seam). Convex combination, hence range-preserving.
//! - Missing EXIF exposure (non-finite or non-positive `exposure_time_s`,
//!   `iso == 0`, non-finite or non-positive `f_number`) is `Unsupported`,
//!   never estimated from pixels.

use crate::{LinearImage, MergeError};
use lumina_core::merge_geom::{clamp_linear, feather_weight, hdr_hat_weight, sample_bilinear};
use lumina_sidecar::MergeExposure;

/// Relative linear exposure of one frame from EXIF only:
/// `t * iso / f^2`. Rejects non-finite/non-positive components loudly so
/// the caller maps the failure to `Unsupported` (never pixel-guessed).
pub fn relative_exposure(exposure: &MergeExposure) -> Result<f64, MergeError> {
    if !exposure.exposure_time_s.is_finite() || exposure.exposure_time_s <= 0.0 {
        return Err(MergeError::Unsupported(format!(
            "hdr merge needs EXIF exposure_time_s > 0, got {}",
            exposure.exposure_time_s
        )));
    }
    if exposure.iso == 0 {
        return Err(MergeError::Unsupported(
            "hdr merge needs EXIF iso > 0".into(),
        ));
    }
    if !exposure.f_number.is_finite() || exposure.f_number <= 0.0 {
        return Err(MergeError::Unsupported(format!(
            "hdr merge needs EXIF f_number > 0, got {}",
            exposure.f_number
        )));
    }
    Ok(exposure.exposure_time_s * exposure.iso as f64 / (exposure.f_number * exposure.f_number))
}

/// Weighted linear HDR merge of same-size frames.
///
/// `frames.len() == exposures.len()`, at least 2; dimensions must match
/// (`Invalid` otherwise, never silently rescaled). Frames are first shifted
/// by `shifts_px` (translation-only, HDR scope) via bilinear resampling,
/// then merged: `radiance_i = pixel / rel_i`,
/// `out = sum(hat(pixel) * radiance) / sum(hat(pixel))`.
/// Fully clipped pixels (all weights 0) fall back to the mean radiance
/// (documented, never NaN).
pub fn merge_hdr_weighted(
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    shifts_px: &[(f64, f64)],
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
    let mut out = vec![0.0f32; w as usize * h as usize * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut num = 0.0f64;
                let mut den = 0.0f64;
                let mut rad_sum = 0.0f64;
                for (i, frame) in frames.iter().enumerate() {
                    let plane = frame.channel_plane(c);
                    let (dx, dy) = shifts_px[i];
                    if !dx.is_finite() || !dy.is_finite() {
                        return Err(MergeError::Invalid(format!(
                            "hdr shift #{i} must be finite, got ({dx}, {dy})"
                        )));
                    }
                    let pixel = sample_bilinear(&plane, w, h, x as f64 - dx, y as f64 - dy) as f64;
                    let weight = hdr_hat_weight(pixel as f32) as f64;
                    let radiance = pixel / rel[i];
                    num += weight * radiance;
                    den += weight;
                    rad_sum += radiance;
                }
                let radiance = if den > 0.0 {
                    num / den
                } else {
                    // All inputs clipped: mean radiance instead of NaN.
                    rad_sum / frames.len() as f64
                };
                out[(y * w * 3 + x * 3) as usize + c] = clamp_linear(radiance as f32);
            }
        }
    }
    LinearImage::new(w, h, out).map_err(|e| MergeError::Invalid(e.to_string()))
}

/// Feather-blended panorama of same-size frames placed side by side.
///
/// `offsets_px` holds the integer-rounded translation of each frame into
/// the canvas (frame 0 conventionally at `(0, 0)`). The canvas spans the
/// union bounding box. In overlaps, frames blend left-to-right with the
/// [`feather_weight`] ramp of `blend_width_px` (0 = hard seam, later
/// frame wins past the centre). Non-overlapping consecutive frames are
/// `Unsupported` (no silent partial panorama). Output is a convex
/// combination of inputs, hence range-preserving in `[0, 1]` up to float
/// error (negatives clamped to 0).
pub fn blend_panorama(
    frames: &[LinearImage],
    offsets_px: &[(i32, i32)],
    blend_width_px: u32,
) -> Result<LinearImage, MergeError> {
    if frames.len() < 2 {
        return Err(MergeError::Invalid(format!(
            "panorama blend needs at least 2 frames, got {}",
            frames.len()
        )));
    }
    if frames.len() != offsets_px.len() {
        return Err(MergeError::Invalid(format!(
            "panorama blend needs one offset per frame, got {} frames, {} offsets",
            frames.len(),
            offsets_px.len()
        )));
    }
    let (w, h) = (frames[0].width(), frames[0].height());
    for (i, frame) in frames.iter().enumerate() {
        if frame.width() != w || frame.height() != h {
            return Err(MergeError::Invalid(format!(
                "panorama frame #{i} is {}x{}, expected {w}x{h} (no silent rescale)",
                frame.width(),
                frame.height()
            )));
        }
    }
    // Canvas: union bounding box over integer offsets.
    let min_x = offsets_px.iter().map(|o| o.0).min().unwrap_or(0);
    let min_y = offsets_px.iter().map(|o| o.1).min().unwrap_or(0);
    let max_x = offsets_px
        .iter()
        .map(|o| o.0 + w as i32)
        .max()
        .unwrap_or(w as i32);
    let max_y = offsets_px
        .iter()
        .map(|o| o.1 + h as i32)
        .max()
        .unwrap_or(h as i32);
    let cw = (max_x - min_x) as u32;
    let ch = (max_y - min_y) as u32;
    // Loud non-overlap gate between consecutive frames in x.
    let mut sorted = offsets_px.to_vec();
    sorted.sort_unstable();
    for pair in sorted.windows(2) {
        if pair[1].0 - pair[0].0 >= w as i32 {
            return Err(MergeError::Unsupported(format!(
                "panorama frames at x={} and x={} do not overlap (width {w}px)",
                pair[0].0, pair[1].0
            )));
        }
    }
    let planes: Vec<Vec<Vec<f32>>> = frames
        .iter()
        .map(|f| (0..3).map(|c| f.channel_plane(c)).collect())
        .collect();
    // Right edge of the union of frames before `i` (chain order): the
    // overlap of frame `i` with the accumulated composite starts at its
    // own left edge and ends at this edge.
    let mut prefix_right = Vec::with_capacity(frames.len());
    let mut running = i32::MIN;
    for (ox, _) in offsets_px.iter() {
        prefix_right.push(running);
        running = running.max(ox + w as i32);
    }
    let mut out = vec![0.0f32; cw as usize * ch as usize * 3];
    for y in 0..ch {
        for x in 0..cw {
            let gx = x as i32 + min_x;
            let gy = y as i32 + min_y;
            // Covering frames in order; blend weights accumulate left to
            // right: acc holds the running composite, later frames blend
            // over it with the feather ramp of their left overlap.
            let mut covering: Vec<usize> = Vec::new();
            for (i, (ox, oy)) in offsets_px.iter().enumerate() {
                let lx = gx - ox;
                let ly = gy - oy;
                if lx >= 0 && ly >= 0 && lx < w as i32 && ly < h as i32 {
                    covering.push(i);
                }
            }
            if covering.is_empty() {
                continue; // Outside every frame: stays 0 (canvas gap).
            }
            for c in 0..3 {
                let mut acc = sample_bilinear(
                    &planes[covering[0]][c],
                    w,
                    h,
                    (gx - offsets_px[covering[0]].0) as f64,
                    (gy - offsets_px[covering[0]].1) as f64,
                );
                for &idx in &covering[1..] {
                    let ox = offsets_px[idx].0;
                    let overlap_start = ox;
                    let overlap_end = prefix_right[idx].min(ox + w as i32);
                    let overlap = (overlap_end - overlap_start).max(0) as u32;
                    let pos = (gx - overlap_start).max(0) as u32;
                    let t = feather_weight(pos, overlap, blend_width_px);
                    let sample = sample_bilinear(
                        &planes[idx][c],
                        w,
                        h,
                        (gx - ox) as f64,
                        (gy - offsets_px[idx].1) as f64,
                    );
                    acc = acc * (1.0 - t) + sample * t;
                }
                out[(y * cw * 3 + x * 3) as usize + c] = clamp_linear(acc);
            }
        }
    }
    LinearImage::new(cw, ch, out).map_err(|e| MergeError::Invalid(e.to_string()))
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

    #[test]
    fn hdr_merge_recovers_shared_radiance() {
        // Two frames of the same radiance 2.0 under 1x/2x exposure.
        // Frame pixels: 0.5 (hat weight 1) and 1.0 clipped... use 0.25/0.5:
        // radiance 0.25/rel(0.01) == 0.5/rel(0.02) == 25/rel-scale.
        let a = LinearImage::solid(4, 4, [0.25, 0.25, 0.25]);
        let b = LinearImage::solid(4, 4, [0.5, 0.5, 0.5]);
        let out = merge_hdr_weighted(&[a, b], &[exposure(0.01), exposure(0.02)], &[(0.0, 0.0); 2])
            .unwrap();
        // rel_a = 0.01*100/64, rel_b = 2*rel_a; radiance both = 0.25/rel_a.
        let expected = 0.25 / (0.01 * 100.0 / 64.0);
        for v in out.pixels() {
            assert!(
                ((*v as f64) - expected).abs() < 1e-4,
                "got {v}, want {expected}"
            );
        }
    }

    #[test]
    fn hdr_merge_output_monotone_in_each_input() {
        let base = LinearImage::solid(4, 4, [0.3, 0.3, 0.3]);
        let brighter = LinearImage::solid(4, 4, [0.4, 0.3, 0.3]);
        let ex = [exposure(0.01), exposure(0.02)];
        let low = merge_hdr_weighted(
            &[base.clone(), LinearImage::solid(4, 4, [0.5, 0.5, 0.5])],
            &ex,
            &[(0.0, 0.0); 2],
        )
        .unwrap();
        let high = merge_hdr_weighted(
            &[brighter, LinearImage::solid(4, 4, [0.5, 0.5, 0.5])],
            &ex,
            &[(0.0, 0.0); 2],
        )
        .unwrap();
        for (l, h) in low.pixels().iter().zip(high.pixels()) {
            assert!(h >= l, "monotone: {h} >= {l}");
        }
    }

    #[test]
    fn hdr_merge_clipped_fallback_never_nan() {
        // Both frames fully white: weights 0 -> mean-radiance fallback.
        let a = LinearImage::solid(2, 2, [1.0, 1.0, 1.0]);
        let b = LinearImage::solid(2, 2, [1.0, 1.0, 1.0]);
        let out = merge_hdr_weighted(&[a, b], &[exposure(0.01), exposure(0.02)], &[(0.0, 0.0); 2])
            .unwrap();
        for v in out.pixels() {
            assert!(v.is_finite() && *v >= 0.0);
        }
    }

    #[test]
    fn hdr_missing_exif_is_unsupported() {
        let a = LinearImage::solid(2, 2, [0.3, 0.3, 0.3]);
        let b = LinearImage::solid(2, 2, [0.5, 0.5, 0.5]);
        for bad in [
            MergeExposure {
                exposure_time_s: f64::NAN,
                iso: 100,
                f_number: 8.0,
            },
            MergeExposure {
                exposure_time_s: 0.0,
                iso: 100,
                f_number: 8.0,
            },
            MergeExposure {
                exposure_time_s: 0.01,
                iso: 0,
                f_number: 8.0,
            },
            MergeExposure {
                exposure_time_s: 0.01,
                iso: 100,
                f_number: f64::INFINITY,
            },
        ] {
            let err = merge_hdr_weighted(
                &[a.clone(), b.clone()],
                &[exposure(0.01), bad],
                &[(0.0, 0.0); 2],
            )
            .unwrap_err();
            assert!(
                matches!(err, MergeError::Unsupported(_)),
                "missing EXIF must be unsupported, got: {err}"
            );
        }
    }

    #[test]
    fn hdr_rejects_mismatched_shapes_loudly() {
        let a = LinearImage::solid(4, 4, [0.3, 0.3, 0.3]);
        let b = LinearImage::solid(2, 2, [0.3, 0.3, 0.3]);
        let err = merge_hdr_weighted(&[a, b], &[exposure(0.01), exposure(0.02)], &[(0.0, 0.0); 2])
            .unwrap_err();
        assert!(matches!(err, MergeError::Invalid(_)));
        let a = LinearImage::solid(2, 2, [0.3, 0.3, 0.3]);
        let err = merge_hdr_weighted(&[a], &[exposure(0.01)], &[(0.0, 0.0)]).unwrap_err();
        assert!(matches!(err, MergeError::Invalid(_)));
    }

    #[test]
    fn panorama_blend_range_and_seam() {
        // 8px frames, offset 4: canvas 12 wide; outside frames must be exact.
        let a = LinearImage::solid(8, 4, [0.2, 0.2, 0.2]);
        let b = LinearImage::solid(8, 4, [0.8, 0.8, 0.8]);
        let out = blend_panorama(&[a, b], &[(0, 0), (4, 0)], 4).unwrap();
        assert_eq!((out.width(), out.height()), (12, 4));
        for v in out.pixels() {
            assert!((0.0..=1.0).contains(v), "range-preserving, got {v}");
        }
        // Far left is pure A, far right pure B.
        assert!((out.pixels()[0] - 0.2).abs() < 1e-6);
        let last = out.pixels()[out.pixels().len() - 3];
        assert!((last - 0.8).abs() < 1e-6);
        // Middle of the 4px overlap ramps between the two.
        let mid = out.pixels()[6 * 3];
        assert!(mid > 0.2 && mid < 0.8, "feathered seam, got {mid}");
    }

    #[test]
    fn panorama_hard_seam_with_zero_blend_width() {
        let a = LinearImage::solid(8, 2, [0.2, 0.2, 0.2]);
        let b = LinearImage::solid(8, 2, [0.8, 0.8, 0.8]);
        let out = blend_panorama(&[a, b], &[(0, 0), (4, 0)], 0).unwrap();
        // Overlap is 4px (x=4..8): hard seam at centre -> x<6 pure A.
        assert!((out.pixels()[4 * 3] - 0.2).abs() < 1e-6);
        assert!((out.pixels()[7 * 3] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn panorama_non_overlapping_is_unsupported() {
        let a = LinearImage::solid(8, 4, [0.2, 0.2, 0.2]);
        let b = LinearImage::solid(8, 4, [0.8, 0.8, 0.8]);
        let err = blend_panorama(&[a, b], &[(0, 0), (8, 0)], 4).unwrap_err();
        assert!(
            matches!(err, MergeError::Unsupported(_)),
            "gap must be unsupported, got: {err}"
        );
    }
}
