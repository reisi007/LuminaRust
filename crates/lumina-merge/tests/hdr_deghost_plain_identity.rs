//! Slice-B honesty anchor for the no-mask path.
//!
//! The `deghost == None` spelling of `merge_hdr_weighted_deghost` must be
//! BIT-IDENTICAL to the pre-slice merge. The pre-slice body below is the
//! verbatim implementation from `main` at `1d42acf`
//! (`git show 1d42acf:crates/lumina-merge/src/merge.rs`), before the HDR
//! pass moved into `hdr_deghost.rs` with the ghost mask. Only the name
//! differs (`merge_hdr_weighted_pre_slice`).
//!
//! A comparison of the current `None` path against the public
//! `merge_hdr_weighted` wrapper CANNOT prove the identity — since Slice B
//! that wrapper calls the very same function. This reference comparison
//! kills every mutation of the `None` path: a changed `contributing`
//! count, a reordered accumulation, a stray float comparison, a
//! re-introduced per-pixel plane extraction — each one moves these bytes.

use lumina_core::merge_geom::{clamp_linear, hdr_hat_weight, sample_bilinear};
use lumina_merge::hdr_deghost::merge_hdr_weighted_deghost;
use lumina_merge::merge::relative_exposure;
use lumina_merge::{LinearImage, MergeError};
use lumina_sidecar::MergeExposure;

/// Weighted linear HDR merge of same-size frames — verbatim pre-Slice-B
/// body (validation, plane pre-extraction, pixel loop). `git show
/// 1d42acf:crates/lumina-merge/src/merge.rs`.
fn merge_hdr_weighted_pre_slice(
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
    // (O(frames * pixels)); the same pattern as `blend_panorama`.
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

fn exposure(time: f64) -> MergeExposure {
    MergeExposure {
        exposure_time_s: time,
        iso: 100,
        f_number: 8.0,
    }
}

/// Three same-size frames whose radiance agrees after exposure
/// normalisation, with a sub-pixel shift (exercises `sample_bilinear`)
/// and a horizontal ramp (exercises `hdr_hat_weight` across its range).
fn agreeing_frames() -> (Vec<LinearImage>, Vec<MergeExposure>, Vec<(f64, f64)>) {
    let (w, h) = (32u32, 24u32);
    let mut frame_b = vec![0.0f32; w as usize * h as usize * 3];
    for y in 0..h {
        for x in 0..w {
            let value = 0.5 * (x as f32) / (w - 1) as f32;
            let index = (y * w * 3 + x * 3) as usize;
            frame_b[index..index + 3].fill(value);
        }
    }
    let frames = vec![
        LinearImage::solid(w, h, [0.4, 0.4, 0.4]),
        LinearImage::new(w, h, frame_b).expect("valid frame"),
        LinearImage::solid(w, h, [0.6, 0.6, 0.6]),
    ];
    let exposures = vec![exposure(0.01), exposure(0.02), exposure(0.005)];
    let shifts = vec![(0.0, 0.0), (0.3, 0.0), (0.0, 0.2)];
    (frames, exposures, shifts)
}

#[test]
fn none_path_is_bit_identical_to_the_pre_slice_merge() {
    let (frames, exposures, shifts) = agreeing_frames();
    let current = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).expect("merge");
    let reference = merge_hdr_weighted_pre_slice(&frames, &exposures, &shifts).expect("merge");
    assert_eq!(
        current.pixels(),
        reference.pixels(),
        "deghost == None must reproduce the pre-slice merge bit for bit"
    );
}

#[test]
fn none_path_matches_pre_slice_on_clipped_pixels() {
    // Fully clipped frames: the fallback branch (`den == 0`, mean
    // radiance) must also be byte-identical — the mask-less pass may not
    // reinterpret it.
    let (w, h) = (16u32, 16u32);
    let frames = vec![
        LinearImage::solid(w, h, [0.0, 0.0, 0.0]),
        LinearImage::solid(w, h, [0.0, 0.0, 0.0]),
    ];
    let exposures = vec![exposure(0.01), exposure(0.02)];
    let shifts = vec![(0.0, 0.0), (0.0, 0.0)];
    let current = merge_hdr_weighted_deghost(&frames, &exposures, &shifts, None).expect("merge");
    let reference = merge_hdr_weighted_pre_slice(&frames, &exposures, &shifts).expect("merge");
    assert_eq!(current.pixels(), reference.pixels());
}
