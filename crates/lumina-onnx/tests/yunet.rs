//! LRPAR-G12-FACE-ADAPTER-25 — YuNet per-stride decoder tests (default feature
//! set, no ONNX Runtime, no weights, no network).
//!
//! These exercise the pure decode/NMS surface (`lumina-onnx::face::yunet`) and
//! the model-identity-anchored head selection. The real ORT path over the
//! crafted twelve-output graph lives in `tests/face_adapter_ort.rs`.

use lumina_onnx::{
    decode_yunet_detections, detection_head_for, face_detect_manifest, face_embed_manifest,
    nms_faces, DetectedFace, FaceDetectHead, YunetStrideTensors, FACE_LANDMARK_NAMES_5PT,
    YUNET_OUTPUT_NAMES, YUNET_STRIDES,
};
use lumina_sidecar::{FaceBoundingBox, FaceLandmark};

fn face(score: f32, x: f32, y: f32, w: f32, h: f32) -> DetectedFace {
    DetectedFace {
        bbox: FaceBoundingBox {
            x,
            y,
            width: w,
            height: h,
        },
        score,
        landmarks: FACE_LANDMARK_NAMES_5PT
            .iter()
            .map(|name| FaceLandmark {
                name: (*name).to_owned(),
                x: 0.5,
                y: 0.5,
            })
            .collect(),
    }
}

/// The declared tensor names and strides are the documented OpenCV contract.
#[test]
fn output_names_and_strides_are_the_documented_contract() {
    assert_eq!(YUNET_STRIDES, [8, 16, 32]);
    assert_eq!(
        YUNET_OUTPUT_NAMES,
        [
            "cls_8", "cls_16", "cls_32", "obj_8", "obj_16", "obj_32", "bbox_8", "bbox_16",
            "bbox_32", "kps_8", "kps_16", "kps_32",
        ]
    );
}

/// One anchor at stride 8 on an 8×8 input; exact expected geometry from the
/// OpenCV decode formulas (hand-computed).
#[test]
fn decode_matches_the_documented_formulas() {
    let cls = [0.81f32];
    let obj = [1.0f32];
    let bbox = [0.5f32, 0.25, 0.0, 0.0];
    let kps = [0.1, 0.2, 0.3, 0.4, 0.5, 0.5, 0.6, 0.6, 0.7, 0.7];
    let tensors = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    let faces = decode_yunet_detections("YuNet", 8, 8, &[tensors], 0.5, 0.3, 5000).unwrap();
    assert_eq!(faces.len(), 1);
    let face = &faces[0];
    // score = sqrt(0.81 * 1.0) = 0.9.
    assert!((face.score - 0.9).abs() < 1e-6, "score {}", face.score);
    // cx = (0 + 0.5) * 8 = 4 -> 4/8 = 0.5; cy = (0 + 0.25) * 8 = 2 -> 0.25;
    // w = h = exp(0) * 8 = 8 -> 1.0. The box clamps to the frame vertically.
    assert!((face.bbox.x - 0.0).abs() < 1e-6, "{:?}", face.bbox);
    assert!((face.bbox.width - 1.0).abs() < 1e-6, "{:?}", face.bbox);
    assert!((face.bbox.height - 0.75).abs() < 1e-6, "{:?}", face.bbox);
    // Landmarks: first = ((0.1 + 0) * 8 / 8, (0.2 + 0) * 8 / 8).
    assert!((faces[0].landmarks[0].x - 0.1).abs() < 1e-6);
    assert!((faces[0].landmarks[0].y - 0.2).abs() < 1e-6);
    assert_eq!(faces[0].landmarks[0].name, "left_eye");
    assert_eq!(faces[0].landmarks.len(), 5);
}

/// The classification/objectness product is clamped before the square root,
/// exactly like OpenCV (`min(max(x, 0), 1)`), and the score threshold filters
/// anchors loudly but quietly skips sub-threshold ones.
#[test]
fn score_is_clamped_and_thresholded() {
    let bbox = [0.0f32, 0.0, 0.0, 0.0];
    let kps = [0.0f32; 10];
    // cls > 1 clamps to 1; obj = 0.64 -> score 0.8.
    let cls = [2.0f32];
    let obj = [0.64f32];
    let tensors = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    let faces = decode_yunet_detections("YuNet", 8, 8, &[tensors], 0.5, 0.3, 5000).unwrap();
    assert_eq!(faces.len(), 1);
    assert!((faces[0].score - 0.8).abs() < 1e-6, "{}", faces[0].score);

    // Below the threshold -> filtered, not an error.
    let cls = [0.25f32];
    let obj = [0.25f32]; // sqrt(0.0625) = 0.25
    let tensors = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    let faces = decode_yunet_detections("YuNet", 8, 8, &[tensors], 0.5, 0.3, 5000).unwrap();
    assert!(faces.is_empty());
}

#[test]
fn nms_suppresses_duplicates_and_respects_the_threshold() {
    // Two identical boxes and one disjoint box, all above the score gate.
    let duplicates = vec![
        face(0.9, 0.1, 0.1, 0.2, 0.2),
        face(0.8, 0.1, 0.1, 0.2, 0.2),
        face(0.7, 0.6, 0.6, 0.2, 0.2),
    ];
    let kept = nms_faces(duplicates, 0.3, 5000);
    assert_eq!(kept.len(), 2, "the duplicate must be suppressed");
    assert!((kept[0].score - 0.9).abs() < 1e-6, "higher score wins");

    // Overlapping but not identical boxes: the IoU threshold genuinely drives
    // the decision. IoU((0,0,1,1), (0.5,0.5,1,1)) = 0.25/1.75 ≈ 0.1429, so 0.3
    // keeps both and 0.1 suppresses the weaker one.
    let overlapping = vec![face(0.9, 0.0, 0.0, 1.0, 1.0), face(0.8, 0.5, 0.5, 1.0, 1.0)];
    assert_eq!(
        nms_faces(overlapping.clone(), 0.3, 5000).len(),
        2,
        "IoU ≈ 0.14 stays below a 0.3 threshold"
    );
    assert_eq!(
        nms_faces(overlapping, 0.1, 5000).len(),
        1,
        "IoU ≈ 0.14 exceeds a 0.1 threshold"
    );
}

#[test]
fn nms_top_k_truncates_deterministically() {
    let faces = vec![
        face(0.5, 0.0, 0.0, 0.1, 0.1),
        face(0.9, 0.2, 0.2, 0.1, 0.1),
        face(0.7, 0.4, 0.4, 0.1, 0.1),
    ];
    let kept = nms_faces(faces, 0.3, 2);
    assert_eq!(kept.len(), 2);
    // Highest two scores, in descending order.
    assert!((kept[0].score - 0.9).abs() < 1e-6);
    assert!((kept[1].score - 0.7).abs() < 1e-6);
}

#[test]
fn decode_is_loud_on_bad_shapes_and_parameters() {
    let cls = [0.9f32];
    let obj = [0.9f32];
    let bbox = [0.0f32, 0.0, 0.0, 0.0];
    let kps = [0.0f32; 10];
    let good = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    // Correct shape works.
    let good_ref = std::slice::from_ref(&good);
    assert!(decode_yunet_detections("YuNet", 8, 8, good_ref, 0.5, 0.3, 5000).is_ok());

    // Wrong channel count, wrong shape rank, wrong element count → loud.
    let wrong_channels = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 2], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    assert!(decode_yunet_detections("YuNet", 8, 8, &[wrong_channels], 0.5, 0.3, 5000).is_err());
    let wrong_rank = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    assert!(decode_yunet_detections("YuNet", 8, 8, &[wrong_rank], 0.5, 0.3, 5000).is_err());
    let short = [0.0f32; 3];
    let wrong_len = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &short),
        kps: (&[1, 1, 10], &kps),
    };
    assert!(decode_yunet_detections("YuNet", 8, 8, &[wrong_len], 0.5, 0.3, 5000).is_err());

    // Non-divisible resolution, bad thresholds, zero top_k → loud.
    assert!(decode_yunet_detections("YuNet", 30, 32, good_ref, 0.5, 0.3, 5000).is_err());
    assert!(decode_yunet_detections("YuNet", 8, 8, good_ref, 1.5, 0.3, 5000).is_err());
    assert!(decode_yunet_detections("YuNet", 8, 8, good_ref, 0.5, f32::NAN, 5000).is_err());
    assert!(decode_yunet_detections("YuNet", 8, 8, good_ref, 0.5, 0.3, 0).is_err());
}

#[test]
fn decode_clamps_boxes_to_the_frame() {
    // A box with a huge extent overflows the frame; it must clamp to a valid
    // in-frame box, not error.
    let cls = [0.9f32];
    let obj = [0.9f32];
    let bbox = [0.5f32, 0.5, 2.0, 2.0]; // w = exp(2) * 8 ≈ 59
    let kps = [0.0f32; 10];
    let tensors = YunetStrideTensors {
        stride: 8,
        cls: (&[1, 1, 1], &cls),
        obj: (&[1, 1, 1], &obj),
        bbox: (&[1, 1, 4], &bbox),
        kps: (&[1, 1, 10], &kps),
    };
    let faces = decode_yunet_detections("YuNet", 8, 8, &[tensors], 0.5, 0.3, 5000).unwrap();
    assert_eq!(faces.len(), 1);
    let bbox = faces[0].bbox;
    assert!(bbox.x >= 0.0 && bbox.x + bbox.width <= 1.0 + 1e-6);
    assert!(bbox.y >= 0.0 && bbox.y + bbox.height <= 1.0 + 1e-6);
}

#[test]
fn detection_head_is_anchored_on_the_pinned_yunet_identity() {
    assert_eq!(
        detection_head_for(&face_detect_manifest()),
        Some(FaceDetectHead::YuNetPerStride)
    );
    assert_eq!(detection_head_for(&face_embed_manifest()), None);
    let mut renamed = face_detect_manifest();
    renamed.model_name = "SCRFD".into();
    assert_eq!(detection_head_for(&renamed), None);
    let mut reversioned = face_detect_manifest();
    reversioned.model_version = "2026".into();
    assert_eq!(detection_head_for(&reversioned), None);
}
