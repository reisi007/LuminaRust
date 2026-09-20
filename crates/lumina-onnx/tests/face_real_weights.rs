//! LRPAR-G12-FACE-ADAPTER-25 — optional real-weight smoke test (ignored).
//!
//! The real OpenCV Zoo YuNet/SFace artifacts are neither committed nor
//! downloadable (Agents.md). When the operator supplies them via
//! `LUMINA_FACE_DETECT_MODEL_PATH` / `LUMINA_FACE_EMBED_MODEL_PATH`, this
//! ignored test loads the **pinned** descriptors against the real graphs and
//! runs one inference each — the end-to-end proof that the adapter's declared
//! I/O contract (YuNet `input`/12 per-stride outputs/BGR/raw; SFace
//! `data`/`fc1`/raw) matches the shipped models. Without the env vars it is a
//! no-op, so `cargo test` stays weight-free and network-free.
//!
//! Run locally:
//! `LUMINA_FACE_DETECT_MODEL_PATH=…/face_detection_yunet_2023mar.onnx \
//!  LUMINA_FACE_EMBED_MODEL_PATH=…/face_recognition_sface_2021dec.onnx \
//!  cargo test -p lumina-onnx --features onnx-rt \
//!  real_weights_match_the_adapter_contract -- --ignored --nocapture`

#![cfg(feature = "onnx-rt")]

use lumina_core::ImageFrame;
use lumina_onnx::face::ort::{OrtFaceDetector, OrtFaceEmbedder};
use lumina_onnx::{
    face_detect_manifest, face_embed_manifest, DetectedFace, FaceDetectionInference,
    FaceEmbeddingInference, FaceInferenceOptions, ModelHashStatus, FACE_EMBED_DIMENSION,
};
use lumina_sidecar::{FaceBoundingBox, FaceLandmark};
use std::path::PathBuf;

fn frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (index % 251) as u8;
        px[1] = ((index * 7) % 251) as u8;
        px[2] = ((index * 13) % 251) as u8;
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

fn detection() -> DetectedFace {
    let landmark = |name: &str, x: f32, y: f32| FaceLandmark {
        name: name.into(),
        x,
        y,
    };
    DetectedFace {
        bbox: FaceBoundingBox {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        score: 0.9,
        landmarks: vec![
            landmark("left_eye", 0.35, 0.40),
            landmark("right_eye", 0.65, 0.40),
            landmark("nose", 0.50, 0.50),
            landmark("mouth_left", 0.40, 0.65),
            landmark("mouth_right", 0.60, 0.65),
        ],
    }
}

#[test]
#[ignore = "requires operator-supplied real YuNet/SFace artifacts (env paths)"]
fn real_weights_match_the_adapter_contract() {
    let (Ok(detect_path), Ok(embed_path)) = (
        std::env::var("LUMINA_FACE_DETECT_MODEL_PATH"),
        std::env::var("LUMINA_FACE_EMBED_MODEL_PATH"),
    ) else {
        eprintln!("skipping: set LUMINA_FACE_DETECT_MODEL_PATH / LUMINA_FACE_EMBED_MODEL_PATH");
        return;
    };

    let detector = OrtFaceDetector::new(
        PathBuf::from(&detect_path),
        face_detect_manifest(),
        FaceInferenceOptions::default(),
    )
    .expect("real YuNet must load through its declared contract");
    assert_eq!(
        detector.hash_status(),
        &ModelHashStatus::Verified,
        "the artifact must match the pinned YuNet model_hash"
    );
    let detections = detector
        .detect(&frame(640, 640))
        .expect("real YuNet must run through the per-stride adapter");
    eprintln!(
        "YuNet detections on a synthetic frame: {}",
        detections.len()
    );

    let embedder = OrtFaceEmbedder::new(
        PathBuf::from(&embed_path),
        face_embed_manifest(),
        FACE_EMBED_DIMENSION,
    )
    .expect("real SFace must load through its declared contract");
    assert_eq!(
        embedder.hash_status(),
        &ModelHashStatus::Verified,
        "the artifact must match the pinned SFace model_hash"
    );
    let embeddings = embedder
        .embed(&frame(640, 640), &[detection()])
        .expect("real SFace must run through the data/fc1 adapter");
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].dimension(), FACE_EMBED_DIMENSION as usize);
    assert!(embeddings[0].is_normalized());
}
