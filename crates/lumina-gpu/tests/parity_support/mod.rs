//! Shared parity-test helpers (extracted from `parity.rs`, file-size ratchet
//! DoD §8). Used by the CPU-oracle vs GPU equivalence harness.
#![cfg(feature = "gpu")]

use lumina_core::ImageFrame;
use lumina_sidecar::{
    BokehShape, CurvePoint, DepthArtifactRef, EditRecipe, FocusRect, HslChannel, LensBlur,
};

pub const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped parity check";

/// The strongest CPU↔GPU equivalence property a recipe is asserted at.
#[derive(Clone, Copy, Debug)]
pub enum Equivalence {
    /// Measured byte-identical on both parity frames (`maxAbsDiff == 0`).
    ByteIdentical,
    /// Measured within this many 8-bit codes on both parity frames.
    Bounded(u8),
}

/// Structural PSNR floor for bounded stages (in addition to `maxAbsDiff`).
pub const MIN_PSNR_DB: f64 = 48.0;
/// Maximum absolute **mean signed** per-byte error for bounded stages: the
/// residual must be rounding noise, not a systematic brightness/colour tilt.
pub const MAX_ABS_MEAN_SIGNED_ERROR: f64 = 0.05;

pub fn gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rx = x as f64 / (width as f64 - 1.0).max(1.0);
            let ry = y as f64 / (height as f64 - 1.0).max(1.0);
            pixels.extend_from_slice(&[
                (rx * 255.0).round() as u8,
                (ry * 255.0).round() as u8,
                (((rx + ry) * 0.5) * 255.0).round() as u8,
                255,
            ]);
        }
    }
    ImageFrame::new(width, height, pixels).expect("synthetic gradient frame")
}

/// A flat RGBA8 frame (used for generative artifacts where any divergence from
/// the substituted canvas is obvious).
pub fn solid_frame(width: u32, height: u32, rgba: [u8; 4]) -> ImageFrame {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..width * height {
        pixels.extend_from_slice(&rgba);
    }
    ImageFrame::new(width, height, pixels).expect("synthetic solid frame")
}

pub fn noise_frame(width: u32, height: u32, seed: u64) -> ImageFrame {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..(width * height) {
        pixels.extend_from_slice(&[
            (next() & 0xFF) as u8,
            (next() & 0xFF) as u8,
            (next() & 0xFF) as u8,
            255,
        ]);
    }
    ImageFrame::new(width, height, pixels).expect("synthetic noise frame")
}

pub fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

pub fn psnr_db(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mut mse = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let e = (*x as f64) - (*y as f64);
        mse += e * e;
    }
    let mse = mse / a.len() as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// Mean signed per-byte error `mean(a - b)`. A tolerance that merely capped the
/// maximum would let a systematic brightness shift through; this metric makes
/// such a bias visible (both signs cancel only for zero-mean rounding noise).
pub fn mean_signed_error(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) - (*y as f64))
        .sum();
    sum / a.len() as f64
}

pub fn curve(points: &[(f32, f32)]) -> Vec<CurvePoint> {
    points
        .iter()
        .map(|&(input, output)| CurvePoint { input, output })
        .collect()
}

pub fn hsl(hue: f32, saturation: f32, luminance: f32) -> HslChannel {
    HslChannel {
        hue,
        saturation,
        luminance,
    }
}

/// A G-05 lens-blur stage spec (enabled), heuristic unless `depth_artifact` is
/// set.
pub fn lens_blur_spec(
    amount: f32,
    near: f32,
    far: f32,
    bokeh: BokehShape,
    depth_artifact: Option<DepthArtifactRef>,
) -> LensBlur {
    LensBlur {
        version: 1,
        enabled: true,
        focus_rect: FocusRect {
            x: 0.2,
            y: 0.25,
            width: 0.5,
            height: 0.4,
        },
        focal_near: near,
        focal_far: far,
        blur_amount: amount,
        bokeh,
        depth_artifact,
    }
}

/// A G-05 lens-blur recipe on the focus-rect heuristic (no external depth
/// artifact).
pub fn lens_blur_recipe(amount: f32, near: f32, far: f32, bokeh: BokehShape) -> EditRecipe {
    EditRecipe {
        lens_blur: Some(lens_blur_spec(amount, near, far, bokeh, None)),
        ..Default::default()
    }
}

/// A G-05 lens-blur recipe referencing an external depth artifact.
pub fn external_depth_lens_blur_recipe() -> EditRecipe {
    EditRecipe {
        lens_blur: Some(lens_blur_spec(
            0.5,
            0.2,
            0.7,
            BokehShape::Round,
            Some(DepthArtifactRef {
                relative_path: "depth/map.bin".into(),
                sha256: "unused".into(),
            }),
        )),
        ..Default::default()
    }
}
