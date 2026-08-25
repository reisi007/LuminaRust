use lumina_core::ImageFrame;
use lumina_onnx::{birefnet_manifest, StubBackend, SubjectInference};

fn solid_frame(width: u32, height: u32, rgb: [u8; 3]) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for p in pixels.as_chunks_mut::<4>().0 {
        p[0] = rgb[0];
        p[1] = rgb[1];
        p[2] = rgb[2];
        p[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

#[test]
fn stub_is_deterministic_and_byte_identical() {
    let backend = StubBackend::new(birefnet_manifest()).unwrap();
    let img = solid_frame(640, 480, [120, 80, 200]);
    let a = backend.infer(&img).unwrap();
    let b = backend.infer(&img).unwrap();
    assert_eq!((a.width, a.height), (640, 480));
    assert_eq!(
        a.values, b.values,
        "same input must yield byte-identical plane"
    );
    let center = (a.height as usize / 2) * a.width as usize + a.width as usize / 2;
    assert!(
        a.values[center] > a.values[0],
        "center must be brighter than corner"
    );
}

#[test]
fn stub_matte_spans_full_range() {
    let backend = StubBackend::new(birefnet_manifest()).unwrap();
    let img = solid_frame(1024, 1024, [10, 20, 30]);
    let m = backend.infer(&img).unwrap();
    let min = *m.values.iter().min().unwrap();
    let max = *m.values.iter().max().unwrap();
    // Corners fully transparent; center near-opaque (even grid -> peak ~65434).
    assert_eq!(min, 0, "corners must be transparent");
    assert!(max >= 65000, "center must be near-opaque, got {max}");
}

/// REVIEW-ONNX-AVAIL-1 — a stub reporting itself unavailable must refuse
/// inference with `MissingModel` instead of silently emitting a matte.
#[test]
fn unavailable_stub_refuses_inference() {
    use lumina_onnx::SubjectInference as _;
    let backend = StubBackend::new(birefnet_manifest())
        .unwrap()
        .with_availability(false);
    let img = solid_frame(64, 64, [5, 5, 5]);
    let err = backend.infer(&img).unwrap_err();
    match &err {
        lumina_onnx::OnnxError::MissingModel { path } => {
            assert_eq!(path, "BiRefNet");
        }
        other => panic!("expected MissingModel, got {other:?}"),
    }
}
