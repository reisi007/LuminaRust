//! SAM 2.1 interactive segmentation adapter (F-082).
//!
//! This module adds the first interactive (prompted) segmentation model to
//! `lumina-onnx`: the SAM 2.1 `hiera_*` family. Unlike BiRefNet (automatic
//! subject segmentation, no prompts), SAM 2 turns a user prompt — a box, one or
//! more point clicks, or a mask — into an object mask.
//!
//! The real ONNX Runtime path is **not** implemented here; it is prepared only
//! as a documented tensor contract and a [`StubSam2Backend`] that produces a
//! deterministic, network-free matte so the F-083 tests run without any model
//! artifact. The genuine `ort`-backed decoder (encoder once per image →
//! embedding + high-res features; decoder per prompt) is follow-up work behind
//! the `onnx-rt` feature.
//!
//! ## Prompt → tensor contract (documented for the ORT path)
//!
//! SAM 2.1 operates in a 1024² long-side coordinate space. The adapter maps
//! every prompt into the following inputs (shapes are per call):
//!
//! * `point_coords` — absolute pixel coordinates in the 1024² space, shape
//!   `[1, N, 2]`.
//! * `point_labels` — same length `N`, encoding (1 positive click / 0 negative
//!   click / −1 padding / 2 box top-left / 3 box bottom-right).
//! * `input_masks` + `has_input_masks` — an optional low-res mask seed for the
//!   `mask_prompt` refinement mode (`has_input_masks = false` when absent).
//! * `images` — the encoder input tensor, RGB NCHW, 1×3×1024×1024.
//!
//! Outputs: `masks` (upsampled to the source resolution), `iou_predictions`
//! (one score per mask) and `low_res_masks` (4× upsampling candidates). The
//! matte is taken from the logits: `value = round(sigmoid(logit) * 65535)`
//! clamped to `u16`, stored as grayscale in `.lumina.zdata`. The stub applies
//! the same Logits→`u16` scaling to its analytic mattes so the contract is
//! uniform across real and stub backends.

use crate::manifest::{ModelManifest, Sam2Variant};
use crate::preprocess::rescale_model_matte;
use crate::OnnxError;
use lumina_core::{ImageFrame, MaskPlane};

/// A 2D point in the model's 1024² coordinate space (absolute pixels).
///
/// **Coordinate-space responsibility** (REVIEW-ONNX-N1): a `PromptPoint` is
/// *always* in model space. Callers holding source-image coordinates convert
/// them explicitly via [`source_point_to_model`] / [`source_box_to_model`];
/// nothing infers the space from context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptPoint {
    /// Horizontal coordinate in `[0, 1023]`.
    pub x: u32,
    /// Vertical coordinate in `[0, 1023]`.
    pub y: u32,
}

/// A 2D point in the **source image's** pixel space (absolute pixels).
///
/// This is what GUI/CLI prompts start as; [`PromptMaskInference`] backends
/// receive model-space data only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePoint {
    /// Horizontal coordinate in `[0, width - 1]` of the source image.
    pub x: u32,
    /// Vertical coordinate in `[0, height - 1]` of the source image.
    pub y: u32,
}

/// Label of a point (click) prompt, matching the SAM 2.1 `point_labels`
/// encoding (REVIEW-ONNX-N1 — the full documented encoding is expressible):
///
/// | value | meaning |
/// | ----- | ------- |
/// | `1`   | positive click |
/// | `0`   | negative click |
/// | `-1`  | padding (ignored by the model) |
/// | `2`   | box top-left corner |
/// | `3`   | box bottom-right corner |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointLabel {
    /// Positive click — "this point is inside the object" (label `1`).
    Positive,
    /// Negative click — "this point is outside the object" (label `0`).
    Negative,
    /// Padding entry — present in the tensor but ignored by the model
    /// (label `-1`).
    Padding,
    /// Box top-left corner marker (label `2`).
    BoxTopLeft,
    /// Box bottom-right corner marker (label `3`).
    BoxBottomRight,
}

impl PointLabel {
    /// The integer SAM `point_labels` encoding for this label.
    pub fn to_sam_label(self) -> i32 {
        match self {
            PointLabel::Positive => 1,
            PointLabel::Negative => 0,
            PointLabel::Padding => -1,
            PointLabel::BoxTopLeft => 2,
            PointLabel::BoxBottomRight => 3,
        }
    }

    /// Parse the integer SAM `point_labels` encoding; unknown values are
    /// rejected (`None`) instead of silently coerced.
    pub fn from_sam_label(value: i32) -> Option<Self> {
        match value {
            1 => Some(PointLabel::Positive),
            0 => Some(PointLabel::Negative),
            -1 => Some(PointLabel::Padding),
            2 => Some(PointLabel::BoxTopLeft),
            3 => Some(PointLabel::BoxBottomRight),
            _ => None,
        }
    }
}

/// A bounding-box prompt defined by two corners in 1024² space.
///
/// Model-space by definition; build one from source coordinates with
/// [`source_box_to_model`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxPrompt {
    /// Top-left corner (must be above/left of `bottom_right`).
    pub top_left: PromptPoint,
    /// Bottom-right corner (must be below/right of `top_left`).
    pub bottom_right: PromptPoint,
}

/// Optional mask-prompt logits seed (the `input_masks` tensor) for iterative
/// refinement. The stub treats each value as a raw logit and applies the
/// documented Logits→`u16` scaling (sigmoid then `* 65535`).
#[derive(Debug, Clone, PartialEq)]
pub struct MaskPromptLogits {
    /// Logit grid width (model resolution).
    pub width: u32,
    /// Logit grid height (model resolution).
    pub height: u32,
    /// Raw logits, row-major, length `width * height`.
    pub logits: Vec<f32>,
}

/// A full segmentation prompt as accepted by [`PromptMaskInference`].
///
/// Any combination of the three modalities may be supplied; the backend decides
/// how to combine them. The deterministic [`StubSam2Backend`] uses a fixed
/// precedence (mask > box > points) so re-runs are byte-identical; the real ORT
/// path will encode all of them into the SAM tensors in one call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentationPrompt {
    /// Optional bounding box.
    pub box_prompt: Option<BoxPrompt>,
    /// Optional point prompts (positive/negative clicks).
    pub points: Vec<(PromptPoint, PointLabel)>,
    /// Optional mask-prompt logits seed.
    pub mask_logits: Option<MaskPromptLogits>,
}

/// Interactive (prompted) segmentation surface.
///
/// Implemented by the [`StubSam2Backend`] today and, later, by a real ORT-
/// backed SAM 2.1 decoder. The model-agnostic [`SubjectInference`] surface in
/// `backend.rs` is separate: SAM 2 is interactive-only, so it exposes prompts
/// instead of whole-image subject inference.
pub trait PromptMaskInference {
    /// The model manifest this backend was built from.
    fn manifest(&self) -> &ModelManifest;

    /// Run prompted segmentation for `image` + `prompt`, returning a matte at
    /// the source image resolution. Must never silently fall back on a missing
    /// artifact, an unsupported capability, or invalid coordinates — it
    /// returns an [`OnnxError`] instead.
    fn infer_prompt(
        &self,
        image: &ImageFrame,
        prompt: &SegmentationPrompt,
    ) -> Result<MaskPlane, OnnxError>;
}

/// Deterministic, dependency-free SAM 2.1 backend.
///
/// It needs no model weights and no network, and is the complete default
/// surface for the interactive path: it maps the prompt contract onto an
/// analytic, distance-weighted matte so the F-083 tests run green without any
/// `.onnx` artifact. The genuine encoder/decoder inference is follow-up work
/// behind `onnx-rt`.
pub struct StubSam2Backend {
    manifest: ModelManifest,
    /// Whether the model artifact/weights required for inference are present.
    /// The stub has no weights, so this is `true` by default; it can be
    /// flipped to simulate an unavailable model for testing.
    available: bool,
}

impl StubSam2Backend {
    /// Build a stub SAM backend from a validated manifest.
    ///
    /// Requires at least one interactive capability (`box_prompt`,
    /// `point_prompt` or `mask_prompt`) to be set, since the SAM backend is
    /// prompted-only.
    pub fn new(manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        let caps = &manifest.capabilities;
        if !(caps.box_prompt || caps.point_prompt || caps.mask_prompt) {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "SAM backend requires at least one interactive capability \
                         (box_prompt, point_prompt or mask_prompt)"
                    .into(),
            });
        }
        Ok(Self {
            manifest,
            available: true,
        })
    }

    /// Override the reported model availability (simulate a missing model).
    pub fn with_availability(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Whether this backend can currently perform inference.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// The model manifest this backend was built from.
    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    /// Validate prompt capabilities and coordinates, then build the model-
    /// resolution matte.
    fn generate_prompt_matte(&self, prompt: &SegmentationPrompt) -> Result<MaskPlane, OnnxError> {
        let res = self.manifest.input.resolution;
        let (w, h) = (res.width, res.height);

        // Capability gate: the model must declare the requested modality.
        if prompt.box_prompt.is_some() && !self.manifest.capabilities.box_prompt {
            return Err(OnnxError::InvalidPrompt {
                name: self.manifest.model_name.clone(),
                reason: "model does not support box_prompt".into(),
            });
        }
        if !prompt.points.is_empty() && !self.manifest.capabilities.point_prompt {
            return Err(OnnxError::InvalidPrompt {
                name: self.manifest.model_name.clone(),
                reason: "model does not support point_prompt".into(),
            });
        }
        if prompt.mask_logits.is_some() && !self.manifest.capabilities.mask_prompt {
            return Err(OnnxError::InvalidPrompt {
                name: self.manifest.model_name.clone(),
                reason: "model does not support mask_prompt".into(),
            });
        }

        // Coordinate / size validation (no silent clamping).
        if let Some(b) = &prompt.box_prompt {
            if b.top_left.x > b.bottom_right.x || b.top_left.y > b.bottom_right.y {
                return Err(OnnxError::InvalidPrompt {
                    name: self.manifest.model_name.clone(),
                    reason: "box top-left must be above/left of bottom-right".into(),
                });
            }
            for p in [b.top_left, b.bottom_right] {
                if p.x >= w || p.y >= h {
                    return Err(OnnxError::InvalidPrompt {
                        name: self.manifest.model_name.clone(),
                        reason: format!(
                            "box coordinate ({}, {}) out of model bounds [0, {}]x[0, {}]",
                            p.x, p.y, w, h
                        ),
                    });
                }
            }
        }
        for (p, _) in &prompt.points {
            if p.x >= w || p.y >= h {
                return Err(OnnxError::InvalidPrompt {
                    name: self.manifest.model_name.clone(),
                    reason: format!(
                        "point coordinate ({}, {}) out of model bounds [0, {}]x[0, {}]",
                        p.x, p.y, w, h
                    ),
                });
            }
        }
        if let Some(m) = &prompt.mask_logits {
            if m.width != w || m.height != h || m.logits.len() != (w as usize * h as usize) {
                return Err(OnnxError::InvalidPrompt {
                    name: self.manifest.model_name.clone(),
                    reason: format!(
                        "mask_logits size {}x{} ({} values) does not match model resolution {}x{}",
                        m.width,
                        m.height,
                        m.logits.len(),
                        w,
                        h
                    ),
                });
            }
        }

        // Modality precedence: mask seed > box > points.
        let values = if let Some(m) = &prompt.mask_logits {
            mask_logits_matte(m)
        } else if let Some(b) = &prompt.box_prompt {
            box_matte(b, w, h)
        } else if !prompt.points.is_empty() {
            point_matte(&prompt.points, w, h)
        } else {
            return Err(OnnxError::InvalidPrompt {
                name: self.manifest.model_name.clone(),
                reason: "no prompt modality provided (need box, points or mask_logits)".into(),
            });
        };

        MaskPlane::new(w, h, values).map_err(|_| OnnxError::InvalidDimensions {
            expected_width: w,
            expected_height: h,
            actual_width: w,
            actual_height: h,
        })
    }
}

impl PromptMaskInference for StubSam2Backend {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn infer_prompt(
        &self,
        image: &ImageFrame,
        prompt: &SegmentationPrompt,
    ) -> Result<MaskPlane, OnnxError> {
        if !self.available {
            return Err(OnnxError::MissingModel {
                path: self.manifest.model_name.clone(),
            });
        }
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        let model_matte = self.generate_prompt_matte(prompt)?;
        rescale_model_matte(
            &model_matte,
            self.manifest.input.resolution,
            (image.width, image.height),
        )
    }
}

/// Soft, distance-weighted box matte. Inside the box is opaque; outside it
/// fades to transparent over a margin of `w/16` pixels. Fully deterministic.
fn box_matte(b: &BoxPrompt, w: u32, h: u32) -> Vec<u16> {
    let margin = (w / 16).max(1) as f64;
    let mut values = vec![0u16; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let dx_out = b
                .top_left
                .x
                .saturating_sub(x)
                .max(x.saturating_sub(b.bottom_right.x));
            let dy_out = b
                .top_left
                .y
                .saturating_sub(y)
                .max(y.saturating_sub(b.bottom_right.y));
            let inside = dx_out == 0 && dy_out == 0;
            let alpha = if inside {
                1.0
            } else {
                let d = (dx_out as f64).hypot(dy_out as f64);
                (1.0 - d / margin).clamp(0.0, 1.0)
            };
            values[(y * w + x) as usize] = (alpha * 65535.0).round() as u16;
        }
    }
    values
}

/// Distance-weighted point matte. Positive clicks attract (peak alpha at the
/// click, fading over a radius of `w/8`); negative clicks subtract. Fully
/// deterministic.
///
/// REVIEW-ONNX-N1 label handling for the extended encoding:
/// * `Padding` contributes nothing (it is ignored by the model too);
/// * `BoxTopLeft` / `BoxBottomRight` act as positive attractors — this is the
///   documented **stub** interpretation of corner-labeled points, since the
///   analytic matte has no box decoder; the real ORT path encodes them as
///   labels `2`/`3` instead.
fn point_matte(points: &[(PromptPoint, PointLabel)], w: u32, h: u32) -> Vec<u16> {
    let radius = (w / 8).max(1) as f64;
    // Attraction strength of a single labeled point at (px, py).
    let contribution = |p: &PromptPoint, x: u32, y: u32| -> f64 {
        let dx = x as f64 - p.x as f64;
        let dy = y as f64 - p.y as f64;
        let d = (dx * dx + dy * dy).sqrt();
        (1.0 - d / radius).clamp(0.0, 1.0)
    };
    let mut values = vec![0u16; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut alpha = 0.0f64;
            for (p, label) in points {
                match label {
                    PointLabel::Padding => {}
                    PointLabel::Positive | PointLabel::BoxTopLeft | PointLabel::BoxBottomRight => {
                        alpha = alpha.max(contribution(p, x, y));
                    }
                    PointLabel::Negative => {
                        alpha = (alpha - contribution(p, x, y)).clamp(0.0, 1.0);
                    }
                }
            }
            values[(y * w + x) as usize] = (alpha * 65535.0).round() as u16;
        }
    }
    values
}

/// Map mask-prompt logits to `u16` grayscale via the documented
/// Logits→`u16` scaling: `value = round(sigmoid(logit) * 65535)`.
fn mask_logits_matte(m: &MaskPromptLogits) -> Vec<u16> {
    m.logits
        .iter()
        .map(|l| {
            let a = 1.0 / (1.0 + (-l).exp());
            (a * 65535.0).round() as u16
        })
        .collect()
}

/// Convenience helper: build a default (available) stub SAM backend for the
/// given variant.
pub fn stub_sam2(variant: Sam2Variant) -> Result<StubSam2Backend, OnnxError> {
    StubSam2Backend::new(variant.manifest())
}

/// Map a source-image point into model space (REVIEW-ONNX-N1).
///
/// Uses the same deterministic integer nearest-neighbor mapping as
/// [`crate::preprocess::preprocess_rgb_to_model`] (`dst = src * dst_dim /
/// src_dim`), so a click lands on exactly the pixel whose RGB value the model
/// sees. Zero dimensions are defensively treated as `1` (upstream inference
/// paths validate non-zero dimensions before prompts are mapped).
pub fn source_point_to_model(
    point: SourcePoint,
    source: (u32, u32),
    model: (u32, u32),
) -> PromptPoint {
    let sw = source.0.max(1) as u64;
    let sh = source.1.max(1) as u64;
    let mw = model.0 as u64;
    let mh = model.1 as u64;
    PromptPoint {
        x: ((point.x as u64 * mw) / sw).min(mw.saturating_sub(1)) as u32,
        y: ((point.y as u64 * mh) / sh).min(mh.saturating_sub(1)) as u32,
    }
}

/// Map a model-space point back to the source image's pixel space.
///
/// Inverse of [`source_point_to_model`] using the same integer mapping in the
/// opposite direction; the round trip is lossy by up to one source pixel
/// (inherent to nearest-neighbor scaling), never out of bounds.
pub fn model_point_to_source(
    point: PromptPoint,
    source: (u32, u32),
    model: (u32, u32),
) -> SourcePoint {
    let sw = source.0 as u64;
    let sh = source.1 as u64;
    let mw = model.0.max(1) as u64;
    let mh = model.1.max(1) as u64;
    SourcePoint {
        x: ((point.x as u64 * sw) / mw).min(sw.saturating_sub(1)) as u32,
        y: ((point.y as u64 * sh) / mh).min(sh.saturating_sub(1)) as u32,
    }
}

/// A bounding-box prompt in **source image** pixel space. Convert with
/// [`source_box_to_model`] before handing it to a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceBox {
    /// Top-left corner (must be above/left of `bottom_right`).
    pub top_left: SourcePoint,
    /// Bottom-right corner (must be below/right of `top_left`).
    pub bottom_right: SourcePoint,
}

/// Map a source-space box into the model's 1024² coordinate space (F-012
/// SOLL: "Eine Box wird in das Koordinatensystem des Modells transformiert").
///
/// Both corners are mapped independently with [`source_point_to_model`]; if
/// rounding collapses the corner order on an extremely small box, the corners
/// are normalized (min/max per axis) so the result always satisfies the
/// [`BoxPrompt`] ordering invariant.
pub fn source_box_to_model(
    box_prompt: SourceBox,
    source: (u32, u32),
    model: (u32, u32),
) -> BoxPrompt {
    let tl = source_point_to_model(box_prompt.top_left, source, model);
    let br = source_point_to_model(box_prompt.bottom_right, source, model);
    BoxPrompt {
        top_left: PromptPoint {
            x: tl.x.min(br.x),
            y: tl.y.min(br.y),
        },
        bottom_right: PromptPoint {
            x: tl.x.max(br.x),
            y: tl.y.max(br.y),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{sam2_1_manifest, SAM2_INFERENCE_HEIGHT, SAM2_INFERENCE_WIDTH};
    use crate::Resolution;

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

    fn sample_points() -> Vec<(PromptPoint, PointLabel)> {
        vec![
            (PromptPoint { x: 512, y: 512 }, PointLabel::Positive),
            (PromptPoint { x: 200, y: 200 }, PointLabel::Negative),
        ]
    }

    // F-083 #1 — prompt roundtrip determinism (same prompt → identical matte).
    #[test]
    fn box_prompt_is_deterministic_and_roundtrips() {
        let backend = stub_sam2(Sam2Variant::BasePlus).unwrap();
        let img = frame(640, 480, [120, 80, 200]);
        let prompt = SegmentationPrompt {
            box_prompt: Some(BoxPrompt {
                top_left: PromptPoint { x: 100, y: 100 },
                bottom_right: PromptPoint { x: 600, y: 500 },
            }),
            points: vec![],
            mask_logits: None,
        };
        let a = backend.infer_prompt(&img, &prompt).unwrap();
        let b = backend.infer_prompt(&img, &prompt).unwrap();
        assert_eq!(a.values, b.values, "box prompt must be byte-identical");

        // A second, independent backend built from the same manifest must also
        // produce an identical matte (no seed / no network).
        let backend2 = stub_sam2(Sam2Variant::BasePlus).unwrap();
        let c = backend2.infer_prompt(&img, &prompt).unwrap();
        assert_eq!(a.values, c.values, "cross-instance determinism");
    }

    #[test]
    fn point_prompt_is_deterministic_and_roundtrips() {
        let backend = stub_sam2(Sam2Variant::Small).unwrap();
        let img = frame(320, 240, [10, 20, 30]);
        let prompt = SegmentationPrompt {
            box_prompt: None,
            points: sample_points(),
            mask_logits: None,
        };
        let a = backend.infer_prompt(&img, &prompt).unwrap();
        let b = backend.infer_prompt(&img, &prompt).unwrap();
        assert_eq!(a.values, b.values, "point prompt must be byte-identical");
        // The positive center click must be brighter than a far corner.
        let center = (a.height as usize / 2) * a.width as usize + a.width as usize / 2;
        assert!(a.values[center] > a.values[0]);
    }

    #[test]
    fn mask_logits_prompt_is_deterministic_and_roundtrips() {
        let backend = stub_sam2(Sam2Variant::Tiny).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let res = Resolution {
            width: SAM2_INFERENCE_WIDTH,
            height: SAM2_INFERENCE_HEIGHT,
        };
        let mut logits = vec![0.0f32; (res.width * res.height) as usize];
        // A single high-logit pixel → near-opaque; the rest → transparent.
        logits[0] = 8.0;
        let prompt = SegmentationPrompt {
            box_prompt: None,
            points: vec![],
            mask_logits: Some(MaskPromptLogits {
                width: res.width,
                height: res.height,
                logits,
            }),
        };
        let a = backend.infer_prompt(&img, &prompt).unwrap();
        let b = backend.infer_prompt(&img, &prompt).unwrap();
        assert_eq!(
            a.values, b.values,
            "mask-logits prompt must be byte-identical"
        );
        // Original pixel (0,0) maps to top-left of the downscaled output.
        assert!(a.values[0] > 60000, "sigmoid(8) ~= 0.9997 → near opaque");
    }

    // F-083 #4 — unsupported prompt: unknown capability / invalid coordinates.
    #[test]
    fn rejects_unsupported_capability_box() {
        // A manifest that declares point_prompt but not box_prompt.
        let mut manifest = sam2_1_manifest(Sam2Variant::Large);
        manifest.capabilities.box_prompt = false;
        let backend = StubSam2Backend::new(manifest).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let prompt = SegmentationPrompt {
            box_prompt: Some(BoxPrompt {
                top_left: PromptPoint { x: 10, y: 10 },
                bottom_right: PromptPoint { x: 50, y: 50 },
            }),
            points: vec![],
            mask_logits: None,
        };
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(
            matches!(err, OnnxError::InvalidPrompt { .. }),
            "unsupported box capability must error, got {err:?}"
        );
    }

    #[test]
    fn rejects_unsupported_capability_mask() {
        let mut manifest = sam2_1_manifest(Sam2Variant::BasePlus);
        manifest.capabilities.mask_prompt = false;
        let backend = StubSam2Backend::new(manifest).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let res = Resolution {
            width: SAM2_INFERENCE_WIDTH,
            height: SAM2_INFERENCE_HEIGHT,
        };
        let prompt = SegmentationPrompt {
            box_prompt: None,
            points: vec![],
            mask_logits: Some(MaskPromptLogits {
                width: res.width,
                height: res.height,
                logits: vec![0.0; (res.width * res.height) as usize],
            }),
        };
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(
            matches!(err, OnnxError::InvalidPrompt { .. }),
            "unsupported mask capability must error, got {err:?}"
        );
    }

    #[test]
    fn rejects_inverted_box() {
        let backend = stub_sam2(Sam2Variant::Small).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let prompt = SegmentationPrompt {
            box_prompt: Some(BoxPrompt {
                // bottom-right above/left of top-left → invalid
                top_left: PromptPoint { x: 50, y: 50 },
                bottom_right: PromptPoint { x: 10, y: 10 },
            }),
            points: vec![],
            mask_logits: None,
        };
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(matches!(err, OnnxError::InvalidPrompt { .. }), "{err:?}");
    }

    #[test]
    fn rejects_out_of_bounds_point() {
        let backend = stub_sam2(Sam2Variant::Small).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let prompt = SegmentationPrompt {
            box_prompt: None,
            points: vec![(PromptPoint { x: 1024, y: 0 }, PointLabel::Positive)],
            mask_logits: None,
        };
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(matches!(err, OnnxError::InvalidPrompt { .. }), "{err:?}");
    }

    #[test]
    fn rejects_wrong_mask_logits_size() {
        let backend = stub_sam2(Sam2Variant::Large).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let prompt = SegmentationPrompt {
            box_prompt: None,
            points: vec![],
            // Wrong size: declares 1024×1024 but only supplies 4 values.
            mask_logits: Some(MaskPromptLogits {
                width: SAM2_INFERENCE_WIDTH,
                height: SAM2_INFERENCE_HEIGHT,
                logits: vec![0.0; 4],
            }),
        };
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(matches!(err, OnnxError::InvalidPrompt { .. }), "{err:?}");
    }

    #[test]
    fn rejects_empty_prompt() {
        let backend = stub_sam2(Sam2Variant::Tiny).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let prompt = SegmentationPrompt::default();
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(matches!(err, OnnxError::InvalidPrompt { .. }), "{err:?}");
    }

    #[test]
    fn unavailable_model_surfaces_missing_model() {
        let backend = stub_sam2(Sam2Variant::BasePlus)
            .unwrap()
            .with_availability(false);
        let img = frame(64, 64, [0, 0, 0]);
        let prompt = SegmentationPrompt {
            box_prompt: Some(BoxPrompt {
                top_left: PromptPoint { x: 10, y: 10 },
                bottom_right: PromptPoint { x: 50, y: 50 },
            }),
            points: vec![],
            mask_logits: None,
        };
        let err = backend.infer_prompt(&img, &prompt).unwrap_err();
        assert!(
            matches!(err, OnnxError::MissingModel { .. }),
            "unavailable model must surface as MissingModel, got {err:?}"
        );
    }

    #[test]
    fn box_matte_is_filled_with_soft_border() {
        // Direct model-resolution check of the analytic matte.
        let b = BoxPrompt {
            top_left: PromptPoint { x: 400, y: 400 },
            bottom_right: PromptPoint { x: 600, y: 600 },
        };
        let m = Resolution {
            width: SAM2_INFERENCE_WIDTH,
            height: SAM2_INFERENCE_HEIGHT,
        };
        let matte = MaskPlane::new(m.width, m.height, box_matte(&b, m.width, m.height)).unwrap();
        let center = (500 * m.width + 500) as usize;
        let corner = 0usize;
        assert_eq!(matte.values[center], 65535, "box center must be opaque");
        assert_eq!(matte.values[corner], 0, "far corner must be transparent");
    }

    // REVIEW-ONNX-N1 — the full SAM label encoding (−1/0/1/2/3) is
    // expressible and round-trips.
    #[test]
    fn point_label_covers_full_sam_encoding() {
        let pairs = [
            (PointLabel::Positive, 1),
            (PointLabel::Negative, 0),
            (PointLabel::Padding, -1),
            (PointLabel::BoxTopLeft, 2),
            (PointLabel::BoxBottomRight, 3),
        ];
        for (label, value) in pairs {
            assert_eq!(label.to_sam_label(), value);
            assert_eq!(PointLabel::from_sam_label(value), Some(label));
        }
        // Unknown encodings are rejected, never coerced.
        assert_eq!(PointLabel::from_sam_label(4), None);
        assert_eq!(PointLabel::from_sam_label(-2), None);
    }

    #[test]
    fn source_point_maps_deterministically_to_model_space() {
        // Same integer mapping as preprocess_rgb_to_model:
        // model = src * model_dim / src_dim.
        let p = SourcePoint { x: 320, y: 240 };
        let m = source_point_to_model(p, (640, 480), (1024, 1024));
        assert_eq!(m.x, (320u64 * 1024 / 640) as u32); // 512
        assert_eq!(m.y, (240u64 * 1024 / 480) as u32); // 512

        // Non-uniform aspect ratios scale per axis (documented square-resize
        // adapter contract).
        let p = SourcePoint { x: 160, y: 480 };
        let m = source_point_to_model(p, (640, 960), (1024, 1024));
        assert_eq!(m.x, (160u64 * 1024 / 640) as u32); // 256
        assert_eq!(m.y, (480u64 * 1024 / 960) as u32); // 512

        // Identity when spaces coincide; corners stay corners; results are
        // always in bounds.
        let corner = SourcePoint { x: 1919, y: 1079 };
        let m = source_point_to_model(corner, (1920, 1080), (1024, 1024));
        assert!(m.x < 1024 && m.y < 1024, "{m:?}");
    }

    #[test]
    fn model_point_maps_back_within_one_source_pixel() {
        let source = (1920u32, 1080u32);
        let model = (1024u32, 1024u32);
        for mx in [0u32, 1, 511, 512, 1000, 1023] {
            for my in [0u32, 7, 300, 1023] {
                let s = model_point_to_source(PromptPoint { x: mx, y: my }, source, model);
                assert!(s.x < source.0 && s.y < source.1, "out of bounds: {s:?}");
                // Re-mapping the recovered source pixel must land back on a
                // neighboring model pixel (±1, nearest-neighbor rounding).
                let back = source_point_to_model(s, source, model);
                assert!(
                    (back.x as i64 - mx as i64).abs() <= 1,
                    "{mx}->{s:?}->{back:?}"
                );
                assert!(
                    (back.y as i64 - my as i64).abs() <= 1,
                    "{my}->{s:?}->{back:?}"
                );
            }
        }
    }

    #[test]
    fn source_box_maps_and_keeps_corner_order() {
        let source = (800u32, 600u32);
        let model = (1024u32, 1024u32);
        let b = source_box_to_model(
            SourceBox {
                top_left: SourcePoint { x: 100, y: 50 },
                bottom_right: SourcePoint { x: 700, y: 550 },
            },
            source,
            model,
        );
        assert!(b.top_left.x <= b.bottom_right.x);
        assert!(b.top_left.y <= b.bottom_right.y);
        assert_eq!(b.top_left.x, (100u64 * 1024 / 800) as u32); // 128
        assert_eq!(b.bottom_right.y, (550u64 * 1024 / 600) as u32); // 938

        // Degenerate one-pixel box must not invert under rounding.
        let tiny = source_box_to_model(
            SourceBox {
                top_left: SourcePoint { x: 10, y: 10 },
                bottom_right: SourcePoint { x: 11, y: 11 },
            },
            source,
            model,
        );
        assert!(tiny.top_left.x <= tiny.bottom_right.x);
        assert!(tiny.top_left.y <= tiny.bottom_right.y);
    }

    #[test]
    fn padding_points_do_not_shape_the_stub_matte() {
        let backend = stub_sam2(Sam2Variant::Tiny).unwrap();
        let img = frame(64, 64, [0, 0, 0]);
        let with_padding = SegmentationPrompt {
            box_prompt: None,
            points: vec![
                (PromptPoint { x: 512, y: 512 }, PointLabel::Positive),
                (PromptPoint { x: 900, y: 900 }, PointLabel::Padding),
            ],
            mask_logits: None,
        };
        let without_padding = SegmentationPrompt {
            box_prompt: None,
            points: vec![(PromptPoint { x: 512, y: 512 }, PointLabel::Positive)],
            mask_logits: None,
        };
        let a = backend.infer_prompt(&img, &with_padding).unwrap();
        let b = backend.infer_prompt(&img, &without_padding).unwrap();
        assert_eq!(
            a.values, b.values,
            "padding points must not influence the matte"
        );
    }

    #[test]
    fn box_corner_labels_act_as_positive_attractors_in_stub() {
        let backend = stub_sam2(Sam2Variant::Small).unwrap();
        // 64px source; output pixel o maps to model coordinate o*16
        // (nearest-neighbor 1024 -> 64).
        let img = frame(64, 64, [0, 0, 0]);
        let corners = SegmentationPrompt {
            box_prompt: None,
            points: vec![
                (PromptPoint { x: 200, y: 200 }, PointLabel::BoxTopLeft),
                (PromptPoint { x: 800, y: 800 }, PointLabel::BoxBottomRight),
            ],
            mask_logits: None,
        };
        let matte = backend.infer_prompt(&img, &corners).unwrap();
        let idx = |x: u32, y: u32| (y * matte.width + x) as usize;
        // Output (12,12) -> model (192,192), ~11px from the top-left corner
        // label: well inside its w/8 attraction radius.
        let near_top_left = matte.values[idx(12, 12)];
        // Output (50,50) -> model (800,800): exactly on the bottom-right
        // corner label -> fully opaque.
        let on_bottom_right = matte.values[idx(50, 50)];
        // Output (63,63) -> model (1008,1008): outside both radii.
        let far_away = matte.values[idx(63, 63)];
        assert!(
            near_top_left > 59000,
            "corner label must attract, got {near_top_left}"
        );
        assert_eq!(on_bottom_right, 65535, "label position itself is opaque");
        assert_eq!(far_away, 0, "pixels outside both radii stay transparent");
    }
}
