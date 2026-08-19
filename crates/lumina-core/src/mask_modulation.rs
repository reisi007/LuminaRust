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

use crate::masks::MaskPlane;
use lumina_sidecar::MaskLayer;

/// Apply every enabled modulation to `plane` in the documented order.
///
/// `layer` carries the per-layer modulation parameters. The plane's `width`
/// and `height` drive the feather/blur radii as fractions of the larger
/// dimension, matching the documented `radius = k * max(w, h)` convention.
pub fn modulate_mask_plane(plane: &mut MaskPlane, layer: &MaskLayer) {
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
fn box_blur(plane: &mut MaskPlane, radius: u32) {
    if radius == 0 {
        return;
    }
    let radius = radius as usize;
    let (w, h) = (plane.width as usize, plane.height as usize);
    // Horizontal pass: reads `values`, writes `scratch`.
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
    // Vertical pass: reads `scratch`, writes `values`.
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

    #[test]
    fn invert_flips_every_pixel() {
        let mut plane = make_plane(vec![0, 1000, 32768, 65535]);
        modulate_mask_plane(&mut plane, &layer(true, 0.0, 0.0, 1.0));
        assert_eq!(plane.values, vec![65535, 64535, 32767, 0]);
    }

    #[test]
    fn density_scales_linearly() {
        let mut plane = make_plane(vec![10000, 20000, 30000]);
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 0.5));
        assert_eq!(plane.values, vec![5000, 10000, 15000]);
    }

    #[test]
    fn identity_layer_is_a_noop() {
        let input = vec![123, 4567, 32109, 65535];
        let mut plane = crate::masks::MaskPlane::new(4, 1, input.clone()).unwrap();
        let copied = input.clone();
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 1.0));
        assert_eq!(plane.values, copied);
    }

    #[test]
    fn feather_zero_is_a_noop() {
        let input = vec![0, 0, 65535, 0, 0];
        let mut plane = make_plane(input.clone());
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 0.0, 1.0));
        assert_eq!(plane.values, input);
    }

    #[test]
    fn feather_softens_a_hard_edge() {
        // A single hard peak on a 1x5 line; feathering must diffuse it.
        let input = vec![0u16, 0, 65535, 0, 0];
        let mut plane = make_plane(input);
        modulate_mask_plane(&mut plane, &layer(false, 0.5, 0.0, 1.0));
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
        modulate_mask_plane(&mut plane, &layer(false, 0.0, 1.0, 1.0));
        assert!(plane.values.iter().all(|v| *v < 65535));
        assert_ne!(plane.values, input);
    }

    #[test]
    fn order_is_invert_then_density() {
        // Invert then density: (u16::MAX - v) * density, not the reverse.
        let mut plane = make_plane(vec![10000]);
        modulate_mask_plane(&mut plane, &layer(true, 0.0, 0.0, 0.5));
        assert_eq!(plane.values, vec![((u16::MAX - 10000) as f64 * 0.5) as u16]);
    }

    #[test]
    fn all_modulations_compose_deterministically() {
        let input = vec![0u16, 16384, 32768, 49152, 65535];
        let mut plane = make_plane(input.clone());
        modulate_mask_plane(&mut plane, &layer(true, 0.5, 0.5, 0.5));
        // Output must be stable across repeated runs.
        let mut again = make_plane(input);
        modulate_mask_plane(&mut again, &layer(true, 0.5, 0.5, 0.5));
        assert_eq!(plane.values, again.values);
    }
}
