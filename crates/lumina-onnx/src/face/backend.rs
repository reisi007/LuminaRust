//! Face inference surface and deterministic, tests-only stub backends.
//!
//! `FACE-20-S2` splits the face pipeline into two exchangeable inference
//! surfaces — [`FaceDetectionInference`] (RGB → boxes/scores/landmarks) and
//! [`FaceEmbeddingInference`] (crop → normalized vector). The real ONNX
//! Runtime implementations live behind `onnx-rt` ([`super::ort`]); the
//! deterministic stubs here are the complete, tested default surface for
//! tests and fixtures.
//!
//! **Tests-only marker:** the stubs are explicitly *not* a production
//! fallback. They are deterministic, weight-free and network-free; a caller
//! that needs real inference must obtain the `onnx-rt` engine, which fails
//! visibly when no artifact is available. A stub reporting itself unavailable
//! refuses inference with [`OnnxError::ModelUnavailable`] — it never silently
//! emits faces.

use crate::manifest::ModelManifest;
use crate::OnnxError;
use lumina_core::ImageFrame;
use lumina_sidecar::{FaceBoundingBox, FaceLandmark, MAX_FACE_LANDMARKS};

/// One detected face in normalized oriented-source coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedFace {
    /// Normalized bounding box (`0..=1`).
    pub bbox: FaceBoundingBox,
    /// Detection confidence in `0..=1`.
    pub score: f32,
    /// Named landmarks in normalized coordinates (order pinned by
    /// [`super::FACE_LANDMARK_NAMES_5PT`] for the planned 5-point alignment).
    pub landmarks: Vec<FaceLandmark>,
}

impl DetectedFace {
    /// Validate the detection against the S1 sidecar contract (loud, no
    /// clamping).
    pub fn validate(&self) -> Result<(), OnnxError> {
        let in_unit = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
        let bbox = self.bbox;
        if !in_unit(bbox.x)
            || !in_unit(bbox.y)
            || !in_unit(bbox.width)
            || !in_unit(bbox.height)
            || bbox.width <= 0.0
            || bbox.height <= 0.0
            || bbox.x + bbox.width > 1.0
            || bbox.y + bbox.height > 1.0
        {
            return Err(OnnxError::InvalidFaceData(
                "face bbox must be finite within 0..=1 with a positive area inside the frame"
                    .into(),
            ));
        }
        if !self.score.is_finite() || !(0.0..=1.0).contains(&self.score) {
            return Err(OnnxError::InvalidFaceData(format!(
                "face score must be finite within 0..=1, got {}",
                self.score
            )));
        }
        if self.landmarks.len() > MAX_FACE_LANDMARKS {
            return Err(OnnxError::InvalidFaceData(format!(
                "face landmarks exceed limit of {MAX_FACE_LANDMARKS}"
            )));
        }
        let mut seen = std::collections::BTreeSet::new();
        for landmark in &self.landmarks {
            if landmark.name.trim().is_empty() || landmark.name.chars().any(char::is_control) {
                return Err(OnnxError::InvalidFaceData(
                    "face landmark name must be non-empty without control characters".into(),
                ));
            }
            if !seen.insert(landmark.name.as_str()) {
                return Err(OnnxError::InvalidFaceData(format!(
                    "duplicate face landmark `{}`",
                    landmark.name
                )));
            }
            if !landmark.x.is_finite()
                || !landmark.y.is_finite()
                || !(0.0..=1.0).contains(&landmark.x)
                || !(0.0..=1.0).contains(&landmark.y)
            {
                return Err(OnnxError::InvalidFaceData(
                    "face landmark coordinates must be finite within 0..=1".into(),
                ));
            }
        }
        Ok(())
    }

    /// Normalized box center `(cx, cy)`.
    #[must_use]
    pub fn center(&self) -> (f32, f32) {
        (
            self.bbox.x + self.bbox.width / 2.0,
            self.bbox.y + self.bbox.height / 2.0,
        )
    }
}

/// A normalized face identity vector (never inlined in the JSON sidecar; the
/// persisted form is a [`lumina_sidecar::FaceVectorRef`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEmbeddingVector {
    values: Vec<f32>,
}

impl FaceEmbeddingVector {
    /// Build from raw values: non-empty, all finite. The vector is *not*
    /// implicitly normalized; callers must request [`Self::l2_normalized`]
    /// when the contract requires it.
    pub fn new(values: Vec<f32>) -> Result<Self, OnnxError> {
        if values.is_empty() {
            return Err(OnnxError::InvalidFaceData(
                "face embedding vector must not be empty".into(),
            ));
        }
        if let Some(bad) = values.iter().position(|value| !value.is_finite()) {
            return Err(OnnxError::InvalidFaceData(format!(
                "face embedding value at index {bad} is not finite"
            )));
        }
        Ok(Self { values })
    }

    /// The raw values.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// The raw values, consuming the vector.
    #[must_use]
    pub fn into_values(self) -> Vec<f32> {
        self.values
    }

    /// Vector length.
    #[must_use]
    pub fn dimension(&self) -> usize {
        self.values.len()
    }

    /// L2-normalize in place. A zero-norm vector is a loud error (never a
    /// silent fallback to an arbitrary vector).
    pub fn l2_normalized(self) -> Result<Self, OnnxError> {
        let norm = self.l2_norm();
        if norm <= f64::EPSILON {
            return Err(OnnxError::InvalidFaceData(
                "face embedding vector has zero L2 norm".into(),
            ));
        }
        let values = self
            .values
            .into_iter()
            .map(|value| (f64::from(value) / norm) as f32)
            .collect();
        Ok(Self { values })
    }

    /// Enforce a declared embedding dimension (loud on mismatch).
    pub fn with_dimension(self, expected: u32) -> Result<Self, OnnxError> {
        if self.values.len() != expected as usize {
            return Err(OnnxError::InvalidFaceData(format!(
                "face embedding has dimension {}, expected {expected}",
                self.values.len()
            )));
        }
        Ok(self)
    }

    /// Whether the vector is unit-normalized within a documented tolerance
    /// (`1e-3`), matching the persisted `l2` contract.
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        (self.l2_norm() - 1.0).abs() <= 1e-3
    }

    fn l2_norm(&self) -> f64 {
        self.values
            .iter()
            .map(|value| f64::from(*value) * f64::from(*value))
            .sum::<f64>()
            .sqrt()
    }
}

/// Whole-image face detection surface.
pub trait FaceDetectionInference {
    /// The model manifest this backend was built from.
    fn manifest(&self) -> &ModelManifest;

    /// Whether the model artifact/weights required for inference are present.
    fn is_available(&self) -> bool {
        true
    }

    /// Detect faces in `image`. Must never silently fall back on a missing or
    /// mismatched artifact.
    fn detect(&self, image: &ImageFrame) -> Result<Vec<DetectedFace>, OnnxError>;
}

/// Per-face embedding surface. Implementations align each detection with the
/// documented landmark template before inference.
pub trait FaceEmbeddingInference {
    /// The model manifest this backend was built from.
    fn manifest(&self) -> &ModelManifest;

    /// Declared embedding dimension.
    fn dimension(&self) -> u32;

    /// Whether the model artifact/weights required for inference are present.
    fn is_available(&self) -> bool {
        true
    }

    /// Embed every detection, returning exactly one vector per detection (same
    /// order). Must never silently skip a detection or substitute a vector.
    fn embed(
        &self,
        image: &ImageFrame,
        detections: &[DetectedFace],
    ) -> Result<Vec<FaceEmbeddingVector>, OnnxError>;
}

/// Deterministic, weight-free detection stub (`FACE-20-S2`, tests-only).
///
/// Default output is a single centered face with the canonical 5-point
/// landmarks, derived purely from the contract — no weights, no network, no
/// pixel dependency. Tests inject exact detections with
/// [`StubFaceDetector::with_detections`].
#[derive(Debug)]
pub struct StubFaceDetector {
    manifest: ModelManifest,
    available: bool,
    detections: Option<Vec<DetectedFace>>,
}

impl StubFaceDetector {
    /// Build a stub from a validated manifest that declares `face_detect`.
    pub fn new(manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        if !manifest.capabilities.face_detect {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "face_detect not declared".into(),
            });
        }
        Ok(Self {
            manifest,
            available: true,
            detections: None,
        })
    }

    /// Override the reported availability (simulates a missing installation).
    #[must_use]
    pub fn with_availability(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Inject an exact detection set (validated eagerly) for deterministic
    /// tests; the stub then returns it verbatim.
    pub fn with_detections(mut self, detections: Vec<DetectedFace>) -> Result<Self, OnnxError> {
        for detection in &detections {
            detection.validate()?;
        }
        self.detections = Some(detections);
        Ok(self)
    }

    /// Whether this backend can currently perform inference.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// The documented deterministic default detection set: one centered face
    /// with the canonical 5-point landmarks.
    #[must_use]
    pub fn default_detections() -> Vec<DetectedFace> {
        let landmark = |name: &str, x: f32, y: f32| FaceLandmark {
            name: name.into(),
            x,
            y,
        };
        vec![DetectedFace {
            bbox: FaceBoundingBox {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            score: 0.5,
            landmarks: vec![
                landmark("left_eye", 0.35, 0.40),
                landmark("right_eye", 0.65, 0.40),
                landmark("nose", 0.50, 0.50),
                landmark("mouth_left", 0.40, 0.65),
                landmark("mouth_right", 0.60, 0.65),
            ],
        }]
    }
}

impl FaceDetectionInference for StubFaceDetector {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn detect(&self, image: &ImageFrame) -> Result<Vec<DetectedFace>, OnnxError> {
        if !self.available {
            return Err(OnnxError::ModelUnavailable {
                name: self.manifest.model_name.clone(),
            });
        }
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        Ok(self
            .detections
            .clone()
            .unwrap_or_else(Self::default_detections))
    }
}

/// Deterministic, weight-free embedding stub (`FACE-20-S2`, tests-only).
///
/// Default vectors are derived from the detection box through a documented
/// integer hash (SplitMix64) and L2-normalized — fully deterministic and
/// platform-independent. Tests inject exact vectors with
/// [`StubFaceEmbedder::with_vectors`].
#[derive(Debug)]
pub struct StubFaceEmbedder {
    manifest: ModelManifest,
    dimension: u32,
    available: bool,
    vectors: Option<Vec<Vec<f32>>>,
}

impl StubFaceEmbedder {
    /// Build a stub from a validated manifest that declares `face_embed`.
    pub fn new(manifest: ModelManifest, dimension: u32) -> Result<Self, OnnxError> {
        manifest.validate()?;
        if !manifest.capabilities.face_embed {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "face_embed not declared".into(),
            });
        }
        if dimension == 0 {
            return Err(OnnxError::InvalidFaceData(
                "face embedding dimension must be > 0".into(),
            ));
        }
        Ok(Self {
            manifest,
            dimension,
            available: true,
            vectors: None,
        })
    }

    /// Override the reported availability (simulates a missing installation).
    #[must_use]
    pub fn with_availability(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Inject exact vectors (one per detection, of the declared dimension).
    pub fn with_vectors(mut self, vectors: Vec<Vec<f32>>) -> Result<Self, OnnxError> {
        for vector in &vectors {
            let vector =
                FaceEmbeddingVector::new(vector.clone())?.with_dimension(self.dimension)?;
            if !vector.is_normalized() {
                return Err(OnnxError::InvalidFaceData(
                    "injected stub vectors must be unit-normalized".into(),
                ));
            }
        }
        self.vectors = Some(vectors);
        Ok(self)
    }

    /// Whether this backend can currently perform inference.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.available
    }

    fn stub_vector(&self, face: &DetectedFace) -> Result<FaceEmbeddingVector, OnnxError> {
        let (cx, cy) = face.center();
        let seed = splitmix64(0x5EED_FACE_u64)
            ^ splitmix64(u64::from(face.bbox.x.to_bits()))
            ^ splitmix64(u64::from(face.bbox.y.to_bits()))
            ^ splitmix64(u64::from(cx.to_bits()))
            ^ splitmix64(u64::from(cy.to_bits()));
        let values: Vec<f32> = (0..self.dimension)
            .map(|index| {
                let hashed = splitmix64(seed ^ u64::from(index));
                let unit = (hashed >> 40) as f32 / (1u64 << 24) as f32; // [0, 1)
                unit * 2.0 - 1.0
            })
            .collect();
        FaceEmbeddingVector::new(values)?.l2_normalized()
    }
}

impl FaceEmbeddingInference for StubFaceEmbedder {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn dimension(&self) -> u32 {
        self.dimension
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn embed(
        &self,
        image: &ImageFrame,
        detections: &[DetectedFace],
    ) -> Result<Vec<FaceEmbeddingVector>, OnnxError> {
        if !self.available {
            return Err(OnnxError::ModelUnavailable {
                name: self.manifest.model_name.clone(),
            });
        }
        if detections.is_empty() {
            return Ok(Vec::new());
        }
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        if let Some(vectors) = &self.vectors {
            if vectors.len() != detections.len() {
                return Err(OnnxError::InvalidFaceData(format!(
                    "injected {} stub vectors for {} detections",
                    vectors.len(),
                    detections.len()
                )));
            }
            return vectors
                .iter()
                .cloned()
                .map(|values| {
                    FaceEmbeddingVector::new(values)?
                        .with_dimension(self.dimension)?
                        .l2_normalized()
                })
                .collect();
        }
        detections
            .iter()
            .map(|face| self.stub_vector(face))
            .collect()
    }
}

/// SplitMix64 — a documented, platform-independent integer hash used only by
/// the deterministic stub.
fn splitmix64(mut state: u64) -> u64 {
    state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Canonical 5-point alignment template for a 112×112 face crop (ArcFace /
/// MobileFaceNet family), in template pixels.
///
/// The template is part of the preprocessing identity
/// ([`super::FACE_PREPROCESSING_NAME`]); the landmark order is
/// [`super::FACE_LANDMARK_NAMES_5PT`]. A different template requires a new
/// preprocessing version (never a silent re-alignment).
pub const FACE_ALIGN_TEMPLATE_5PT: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

/// Reference size the [`FACE_ALIGN_TEMPLATE_5PT`] template is defined for.
pub const FACE_ALIGN_TEMPLATE_BASE: f32 = 112.0;

/// Documented canonical **detection output contract**.
///
/// A detector emits a single float32 tensor of shape `[N, C]` or `[1, N, C]`
/// with `C = 5 + 2*K`:
///
/// | columns | meaning |
/// | --- | --- |
/// | `0..=3` | normalized box center + size `(cx, cy, w, h)` |
/// | `4` | detection `score` in `0..=1` |
/// | `5..` | `K` landmark pairs `(x, y)`, normalized |
///
/// `K = 5` uses the canonical [`super::FACE_LANDMARK_NAMES_5PT`] names; any
/// other `K` uses `landmark_<i>`. Every row is validated against the S1
/// sidecar contract — an out-of-frame box or a non-finite value is a loud
/// [`OnnxError::InferenceFailed`], never a silent clamp/reshape.
pub fn decode_face_detections(
    model_name: &str,
    shape: &[usize],
    data: &[f32],
    score_threshold: f32,
) -> Result<Vec<DetectedFace>, OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    if !score_threshold.is_finite() || !(0.0..=1.0).contains(&score_threshold) {
        return Err(fail(format!(
            "detection score threshold must be finite within 0..=1, got {score_threshold}"
        )));
    }
    let (rows, columns) = match shape {
        [rows, columns] => (*rows, *columns),
        [1, rows, columns] => (*rows, *columns),
        _ => {
            return Err(fail(format!(
                "unexpected detection output shape {shape:?}: expected [N, C] or [1, N, C]"
            )))
        }
    };
    if columns < 5 || (columns - 5) % 2 != 0 {
        return Err(fail(format!(
            "detection output has {columns} columns; expected 5 + 2*K (box + score + landmarks)"
        )));
    }
    if data.len() != rows * columns {
        return Err(fail(format!(
            "detection output holds {} values for shape {rows}x{columns}",
            data.len()
        )));
    }
    let landmarks_per_face = (columns - 5) / 2;
    let mut detections = Vec::new();
    for index in 0..rows {
        let start = index * columns;
        let values = &data[start..start + columns];
        let score = values[4];
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            return Err(fail(format!(
                "detection {index} score must be finite within 0..=1, got {score}"
            )));
        }
        if score < score_threshold {
            continue;
        }
        let (cx, cy, width, height) = (values[0], values[1], values[2], values[3]);
        let bbox = FaceBoundingBox {
            x: cx - width / 2.0,
            y: cy - height / 2.0,
            width,
            height,
        };
        let landmarks = (0..landmarks_per_face)
            .map(|landmark| FaceLandmark {
                name: if landmarks_per_face == super::FACE_LANDMARK_NAMES_5PT.len() {
                    super::FACE_LANDMARK_NAMES_5PT[landmark].to_owned()
                } else {
                    format!("landmark_{landmark}")
                },
                x: values[5 + landmark * 2],
                y: values[5 + landmark * 2 + 1],
            })
            .collect();
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

/// Documented canonical **embedding output contract**.
///
/// An embedder emits a single float32 tensor whose element count equals the
/// declared `dimension`, with at most one non-unit axis (`[D]`, `[1, D]`, …).
/// The vector is L2-normalized before it is returned; a shape that would need a
/// silent reshape (e.g. a spatial map) is a loud
/// [`OnnxError::InferenceFailed`].
pub fn decode_face_embedding(
    model_name: &str,
    shape: &[usize],
    data: &[f32],
    dimension: u32,
) -> Result<FaceEmbeddingVector, OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason,
    };
    if dimension == 0 {
        return Err(fail("declared embedding dimension must be > 0".into()));
    }
    let non_unit: Vec<usize> = shape.iter().copied().filter(|&axis| axis != 1).collect();
    if non_unit.len() > 1 {
        return Err(fail(format!(
            "embedding output shape {shape:?} has more than one non-unit axis; expected [D] or \
             [1, D, ...]"
        )));
    }
    if data.len() != dimension as usize {
        return Err(fail(format!(
            "embedding output holds {} values, expected dimension {dimension}",
            data.len()
        )));
    }
    FaceEmbeddingVector::new(data.to_vec())?.l2_normalized()
}

/// Align a face crop to the canonical 5-point template with a deterministic
/// nearest-neighbour similarity warp.
///
/// The similarity transform (rotation + uniform scale + translation, no shear)
/// is the least-squares fit of the detection landmarks to
/// [`FACE_ALIGN_TEMPLATE_5PT`], scaled to `resolution`. Sampling is
/// nearest-neighbour with round-half-away-from-zero and clamped to the source
/// frame, so the result is platform-independent. Missing landmarks or a
/// degenerate (coincident) point set are loud errors — there is no silent
/// box-crop fallback.
pub fn align_face_to_template(
    image: &ImageFrame,
    landmarks: &[FaceLandmark],
    resolution: (u32, u32),
) -> Result<ImageFrame, OnnxError> {
    let fail = |reason: String| OnnxError::InferenceFailed {
        name: super::FACE_PREPROCESSING_NAME.to_owned(),
        reason,
    };
    if image.width == 0 || image.height == 0 || resolution.0 == 0 || resolution.1 == 0 {
        return Err(OnnxError::InvalidDimensions {
            expected_width: resolution.0,
            expected_height: resolution.1,
            actual_width: image.width,
            actual_height: image.height,
        });
    }
    if landmarks.len() != FACE_ALIGN_TEMPLATE_5PT.len() {
        return Err(fail(format!(
            "alignment requires exactly {} landmarks, got {}",
            FACE_ALIGN_TEMPLATE_5PT.len(),
            landmarks.len()
        )));
    }

    // Map declared landmark names onto the template order.
    let mut source = [(0.0f32, 0.0f32); 5];
    for (index, name) in super::FACE_LANDMARK_NAMES_5PT.iter().enumerate() {
        let landmark = landmarks
            .iter()
            .find(|landmark| landmark.name == *name)
            .ok_or_else(|| fail(format!("alignment requires the landmark `{name}`")))?;
        source[index] = (
            landmark.x * image.width as f32,
            landmark.y * image.height as f32,
        );
    }
    let scale_x = resolution.0 as f32 / FACE_ALIGN_TEMPLATE_BASE;
    let scale_y = resolution.1 as f32 / FACE_ALIGN_TEMPLATE_BASE;
    let target = FACE_ALIGN_TEMPLATE_5PT.map(|(x, y)| (x * scale_x, y * scale_y));

    let count = source.len() as f32;
    let source_mean = (
        source.iter().map(|p| p.0).sum::<f32>() / count,
        source.iter().map(|p| p.1).sum::<f32>() / count,
    );
    let target_mean = (
        target.iter().map(|p| p.0).sum::<f32>() / count,
        target.iter().map(|p| p.1).sum::<f32>() / count,
    );
    let mut denominator = 0.0f64;
    let mut a = 0.0f64;
    let mut b = 0.0f64;
    for (src, dst) in source.iter().zip(target.iter()) {
        let x = f64::from(src.0 - source_mean.0);
        let y = f64::from(src.1 - source_mean.1);
        let u = f64::from(dst.0 - target_mean.0);
        let v = f64::from(dst.1 - target_mean.1);
        denominator += x * x + y * y;
        a += x * u + y * v;
        b += x * v - y * u;
    }
    if denominator <= f64::EPSILON {
        return Err(fail(
            "alignment landmarks are degenerate (coincident points)".into(),
        ));
    }
    a /= denominator;
    b /= denominator;
    let norm = a * a + b * b;
    if norm <= f64::EPSILON {
        return Err(fail(
            "alignment transform is degenerate (zero scale)".into(),
        ));
    }
    // Forward: target = [[a, -b], [b, a]] * source + translation. Sample the
    // inverse: source = M⁻¹ * (target - translation).
    let tx =
        f64::from(target_mean.0) - (a * f64::from(source_mean.0) - b * f64::from(source_mean.1));
    let ty =
        f64::from(target_mean.1) - (b * f64::from(source_mean.0) + a * f64::from(source_mean.1));

    let (width, height) = (resolution.0 as usize, resolution.1 as usize);
    let (source_width, source_height) = (image.width as i64, image.height as i64);
    let mut pixels = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let u = x as f64 - tx;
            let v = y as f64 - ty;
            let sx = (a * u + b * v) / norm;
            let sy = (-b * u + a * v) / norm;
            let px = (sx.round() as i64).clamp(0, source_width - 1);
            let py = (sy.round() as i64).clamp(0, source_height - 1);
            let source_index = ((py * source_width + px) as usize) * 4;
            let target_index = (y * width + x) * 4;
            pixels[target_index..target_index + 4]
                .copy_from_slice(&image.pixels[source_index..source_index + 4]);
        }
    }
    ImageFrame::new(resolution.0, resolution.1, pixels).map_err(|error| fail(error.to_string()))
}

#[cfg(test)]
mod tests {
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
        assert!(
            decode_face_detections("M", &[1, 5], &[0.5, 0.5, 0.1, 0.1, f32::NAN], 0.0).is_err()
        );
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
        assert_eq!((aligned.width, aligned.height), (112, 112));
        assert_eq!(
            aligned.pixels, frame.pixels,
            "template landmarks on a 112×112 frame must be an identity warp"
        );

        // Deterministic across calls.
        assert_eq!(
            align_face_to_template(&frame, &landmarks, (112, 112))
                .unwrap()
                .pixels,
            aligned.pixels
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
        assert_eq!((aligned.width, aligned.height), (112, 112));

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
}
