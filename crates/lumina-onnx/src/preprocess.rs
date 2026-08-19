//! Pure preprocessing / postprocessing helpers for the adapter boundary.
//!
//! These functions are deliberately deterministic and dependency-free so they
//! can be unit-tested without models or network. A real ONNX Runtime backend
//! performs the model's *own* preprocessing internally; these helpers exist for
//! the adapter contract (resize to inference resolution, rescale the emitted
//! matte back to source dimensions) and are shared by the stub and ORT backends.

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

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::ImageFrame;

    fn frame(width: u32, height: u32, rgb: [u8; 3]) -> ImageFrame {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for p in pixels.chunks_exact_mut(4) {
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
}
