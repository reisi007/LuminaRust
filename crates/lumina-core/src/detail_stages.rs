//! The shared detail mathematics of the global recipe and the mask-local
//! MASK-LOCAL-P1.2d block.
//!
//! This module owns **every number** of the two F-095/F-096 detail stages: the
//! bilateral weights and accumulators of the 5x5 noise-reduction kernel, the
//! chroma offsets, the separable-Gaussian kernel and its radius formula, the
//! Rec.709 luminance, the `gx`/`gy` gradient magnitude and its frame maximum,
//! the detail mixing and the flat-area masking factor. The global kernel
//! ([`crate::ImageFrame::apply_recipe`]) and the mask-local chain
//! ([`crate::render::local_detail`]) call these very functions, so "a local
//! detail is the global detail at a different place in the chain" is a
//! structural property, not two kernels kept in agreement by hand.
//!
//! # One implementation, two quantization policies
//!
//! * **Where the quantization happens.** The global kernel owns a `u8` frame, so
//!   it rounds back to `u8` after the noise-reduction stage and again after the
//!   sharpening write. The local chain owns one un-quantized `f32` plane and
//!   rounds exactly **once**, at the end of the whole layer. This quantization
//!   divergence is intentional, documented and tested — and there deliberately
//!   is **no** test claiming byte-equality between the two paths, because it is
//!   false by design.
//! * **Which plane the neighbourhood reads.** The global kernel reads an RGBA8
//!   snapshot ([`Rgba8Plane`]), the local chain reads the un-quantized float
//!   chain ([`FloatPlane`]). The *arithmetic* is identical: both planes are
//!   `f32` in `0..=255`, and a `u8` sample converts to the exactly representable
//!   `f32` of the same value, so every `f32` accumulation is bit-for-bit the
//!   same operation.
//!
//! # The position inside the local chain
//!
//! These stages run **last** inside a mask-local layer, over the un-quantized
//! whole-frame plane the local tone curve and the four local colour stages have
//! already written — the exact mirror of the global kernel, which runs its colour
//! stages before its noise reduction and its sharpening. That order is load
//! bearing: colour changes neighbour luminance, and neighbour luminance is what
//! the bilateral similarity weights, the separable Gaussian and the
//! whole-frame gradient maximum are computed from. It is pinned in
//! `render::tests::local_detail_order` against two independently transcribed
//! chains, in one layer that carries both a colour block and a detail block.
//!
//! # Full-frame neighbourhoods, always
//!
//! Nothing here takes a mask, a region of interest, or a window size derived
//! from one. The 5x5 bilateral window, the separable Gaussian support and the
//! gradient maximum are the whole image. The mask-local detail therefore has
//! **no** mask-dependent statistic, **no** ROI resize fallback and **no** seam at
//! the mask edge: the mask only ever gates the blend amount, in the P0
//! compositor. A layer's render identity therefore depends on the persisted
//! detail values, the global render scale and the mask plane, never on the image
//! geometry.
//!
//! All of the arithmetic is `f32` on purpose: that is the domain the global
//! kernel has always used for these two stages, and sharing the code means
//! sharing the domain. It is not an extra quantization boundary — the local
//! layer's single boundary remains its final RGBA8 rounding.

use lumina_sidecar::{NoiseReduction, Sharpening};

/// Rec.709 luminance weights. One place, so the noise-reduction kernel, the
/// sharpening detail and the tests all name the same coefficients.
pub(crate) const LUMA_R: f32 = 0.2126;
pub(crate) const LUMA_G: f32 = 0.7152;
pub(crate) const LUMA_B: f32 = 0.0722;

/// The 5x5 noise-reduction window radius. F-096 documents a fixed 5x5
/// neighbourhood, so this is a constant, not a recipe value.
pub(crate) const NOISE_RADIUS: i32 = 2;

/// The Gaussian support of the sharpening kernel: three sigma, rounded up.
pub(crate) const SHARPEN_SIGMA_SUPPORT: f32 = 3.0;

/// The smallest Gaussian sigma the F-095 stage will use. At exactly this sigma
/// the first off-centre tap still has weight `exp(-1/(2*0.5^2)) = exp(-2)`, so
/// the kernel is **not** an identity — which is why a stored radius can never
/// make a sharpening block pixel-neutral.
pub(crate) const SHARPEN_SIGMA_FLOOR: f32 = 0.5;

/// Read-only access to a three-channel plane in `0..=255`, so one kernel serves
/// the global RGBA8 snapshot and the local un-quantized float chain.
pub(crate) trait DetailPlane {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn channel(&self, x: usize, y: usize, c: usize) -> f32;
}

/// The global kernel's RGBA8 neighbourhood source.
pub(crate) struct Rgba8Plane<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl DetailPlane for Rgba8Plane<'_> {
    fn width(&self) -> usize {
        self.width
    }
    fn height(&self) -> usize {
        self.height
    }
    fn channel(&self, x: usize, y: usize, c: usize) -> f32 {
        f32::from(self.pixels[(y * self.width + x) * 4 + c])
    }
}

/// The mask-local chain's un-quantized neighbourhood source.
pub(crate) struct FloatPlane<'a> {
    pub pixels: &'a [[f32; 3]],
    pub width: usize,
    pub height: usize,
}

impl DetailPlane for FloatPlane<'_> {
    fn width(&self) -> usize {
        self.width
    }
    fn height(&self) -> usize {
        self.height
    }
    fn channel(&self, x: usize, y: usize, c: usize) -> f32 {
        self.pixels[y * self.width + x][c]
    }
}

/// Rec.709 luminance of one `(0..=255)` triple.
pub(crate) fn luminance(red: f32, green: f32, blue: f32) -> f32 {
    LUMA_R * red + LUMA_G * green + LUMA_B * blue
}

// ------------------------------------------------------- Noise reduction (F-096)

/// The three un-quantized `(0..=255)` noise-reduction writes of one pixel:
/// `Y' + c_r'`, `Y' + c_g`, `Y' + c_b`.
///
/// The global kernel rounds and clips them into `u8`; the mask-local chain
/// carries them into its single final quantization. The caller owns that
/// decision, which is the only place the two paths differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NoiseWrite {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

/// The fixed F-096 bilateral kernel over the clamped 5x5 window around `(x, y)`.
///
/// Luminance weights are `exp(-d²/(2·1.5²)) · exp(-(Y-Yn)²/(2·(0.12·255)²))`;
/// the chroma offsets use the same 5x5 spatial window with sigma `2.0` and no
/// similarity term. Edges replicate, and the window is walked in the global
/// kernel's own `dy`-outer/`dx`-inner order with the accumulators updated in
/// its own order (weighted luminance, then the weight sum, then the chroma
/// weight sum, then red, then blue), so every `f32` sum is bit-for-bit the
/// historical one.
pub(crate) fn noise_reduction_write<P: DetailPlane + ?Sized>(
    plane: &P,
    x: usize,
    y: usize,
    n: &NoiseReduction,
) -> NoiseWrite {
    let (width, height) = (plane.width(), plane.height());
    let base_luminance = luminance(
        plane.channel(x, y, 0),
        plane.channel(x, y, 1),
        plane.channel(x, y, 2),
    );
    let mut luminance_sum = 0.0_f32;
    let mut chroma_red_sum = 0.0_f32;
    let mut chroma_blue_sum = 0.0_f32;
    let mut weight_sum = 0.0_f32;
    let mut chroma_weight_sum = 0.0_f32;
    for dy in -NOISE_RADIUS..=NOISE_RADIUS {
        for dx in -NOISE_RADIUS..=NOISE_RADIUS {
            let xx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
            let yy = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
            let neighbour_luminance = luminance(
                plane.channel(xx, yy, 0),
                plane.channel(xx, yy, 1),
                plane.channel(xx, yy, 2),
            );
            let squared_distance = (dx * dx + dy * dy) as f32;
            let spatial = (-squared_distance / (2.0 * 1.5 * 1.5)).exp();
            let similar = (-((base_luminance - neighbour_luminance).powi(2))
                / (2.0 * 0.12 * 255.0 * 0.12 * 255.0))
                .exp();
            let weight = spatial * similar;
            luminance_sum += weight * neighbour_luminance;
            weight_sum += weight;
            let chroma_weight = (-squared_distance / (2.0 * 2.0 * 2.0)).exp();
            chroma_weight_sum += chroma_weight;
            chroma_red_sum += chroma_weight * (plane.channel(xx, yy, 0) - neighbour_luminance);
            chroma_blue_sum += chroma_weight * (plane.channel(xx, yy, 2) - neighbour_luminance);
        }
    }
    let filtered_luminance = luminance_sum / weight_sum;
    let mixed_luminance = base_luminance * (1.0 - n.luminance) + filtered_luminance * n.luminance;
    let red_chroma = plane.channel(x, y, 0) - base_luminance;
    let green_chroma = plane.channel(x, y, 1) - base_luminance;
    let blue_chroma = plane.channel(x, y, 2) - base_luminance;
    let red = red_chroma * (1.0 - n.color) + (chroma_red_sum / chroma_weight_sum) * n.color;
    let blue = blue_chroma * (1.0 - n.color) + (chroma_blue_sum / chroma_weight_sum) * n.color;
    // Green chroma is preserved from the source, exactly as the global kernel
    // has always derived it.
    NoiseWrite {
        red: mixed_luminance + red,
        green: mixed_luminance + green_chroma,
        blue: mixed_luminance + blue,
    }
}

// ------------------------------------------------------------ Sharpening (F-095)

/// The global radius formula: `sigma = max(radius * effective_scale, 0.5)`.
///
/// `effective_scale` is the **global** render scale — the very value the global
/// F-096 sharpening stage is given and the very value the mask-local detail
/// block follows. The local block has no scale option of its own and never
/// overrides the global one.
pub(crate) fn sharpen_sigma(radius: f32, effective_scale: f32) -> f32 {
    (radius * effective_scale).max(SHARPEN_SIGMA_FLOOR)
}

/// The integer kernel half-width `ceil(3 * sigma)`.
pub(crate) fn sharpen_support(sigma: f32) -> i32 {
    (sigma * SHARPEN_SIGMA_SUPPORT).ceil() as i32
}

/// The two radii the F-095 detail mix is built from: `max(0.5·r, 0.5)` for the
/// fine and `max(1.5·r, 0.5)` for the coarse pass, each then scaled by
/// [`sharpen_sigma`].
pub(crate) fn sharpen_blur_radii(sharpening: &Sharpening) -> (f32, f32) {
    (
        (sharpening.radius * 0.5).max(SHARPEN_SIGMA_FLOOR),
        (sharpening.radius * 1.5).max(SHARPEN_SIGMA_FLOOR),
    )
}

/// The normalized separable-Gaussian kernel of half-width `radius`, in the
/// global kernel's own `k = -r..=r` order and normalization.
pub(crate) fn gaussian_kernel(radius: f32, effective_scale: f32) -> Vec<f32> {
    let sigma = sharpen_sigma(radius, effective_scale);
    let r = sharpen_support(sigma);
    let mut kernel = Vec::new();
    for k in -r..=r {
        kernel.push((-(k * k) as f32 / (2.0 * sigma * sigma)).exp());
    }
    let z: f32 = kernel.iter().sum();
    for value in &mut kernel {
        *value /= z;
    }
    kernel
}

/// The separable Gaussian over a whole luminance plane, replicating edges.
pub(crate) fn gaussian_blur(
    lum: &[f32],
    width: usize,
    height: usize,
    radius: f32,
    effective_scale: f32,
) -> Vec<f32> {
    let kernel = gaussian_kernel(radius, effective_scale);
    let r = (kernel.len() as i32 - 1) / 2;
    let mut tmp = vec![0.0_f32; width * height];
    let mut out = vec![0.0_f32; width * height];
    for y in 0..height {
        for x in 0..width {
            for k in -r..=r {
                tmp[y * width + x] += kernel[(k + r) as usize]
                    * lum[y * width + (x as i32 + k).clamp(0, width as i32 - 1) as usize];
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            for k in -r..=r {
                out[y * width + x] += kernel[(k + r) as usize]
                    * tmp[(y as i32 + k).clamp(0, height as i32 - 1) as usize * width + x];
            }
        }
    }
    out
}

/// The whole-frame `|gx| + |gy|` gradient magnitude plane and its maximum.
///
/// The maximum is taken over the **entire frame** — the flat-area masking factor
/// is a whole-image normalisation, never a mask-restricted one. Both are
/// full-frame by construction: the function only ever sees the luminance plane.
pub(crate) fn gradient_plane(lum: &[f32], width: usize, height: usize) -> (Vec<f32>, f32) {
    let mut gradients = vec![0.0_f32; width * height];
    let mut max_gradient: f32 = 0.0;
    for y in 0..height {
        for x in 0..width {
            let gx = lum[y * width + (x as i32 + 1).min(width as i32 - 1) as usize]
                - lum[y * width + x.saturating_sub(1)];
            let gy = lum[(y as i32 + 1).min(height as i32 - 1) as usize * width + x]
                - lum[y.saturating_sub(1) * width + x];
            gradients[y * width + x] = gx.abs() + gy.abs();
            max_gradient = max_gradient.max(gradients[y * width + x]);
        }
    }
    (gradients, max_gradient)
}

/// The F-095 detail mix `detail·d_fine + (1−detail)·d_coarse`.
pub(crate) fn sharpen_detail(lum: f32, fine: f32, coarse: f32, detail: f32) -> f32 {
    detail * (lum - fine) + (1.0 - detail) * (lum - coarse)
}

/// The normalized edge term of the flat-area masking. A frame without any
/// gradient at all resolves to `0.0`, which is the global kernel's documented
/// behaviour.
pub(crate) fn sharpen_edge_factor(gradient: f32, max_gradient: f32) -> f32 {
    if max_gradient > 0.0 {
        (gradient / max_gradient).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The per-pixel sharpening amount `amount · ((1−masking) + masking·edge)`.
///
/// `masking = 0` is the **strongest** setting: the factor becomes `1.0` for every
/// pixel, so a whole frame including its flat areas is sharpened. That is why a
/// `masking = 0` block with a non-zero amount must never be treated as a no-op.
pub(crate) fn sharpen_amount(sharpening: &Sharpening, edge: f32) -> f32 {
    sharpening.amount * ((1.0 - sharpening.masking) + sharpening.masking * edge)
}

/// The luminance-preserving ratio the sharpening applies to the colour channels:
/// `clamp(lum + amount·d) / lum`, or `0.0` for an (almost) black pixel.
pub(crate) fn sharpen_ratio(lum: f32, amount_times_detail: f32) -> f32 {
    let new_luminance = (lum + amount_times_detail).clamp(0.0, 255.0);
    if lum > 1e-6 {
        new_luminance / lum
    } else {
        0.0
    }
}
