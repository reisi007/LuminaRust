//! Geometry, lens and resampling kernels plus the structural validators that back
//! the `ImageFrame` stages.
//!
//! Pure structural split out of `lib.rs` (FILE-SIZE-1000-ALL-EXCEPTIONS-100):
//! every function body is byte-identical to its former root location.

use crate::for_each_rgba_mut;
use crate::geometry_stage_api::validate_perspective;
use crate::{ChannelLutParams, CoreError, ImageFrame, MemoryBudget};

pub(crate) fn apply_channel_lut_adjustments(pixels: &mut [u8], params: &ChannelLutParams) {
    let ChannelLutParams {
        wb_gains,
        exposure_multiplier,
        contrast_factor,
        shadows,
        highlights,
        whites,
        blacks,
    } = params;
    if wb_gains.is_none()
        && exposure_multiplier.is_none()
        && contrast_factor.is_none()
        && shadows.is_none()
        && highlights.is_none()
        && whites.is_none()
        && blacks.is_none()
    {
        return;
    }

    // Per-channel lookup tables: `lut[channel][value]` is the composed result for
    // that channel at the given input byte. The table build runs the same `f64`
    // math as the original per-pixel passes, but only 256 times per channel.
    let mut lut = [[0u8; 256]; 3];
    for (channel, lut_channel) in lut.iter_mut().enumerate() {
        for input in 0u16..=255 {
            let mut value = input as f64;
            if let Some(gains) = wb_gains {
                value = (value * gains[channel]).round().clamp(0.0, 255.0);
            }
            if let Some(multiplier) = exposure_multiplier {
                value = (value * multiplier).round().clamp(0.0, 255.0);
            }
            if let Some(factor) = contrast_factor {
                value = ((value - 128.0) * factor + 128.0).round().clamp(0.0, 255.0);
            }
            if let Some(amount) = shadows {
                let x = value / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
                value = ((x + amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            if let Some(amount) = highlights {
                let x = value / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
                value = ((x + amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            if let Some(amount) = whites {
                let x = value / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0);
                value = ((x + amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            if let Some(amount) = blacks {
                let x = value / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0);
                value = ((x - amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            lut_channel[input as usize] = value.clamp(0.0, 255.0) as u8;
        }
    }

    // Single fused pass: three table lookups per pixel, no floating point.
    // `for_each_rgba_mut` keeps this auto-vectorizable (and parallel under the
    // `parallel` feature) while staying bit-identical to a sequential loop.
    for_each_rgba_mut(pixels, |pixel| {
        pixel[0] = lut[0][pixel[0] as usize];
        pixel[1] = lut[1][pixel[1] as usize];
        pixel[2] = lut[2][pixel[2] as usize];
    });
}

pub(crate) fn validate_generative_edit(
    g: &lumina_sidecar::GenerativeEdit,
) -> Result<(), CoreError> {
    // GEN-PIPELINE-DECOUPLE: structural validation of the GenerativeEdit
    // recipe stage (mirrors the sidecar rules). Any violation is
    // `InvalidAdjustment` — never a silent fallback to "render as if the
    // stage were absent".
    if g.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_edit.version".into(),
            value: g.version as f64,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    let expand = g.expand_beyond_image.unwrap_or(false);
    if expand && g.canvas.is_none() {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_edit.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    if !expand && g.canvas.is_some() {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_edit.canvas".into(),
            value: 1.0,
            minimum: 0.0,
            maximum: 0.0,
        });
    }
    if let Some(canvas) = &g.canvas {
        if canvas.output_width == 0 || canvas.output_height == 0 {
            return Err(CoreError::InvalidAdjustment {
                name: "generative_edit.canvas.output".into(),
                value: 0.0,
                minimum: 1.0,
                maximum: f64::from(u32::MAX),
            });
        }
    }
    Ok(())
}
/// LRPAR-G06-UPRIGHT-15: validiert die additive Upright-Rezept-Stufe laut.
/// `enabled` ohne persistierte Analyse wird abgelehnt (kein stiller
/// Identitäts-Render); die Vorschlagswerte bleiben in der F-099-Domäne.
pub fn validate_upright(u: &lumina_sidecar::Upright) -> Result<(), CoreError> {
    let invalid = |name: &str, value: f64, lo: f64, hi: f64| CoreError::InvalidAdjustment {
        name: name.into(),
        value,
        minimum: lo,
        maximum: hi,
    };
    if u.version != 1 {
        return Err(invalid("upright.version", u.version as f64, 1.0, 1.0));
    }
    if u.enabled && u.analysis.is_none() {
        return Err(CoreError::UnsupportedAdjustment {
            key: "upright enabled without a persisted analysis".into(),
        });
    }
    if let Some(analysis) = &u.analysis {
        for (field, value) in [
            ("vertical", analysis.vertical),
            ("horizontal", analysis.horizontal),
            ("rotation", analysis.rotation),
        ] {
            if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                return Err(invalid(
                    &format!("upright.{field}"),
                    value as f64,
                    -1.0,
                    1.0,
                ));
            }
        }
        if !analysis.confidence.is_finite() || !(0.0..=1.0).contains(&analysis.confidence) {
            return Err(invalid(
                "upright.confidence",
                analysis.confidence as f64,
                0.0,
                1.0,
            ));
        }
        if analysis.line_count > lumina_sidecar::UPRIGHT_MAX_LINE_COUNT {
            return Err(invalid(
                "upright.line_count",
                analysis.line_count as f64,
                0.0,
                lumina_sidecar::UPRIGHT_MAX_LINE_COUNT as f64,
            ));
        }
        for (field, value) in [
            ("algorithm", &analysis.fingerprint.algorithm),
            ("version", &analysis.fingerprint.version),
            ("input_fingerprint", &analysis.fingerprint.input_fingerprint),
        ] {
            if value.is_empty() {
                return Err(CoreError::UnsupportedAdjustment {
                    key: format!("upright fingerprint {field} must not be empty"),
                });
            }
        }
    }
    Ok(())
}

/// Forward matrix is `T(shift) * R(rotation*pi/4) * S(scale,aspect) *
/// Hy(vertical) * Hx(horizontal)` on column vectors and normalized corners
/// `[-1,1]^2`.  `Hx=[[1,0,0],[0,1,0],[tan(h),0,1]]`,
/// `Hy=[[1,0,0],[0,1,0],[0,tan(v),1]]`, `S=diag(scale,scale*aspect,1)`,
/// and `T,R` are the usual translation and counter-clockwise rotation.
/// Rendering uses the inverse of this exact matrix for every output pixel.
fn perspective_matrix(p: &lumina_sidecar::Perspective) -> [[f32; 3]; 3] {
    let sh = (p.horizontal * std::f32::consts::FRAC_PI_4).tan();
    let sv = (p.vertical * std::f32::consts::FRAC_PI_4).tan();
    let a = p.rotation * std::f32::consts::FRAC_PI_4;
    let (s, c) = (a.sin(), a.cos());
    // `scale` is an output magnification: scale=2 doubles the projected
    // bounding box instead of shrinking it to half size.
    let sx = p.scale;
    let sy = p.scale * p.aspect_ratio;
    let t = [[1., 0., p.shift_x], [0., 1., p.shift_y], [0., 0., 1.]];
    let r = [[c, -s, 0.], [s, c, 0.], [0., 0., 1.]];
    let scale = [[sx, 0., 0.], [0., sy, 0.], [0., 0., 1.]];
    let hy = [[1., 0., 0.], [0., 1., 0.], [0., sv, 1.]];
    let hx = [[1., 0., 0.], [0., 1., 0.], [sh, 0., 1.]];
    fn mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut o = [[0.; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    o[i][j] += a[i][k] * b[k][j];
                }
            }
        }
        o
    }
    mul(mul(mul(mul(t, r), scale), hy), hx)
}

pub(crate) fn perspective_dimensions(
    w: u32,
    h: u32,
    p: Option<&lumina_sidecar::Perspective>,
) -> Result<(u32, u32), CoreError> {
    let Some(p) = p else {
        return Ok((w, h));
    };
    validate_perspective(p)?;
    let m = perspective_matrix(p);
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let d = m[2][0] * x + m[2][1] * y + m[2][2];
            if !d.is_finite() || d.abs() < 1e-6 {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: d as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            if !q[0].is_finite() || !q[1].is_finite() {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: q[0] as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    let ow_f = ((max[0] - min[0]) * w as f32 / 2.0).ceil().max(1.0);
    let oh_f = ((max[1] - min[1]) * h as f32 / 2.0).ceil().max(1.0);
    if !ow_f.is_finite() || !oh_f.is_finite() {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective".into(),
            value: ow_f as f64,
            minimum: 1.0,
            maximum: 1_000_000.0,
        });
    }
    let ow = ow_f as u32;
    let oh = oh_f as u32;
    // Bounding box limit + MemoryBudget guard (REVIEW-CORE-PERSP-1).
    MemoryBudget::default()
        .check_decode(ow as u64, oh as u64, 4, 1)
        .map_err(|e| CoreError::InvalidAdjustment {
            name: "perspective canvas too large".into(),
            value: ow as f64 * oh as f64,
            minimum: 0.0,
            maximum: e.limit() as f64,
        })?;
    Ok((ow, oh))
}

// Presets are deliberately small and built in: wide-light (k1=0.12,k2=-0.04,k3=0.01,
// c0=1,c1=0,c2=0, CA R=0.006 B=-0.006), standard-neutral (0,0,0,1,0,0,0,0),
// tele-light (k1=-0.08,k2=0.02,k3=0,c0=1,c1=0,c2=0, CA R=-0.004 B=0.004).
fn lens_coefficients(l: &lumina_sidecar::LensCorrection) -> [f32; 8] {
    let mut c = match l.profile.as_deref() {
        Some("wide-light") => [0.12, -0.04, 0.01, 1., 0., 0., 0.006, -0.006],
        Some("tele-light") => [-0.08, 0.02, 0., 1., 0., 0., -0.004, 0.004],
        Some("standard-neutral") => [0., 0., 0., 1., 0., 0., 0., 0.],
        None => [0., 0., 0., 1., 0., 0., 0., 0.],
        Some(other) => panic!("validated lens profile unexpectedly reached renderer: {other}"),
    };
    let explicit = [
        l.distortion_k1.unwrap_or(c[0]),
        l.distortion_k2.unwrap_or(c[1]),
        l.distortion_k3.unwrap_or(c[2]),
        l.vignette_c0.unwrap_or(c[3]),
        l.vignette_c1.unwrap_or(c[4]),
        l.vignette_c2.unwrap_or(c[5]),
        l.ca_red.unwrap_or(c[6]),
        l.ca_blue.unwrap_or(c[7]),
    ];
    c.copy_from_slice(&explicit);
    c
}
pub(crate) fn sample(frame: &ImageFrame, x: f32, y: f32, ch: usize) -> f32 {
    if x < 0.0 || y < 0.0 || x >= frame.width as f32 || y >= frame.height as f32 {
        return 0.0;
    }
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(frame.width - 1);
    let y1 = (y0 + 1).min(frame.height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let at = |xx, yy| frame.pixels[(yy * frame.width + xx) as usize * 4 + ch] as f32;
    (at(x0, y0) * (1. - fx) + at(x1, y0) * fx) * (1. - fy)
        + (at(x0, y1) * (1. - fx) + at(x1, y1) * fx) * fy
}
/// Empty manual lens model used to drive [`apply_lens`] through the Lensfun
/// corrector path when the recipe carries no manual `LensCorrection` (F-098-N1).
#[cfg(feature = "lensfun")]
pub(crate) const EMPTY_LENS: lumina_sidecar::LensCorrection = lumina_sidecar::LensCorrection {
    version: 1,
    profile: None,
    distortion_k1: None,
    distortion_k2: None,
    distortion_k3: None,
    vignette_c0: None,
    vignette_c1: None,
    vignette_c2: None,
    ca_red: None,
    ca_blue: None,
};

pub(crate) fn apply_lens(
    frame: &mut ImageFrame,
    l: &lumina_sidecar::LensCorrection,
    #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
) {
    // F-098-N1: a Lensfun corrector (when present and non-identity) replaces the
    // manual radial-distortion Newton iteration and the vignette polynomial with
    // the database profile. The corrector's geometry maps a destination
    // (corrected) pixel `(x, y)` in `[0, width-1] × [0, height-1]` to the
    // source (distorted) pixel to sample — the same pixel space `apply_lens`
    // iterates over. Vignetting is applied on the RGB channels only; the alpha
    // channel is left untouched (same structure as the manual model).
    //
    // R2-LENS-01: the loop is *row-wise* — every row crosses the lensfun FFI
    // boundary once per pixel for the geometry via `geometry_row` (which
    // issues one native width=1 call per column, bit-identical to per-pixel
    // `geometry` — a single multi-pixel native geometry batch call is
    // provably wrong on x86_64, see the `lumina-lensfun` docs) plus once per
    // row for the vignetting via `apply_vignetting_row`. The vignetting batch
    // results are not byte-identical to the previous per-pixel `color_gain`
    // calls: the first column is bit-identical, but later columns drift by
    // float rounding that grows with the row width (documented in the
    // `lumina-lensfun` wrapper contract). That is why this switch
    // deliberately changes the output and requires a Golden rebaseline
    // (F-043), it is not a silent fallback.
    //
    // G-06 Lensfun-Vollausbau (TCA): when the corrector carries TCA
    // calibration (`has_tca`), R/G/B are sampled at separate subpixel
    // coordinates (`subpixel_row`, green = reference like the manual CA
    // model) in the SAME lens-stage resampling pass — TCA is a geometric
    // lens property, so it is corrected together with the distortion, before
    // perspective (unlike the manual channel-scale hack, which stays after
    // perspective in `apply_perspective_stage`). The manual `apply_ca` must
    // therefore be skipped whenever this TCA path ran (no double
    // correction); see `lensfun_tca_active`.
    #[cfg(feature = "lensfun")]
    if let Some(corrector) = lensfun {
        if !corrector.is_identity() {
            let src = frame.clone();
            let width = frame.width as usize;
            if corrector.has_tca() {
                let mut coords = vec![((0.0, 0.0), (0.0, 0.0), (0.0, 0.0)); width];
                let mut rgb = vec![0f32; width * 3];
                for y in 0..frame.height {
                    corrector.subpixel_row(0.0, y as f64, &mut coords);
                    for (i, (r, g, b)) in coords.iter().enumerate() {
                        rgb[i * 3] = sample(&src, r.0 as f32, r.1 as f32, 0);
                        rgb[i * 3 + 1] = sample(&src, g.0 as f32, g.1 as f32, 1);
                        rgb[i * 3 + 2] = sample(&src, b.0 as f32, b.1 as f32, 2);
                    }
                    corrector.apply_vignetting_row(&mut rgb, 0.0, y as f64);
                    let row_base = y as usize * width * 4;
                    for (i, (_, g, _)) in coords.iter().enumerate() {
                        let dst = row_base + i * 4;
                        frame.pixels[dst] = rgb[i * 3].round().clamp(0.0, 255.0) as u8;
                        frame.pixels[dst + 1] = rgb[i * 3 + 1].round().clamp(0.0, 255.0) as u8;
                        frame.pixels[dst + 2] = rgb[i * 3 + 2].round().clamp(0.0, 255.0) as u8;
                        frame.pixels[dst + 3] = sample(&src, g.0 as f32, g.1 as f32, 3)
                            .round()
                            .clamp(0.0, 255.0)
                            as u8;
                    }
                }
                return;
            }
            let mut coords = vec![(0.0, 0.0); width];
            let mut rgb = vec![0f32; width * 3];
            for y in 0..frame.height {
                // One batch FFI call maps the whole destination row to its
                // source row (identical to `geometry` per pixel up to the
                // documented sub-pixel drift).
                corrector.geometry_row(0.0, y as f64, &mut coords);
                for (i, (ref sx, ref sy)) in coords.iter().enumerate() {
                    rgb[i * 3] = sample(&src, *sx as f32, *sy as f32, 0);
                    rgb[i * 3 + 1] = sample(&src, *sx as f32, *sy as f32, 1);
                    rgb[i * 3 + 2] = sample(&src, *sx as f32, *sy as f32, 2);
                }
                // One batch FFI call applies the vignetting gain to the whole
                // sampled RGB row in place.
                corrector.apply_vignetting_row(&mut rgb, 0.0, y as f64);
                let row_base = y as usize * width * 4;
                for (i, (ref sx, ref sy)) in coords.iter().enumerate() {
                    let dst = row_base + i * 4;
                    frame.pixels[dst] = rgb[i * 3].round().clamp(0.0, 255.0) as u8;
                    frame.pixels[dst + 1] = rgb[i * 3 + 1].round().clamp(0.0, 255.0) as u8;
                    frame.pixels[dst + 2] = rgb[i * 3 + 2].round().clamp(0.0, 255.0) as u8;
                    frame.pixels[dst + 3] = sample(&src, *sx as f32, *sy as f32, 3)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
            return;
        }
    }
    // Manual model (unchanged behaviour).
    let c = lens_coefficients(l);
    let src = frame.clone();
    let (w, h) = (frame.width as f32, frame.height as f32);
    let diag = (w * w + h * h).sqrt() / 2.;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let nx = (x as f32 - (w - 1.) / 2.) / diag;
            let ny = (y as f32 - (h - 1.) / 2.) / diag;
            let target = (nx * nx + ny * ny).sqrt();
            let mut r = target;
            for _ in 0..8 {
                let f = r * (1. + c[0] * r * r + c[1] * r.powi(4) + c[2] * r.powi(6)) - target;
                let d = 1. + 3. * c[0] * r * r + 5. * c[1] * r.powi(4) + 7. * c[2] * r.powi(6);
                r = (r - f / d).max(0.);
            }
            let q = if target > 1e-6 { r / target } else { 1. };
            let sx = (w - 1.) / 2. + nx * q * diag;
            let sy = (h - 1.) / 2. + ny * q * diag;
            let vig = (c[3] + c[4] * target * target + c[5] * target.powi(4)).max(0.01);
            let i = (y * frame.width + x) as usize * 4;
            for ch in 0..3 {
                frame.pixels[i + ch] =
                    (sample(&src, sx, sy, ch) * vig).round().clamp(0., 255.) as u8;
            }
            frame.pixels[i + 3] = sample(&src, sx, sy, 3).round().clamp(0., 255.) as u8;
        }
    }
}
pub(crate) fn apply_ca(frame: &mut ImageFrame, l: &lumina_sidecar::LensCorrection) {
    let c = lens_coefficients(l);
    let src = frame.clone();
    let cx = (frame.width as f32 - 1.) / 2.;
    let cy = (frame.height as f32 - 1.) / 2.;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let i = (y * frame.width + x) as usize * 4;
            for (ch, k) in [(0, c[6]), (2, c[7])] {
                let sx = cx + (x as f32 - cx) * (1. + k);
                let sy = cy + (y as f32 - cy) * (1. + k);
                frame.pixels[i + ch] = sample(&src, sx, sy, ch).round().clamp(0., 255.) as u8;
            }
        }
    }
}
pub(crate) fn apply_perspective(
    src: &ImageFrame,
    p: &lumina_sidecar::Perspective,
) -> Result<ImageFrame, CoreError> {
    if p.vertical == 0.
        && p.horizontal == 0.
        && p.rotation == 0.
        && p.scale == 1.
        && p.aspect_ratio == 1.
        && p.shift_x == 0.
        && p.shift_y == 0.
    {
        return Ok(src.clone());
    }
    let m = perspective_matrix(p);
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let d = m[2][0] * x + m[2][1] * y + m[2][2];
            if !d.is_finite() || d.abs() < 1e-6 {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: d as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            if !q[0].is_finite() || !q[1].is_finite() {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: q[0] as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    let ow_f = ((max[0] - min[0]) * src.width as f32 / 2.0).ceil().max(1.0);
    let oh_f = ((max[1] - min[1]) * src.height as f32 / 2.0)
        .ceil()
        .max(1.0);
    if !ow_f.is_finite() || !oh_f.is_finite() {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective".into(),
            value: ow_f as f64,
            minimum: 1.0,
            maximum: 1_000_000.0,
        });
    }
    let ow = ow_f as u32;
    let oh = oh_f as u32;
    MemoryBudget::default()
        .check_decode(ow as u64, oh as u64, 4, 1)
        .map_err(|e| CoreError::InvalidAdjustment {
            name: "perspective canvas too large".into(),
            value: ow as f64 * oh as f64,
            minimum: 0.0,
            maximum: e.limit() as f64,
        })?;
    let ow = ow.max(1);
    let oh = oh.max(1);
    let alloc_len = (ow as usize)
        .checked_mul(oh as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or(CoreError::InvalidAdjustment {
            name: "perspective canvas too large".into(),
            value: ow as f64 * oh as f64,
            minimum: 0.0,
            maximum: MemoryBudget::default().max_alloc_bytes as f64,
        })?;
    // Guard already via MemoryBudget, but keep explicit overflow check.
    let mut out = ImageFrame::new(ow, oh, vec![0; alloc_len]).unwrap();
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if !det.is_finite() || det.abs() < 1e-6 {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective".into(),
            value: det as f64,
            minimum: -1.0,
            maximum: 1.0,
        });
    }
    let inv = |x: f32, y: f32| {
        let z = [x, y, 1.];
        let a = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * z[0]
            + (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * z[1]
            + (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * z[2];
        let b = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * z[0]
            + (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * z[1]
            + (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * z[2];
        let d = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * z[0]
            + (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * z[1]
            + (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * z[2];
        (a / det, b / det, d / det)
    };
    for y in 0..out.height {
        for x in 0..out.width {
            // Keep the output canvas centered. Translation therefore remains
            // visible as a shift within the projected bounding box instead of
            // being cancelled by translating both bbox endpoints.
            let range_x = max[0] - min[0];
            let range_y = max[1] - min[1];
            let canvas_min_x = -range_x / 2.0;
            let canvas_min_y = -range_y / 2.0;
            let nx =
                canvas_min_x + (x as f32 / (out.width.saturating_sub(1).max(1)) as f32) * range_x;
            let ny =
                canvas_min_y + (y as f32 / (out.height.saturating_sub(1).max(1)) as f32) * range_y;
            let (sx, sy, sd) = inv(nx, ny);
            if !sd.is_finite() || sd.abs() < 1e-6 {
                continue;
            }
            let sx = sx / sd;
            let sy = sy / sd;
            let px = (sx / 2. + 0.5) * (src.width - 1) as f32;
            let py = (sy / 2. + 0.5) * (src.height - 1) as f32;
            let i = (y * out.width + x) as usize * 4;
            for ch in 0..4 {
                out.pixels[i + ch] = sample(src, px, py, ch).round().clamp(0., 255.) as u8;
            }
        }
    }
    Ok(out)
}

pub(crate) fn crop_rect(
    width: u32,
    height: u32,
    crop: Option<&lumina_sidecar::Crop>,
) -> Result<(u32, u32, u32, u32), CoreError> {
    let (x, y, w, h) = match crop {
        None => (0.0, 0.0, 1.0, 1.0),
        Some(lumina_sidecar::Crop::Free {
            x,
            y,
            width,
            height,
        }) => (*x as f64, *y as f64, *width as f64, *height as f64),
        Some(lumina_sidecar::Crop::Aspect { preset }) => {
            let ratio = match preset {
                lumina_sidecar::AspectPreset::Original => width as f64 / height as f64,
                lumina_sidecar::AspectPreset::OneToOne => 1.0,
                lumina_sidecar::AspectPreset::FourToFive => 4.0 / 5.0,
                lumina_sidecar::AspectPreset::FiveToFour => 5.0 / 4.0,
                lumina_sidecar::AspectPreset::ThreeToTwo => 3.0 / 2.0,
                lumina_sidecar::AspectPreset::TwoToThree => 2.0 / 3.0,
                lumina_sidecar::AspectPreset::FourToThree => 4.0 / 3.0,
                lumina_sidecar::AspectPreset::ThreeToFour => 3.0 / 4.0,
                lumina_sidecar::AspectPreset::SixteenToNine => 16.0 / 9.0,
                lumina_sidecar::AspectPreset::NineToSixteen => 9.0 / 16.0,
            };
            let source_ratio = width as f64 / height as f64;
            if source_ratio > ratio {
                (
                    (1.0 - ratio / source_ratio) / 2.0,
                    0.0,
                    ratio / source_ratio,
                    1.0,
                )
            } else {
                (
                    0.0,
                    (1.0 - source_ratio / ratio) / 2.0,
                    1.0,
                    source_ratio / ratio,
                )
            }
        }
    };
    if ![x, y, w, h].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || x < 0.0
        || y < 0.0
        // REVIEW-CORE-CROP-1: normalized coordinates are bounded by 1 on BOTH
        // ends. Without the explicit upper bound the 1e-6 tolerance window of
        // the rectangle sum allowed `x` slightly above 1, whose rounded pixel
        // origin could land past the frame edge and underflow the
        // `width - px` extent below.
        || x > 1.0
        || y > 1.0
        || x + w > 1.0 + 1e-6
        || y + h > 1.0 + 1e-6
    {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    // Rounding can move the origin onto (or past) the frame edge even for a
    // legal `0..=1` rectangle (for example x == 1.0 with width rounding to
    // exactly `width`). Clamp the origin into range — keeping at least one
    // pixel for non-degenerate frames — and derive the extent with saturating
    // arithmetic so no u32 underflow can produce a bogus crop rect
    // (REVIEW-CORE-CROP-1).
    if width == 0 || height == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty frame)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let px = (((x * width as f64).round() as i64).clamp(0, i64::from(width) - 1)) as u32;
    let py = (((y * height as f64).round() as i64).clamp(0, i64::from(height) - 1)) as u32;
    let pw = ((w * width as f64).round() as u32).max(1).min(width - px);
    let ph = ((h * height as f64).round() as u32).max(1).min(height - py);
    // REVIEW-CORE-CROP-1: an empty extent is a hard error, never a zero-size
    // frame that would flow through the rest of the pipeline. With both
    // clamps above this only triggers on degenerate inputs; it stays as an
    // explicit guard instead of an implicit invariant.
    if pw == 0 || ph == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty crop rectangle)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    Ok((px, py, pw, ph))
}

pub(crate) fn crop_frame(
    frame: &ImageFrame,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Result<ImageFrame, CoreError> {
    let mut out = vec![0; w as usize * h as usize * 4];
    for row in 0..h {
        let src = ((y + row) * frame.width + x) as usize * 4;
        let dst = (row * w) as usize * 4;
        out[dst..dst + w as usize * 4].copy_from_slice(&frame.pixels[src..src + w as usize * 4]);
    }
    ImageFrame::new(w, h, out)
}

/// CROP-MAXRECT-1: the largest all-content rectangle of `frame`, or the full
/// frame when it has no content at all (degenerate all-transparent frame). The
/// full-frame fallback is the documented identity of the default crop — it
/// never collapses to an empty rectangle.
pub(crate) fn default_content_crop(frame: &ImageFrame) -> (u32, u32, u32, u32) {
    crate::crop_max_rect::maximum_content_rect(frame).unwrap_or((0, 0, frame.width, frame.height))
}
pub(crate) fn rotate_dimensions(w: u32, h: u32, degrees: f32) -> (u32, u32) {
    let quarter_turn = degrees.rem_euclid(180.0).abs() < 1e-4;
    if quarter_turn {
        return (w.max(1), h.max(1));
    }
    let right_angle = (degrees - 90.0).rem_euclid(180.0).abs() < 1e-4;
    if right_angle {
        return (h.max(1), w.max(1));
    }
    let r = degrees.to_radians();
    (
        (w as f32 * r.cos().abs() + h as f32 * r.sin().abs())
            .ceil()
            .max(1.0) as u32,
        (w as f32 * r.sin().abs() + h as f32 * r.cos().abs())
            .ceil()
            .max(1.0) as u32,
    )
}
pub(crate) fn rotate_frame(frame: &ImageFrame, degrees: f32) -> ImageFrame {
    let turns = (degrees / 90.0).round();
    if (degrees - turns * 90.0).abs() < 1e-4 {
        let turn = (turns as i32).rem_euclid(4);
        if turn == 0 {
            return frame.clone();
        }
        let (ow, oh) = if turn % 2 == 0 {
            (frame.width, frame.height)
        } else {
            (frame.height, frame.width)
        };
        let mut out = vec![0; ow as usize * oh as usize * 4];
        for y in 0..frame.height {
            for x in 0..frame.width {
                let (dx, dy) = match turn {
                    1 => (frame.height - 1 - y, x),
                    2 => (frame.width - 1 - x, frame.height - 1 - y),
                    _ => (y, frame.width - 1 - x),
                };
                let source = (y * frame.width + x) as usize * 4;
                let destination = (dy * ow + dx) as usize * 4;
                out[destination..destination + 4]
                    .copy_from_slice(&frame.pixels[source..source + 4]);
            }
        }
        return ImageFrame::new(ow, oh, out).unwrap();
    }
    if degrees.rem_euclid(360.0).abs() < f32::EPSILON {
        return frame.clone();
    }
    let (ow, oh) = rotate_dimensions(frame.width, frame.height, degrees);
    let mut out = vec![0; ow as usize * oh as usize * 4];
    let r = degrees.to_radians();
    let (s, c) = (r.sin(), r.cos());
    for y in 0..oh {
        for x in 0..ow {
            let dx = x as f32 - (ow as f32 - 1.0) / 2.0;
            let dy = y as f32 - (oh as f32 - 1.0) / 2.0;
            let sx = c * dx + s * dy + (frame.width as f32 - 1.0) / 2.0;
            let sy = -s * dx + c * dy + (frame.height as f32 - 1.0) / 2.0;
            if sx >= 0.0 && sy >= 0.0 && sx < frame.width as f32 && sy < frame.height as f32 {
                let x0 = sx.floor() as u32;
                let y0 = sy.floor() as u32;
                let x1 = (x0 + 1).min(frame.width - 1);
                let y1 = (y0 + 1).min(frame.height - 1);
                let fx = sx - x0 as f32;
                let fy = sy - y0 as f32;
                let oi = (y * ow + x) as usize * 4;
                for ch in 0..4 {
                    let a = frame.pixels[(y0 * frame.width + x0) as usize * 4 + ch] as f32;
                    let b = frame.pixels[(y0 * frame.width + x1) as usize * 4 + ch] as f32;
                    let d = frame.pixels[(y1 * frame.width + x0) as usize * 4 + ch] as f32;
                    let e = frame.pixels[(y1 * frame.width + x1) as usize * 4 + ch] as f32;
                    out[oi + ch] = ((a * (1.0 - fx) + b * fx) * (1.0 - fy)
                        + (d * (1.0 - fx) + e * fx) * fy)
                        .round() as u8;
                }
            }
        }
    }
    ImageFrame::new(ow, oh, out).unwrap()
}
pub(crate) fn flip_horizontal(f: &mut ImageFrame) {
    for y in 0..f.height {
        for x in 0..f.width / 2 {
            let a = (y * f.width + x) as usize * 4;
            let b = (y * f.width + f.width - 1 - x) as usize * 4;
            for c in 0..4 {
                f.pixels.swap(a + c, b + c);
            }
        }
    }
}
pub(crate) fn flip_vertical(f: &mut ImageFrame) {
    for y in 0..f.height / 2 {
        for x in 0..f.width {
            let a = (y * f.width + x) as usize * 4;
            let b = ((f.height - 1 - y) * f.width + x) as usize * 4;
            for c in 0..4 {
                f.pixels.swap(a + c, b + c);
            }
        }
    }
}
