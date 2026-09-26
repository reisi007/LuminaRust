//! The shared presence mathematics of the global recipe and the mask-local
//! MASK-LOCAL-P1.2c block.
//!
//! # One implementation, two quantization policies
//!
//! Every number in the presence stage — the box mean and the DoG detail, the
//! dark channel, the airlight percentile, the dehaze transmission and the
//! dehaze write — is computed *here*, once. The global kernel
//! ([`crate::ImageFrame::apply_recipe`]) and the mask-local chain
//! ([`crate::render::local_presence`]) call these very functions, so "a local
//! presence is the global presence stage at a different place in the chain" is
//! a structural property and not two kernels kept in agreement by hand.
//!
//! What the two callers do *differ*, and the difference is deliberate:
//!
//! * **Where the quantization happens.** The global kernel owns a `u8` frame, so
//!   it rounds back to `u8` after each sub-stage (after each DoG pass and after
//!   the dehaze). The local chain owns one un-quantized `f32` plane and rounds
//!   exactly **once**, at the end of the whole layer. This quantization
//!   divergence is intentional, documented and tested — and there deliberately
//!   is **no** test claiming byte-equality between the two, because it is false
//!   by design.
//! * **Which plane the neighbourhood reads.** The global kernel reads an RGBA8
//!   snapshot ([`Rgba8Plane`]), the local chain reads the un-quantized float
//!   chain ([`FloatPlane`]). The *arithmetic* is identical: both planes are
//!   `f32` in `0..=255`, and an `u8` sample converts to the exactly
//!   representable `f32` of the same value, so the `f32` box sums, means and
//!   details are bit-for-bit the same operation.
//!
//! # Full-frame statistics, always
//!
//! Nothing here takes a mask, a region of interest, or a window size derived
//! from one. The DoG neighbourhood is the whole image, and the dehaze
//! airlight is a percentile over the whole dark channel. The mask-local
//! presence therefore has **no** mask-dependent statistic, **no** ROI resize
//! fallback and **no** seam at the mask edge: the mask only ever gates the
//! blend amount, in the P0 compositor. A layer's render identity therefore
//! depends on the persisted presence values and the mask plane, never on the
//! image geometry.
//!
//! All of the arithmetic is `f32` on purpose: that is the domain the global
//! kernel has always used for this stage, and sharing the code means sharing
//! the domain. It is not an extra quantization boundary — the local layer's
//! single boundary remains its final RGBA8 rounding.

use lumina_sidecar::Presence;

/// The radius of a box window that replicates at the image edge.
pub(crate) type Radius = usize;

// --------------------------------------------------------------- DoG radii

/// Texture DoG radius: `1 + round(|texture| * 2)`, so a full `±1` amount uses
/// the widest documented texture window of 3.
pub(crate) fn texture_radius(texture: f32) -> Radius {
    1 + (f64::from(texture.abs()) * 2.0).round() as Radius
}

/// Clarity DoG radius: `8 + round(|clarity| * 24)`, so a full `±1` amount uses
/// the widest documented clarity window of 32.
pub(crate) fn clarity_radius(clarity: f32) -> Radius {
    8 + (f64::from(clarity.abs()) * 24.0).round() as Radius
}

// ------------------------------------------------------------ Plane access

/// One full-frame channel plane the presence maths can read.
///
/// Implemented by the RGBA8 frame (global kernel) and by the un-quantized
/// `f32` RGB chain (mask-local kernel). `channel` must return the sample as an
/// `f32` in `0..=255`, which is exactly what both planes hold.
pub(crate) trait PresencePlane {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    /// Sample `c` (`0..3`) of the pixel at `(x, y)` as an `f32` in `0..=255`.
    fn channel(&self, x: usize, y: usize, c: usize) -> f32;
}

/// An RGBA8 frame seen as a presence plane. The global kernel snapshots the
/// frame with `to_vec()` before writing, exactly as it always has, so the
/// neighbourhood still reads pre-stage values.
pub(crate) struct Rgba8Plane<'a> {
    pub pixels: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl PresencePlane for Rgba8Plane<'_> {
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

/// An un-quantized `f32` RGB chain in `0..=255` seen as a presence plane. This
/// is the mask-local kernel's plane: same domain, same arithmetic, but no `u8`
/// rounding has happened yet.
pub(crate) struct FloatPlane<'a> {
    pub pixels: &'a [[f32; 3]],
    pub width: usize,
    pub height: usize,
}

impl PresencePlane for FloatPlane<'_> {
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

// ------------------------------------------------------------- Box / DoG

/// The clamped window bounds `(y0, y1, x0, x1)` of the box kernel around
/// `(x, y)`. The image edge replicates: a radius that would leave the frame is
/// clipped and the sample count shrinks accordingly, so the mean is over the
/// *present* neighbours.
pub(crate) fn box_bounds(
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    radius: Radius,
) -> (usize, usize, usize, usize) {
    (
        y.saturating_sub(radius),
        (y + radius).min(height - 1),
        x.saturating_sub(radius),
        (x + radius).min(width - 1),
    )
}

/// The number of samples in the clamped box window around `(x, y)`.
pub(crate) fn box_count(width: usize, height: usize, x: usize, y: usize, radius: Radius) -> usize {
    let (y0, y1, x0, x1) = box_bounds(width, height, x, y, radius);
    (y1 - y0 + 1) * (x1 - x0 + 1)
}

/// The box mean of one channel over the clamped window around `(x, y)`. A box
/// kernel is the portable, separable Gaussian approximation; the sum is
/// accumulated in `f32` in row-major order, which is the order the global
/// kernel has always used.
pub(crate) fn box_mean<P: PresencePlane + ?Sized>(
    plane: &P,
    x: usize,
    y: usize,
    c: usize,
    radius: Radius,
) -> f32 {
    let (width, height) = (plane.width(), plane.height());
    let (y0, y1, x0, x1) = box_bounds(width, height, x, y, radius);
    let mut sum = 0.0_f32;
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            sum += plane.channel(xx, yy, c);
        }
    }
    sum / box_count(width, height, x, y, radius) as f32
}

/// The DoG detail `value - box_mean(value, radius)`: positive on the bright side
/// of an edge, negative on the dark side.
pub(crate) fn box_detail<P: PresencePlane + ?Sized>(
    plane: &P,
    x: usize,
    y: usize,
    c: usize,
    radius: Radius,
) -> f32 {
    let value = plane.channel(x, y, c);
    value - box_mean(plane, x, y, c, radius)
}

/// The DoG write `value + amount * detail`, clamped into `0..=255`. The caller
/// decides whether that float is rounded to `u8` (global) or carried into the
/// rest of the local float chain (local).
pub(crate) fn dog_value(value: f32, detail: f32, amount: f32) -> f32 {
    (value + amount * detail).clamp(0.0, 255.0)
}

/// The DoG result of one channel: `value + amount * (value - box_mean)`. The
/// neighbourhood is always full-frame — the window size comes from the
/// persisted amount alone, never from a mask.
pub(crate) fn dog_channel<P: PresencePlane + ?Sized>(
    plane: &P,
    x: usize,
    y: usize,
    c: usize,
    radius: Radius,
    amount: f32,
) -> f32 {
    dog_value(
        plane.channel(x, y, c),
        box_detail(plane, x, y, c, radius),
        amount,
    )
}

// ------------------------------------------------------------- Dark channel

/// The radius of the dark channel's local minimum, in pixels.
const DARK_RADIUS: Radius = 2;

/// The dark channel of one pixel: `min(R, G, B)` followed by a radius-2 local
/// minimum, normalized to `0..=1`. Starting from `1.0` makes an all-zero window
/// a no-op, which is the documented edge behaviour.
pub(crate) fn dark_channel_pixel<P: PresencePlane + ?Sized>(plane: &P, x: usize, y: usize) -> f32 {
    let (width, height) = (plane.width(), plane.height());
    let (y0, y1, x0, x1) = box_bounds(width, height, x, y, DARK_RADIUS);
    let mut minimum = 1.0_f32;
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let value = plane
                .channel(xx, yy, 0)
                .min(plane.channel(xx, yy, 1))
                .min(plane.channel(xx, yy, 2))
                / 255.0;
            minimum = minimum.min(value);
        }
    }
    minimum
}

/// The whole frame's dark channel, one normalized `f32` per pixel, in
/// row-major order. The statistic is over the **entire frame**: a mask-local
/// presence never restricts it to the masked region.
pub(crate) fn dark_channel<P: PresencePlane + ?Sized>(plane: &P) -> Vec<f32> {
    let (width, height) = (plane.width(), plane.height());
    let mut dark = vec![0.0_f32; width * height];
    for y in 0..height {
        for x in 0..width {
            dark[y * width + x] = dark_channel_pixel(plane, x, y);
        }
    }
    dark
}

/// The airlight constant `A`: the deterministic 95th percentile of the whole
/// dark channel, with a `0.05` floor so a bright, hazeless frame cannot divide
/// by zero. The sort is a total-order stable sort, so an equal-valued frame
/// resolves exactly as the global kernel always resolved it.
pub(crate) fn airlight(dark: &[f32]) -> f32 {
    let mut sorted = dark.to_vec();
    sorted.sort_by(f32::total_cmp);
    let index = ((sorted.len() as f32 * 0.95) as usize).min(sorted.len().saturating_sub(1));
    sorted[index].max(0.05)
}

// ------------------------------------------------------------------- Dehaze

/// The dehaze transmission `t` of one pixel. A positive amount removes haze
/// (`t` moves toward 1, which scales the scene down toward the airlight); a
/// negative amount adds haze at half strength.
pub(crate) fn dehaze_transmission(dark: f32, airlight: f32, amount: f32) -> f32 {
    let base_t = (1.0 - 0.95 * dark / airlight).clamp(0.05, 1.0);
    if amount > 0.0 {
        1.0 - amount * (1.0 - base_t)
    } else {
        1.0 + (-amount) * 0.5 * (1.0 - base_t)
    }
}

/// The dehaze write for one **normalized** (`0..=1`) channel value. The caller
/// owns the domain conversion: the global kernel normalizes, calls this and
/// rounds to `u8`; the local chain scales the float result back into its
/// `0..=255` chain and does not round.
pub(crate) fn dehaze_value(x: f32, airlight: f32, transmission: f32) -> f32 {
    ((x - airlight) / transmission + airlight).clamp(0.0, 1.0)
}

// ------------------------------------------------- The global u8 entry point

/// F-094 deterministic raster heuristic, global recipe. DoG is
/// `x - box_blur(x, radius)`; the radius is [`texture_radius`] (1..3) and
/// [`clarity_radius`] (8..32). A box kernel is the portable, separable Gaussian
/// approximation (edge pixels replicate).
///
/// This wrapper keeps the global kernel's existing `u8` quantization points —
/// after each DoG pass and after the dehaze — so extracting the shared maths
/// above changes **no** global byte. See the module docs for exactly why the
/// local chain quantizes differently.
pub(crate) fn apply_presence(pixels: &mut [u8], width: u32, height: u32, p: &Presence) {
    apply_dog(pixels, width, height, texture_radius(p.texture), p.texture);
    apply_dog(pixels, width, height, clarity_radius(p.clarity), p.clarity);
    if p.dehaze == 0.0 {
        return;
    }
    // Dark channel is min(R,G,B) followed by a radius-2 local minimum. A is
    // the deterministic 95th percentile of that channel, with a floor.
    let (w, h) = (width as usize, height as usize);
    let dark = dark_channel(&Rgba8Plane {
        pixels,
        width: w,
        height: h,
    });
    let airlight = airlight(&dark);
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let transmission = dehaze_transmission(dark[index], airlight, p.dehaze);
        for c in &mut px[..3] {
            let x = *c as f32 / 255.0;
            *c = (dehaze_value(x, airlight, transmission) * 255.0).round() as u8;
        }
    }
}

/// One full-frame DoG pass over an RGBA8 frame, quantized back to `u8` at the
/// end of the pass.
fn apply_dog(pixels: &mut [u8], width: u32, height: u32, radius: Radius, amount: f32) {
    if amount == 0.0 {
        return;
    }
    let source = pixels.to_vec();
    let (w, h) = (width as usize, height as usize);
    let plane = Rgba8Plane {
        pixels: &source,
        width: w,
        height: h,
    };
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                pixels[(y * w + x) * 4 + c] =
                    dog_channel(&plane, x, y, c, radius, amount).round() as u8;
            }
        }
    }
}
