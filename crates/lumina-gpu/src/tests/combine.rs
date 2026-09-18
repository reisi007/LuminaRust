use super::*;

/// GPU-STAGE-1: combining evaluated layer planes follows the F-041
/// intersection-product semantics.
#[test]
fn combine_mask_planes_product_semantics() {
    use lumina_core::masks::MaskPlane;
    let plane = |values: &[u16]| MaskPlane {
        width: 2,
        height: 2,
        values: values.to_vec(),
    };
    // Empty input = no effective mask (valid state).
    assert!(combine_mask_planes(&[]).unwrap().is_none());

    // Single plane passes through unchanged.
    let single = plane(&[0, 32768, 65535, 123]);
    assert_eq!(
        combine_mask_planes(std::slice::from_ref(&single))
            .unwrap()
            .unwrap()
            .values,
        single.values
    );

    // Product: 50% ∩ full = 50%; anything ∩ 0 = 0; all-MAX = identity.
    let half = plane(&[u16::MAX, 32768, u16::MAX, 40000]);
    let full = plane(&[u16::MAX, u16::MAX, u16::MAX, u16::MAX]);
    let zero = plane(&[0, 0, 0, 0]);
    let combined = combine_mask_planes(&[half.clone(), full]).unwrap().unwrap();
    assert_eq!(combined.values, vec![u16::MAX, 32768, u16::MAX, 40000]);
    let killed = combine_mask_planes(&[half, zero]).unwrap().unwrap();
    assert_eq!(killed.values, vec![0, 0, 0, 0]);

    // Dimension mismatch is an explicit error, never a silent resample.
    let other = MaskPlane {
        width: 1,
        height: 4,
        values: vec![0; 4],
    };
    let err = combine_mask_planes(&[single, other]).unwrap_err();
    assert!(err.contains("does not match"), "got: {err}");
}
