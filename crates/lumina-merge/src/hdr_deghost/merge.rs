use super::deghost_mask;
use crate::merge::relative_exposure;
use crate::{LinearImage, MergeError};
use lumina_core::merge_geom::{clamp_linear, hdr_hat_weight, sample_bilinear};
use lumina_sidecar::{HdrDeghost, MergeExposure};

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
