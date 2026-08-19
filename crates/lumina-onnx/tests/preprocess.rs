use lumina_core::{ImageFrame, MaskPlane};
use lumina_onnx::{preprocess_rgb_to_model, rescale_model_matte, Resolution};

#[test]
fn preprocess_is_deterministic_and_independent_of_content() {
    let a = ImageFrame::new(3, 3, vec![10; 3 * 3 * 4]).unwrap();
    let b = ImageFrame::new(3, 3, vec![200; 3 * 3 * 4]).unwrap();
    let ra = preprocess_rgb_to_model(
        &a,
        Resolution {
            width: 6,
            height: 6,
        },
    );
    let rb = preprocess_rgb_to_model(
        &b,
        Resolution {
            width: 6,
            height: 6,
        },
    );
    // Same geometry -> same length; different content -> different bytes.
    assert_eq!(ra.len(), rb.len());
    assert_ne!(ra, rb);
    let ra2 = preprocess_rgb_to_model(
        &a,
        Resolution {
            width: 6,
            height: 6,
        },
    );
    assert_eq!(ra, ra2, "preprocessing must be deterministic");
}

#[test]
fn rescale_matte_back_to_source_is_nearest() {
    let model = MaskPlane::new(2, 2, vec![65535, 100, 200, 0]).unwrap();
    let out = rescale_model_matte(
        &model,
        Resolution {
            width: 2,
            height: 2,
        },
        (4, 4),
    )
    .unwrap();
    assert_eq!((out.width, out.height), (4, 4));
    assert_eq!(out.values.len(), 16);
    // nearest mapping: source (0,0) -> model (0,0); source (3,3) -> model (1,1)
    assert_eq!(out.values[0], 65535);
    assert_eq!(out.values[15], 0);
}
