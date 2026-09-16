//! Alignment: HDR translation + panorama translation/rotation-light.
//!
//! - HDR: exhaustive integer search over `±max_shift_px` on luminance with
//!   the SAD metric, then subpixel-light refinement at half-pixel steps via
//!   bilinear resampling. Reports shift + residual (shift magnitude, px).
//!   No homography, no ghost removal (1.5 scope).
//! - Panorama: cylindrical warp (focal = width) of both frames, then joint
//!   search over integer translation and small rotations
//!   (`-2..=+2` degrees, step 1) of the moving frame about its centre.
//!   Returns a row-major 3x3 matrix mapping moving-frame coordinates into
//!   the reference frame (`T · R` with the rotation about the frame centre,
//!   see [`pano_matrix`]).
//!   Non-overlapping results (overlap `< PANO_MIN_OVERLAP_PX` in either
//!   axis) are `Unsupported`; spherical/fisheye is out of scope.

use crate::{AlignStatus, LinearImage, MergeError, HDR_SHIFT_WARN_PX, PANO_MIN_OVERLAP_PX};
use lumina_core::merge_geom::{cylindrical_warp_plane, sample_bilinear};

/// HDR translation of one frame into the reference frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HdrShift {
    pub dx: f64,
    pub dy: f64,
    pub residual_px: f64,
    pub status: AlignStatus,
}

/// Panorama transform of one frame into the reference frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanoTransform {
    /// Row-major 3x3 homogeneous matrix mapping moving-frame coordinates
    /// into the reference frame: `T(dx, dy) · R_centre` (rotation about the
    /// frame centre, see [`pano_matrix`]).
    pub matrix_3x3: [f64; 9],
    /// Applied rotation in degrees (from `-2..=+2` search grid).
    pub rotation_deg: f64,
    pub residual_px: f64,
    pub status: AlignStatus,
}

fn sad_at_offset(
    ref_luma: &[f32],
    mov_luma: &[f32],
    width: u32,
    height: u32,
    dx: f64,
    dy: f64,
) -> f64 {
    let mut acc = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            let sx = x as f64 - dx;
            let sy = y as f64 - dy;
            let sample = sample_bilinear(mov_luma, width, height, sx, sy) as f64;
            let r = ref_luma[(y * width + x) as usize] as f64;
            acc += (r - sample).abs();
        }
    }
    acc
}

fn status_for_shift(dx: f64, dy: f64) -> (f64, AlignStatus) {
    let residual = (dx * dx + dy * dy).sqrt();
    let status = if residual > HDR_SHIFT_WARN_PX {
        AlignStatus::AlignedWithResidual
    } else {
        AlignStatus::Aligned
    };
    (residual, status)
}

/// Estimate HDR translation of `moving` into `reference` (same dimensions
/// required; mismatch is `Invalid`, never silently rescaled).
pub fn estimate_hdr_translation(
    reference: &LinearImage,
    moving: &LinearImage,
    max_shift_px: i32,
) -> Result<HdrShift, MergeError> {
    if reference.width() != moving.width() || reference.height() != moving.height() {
        return Err(MergeError::Invalid(format!(
            "hdr alignment needs equal dimensions, got {}x{} vs {}x{}",
            reference.width(),
            reference.height(),
            moving.width(),
            moving.height()
        )));
    }
    if max_shift_px < 0 {
        return Err(MergeError::Invalid(format!(
            "max_shift_px must be >= 0, got {max_shift_px}"
        )));
    }
    let (w, h) = (reference.width(), reference.height());
    let ref_luma = reference.luminance();
    let mov_luma = moving.luminance();
    let mut best = (0i32, 0i32);
    let mut best_sad = f64::INFINITY;
    for dy in -max_shift_px..=max_shift_px {
        for dx in -max_shift_px..=max_shift_px {
            let sad = sad_at_offset(&ref_luma, &mov_luma, w, h, dx as f64, dy as f64);
            if sad < best_sad {
                best_sad = sad;
                best = (dx, dy);
            }
        }
    }
    // Subpixel-light: half-pixel refinement around the integer best.
    let mut fine = (best.0 as f64, best.1 as f64);
    let mut fine_sad = best_sad;
    for dy in [-0.5, 0.0, 0.5] {
        for dx in [-0.5, 0.0, 0.5] {
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            let cand = (best.0 as f64 + dx, best.1 as f64 + dy);
            let sad = sad_at_offset(&ref_luma, &mov_luma, w, h, cand.0, cand.1);
            if sad < fine_sad {
                fine_sad = sad;
                fine = cand;
            }
        }
    }
    let (residual, status) = status_for_shift(fine.0, fine.1);
    Ok(HdrShift {
        dx: fine.0,
        dy: fine.1,
        residual_px: residual,
        status,
    })
}

fn rotate_point(x: f64, y: f64, cx: f64, cy: f64, angle_rad: f64) -> (f64, f64) {
    let (s, c) = angle_rad.sin_cos();
    let dx = x - cx;
    let dy = y - cy;
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

/// Row-major 3x3 panorama transform for translation `(dx, dy)` and rotation
/// `angle_deg` **about the frame centre** `(cx, cy)`:
///
/// ```text
/// T(dx, dy) · Translate(cx, cy) · R(angle_deg) · Translate(-cx, -cy)
/// ```
///
/// The rotation centre is the same one [`sad_pano_at`] searches, so applying
/// this matrix to the moving frame reproduces exactly where the SAD matched
/// it. A rotation about the origin (the previous construction) mismatched the
/// search by `C - R·C` — ~126 px at 6000x4000 / 2 deg (C2).
#[must_use]
pub fn pano_matrix(dx: f64, dy: f64, angle_deg: f64, cx: f64, cy: f64) -> [f64; 9] {
    let (s, c) = angle_deg.to_radians().sin_cos();
    // R_c(p) = R·(p - C) + C = R·p + (C - R·C); then add the translation.
    let tx = dx + cx - (c * cx - s * cy);
    let ty = dy + cy - (s * cx + c * cy);
    [c, -s, tx, s, c, ty, 0.0, 0.0, 1.0]
}

fn sad_pano_at(
    ref_luma: &[f32],
    mov_luma: &[f32],
    width: u32,
    height: u32,
    dx: f64,
    dy: f64,
    angle_deg: f64,
) -> f64 {
    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;
    let angle = angle_deg.to_radians();
    let mut acc = 0.0f64;
    for y in 0..height {
        for x in 0..width {
            // Output (x,y) in reference frame: undo translation, then undo
            // rotation about the moving-frame centre, then sample.
            let ux = x as f64 - dx;
            let uy = y as f64 - dy;
            let (sx, sy) = rotate_point(ux, uy, cx, cy, -angle);
            let sample = sample_bilinear(mov_luma, width, height, sx, sy) as f64;
            acc += (ref_luma[(y * width + x) as usize] as f64 - sample).abs();
        }
    }
    acc
}

/// Estimate a panorama transform of `moving` into `reference`.
///
/// Both frames are cylindrically projected (focal = width) before
/// matching. The search covers integer translations within
/// `±max_shift_px` and rotations `-2..=+2` deg. Overlap after the best
/// translation must be `>= PANO_MIN_OVERLAP_PX` in both axes, else
/// `Unsupported` (no silent partial panorama). Dimension mismatch is
/// `Invalid`.
pub fn estimate_pano_transform(
    reference: &LinearImage,
    moving: &LinearImage,
    max_shift_px: i32,
) -> Result<PanoTransform, MergeError> {
    if reference.width() != moving.width() || reference.height() != moving.height() {
        return Err(MergeError::Invalid(format!(
            "pano alignment needs equal dimensions, got {}x{} vs {}x{}",
            reference.width(),
            reference.height(),
            moving.width(),
            moving.height()
        )));
    }
    if max_shift_px < 0 {
        return Err(MergeError::Invalid(format!(
            "max_shift_px must be >= 0, got {max_shift_px}"
        )));
    }
    let (w, h) = (reference.width(), reference.height());
    let focal = w as f64;
    let ref_cyl = cylindrical_warp_plane(&reference.luminance(), w, h, focal);
    let mov_cyl = cylindrical_warp_plane(&moving.luminance(), w, h, focal);
    let mut best = (0i32, 0i32, 0i32);
    let mut best_sad = f64::INFINITY;
    for angle in -2..=2 {
        for dy in -max_shift_px..=max_shift_px {
            for dx in -max_shift_px..=max_shift_px {
                let sad = sad_pano_at(&ref_cyl, &mov_cyl, w, h, dx as f64, dy as f64, angle as f64);
                if sad < best_sad {
                    best_sad = sad;
                    best = (dx, dy, angle);
                }
            }
        }
    }
    let overlap_x = w as i64 - best.0.abs() as i64;
    let overlap_y = h as i64 - best.1.abs() as i64;
    if overlap_x < PANO_MIN_OVERLAP_PX as i64 || overlap_y < PANO_MIN_OVERLAP_PX as i64 {
        return Err(MergeError::Unsupported(format!(
            "panorama frames do not overlap (overlap {overlap_x}x{overlap_y}px, minimum {}px)",
            PANO_MIN_OVERLAP_PX
        )));
    }
    // Rotation is about the frame centre — exactly where `sad_pano_at`
    // matched it — so the matrix places the moving frame where alignment
    // found it (C2).
    let matrix_3x3 = pano_matrix(
        best.0 as f64,
        best.1 as f64,
        best.2 as f64,
        w as f64 / 2.0,
        h as f64 / 2.0,
    );
    let (residual, status) = status_for_shift(best.0 as f64, best.1 as f64);
    Ok(PanoTransform {
        matrix_3x3,
        rotation_deg: best.2 as f64,
        residual_px: residual,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::merge_geom::apply_matrix_3x3;

    fn shifted_pair() -> (LinearImage, LinearImage) {
        // 8x8 gradient shifted by (+1, 0): moving[x] = ref[x-1].
        let w = 8u32;
        let h = 8u32;
        let mut a = Vec::new();
        let mut b = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (x as f32 + y as f32 * 8.0) / 64.0;
                a.extend_from_slice(&[v, v, v]);
                let v2 = if x == 0 {
                    0.0
                } else {
                    ((x - 1) as f32 + y as f32 * 8.0) / 64.0
                };
                b.extend_from_slice(&[v2, v2, v2]);
            }
        }
        (
            LinearImage::new(w, h, a).unwrap(),
            LinearImage::new(w, h, b).unwrap(),
        )
    }

    #[test]
    fn hdr_translation_recovers_integer_shift() {
        let (a, b) = shifted_pair();
        // `b[x] = a[x-1]` (content moved right by 1); aligning `b` onto
        // `a` shifts it back left, i.e. dx = -1 (output reads input at
        // `x - dx`).
        let shift = estimate_hdr_translation(&a, &b, 4).unwrap();
        assert_eq!((shift.dx as i32, shift.dy as i32), (-1, 0));
        assert_eq!(shift.status, AlignStatus::Aligned);
        assert!((shift.residual_px - 1.0).abs() < 1e-9);
    }

    #[test]
    fn hdr_identical_frames_align_at_zero() {
        let frame = LinearImage::solid(8, 8, [0.4, 0.4, 0.4]);
        let shift = estimate_hdr_translation(&frame, &frame, 4).unwrap();
        assert_eq!((shift.dx, shift.dy), (0.0, 0.0));
        assert_eq!(shift.residual_px, 0.0);
    }

    #[test]
    fn hdr_large_shift_warns_with_residual() {
        // Moving frame shifted far: estimator saturates at the window edge
        // and reports the residual loudly instead of silently cropping.
        let w = 32u32;
        let h = 8u32;
        let mut a = Vec::new();
        let mut b = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (x as f32 + y as f32) / 40.0;
                a.extend_from_slice(&[v, v, v]);
                let v2 = x
                    .checked_sub(20)
                    .map_or(0.0, |xs| (xs as f32 + y as f32) / 40.0);
                b.extend_from_slice(&[v2, v2, v2]);
            }
        }
        let a = LinearImage::new(w, h, a).unwrap();
        let b = LinearImage::new(w, h, b).unwrap();
        let shift = estimate_hdr_translation(&a, &b, 24).unwrap();
        assert!(shift.residual_px > HDR_SHIFT_WARN_PX);
        assert_eq!(shift.status, AlignStatus::AlignedWithResidual);
    }

    #[test]
    fn hdr_dimension_mismatch_is_invalid_not_rescaled() {
        let a = LinearImage::solid(8, 8, [0.3, 0.3, 0.3]);
        let b = LinearImage::solid(4, 8, [0.3, 0.3, 0.3]);
        let err = estimate_hdr_translation(&a, &b, 4).unwrap_err();
        assert!(matches!(err, MergeError::Invalid(_)));
    }

    #[test]
    fn pano_identical_frames_yield_identity() {
        // Deterministic gradient so the identity SAD minimum is unique.
        let w = 16u32;
        let h = 8u32;
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (x as f32 * 0.7 + y as f32 * 3.1) / 32.0;
                px.extend_from_slice(&[v, v, v]);
            }
        }
        let frame = LinearImage::new(w, h, px).unwrap();
        // Zero search window: only the identity candidate is evaluated, so
        // the result is exactly identity regardless of projection.
        let t = estimate_pano_transform(&frame, &frame, 0).unwrap();
        assert_eq!(t.rotation_deg, 0.0);
        assert_eq!(t.matrix_3x3, [1.0, -0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn pano_no_overlap_is_unsupported() {
        // 16x8 ramp in `a`; `b` carries the same ramp shifted by 12px, so
        // the SAD minimum sits at dx=12 with only 4px overlap (< 8px min).
        let w = 16u32;
        let h = 8u32;
        let mut pa = Vec::new();
        let mut pb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (x as f32 + y as f32) / 32.0;
                pa.extend_from_slice(&[v, v, v]);
                let v2 = if x < 12 {
                    0.0
                } else {
                    ((x - 12) as f32 + y as f32) / 32.0
                };
                pb.extend_from_slice(&[v2, v2, v2]);
            }
        }
        let a = LinearImage::new(w, h, pa).unwrap();
        let b = LinearImage::new(w, h, pb).unwrap();
        let err = estimate_pano_transform(&a, &b, 12).unwrap_err();
        assert!(
            matches!(err, MergeError::Unsupported(_)),
            "non-overlapping pair must be unsupported, got: {err}"
        );
    }

    #[test]
    fn hdr_subpixel_refinement_recovers_half_pixel_shift() {
        // C5: the subpixel-light refinement must return a non-integer shift.
        // `moving = a` sampled at `x + 0.5` on a ramp that is linear in both
        // axes: the SAD minimum sits at dx = +0.5, so the integer coarse
        // search has to be refined on the 0.5 grid. (The x=0 column carries a
        // small zero-fill edge error, but the interior match dominates; the
        // y slope keeps dy = 0 unique.)
        let w = 16u32;
        let h = 8u32;
        let mut pa = Vec::new();
        let mut pb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let base = 0.1 + (x as f32 + y as f32) / 64.0;
                pa.extend_from_slice(&[base, base, base]);
                let shifted = 0.1 + (x as f32 + 0.5 + y as f32) / 64.0;
                pb.extend_from_slice(&[shifted, shifted, shifted]);
            }
        }
        let a = LinearImage::new(w, h, pa).unwrap();
        let b = LinearImage::new(w, h, pb).unwrap();
        let shift = estimate_hdr_translation(&a, &b, 2).unwrap();
        assert!(
            (shift.dx - 0.5).abs() < 1e-9,
            "expected the half-pixel shift dx = 0.5, got {shift:?}"
        );
        assert!(
            shift.dx.fract().abs() > 1e-9,
            "shift must be subpixel (non-integer), got {}",
            shift.dx
        );
        assert!(
            shift.dy.abs() < 1e-9,
            "vertical shift must stay 0, got {}",
            shift.dy
        );
    }

    #[test]
    fn pano_matrix_rotates_about_the_frame_centre() {
        // C2: the matrix must use the same rotation centre as the SAD search.
        // SAD convention: reference `r` samples moving at
        // `rotate_center(r - t, -angle)`, so the matrix must map a moving
        // point `s` to `rotate_center(s, +angle) + t` (<= 1e-6).
        let (w, h) = (6000.0f64, 4000.0f64);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let (dx, dy, angle) = (37.0, -21.0, 2.0);
        let matrix = pano_matrix(dx, dy, angle, cx, cy);
        let angle_rad = angle.to_radians();
        for &(sx, sy) in &[(cx, cy), (0.0, 0.0), (w, h), (1234.0, 3210.0)] {
            let (ex, ey) = rotate_point(sx, sy, cx, cy, angle_rad);
            let (want_x, want_y) = (ex + dx, ey + dy);
            let (got_x, got_y) = apply_matrix_3x3(&matrix, sx, sy).unwrap();
            assert!(
                (got_x - want_x).abs() < 1e-6 && (got_y - want_y).abs() < 1e-6,
                "({sx},{sy}) -> ({got_x},{got_y}), want ({want_x},{want_y})"
            );
        }
        // Regression magnitude: a rotation about the origin would displace the
        // (0,0) corner by |C - R*C| ~ 126 px at 6000x4000 / 2 deg, far above
        // the 1e-6 contract.
        let (got_x, got_y) = apply_matrix_3x3(&matrix, 0.0, 0.0).unwrap();
        let origin_error = ((got_x - dx).powi(2) + (got_y - dy).powi(2)).sqrt();
        assert!(
            origin_error > 100.0,
            "origin-rotation bug would be undetectable here, error {origin_error}"
        );
    }

    #[test]
    fn pano_estimate_rotation_centre_is_the_frame_centre() {
        // Estimator anchor: whatever transform is found, its rotation centre
        // must be the frame centre — with a zero translation window the
        // centre maps to itself, whereas an origin rotation would displace it
        // by |C - R*C| (large for a big frame).
        let w = 400u32;
        let h = 240u32;
        let angle = 2.0f64.to_radians();
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = (x as f32 * 0.7 + y as f32 * 3.1) / 256.0;
                px.extend_from_slice(&[v, v, v]);
            }
        }
        let a = LinearImage::new(w, h, px).unwrap();
        let a_plane = a.channel_plane(0);
        let mut pb = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = rotate_point(x as f64, y as f64, cx, cy, -angle);
                let v = sample_bilinear(&a_plane, w, h, sx, sy);
                pb.extend_from_slice(&[v, v, v]);
            }
        }
        let b = LinearImage::new(w, h, pb).unwrap();
        let t = estimate_pano_transform(&a, &b, 0).unwrap();
        assert!(
            t.rotation_deg.abs() >= 1.0,
            "expected a nonzero rotation for the rotated pair, got {t:?}"
        );
        let (ox, oy) = apply_matrix_3x3(&t.matrix_3x3, cx, cy).unwrap();
        let centre_error = ((ox - cx).powi(2) + (oy - cy).powi(2)).sqrt();
        assert!(
            centre_error < 1e-6,
            "rotation centre moved by {centre_error}px; origin rotation bug?"
        );
    }
}
