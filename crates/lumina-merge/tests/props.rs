//! MERGE-CORE-1 property tests: ranges, monotonicity, clipping.

use lumina_core::merge_geom::{feather_weight, hdr_hat_weight};
use lumina_merge::{blend_panorama, LinearImage};
use proptest::prelude::*;

proptest! {
    #[test]
    fn hat_weight_always_in_unit_range(v in -1.0f32..2.0f32) {
        let w = hdr_hat_weight(v);
        prop_assert!((0.0..=1.0).contains(&w), "out of range for {v}: {w}");
    }

    #[test]
    fn feather_weight_in_range_and_monotone(
        overlap in 1u32..200u32,
        blend in 0u32..200u32,
    ) {
        let mut prev = -1.0f32;
        for pos in 0..overlap {
            let w = feather_weight(pos, overlap, blend);
            prop_assert!((0.0..=1.0).contains(&w), "out of range: {w}");
            prop_assert!(w >= prev, "not monotone at pos {pos}: {w} < {prev}");
            prev = w;
        }
    }

    #[test]
    fn pano_blend_output_finite_nonnegative(
        va in 0.0f32..1.0f32,
        vb in 0.0f32..1.0f32,
        blend in 0u32..8u32,
    ) {
        let a = LinearImage::solid(8, 4, [va, va, va]);
        let b = LinearImage::solid(8, 4, [vb, vb, vb]);
        let out = blend_panorama(&[a, b], &[(0, 0), (4, 0)], blend).unwrap();
        for v in out.pixels() {
            prop_assert!(v.is_finite() && *v >= 0.0, "bad output {v}");
            prop_assert!(*v <= va.max(vb) + 1e-5, "convex blend exceeded inputs: {v}");
        }
    }
}
