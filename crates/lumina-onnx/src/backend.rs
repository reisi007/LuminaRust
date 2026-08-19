//! Exchangeable inference surface and the deterministic stub backend.

use crate::manifest::ModelManifest;
use crate::preprocess::{preprocess_rgb_to_model, rescale_model_matte};
use crate::OnnxError;
use lumina_core::{ImageFrame, MaskPlane};

/// Exchangeable subject-mask inference surface.
///
/// Every backend (the deterministic [`StubBackend`], a future real ONNX Runtime
/// backend, or a SAM 2 backend) implements this same trait, so the adapter
/// stays model-agnostic and `lumina-core` is never coupled to a concrete model.
pub trait SubjectInference {
    /// The model manifest this backend was built from.
    fn manifest(&self) -> &ModelManifest;

    /// Whether the model artifact/weights required for inference are present.
    /// The default (`true`) suits backends without external weight files; a real
    /// ONNX Runtime backend overrides this to check for the `.onnx` artifact.
    fn is_available(&self) -> bool {
        true
    }

    /// Run subject inference for `image`, returning a matte at the source image
    /// resolution. Must never silently fall back on a missing or mismatched
    /// artifact — it returns [`OnnxError`] instead.
    fn infer(&self, image: &ImageFrame) -> Result<MaskPlane, OnnxError>;
}

/// Deterministic, dependency-free backend. It is the complete default surface:
/// it needs no model weights and no network, and it is the basis for the
/// adapter's tests.
///
/// The matte is a centered radial/elliptic alpha disk derived **purely** from
/// the inference resolution — see [`StubBackend::generate_model_matte`].
pub struct StubBackend {
    manifest: ModelManifest,
    /// Whether the model weights/artifact required for inference are present.
    /// The stub has no weights, so this is `true` by default; it can be flipped
    /// to simulate an unavailable model (F-051) for testing.
    available: bool,
}

impl StubBackend {
    /// Build a stub backend from a validated manifest.
    pub fn new(manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        let res = manifest.input.resolution;
        if res.width == 0 || res.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: res.width,
                expected_height: res.height,
                actual_width: res.width,
                actual_height: res.height,
            });
        }
        Ok(Self {
            manifest,
            available: true,
        })
    }

    /// Override the reported model availability. Used to simulate a missing
    /// model (F-051) without a real weights file.
    pub fn with_availability(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Whether this backend can currently perform inference.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Generate the model-resolution matte: a centered radial disk in normalized
    /// coordinate space.
    ///
    /// For pixel `(x, y)` in a `W×H` grid:
    /// ```text
    /// cx = (W - 1) / 2,  cy = (H - 1) / 2
    /// dx = (x - cx) / (W / 2),  dy = (y - cy) / (H / 2)   // edges at ±1
    /// r  = sqrt(dx² + dy²)
    /// alpha = clamp(1 - r, 0, 1)
    /// value = round(alpha * 65535)
    /// ```
    /// The center pixel maps to `65535`, corners where `r >= 1` map to `0`. The
    /// formula is fully deterministic and free of any model weight or seed.
    pub fn generate_model_matte(&self) -> Result<MaskPlane, OnnxError> {
        let res = self.manifest.input.resolution;
        let (w, h) = (res.width as i64, res.height as i64);
        let cx = (w - 1) as f64 / 2.0;
        let cy = (h - 1) as f64 / 2.0;
        let half_w = (w as f64) / 2.0;
        let half_h = (h as f64) / 2.0;
        let mut values = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let dx = (x as f64 - cx) / half_w;
                let dy = (y as f64 - cy) / half_h;
                let r = (dx * dx + dy * dy).sqrt();
                let alpha = (1.0 - r).clamp(0.0, 1.0);
                values.push((alpha * 65535.0).round() as u16);
            }
        }
        MaskPlane::new(res.width, res.height, values).map_err(|_| OnnxError::InvalidDimensions {
            expected_width: res.width,
            expected_height: res.height,
            actual_width: res.width,
            actual_height: res.height,
        })
    }
}

impl SubjectInference for StubBackend {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn infer(&self, image: &ImageFrame) -> Result<MaskPlane, OnnxError> {
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        // Exercise the real adapter preprocessing path (documented boundary);
        // the stub does not consume the resized RGB, but the dimension contract
        // is validated and shared with the ORT backend.
        let _rgb = preprocess_rgb_to_model(image, self.manifest.input.resolution);
        let model_matte = self.generate_model_matte()?;
        rescale_model_matte(
            &model_matte,
            self.manifest.input.resolution,
            (image.width, image.height),
        )
    }
}

/// Bridge the native adapter to `lumina-core`'s platform-neutral
/// [`lumina_core::MaskInference`] trait, mapping adapter errors to
/// [`lumina_core::CoreError`] (no silent fallbacks). This is the surface the
/// mask-loading decision layer (F-048 / F-051) consumes.
impl lumina_core::MaskInference for StubBackend {
    fn is_available(&self) -> bool {
        self.available
    }

    fn infer(&self, frame: &ImageFrame) -> Result<MaskPlane, lumina_core::CoreError> {
        <Self as SubjectInference>::infer(self, frame).map_err(|error| {
            lumina_core::CoreError::MaskInference {
                reason: error.to_string(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::birefnet_manifest;

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
    fn stub_is_deterministic_and_centered() {
        let backend = StubBackend::new(birefnet_manifest()).unwrap();
        let img = frame(640, 480, [120, 80, 200]);
        let a = backend.infer(&img).unwrap();
        let b = backend.infer(&img).unwrap();
        assert_eq!(a.width, 640);
        assert_eq!(a.height, 480);
        assert_eq!(a.values, b.values, "stub must be byte-identical");
        let center = (a.height as usize / 2) * a.width as usize + a.width as usize / 2;
        assert!(a.values[center] > a.values[0], "center must be brighter");
    }

    #[test]
    fn stub_matte_spans_full_range_at_inference_res() {
        let backend = StubBackend::new(birefnet_manifest()).unwrap();
        let matte = backend.generate_model_matte().unwrap();
        assert_eq!(
            (matte.width, matte.height),
            (
                crate::BIREFNET_INFERENCE_WIDTH,
                crate::BIREFNET_INFERENCE_HEIGHT
            )
        );
        let min = *matte.values.iter().min().unwrap();
        let max = *matte.values.iter().max().unwrap();
        // Corners are fully transparent; the center is near-opaque. (An even
        // grid has no pixel exactly at the geometric center, so the peak is
        // ~65434 rather than exactly 65535 — still full usable range.)
        assert_eq!(min, 0, "corners must be transparent");
        assert!(max >= 65000, "center must be near-opaque, got {max}");
    }

    #[test]
    fn infer_rejects_zero_dimension_input() {
        let backend = StubBackend::new(birefnet_manifest()).unwrap();
        let img = ImageFrame::new(0, 0, vec![]).unwrap();
        let err = backend.infer(&img).unwrap_err();
        assert!(
            matches!(err, OnnxError::InvalidDimensions { .. }),
            "{err:?}"
        );
    }
}
