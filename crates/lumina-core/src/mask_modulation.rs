//! Mask-layer modulation (F-049).
//!
//! After a mask plane is resolved and resampled to the render frame, each
//! [`MaskLayer`] applies a deterministic modulation pipeline *before* the plane
//! weights the rendered adjustments:
//!
//! 1. **Invert** (`inverted`): `value = u16::MAX - value`.
//! 2. **Feather** (`feather > 0`): a box blur softening hard edges.
//! 3. **Blur** (`blur > 0`): a second box blur applied *after* feathering.
//! 4. **Density** (`density < 1`): `value = value * density` (linear scaling).
//!
//! Each step is a no-op at its identity value (`inverted = false`,
//! `feather = 0.0`, `blur = 0.0`, `density = 1.0`), so an unmodulated layer is
//! byte-identical to the input plane.
//!
//! REVIEW-MASK-N2: the modulation parameters are validated before any pixel
//! is touched. A density outside `0..=1` (or non-finite) is a hard
//! [`MaskError::InvalidDensity`] — never a silent erasure (`density < 0`
//! would zero the whole matte) or a silently ignored control (`density > 1`
//! was clamped away by the `u16` cast).

use crate::masks::{MaskError, MaskPlane};
use lumina_sidecar::MaskLayer;

/// Apply every enabled modulation to `plane` in the documented order.
///
/// `layer` carries the per-layer modulation parameters. The plane's `width`
/// and `height` drive the feather/blur radii as fractions of the larger
/// dimension, matching the documented `radius = k * max(w, h)` convention.
///
/// # Errors
///
/// Returns [`MaskError::InvalidDensity`] when `layer.density` is not a finite
/// value in `0..=1`. Validation runs before any mutation, so a rejected layer
/// leaves `plane` byte-identical to its input (no partial modulation).
pub fn modulate_mask_plane(plane: &mut MaskPlane, layer: &MaskLayer) -> Result<(), MaskError> {
    // REVIEW-MASK-N2: validate first — no partial mutation, no silent fallback.
    if !layer.density.is_finite() || !(0.0..=1.0).contains(&layer.density) {
        return Err(MaskError::InvalidDensity {
            value: format!("{:e}", layer.density),
        });
    }
    if layer.inverted {
        for value in &mut plane.values {
            *value = u16::MAX - *value;
        }
    }
    if layer.feather > 0.0 {
        let radius = (layer.feather * plane.width.max(plane.height) as f32 / 2.0) as u32;
        // Feathering is applied iteratively (a few box-blur passes) to soften
        // hard edges without a separable Gaussian.
        box_blur_passes(plane, radius, 3);
    }
    if layer.blur > 0.0 {
        let radius = (layer.blur * plane.width.max(plane.height) as f32 / 4.0) as u32;
        box_blur_passes(plane, radius, 1);
    }
    if layer.density < 1.0 {
        let density = layer.density as f64;
        for value in &mut plane.values {
            *value = (*value as f64 * density) as u16;
        }
    }
    Ok(())
}

/// Apply a separable box blur `passes` times with the given integer radius.
/// Out-of-bounds samples are simply skipped so edge pixels average only their
/// valid neighbours (clamp-by-exclusion, preserving the value at the very edge
/// of a 1-pixel line). A zero radius is a no-op.
fn box_blur_passes(plane: &mut MaskPlane, radius: u32, passes: usize) {
    for _ in 0..passes {
        box_blur(plane, radius);
    }
}

/// One separable box-blur pass: a horizontal sweep followed by a vertical sweep,
/// each averaging the `radius` neighbours on every side (inclusive). Averages
/// are computed in `u64` to stay exact for `u16` inputs.
///
/// REVIEW-MASK-BLUR-1: both sweeps use a sliding window. The window sum is
/// carried from one output column/row to the next by adding the entering
/// sample and subtracting the leaving one, which makes the pass `O(w·h)`
/// independent of the radius instead of `O(w·h·radius)`. Because the sums are
/// exact integers and the window bounds `[x.saturating_sub(radius),
/// min(x+radius, n-1)]` are identical to the previous per-pixel summation,
/// every output value — including the integer division result — is
/// **byte-identical** to the previous implementation
/// (`box_blur_is_byte_identical_to_previous_implementation` proves this
/// against an in-test mirror of the old algorithm).
fn box_blur(plane: &mut MaskPlane, radius: u32) {
    if radius == 0 {
        return;
    }
    let radius = radius as usize;
    let (w, h) = (plane.width as usize, plane.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    // Horizontal pass: reads `values`, writes `scratch`.
    let mut scratch = vec![0u64; w * h];
    for y in 0..h {
        let row = &plane.values[y * w..(y + 1) * w];
        let out_row = &mut scratch[y * w..(y + 1) * w];
        let initial_end = radius.min(w - 1);
        let mut sum: u64 = row[..=initial_end].iter().map(|&v| u64::from(v)).sum();
        let mut count = initial_end as u64 + 1;
        out_row[0] = sum / count;
        let mut window_end = initial_end;
        for x in 1..w {
            let new_end = (x + radius).min(w - 1);
            if new_end > window_end {
                // The right edge of the window entered a new sample.
                sum += u64::from(row[new_end]);
                count += 1;
                window_end = new_end;
            }
            if x > radius {
                // The left edge left the sample at index `x - radius - 1`.
                sum -= u64::from(row[x - radius - 1]);
                count -= 1;
            }
            out_row[x] = sum / count;
        }
    }
    // Vertical pass: reads `scratch`, writes `values`.
    for x in 0..w {
        let initial_end = radius.min(h - 1);
        let mut sum: u64 = (0..=initial_end).map(|k| scratch[k * w + x]).sum();
        let mut count = initial_end as u64 + 1;
        plane.values[x] = (sum / count) as u16;
        let mut window_end = initial_end;
        for y in 1..h {
            let new_end = (y + radius).min(h - 1);
            if new_end > window_end {
                sum += scratch[new_end * w + x];
                count += 1;
                window_end = new_end;
            }
            if y > radius {
                sum -= scratch[(y - radius - 1) * w + x];
                count -= 1;
            }
            plane.values[y * w + x] = (sum / count) as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::MaskLayer;

    fn layer(inverted: bool, feather: f32, blur: f32, density: f32) -> MaskLayer {
        MaskLayer {
            id: "l".into(),
            mask: lumina_sidecar::MaskReference {
                copy_id: "vc".into(),
                mask_id: "m".into(),
                extras: Default::default(),
            },
            inverted,
            feather,
            blur,
            density,
            extras: Default::default(),
        }
    }

    fn make_plane(values: Vec<u16>) -> MaskPlane {
        let n = values.len() as u32;
        MaskPlane::new(n, 1, values).unwrap()
    }

    fn rect_plane(width: u32, height: u32, seed: u64) -> MaskPlane {
        // Deterministic xorshift fill so sweeps cover varied values without
        // RNG dependencies.
        let mut state = seed | 1;
        let mut values = Vec::with_capacity((width * height) as usize);
        for _ in 0..width * height {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            values.push((state % (u16::MAX as u64 + 1)) as u16);
        }
        MaskPlane::new(width, height, values).unwrap()
    }

    /// Byte-exact mirror of the pre-REVIEW-MASK-BLUR-1 implementation: per
    /// output pixel it re-sums the whole `[start..=end]` window. Kept ONLY in
    /// the test module to prove the sliding-window rewrite is byte-identical.
    fn reference_box_blur_previous_implementation(plane: &MaskPlane, radius: u32) -> MaskPlane {
        let radius = radius as usize;
        let (w, h) = (plane.width as usize, plane.height as usize);
        let mut scratch = vec![0u64; w * h];
        for y in 0..h {
            for x in 0..w {
                let start = x.saturating_sub(radius);
                let end = (x + radius).min(w - 1);
                let mut sum = 0u64;
                let mut count = 0u64;
                for k in start..=end {
                    sum += plane.values[y * w + k] as u64;
                    count += 1;
                }
                scratch[y * w + x] = sum / count;
            }
        }
        let mut values = plane.values.clone();
        for y in 0..h {
            for x in 0..w {
                let start = y.saturating_sub(radius);
                let end = (y + radius).min(h - 1);
                let mut sum = 0u64;
                let mut count = 0u64;
                for k in start..=end {
                    sum += scratch[k * w + x];
                    count += 1;
                }
                values[y * w + x] = (sum / count) as u16;
            }
        }
        MaskPlane {
            width: plane.width,
            height: plane.height,
            values,
        }
    }

    fn blurred(mut plane: MaskPlane, radius: u32, passes: usize) -> MaskPlane {
        box_blur_passes(&mut plane, radius, passes);
        plane
    }

    #[test]
    fn invert_flips_every_pixel() {
        let mut plane = make_plane(vec![0, 1000, 32768, 65535]);
        modulate_mask_plane(&mut plane, &layer(true, 0.0, 0.0, 1.0)).unwrap();
        assert_eq!(plane.values, vec![65535, 64535, 32767, 0]);
    }

    #[test]
    fn density_scales_linearly() {
        let mut plane = make_plane(vec![10000, 20000, 30000]);
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 0.5)).unwrap();
        assert_eq!(plane.values, vec![5000, 10000, 15000]);
    }

    #[test]
    fn identity_layer_is_a_noop() {
        let input = vec![123, 4567, 32109, 65535];
        let mut plane = crate::masks::MaskPlane::new(4, 1, input.clone()).unwrap();
        let copied = input.clone();
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 1.0)).unwrap();
        assert_eq!(plane.values, copied);
    }

    #[test]
    fn feather_zero_is_a_noop() {
        let input = vec![0, 0, 65535, 0, 0];
        let mut plane = make_plane(input.clone());
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 1.0)).unwrap();
        assert_eq!(plane.values, input);
    }

    #[test]
    fn feather_softens_a_hard_edge() {
        // A single hard peak on a 1x5 line; feathering must diffuse it.
        let input = vec![0u16, 0, 65535, 0, 0];
        let mut plane = make_plane(input);
        modulate_mask_plane(&mut plane, &layer(false, 0.5, 0.0, 1.0)).unwrap();
        // The peak is reduced (energy spread); every value is a valid u16.
        assert_eq!(*plane.values.iter().max().unwrap(), 16990);
        // The previously-empty neighbours of the peak gain energy.
        assert!(plane.values[0] > 0);
        assert!(plane.values[1] > 0);
        assert!(plane.values[3] > 0);
        assert!(plane.values[4] > 0);
        assert_ne!(plane.values, vec![0u16, 0, 65535, 0, 0]);
    }

    #[test]
    fn blur_is_applied_after_feather_and_softens() {
        let input = vec![0u16, 0, 65535, 0, 0];
        let mut plane = make_plane(input.clone());
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 1.0, 1.0)).unwrap();
        assert!(plane.values.iter().all(|v| *v < 65535));
        assert_ne!(plane.values, input);
    }

    #[test]
    fn order_is_invert_then_density() {
        // Invert then density: (u16::MAX - v) * density, not the reverse.
        let mut plane = make_plane(vec![10000]);
        modulate_mask_plane(&mut plane, &layer(true, 0.0, 0.0, 0.5)).unwrap();
        assert_eq!(plane.values, vec![((u16::MAX - 10000) as f64 * 0.5) as u16]);
    }

    #[test]
    fn all_modulations_compose_deterministically() {
        let input = vec![0u16, 16384, 32768, 49152, 65535];
        let mut plane = make_plane(input.clone());
        modulate_mask_plane(&mut plane, &layer(true, 0.5, 0.5, 0.5)).unwrap();
        // Output must be stable across repeated runs.
        let mut again = make_plane(input);
        modulate_mask_plane(&mut again, &layer(true, 0.5, 0.5, 0.5)).unwrap();
        assert_eq!(plane.values, again.values);
    }

    // ---- REVIEW-MASK-N2: density validation ----

    #[test]
    fn density_outside_zero_one_is_a_hard_error_without_mutation() {
        for density in [-0.25f32, 1.5, f32::NEG_INFINITY, f32::INFINITY, f32::NAN] {
            let original = make_plane(vec![10_000, 20_000, 30_000]);
            let snapshot = original.clone();
            let mut plane = original;
            let error = modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, density))
                .err()
                .unwrap_or_else(|| panic!("density {density} must be rejected"));
            match error {
                MaskError::InvalidDensity { value } => {
                    assert!(!value.is_empty(), "density {density} must be reported");
                }
                other => panic!("unexpected error for density {density}: {other:?}"),
            }
            // No partial modulation: the plane stays byte-identical.
            assert_eq!(plane, snapshot, "plane mutated despite rejection");
        }
    }

    #[test]
    fn density_boundaries_are_valid() {
        // 0.0 is an explicit full-density reduction (erases intentionally)
        // and 1.0 is the identity — both inside the validated range.
        let mut plane = make_plane(vec![12_345, 65_535]);
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 0.0)).unwrap();
        assert_eq!(plane.values, vec![0, 0]);

        let mut plane = make_plane(vec![12_345, 65_535]);
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 1.0)).unwrap();
        assert_eq!(plane.values, vec![12_345, 65_535]);
    }

    // ---- REVIEW-MASK-BLUR-1: sliding-window box blur, byte-identical ----

    #[test]
    fn box_blur_is_byte_identical_to_previous_implementation() {
        // Sweep deterministic planes over shapes (lines, rows, rectangles),
        // radii (0, sub-radius, boundary, super-radius) and pass counts; the
        // rewritten O(w·h) sliding window must match the mirrored previous
        // O(w·h·radius) implementation byte for byte.
        const SHAPES: [(u32, u32); 8] = [
            (1, 1),
            (1, 7),
            (7, 1),
            (2, 13),
            (13, 2),
            (5, 5),
            (16, 9),
            (9, 16),
        ];
        for (w, h) in SHAPES {
            for seed in [1u64, 0xDEAD_BEEF, 0x5EED_5EED] {
                let plane = rect_plane(w, h, seed);
                let long = w.max(h);
                for &radius in &[0u32, 1, 2, long / 2, long - 1, long, long * 2] {
                    for &passes in &[1usize, 3] {
                        let mut expected = plane.clone();
                        for _ in 0..passes {
                            expected =
                                reference_box_blur_previous_implementation(&expected, radius);
                        }
                        assert_eq!(
                            blurred(plane.clone(), radius, passes),
                            expected,
                            "byte-identity failed at {w}x{h} radius={radius} passes={passes}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn box_blur_radius_above_dimension_preserves_uniform_planes() {
        // A uniform plane under any radius (including radius ≥ dimension)
        // averages to itself: the window always covers all samples.
        let plane = rect_plane_filled(6, 4, 42_000);
        let blurred = blurred(plane.clone(), 9, 3);
        assert_eq!(blurred.values, plane.values);
    }

    fn rect_plane_filled(width: u32, height: u32, value: u16) -> MaskPlane {
        MaskPlane::new(width, height, vec![value; (width * height) as usize]).unwrap()
    }

    #[test]
    fn box_blur_spreads_energy_from_a_single_peak_two_dimensional() {
        // 3x3 plane with a single bright centre pixel and radius 1. The exact
        // hand-computed result pins both sweeps: the horizontal pass averages
        // each row window (centre row -> [4500, 3000, 4500]), then the
        // vertical pass averages those columns down to the grid below.
        let mut plane = rect_plane_filled(3, 3, 0);
        plane.values[4] = 9_000;
        let blurred = blurred(plane, 1, 1);
        assert_eq!(
            blurred.values,
            vec![
                2_250, 1_500, 2_250, //
                1_500, 1_000, 1_500, //
                2_250, 1_500, 2_250,
            ]
        );
    }
}
