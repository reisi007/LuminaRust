//! YuNet per-stride detection adapter (LRPAR-G12-FACE-ADAPTER-25).
//!
//! The shipped YuNet graph (`opencv/opencv_zoo`, `face_detection_yunet_2023mar`)
//! does **not** emit the canonical S2 fused `[N, C]` detection tensor. It emits
//! twelve per-stride tensors — `cls_/obj_/bbox_/kps_{8,16,32}` — with one
//! anchor per feature-map cell:
//!
//! | tensor | shape (`n = (H/s)·(W/s)`) | meaning |
//! | --- | --- | --- |
//! | `cls_s` | `[1, n, 1]` | classification score |
//! | `obj_s` | `[1, n, 1]` | objectness score |
//! | `bbox_s` | `[1, n, 4]` | box regression `(dx, dy, log w, log h)` |
//! | `kps_s` | `[1, n, 10]` | 5 landmark pairs, cell-relative |
//!
//! This module decodes exactly that layout into the S2 [`DetectedFace`]
//! contract, mirroring OpenCV's `FaceDetectorYNImpl::postProcess`
//! (`modules/objdetect/src/face_detect.cpp`) byte for byte:
//!
//! * `score = sqrt(clamp(cls, 0, 1) · clamp(obj, 0, 1))`;
//! * `cx = (col + bbox[0]) · s`, `cy = (row + bbox[1]) · s`;
//! * `w = exp(bbox[2]) · s`, `h = exp(bbox[3]) · s`;
//! * landmark `i = ((kps[2i] + col) · s, (kps[2i+1] + row) · s)`.
//!
//! Detections are thresholded by score, boxes/landmarks are clamped to the
//! input frame (a deterministic decoder normalization, never a silent drop),
//! and the surviving boxes are suppressed by greedy non-maximum suppression.
//! All parameters that affect the result (`score_threshold`, `nms_threshold`,
//! `top_k`) are part of the persisted face identity — a change makes persisted
//! analyses `stale` instead of silently re-filtering them.
//!
//! No step guesses: a wrong shape, a wrong element count, a non-finite value or
//! an input resolution that is not divisible by the stride is a loud
//! [`OnnxError::InferenceFailed`].

use lumina_sidecar::{FaceBoundingBox, FaceLandmark};

use super::backend::DetectedFace;
use super::FACE_LANDMARK_NAMES_5PT;
use crate::manifest::ModelManifest;
use crate::OnnxError;

/// Per-stride anchors of the shipped YuNet graph (OpenCV divisor 32).
pub const YUNET_STRIDES: [u32; 3] = [8, 16, 32];
/// Landmark coordinates per anchor (5 points × 2 values).
pub const YUNET_LANDMARK_VALUES: usize = 2 * 5;

/// Canonical YuNet output tensor names in the documented order (OpenCV
/// `FaceDetectorYNImpl::detect`: `cls_8, cls_16, cls_32, obj_8, … kps_32`).
pub const YUNET_OUTPUT_NAMES: [&str; 12] = [
    "cls_8", "cls_16", "cls_32", "obj_8", "obj_16", "obj_32", "bbox_8", "bbox_16", "bbox_32",
    "kps_8", "kps_16", "kps_32",
];

/// Declared detection-head layout of a face-detection manifest.
///
/// `None` (returned by [`detection_head_for`] for every manifest that is not
/// the shipped YuNet descriptor) is the canonical S2 single-output contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceDetectHead {
    /// YuNet per-stride head: [`YUNET_OUTPUT_NAMES`].
    YuNetPerStride,
}

/// The declared detection head for `manifest` (see [`FaceDetectHead`]).
///
/// The decision is anchored on the **pinned YuNet identity** (`model_name` +
/// `model_version`), not on a graph probe: a manifest claiming the shipped
/// YuNet identity must provide the twelve declared tensors and is refused
/// loudly otherwise — never decoded as if it were the fused contract.
#[must_use]
pub fn detection_head_for(manifest: &ModelManifest) -> Option<FaceDetectHead> {
    if manifest.model_name == super::FACE_DETECT_MODEL_NAME
        && manifest.model_version == super::FACE_DETECT_MODEL_VERSION
    {
        Some(FaceDetectHead::YuNetPerStride)
    } else {
        None
    }
}

/// One stride's raw session outputs (shape + flattened `f32` data).
pub struct YunetStrideTensors<'a> {
    /// Anchor stride (`8`, `16` or `32`).
    pub stride: u32,
    /// `cls_s` shape and data.
    pub cls: (&'a [usize], &'a [f32]),
    /// `obj_s` shape and data.
    pub obj: (&'a [usize], &'a [f32]),
    /// `bbox_s` shape and data.
    pub bbox: (&'a [usize], &'a [f32]),
    /// `kps_s` shape and data.
    pub kps: (&'a [usize], &'a [f32]),
}

/// Decode the twelve YuNet tensors into the S2 [`DetectedFace`] contract.
///
/// `strides` must carry the per-stride tensors in the canonical order; each
/// tensor's anchor count is derived from `input_{width,height} / stride` and
/// validated against its declared shape and element count. `score_threshold`,
/// `nms_threshold` must be finite in `0..=1`; `top_k` must be `> 0`.
pub fn decode_yunet_detections(
    model_name: &str,
    input_width: u32,
    input_height: u32,
    strides: &[YunetStrideTensors<'_>],
    score_threshold: f32,
    nms_threshold: f32,
    top_k: usize,
) -> Result<Vec<DetectedFace>, OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    for (label, value) in [
        ("score_threshold", score_threshold),
        ("nms_threshold", nms_threshold),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(fail(format!(
                "YuNet {label} must be finite within 0..=1, got {value}"
            )));
        }
    }
    if top_k == 0 {
        return Err(fail("YuNet top_k must be > 0".into()));
    }
    if input_width == 0 || input_height == 0 {
        return Err(fail(format!(
            "YuNet input resolution must be non-zero, got {input_width}x{input_height}"
        )));
    }

    let mut detections = Vec::new();
    for tensors in strides {
        detections.extend(decode_stride(
            model_name,
            input_width,
            input_height,
            tensors,
            score_threshold,
        )?);
    }
    Ok(nms_faces(detections, nms_threshold, top_k))
}

fn decode_stride(
    model_name: &str,
    input_width: u32,
    input_height: u32,
    tensors: &YunetStrideTensors<'_>,
    score_threshold: f32,
) -> Result<Vec<DetectedFace>, OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    let stride = tensors.stride;
    if stride == 0 {
        return Err(fail("YuNet stride must be > 0".into()));
    }
    if !input_width.is_multiple_of(stride) || !input_height.is_multiple_of(stride) {
        return Err(fail(format!(
            "YuNet input resolution {input_width}x{input_height} is not divisible by stride {stride}"
        )));
    }
    let rows = (input_height / stride) as usize;
    let cols = (input_width / stride) as usize;
    let anchors = rows * cols;

    let cls = stride_values("cls", tensors.cls, anchors, 1, model_name, stride)?;
    let obj = stride_values("obj", tensors.obj, anchors, 1, model_name, stride)?;
    let bbox = stride_values("bbox", tensors.bbox, anchors, 4, model_name, stride)?;
    let kps = stride_values(
        "kps",
        tensors.kps,
        anchors,
        YUNET_LANDMARK_VALUES,
        model_name,
        stride,
    )?;

    let (frame_w, frame_h) = (input_width as f32, input_height as f32);
    let mut detections = Vec::new();
    for index in 0..anchors {
        let cls_score = cls[index].clamp(0.0, 1.0);
        let obj_score = obj[index].clamp(0.0, 1.0);
        let score = (cls_score * obj_score).sqrt();
        if !score.is_finite() {
            return Err(fail(format!(
                "YuNet stride {stride} anchor {index} produced a non-finite score"
            )));
        }
        if score < score_threshold {
            continue;
        }
        let row = index / cols;
        let col = index % cols;

        let cx = (col as f32 + bbox[index * 4]) * stride as f32;
        let cy = (row as f32 + bbox[index * 4 + 1]) * stride as f32;
        let width = bbox[index * 4 + 2].exp() * stride as f32;
        let height = bbox[index * 4 + 3].exp() * stride as f32;
        let x1 = (cx - width / 2.0) / frame_w;
        let y1 = (cy - height / 2.0) / frame_h;
        let x2 = (cx + width / 2.0) / frame_w;
        let y2 = (cy + height / 2.0) / frame_h;
        if ![x1, y1, x2, y2].iter().all(|value| value.is_finite()) {
            return Err(fail(format!(
                "YuNet stride {stride} anchor {index} produced a non-finite box"
            )));
        }
        let bbox = clamp_box(x1, y1, x2, y2);
        // A box that clamps to zero area (entirely outside the frame) carries
        // no in-frame face; it is excluded deterministically.
        if bbox.width <= 0.0 || bbox.height <= 0.0 {
            continue;
        }

        let mut landmarks = Vec::with_capacity(FACE_LANDMARK_NAMES_5PT.len());
        for (point, name) in FACE_LANDMARK_NAMES_5PT.iter().enumerate() {
            let x = (kps[index * YUNET_LANDMARK_VALUES + point * 2] + col as f32) * stride as f32
                / frame_w;
            let y = (kps[index * YUNET_LANDMARK_VALUES + point * 2 + 1] + row as f32)
                * stride as f32
                / frame_h;
            if !x.is_finite() || !y.is_finite() {
                return Err(fail(format!(
                    "YuNet stride {stride} anchor {index} produced a non-finite landmark"
                )));
            }
            landmarks.push(FaceLandmark {
                name: (*name).to_owned(),
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
            });
        }
        let detection = DetectedFace {
            bbox,
            score,
            landmarks,
        };
        detection.validate()?;
        detections.push(detection);
    }
    Ok(detections)
}

/// Validate one per-stride tensor and return its `f32` payload.
fn stride_values<'a>(
    kind: &str,
    tensor: (&'a [usize], &'a [f32]),
    anchors: usize,
    channels: usize,
    model_name: &str,
    stride: u32,
) -> Result<&'a [f32], OnnxError> {
    let (shape, data) = tensor;
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    let (rows, cols) = match shape {
        [n, c] => (*n, *c),
        [1, n, c] => (*n, *c),
        _ => {
            return Err(fail(format!(
                "YuNet `{kind}_{stride}` has unexpected shape {shape:?}, expected [n, {channels}] \
                 or [1, n, {channels}]"
            )))
        }
    };
    if rows != anchors || cols != channels {
        return Err(fail(format!(
            "YuNet `{kind}_{stride}` shape {shape:?} does not match {anchors} anchors × {channels} \
             channel(s)"
        )));
    }
    if data.len() != anchors * channels {
        return Err(fail(format!(
            "YuNet `{kind}_{stride}` holds {} values, expected {}",
            data.len(),
            anchors * channels
        )));
    }
    Ok(data)
}

/// Clamp a normalized box to the frame `[0, 1]²`, preserving non-negative extents.
fn clamp_box(x1: f32, y1: f32, x2: f32, y2: f32) -> FaceBoundingBox {
    let x1 = x1.clamp(0.0, 1.0);
    let y1 = y1.clamp(0.0, 1.0);
    let x2 = x2.clamp(0.0, 1.0);
    let y2 = y2.clamp(0.0, 1.0);
    FaceBoundingBox {
        x: x1.min(x2),
        y: y1.min(y2),
        width: (x2 - x1).abs(),
        height: (y2 - y1).abs(),
    }
}

/// Greedy non-maximum suppression over normalized boxes.
///
/// Detections are sorted by score (descending; ties keep their input order, so
/// the result is deterministic) and a candidate is kept only when its
/// intersection-over-union with every already-kept box is `<= iou_threshold`.
/// At most `top_k` boxes are returned.
#[must_use]
pub fn nms_faces(
    mut faces: Vec<DetectedFace>,
    iou_threshold: f32,
    top_k: usize,
) -> Vec<DetectedFace> {
    // Stable sort keeps the input order for equal scores.
    faces.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<DetectedFace> = Vec::new();
    for face in faces {
        if kept.len() >= top_k {
            break;
        }
        if kept
            .iter()
            .all(|kept| intersection_over_union(&kept.bbox, &face.bbox) <= iou_threshold)
        {
            kept.push(face);
        }
    }
    kept
}

/// Intersection-over-union of two normalized boxes.
fn intersection_over_union(a: &FaceBoundingBox, b: &FaceBoundingBox) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let inter_w = (ax2.min(bx2) - a.x.max(b.x)).max(0.0);
    let inter_h = (ay2.min(by2) - a.y.max(b.y)).max(0.0);
    let inter = inter_w * inter_h;
    let union = a.width * a.height + b.width * b.height - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}
