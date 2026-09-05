//! LRPAR-G13-MERGE-15 / MERGE-CORE-1: platform-neutral geometry/blend
//! primitives for HDR/panorama merge.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Ausrichtung, Folge-Tasks Nr. 2). This module is the only `lumina-core`
//! share of MERGE-CORE-1: pure math on `f32` planes, no filesystem, no DNG
//! IO, no `Pipeline` changes. All merge orchestration (EXIF gating,
//! `unsupported` policy, digest) lives in the new `lumina-merge` crate,
//! which consumes these primitives.
//!
//! Determinism: every function here is pure and deterministic for identical
//! inputs. Bilinear sampling uses `f64` accumulation with round-to-nearest
//! at the end where quantisation is needed; callers that need cross-platform
//! float tolerance document it (`lumina-merge` digest docs, tolerance
//! `1e-6`).

/// Clamp a linear light value to the non-negative half-line. Negative
/// inputs only arise from float resampling overshoot; values above 1 are
/// HDR scene data and are preserved (never clipped here).
pub fn clamp_linear(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    value.max(0.0)
}

/// HDR per-pixel weight ("hat" function): 0 at black/clip, 1 at mid-tone.
///
/// Monotone rising on `[0, 0.5]`, monotone falling on `[0.5, 1]`, zero
/// outside `[0, 1]` (clipped inputs contribute nothing, which is what
/// keeps saturated pixels out of the HDR radiance mean).
pub fn hdr_hat_weight(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    if value <= 0.0 || value >= 1.0 {
        return 0.0;
    }
    if value <= 0.5 {
        value * 2.0
    } else {
        (1.0 - value) * 2.0
    }
}

/// Feather-blend ramp for panorama overlap.
///
/// `position` counts from the overlap start (0 = fully image A,
/// `overlap-1` = fully image B). A `blend_width` of 0 means hard seam
/// (0 left of centre, 1 right of centre). Otherwise a linear ramp of
/// `blend_width` pixels is centred in the overlap; outside the ramp the
/// weight is 0/1. Returns the weight of image B in `[0, 1]`.
pub fn feather_weight(position: u32, overlap: u32, blend_width: u32) -> f32 {
    if overlap == 0 {
        return 0.0;
    }
    if blend_width == 0 {
        return if position * 2 < overlap { 0.0 } else { 1.0 };
    }
    let width = blend_width.min(overlap) as f64;
    let start = (overlap as f64 - width) / 2.0;
    let t = (position as f64 - start) / width;
    t.clamp(0.0, 1.0) as f32
}

/// Bilinear sample of one channel plane with zero fill outside bounds.
///
/// Pure function; out-of-bounds reads yield 0.0 (documented fill, not a
/// silent crop: the caller decides canvas size and `unsupported` policy).
pub fn sample_bilinear(plane: &[f32], width: u32, height: u32, x: f64, y: f64) -> f32 {
    if width == 0 || height == 0 {
        return 0.0;
    }
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    let w = width as i64;
    let h = height as i64;
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let tx = (x - x0 as f64).clamp(0.0, 1.0);
    let ty = (y - y0 as f64).clamp(0.0, 1.0);
    let at = |xx: i64, yy: i64| -> f64 {
        if xx < 0 || yy < 0 || xx >= w || yy >= h {
            0.0
        } else {
            plane[(yy as u32 * width + xx as u32) as usize] as f64
        }
    };
    let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
    let bottom = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
    (top * (1.0 - ty) + bottom * ty) as f32
}

/// Translate one channel plane by `(dx, dy)` (output pixel `(x, y)` reads
/// input `(x - dx, y - dy)`), bilinear, zero fill outside.
pub fn translate_plane(plane: &[f32], width: u32, height: u32, dx: f64, dy: f64) -> Vec<f32> {
    let mut out = vec![0.0f32; plane.len()];
    for y in 0..height {
        for x in 0..width {
            out[(y * width + x) as usize] =
                sample_bilinear(plane, width, height, x as f64 - dx, y as f64 - dy);
        }
    }
    out
}

/// Apply a row-major 3x3 homogeneous matrix to a point.
pub fn apply_matrix_3x3(matrix: &[f64; 9], x: f64, y: f64) -> Option<(f64, f64)> {
    let w = matrix[6] * x + matrix[7] * y + matrix[8];
    if !w.is_finite() || w.abs() < 1e-12 {
        return None;
    }
    let ox = (matrix[0] * x + matrix[1] * y + matrix[2]) / w;
    let oy = (matrix[3] * x + matrix[4] * y + matrix[5]) / w;
    if !ox.is_finite() || !oy.is_finite() {
        return None;
    }
    Some((ox, oy))
}

/// Check that a 3x3 matrix is translation+rotation-light (1.5 scope):
/// last row is `(0, 0, 1)` within `tol` and the upper-left 2x2 is a
/// scaled rotation (columns orthogonal, equal norm) within `tol`.
/// HDR matrices must additionally be pure translation (checked by the
/// caller via `is_translation`).
pub fn is_translation_rotation_light(matrix: &[f64; 9], tol: f64) -> bool {
    if matrix.iter().any(|v| !v.is_finite()) {
        return false;
    }
    if matrix[6].abs() > tol || matrix[7].abs() > tol || (matrix[8] - 1.0).abs() > tol {
        return false;
    }
    let (a, b, c, d) = (matrix[0], matrix[1], matrix[3], matrix[4]);
    let norm0 = a * a + c * c;
    let norm1 = b * b + d * d;
    if norm0 <= 0.0 || norm1 <= 0.0 {
        return false;
    }
    let dot = a * b + c * d;
    (norm0 - norm1).abs() / norm0.max(norm1) <= tol && dot.abs() / norm0.max(norm1) <= tol
}

/// Check pure translation: identity rotation/scale part within `tol`.
pub fn is_translation(matrix: &[f64; 9], tol: f64) -> bool {
    if matrix.iter().any(|v| !v.is_finite()) {
        return false;
    }
    (matrix[0] - 1.0).abs() <= tol
        && matrix[1].abs() <= tol
        && matrix[3].abs() <= tol
        && (matrix[4] - 1.0).abs() <= tol
        && matrix[6].abs() <= tol
        && matrix[7].abs() <= tol
        && (matrix[8] - 1.0).abs() <= tol
}

/// Cylindrical projection map: output pixel `(x, y)` reads input at the
/// returned `(sx, sy)`. `focal_px` is the cylinder focal length in pixels
/// (conventionally image width); must be finite and positive.
///
/// Inverse (sampling) form: `sx = f * atan((x - cx) / f) + cx`,
/// `sy = f * (y - cy) / sqrt((x - cx)^2 + f^2) + cy`.
/// Only cylindrical projection is in 1.5 scope (no spherical/fisheye).
pub fn cylindrical_sample_pos(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    focal_px: f64,
) -> Option<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() || !focal_px.is_finite() || focal_px <= 0.0 {
        return None;
    }
    let cx = width / 2.0;
    let cy = height / 2.0;
    let dx = x - cx;
    let sx = focal_px * (dx / focal_px).atan() + cx;
    let sy = focal_px * (y - cy) / (dx * dx + focal_px * focal_px).sqrt() + cy;
    if !sx.is_finite() || !sy.is_finite() {
        return None;
    }
    Some((sx, sy))
}

/// Warp one channel plane through the cylindrical projection (same
/// dimensions out as in), bilinear, zero fill outside.
pub fn cylindrical_warp_plane(plane: &[f32], width: u32, height: u32, focal_px: f64) -> Vec<f32> {
    let mut out = vec![0.0f32; plane.len()];
    for y in 0..height {
        for x in 0..width {
            if let Some((sx, sy)) =
                cylindrical_sample_pos(x as f64, y as f64, width as f64, height as f64, focal_px)
            {
                out[(y * width + x) as usize] = sample_bilinear(plane, width, height, sx, sy);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_weight_range_and_monotonicity() {
        assert_eq!(hdr_hat_weight(0.0), 0.0);
        assert_eq!(hdr_hat_weight(1.0), 0.0);
        assert_eq!(hdr_hat_weight(-0.5), 0.0);
        assert_eq!(hdr_hat_weight(1.5), 0.0);
        assert_eq!(hdr_hat_weight(f32::NAN), 0.0);
        assert_eq!(hdr_hat_weight(f32::INFINITY), 0.0);
        let mut prev = 0.0f32;
        let mut v = 0.0f32;
        while v <= 0.5 {
            let w = hdr_hat_weight(v);
            assert!(w >= prev, "rising on [0,0.5]");
            assert!((0.0..=1.0).contains(&w));
            prev = w;
            v += 0.05;
        }
        let mut prev = hdr_hat_weight(0.5);
        let mut v = 0.5f32;
        while v <= 1.0 {
            let w = hdr_hat_weight(v);
            assert!(w <= prev + 1e-6, "falling on [0.5,1]");
            prev = w;
            v += 0.05;
        }
    }

    #[test]
    fn feather_ramp_bounds_and_monotonicity() {
        assert_eq!(feather_weight(0, 0, 64), 0.0);
        let overlap = 100;
        let mut prev = -1.0f32;
        for pos in 0..overlap {
            let w = feather_weight(pos, overlap, 64);
            assert!((0.0..=1.0).contains(&w), "in range");
            assert!(w >= prev, "monotone");
            prev = w;
        }
        assert_eq!(feather_weight(0, overlap, 64), 0.0);
        assert_eq!(feather_weight(overlap - 1, overlap, 64), 1.0);
        // Hard seam with blend width 0.
        assert_eq!(feather_weight(0, 10, 0), 0.0);
        assert_eq!(feather_weight(9, 10, 0), 1.0);
    }

    #[test]
    fn translate_identity_and_integer_shift() {
        let plane = vec![1.0, 2.0, 3.0, 4.0];
        let same = translate_plane(&plane, 2, 2, 0.0, 0.0);
        assert_eq!(same, plane);
        let shifted = translate_plane(&plane, 2, 2, 1.0, 0.0);
        assert_eq!(shifted, vec![0.0, 1.0, 0.0, 3.0]);
    }

    #[test]
    fn matrix_scope_checks() {
        let ident = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert!(is_translation_rotation_light(&ident, 1e-9));
        assert!(is_translation(&ident, 1e-9));
        let rot90 = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        assert!(is_translation_rotation_light(&rot90, 1e-9));
        assert!(!is_translation(&rot90, 1e-9));
        // Perspective row /= scope.
        let persp = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.01, 0.0, 1.0];
        assert!(!is_translation_rotation_light(&persp, 1e-9));
        // Shear /= rotation.
        let shear = [1.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert!(!is_translation_rotation_light(&shear, 1e-9));
    }

    #[test]
    fn cylindrical_centre_is_fixed_point() {
        let (sx, sy) = cylindrical_sample_pos(50.0, 30.0, 100.0, 60.0, 100.0).unwrap();
        assert!((sx - 50.0).abs() < 1e-9);
        assert!((sy - 30.0).abs() < 1e-9);
        assert!(cylindrical_sample_pos(0.0, 0.0, 100.0, 60.0, 0.0).is_none());
        assert!(cylindrical_sample_pos(0.0, 0.0, 100.0, 60.0, f64::NAN).is_none());
    }
}
