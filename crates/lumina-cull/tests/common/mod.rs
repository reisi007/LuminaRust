//! Shared synthetic-frame helpers for the culling integration tests.
//!
//! Everything here is generated in-process and deterministic (no network, no
//! original files, no image codecs). Frames are grayscale (R=G=B) RGBA8.

use lumina_core::ImageFrame;

/// splitmix64: a small deterministic PRNG so fixtures are reproducible without
/// a `rand` dependency.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Builds an RGBA8 frame from a grayscale function.
pub fn gray_frame(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> u8) -> ImageFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = f(x, y);
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    ImageFrame::new(width, height, pixels).expect("exact buffer")
}

/// High-contrast checkerboard (structure without pixel noise).
pub fn checkerboard(width: u32, height: u32, block: u32, dark: u8, light: u8) -> ImageFrame {
    gray_frame(width, height, |x, y| {
        if (x / block + y / block).is_multiple_of(2) {
            dark
        } else {
            light
        }
    })
}

/// Coarse deterministic "texture": random *blocks* rather than pixels, so it
/// has strong structure but negligible per-pixel noise.
pub fn block_texture(width: u32, height: u32, block: u32, seed: u64) -> ImageFrame {
    let mut state = seed;
    let cells_x = width.div_ceil(block);
    let cells_y = height.div_ceil(block);
    let values: Vec<u8> = (0..cells_x * cells_y)
        .map(|_| (splitmix64(&mut state) % 160 + 40) as u8)
        .collect();
    gray_frame(width, height, |x, y| {
        values[(y / block * cells_x + x / block) as usize]
    })
}

/// Flat field with strong per-pixel noise (sensor-noise surrogate).
pub fn noise_frame(width: u32, height: u32, base: u8, amplitude: u8, seed: u64) -> ImageFrame {
    let mut state = seed;
    gray_frame(width, height, |_, _| {
        let noise =
            (splitmix64(&mut state) % (u64::from(amplitude) * 2 + 1)) as i32 - i32::from(amplitude);
        (i32::from(base) + noise).clamp(0, 255) as u8
    })
}

/// Uniform solid frame.
pub fn solid_frame(width: u32, height: u32, value: u8) -> ImageFrame {
    gray_frame(width, height, |_, _| value)
}

/// Separable box blur with independent horizontal/vertical radii (replicated
/// borders). `(radius, radius)` = isotropic defocus surrogate; `(radius, 0)` =
/// horizontal smear (motion-blur surrogate).
pub fn box_blur(frame: &ImageFrame, radius_x: u32, radius_y: u32) -> ImageFrame {
    let (width, height) = (frame.width, frame.height);
    let gray = |x: u32, y: u32| f64::from(frame.pixels[((y * width + x) * 4) as usize]);
    let mut horizontal = vec![0f64; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            horizontal[(y * width + x) as usize] =
                window_mean(x, radius_x, width, |xx| gray(xx, y));
        }
    }
    let mut output = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = window_mean(y, radius_y, height, |yy| {
                horizontal[(yy * width + x) as usize]
            });
            let value = value.round().clamp(0.0, 255.0) as u8;
            let base = ((y * width + x) * 4) as usize;
            output[base] = value;
            output[base + 1] = value;
            output[base + 2] = value;
            output[base + 3] = 255;
        }
    }
    ImageFrame::new(width, height, output).expect("exact buffer")
}

fn window_mean(center: u32, radius: u32, length: u32, mut sample: impl FnMut(u32) -> f64) -> f64 {
    if radius == 0 {
        return sample(center);
    }
    let start = center.saturating_sub(radius);
    let end = (center + radius).min(length - 1);
    let mut sum = 0.0;
    let mut count = 0u32;
    for index in start..=end {
        sum += sample(index);
        count += 1;
    }
    sum / f64::from(count)
}

/// Self-test: keeps every shared helper referenced in each integration test
/// binary (no `allow(dead_code)` needed) and pins the generators'
/// determinism.
#[test]
fn fixture_helpers_are_deterministic() {
    assert_eq!(splitmix64(&mut 1), splitmix64(&mut 1));
    assert_eq!(
        gray_frame(2, 2, |x, y| (x + y) as u8),
        gray_frame(2, 2, |x, y| (x + y) as u8)
    );
    assert_eq!(checkerboard(8, 8, 2, 0, 255), checkerboard(8, 8, 2, 0, 255));
    assert_eq!(block_texture(8, 8, 2, 5), block_texture(8, 8, 2, 5));
    assert_eq!(noise_frame(8, 8, 100, 20, 3), noise_frame(8, 8, 100, 20, 3));
    assert_eq!(solid_frame(4, 4, 42), solid_frame(4, 4, 42));
    assert_eq!(
        box_blur(&checkerboard(16, 16, 4, 0, 255), 2, 1),
        box_blur(&checkerboard(16, 16, 4, 0, 255), 2, 1)
    );
}
