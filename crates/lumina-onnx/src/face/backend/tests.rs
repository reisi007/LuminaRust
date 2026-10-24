use super::*;
use crate::face::{
    face_detect_manifest, face_embed_manifest, FACE_DETECT_INFERENCE_HEIGHT,
    FACE_DETECT_INFERENCE_WIDTH, FACE_EMBED_DIMENSION,
};

fn frame(width: u32, height: u32) -> ImageFrame {
    ImageFrame::new(width, height, vec![0u8; (width * height * 4) as usize]).unwrap()
}

fn face() -> DetectedFace {
    let mut faces = StubFaceDetector::default_detections();
    faces.remove(0)
}

#[test]
fn stub_detector_is_deterministic() {
    let backend = StubFaceDetector::new(face_detect_manifest()).unwrap();
    let a = backend.detect(&frame(64, 64)).unwrap();
    let b = backend.detect(&frame(64, 64)).unwrap();
    assert_eq!(a, b, "same input must yield identical detections");
    assert_eq!(a.len(), 1);
    assert_eq!(
        (a[0].bbox.x, a[0].bbox.y, a[0].bbox.width, a[0].bbox.height),
        (0.25, 0.25, 0.5, 0.5)
    );
    assert_eq!(a[0].landmarks.len(), 5);
    // Deterministic even for a different frame size (content-independent).
    assert_eq!(a, backend.detect(&frame(1024, 768)).unwrap());
}

#[test]
fn stub_detector_injection_and_availability() {
    let injected = vec![face(), face()];
    let backend = StubFaceDetector::new(face_detect_manifest())
        .unwrap()
        .with_detections(injected.clone())
        .unwrap();
    assert_eq!(backend.detect(&frame(8, 8)).unwrap(), injected);

    let unavailable = StubFaceDetector::new(face_detect_manifest())
        .unwrap()
        .with_availability(false);
    assert!(matches!(
        unavailable.detect(&frame(8, 8)),
        Err(OnnxError::ModelUnavailable { .. })
    ));
}

#[test]
fn stub_detector_rejects_zero_dimensions_and_wrong_capability() {
    let backend = StubFaceDetector::new(face_detect_manifest()).unwrap();
    assert!(matches!(
        backend.detect(&frame(0, 0)),
        Err(OnnxError::InvalidDimensions { .. })
    ));
    let err = StubFaceDetector::new(face_embed_manifest()).unwrap_err();
    assert!(matches!(err, OnnxError::UnsupportedModel { .. }), "{err:?}");
}

#[test]
fn stub_embedder_is_deterministic_and_unit_normalized() {
    let backend = StubFaceEmbedder::new(face_embed_manifest(), FACE_EMBED_DIMENSION).unwrap();
    let faces = vec![face()];
    let a = backend.embed(&frame(64, 64), &faces).unwrap();
    let b = backend.embed(&frame(64, 64), &faces).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a, b);
    assert_eq!(a[0].dimension(), FACE_EMBED_DIMENSION as usize);
    assert!(a[0].is_normalized());
}

#[test]
fn stub_embedder_empty_detections_yields_empty() {
    let backend = StubFaceEmbedder::new(face_embed_manifest(), FACE_EMBED_DIMENSION).unwrap();
    assert!(backend.embed(&frame(0, 0), &[]).unwrap().is_empty());
}

#[test]
fn stub_embedder_injection_is_validated() {
    let backend = StubFaceEmbedder::new(face_embed_manifest(), 2).unwrap();
    let vectors = vec![vec![0.6f32, 0.8f32]];
    let backend = backend.with_vectors(vectors.clone()).unwrap();
    let embedded = backend.embed(&frame(4, 4), &[face()]).unwrap();
    assert_eq!(embedded[0].values(), &vectors[0][..]);
    assert_eq!(embedded[0].dimension(), 2);

    let mismatched = StubFaceEmbedder::new(face_embed_manifest(), 4)
        .unwrap()
        .with_vectors(vectors)
        .unwrap_err();
    assert!(
        matches!(mismatched, OnnxError::InvalidFaceData(_)),
        "{mismatched:?}"
    );
}

/// The `embed` contract is "exactly one vector per detection, same order":
/// the default path yields the full count, and an injected vector list that
/// does not match the detection count is a loud error — never a truncated
/// or padded result.
#[test]
fn stub_embedder_returns_exactly_one_vector_per_detection() {
    let backend = StubFaceEmbedder::new(face_embed_manifest(), FACE_EMBED_DIMENSION).unwrap();
    let faces = vec![face(), face(), face()];
    let embeddings = backend.embed(&frame(8, 8), &faces).unwrap();
    assert_eq!(
        embeddings.len(),
        faces.len(),
        "the default path must return exactly one vector per detection"
    );

    // Two injected vectors for three detections → loud, never silently
    // zipped/truncated.
    let mismatched = StubFaceEmbedder::new(face_embed_manifest(), 2)
        .unwrap()
        .with_vectors(vec![vec![1.0, 0.0], vec![0.0, 1.0]])
        .unwrap();
    let err = mismatched.embed(&frame(8, 8), &faces).unwrap_err();
    assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
}

#[test]
fn stub_embedder_availability_refuses() {
    let backend = StubFaceEmbedder::new(face_embed_manifest(), 4)
        .unwrap()
        .with_availability(false);
    assert!(matches!(
        backend.embed(&frame(4, 4), &[face()]),
        Err(OnnxError::ModelUnavailable { .. })
    ));
}

#[test]
fn embedding_vector_validation_is_loud() {
    assert!(FaceEmbeddingVector::new(vec![]).is_err());
    assert!(FaceEmbeddingVector::new(vec![f32::NAN]).is_err());
    assert!(FaceEmbeddingVector::new(vec![0.0, 0.0])
        .unwrap()
        .l2_normalized()
        .is_err());
    let normalized = FaceEmbeddingVector::new(vec![3.0, 4.0])
        .unwrap()
        .l2_normalized()
        .unwrap();
    assert!(normalized.is_normalized());
    assert_eq!(normalized.values(), &[0.6, 0.8]);
    assert!(normalized.with_dimension(3).is_err());
}

#[test]
fn detection_validation_is_loud() {
    let cases: [fn(&mut DetectedFace); 5] = [
        |face| face.score = 1.5,
        |face| face.bbox.width = 0.0,
        |face| face.bbox.x = 0.9,
        |face| face.landmarks[0].x = 2.0,
        |face| face.landmarks[1].name = "left_eye".into(),
    ];
    for mutate in cases {
        let mut face = face();
        mutate(&mut face);
        assert!(
            face.validate().is_err(),
            "invalid detection must be rejected"
        );
    }
    // The documented inference resolutions are part of the contract.
    assert_eq!(
        face_detect_manifest().input.resolution.width,
        FACE_DETECT_INFERENCE_WIDTH
    );
    assert_eq!(
        face_detect_manifest().input.resolution.height,
        FACE_DETECT_INFERENCE_HEIGHT
    );
}

#[test]
fn decode_detections_decodes_filters_and_rejects_bad_shapes() {
    // Two rows, box+score only (K = 0).
    let shape = [2usize, 5];
    let data = [0.5f32, 0.5, 0.2, 0.2, 0.9, 0.3, 0.3, 0.1, 0.1, 0.1];
    let detections = decode_face_detections("M", &shape, &data, 0.5).unwrap();
    assert_eq!(detections.len(), 1, "score below threshold is filtered");
    assert_eq!(
        (
            detections[0].bbox.x,
            detections[0].bbox.y,
            detections[0].bbox.width,
            detections[0].bbox.height
        ),
        (0.4, 0.4, 0.2, 0.2)
    );
    assert!(detections[0].landmarks.is_empty());

    // Rank 3 `[1, N, C]` with 5 landmarks uses the canonical names.
    let mut row = vec![0.5f32, 0.5, 0.2, 0.2, 0.9];
    for _ in crate::face::FACE_LANDMARK_NAMES_5PT {
        row.push(0.5);
        row.push(0.5);
    }
    let detections = decode_face_detections("M", &[1, 1, 15], &row, 0.0).unwrap();
    assert_eq!(detections.len(), 1);
    assert_eq!(detections[0].landmarks.len(), 5);
    assert_eq!(detections[0].landmarks[0].name, "left_eye");

    // Loud failures: spatial rank-4 output, odd columns, bad values.
    assert!(decode_face_detections("M", &[1, 1, 8, 8], &[0.0; 64], 0.5).is_err());
    assert!(decode_face_detections("M", &[1, 6], &[0.0; 6], 0.5).is_err());
    assert!(decode_face_detections("M", &[1, 5], &[0.0; 4], 0.5).is_err());
    // Out-of-frame box → loud, never clamped.
    assert!(decode_face_detections("M", &[1, 5], &[0.1, 0.1, 0.5, 0.5, 0.9], 0.0).is_err());
    // NaN score → loud, never silently dropped.
    assert!(decode_face_detections("M", &[1, 5], &[0.5, 0.5, 0.1, 0.1, f32::NAN], 0.0).is_err());
}

#[test]
fn decode_embedding_normalizes_and_rejects_spatial_maps() {
    let embedding = decode_face_embedding("M", &[2], &[3.0, 4.0], 2).unwrap();
    assert_eq!(embedding.values(), &[0.6, 0.8]);
    assert!(embedding.is_normalized());
    // `[1, D]` is accepted, a spatial map is not.
    assert!(decode_face_embedding("M", &[1, 2], &[3.0, 4.0], 2).is_ok());
    assert!(decode_face_embedding("M", &[1, 1, 8, 8], &[0.0; 64], 64).is_err());
    assert!(decode_face_embedding("M", &[3], &[1.0, 0.0, 0.0], 2).is_err());
    assert!(decode_face_embedding("M", &[0], &[], 0).is_err());
}

fn template_landmarks() -> Vec<FaceLandmark> {
    FACE_ALIGN_TEMPLATE_5PT
        .iter()
        .zip(crate::face::FACE_LANDMARK_NAMES_5PT)
        .map(|((x, y), name)| FaceLandmark {
            name: name.into(),
            x: *x / FACE_ALIGN_TEMPLATE_BASE,
            y: *y / FACE_ALIGN_TEMPLATE_BASE,
        })
        .collect()
}

#[test]
fn alignment_at_template_scale_is_identity() {
    // A 112×112 frame whose landmarks are exactly the template points
    // must align to itself.
    let mut pixels = vec![0u8; 112 * 112 * 4];
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (index % 256) as u8;
        px[1] = ((index / 256) % 256) as u8;
        px[2] = 7;
        px[3] = 255;
    }
    let frame = ImageFrame::new(112, 112, pixels).unwrap();
    let landmarks = template_landmarks();
    let aligned = align_face_to_template(&frame, &landmarks, (112, 112)).unwrap();
    assert_eq!((aligned.width(), aligned.height()), (112, 112));
    assert_eq!(
        aligned.pixels(),
        frame.pixels(),
        "template landmarks on a 112×112 frame must be an identity warp"
    );

    // Deterministic across calls.
    assert_eq!(
        align_face_to_template(&frame, &landmarks, (112, 112))
            .unwrap()
            .pixels(),
        aligned.pixels()
    );
}

#[test]
fn alignment_scales_template_and_is_loud_on_bad_input() {
    let frame = ImageFrame::new(224, 224, vec![128u8; 224 * 224 * 4]).unwrap();
    // Landmarks in normalized coordinates for a 224 frame → a 2× scale.
    let landmarks: Vec<FaceLandmark> = FACE_ALIGN_TEMPLATE_5PT
        .iter()
        .zip(crate::face::FACE_LANDMARK_NAMES_5PT)
        .map(|((x, y), name)| FaceLandmark {
            name: name.into(),
            x: *x / FACE_ALIGN_TEMPLATE_BASE,
            y: *y / FACE_ALIGN_TEMPLATE_BASE,
        })
        .collect();
    let aligned = align_face_to_template(&frame, &landmarks, (112, 112)).unwrap();
    assert_eq!((aligned.width(), aligned.height()), (112, 112));

    // Missing landmark → loud.
    let mut missing = landmarks.clone();
    missing.pop();
    assert!(align_face_to_template(&frame, &missing, (112, 112)).is_err());

    // Degenerate (coincident) landmarks → loud.
    let degenerate: Vec<FaceLandmark> = crate::face::FACE_LANDMARK_NAMES_5PT
        .iter()
        .map(|name| FaceLandmark {
            name: (*name).into(),
            x: 0.5,
            y: 0.5,
        })
        .collect();
    assert!(align_face_to_template(&frame, &degenerate, (112, 112)).is_err());

    // Zero-sized frames/resolutions → loud.
    let empty = ImageFrame::new(0, 0, vec![]).unwrap();
    assert!(align_face_to_template(&empty, &landmarks, (112, 112)).is_err());
    assert!(align_face_to_template(&frame, &landmarks, (0, 112)).is_err());
}
