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
use lumina_core::merge_geom::{
    apply_matrix_3x3, clamp_linear, feather_weight, hdr_hat_weight, is_translation_rotation_light,
    sample_bilinear,
};
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
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut num = 0.0f64;
                let mut den = 0.0f64;
                let mut rad_sum = 0.0f64;
                for (i, frame_planes) in planes.iter().enumerate() {
                    let (dx, dy) = shifts_px[i];
                    let pixel =
                        sample_bilinear(&frame_planes[c], w, h, x as f64 - dx, y as f64 - dy)
                            as f64;
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
/// `Unsupported` (no silent partial panorama), checked in **both** axes
/// (x *and* y). Output is a convex combination of inputs, hence
/// range-preserving in `[0, 1]` up to float error (negatives clamped to 0).
///
/// This path is translation-only (integer offsets, used by the CLI wiring);
/// rotation must go through [`blend_panorama_transformed`], which applies the
/// full 3x3 matrix about the frame centre (C3).
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
    // Loud non-overlap gate between consecutive frames in x *and* y: after
    // sorting by an axis, consecutive offsets must be closer than the frame
    // extent on that axis, else the union is disconnected and the result
    // would be a silent partial panorama. The previous gate checked x only,
    // so vertically separated frames produced a gap-filled canvas (C4).
    let mut sorted_x = offsets_px.to_vec();
    sorted_x.sort_unstable_by_key(|o| o.0);
    for pair in sorted_x.windows(2) {
        if pair[1].0 - pair[0].0 >= w as i32 {
            return Err(MergeError::Unsupported(format!(
                "panorama frames at x={} and x={} do not overlap (width {w}px)",
                pair[0].0, pair[1].0
            )));
        }
    }
    let mut sorted_y = offsets_px.to_vec();
    sorted_y.sort_unstable_by_key(|o| o.1);
    for pair in sorted_y.windows(2) {
        if pair[1].1 - pair[0].1 >= h as i32 {
            return Err(MergeError::Unsupported(format!(
                "panorama frames at y={} and y={} do not overlap (height {h}px)",
                pair[0].1, pair[1].1
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

/// Transformation-aware feather-blended panorama (C3).
///
/// `matrices[i]` is a row-major 3x3 transform mapping frame `i`'s pixel
/// coordinates into canvas coordinates (same convention as
/// [`crate::align::PanoTransform::matrix_3x3`]: translation + rotation about
/// the frame centre). Unlike [`blend_panorama`], which only honours integer
/// `offsets_px` and therefore silently dropped rotation, this path samples
/// each frame through its full inverse matrix, so a rotated frame is placed
/// rotated.
///
/// Canvas and weights: the canvas is the union of the transformed frame
/// bounding boxes; a canvas pixel is covered by frame `i` when its inverse
/// transform lands inside the frame. Covering frames are combined with
/// normalised feather weights ramped from each frame's border by
/// `blend_width_px` (0 = unweighted average over the covering frames); a
/// convex combination, hence range-preserving. Pixels covered by no frame
/// stay 0 (canvas gap). Consecutive frames whose transformed boxes do not
/// overlap in both axes are [`MergeError::Unsupported`] (no silent partial
/// panorama).
pub fn blend_panorama_transformed(
    frames: &[LinearImage],
    matrices: &[[f64; 9]],
    blend_width_px: u32,
) -> Result<LinearImage, MergeError> {
    if frames.len() < 2 {
        return Err(MergeError::Invalid(format!(
            "panorama blend needs at least 2 frames, got {}",
            frames.len()
        )));
    }
    if frames.len() != matrices.len() {
        return Err(MergeError::Invalid(format!(
            "panorama blend needs one matrix per frame, got {} frames, {} matrices",
            frames.len(),
            matrices.len()
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
    // Scope + invertibility: only translation+rotation-light is 1.5 scope
    // (loud `Unsupported`, never silently approximated).
    let mut inverses = Vec::with_capacity(matrices.len());
    for (i, matrix) in matrices.iter().enumerate() {
        if !is_translation_rotation_light(matrix, 1e-6) {
            return Err(MergeError::Unsupported(format!(
                "panorama transform #{i} is not translation+rotation-light (1.5 scope)"
            )));
        }
        let inverse = invert_matrix_3x3(matrix).ok_or_else(|| {
            MergeError::Invalid(format!("panorama transform #{i} is not invertible"))
        })?;
        inverses.push(inverse);
    }
    // Transformed frame bounding boxes in canvas space.
    let mut bounds: Vec<(f64, f64, f64, f64)> = Vec::with_capacity(matrices.len());
    for (i, matrix) in matrices.iter().enumerate() {
        let corners = [
            (0.0, 0.0),
            (w as f64, 0.0),
            (0.0, h as f64),
            (w as f64, h as f64),
        ];
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for (x, y) in corners {
            let (ox, oy) = apply_matrix_3x3(matrix, x, y).ok_or_else(|| {
                MergeError::Invalid(format!("panorama transform #{i} maps no finite corner"))
            })?;
            min_x = min_x.min(ox);
            min_y = min_y.min(oy);
            max_x = max_x.max(ox);
            max_y = max_y.max(oy);
        }
        bounds.push((min_x.floor(), min_y.floor(), max_x.ceil(), max_y.ceil()));
    }
    let canvas_min_x = bounds.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
    let canvas_min_y = bounds.iter().map(|b| b.1).fold(f64::INFINITY, f64::min);
    let canvas_max_x = bounds.iter().map(|b| b.2).fold(f64::NEG_INFINITY, f64::max);
    let canvas_max_y = bounds.iter().map(|b| b.3).fold(f64::NEG_INFINITY, f64::max);
    let cw = (canvas_max_x - canvas_min_x).max(0.0) as u32;
    let ch = (canvas_max_y - canvas_min_y).max(0.0) as u32;
    if cw == 0 || ch == 0 {
        return Err(MergeError::Unsupported(
            "panorama canvas is empty after transforms".into(),
        ));
    }
    // Non-overlap gate on the transformed boxes (chain order), both axes.
    for pair in bounds.windows(2) {
        let overlap_x = pair[0].2.min(pair[1].2) - pair[0].0.max(pair[1].0);
        let overlap_y = pair[0].3.min(pair[1].3) - pair[0].1.max(pair[1].1);
        if overlap_x <= 0.0 || overlap_y <= 0.0 {
            return Err(MergeError::Unsupported(format!(
                "panorama frames do not overlap after transform (overlap {overlap_x}x{overlap_y}px)"
            )));
        }
    }
    let planes: Vec<Vec<Vec<f32>>> = frames
        .iter()
        .map(|frame| (0..3).map(|c| frame.channel_plane(c)).collect())
        .collect();
    let mut out = vec![0.0f32; cw as usize * ch as usize * 3];
    for y in 0..ch {
        for x in 0..cw {
            let gx = x as f64 + canvas_min_x;
            let gy = y as f64 + canvas_min_y;
            let mut num = [0.0f64; 3];
            let mut den = [0.0f64; 3];
            let mut radiance = [0.0f64; 3];
            let mut covering = 0u32;
            for (inverse, frame_planes) in inverses.iter().zip(&planes) {
                let (sx, sy) = apply_matrix_3x3(inverse, gx, gy).ok_or_else(|| {
                    MergeError::Invalid("panorama inverse transform is singular".into())
                })?;
                if sx < 0.0 || sy < 0.0 || sx >= w as f64 || sy >= h as f64 {
                    continue;
                }
                covering += 1;
                // Feather ramp measured from the frame border (rotation-aware:
                // it follows the frame's own local axes).
                let edge = sx.min((w - 1) as f64 - sx).min(sy).min((h - 1) as f64 - sy);
                let weight = if blend_width_px == 0 {
                    1.0
                } else {
                    (edge.max(0.0) / blend_width_px as f64).clamp(0.0, 1.0)
                };
                for c in 0..3 {
                    let sample = sample_bilinear(&frame_planes[c], w, h, sx, sy) as f64;
                    num[c] += weight * sample;
                    den[c] += weight;
                    radiance[c] += sample;
                }
            }
            if covering == 0 {
                continue; // Outside every frame: stays 0 (canvas gap).
            }
            for c in 0..3 {
                let value = if den[c] > 0.0 {
                    num[c] / den[c]
                } else {
                    // All covering samples sit on a frame border: mean, never NaN.
                    radiance[c] / covering as f64
                };
                out[(y * cw * 3 + x * 3) as usize + c] = clamp_linear(value as f32);
            }
        }
    }
    LinearImage::new(cw, ch, out).map_err(|e| MergeError::Invalid(e.to_string()))
}

/// Inverse of a row-major 3x3 matrix; `None` when numerically singular.
fn invert_matrix_3x3(matrix: &[f64; 9]) -> Option<[f64; 9]> {
    let m = matrix;
    if m.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    // Row-major: [inv00, inv01, inv02, inv10, inv11, inv12, inv20, inv21, inv22].
    Some([
        (m[4] * m[8] - m[5] * m[7]) * inv_det,
        (m[2] * m[7] - m[1] * m[8]) * inv_det,
        (m[1] * m[5] - m[2] * m[4]) * inv_det,
        (m[5] * m[6] - m[3] * m[8]) * inv_det,
        (m[0] * m[8] - m[2] * m[6]) * inv_det,
        (m[2] * m[3] - m[0] * m[5]) * inv_det,
        (m[3] * m[7] - m[4] * m[6]) * inv_det,
        (m[1] * m[6] - m[0] * m[7]) * inv_det,
        (m[0] * m[4] - m[1] * m[3]) * inv_det,
    ])
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

    #[test]
    fn panorama_vertical_gap_is_unsupported() {
        // C4: the non-overlap gate must check both axes. These frames overlap
        // in x (4 < 8) but not in y (8 >= height 8); the previous x-only gate
        // accepted them and produced a gap-filled canvas.
        let a = LinearImage::solid(8, 8, [0.2, 0.2, 0.2]);
        let b = LinearImage::solid(8, 8, [0.8, 0.8, 0.8]);
        let err = blend_panorama(&[a, b], &[(0, 0), (4, 8)], 4).unwrap_err();
        assert!(
            matches!(err, MergeError::Unsupported(_)),
            "vertical gap must be unsupported, got: {err}"
        );
    }

    #[test]
    fn hdr_merge_scales_with_pixels_not_pixels_squared() {
        // C1 scaling anchor. The merge must be O(pixels): `channel_plane(c)`
        // is extracted once per frame/channel, not once per pixel. The
        // previous implementation re-extracted a full plane for every
        // pixel/channel/frame (O(pixels^2)); at 512x512x2 that was ~54 s
        // release and minutes in debug. The budget below is generous for the
        // linear implementation (well under 30 s even on a slow debug CI
        // runner) but fails hard for the quadratic one — a real gate.
        let w = 512u32;
        let h = 512u32;
        let mut pa = Vec::with_capacity((w * h * 3) as usize);
        let mut pb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let v = ((x + y) % 251) as f32 / 255.0;
                pa.extend_from_slice(&[v, v, v]);
                pb.extend_from_slice(&[v * 0.5, v * 0.5, v * 0.5]);
            }
        }
        let a = LinearImage::new(w, h, pa).unwrap();
        let b = LinearImage::new(w, h, pb).unwrap();
        let start = std::time::Instant::now();
        let out = merge_hdr_weighted(
            &[a, b],
            &[exposure(0.01), exposure(0.02)],
            &[(0.0, 0.0), (0.5, -0.5)],
        )
        .unwrap();
        let elapsed = start.elapsed();
        println!("hdr merge 512x512x2 (debug) elapsed: {elapsed:?}");
        assert_eq!((out.width(), out.height()), (w, h));
        assert!(
            elapsed < std::time::Duration::from_secs(30),
            "hdr merge of 512x512x2 took {elapsed:?}; the merge is no longer \
             O(pixels) (quadratic plane re-extraction regression?)"
        );
    }

    #[test]
    fn invert_matrix_3x3_roundtrip() {
        let m = crate::pano_matrix(7.0, -3.0, 2.0, 12.0, 8.0);
        let inv = invert_matrix_3x3(&m).expect("rotation+translation is invertible");
        for &(x, y) in &[(0.0, 0.0), (24.0, 16.0), (3.5, 9.25), (-2.0, 40.0)] {
            let (rx, ry) = apply_matrix_3x3(&m, x, y).unwrap();
            let (bx, by) = apply_matrix_3x3(&inv, rx, ry).unwrap();
            assert!(
                (bx - x).abs() < 1e-9 && (by - y).abs() < 1e-9,
                "({x},{y}) -> ({rx},{ry}) -> ({bx},{by})"
            );
        }
        let singular = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        assert!(invert_matrix_3x3(&singular).is_none());
    }

    #[test]
    fn pano_blend_transformed_applies_rotation() {
        // C3: the blend must apply the full transform. Frame B is a
        // horizontal ramp translated right and rotated 2 deg about its
        // centre. Canvas pixels covered only by B must equal B sampled
        // through the matrix's inverse; the offsets-only blend placed B
        // unrotated and would fail this.
        let w = 200u32;
        let h = 120u32;
        let shade = 170.0f64;
        let a = LinearImage::solid(w, h, [0.0, 0.0, 0.0]);
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for _y in 0..h {
            for x in 0..w {
                let v = x as f32 / (w - 1) as f32;
                px.extend_from_slice(&[v, v, v]);
            }
        }
        let b = LinearImage::new(w, h, px).unwrap();
        let matrix_b = crate::pano_matrix(shade, 0.0, 2.0, w as f64 / 2.0, h as f64 / 2.0);
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let out = blend_panorama_transformed(&[a, b.clone()], &[identity, matrix_b], 8).unwrap();

        // Frame A (identity) spans canvas x in [0, w); only B covers x >= w.
        // Recover the canvas y origin from B's transformed corners.
        let mut canvas_min_y = 0.0f64;
        for (cx, cy) in [
            (0.0, 0.0),
            (w as f64, 0.0),
            (0.0, h as f64),
            (w as f64, h as f64),
        ] {
            let (_, oy) = apply_matrix_3x3(&matrix_b, cx, cy).unwrap();
            canvas_min_y = canvas_min_y.min(oy);
        }
        let canvas_min_y = canvas_min_y.floor();
        let inverse_b = invert_matrix_3x3(&matrix_b).unwrap();
        let b_plane = b.channel_plane(0);
        let (mut checked, mut distinguishes) = (0usize, 0usize);
        for y in 0..out.height() {
            let gy = y as f64 + canvas_min_y;
            for gx in w..out.width() {
                let (sx, sy) = apply_matrix_3x3(&inverse_b, gx as f64, gy).unwrap();
                if sx < 0.0 || sy < 0.0 || sx >= w as f64 || sy >= h as f64 {
                    continue;
                }
                let expected = sample_bilinear(&b_plane, w, h, sx, sy);
                let got = out.pixels()[((y * out.width() + gx) * 3) as usize];
                assert!(
                    (got - expected).abs() < 1e-6,
                    "({gx},{gy}): got {got}, want {expected}"
                );
                let unrotated = sample_bilinear(&b_plane, w, h, gx as f64 - shade, gy);
                if (expected - unrotated).abs() > 1e-3 {
                    distinguishes += 1;
                }
                checked += 1;
            }
        }
        assert!(checked > 1000, "only {checked} B-only pixels checked");
        assert!(
            distinguishes > 0,
            "rotation never changed the sampling (vacuous test)"
        );
    }

    #[test]
    fn pano_blend_transformed_rejects_non_overlap_and_scope() {
        let a = LinearImage::solid(8, 8, [0.2, 0.2, 0.2]);
        let b = LinearImage::solid(8, 8, [0.8, 0.8, 0.8]);
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        // Vertical gap: overlap_x > 0 but overlap_y == 0.
        let shifted_y = [1.0, 0.0, 0.0, 0.0, 1.0, 8.0, 0.0, 0.0, 1.0];
        let err = blend_panorama_transformed(&[a.clone(), b.clone()], &[identity, shifted_y], 4)
            .unwrap_err();
        assert!(matches!(err, MergeError::Unsupported(_)), "got: {err}");
        // Perspective/shear is outside the 1.5 rotation-light scope.
        let shear = [1.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let err = blend_panorama_transformed(&[a, b], &[identity, shear], 4).unwrap_err();
        assert!(matches!(err, MergeError::Unsupported(_)), "got: {err}");
    }
}
