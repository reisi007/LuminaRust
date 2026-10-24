//! LRPAR-G12-FACE-20 / FACE-20-S2 (ONNX) + S3 bridge — native face pipeline.
//!
//! SOLL: `feature/decisions/LRPAR-G12-FACE-20.md` §2 „Modell-/Lizenz-/
//! Capability-Entscheid" and §4 „Persistenz-Scope (Sidecar-first)". The face
//! pipeline has three independently versioned stages:
//!
//! 1. **Detection** (ONNX): RGB → face boxes + scores + landmarks.
//! 2. **Embedding** (ONNX): aligned face crop → normalized identity vector.
//! 3. **Clustering** (local, model-free, deterministic): vectors → clusters
//!    (see [`cluster`]).
//!
//! This module owns stages 1/2's manifest descriptors, the shared inference
//! identity mapping onto the S1 sidecar schema (`lumina-sidecar::face`) and
//! the deterministic, tests-only stub backends ([`backend`]). The real
//! ONNX Runtime path lives behind the non-default `onnx-rt` feature
//! ([`ort`]).
//!
//! ## Model decision and licences (F-078, verified — FACE-20-S6)
//!
//! Two weight-verified OpenCV Zoo models are declared: **YuNet** (detection,
//! MIT © 2020 Shiqi Yu) and **SFace/MobileFaceNet** (embedding, Apache-2.0).
//! Licence, weight grant and exact SHA-256 were verified at the source
//! (2026-09-20); the constants and provenance live in [`pins`]. The real I/O
//! contract is decoded by [`yunet`] (YuNet: raw `0..=255` **BGR**, twelve
//! per-stride tensors + NMS) and [`backend::decode_face_embedding`] (SFace:
//! raw `0..=255` **RGB** `data` → 128-d `fc1`, the graph bakes
//! `(x−127.5)·1/128`). A graph that does not match its declared contract is
//! refused loudly (`InferenceFailed` listing the available tensors) — no silent
//! re-shaping, no stub substitution. Tests run against deterministic stubs and
//! the hash-pinned crafted behavior fixtures; **no weights are committed and
//! nothing is downloaded** (Agents.md).
//!
//! ## No silent fallback
//!
//! A missing/stale/mismatched artifact or an unsupported capability is a hard
//! [`OnnxError`] (`MissingModel` / `ModelArtifactStale` / `InferenceFailed` /
//! `UnsupportedModel`) — never a silent stub substitution and never a silent
//! re-inference. Missing model artifacts are surfaced visibly
//! (`FaceArtifactStatus::Missing`); a changed model, decode context or
//! clustering version makes persisted results `Stale`
//! ([`face_artifact_status`]).
//!
//! ## Privacy / capability
//!
//! Face data (boxes, embeddings, clusters, names) stays local in sidecar
//! artifacts. There is no cloud path, no telemetry and no cross-catalogue
//! person identity: clustering is image-local (FACE-20 §2.3/§4). Person names
//! are user data and never appear in model or benchmark fixtures.

pub mod backend;
pub mod cluster;
pub mod model;
#[cfg(feature = "onnx-rt")]
pub mod ort;
#[cfg(feature = "onnx-rt")]
pub mod ort_io;
pub mod pins;
pub mod yunet;

pub use model::{
    face_detect_manifest, face_embed_manifest, face_model_hash_is_pinned, FaceInferenceOptions,
    FaceModelSuite, FACE_DETECTION_NMS_THRESHOLD_DEFAULT, FACE_DETECTION_SCORE_THRESHOLD_DEFAULT,
    FACE_DETECTION_TOP_K_DEFAULT, FACE_DETECT_INFERENCE_HEIGHT, FACE_DETECT_INFERENCE_WIDTH,
    FACE_EMBEDDING_NORMALIZATION, FACE_EMBED_DIMENSION, FACE_EMBED_INFERENCE_HEIGHT,
    FACE_EMBED_INFERENCE_WIDTH,
};

use std::collections::BTreeMap;
use std::path::Path;

use lumina_sidecar::{
    validate_face_analysis, CoordinateSystem, DecodeFingerprint, Extras, FaceAnalysis,
    FaceArtifactStatus, FaceCluster, FaceClusteringIdentity, FaceDetection, FaceEmbedding,
    FaceIdentity, FacePerson, FaceVectorRef, GeometryFingerprint, Preprocessing, SourceFingerprint,
};

/// Evidence about the persisted binary face artifacts, owned by the sidecar
/// layer (`lumina_sidecar::face_artifact_evidence`) so CLI, GUI and this
/// status mapping share exactly one contract. Re-exported here because
/// [`face_artifact_status`] consumes it.
pub use lumina_sidecar::FaceArtifactEvidence;

use crate::hash::compute_sha256_hex;
use crate::OnnxError;

pub use backend::{
    DetectedFace, FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingVector,
    StubFaceDetector, StubFaceEmbedder,
};
pub use cluster::{
    cluster_embeddings, clusters_from_labels, confirm_person, merge_clusters, split_cluster,
    FaceClusteringParams, FACE_CLUSTERING_EPS_DEFAULT, FACE_CLUSTERING_METHOD,
    FACE_CLUSTERING_MIN_SAMPLES_DEFAULT, FACE_CLUSTERING_VERSION,
};

pub use pins::{
    FACE_DETECT_LICENSE, FACE_DETECT_MODEL_HASH, FACE_DETECT_MODEL_NAME, FACE_DETECT_MODEL_VERSION,
    FACE_EMBED_LICENSE, FACE_EMBED_MODEL_HASH, FACE_EMBED_MODEL_NAME, FACE_EMBED_MODEL_VERSION,
};

/// Canonical 5-point alignment landmark order (part of the identity contract).
/// The detector emits landmarks in this order and the embedder aligns to the
/// documented 112×112 template with exactly these correspondences.
pub const FACE_LANDMARK_NAMES_5PT: [&str; 5] =
    ["left_eye", "right_eye", "nose", "mouth_left", "mouth_right"];

/// Name of the shared face preprocessing contract (landmark alignment).
pub const FACE_PREPROCESSING_NAME: &str = "face_align_5pt";
/// Version of the shared face preprocessing contract.
pub const FACE_PREPROCESSING_VERSION: &str = "1";
/// Documented rescaling method from detector coordinates to the source frame.
/// The detector emits normalized (`0..=1`) boxes in the oriented source frame,
/// so no further scaling is applied (`identity`).
pub const FACE_RESCALING_METHOD: &str = "identity";
/// Key under which the deterministic identity digest
/// ([`face_identity_digest`]) is persisted in the sidecar
/// [`FaceIdentity::extras`].
pub const FACE_IDENTITY_DIGEST_KEY: &str = "face_identity_digest";

/// Build the reproducible stage 1/2 identity for the sidecar
/// ([`lumina_sidecar::FaceIdentity`]).
///
/// The identity bundles the source/decode/geometry fingerprints, both model
/// identities (each carrying its input-spec digest via
/// [`ModelManifest::to_model_identity`]), the inference resolution and the
/// shared preprocessing contract. The clustering procedure is a separate
/// stage but part of the same persisted identity (FACE-20 §4): a clustering
/// version/threshold change is therefore visible as `stale`.
///
/// The identity does **not** carry a matrix/vector payload; only the
/// reproducible parameters. No absolute paths and no image bytes enter here.
pub fn face_identity(
    suite: &FaceModelSuite,
    source: SourceFingerprint,
    decode: DecodeFingerprint,
    geometry: GeometryFingerprint,
    clustering: FaceClusteringIdentity,
    options: &FaceInferenceOptions,
) -> Result<FaceIdentity, OnnxError> {
    suite.validate()?;
    options.validate()?;
    validate_clustering_identity(&clustering)?;

    let detection_identity = suite.detection.to_model_identity();
    let embedding_identity = suite.embedding.to_model_identity();

    let mut parameters = BTreeMap::new();
    parameters.insert(
        "detect_inference_resolution".into(),
        format!(
            "{}x{}",
            suite.detection.input.resolution.width, suite.detection.input.resolution.height
        ),
    );
    parameters.insert(
        "embed_inference_resolution".into(),
        format!(
            "{}x{}",
            suite.embedding.input.resolution.width, suite.embedding.input.resolution.height
        ),
    );
    parameters.insert("alignment".into(), FACE_PREPROCESSING_NAME.into());
    parameters.insert("landmark_names".into(), FACE_LANDMARK_NAMES_5PT.join(","));
    parameters.insert(
        "detection_score_threshold".into(),
        format!("{:.6}", options.detection_score_threshold),
    );
    parameters.insert(
        "detection_nms_threshold".into(),
        format!("{:.6}", options.detection_nms_threshold),
    );
    parameters.insert(
        "detection_top_k".into(),
        options.detection_top_k.to_string(),
    );
    parameters.insert(
        "embedding_normalization".into(),
        options.embedding_normalization.clone(),
    );
    parameters.insert(
        "embedding_dimension".into(),
        suite.embedding_dimension.to_string(),
    );

    let identity = FaceIdentity {
        source,
        decode,
        geometry,
        detection_model: detection_identity,
        embedding_model: embedding_identity,
        inference_resolution: lumina_sidecar::Resolution {
            width: suite.detection.input.resolution.width,
            height: suite.detection.input.resolution.height,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: FACE_PREPROCESSING_NAME.into(),
            version: FACE_PREPROCESSING_VERSION.into(),
            parameters,
            extras: Extras::new(),
        },
        rescaling_method: FACE_RESCALING_METHOD.into(),
        coordinate_system: CoordinateSystem::Normalized,
        clustering,
        extras: Extras::new(),
    };
    Ok(identity)
}

fn validate_clustering_identity(clustering: &FaceClusteringIdentity) -> Result<(), OnnxError> {
    if clustering.method.trim().is_empty() {
        return Err(OnnxError::InvalidFaceData(
            "face clustering.method must not be empty".into(),
        ));
    }
    if clustering.version == 0 {
        return Err(OnnxError::InvalidFaceData(
            "face clustering.version must be >= 1".into(),
        ));
    }
    for (key, value) in &clustering.parameters {
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(OnnxError::InvalidFaceData(
                "face clustering parameters must be non-empty".into(),
            ));
        }
    }
    Ok(())
}

/// Append one unambiguous, length-prefixed component to a canonical text.
///
/// The length prefix makes the encoding injective (`"ab"+"c"` can never
/// collide with `"a"+"bc"`), so the digest is collision-free for the fields we
/// control.
fn push_component(out: &mut String, value: &str) {
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
    out.push('|');
}

fn push_map(out: &mut String, map: &BTreeMap<String, String>) {
    for (key, value) in map {
        push_component(out, key);
        push_component(out, value);
    }
}

fn push_json_map(out: &mut String, map: &Extras) {
    for (key, value) in map {
        push_component(out, key);
        push_component(out, &value.to_string());
    }
}

/// Push the top-level identity extras, deliberately skipping the persisted
/// [`FACE_IDENTITY_DIGEST_KEY`].
///
/// The digest is derived from the identity *without* this entry, so covering it
/// would make the digest self-referential: `digest(identity_with_digest)` would
/// differ from the stored value and mixed use (one identity carrying the extra,
/// one not) would report a false `Stale`. Every *other* extras entry stays part
/// of the digest — the skip is scoped to exactly this one derived key.
fn push_identity_extras(out: &mut String, extras: &Extras) {
    for (key, value) in extras {
        if key == FACE_IDENTITY_DIGEST_KEY {
            continue;
        }
        push_component(out, key);
        push_component(out, &value.to_string());
    }
}

fn push_model_identity(out: &mut String, model: &lumina_sidecar::ModelIdentity) {
    push_component(out, &model.name);
    push_component(out, &model.version);
    push_component(out, &model.hash);
    push_json_map(out, &model.extras);
}

/// Canonical, versioned text form of a face identity.
///
/// Hand-encoded with fixed field order and length-prefixed components so the
/// digest is stable independent of any serializer's float formatting, and any
/// identity change (source, decode, geometry, either model incl. its
/// input-spec digest, inference resolution, preprocessing, rescaling,
/// coordinate system or clustering) flips the digest.
///
/// The persisted [`FACE_IDENTITY_DIGEST_KEY`] extra is excluded
/// ([`push_identity_extras`]) so the digest never covers itself; all other
/// top-level extras remain identity-bearing.
fn canonical_face_identity_text(identity: &FaceIdentity) -> String {
    let mut out = String::with_capacity(512);
    push_component(&mut out, "lumina-face-identity-v1");

    push_component(&mut out, &identity.source.content_hash);
    push_component(&mut out, &identity.source.byte_length.to_string());
    push_json_map(&mut out, &identity.source.extras);

    push_component(&mut out, &identity.decode.decoder);
    push_component(&mut out, &identity.decode.version);
    push_map(&mut out, &identity.decode.parameters);
    push_json_map(&mut out, &identity.decode.extras);

    push_component(&mut out, &identity.geometry.width.to_string());
    push_component(&mut out, &identity.geometry.height.to_string());
    push_component(&mut out, &identity.geometry.orientation.to_string());
    push_component(
        &mut out,
        &identity.geometry.pixel_aspect_ratio.to_bits().to_string(),
    );
    push_json_map(&mut out, &identity.geometry.extras);

    push_model_identity(&mut out, &identity.detection_model);
    push_model_identity(&mut out, &identity.embedding_model);

    push_component(&mut out, &identity.inference_resolution.width.to_string());
    push_component(&mut out, &identity.inference_resolution.height.to_string());
    push_json_map(&mut out, &identity.inference_resolution.extras);

    push_component(&mut out, &identity.preprocessing.name);
    push_component(&mut out, &identity.preprocessing.version);
    push_map(&mut out, &identity.preprocessing.parameters);
    push_json_map(&mut out, &identity.preprocessing.extras);

    push_component(&mut out, &identity.rescaling_method);
    push_component(
        &mut out,
        format!("{:?}", identity.coordinate_system).as_str(),
    );

    push_component(&mut out, &identity.clustering.method);
    push_component(&mut out, &identity.clustering.version.to_string());
    push_map(&mut out, &identity.clustering.parameters);
    push_json_map(&mut out, &identity.clustering.extras);

    push_identity_extras(&mut out, &identity.extras);
    out
}

/// Deterministic SHA-256 identity digest of a face analysis
/// (`sha256:<64 lowercase hex>`).
///
/// The digest covers the whole reproducible identity (source, decode,
/// geometry, both models, inference resolution, preprocessing, rescaling,
/// coordinate system and clustering) and every top-level extra **except** the
/// persisted [`FACE_IDENTITY_DIGEST_KEY`] itself. It is therefore stable
/// whether or not [`face_identity_with_digest`] has already stored the digest,
/// so `face_identity_digest(&identity) == identity.extras[KEY]` holds and
/// mixed use can never report a false `Stale`.
#[must_use]
pub fn face_identity_digest(identity: &FaceIdentity) -> String {
    let digest = compute_sha256_hex(canonical_face_identity_text(identity).as_bytes())
        .expect("hashing an in-memory buffer cannot fail");
    format!("sha256:{digest}")
}

/// [`face_identity`] plus the persisted [`face_identity_digest`] in the
/// identity extras.
///
/// The digest is an additive optional extras entry (no schema-version bump, no
/// migration; sidecars written before it keep parsing) and is **not** part of
/// [`face_identity_digest`] itself — [`canonical_face_identity_text`] skips the
/// [`FACE_IDENTITY_DIGEST_KEY`] entry, so writing it is idempotent:
/// `face_identity_digest(&identity_with_digest)` equals the stored value and
/// never self-references.
pub fn face_identity_with_digest(
    suite: &FaceModelSuite,
    source: SourceFingerprint,
    decode: DecodeFingerprint,
    geometry: GeometryFingerprint,
    clustering: FaceClusteringIdentity,
    options: &FaceInferenceOptions,
) -> Result<FaceIdentity, OnnxError> {
    let mut identity = face_identity(suite, source, decode, geometry, clustering, options)?;
    let digest = face_identity_digest(&identity);
    identity.extras.insert(
        FACE_IDENTITY_DIGEST_KEY.into(),
        serde_json::Value::String(digest),
    );
    Ok(identity)
}

/// Resolve the visible status of a persisted face analysis (FACE-20 §4).
///
/// Rules (loud, never a silent fallback):
///
/// | evidence | identity | status |
/// | --- | --- | --- |
/// | [`FaceArtifactEvidence::Missing`] | any | `missing` |
/// | present, checksum mismatch | any | `corrupt` |
/// | present, checksum ok | changed | `stale` |
/// | present, checksum ok | equal | `valid` |
///
/// A changed model (`model_hash`/version), decode/geometry context,
/// preprocessing or clustering version therefore makes the persisted result
/// `stale` — the consumer must re-run explicitly; nothing is recomputed
/// silently. `missing`/`corrupt` dominate an identity change because they
/// describe an unusable artifact first.
#[must_use]
pub fn face_artifact_status(
    current: &FaceIdentity,
    persisted: &FaceIdentity,
    evidence: FaceArtifactEvidence,
) -> FaceArtifactStatus {
    match evidence {
        FaceArtifactEvidence::Missing => FaceArtifactStatus::Missing,
        FaceArtifactEvidence::Present {
            checksum_matches: false,
        } => FaceArtifactStatus::Corrupt,
        FaceArtifactEvidence::Present {
            checksum_matches: true,
        } => {
            if face_identity_digest(current) == face_identity_digest(persisted) {
                FaceArtifactStatus::Valid
            } else {
                FaceArtifactStatus::Stale
            }
        }
    }
}

/// A resolvable real face engine — or the explicit statement that this build
/// cannot provide one.
///
/// Mirrors [`crate::resolve::OnnxEngine`] for the face pipeline so a CLI/core
/// caller can obtain the real detector + embedder without a silent fallback.
pub enum FaceOnnxEngine {
    /// Real, artifact-verified ONNX Runtime face backends. Only exists when the
    /// `onnx-rt` feature is compiled in.
    #[cfg(feature = "onnx-rt")]
    OnnxRuntime {
        /// Verified detection backend.
        detector: Box<ort::OrtFaceDetector>,
        /// Verified embedding backend.
        embedder: Box<ort::OrtFaceEmbedder>,
    },
    /// The `onnx-rt` capability is not compiled into this build. A deliberate,
    /// visible state — never a silent stub fallback.
    RuntimeDisabled,
}

impl std::fmt::Debug for FaceOnnxEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "onnx-rt")]
            FaceOnnxEngine::OnnxRuntime { .. } => f.write_str("OnnxRuntime(<real face engine>)"),
            FaceOnnxEngine::RuntimeDisabled => f.write_str("RuntimeDisabled"),
        }
    }
}

/// Attempt to load the real face engine (detector + embedder) for `suite`.
///
/// # Contract (no silent fallback)
///
/// | situation | result |
/// | --- | --- |
/// | `onnx-rt` **off** | `Ok(FaceOnnxEngine::RuntimeDisabled)` |
/// | `onnx-rt` **on**, both artifacts present and tensor-verified | `Ok(FaceOnnxEngine::OnnxRuntime { .. })` |
/// | `onnx-rt` **on**, artifact missing/unreadable | `Err(OnnxError::MissingModel)` |
/// | `onnx-rt` **on**, digest ≠ pinned hash | loads, then refuses inference with `ModelArtifactStale` |
/// | `onnx-rt` **on**, declared tensor name absent | `Err(OnnxError::InferenceFailed)` |
#[cfg_attr(not(feature = "onnx-rt"), allow(unused_variables))]
pub fn try_load_face_engine(
    detector_path: &Path,
    suite: &FaceModelSuite,
    embedder_path: &Path,
    options: &FaceInferenceOptions,
) -> Result<FaceOnnxEngine, OnnxError> {
    suite.validate()?;
    options.validate()?;
    #[cfg(feature = "onnx-rt")]
    {
        let detector =
            ort::OrtFaceDetector::new(detector_path, suite.detection.clone(), options.clone())?;
        let embedder = ort::OrtFaceEmbedder::new(
            embedder_path,
            suite.embedding.clone(),
            suite.embedding_dimension,
        )?;
        Ok(FaceOnnxEngine::OnnxRuntime {
            detector: Box::new(detector),
            embedder: Box::new(embedder),
        })
    }
    #[cfg(not(feature = "onnx-rt"))]
    {
        Ok(FaceOnnxEngine::RuntimeDisabled)
    }
}

// ---------------------------------------------------------------------------
// Stable IDs and sidecar bridging
// ---------------------------------------------------------------------------
fn content_id(prefix: &str, canonical: &str) -> String {
    let digest =
        compute_sha256_hex(canonical.as_bytes()).expect("hashing an in-memory buffer cannot fail");
    format!("{prefix}-{}", &digest[..32])
}

/// Stable, content-derived sidecar id of a detection.
///
/// The id is derived from the box, score and landmarks — never from an array
/// position (FACE-20 §4 / S1 contract), so reordering detections keeps their
/// identities.
#[must_use]
pub fn detected_face_id(face: &DetectedFace) -> String {
    let mut canonical = String::new();
    push_component(&mut canonical, &face.bbox.x.to_bits().to_string());
    push_component(&mut canonical, &face.bbox.y.to_bits().to_string());
    push_component(&mut canonical, &face.bbox.width.to_bits().to_string());
    push_component(&mut canonical, &face.bbox.height.to_bits().to_string());
    push_component(&mut canonical, &face.score.to_bits().to_string());
    for landmark in &face.landmarks {
        push_component(&mut canonical, &landmark.name);
        push_component(&mut canonical, &landmark.x.to_bits().to_string());
        push_component(&mut canonical, &landmark.y.to_bits().to_string());
    }
    content_id("face", &canonical)
}

/// Stable sidecar id of the embedding belonging to `detection_id`.
#[must_use]
pub fn embedding_id_for(detection_id: &str) -> String {
    content_id("emb", detection_id)
}

/// A produced embedding together with the portable binary reference the
/// persistence layer will record for it.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEmbeddingRecord {
    /// Index into the detection list this embedding belongs to.
    pub detection_index: usize,
    /// The normalized identity vector (used for clustering; never inlined in
    /// the JSON sidecar).
    pub vector: FaceEmbeddingVector,
    /// Portable reference (relative path + checksum + dimension), written by
    /// the persistence layer.
    pub reference: FaceVectorRef,
}

/// Complete stage-1/2/3 output ready to be persisted as a sidecar
/// [`FaceAnalysis`].
#[derive(Debug, Clone, PartialEq)]
pub struct FaceAnalysisOutput {
    /// Reproducible identity (source/decode/geometry + both models +
    /// preprocessing + clustering).
    pub identity: FaceIdentity,
    /// Timestamp persisted as `created_at`.
    pub created_at: String,
    /// Detected faces.
    pub detections: Vec<DetectedFace>,
    /// Embeddings with their binary references.
    pub embeddings: Vec<FaceEmbeddingRecord>,
    /// Clusters produced by [`cluster`] (or user ops).
    pub clusters: Vec<FaceCluster>,
    /// Person labels (image-local, stable ids).
    pub persons: Vec<FacePerson>,
}

impl FaceAnalysisOutput {
    /// Convert to the sidecar [`FaceAnalysis`], assigning content-derived
    /// stable ids and reciprocal detection↔embedding links.
    ///
    /// The result is validated with the S1 schema validator before it is
    /// returned: an inconsistent cross-reference is a loud
    /// [`OnnxError::InvalidFaceData`], never silently repaired.
    pub fn into_sidecar(self) -> Result<FaceAnalysis, OnnxError> {
        let detection_ids: Vec<String> = self.detections.iter().map(detected_face_id).collect();

        let mut embeddings = Vec::with_capacity(self.embeddings.len());
        let mut embedding_by_detection: BTreeMap<usize, String> = BTreeMap::new();
        for record in &self.embeddings {
            let Some(detection_id) = detection_ids.get(record.detection_index) else {
                return Err(OnnxError::InvalidFaceData(format!(
                    "face embedding references detection index {} out of range",
                    record.detection_index
                )));
            };
            if !record.vector.is_normalized() {
                return Err(OnnxError::InvalidFaceData(format!(
                    "face embedding for detection `{detection_id}` is not unit-normalized"
                )));
            }
            if record.vector.dimension() != record.reference.dimension as usize {
                return Err(OnnxError::InvalidFaceData(format!(
                    "face embedding for detection `{detection_id}` has {} values but its reference \
                     declares dimension {}",
                    record.vector.dimension(),
                    record.reference.dimension
                )));
            }
            let embedding_id = embedding_id_for(detection_id);
            if embedding_by_detection
                .insert(record.detection_index, embedding_id.clone())
                .is_some()
            {
                return Err(OnnxError::InvalidFaceData(format!(
                    "detection `{detection_id}` has more than one embedding"
                )));
            }
            embeddings.push(FaceEmbedding {
                id: embedding_id,
                detection_id: detection_id.clone(),
                vector: record.reference.clone(),
                status: FaceArtifactStatus::Valid,
                error: None,
                extras: Extras::new(),
            });
        }

        let mut detections = Vec::with_capacity(self.detections.len());
        for (index, face) in self.detections.iter().enumerate() {
            face.validate()?;
            detections.push(FaceDetection {
                id: detection_ids[index].clone(),
                bbox: face.bbox,
                score: face.score,
                landmarks: face.landmarks.clone(),
                embedding_id: embedding_by_detection.get(&index).cloned(),
                matte: None,
                status: FaceArtifactStatus::Valid,
                error: None,
                extras: Extras::new(),
            });
        }

        let analysis = FaceAnalysis {
            version: lumina_sidecar::FACE_SCHEMA_VERSION,
            identity: self.identity,
            detections,
            embeddings,
            clusters: self.clusters,
            persons: self.persons,
            created_at: self.created_at,
            status: FaceArtifactStatus::Valid,
            error: None,
            extras: Extras::new(),
        };
        validate_face_analysis(&analysis).map_err(|error| {
            OnnxError::InvalidFaceData(format!("invalid face analysis: {error}"))
        })?;
        Ok(analysis)
    }
}

#[cfg(test)]
mod tests;
