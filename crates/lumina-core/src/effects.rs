//! Deterministic per-pixel effects: noise reduction, red-eye correction,
//! sharpening, vignette, grain and export dithering.
//!
//! Pure structural split out of `lib.rs` (FILE-SIZE-1000-ALL-EXCEPTIONS-100):
//! every function body is byte-identical to its former root location.

use crate::detail_stages;

/// F-096: Y is filtered with a 5x5 bilateral kernel
/// `exp(-d²/(2*1.5²))*exp(-(Y-Yn)²/(2*0.12²))`; chroma offsets (R-Y,B-Y)
/// use the same 5x5 spatial window with sigma 2.0 and no similarity term.
/// Strength linearly mixes the source and filtered value. Edges replicate.
///
/// Every number lives in [`detail_stages`], which the mask-local P1.2d chain
/// shares verbatim; this wrapper only keeps the global kernel's `u8`
/// quantization points, so extracting the maths changes **no** global byte.
pub(crate) fn apply_noise_reduction(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    n: &lumina_sidecar::NoiseReduction,
) {
    if n.luminance == 0.0 && n.color == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let src = pixels.to_vec();
    let plane = detail_stages::Rgba8Plane {
        pixels: &src,
        width: w,
        height: h,
    };
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let out = detail_stages::noise_reduction_write(&plane, x, y, n);
            for (c, value) in [out.red, out.green, out.blue].into_iter().enumerate() {
                pixels[i + c] = value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// LRPAR-G14-REDEYE-15 (Release 1.5): deterministic red-eye correction.
///
/// Each region is a normalized pupil center (`x * width`, `y * height`) with
/// a pixel radius of `radius * min(width, height)`. A pixel's correction
/// weight is its red-dominance
/// (`clamp((R - max(G, B)) / max(R, ε), 0, 1)`, so grey and non-red pixels
/// are untouched) times a spatial falloff (full strength inside 75 % of the
/// radius, linear falloff to the edge). Desaturation pulls the red channel
/// toward the Rec.709 luminance; darkening scales all three channels. Alpha
/// is preserved; results are rounded and clipped to `0..=255`. An empty
/// region list (or only zero strengths) is identity. Pure, platform-neutral
/// arithmetic: no FS/IO, no randomness — identical inputs render
/// byte-identical outputs.
pub(crate) fn apply_red_eye(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    red_eye: &lumina_sidecar::RedEyeCorrection,
) {
    if red_eye.regions.is_empty() {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return;
    }
    if red_eye
        .regions
        .iter()
        .all(|r| r.desaturate == 0.0 && r.darken == 0.0)
    {
        return;
    }
    let min_dim = w.min(h) as f32;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let r = pixels[i] as f32 / 255.0;
            let g = pixels[i + 1] as f32 / 255.0;
            let b = pixels[i + 2] as f32 / 255.0;
            let redness = ((r - g.max(b)) / r.max(1e-3)).clamp(0.0, 1.0);
            if redness <= 0.0 {
                continue;
            }
            let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let (mut out_r, mut out_g, mut out_b) = (r, g, b);
            for region in &red_eye.regions {
                let cx = region.x * w as f32;
                let cy = region.y * h as f32;
                let radius_px = region.radius * min_dim;
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = dx.hypot(dy);
                if dist > radius_px {
                    continue;
                }
                // Full strength inside 75 % of the radius, linear falloff to
                // the edge (guarded against a degenerate zero radius, which
                // validation rejects but costs nothing to tolerate here).
                let feather = (radius_px * 0.25).max(1e-6);
                let falloff = ((radius_px - dist) / feather).clamp(0.0, 1.0);
                let desat_k = region.desaturate * falloff * redness;
                out_r += (luminance - out_r) * desat_k;
                let darken_k = region.darken * falloff * redness;
                let factor = 1.0 - darken_k;
                out_r *= factor;
                out_g *= factor;
                out_b *= factor;
            }
            pixels[i] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels[i + 1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels[i + 2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// F-095: separable Gaussian (three-sigma support, replicate edges) on Rec.709
/// luminance. `r_fine=0.5*r`, `r_coarse=1.5*r` (both >=.5); final detail is
/// `detail*d_fine+(1-detail)*d_coarse`. Masking uses
/// `((1-masking)+masking*clamp(|gx|+|gy| / global_max,0,1))`.
///
/// Every number lives in [`detail_stages`], which the mask-local P1.2d chain
/// shares verbatim — including the global `render_scale` radius formula, which
/// the local block follows. This wrapper only keeps the global kernel's `u8`
/// quantization points, so extracting the maths changes **no** global byte.
pub(crate) fn apply_sharpening(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    s: &lumina_sidecar::Sharpening,
    scale: f32,
) {
    if s.amount == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let lum: Vec<f32> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| detail_stages::luminance(f32::from(p[0]), f32::from(p[1]), f32::from(p[2])))
        .collect();
    let (fine_radius, coarse_radius) = detail_stages::sharpen_blur_radii(s);
    let fine = detail_stages::gaussian_blur(&lum, w, h, fine_radius, scale);
    let coarse = detail_stages::gaussian_blur(&lum, w, h, coarse_radius, scale);
    let (gradients, max_gradient) = detail_stages::gradient_plane(&lum, w, h);
    for (idx, p) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let detail = detail_stages::sharpen_detail(lum[idx], fine[idx], coarse[idx], s.detail);
        let edge = detail_stages::sharpen_edge_factor(gradients[idx], max_gradient);
        let ratio =
            detail_stages::sharpen_ratio(lum[idx], detail_stages::sharpen_amount(s, edge) * detail);
        for channel in p.iter_mut().take(3) {
            *channel = (f32::from(*channel) * ratio).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// F-090b Point Color: targeted color selection with a free hue center.
/// Each entry weights pixels by a cyclic triangular function of the hue
/// distance to `hue_center` (`1` at the center, linearly to `0` at
/// `hue_range`; `hue_range == 0` matches only the exact center hue) and
/// applies its shifts weighted: hue rotation (`hue_shift * 30°`), additive
/// saturation and luminance. Entries apply sequentially in list order in
/// sRGB-codified HSL; all-zero shifts are identity. Outputs clip to `0..=1`.
/// Smooth Hermite interpolation `t*t*(3-2t)` clamped to `[0,1]` over
/// `[edge0, edge1]`. Used by the vignette transition.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// F-097: deterministic radial vignette. Applied to the RGB channels only; the
/// alpha channel (index 3) is never touched.
///
/// Invariants: `amount == 0` is an early-return identity; the centre pixel(s)
/// of the image always keep factor `1.0` (normalised radius `0`); for positive
/// `amount` the edges/corners are darkened and for negative `amount` they are
/// lightened; the factor is symmetric under reflection through the centre and
/// monotonic in the normalised radius. `midpoint` shifts where the falloff
/// begins, `roundness` controls the elliptical aspect (1 = circular) and
/// `feather` controls transition softness.
pub(crate) fn apply_vignette(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    v: &lumina_sidecar::Vignette,
) {
    if v.amount == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let cx = (width - 1) as f32 / 2.0;
    let cy = (height - 1) as f32 / 2.0;
    let half_w = ((width - 1) as f32 / 2.0).max(1.0);
    let half_h = ((height - 1) as f32 / 2.0).max(1.0);
    // `roundness == 1` is circular; lower values stretch the falloff along y
    // (elliptical aspect).
    let ry_scale = 1.0 + (1.0 - v.roundness) * 0.5;
    // First pass: normalised radius per pixel, tracking the min/max so the
    // centre pixel(s) always map to radius `0` (factor `1.0`) regardless of
    // parity.
    let mut radii = vec![0.0f32; w * h];
    let mut r_min = f32::MAX;
    let mut r_max = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - cx) / half_w;
            let dy = (y as f32 - cy) / half_h * ry_scale;
            let r = (dx * dx + dy * dy).sqrt();
            let idx = y * w + x;
            radii[idx] = r;
            r_min = r_min.min(r);
            r_max = r_max.max(r);
        }
    }
    let denom = (r_max - r_min).max(1e-6);
    let feather_width = 0.15 + v.feather * 0.7;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let rn = (radii[y * w + x] - r_min) / denom;
            // `midpoint` shifts where the falloff begins (0 at the centre).
            let t = ((rn - v.midpoint) / (1.0 - v.midpoint).max(1e-6)).clamp(0.0, 1.0);
            let falloff = smoothstep(0.5 - feather_width / 2.0, 0.5 + feather_width / 2.0, t);
            let factor = 1.0 - v.amount * falloff;
            for c in 0..3 {
                pixels[i + c] = (pixels[i + c] as f32 * factor).clamp(0.0, 255.0).round() as u8;
            }
        }
    }
}

/// Deterministic, dependency-free integer hash producing a `u32`. Used to
/// derive the per-cell grain noise.
fn grain_hash(mut z: u32) -> u32 {
    z = z.wrapping_add(0x9e3779b9);
    z = (z ^ (z >> 16)).wrapping_mul(0x85ebca6b);
    z = (z ^ (z >> 13)).wrapping_mul(0xc2b2ae35);
    z ^= z >> 16;
    z
}

/// F-097: deterministic procedural grain. One noise value is generated per
/// spatial cell (size controls the cell scale) and the SAME value is added to
/// the R, G and B channels of every pixel in that cell (channel-coupled).
/// `roughness` blends between a smoothed low-frequency field and the raw
/// per-cell noise. The effective seed is folded with the image dimensions, so
/// the same `seed` on the same image reproduces identical grain while a
/// different `seed` changes it. `amount == 0` is an early-return identity; the
/// alpha channel is never touched and channels are clamped to `[0, 255]`.
pub(crate) fn apply_grain(pixels: &mut [u8], width: u32, height: u32, g: &lumina_sidecar::Grain) {
    if g.amount == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    // Derive a dimension-aware seed (deterministic proxy for the RenderKey,
    // which includes the image dimensions).
    let mut seed_state = g.seed;
    seed_state = seed_state.wrapping_add((width as u64) << 32);
    seed_state = seed_state.wrapping_add(height as u64);
    seed_state ^= seed_state >> 32;
    seed_state = seed_state.wrapping_mul(0x9e3779b9);
    let seed32 = grain_hash(seed_state as u32);
    let cell = (1 + (g.size * 7.0).round() as usize).max(1);
    let noise = |cx: u32, cy: u32| -> f32 {
        let n = grain_hash(cx.wrapping_add(seed32)) ^ grain_hash(cy.wrapping_mul(0x85ebca6b));
        (grain_hash(n) as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let cx = (x / cell) as u32;
            let cy = (y / cell) as u32;
            let raw = noise(cx, cy);
            // 3x3 neighbourhood average gives a smoothed, low-frequency field;
            // `roughness` blends between it and the raw per-cell noise.
            let mut sum = 0.0f32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let ncx = (cx as i32 + dx).max(0) as u32;
                    let ncy = (cy as i32 + dy).max(0) as u32;
                    sum += noise(ncx, ncy);
                }
            }
            let low = sum / 9.0;
            let value = low * (1.0 - g.roughness) + raw * g.roughness;
            let delta = (value * g.amount * 40.0).round() as i32;
            for c in 0..3 {
                let v = pixels[i + c] as i32 + delta;
                pixels[i + c] = v.clamp(0, 255) as u8;
            }
        }
    }
}

pub(crate) fn dither_rgba8(pixels: &mut [u8], seed: u64) {
    let mut state = seed ^ 0x9e3779b97f4a7c15;
    for (index, value) in pixels.iter_mut().enumerate() {
        if index % 4 == 3 {
            continue;
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        // The frame is already quantized to u8; a one-code stochastic offset
        // is the useful deterministic approximation for this round-trip.
        let delta = if state & 1 == 0 { -1.0 } else { 1.0 };
        *value = (*value as f64 + delta).round().clamp(0.0, 255.0) as u8;
    }
}
