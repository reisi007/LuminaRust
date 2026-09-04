//! G-05 Lens Blur: deterministic depth bokeh.
//!
//! The recipe stage ([`lumina_sidecar::LensBlur`]) is validated here (mirroring
//! the sidecar rules) and rendered as a sub-stage of `Crop`: after
//! crop/rotation/mirroring, before masks/output (see
//! `feature/architecture/pipeline.md` § „G-05 Lens Blur").
//!
//! Depth source decision: without `depth_artifact` the deterministic
//! focus-rect heuristic applies (depth 0 inside the focus rectangle, rising
//! with the diagonal-normalized distance to its edge). With a referenced
//! artifact the caller MUST supply the [`DepthPlane`]; a missing plane aborts
//! loudly — never a silent heuristic fallback.
//!
//! Blur model: per-pixel weight 0 inside `[focal_near, focal_far]`, linear
//! ramps outside; output = `lerp(original, bokeh_blur(original), weight)`
//! with `radius = round(blur_amount * 16)` px. The three [`BokehShape`]
//! kernels are integer masks (no randomness): disk, 2:1 ellipse, hexagon.
//! RGB only, alpha is preserved, results are clamped to `0..=255`.

use crate::{CoreError, ImageFrame};
use lumina_sidecar::{BokehShape, LensBlur};

/// Maximum blur radius in pixels (`blur_amount == 1`).
pub const LENS_BLUR_MAX_RADIUS: f32 = 16.0;

/// An explicit external depth map: one finite `0..=1` value per pixel, frame
/// sized. 0 is the focal plane, 1 is maximally distant.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthPlane {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

impl DepthPlane {
    pub fn new(width: u32, height: u32, values: Vec<f32>) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidAdjustment {
                name: "lens_blur.depth_plane.dimensions".into(),
                value: 0.0,
                minimum: 1.0,
                maximum: f64::from(u32::MAX),
            });
        }
        if values.len() != width as usize * height as usize {
            return Err(CoreError::InvalidMaskPlane {
                width,
                height,
                length: values.len(),
            });
        }
        Ok(Self {
            width,
            height,
            values,
        })
    }
}

/// User-visible lens-blur depth status for CLI/GUI: `None` (no recipe stage)
/// and disabled/zero-amount stages are `off`; a referenced artifact without a
/// supplied plane is `missing`.
pub fn lens_blur_status(blur: Option<&LensBlur>, depth_present: bool) -> &'static str {
    match blur {
        None => "off",
        Some(b) if !b.enabled || b.blur_amount == 0.0 => "off",
        Some(b) if b.depth_artifact.is_some() => {
            if depth_present {
                "external depth active"
            } else {
                "missing depth artifact"
            }
        }
        Some(_) => "heuristic active",
    }
}

/// Mirrors the sidecar `validate_adjustments` lens-blur rules. Any violation
/// is [`CoreError::InvalidAdjustment`] — never a silent reinterpretation.
pub fn validate_lens_blur(b: &LensBlur) -> Result<(), CoreError> {
    if b.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "lens_blur.version".into(),
            value: f64::from(b.version),
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    let r = &b.focus_rect;
    for (name, v) in [
        ("x", r.x),
        ("y", r.y),
        ("width", r.width),
        ("height", r.height),
    ] {
        if !v.is_finite() {
            return Err(CoreError::InvalidAdjustment {
                name: format!("lens_blur.focus_rect.{name}"),
                value: v as f64,
                minimum: 0.0,
                maximum: 1.0,
            });
        }
    }
    if !(0.0..=1.0).contains(&r.x)
        || !(0.0..=1.0).contains(&r.y)
        || r.width <= 0.0
        || r.height <= 0.0
        || r.x + r.width > 1.0
        || r.y + r.height > 1.0
    {
        return Err(CoreError::InvalidAdjustment {
            name: "lens_blur.focus_rect".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    for (name, v) in [
        ("focal_near", b.focal_near),
        ("focal_far", b.focal_far),
        ("blur_amount", b.blur_amount),
    ] {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            return Err(CoreError::InvalidAdjustment {
                name: format!("lens_blur.{name}"),
                value: v as f64,
                minimum: 0.0,
                maximum: 1.0,
            });
        }
    }
    if b.focal_near > b.focal_far {
        return Err(CoreError::InvalidAdjustment {
            name: "lens_blur.focal_range".into(),
            value: b.focal_near as f64,
            minimum: 0.0,
            maximum: f64::from(b.focal_far),
        });
    }
    Ok(())
}

/// Applies the G-05 lens-blur stage to `frame` (RGB, alpha preserved).
///
/// `None`/disabled/zero-amount is identity (after validation). A referenced
/// `depth_artifact` without a supplied `depth` plane aborts with
/// [`CoreError::InvalidAdjustment`]; a supplied plane must match the frame
/// dimensions and carry finite `0..=1` values.
pub fn apply_lens_blur(
    frame: &mut ImageFrame,
    blur: &LensBlur,
    depth: Option<&DepthPlane>,
) -> Result<(), CoreError> {
    validate_lens_blur(blur)?;
    if !blur.enabled || blur.blur_amount == 0.0 {
        return Ok(());
    }
    if blur.depth_artifact.is_some() && depth.is_none() {
        return Err(CoreError::InvalidAdjustment {
            name: "lens_blur.depth_artifact".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 0.0,
        });
    }
    if let Some(plane) = depth {
        if plane.width != frame.width || plane.height != frame.height {
            return Err(CoreError::InvalidMaskPlane {
                width: plane.width,
                height: plane.height,
                length: plane.values.len(),
            });
        }
        for v in &plane.values {
            if !v.is_finite() || !(0.0..=1.0).contains(v) {
                return Err(CoreError::InvalidAdjustment {
                    name: "lens_blur.depth_plane.value".into(),
                    value: *v as f64,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
    }
    let radius = (blur.blur_amount * LENS_BLUR_MAX_RADIUS).round() as i32;
    if radius <= 0 {
        return Ok(());
    }
    let weights = match depth {
        Some(plane) => depth_weights(&plane.values, blur.focal_near, blur.focal_far),
        None => heuristic_weights(frame.width, frame.height, blur),
    };
    if weights.iter().all(|w| *w == 0.0) {
        return Ok(());
    }
    let kernel = bokeh_kernel(blur.bokeh, radius);
    let blurred = convolve(frame, &kernel);
    for (index, pixel) in frame.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let w = f64::from(weights[index]);
        if w == 0.0 {
            continue;
        }
        let base = index * 4;
        for (c, channel) in pixel.iter_mut().take(3).enumerate() {
            let v = f64::from(*channel) * (1.0 - w) + f64::from(blurred[base + c]) * w;
            *channel = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(())
}

/// Deterministic heuristic depth: 0 inside the focus rectangle, otherwise the
/// edge distance normalized by the frame diagonal (`0..=1`).
fn heuristic_weights(width: u32, height: u32, blur: &LensBlur) -> Vec<f32> {
    let r = &blur.focus_rect;
    let (w, h) = (width as f32, height as f32);
    let diag = (w * w + h * h).sqrt().max(1.0);
    let mut out = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        let ny = (y as f32 + 0.5) / h;
        for x in 0..width {
            let nx = (x as f32 + 0.5) / w;
            let dx = (r.x - nx).max(0.0).max(nx - (r.x + r.width));
            let dy = (r.y - ny).max(0.0).max(ny - (r.y + r.height));
            let depth = ((dx * w).powi(2) + (dy * h).powi(2)).sqrt() / diag;
            out.push(focal_weight(
                depth.clamp(0.0, 1.0),
                blur.focal_near,
                blur.focal_far,
            ));
        }
    }
    out
}

fn depth_weights(depth: &[f32], near: f32, far: f32) -> Vec<f32> {
    depth.iter().map(|d| focal_weight(*d, near, far)).collect()
}

/// 0 inside `[near, far]`, linear ramps to 1 at depth 0 (near side) and
/// depth 1 (far side).
fn focal_weight(depth: f32, near: f32, far: f32) -> f32 {
    if depth < near {
        if near <= 0.0 {
            0.0
        } else {
            ((near - depth) / near).clamp(0.0, 1.0)
        }
    } else if depth > far {
        if far >= 1.0 {
            0.0
        } else {
            ((depth - far) / (1.0 - far)).clamp(0.0, 1.0)
        }
    } else {
        0.0
    }
}

/// Integer kernel offsets for a bokeh shape at `radius >= 1`.
fn bokeh_kernel(shape: BokehShape, radius: i32) -> Vec<(i32, i32)> {
    let mut taps = Vec::new();
    let r = radius as f32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let inside = match shape {
                BokehShape::Round => (dx * dx + dy * dy) as f32 <= r * r,
                BokehShape::Elliptical => (dx * dx + 4 * dy * dy) as f32 <= r * r,
                BokehShape::Hexagonal => (dx.abs() + dy.abs() + (dx + dy).abs()) as f32 <= 2.0 * r,
            };
            if inside {
                taps.push((dx, dy));
            }
        }
    }
    taps
}

/// Normalized box-style convolution with clamp-to-edge sampling (RGB only).
fn convolve(frame: &ImageFrame, kernel: &[(i32, i32)]) -> Vec<u8> {
    let (w, h) = (frame.width as i32, frame.height as i32);
    let n = kernel.len() as f32;
    let mut out = vec![0u8; frame.pixels.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 3];
            for (dx, dy) in kernel {
                let sx = (x + dx).clamp(0, w - 1) as u32;
                let sy = (y + dy).clamp(0, h - 1) as u32;
                let base = (sy * frame.width + sx) as usize * 4;
                for (slot, value) in acc.iter_mut().zip(frame.pixels[base..base + 3].iter()) {
                    *slot += u32::from(*value);
                }
            }
            let base = (y as u32 * frame.width + x as u32) as usize * 4;
            for c in 0..3 {
                out[base + c] = ((acc[c] as f32 / n).round().clamp(0.0, 255.0)) as u8;
            }
            out[base + 3] = frame.pixels[base + 3];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{DepthArtifactRef, FocusRect};

    fn blur() -> LensBlur {
        LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.0,
            focal_far: 0.05,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        }
    }

    fn striped_frame() -> ImageFrame {
        striped_frame_sized(32, 32)
    }

    /// `w`×`h` vertical stripes: even columns 0, odd columns 255.
    fn striped_frame_sized(w: u32, h: u32) -> ImageFrame {
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for _ in 0..h {
            for x in 0..w {
                let v = if x % 2 == 0 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }

    #[test]
    fn disabled_or_zero_amount_is_identity() {
        for mut b in [
            LensBlur {
                enabled: false,
                ..blur()
            },
            LensBlur {
                blur_amount: 0.0,
                ..blur()
            },
        ] {
            let mut frame = striped_frame();
            let before = frame.clone();
            apply_lens_blur(&mut frame, &b, None).unwrap();
            assert_eq!(frame, before);
            // Disabled with a referenced artifact must NOT demand the plane.
            b.depth_artifact = Some(DepthArtifactRef {
                relative_path: "depth/raw.bin".into(),
                sha256: "abc".into(),
            });
            b.enabled = false;
            let mut frame = striped_frame();
            apply_lens_blur(&mut frame, &b, None).unwrap();
            assert_eq!(frame, before);
        }
    }

    #[test]
    fn invalid_values_are_rejected_not_clipped() {
        let mut cases: Vec<LensBlur> = Vec::new();
        let mut v = blur();
        v.version = 2;
        cases.push(v);
        let mut v = blur();
        v.focal_near = f32::NAN;
        cases.push(v);
        let mut v = blur();
        v.blur_amount = 1.5;
        cases.push(v);
        let mut v = blur();
        v.focus_rect.width = 0.0;
        cases.push(v);
        let mut v = blur();
        v.focus_rect.x = -0.1;
        cases.push(v);
        // focal_near > focal_far is a range violation even with valid edges.
        let mut v = blur();
        v.focal_near = 0.6;
        v.focal_far = 0.4;
        cases.push(v);
        for b in cases {
            let mut frame = striped_frame();
            assert!(
                apply_lens_blur(&mut frame, &b, None).is_err(),
                "must reject {b:?}"
            );
        }
    }

    #[test]
    fn missing_depth_artifact_aborts_loudly() {
        let b = LensBlur {
            depth_artifact: Some(DepthArtifactRef {
                relative_path: "depth/raw.bin".into(),
                sha256: "abc".into(),
            }),
            ..blur()
        };
        let mut frame = striped_frame();
        let err = apply_lens_blur(&mut frame, &b, None).unwrap_err();
        assert!(matches!(err, CoreError::InvalidAdjustment { .. }));
        assert_eq!(frame, striped_frame(), "failed render must not mutate");
    }

    #[test]
    fn depth_plane_dimension_mismatch_is_rejected() {
        let plane = DepthPlane::new(4, 4, vec![0.5; 16]).unwrap();
        let mut frame = striped_frame();
        let err = apply_lens_blur(&mut frame, &blur(), Some(&plane)).unwrap_err();
        assert!(matches!(err, CoreError::InvalidMaskPlane { .. }));
    }

    #[test]
    fn deterministic_and_alpha_preserving() {
        let mut a = striped_frame();
        let mut b = striped_frame();
        // Distinct alpha must survive the blur untouched.
        for pixel in a.pixels.as_chunks_mut::<4>().0.iter_mut() {
            pixel[3] = 7;
        }
        b.pixels.copy_from_slice(&a.pixels);
        let unchanged = a.clone();
        apply_lens_blur(&mut a, &blur(), None).unwrap();
        apply_lens_blur(&mut b, &blur(), None).unwrap();
        assert_eq!(a, b, "same input must render byte-identical");
        assert_ne!(a.pixels, unchanged.pixels, "blur must change pixels");
        for pixel in a.pixels.as_chunks::<4>().0.iter() {
            assert_eq!(pixel[3], 7);
        }
    }

    #[test]
    fn bokeh_shapes_differ_and_center_stays_sharp() {
        let mut round = striped_frame();
        let mut hex = striped_frame();
        apply_lens_blur(&mut round, &blur(), None).unwrap();
        let mut hb = blur();
        hb.bokeh = BokehShape::Hexagonal;
        apply_lens_blur(&mut hex, &hb, None).unwrap();
        assert_ne!(round, hex, "bokeh shapes must render differently");
        // Focus-rect center pixels are depth 0, inside the sharp band
        // [0, 0.05]: they must keep their exact source value.
        for frame in [&round, &hex] {
            for (x, y) in [(15, 15), (16, 16)] {
                let base = (y * 32 + x) as usize * 4;
                let expected = if x % 2 == 0 { 0 } else { 255 };
                assert_eq!(frame.pixels[base], expected, "focus pixel must stay sharp");
            }
        }
    }

    #[test]
    fn status_names_every_state() {
        assert_eq!(lens_blur_status(None, false), "off");
        let mut b = blur();
        b.enabled = false;
        assert_eq!(lens_blur_status(Some(&b), false), "off");
        b.enabled = true;
        b.blur_amount = 0.0;
        assert_eq!(lens_blur_status(Some(&b), false), "off");
        b.blur_amount = 0.5;
        assert_eq!(lens_blur_status(Some(&b), false), "heuristic active");
        b.depth_artifact = Some(DepthArtifactRef {
            relative_path: "d.bin".into(),
            sha256: "s".into(),
        });
        assert_eq!(lens_blur_status(Some(&b), false), "missing depth artifact");
        assert_eq!(lens_blur_status(Some(&b), true), "external depth active");
    }

    /// G-05 render integration: `render_frame` runs the blur after crop and
    /// before masks (same entry point as CLI/GUI — no second pipeline), and a
    /// referenced-but-missing depth artifact aborts the whole render loudly.
    #[test]
    fn render_frame_applies_lens_blur_and_rejects_missing_depth() {
        use crate::render::{render_frame, RenderContext};
        use lumina_sidecar::EditRecipe;

        let frame = striped_frame_sized(32, 32);
        let plain = EditRecipe::default();
        let plain_out = render_frame(
            &frame,
            &RenderContext {
                recipe: &plain,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                depth: None,
                masks: None,
            },
        )
        .unwrap();
        assert_eq!(plain_out.frame, frame);

        let blurred_recipe = EditRecipe {
            lens_blur: Some(blur()),
            ..Default::default()
        };
        let blurred_out = render_frame(
            &frame,
            &RenderContext {
                recipe: &blurred_recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                depth: None,
                masks: None,
            },
        )
        .unwrap();
        // Manual stage application matches the integrated render byte-exactly.
        let mut expected = frame.clone();
        apply_lens_blur(&mut expected, &blur(), None).unwrap();
        assert_eq!(blurred_out.frame, expected);
        assert_ne!(blurred_out.frame, frame);

        // Missing depth artifact: the integrated render aborts, no pixels out.
        let missing_recipe = EditRecipe {
            lens_blur: Some(LensBlur {
                depth_artifact: Some(DepthArtifactRef {
                    relative_path: "depth/raw.bin".into(),
                    sha256: "abc".into(),
                }),
                ..blur()
            }),
            ..Default::default()
        };
        assert!(render_frame(
            &frame,
            &RenderContext {
                recipe: &missing_recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                depth: None,
                masks: None,
            },
        )
        .is_err());
    }

    /// G-05 Golden/PSNR gates (documented tolerances): the blur visibly
    /// changes the frame (PSNR vs. source below 40 dB), the three bokeh
    /// shapes are pairwise distinguishable, and re-renders are byte-identical
    /// (PSNR infinite).
    #[test]
    fn golden_psnr_gates() {
        use crate::psnr;

        let frame = striped_frame_sized(32, 32);
        let mut round = frame.clone();
        let mut elliptical = frame.clone();
        let mut hexagonal = frame.clone();
        apply_lens_blur(&mut round, &blur(), None).unwrap();
        let mut e = blur();
        e.bokeh = BokehShape::Elliptical;
        apply_lens_blur(&mut elliptical, &e, None).unwrap();
        let mut h = blur();
        h.bokeh = BokehShape::Hexagonal;
        apply_lens_blur(&mut hexagonal, &h, None).unwrap();

        // Visible effect: PSNR against the source is finite and below 40 dB.
        for (name, out) in [
            ("round", &round),
            ("elliptical", &elliptical),
            ("hexagonal", &hexagonal),
        ] {
            let value = psnr(&frame, out);
            assert!(
                value.is_finite() && value < 40.0,
                "{name} must visibly blur (PSNR {value} dB)"
            );
        }
        // Pairwise distinguishable: PSNR between shapes is finite (not equal).
        assert!(psnr(&round, &elliptical).is_finite());
        assert!(psnr(&round, &hexagonal).is_finite());
        assert!(psnr(&elliptical, &hexagonal).is_finite());
        // Deterministic re-render: byte-identical (PSNR infinite).
        let mut rerun = frame.clone();
        apply_lens_blur(&mut rerun, &blur(), None).unwrap();
        assert_eq!(rerun, round);
        assert_eq!(psnr(&round, &rerun), f64::INFINITY);
    }
}
