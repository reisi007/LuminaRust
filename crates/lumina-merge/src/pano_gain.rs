//! Slice A (gain compensation): panorama brightness matching plus the
//! panorama merge pass that applies it.
//!
//! - [`PanoGainMode`] is the one frontend-facing mode enum (`off` |
//!   `overlap-median`); [`estimate_pano_gains`] measures the gains from
//!   pixels, [`blend_panorama_with_gains`] /
//!   [`blend_panorama_transformed_with_gains`] apply them multiplicatively
//!   before the feather combination, and [`align_and_merge_pano`] is the
//!   bundle-side pass (align → estimate → blend → recipe value).
//! - Extracted from `merge.rs` / `bundle.rs` (file-size ratchet); no
//!   behaviour lives here twice and no blend math changed in the move.

use log::info;
use lumina_core::merge_geom::{apply_matrix_3x3, is_translation_rotation_light, sample_bilinear};
use lumina_sidecar::{
    MergeMode, MergeTransform, PanoGainCompensation, PanoGainMethod, A_MIN_OVERLAP_STAT_PX,
    MAX_PANO_GAIN,
};

use crate::bundle::{map_merge_error, MergePass, MergeRunError};
use crate::merge::{blend_panorama_impl, blend_panorama_transformed_impl};
use crate::{blend_panorama_transformed, estimate_pano_transform, LinearImage, MergeError};

/// Slice A: panorama gain-compensation mode (one enum, three spellings).
/// `Off` is the default on every frontend (no behaviour change); the only
/// estimating variant in 1.0 is `OverlapMedian`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanoGainMode {
    #[default]
    Off,
    OverlapMedian,
}

impl PanoGainMode {
    /// Parses the shared CLI/MCP/GUI spelling (`off` | `overlap-median`).
    /// Unknown values are a loud `Err` with the accepted values — each
    /// frontend maps it onto its own transport error (CLI usage, MCP
    /// `InvalidParams`, GUI string). One message for all three spellings of
    /// the field (`--gain-compensation` flag / `gain_compensation` key).
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "off" => Ok(PanoGainMode::Off),
            "overlap-median" => Ok(PanoGainMode::OverlapMedian),
            other => Err(format!(
                "invalid `gain_compensation` value `{other}`: expected `off` or `overlap-median`"
            )),
        }
    }
}

/// Slice A: the panorama-only options gate (runs in `run_merge` before any
/// decode or file write, like the `max_shift_px < 0` guard).
pub(crate) fn check_gain_mode(mode: MergeMode, gain: PanoGainMode) -> Result<(), String> {
    if mode == MergeMode::Hdr && gain != PanoGainMode::Off {
        return Err(
            "invalid --gain-compensation `overlap-median`: gain compensation is panorama only"
                .into(),
        );
    }
    Ok(())
}

/// Slice A: estimate after alignment and shape the recipe value.
pub(crate) fn estimate_recipe_gain(
    frames: &[LinearImage],
    matrices: &[[f64; 9]],
) -> Result<PanoGainCompensation, MergeError> {
    let estimate = estimate_pano_gains(frames, matrices)?;
    Ok(PanoGainCompensation {
        method: PanoGainMethod::OverlapMedian,
        gains: estimate.gains,
        min_overlap_px: estimate.min_overlap_px,
    })
}

/// The panorama merge pass: align (translation+rotation-light matrices),
/// then — only for `OverlapMedian` — estimate the gain and blend
/// gain-scaled; otherwise blend as before. Moved from `bundle.rs`
/// (`align_and_merge`, panorama arm) unchanged apart from the signature.
pub(crate) fn align_and_merge_pano(
    frames: &[LinearImage],
    max_shift_px: i32,
    blend_width_px: u32,
    gain_compensation: PanoGainMode,
) -> Result<MergePass, MergeRunError> {
    // Full 3x3 matrix per frame (translation + rotation about the
    // frame centre, `lumina-merge::pano_matrix`). The blend applies
    // each frame through its **full inverse matrix**
    // ([`blend_panorama_transformed`]), never only the integer
    // offsets (which silently dropped the estimated rotation).
    let mut matrices = Vec::with_capacity(frames.len());
    let mut transforms = Vec::new();
    let mut residual_px: f64 = 0.0;
    let mut residual_warning = false;
    matrices.push([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    for (index, frame) in frames.iter().enumerate().skip(1) {
        let transform =
            estimate_pano_transform(&frames[0], frame, max_shift_px).map_err(map_merge_error)?;
        matrices.push(transform.matrix_3x3);
        transforms.push(MergeTransform {
            source_index: index,
            matrix_3x3: transform.matrix_3x3,
        });
        info!(
            "merge-pano: source #{index} aligned with rotation {:.3}deg (matrix {:?})",
            transform.rotation_deg, transform.matrix_3x3
        );
        residual_px = residual_px.max(transform.residual_px);
        residual_warning |= transform.status.residual_flag();
    }
    let (merged, gain_compensation) = match gain_compensation {
        // Slice A: estimate after alignment, blend gain-scaled.
        PanoGainMode::OverlapMedian => {
            let gain = estimate_recipe_gain(frames, &matrices).map_err(map_merge_error)?;
            info!(
                "merge-pano: gain compensation ({:?}): gains {} (min overlap {}px)",
                gain.method,
                gain.gains
                    .iter()
                    .map(|gain| format!("{gain:.4}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                gain.min_overlap_px
            );
            let merged = blend_panorama_transformed_with_gains(
                frames,
                &matrices,
                blend_width_px,
                &gain.gains,
            )
            .map_err(map_merge_error)?;
            (merged, Some(gain))
        }
        PanoGainMode::Off => (
            blend_panorama_transformed(frames, &matrices, blend_width_px)
                .map_err(map_merge_error)?,
            None,
        ),
    };
    Ok(MergePass {
        merged,
        transforms,
        residual_px,
        residual_warning,
        gain_compensation,
    })
}

/// Luminance floor for the gain statistic: a pair whose frame luminance is
/// at or below this is skipped (the ratio would divide by ~0). Documented,
/// never silently replaced by a guessed value.
const GAIN_LUMINANCE_EPS: f64 = 1e-6;

/// Outcome of [`estimate_pano_gains`]: one gain per frame (`gains[0] == 1.0`
/// by reference definition) plus the smallest pairwise overlap pixel count
/// the statistic stands on (persisted as `min_overlap_px`).
#[derive(Debug, Clone, PartialEq)]
pub struct PanoGainEstimate {
    pub gains: Vec<f64>,
    pub min_overlap_px: u64,
}

/// Estimates panorama gain compensation from pixels (Slice A).
///
/// For every frame `i >= 1`, the per-pixel luminance ratio
/// `L_0 / L_i` (Rec.709, same plane the alignment matches on) is collected
/// over the **full geometric overlap** of frame `i` with frame 0 — every
/// canvas pixel covered by both frames under the same coverage predicate the
/// transformed blend uses (inverse transform inside `[0, w) x [0, h)`), not
/// the feather ramp — and the gain is the **median** of those ratios
/// (sort-based, robust against ghosts/saturation outliers, deterministic:
/// fixed canvas row-major and frame-index iteration order, no hash maps, no
/// randomness). `gains[0]` is 1.0 by definition.
///
/// Loud limits (never clamped):
/// - pairwise overlap `< A_MIN_OVERLAP_STAT_PX` → `Unsupported` (with both
///   pixel counts in the text);
/// - estimated gain `> MAX_PANO_GAIN` → `Unsupported` (with the measured
///   value; almost always mis-alignment, not exposure);
/// - non-finite/non-positive gain → `Invalid` (defensive; unreachable from
///   the positive-ratio path).
pub fn estimate_pano_gains(
    frames: &[LinearImage],
    matrices: &[[f64; 9]],
) -> Result<PanoGainEstimate, MergeError> {
    if frames.len() < 2 {
        return Err(MergeError::Invalid(format!(
            "pano gain estimation needs at least 2 frames, got {}",
            frames.len()
        )));
    }
    if frames.len() != matrices.len() {
        return Err(MergeError::Invalid(format!(
            "pano gain estimation needs one matrix per frame, got {} frames, {} matrices",
            frames.len(),
            matrices.len()
        )));
    }
    let (w, h) = (frames[0].width(), frames[0].height());
    for (i, frame) in frames.iter().enumerate() {
        if frame.width() != w || frame.height() != h {
            return Err(MergeError::Invalid(format!(
                "pano gain frame #{i} is {}x{}, expected {w}x{h} (no silent rescale)",
                frame.width(),
                frame.height()
            )));
        }
    }
    // Same scope as the transformed blend: translation+rotation-light only.
    let mut inverses = Vec::with_capacity(matrices.len());
    for (i, matrix) in matrices.iter().enumerate() {
        if !is_translation_rotation_light(matrix, 1e-6) {
            return Err(MergeError::Unsupported(format!(
                "pano gain transform #{i} is not translation+rotation-light (1.5 scope)"
            )));
        }
        let inverse = invert_matrix_3x3(matrix).ok_or_else(|| {
            MergeError::Invalid(format!("pano gain transform #{i} is not invertible"))
        })?;
        inverses.push(inverse);
    }
    // Canvas bounds: union of the transformed frame boxes (same construction
    // as `blend_panorama_transformed`, integer canvas pixels).
    let mut bounds = Vec::with_capacity(matrices.len());
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
                MergeError::Invalid(format!("pano gain transform #{i} maps no finite corner"))
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
    let planes: Vec<Vec<f32>> = frames.iter().map(|frame| frame.luminance()).collect();
    let mut gains = vec![1.0; frames.len()];
    let mut min_overlap_px = u64::MAX;
    for i in 1..frames.len() {
        let mut ratios: Vec<f64> = Vec::new();
        let mut overlap_px: u64 = 0;
        for y in 0..ch {
            for x in 0..cw {
                let gx = x as f64 + canvas_min_x;
                let gy = y as f64 + canvas_min_y;
                let (sx0, sy0) = apply_matrix_3x3(&inverses[0], gx, gy).ok_or_else(|| {
                    MergeError::Invalid("pano gain inverse transform is singular".into())
                })?;
                if sx0 < 0.0 || sy0 < 0.0 || sx0 >= w as f64 || sy0 >= h as f64 {
                    continue;
                }
                let (sxi, syi) = apply_matrix_3x3(&inverses[i], gx, gy).ok_or_else(|| {
                    MergeError::Invalid("pano gain inverse transform is singular".into())
                })?;
                if sxi < 0.0 || syi < 0.0 || sxi >= w as f64 || syi >= h as f64 {
                    continue;
                }
                overlap_px += 1;
                let reference = sample_bilinear(&planes[0], w, h, sx0, sy0) as f64;
                let sample = sample_bilinear(&planes[i], w, h, sxi, syi) as f64;
                if !reference.is_finite() || !sample.is_finite() || sample <= GAIN_LUMINANCE_EPS {
                    continue;
                }
                ratios.push(reference / sample);
            }
        }
        if overlap_px < A_MIN_OVERLAP_STAT_PX {
            return Err(MergeError::Unsupported(format!(
                "gain compensation needs at least {A_MIN_OVERLAP_STAT_PX} overlap pixels, got {overlap_px} (frame #{i} vs. reference)"
            )));
        }
        if ratios.is_empty() {
            return Err(MergeError::Unsupported(format!(
                "gain compensation found no usable overlap pixels (frame #{i} vs. reference over {overlap_px}px overlap)"
            )));
        }
        let gain = median_sorted(&mut ratios);
        if !gain.is_finite() || gain <= 0.0 {
            return Err(MergeError::Invalid(format!(
                "pano gain estimate for frame #{i} must be finite and > 0, got {gain}"
            )));
        }
        if gain > MAX_PANO_GAIN {
            return Err(MergeError::Unsupported(format!(
                "pano gain {gain} for frame #{i} exceeds {MAX_PANO_GAIN} — frames differ by more than 3 EV, refusing to guess"
            )));
        }
        gains[i] = gain;
        min_overlap_px = min_overlap_px.min(overlap_px);
    }
    Ok(PanoGainEstimate {
        gains,
        min_overlap_px,
    })
}

/// Median of a non-empty finite slice, via sort (deterministic). Even
/// counts take the mean of the two middle values (statistical median).
fn median_sorted(values: &mut [f64]) -> f64 {
    debug_assert!(!values.is_empty());
    values.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Loud validation of caller-supplied blend gains (Slice A): one finite,
/// positive gain per frame, `gains[0] == 1.0` exactly (reference definition).
/// Never clamped — any deviation is `Invalid`.
fn check_pano_gains(gains: &[f64], frame_count: usize) -> Result<(), MergeError> {
    if gains.len() != frame_count {
        return Err(MergeError::Invalid(format!(
            "panorama blend needs one gain per frame, got {} gains for {frame_count} frames (never padded)",
            gains.len()
        )));
    }
    if gains[0] != 1.0 {
        return Err(MergeError::Invalid(format!(
            "panorama blend gains[0] must be 1.0 (reference definition), got {}",
            gains[0]
        )));
    }
    for (index, gain) in gains.iter().enumerate() {
        if !gain.is_finite() || *gain <= 0.0 {
            return Err(MergeError::Invalid(format!(
                "panorama blend gains[#{index}] must be finite and > 0, got {gain} (never clamped)"
            )));
        }
    }
    Ok(())
}

/// [`blend_panorama`] with Slice-A gain compensation: every sample of frame
/// `i` (including the running-composite seed) is multiplied by `gains[i]`
/// before the feather ramp combines it. See [`check_pano_gains`] for the
/// loud contract.
pub fn blend_panorama_with_gains(
    frames: &[LinearImage],
    offsets_px: &[(i32, i32)],
    blend_width_px: u32,
    gains: &[f64],
) -> Result<LinearImage, MergeError> {
    check_pano_gains(gains, frames.len())?;
    blend_panorama_impl(frames, offsets_px, blend_width_px, Some(gains))
}

/// [`blend_panorama_transformed`] with Slice-A gain compensation. See
/// [`check_pano_gains`] for the loud contract (never clamped).
pub fn blend_panorama_transformed_with_gains(
    frames: &[LinearImage],
    matrices: &[[f64; 9]],
    blend_width_px: u32,
    gains: &[f64],
) -> Result<LinearImage, MergeError> {
    check_pano_gains(gains, frames.len())?;
    blend_panorama_transformed_impl(frames, matrices, blend_width_px, Some(gains))
}

/// Inverse of a row-major 3x3 matrix; `None` when numerically singular.
pub(crate) fn invert_matrix_3x3(matrix: &[f64; 9]) -> Option<[f64; 9]> {
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
}
