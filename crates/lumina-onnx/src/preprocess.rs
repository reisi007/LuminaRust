//! Pure preprocessing / postprocessing helpers for the adapter boundary.
//!
//! These functions are deliberately deterministic and dependency-free so they
//! can be unit-tested without models or network. A real ONNX Runtime backend
//! performs the model's *own* preprocessing internally; these helpers exist for
//! the adapter contract (resize to inference resolution, rescale the emitted
//! matte back to source dimensions) and are shared by the stub and ORT backends.

use crate::manifest::InputNormalization;
use crate::{OnnxError, Resolution};
use lumina_core::MaskError;
use lumina_core::MaskPlane;

/// Deterministic **nearest-neighbor** resize of the RGB channels of `image` to
/// `target` resolution, returning `3 * width * height` bytes (R,G,B per pixel,
/// no alpha).
///
/// We choose nearest-neighbor with integer coordinate mapping
/// `src = (dst * src_dim) / dst_dim` (rounding toward zero) over bilinear
/// because it is exactly reproducible with no floating-point rounding ambiguity
/// across platforms and compiler versions — important for a deterministic,
/// dependency-free test surface. A real backend uses the model's own
/// preprocessing; this is the documented adapter boundary.
pub fn preprocess_rgb_to_model(image: &lumina_core::ImageFrame, target: Resolution) -> Vec<u8> {
    let (sw, sh) = (image.width as usize, image.height as usize);
    let (dw, dh) = (target.width as usize, target.height as usize);
    let mut out = vec![0u8; dw * dh * 3];
    for y in 0..dh {
        let sy = if dh <= 1 { 0 } else { (y * sh) / dh };
        for x in 0..dw {
            let sx = if dw <= 1 { 0 } else { (x * sw) / dw };
            let src = (sy * sw + sx) * 4;
            let dst = (y * dw + x) * 3;
            out[dst] = image.pixels[src];
            out[dst + 1] = image.pixels[src + 1];
            out[dst + 2] = image.pixels[src + 2];
        }
    }
    out
}

/// Rescale a model-resolution [`MaskPlane`] (produced at `inference`
/// resolution) back to the source `source` dimensions using deterministic
/// nearest-neighbor (same integer mapping as [`preprocess_rgb_to_model`]).
///
/// Returns [`OnnxError::InvalidDimensions`] when the model plane dimensions
/// disagree with the declared `inference` resolution — this guards the
/// "stub/ORT input vs. inference resolution mismatch" contract: the emitted
/// matte must match what the manifest claims, otherwise the result would be a
/// silent misresize.
pub fn rescale_model_matte(
    model: &MaskPlane,
    inference: Resolution,
    source: (u32, u32),
) -> Result<MaskPlane, OnnxError> {
    if model.width != inference.width || model.height != inference.height {
        return Err(OnnxError::InvalidDimensions {
            expected_width: inference.width,
            expected_height: inference.height,
            actual_width: model.width,
            actual_height: model.height,
        });
    }
    let (dw, dh) = (source.0 as usize, source.1 as usize);
    if dw == 0 || dh == 0 {
        return Err(OnnxError::InvalidDimensions {
            expected_width: inference.width,
            expected_height: inference.height,
            actual_width: source.0,
            actual_height: source.1,
        });
    }
    let (sw, sh) = (inference.width as usize, inference.height as usize);
    let mut values = vec![0u16; dw * dh];
    for y in 0..dh {
        let sy = if dh <= 1 { 0 } else { (y * sh) / dh };
        for x in 0..dw {
            let sx = if dw <= 1 { 0 } else { (x * sw) / dw };
            values[y * dw + x] = model.values[sy * sw + sx];
        }
    }
    MaskPlane::new(source.0, source.1, values).map_err(invalid_dimensions)
}

fn invalid_dimensions(_: MaskError) -> OnnxError {
    OnnxError::InvalidDimensions {
        expected_width: 0,
        expected_height: 0,
        actual_width: 0,
        actual_height: 0,
    }
}

/// Normalize interleaved RGB bytes (as produced by
/// [`preprocess_rgb_to_model`]) into a **CHW-ordered** `f32` vector using the
/// manifest's per-channel mean/std: `value = (byte / 255 - mean[c]) / std[c]`
/// (REVIEW-ONNX-PREPROC-1).
///
/// The output layout is channels-first (`[R plane, G plane, B plane]`) to
/// match the manifest's `Nchw` tensor format; the previous adapter code wrote
/// interleaved data into an NCHW-declared tensor, which this function fixes
/// ahead of real weight integration. Fails with
/// [`OnnxError::InferenceFailed`] when `rgb` is not a multiple of 3 (never
/// silently truncates).
pub fn normalize_rgb_to_nchw(
    rgb_interleaved: &[u8],
    model_name: &str,
    norm: &InputNormalization,
) -> Result<Vec<f32>, OnnxError> {
    if !rgb_interleaved.len().is_multiple_of(3) {
        return Err(OnnxError::InferenceFailed {
            name: model_name.to_owned(),
            reason: format!(
                "preprocessing input must contain whole RGB pixels (multiple of 3 bytes), got {}",
                rgb_interleaved.len()
            ),
        });
    }
    let pixels = rgb_interleaved.len() / 3;
    let mut out = vec![0f32; pixels * 3];
    for c in 0..3 {
        let mean = norm.mean[c];
        let std = norm.std[c];
        let plane = &mut out[c * pixels..(c + 1) * pixels];
        for (p, plane_value) in plane.iter_mut().enumerate() {
            let raw = rgb_interleaved[p * 3 + c];
            *plane_value = (raw as f32 / 255.0 - mean) / std;
        }
    }
    Ok(out)
}

/// Convert a raw model output plane of `[0, 1]` probabilities into `u16`
/// grayscale matte values: `value = round(clamp(v, 0, 1) * 65535)`.
///
/// Values outside `[0, 1]` are clamped (documented boundary behavior); NaN
/// maps to `0` via Rust's saturating float→int cast.
pub fn matte_values_from_unit_f32(values: &[f32]) -> Vec<u16> {
    values
        .iter()
        .map(|&v| ((v.clamp(0.0, 1.0) * 65535.0).round() as i32).clamp(0, 65535) as u16)
        .collect()
}

/// Validate the shape of a model's primary output tensor against the declared
/// inference resolution (REVIEW-ONNX-PREPROC-1 — the output shape was
/// previously unchecked).
///
/// Accepted shapes for a single-image matte:
/// * rank 4: `[batch=1, channels=1, H=res.height, W=res.width]`
/// * rank 3: `[batch=1, H=res.height, W=res.width]`
///
/// Anything else is [`OnnxError::InferenceFailed`] — a differently shaped
/// output must never be silently reshaped or truncated into a matte.
pub fn validate_output_shape(
    shape: &[i64],
    res: Resolution,
    model_name: &str,
) -> Result<(), OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    let expected = format!(
        "[1, 1, {}, {}] (or [1, {}, {}])",
        res.height, res.width, res.height, res.width
    );
    match shape {
        [b, h, w] if *b == 1 => {
            if *h != res.height as i64 || *w != res.width as i64 {
                return Err(fail(format!(
                    "output spatial dims {h}x{w} do not match inference resolution {}x{}",
                    res.height, res.width
                )));
            }
            Ok(())
        }
        [b, c, h, w] if *b == 1 => {
            if *c != 1 {
                return Err(fail(format!(
                    "output channel count must be 1 for a matte, got {c} (expected {expected})"
                )));
            }
            if *h != res.height as i64 || *w != res.width as i64 {
                return Err(fail(format!(
                    "output spatial dims {h}x{w} do not match inference resolution {}x{}",
                    res.height, res.width
                )));
            }
            Ok(())
        }
        _ => Err(fail(format!(
            "unexpected output rank {}: expected {expected}",
            shape.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::ImageFrame;

    fn frame(width: u32, height: u32, rgb: [u8; 3]) -> ImageFrame {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for p in pixels.as_chunks_mut::<4>().0 {
            p[0] = rgb[0];
            p[1] = rgb[1];
            p[2] = rgb[2];
            p[3] = 255;
        }
        ImageFrame::new(width, height, pixels).unwrap()
    }

    #[test]
    fn preprocess_dimensions_and_determinism() {
        let img = frame(2, 2, [120, 80, 200]);
        let a = preprocess_rgb_to_model(
            &img,
            Resolution {
                width: 4,
                height: 4,
            },
        );
        let b = preprocess_rgb_to_model(
            &img,
            Resolution {
                width: 4,
                height: 4,
            },
        );
        assert_eq!(a.len(), 4 * 4 * 3);
        assert_eq!(a, b);
        // top-left source pixel (120,80,200) maps to top-left of the 4x4 output
        assert_eq!(&a[0..3], &[120, 80, 200]);
    }

    #[test]
    fn rescale_preserves_corners_nearest() {
        let model = MaskPlane::new(2, 2, vec![65535, 100, 200, 0]).unwrap();
        let out = rescale_model_matte(
            &model,
            Resolution {
                width: 2,
                height: 2,
            },
            (4, 4),
        )
        .unwrap();
        assert_eq!((out.width, out.height), (4, 4));
        assert_eq!(out.values.len(), 16);
        // nearest: source (0,0) -> model (0,0); source (3,3) -> model (1,1)
        assert_eq!(out.values[0], 65535);
        assert_eq!(out.values[15], 0);
    }

    #[test]
    fn rescale_rejects_inference_mismatch() {
        let model = MaskPlane::new(4, 4, vec![0u16; 16]).unwrap();
        let err = rescale_model_matte(
            &model,
            Resolution {
                width: 2,
                height: 2,
            },
            (8, 8),
        )
        .unwrap_err();
        assert!(
            matches!(err, OnnxError::InvalidDimensions { .. }),
            "{err:?}"
        );
    }

    // REVIEW-ONNX-PREPROC-1 — normalization comes from the manifest and the
    // output is CHW-ordered.
    #[test]
    fn normalize_applies_manifest_mean_std_in_chw_order() {
        use crate::manifest::InputNormalization;
        let norm = InputNormalization {
            mean: [0.5, 0.25, 0.75],
            std: [0.5, 0.25, 1.0],
        };
        // One pixel: R=255, G=0, B=128.
        let out = normalize_rgb_to_nchw(&[255, 0, 128], "M", &norm).unwrap();
        assert_eq!(out.len(), 3);
        assert!((out[0] - (1.0 - 0.5) / 0.5).abs() < 1e-6, "{out:?}");
        assert!((out[1] - (0.0 - 0.25) / 0.25).abs() < 1e-6, "{out:?}");
        assert!((out[2] - (128.0 / 255.0 - 0.75)).abs() < 1e-6, "{out:?}");

        // Two pixels must produce channel planes (CHW), not interleaving:
        // plane R = [p0.R, p1.R], plane G = [p0.G, p1.G], …
        let out = normalize_rgb_to_nchw(&[10, 20, 30, 40, 50, 60], "M", &norm).unwrap();
        assert_eq!(out.len(), 6);
        assert!((out[0] - (10.0 / 255.0 - 0.5) / 0.5).abs() < 1e-6);
        assert!(
            (out[1] - (40.0 / 255.0 - 0.5) / 0.5).abs() < 1e-6,
            "R plane second pixel"
        );
        assert!(
            (out[2] - (20.0 / 255.0 - 0.25) / 0.25).abs() < 1e-6,
            "G plane first pixel"
        );
    }

    #[test]
    fn normalize_rejects_non_pixel_input() {
        use crate::manifest::InputNormalization;
        let err = normalize_rgb_to_nchw(&[1, 2, 3, 4], "BiRefNet", &InputNormalization::IMAGENET)
            .unwrap_err();
        assert!(matches!(err, OnnxError::InferenceFailed { .. }), "{err:?}");
    }

    #[test]
    fn matte_values_clamp_and_scale() {
        assert_eq!(
            matte_values_from_unit_f32(&[-1.0, 0.0, 0.5, 1.0, 2.0, f32::NAN]),
            vec![0, 0, ((0.5f32 * 65535.0).round()) as u16, 65535, 65535, 0]
        );
    }

    #[test]
    fn output_shape_accepts_matching_rank4_and_rank3() {
        let res = Resolution {
            width: 1024,
            height: 1024,
        };
        assert!(validate_output_shape(&[1, 1, 1024, 1024], res, "M").is_ok());
        assert!(validate_output_shape(&[1, 1024, 1024], res, "M").is_ok());
        let small = Resolution {
            width: 320,
            height: 240,
        };
        assert!(validate_output_shape(&[1, 1, 240, 320], small, "M").is_ok());
    }

    #[test]
    fn output_shape_rejects_mismatches() {
        let res = Resolution {
            width: 1024,
            height: 1024,
        };
        // Wrong spatial dims.
        let err = validate_output_shape(&[1, 1, 512, 512], res, "M").unwrap_err();
        assert!(
            err.to_string()
                .contains("do not match inference resolution"),
            "{err}"
        );
        // Multi-channel output is not a matte.
        let err = validate_output_shape(&[1, 3, 1024, 1024], res, "M").unwrap_err();
        assert!(err.to_string().contains("channel count"), "{err}");
        // Wrong batch.
        let err = validate_output_shape(&[2, 1, 1024, 1024], res, "M").unwrap_err();
        assert!(matches!(err, OnnxError::InferenceFailed { .. }), "{err:?}");
        // Wrong rank.
        let err = validate_output_shape(&[1024 * 1024], res, "M").unwrap_err();
        assert!(err.to_string().contains("unexpected output rank"), "{err}");
    }
}
