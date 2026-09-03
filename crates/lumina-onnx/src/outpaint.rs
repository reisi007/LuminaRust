//! Deterministic generative outpaint stub (GEN-EXPAND-1, local ONNX).
//!
//! [`StubOutpaintBackend`] expands an RGBA image onto a larger canvas
//! (`canvas.output_* > source_*` on at least one edge, ">100 %" per
//! `feature/product/generative-expand.md`): source pixels are copied to
//! `source_offset`, the new border is filled deterministically from the
//! source mean plus a prompt/seed/canvas hash offset.
//!
//! This is the complete, tested default surface — **no weights, no network**.
//! A backend with `available == false` reports
//! [`OnnxError::ModelUnavailable`](crate::OnnxError::ModelUnavailable)
//! visibly (no silent fallback to inpaint or to "render without the
//! generative stage"). A manifest without the `outpaint` capability is
//! rejected with `UnsupportedModel` (capability gating, read from the
//! manifest — never guessed from the model name).

use crate::{manifest::outpaint_expand_manifest, ModelManifest, OnnxError};

/// Target canvas geometry for an outpaint expansion.
///
/// `output_*` is the new canvas size in pixels; `source_offset_*` places the
/// source's top-left corner inside the canvas (translation source -> canvas,
/// deterministic). Negative offsets are allowed while the source still lands
/// inside the canvas (`0 <= offset + source_dim <= output_dim` per
/// `feature/product/generative-expand.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutpaintCanvas {
    pub output_width: u32,
    pub output_height: u32,
    pub source_offset_x: i32,
    pub source_offset_y: i32,
}

impl OutpaintCanvas {
    /// Validate this canvas against a source of `source_width` x
    /// `source_height`.
    ///
    /// Violations surface as [`OnnxError::InvalidDimensions`] with
    /// `expected_*` carrying the requested canvas size and `actual_*` the
    /// source size — visible errors, never a silent clamp or crop.
    pub fn validate(&self, source_width: u32, source_height: u32) -> Result<(), OnnxError> {
        let expected_width = self.output_width;
        let expected_height = self.output_height;
        let invalid = || OnnxError::InvalidDimensions {
            expected_width,
            expected_height,
            actual_width: source_width,
            actual_height: source_height,
        };
        if self.output_width == 0 || self.output_height == 0 {
            return Err(invalid());
        }
        // Outpaint requires expansion beyond the original surface on at
        // least one edge (">100 %"); pure in-place work belongs to inpaint.
        if self.output_width <= source_width && self.output_height <= source_height {
            return Err(invalid());
        }
        let fits = |offset: i32, source: u32, output: u32| -> bool {
            let end = offset as i64 + source as i64;
            end >= 0 && end <= output as i64 && (offset as i64) < output as i64
        };
        if !fits(self.source_offset_x, source_width, self.output_width)
            || !fits(self.source_offset_y, source_height, self.output_height)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Generative outpaint request: prompt identity plus target canvas.
///
/// `prompt` (possibly empty, roundtrip-stable), `negative_prompt`
/// (`None`/absent is identity, never implicitly empty) and `seed` belong to
/// the operation identity alongside the canvas geometry: identical
/// source + model context + prompt + seed + canvas yields a byte-identical
/// canvas.
#[derive(Debug, Clone)]
pub struct OutpaintRequest {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub seed: u64,
    pub canvas: OutpaintCanvas,
}

/// Deterministic stub outpaint backend (no weights, no network).
pub struct StubOutpaintBackend {
    pub available: bool,
}

impl Default for StubOutpaintBackend {
    fn default() -> Self {
        Self { available: true }
    }
}

impl StubOutpaintBackend {
    pub fn manifest() -> ModelManifest {
        outpaint_expand_manifest()
    }

    /// Expand `image` (`width` x `height` RGBA8) onto the request canvas.
    pub fn expand(
        &self,
        image: &[u8],
        width: u32,
        height: u32,
        request: &OutpaintRequest,
    ) -> Result<Vec<u8>, OnnxError> {
        self.expand_with_manifest(image, width, height, request, &Self::manifest())
    }

    fn expand_with_manifest(
        &self,
        image: &[u8],
        width: u32,
        height: u32,
        request: &OutpaintRequest,
        manifest: &ModelManifest,
    ) -> Result<Vec<u8>, OnnxError> {
        if !self.available {
            return Err(OnnxError::ModelUnavailable {
                name: manifest.model_name.clone(),
            });
        }
        if image.len() != width as usize * height as usize * 4 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: width,
                expected_height: height,
                actual_width: width,
                actual_height: height,
            });
        }
        request.canvas.validate(width, height)?;
        if !manifest.capabilities.outpaint {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "outpaint not declared".into(),
            });
        }
        let canvas = request.canvas;
        let out_w = canvas.output_width as usize;
        let out_h = canvas.output_height as usize;

        let mut sum = [0u64; 3];
        for px in image.as_chunks::<4>().0 {
            sum[0] += px[0] as u64;
            sum[1] += px[1] as u64;
            sum[2] += px[2] as u64;
        }
        let count = (width as u64 * height as u64).max(1);
        let mean = [
            (sum[0] / count) as u8,
            (sum[1] / count) as u8,
            (sum[2] / count) as u8,
        ];

        let mut seed_hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            request.prompt.hash(&mut h);
            request.negative_prompt.hash(&mut h);
            request.seed.hash(&mut h);
            canvas.output_width.hash(&mut h);
            canvas.output_height.hash(&mut h);
            canvas.source_offset_x.hash(&mut h);
            canvas.source_offset_y.hash(&mut h);
            h.finish()
        };
        let r_off = (seed_hash & 0xFF) as i16 - 128;
        seed_hash >>= 8;
        let g_off = (seed_hash & 0xFF) as i16 - 128;
        seed_hash >>= 8;
        let b_off = (seed_hash & 0xFF) as i16 - 128;
        let fill = [
            (mean[0] as i16 + r_off).clamp(0, 255) as u8,
            (mean[1] as i16 + g_off).clamp(0, 255) as u8,
            (mean[2] as i16 + b_off).clamp(0, 255) as u8,
        ];

        let mut out = vec![0u8; out_w * out_h * 4];
        for y in 0..out_h {
            for x in 0..out_w {
                let base = (y * out_w + x) * 4;
                out[base + 3] = 255;
            }
        }
        // Copy the source block at its canvas offset (clipped to the canvas;
        // validation above guarantees a non-empty overlap).
        for y in 0..height as usize {
            for x in 0..width as usize {
                let dx = x as i64 + canvas.source_offset_x as i64;
                let dy = y as i64 + canvas.source_offset_y as i64;
                if dx < 0 || dy < 0 || dx >= out_w as i64 || dy >= out_h as i64 {
                    continue;
                }
                let src = (y * width as usize + x) * 4;
                let dst = (dy as usize * out_w + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&image[src..src + 4]);
            }
        }
        // Deterministic fill for every pixel outside the source block.
        for y in 0..out_h {
            for x in 0..out_w {
                let dx = x as i64 - canvas.source_offset_x as i64;
                let dy = y as i64 - canvas.source_offset_y as i64;
                let inside_source = dx >= 0 && dy >= 0 && dx < width as i64 && dy < height as i64;
                if !inside_source {
                    let base = (y * out_w + x) * 4;
                    out[base] = fill[0];
                    out[base + 1] = fill[1];
                    out[base + 2] = fill[2];
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_canvas() -> OutpaintCanvas {
        OutpaintCanvas {
            output_width: 6,
            output_height: 4,
            source_offset_x: 1,
            source_offset_y: 0,
        }
    }

    fn expand_request(seed: u64) -> OutpaintRequest {
        OutpaintRequest {
            prompt: "extend the sky to the right".into(),
            negative_prompt: None,
            seed,
            canvas: expand_canvas(),
        }
    }

    #[test]
    fn deterministic_expand_same_inputs_byte_identical() {
        let backend = StubOutpaintBackend::default();
        // 4x4 source, RGBA8.
        let image = vec![100u8; 4 * 4 * 4];
        let req = expand_request(42);
        let a = backend.expand(&image, 4, 4, &req).unwrap();
        let b = backend.expand(&image, 4, 4, &req).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 6 * 4 * 4);

        // Seed is identity: a different seed must change the border fill.
        let seeded = OutpaintRequest {
            seed: 43,
            ..req.clone()
        };
        assert_ne!(a, backend.expand(&image, 4, 4, &seeded).unwrap());

        // Prompt is identity too.
        let prompted = OutpaintRequest {
            prompt: "extend the sea".into(),
            ..req.clone()
        };
        assert_ne!(a, backend.expand(&image, 4, 4, &prompted).unwrap());

        // Negative prompt participates in the identity as well.
        let negated = OutpaintRequest {
            negative_prompt: Some("blurry".into()),
            ..req.clone()
        };
        assert_ne!(a, backend.expand(&image, 4, 4, &negated).unwrap());

        // Canvas geometry is identity: another expand region differs.
        let moved = OutpaintRequest {
            canvas: OutpaintCanvas {
                output_width: 8,
                ..expand_canvas()
            },
            ..req.clone()
        };
        let c = backend.expand(&image, 4, 4, &moved).unwrap();
        assert_eq!(c.len(), 8 * 4 * 4);
        assert_ne!(a, c);
    }

    #[test]
    fn source_block_is_preserved_at_offset() {
        let backend = StubOutpaintBackend::default();
        let (w, h) = (4u32, 4u32);
        let mut image = vec![0u8; (w * h * 4) as usize];
        for (i, px) in image.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = (i * 7 % 256) as u8;
            px[1] = (i * 13 % 256) as u8;
            px[2] = (i * 29 % 256) as u8;
            px[3] = 255;
        }
        let req = expand_request(1);
        let out = backend.expand(&image, w, h, &req).unwrap();
        let ox = req.canvas.source_offset_x as usize;
        let oy = req.canvas.source_offset_y as usize;
        let out_w = req.canvas.output_width as usize;
        // Every source pixel reappears verbatim at (x + offset, y + offset).
        for y in 0..h as usize {
            for x in 0..w as usize {
                let src = (y * w as usize + x) * 4;
                let dst = ((y + oy) * out_w + (x + ox)) * 4;
                assert_eq!(&out[dst..dst + 4], &image[src..src + 4]);
            }
        }
        // The border is opaque and was actually generated (not left zero).
        assert_eq!(out[3], 255);
        assert!(out[0] != 0 || out[1] != 0 || out[2] != 0);
    }

    #[test]
    fn unavailable_model_reported_visible_not_silent() {
        let backend = StubOutpaintBackend { available: false };
        let image = vec![100u8; 4 * 4 * 4];
        let req = expand_request(42);
        assert!(matches!(
            backend.expand(&image, 4, 4, &req),
            Err(OnnxError::ModelUnavailable { .. })
        ));
    }

    #[test]
    fn capability_gate_rejects_model_without_outpaint() {
        let backend = StubOutpaintBackend::default();
        let image = vec![100u8; 4 * 4 * 4];
        let req = expand_request(42);
        let mut manifest = StubOutpaintBackend::manifest();
        manifest.capabilities.outpaint = false;
        manifest.capabilities.inpaint_heal = true;
        let err = backend
            .expand_with_manifest(&image, 4, 4, &req, &manifest)
            .unwrap_err();
        assert!(
            matches!(err, OnnxError::UnsupportedModel { .. }),
            "a model without `outpaint` must be rejected visibly, got {err:?}"
        );
    }

    #[test]
    fn canvas_without_expansion_is_rejected() {
        let backend = StubOutpaintBackend::default();
        let image = vec![100u8; 4 * 4 * 4];
        // Canvas == source is inpaint territory, not outpaint.
        let req = OutpaintRequest {
            canvas: OutpaintCanvas {
                output_width: 4,
                output_height: 4,
                source_offset_x: 0,
                source_offset_y: 0,
            },
            ..expand_request(42)
        };
        assert!(matches!(
            backend.expand(&image, 4, 4, &req),
            Err(OnnxError::InvalidDimensions { .. })
        ));
    }

    #[test]
    fn canvas_out_of_bounds_is_rejected_not_clamped() {
        let backend = StubOutpaintBackend::default();
        let image = vec![100u8; 4 * 4 * 4];
        let req = OutpaintRequest {
            canvas: OutpaintCanvas {
                output_width: 6,
                output_height: 6,
                source_offset_x: 5,
                source_offset_y: 0,
            },
            ..expand_request(42)
        };
        assert!(matches!(
            backend.expand(&image, 4, 4, &req),
            Err(OnnxError::InvalidDimensions { .. })
        ));
    }

    #[test]
    fn negative_offset_within_bounds_is_accepted() {
        let backend = StubOutpaintBackend::default();
        let image = vec![100u8; 4 * 4 * 4];
        let req = OutpaintRequest {
            canvas: OutpaintCanvas {
                output_width: 6,
                output_height: 4,
                source_offset_x: -1,
                source_offset_y: 0,
            },
            ..expand_request(42)
        };
        let out = backend.expand(&image, 4, 4, &req).unwrap();
        assert_eq!(out.len(), 6 * 4 * 4);
    }

    #[test]
    fn manifest_carries_outpaint_capability() {
        let m = StubOutpaintBackend::manifest();
        assert!(m.capabilities.outpaint);
        assert!(!m.capabilities.inpaint_heal);
        assert!(!m.capabilities.subject_segmentation);
        assert_eq!(m.input.resolution.width, 1024);
        assert_eq!(m.input.resolution.height, 1024);
        assert_eq!(m.model_name, "inpaint-outpaint-xl");
        assert_eq!(m.model_hash, crate::hash::PENDING_INTEGRATION_HASH);
        assert!(m.validate().is_ok());
        // Roundtrip preserves the new capability verbatim.
        let back = ModelManifest::from_json(&m.to_json().unwrap()).unwrap();
        assert_eq!(m, back);
    }
}
