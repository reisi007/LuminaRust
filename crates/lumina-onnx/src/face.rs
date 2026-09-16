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
//! ## Model decision and licences (F-078, **candidate/proposal — pending S6**)
//!
//! Two **candidate** ONNX models from the OpenCV Zoo are declared here; the
//! exact variant *and* the licence/weight grant are still to be verified at
//! weight-integration time (benchmark + licence gate, FACE-20 §2.1/§2.2, S6 in
//! §6, tracked as `FACE-20-S6` in `feature/quality/fixtures-licensing.md` §5):
//!
//! | Stage | Model | Declared licence (candidate) | Status |
//! | --- | --- | --- | --- |
//! | detection | `YuNet` (`face_detection_yunet`) | **MIT** (OpenCV Zoo model-dir `LICENSE`) | planned, `model_hash = "pending-integration"` |
//! | embedding | `SFace` (`face_recognition_sface`, MobileFaceNet) | **Apache-2.0** (OpenCV Zoo model-dir `LICENSE`) | planned, `model_hash = "pending-integration"` |
//!
//! The model-directory `LICENSE` covers the OpenCV Zoo *code*; it is **not** by
//! itself a grant for the model **weights**, so the weight licence and the
//! exact release/commit must be verified against the actual weight source
//! before any hash pin lands (S6). Both are expected to be OSI-permissive; the
//! known trap is the non-commercial InsightFace/ArcFace weight licence and the
//! AGPL `ultralytics` tooling — see `feature/quality/fixtures-licensing.md` §5.
//! **No weights are committed and nothing is downloaded** (Agents.md); until
//! hash-pinned weights land, every planned descriptor keeps the documented
//! [`PENDING_INTEGRATION_HASH`] placeholder and can never report `Verified`
//! (FACE-20 §2.2). Tests run exclusively against deterministic stubs or a
//! locally committed behavior fixture.
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
#[cfg(feature = "onnx-rt")]
pub mod ort;

use std::collections::BTreeMap;
use std::path::Path;

use lumina_sidecar::{
    validate_face_analysis, CoordinateSystem, DecodeFingerprint, Extras, FaceAnalysis,
    FaceArtifactStatus, FaceCluster, FaceClusteringIdentity, FaceDetection, FaceEmbedding,
    FaceIdentity, FacePerson, FaceVectorRef, GeometryFingerprint, Preprocessing, SourceFingerprint,
};

use crate::hash::{compute_sha256_hex, PENDING_INTEGRATION_HASH};
use crate::manifest::{
    ChannelLayout, InputNormalization, ModelCapabilities, ModelInputSpec, ModelManifest,
    Resolution as ModelResolution, TensorFormat,
};
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

/// Detection inference resolution (square). Planned YuNet input; part of the
/// face identity.
pub const FACE_DETECT_INFERENCE_WIDTH: u32 = 640;
/// Detection inference resolution (square). See [`FACE_DETECT_INFERENCE_WIDTH`].
pub const FACE_DETECT_INFERENCE_HEIGHT: u32 = 640;
/// Embedding inference resolution (square, `112×112` is the standard aligned
/// face crop size of the ArcFace/MobileFaceNet family). Part of the shared
/// preprocessing identity.
pub const FACE_EMBED_INFERENCE_WIDTH: u32 = 112;
/// Embedding inference resolution (square). See [`FACE_EMBED_INFERENCE_WIDTH`].
pub const FACE_EMBED_INFERENCE_HEIGHT: u32 = 112;
/// Planned embedding vector length (SFace/MobileFaceNet output). Part of the
/// persisted [`FaceVectorRef::dimension`]; a change requires re-clustering.
pub const FACE_EMBED_DIMENSION: u32 = 128;

/// Planned detection model name (OpenCV Zoo `face_detection_yunet`, MIT).
pub const FACE_DETECT_MODEL_NAME: &str = "YuNet";
/// Planned detection model version (OpenCV Zoo release tag).
pub const FACE_DETECT_MODEL_VERSION: &str = "2023mar";
/// Declared detection licence **candidate** (OpenCV Zoo model-dir `LICENSE`);
/// the weight grant is verified in FACE-20-S6, not here (F-078).
pub const FACE_DETECT_LICENSE: &str = "MIT";
/// Planned embedding model name (OpenCV Zoo `face_recognition_sface`).
pub const FACE_EMBED_MODEL_NAME: &str = "SFace";
/// Planned embedding model version (OpenCV Zoo release tag).
pub const FACE_EMBED_MODEL_VERSION: &str = "2021dec";
/// Declared embedding licence **candidate** (OpenCV Zoo model-dir `LICENSE`);
/// the weight grant is verified in FACE-20-S6, not here (F-078).
pub const FACE_EMBED_LICENSE: &str = "Apache-2.0";

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
/// Default detection score threshold applied when building persisted
/// detections. It is part of the preprocessing identity, so changing it makes
/// previously persisted analyses `stale` (never a silent re-filter).
pub const FACE_DETECTION_SCORE_THRESHOLD_DEFAULT: f32 = 0.5;
/// Documented embedding normalization applied before clustering and
/// persistence.
pub const FACE_EMBEDDING_NORMALIZATION: &str = "l2";
/// Key under which the deterministic identity digest
/// ([`face_identity_digest`]) is persisted in the sidecar
/// [`FaceIdentity::extras`].
pub const FACE_IDENTITY_DIGEST_KEY: &str = "face_identity_digest";

/// Whether `manifest` carries a real (non-placeholder) `model_hash`.
///
/// FACE-20 §2.2: until hash-pinned weights are committed every face manifest
/// carries [`PENDING_INTEGRATION_HASH`] and must never be reported as verified.
#[must_use]
pub fn face_model_hash_is_pinned(manifest: &ModelManifest) -> bool {
    manifest.model_hash != PENDING_INTEGRATION_HASH
}

/// Build the planned detection descriptor (YuNet, MIT candidate,
/// `pending-integration`).
///
/// Declares only `face_detect`. The tensor names are the planned contract and
/// are confirmed at weight-integration time; while the hash stays
/// `pending-integration` the descriptor is a declaration, not a verified
/// identity.
#[must_use]
pub fn face_detect_manifest() -> ModelManifest {
    ModelManifest {
        model_name: FACE_DETECT_MODEL_NAME.into(),
        model_version: FACE_DETECT_MODEL_VERSION.into(),
        model_hash: PENDING_INTEGRATION_HASH.into(),
        license: FACE_DETECT_LICENSE.into(),
        input: ModelInputSpec {
            resolution: ModelResolution {
                width: FACE_DETECT_INFERENCE_WIDTH,
                height: FACE_DETECT_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "input".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: "detections".into(),
        capabilities: ModelCapabilities {
            face_detect: true,
            ..Default::default()
        },
    }
}

/// Build the planned embedding descriptor (SFace/MobileFaceNet, Apache-2.0
/// candidate, `pending-integration`).
///
/// Declares only `face_embed`.
#[must_use]
pub fn face_embed_manifest() -> ModelManifest {
    ModelManifest {
        model_name: FACE_EMBED_MODEL_NAME.into(),
        model_version: FACE_EMBED_MODEL_VERSION.into(),
        model_hash: PENDING_INTEGRATION_HASH.into(),
        license: FACE_EMBED_LICENSE.into(),
        input: ModelInputSpec {
            resolution: ModelResolution {
                width: FACE_EMBED_INFERENCE_WIDTH,
                height: FACE_EMBED_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "input".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: "output".into(),
        capabilities: ModelCapabilities {
            face_embed: true,
            ..Default::default()
        },
    }
}

/// The pair of face models plus the embedding dimension that fully describes
/// stages 1/2 of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceModelSuite {
    /// Detection model descriptor (must declare `face_detect`).
    pub detection: ModelManifest,
    /// Embedding model descriptor (must declare `face_embed`).
    pub embedding: ModelManifest,
    /// Embedding vector length (`> 0`), persisted in the vector reference.
    pub embedding_dimension: u32,
}

impl FaceModelSuite {
    /// Build a suite from explicit descriptors, validating the capability and
    /// dimension contract loudly (no silent capability guessing).
    pub fn new(
        detection: ModelManifest,
        embedding: ModelManifest,
        embedding_dimension: u32,
    ) -> Result<Self, OnnxError> {
        let suite = Self {
            detection,
            embedding,
            embedding_dimension,
        };
        suite.validate()?;
        Ok(suite)
    }

    /// The planned descriptor pair (YuNet MIT + SFace Apache-2.0, both
    /// `pending-integration`).
    #[must_use]
    pub fn candidate() -> Self {
        Self {
            detection: face_detect_manifest(),
            embedding: face_embed_manifest(),
            embedding_dimension: FACE_EMBED_DIMENSION,
        }
    }

    /// Validate both manifests and the declared capabilities/dimension.
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.detection.validate()?;
        self.embedding.validate()?;
        if !self.detection.capabilities.face_detect {
            return Err(OnnxError::UnsupportedModel {
                name: self.detection.model_name.clone(),
                reason: "detection model does not declare the `face_detect` capability".into(),
            });
        }
        if !self.embedding.capabilities.face_embed {
            return Err(OnnxError::UnsupportedModel {
                name: self.embedding.model_name.clone(),
                reason: "embedding model does not declare the `face_embed` capability".into(),
            });
        }
        if self.embedding_dimension == 0 {
            return Err(OnnxError::InvalidFaceData(
                "face embedding_dimension must be > 0".into(),
            ));
        }
        Ok(())
    }
}

/// Identity-bearing inference options (part of the persisted preprocessing
/// contract, so a change invalidates persisted analyses).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceInferenceOptions {
    /// Detection score threshold applied when persisting detections.
    pub detection_score_threshold: f32,
    /// Embedding normalization applied before clustering/persistence.
    pub embedding_normalization: String,
}

impl Default for FaceInferenceOptions {
    fn default() -> Self {
        Self {
            detection_score_threshold: FACE_DETECTION_SCORE_THRESHOLD_DEFAULT,
            embedding_normalization: FACE_EMBEDDING_NORMALIZATION.into(),
        }
    }
}

impl FaceInferenceOptions {
    /// Validate the option contract loudly (finite threshold in `0..=1`,
    /// non-empty normalization name).
    pub fn validate(&self) -> Result<(), OnnxError> {
        if !self.detection_score_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.detection_score_threshold)
        {
            return Err(OnnxError::InvalidFaceData(format!(
                "face detection_score_threshold must be finite within 0..=1, got {}",
                self.detection_score_threshold
            )));
        }
        if self.embedding_normalization.trim().is_empty() {
            return Err(OnnxError::InvalidFaceData(
                "face embedding_normalization must not be empty".into(),
            ));
        }
        Ok(())
    }
}

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

/// Evidence about the persisted binary face artifact backing an analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceArtifactEvidence {
    /// The referenced artifact does not exist.
    Missing,
    /// The artifact exists; `checksum_matches` reports whether its digest
    /// equals the persisted checksum.
    Present {
        /// Whether the on-disk payload hashes to the persisted checksum.
        checksum_matches: bool,
    },
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
mod tests {
    use super::*;
    use crate::face::cluster::FaceClusteringParams;

    fn fingerprints() -> (SourceFingerprint, DecodeFingerprint, GeometryFingerprint) {
        (
            SourceFingerprint {
                content_hash: "blake3:abc".into(),
                byte_length: 42,
                extras: Extras::new(),
            },
            DecodeFingerprint {
                decoder: "libraw".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            GeometryFingerprint {
                width: 6000,
                height: 4000,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
        )
    }

    fn identity() -> FaceIdentity {
        let (source, decode, geometry) = fingerprints();
        face_identity(
            &FaceModelSuite::candidate(),
            source,
            decode,
            geometry,
            FaceClusteringParams::default().to_identity(),
            &FaceInferenceOptions::default(),
        )
        .unwrap()
    }

    fn face() -> DetectedFace {
        DetectedFace {
            bbox: lumina_sidecar::FaceBoundingBox {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.4,
            },
            score: 0.9,
            landmarks: vec![lumina_sidecar::FaceLandmark {
                name: "left_eye".into(),
                x: 0.2,
                y: 0.3,
            }],
        }
    }

    #[test]
    fn planned_descriptors_declare_exactly_their_capability_and_stay_pending() {
        let detect = face_detect_manifest();
        assert!(detect.capabilities.face_detect);
        assert!(!detect.capabilities.face_embed);
        assert!(!detect.capabilities.subject_segmentation);
        assert!(!detect.capabilities.box_prompt);
        assert!(!detect.capabilities.point_prompt);
        assert!(!detect.capabilities.mask_prompt);
        assert!(!detect.capabilities.class_detection);
        assert!(!detect.capabilities.instance_segmentation);
        assert!(!detect.capabilities.inpaint_heal);
        assert!(!detect.capabilities.outpaint);
        assert_eq!(detect.model_hash, PENDING_INTEGRATION_HASH);
        assert!(!face_model_hash_is_pinned(&detect));
        assert_eq!(detect.license, FACE_DETECT_LICENSE);

        let embed = face_embed_manifest();
        assert!(embed.capabilities.face_embed);
        assert!(!embed.capabilities.face_detect);
        assert_eq!(embed.model_hash, PENDING_INTEGRATION_HASH);
        assert!(!face_model_hash_is_pinned(&embed));
        assert_eq!(embed.license, FACE_EMBED_LICENSE);

        // Both descriptors are internally consistent.
        assert!(detect.validate().is_ok());
        assert!(embed.validate().is_ok());
        assert!(FaceModelSuite::candidate().validate().is_ok());
    }

    #[test]
    fn suite_rejects_swapped_capabilities() {
        let err =
            FaceModelSuite::new(face_embed_manifest(), face_detect_manifest(), 128).unwrap_err();
        assert!(matches!(err, OnnxError::UnsupportedModel { .. }), "{err:?}");
    }

    #[test]
    fn suite_rejects_zero_dimension() {
        let err =
            FaceModelSuite::new(face_detect_manifest(), face_embed_manifest(), 0).unwrap_err();
        assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
    }

    #[test]
    fn identity_is_deterministic_and_digest_is_stable() {
        let a = identity();
        let b = identity();
        assert_eq!(a, b);
        assert_eq!(face_identity_digest(&a), face_identity_digest(&b));
        assert!(face_identity_digest(&a).starts_with("sha256:"));
        assert_eq!(face_identity_digest(&a).len(), "sha256:".len() + 64);
    }

    #[test]
    fn identity_digest_flips_on_model_change() {
        let base = identity();
        let mut changed = base.clone();
        changed.detection_model.hash = format!("sha256:{}", "11".repeat(32));
        assert_ne!(face_identity_digest(&base), face_identity_digest(&changed));
    }

    #[test]
    fn identity_digest_flips_on_clustering_change() {
        let base = identity();
        let mut changed = base.clone();
        changed.clustering.version += 1;
        assert_ne!(face_identity_digest(&base), face_identity_digest(&changed));
    }

    #[test]
    fn identity_digest_flips_on_threshold_change() {
        let (source, decode, geometry) = fingerprints();
        let suite = FaceModelSuite::candidate();
        let params = FaceClusteringParams::default().to_identity();
        let a = face_identity(
            &suite,
            source.clone(),
            decode.clone(),
            geometry.clone(),
            params.clone(),
            &FaceInferenceOptions::default(),
        )
        .unwrap();
        let b = face_identity(
            &suite,
            source,
            decode,
            geometry,
            params,
            &FaceInferenceOptions {
                detection_score_threshold: 0.75,
                ..FaceInferenceOptions::default()
            },
        )
        .unwrap();
        assert_ne!(
            face_identity_digest(&a),
            face_identity_digest(&b),
            "score threshold is part of the persisted identity"
        );
    }

    /// F1 (digest contract): the persisted digest extra is not part of the
    /// digest, so the stored value equals a recomputation over the *same*
    /// identity (no manual extra-strip needed) and adding the extra does not
    /// change the digest. All *other* extras stay identity-bearing.
    #[test]
    fn persisted_digest_matches_recomputation_and_ignores_only_itself() {
        let (source, decode, geometry) = fingerprints();
        let with_digest = face_identity_with_digest(
            &FaceModelSuite::candidate(),
            source,
            decode,
            geometry,
            FaceClusteringParams::default().to_identity(),
            &FaceInferenceOptions::default(),
        )
        .unwrap();
        let stored = with_digest.extras.get(FACE_IDENTITY_DIGEST_KEY).unwrap();
        let serde_json::Value::String(stored) = stored else {
            panic!("digest extra must be a string");
        };
        // Invariant: stored digest == digest(&with_digest), no stripping.
        assert_eq!(&face_identity_digest(&with_digest), stored);
        assert_eq!(
            face_identity_digest(&with_digest),
            face_identity_digest(&identity()),
            "the digest extra must not change the digest"
        );

        // Any other top-level extra still flips the digest (the skip is scoped).
        let mut extra = identity();
        extra
            .extras
            .insert("note".into(), serde_json::Value::String("x".into()));
        assert_ne!(
            face_identity_digest(&identity()),
            face_identity_digest(&extra)
        );
    }

    /// F1 (mixed use): an identity carrying the persisted digest and one
    /// without it describe the same analysis and must never report a false
    /// `Stale` in either direction.
    #[test]
    fn mixed_digest_presence_never_reports_false_stale() {
        let with = {
            let (source, decode, geometry) = fingerprints();
            face_identity_with_digest(
                &FaceModelSuite::candidate(),
                source,
                decode,
                geometry,
                FaceClusteringParams::default().to_identity(),
                &FaceInferenceOptions::default(),
            )
            .unwrap()
        };
        let without = identity();
        let evidence = FaceArtifactEvidence::Present {
            checksum_matches: true,
        };
        assert_eq!(
            face_artifact_status(&with, &without, evidence),
            FaceArtifactStatus::Valid
        );
        assert_eq!(
            face_artifact_status(&without, &with, evidence),
            FaceArtifactStatus::Valid
        );

        // A genuine identity change is still `Stale` even with digests present.
        let mut changed = with.clone();
        changed.embedding_model.version = "9".into();
        assert_eq!(
            face_artifact_status(&changed, &with, evidence),
            FaceArtifactStatus::Stale
        );
    }

    #[test]
    fn artifact_status_covers_valid_stale_missing_corrupt() {
        let current = identity();
        assert_eq!(
            face_artifact_status(
                &current,
                &current,
                FaceArtifactEvidence::Present {
                    checksum_matches: true
                }
            ),
            FaceArtifactStatus::Valid
        );

        let mut changed = current.clone();
        changed.embedding_model.version = "9".into();
        assert_eq!(
            face_artifact_status(
                &current,
                &changed,
                FaceArtifactEvidence::Present {
                    checksum_matches: true
                }
            ),
            FaceArtifactStatus::Stale
        );

        assert_eq!(
            face_artifact_status(&current, &current, FaceArtifactEvidence::Missing),
            FaceArtifactStatus::Missing
        );
        assert_eq!(
            face_artifact_status(
                &current,
                &current,
                FaceArtifactEvidence::Present {
                    checksum_matches: false
                }
            ),
            FaceArtifactStatus::Corrupt
        );
    }

    #[test]
    fn detected_face_id_is_content_derived_and_stable() {
        let face = face();
        let id = detected_face_id(&face);
        assert!(id.starts_with("face-"));
        assert_eq!(id, detected_face_id(&face.clone()));
        let mut other = face.clone();
        other.score = 0.5;
        assert_ne!(id, detected_face_id(&other));
    }

    #[test]
    fn output_bridges_to_sidecar_and_validates() {
        let face = face();
        let vector = FaceEmbeddingVector::new(vec![0.6, 0.8]).unwrap();
        let record = FaceEmbeddingRecord {
            detection_index: 0,
            vector,
            reference: FaceVectorRef {
                relative_path: "IMAGE.ARW.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: "aa".repeat(32),
                dimension: 2,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            },
        };
        let detection_id = detected_face_id(&face);
        let analysis = FaceAnalysisOutput {
            identity: identity(),
            created_at: "2026-09-16T08:00:00Z".into(),
            detections: vec![face],
            embeddings: vec![record],
            clusters: vec![FaceCluster {
                id: "cluster-1".into(),
                detection_ids: vec![detection_id.clone()],
                extras: Extras::new(),
            }],
            persons: vec![FacePerson {
                id: "person-1".into(),
                name: "Example".into(),
                confirmed: true,
                cluster_ids: vec!["cluster-1".into()],
                extras: Extras::new(),
            }],
        }
        .into_sidecar()
        .unwrap();

        assert_eq!(analysis.version, lumina_sidecar::FACE_SCHEMA_VERSION);
        assert_eq!(analysis.detections[0].id, detection_id);
        assert_eq!(
            analysis.detections[0].embedding_id.as_deref(),
            Some(embedding_id_for(&detection_id).as_str())
        );
        assert_eq!(analysis.embeddings[0].detection_id, detection_id);
    }

    #[test]
    fn output_rejects_non_normalized_embedding() {
        let record = FaceEmbeddingRecord {
            detection_index: 0,
            vector: FaceEmbeddingVector::new(vec![1.0, 1.0]).unwrap(),
            reference: FaceVectorRef {
                relative_path: "IMAGE.ARW.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: "aa".repeat(32),
                dimension: 2,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            },
        };
        let err = FaceAnalysisOutput {
            identity: identity(),
            created_at: "2026-09-16T08:00:00Z".into(),
            detections: vec![face()],
            embeddings: vec![record],
            clusters: vec![],
            persons: vec![],
        }
        .into_sidecar()
        .unwrap_err();
        assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
    }

    /// F3: the persisted [`FaceVectorRef::dimension`] must equal the actual
    /// (normalized) vector length — a 2-value vector with a `512` reference
    /// is a loud `InvalidFaceData`, never silently persisted.
    #[test]
    fn output_rejects_reference_dimension_mismatch() {
        let record = FaceEmbeddingRecord {
            detection_index: 0,
            // Two-dimensional and unit-normalized, so only the dimension check
            // can reject it.
            vector: FaceEmbeddingVector::new(vec![0.6, 0.8]).unwrap(),
            reference: FaceVectorRef {
                relative_path: "IMAGE.ARW.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: "aa".repeat(32),
                dimension: 512,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            },
        };
        let err = FaceAnalysisOutput {
            identity: identity(),
            created_at: "2026-09-16T08:00:00Z".into(),
            detections: vec![face()],
            embeddings: vec![record],
            clusters: vec![],
            persons: vec![],
        }
        .into_sidecar()
        .unwrap_err();
        assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
    }
}
