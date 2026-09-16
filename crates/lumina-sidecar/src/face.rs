//! LRPAR-G12-FACE-20 / FACE-20-S1: source-level face-detection schema.
//!
//! SOLL: `feature/decisions/LRPAR-G12-FACE-20.md` §4 „Persistenz-Scope
//! (Sidecar-first)" plus §6.1 (`FACE-20-S1 Schema`). Face data is
//! **source-level** (shared by every virtual copy, like mask artefacts):
//! detections (boxes/landmarks/scores), embeddings and cluster assignments
//! describe the same pixels for every copy. Person labels carry stable,
//! sidecar-unique IDs — never an array position. Mask layers, inversion and
//! local adjustments derived from a face region remain per virtual copy and
//! are handled by the existing mask schema.
//!
//! Binary payloads (embedding vectors, face mattes) are never inlined in the
//! JSON: they are referenced with a portable relative path, format, checksum,
//! resolution/channel type and data version, and live in the binary sidecar
//! (`<original>.lumina.zdata`). See `feature/architecture/sidecar.md`
//! § Persistenzregeln and `Agents.md` § Persistenz und Sidecars.
//!
//! Scope: schema + validation + roundtrip only. Detection/embedding models,
//! clustering evaluation, CLI and GUI are later slices (FACE-20-S2 … S6).
//!
//! Failure policy (Agents.md): every deviation from the contract is rejected
//! loudly — never silently clamped, defaulted, deduplicated or reinterpreted.
//! Pre-MVP: additive schema field, `schema_version` stays 2; an absent `face`
//! section is the valid legacy state ("no face analysis"), not an error.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    invalid, validate_artifact, validate_metadata_timestamp, validate_name, validate_relative_path,
    ArtifactReference, CoordinateSystem, DecodeFingerprint, Extras, GeometryFingerprint,
    ModelIdentity, Preprocessing, Resolution, SidecarError, SourceFingerprint,
};

/// Current face-analysis schema version. Foreign versions are rejected loudly
/// (pre-MVP: no back-compat obligation, but always versioned).
pub const FACE_SCHEMA_VERSION: u8 = 1;

/// F-078 gate marker: until licence-checked, hash-pinned weights are
/// committed, a face model manifest carries `pending-integration` instead of a
/// `sha256:<hex>` pin (FACE-20 §2.2). It is *not* verifiable and must never be
/// reported as verified — but it is a legal, explicit manifest state.
pub const FACE_PENDING_MODEL_HASH: &str = "pending-integration";
/// Prefix of the SHA-256 hash contract (`sha256:<64 lowercase hex>`).
pub const FACE_SHA256_PREFIX: &str = "sha256:";
/// Hex length of a SHA-256 digest.
pub const FACE_HASH_HEX_LEN: usize = 64;

/// Bounds against hostile/degenerate documents. Legitimate single-image face
/// analyses stay far below these caps.
pub const MAX_FACE_DETECTIONS: usize = 10_000;
pub const MAX_FACE_EMBEDDINGS: usize = 10_000;
pub const MAX_FACE_CLUSTERS: usize = 1_000;
pub const MAX_FACE_PERSONS: usize = 1_000;
pub const MAX_FACE_LANDMARKS: usize = 128;
pub const MAX_FACE_CLUSTER_MEMBERS: usize = 10_000;
pub const MAX_FACE_ID_CHARS: usize = 128;
pub const MAX_FACE_NAME_CHARS: usize = 256;
pub const MAX_FACE_ERROR_CHARS: usize = 1024;

/// Status of a persisted face artifact (per analysis and per detection/
/// embedding). `stale`, `missing` and `corrupt` are visible states; there is
/// no silent re-inference as the only option and no silent fallback to a
/// different model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaceArtifactStatus {
    Valid,
    Stale,
    Missing,
    Corrupt,
}

/// Normalized face bounding box (`0..=1` relative to the oriented source
/// frame; `x`/`y` is the top-left corner, `width`/`height` are strictly
/// positive and stay inside the frame).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceBoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One named landmark in normalized source coordinates (`0..=1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceLandmark {
    pub name: String,
    pub x: f32,
    pub y: f32,
}

/// Declarative clustering identity: the backend-free, deterministic procedure
/// plus its versioned thresholds. A change here invalidates every persisted
/// cluster (visible `stale`), never a silent re-clustering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceClusteringIdentity {
    pub method: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Binary reference to a face payload (embedding vector or matte) stored in
/// the sidecar's `.lumina.zdata` bundle.
///
/// A vector is one-dimensional: `dimension` is its length (the "resolution"
/// of a 1-D payload), `channels` is the element type (e.g. `f32`/`f16`). A
/// matte uses [`ArtifactReference`] instead. Both are portable: relative path
/// only, absolute paths are forbidden, and the digest/channel/data-version
/// metadata makes the payload identity explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceVectorRef {
    pub relative_path: String,
    pub format: String,
    pub checksum: String,
    /// Vector length (must be `> 0`).
    pub dimension: u32,
    pub channels: String,
    pub data_version: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// One detected face. `id` is a stable, sidecar-unique identity (never an
/// array position); `embedding_id` links to the matching [`FaceEmbedding`],
/// `matte` references the binary face matte once it was produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceDetection {
    pub id: String,
    pub bbox: FaceBoundingBox,
    pub score: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub landmarks: Vec<FaceLandmark>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matte: Option<ArtifactReference>,
    pub status: FaceArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Identity vector of one detected face (binary reference only — the vector
/// itself never appears in the JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceEmbedding {
    pub id: String,
    pub detection_id: String,
    pub vector: FaceVectorRef,
    pub status: FaceArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// A deterministic cluster of detections (same pixels ⇒ same cluster for
/// every virtual copy). Membership is expressed through stable detection IDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceCluster {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detection_ids: Vec<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// A source-level person label. `id` is stable and sidecar-unique; the
/// display name never doubles as identity, so renaming keeps the ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FacePerson {
    pub id: String,
    pub name: String,
    /// `true` once the user confirmed the label; `false` for a suggestion.
    pub confirmed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cluster_ids: Vec<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Reproducible identity of one face analysis run (source + decode/geometry +
/// detection model + embedding model + preprocessing/coordinate system +
/// clustering procedure). A change to any part means every derived artefact is
/// `stale` — never silently recomputed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceIdentity {
    pub source: SourceFingerprint,
    pub decode: DecodeFingerprint,
    pub geometry: GeometryFingerprint,
    pub detection_model: ModelIdentity,
    pub embedding_model: ModelIdentity,
    pub inference_resolution: Resolution,
    pub preprocessing: Preprocessing,
    pub rescaling_method: String,
    pub coordinate_system: CoordinateSystem,
    pub clustering: FaceClusteringIdentity,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Source-level face analysis section (`SidecarDocument::face`). Additive;
/// absent is the valid "no face analysis" state and serializes back absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceAnalysis {
    pub version: u8,
    pub identity: FaceIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detections: Vec<FaceDetection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embeddings: Vec<FaceEmbedding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clusters: Vec<FaceCluster>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub persons: Vec<FacePerson>,
    pub created_at: String,
    pub status: FaceArtifactStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

fn validate_face_id(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.is_empty() || value.trim() != value {
        return invalid(format!(
            "{field} must be non-empty and without leading/trailing whitespace"
        ));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!("{field} must not contain control characters"));
    }
    if value.chars().count() > MAX_FACE_ID_CHARS {
        return invalid(format!(
            "{field} exceeds limit of {MAX_FACE_ID_CHARS} characters"
        ));
    }
    Ok(())
}

fn validate_face_name(field: &str, value: &str) -> Result<(), SidecarError> {
    validate_name(field, value)?;
    if value.chars().any(char::is_control) {
        return invalid(format!("{field} must not contain control characters"));
    }
    if value.chars().count() > MAX_FACE_NAME_CHARS {
        return invalid(format!(
            "{field} exceeds limit of {MAX_FACE_NAME_CHARS} characters"
        ));
    }
    Ok(())
}

fn validate_face_error(error: Option<&str>) -> Result<(), SidecarError> {
    if let Some(text) = error {
        if text.trim().is_empty() {
            return invalid("face error text must not be empty or whitespace-only");
        }
        if text.chars().count() > MAX_FACE_ERROR_CHARS {
            return invalid(format!(
                "face error text exceeds limit of {MAX_FACE_ERROR_CHARS} characters"
            ));
        }
    }
    Ok(())
}

/// Loud `sha256:<64 lowercase hex>` contract (shared by the face model hash
/// and the persisted analysis digest).
pub fn validate_face_sha256(field: &str, value: &str) -> Result<(), SidecarError> {
    let Some(hex) = value.strip_prefix(FACE_SHA256_PREFIX) else {
        return invalid(format!(
            "{field} must be `{FACE_SHA256_PREFIX}<{FACE_HASH_HEX_LEN} lowercase hex>`"
        ));
    };
    if hex.len() != FACE_HASH_HEX_LEN
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return invalid(format!(
            "{field} must be `{FACE_SHA256_PREFIX}<{FACE_HASH_HEX_LEN} lowercase hex>`"
        ));
    }
    Ok(())
}

/// A model hash is either a real `sha256:<hex>` pin or the explicit
/// `pending-integration` F-078 gate marker (never a silent empty/dummy value).
fn validate_face_model_hash(field: &str, value: &str) -> Result<(), SidecarError> {
    if value == FACE_PENDING_MODEL_HASH {
        return Ok(());
    }
    validate_face_sha256(field, value)
}

fn validate_face_model(field: &str, model: &ModelIdentity) -> Result<(), SidecarError> {
    validate_face_name(&format!("{field}.name"), &model.name)?;
    validate_face_name(&format!("{field}.version"), &model.version)?;
    validate_face_model_hash(&format!("{field}.hash"), &model.hash)
}

fn validate_face_bbox(bbox: &FaceBoundingBox) -> Result<(), SidecarError> {
    let in_unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    if !in_unit(bbox.x)
        || !in_unit(bbox.y)
        || !in_unit(bbox.width)
        || !in_unit(bbox.height)
        || bbox.width <= 0.0
        || bbox.height <= 0.0
        || bbox.x + bbox.width > 1.0
        || bbox.y + bbox.height > 1.0
    {
        return invalid(
            "face bbox must have finite normalized coordinates within 0..=1 with a positive \
             area inside the frame",
        );
    }
    Ok(())
}

fn validate_face_landmarks(landmarks: &[FaceLandmark]) -> Result<(), SidecarError> {
    if landmarks.len() > MAX_FACE_LANDMARKS {
        return invalid(format!(
            "face landmarks exceed limit of {MAX_FACE_LANDMARKS}"
        ));
    }
    let mut seen = BTreeSet::new();
    for landmark in landmarks {
        validate_face_name("face landmark name", &landmark.name)?;
        if !seen.insert(&landmark.name) {
            return invalid(format!("duplicate face landmark `{}`", landmark.name));
        }
        if !landmark.x.is_finite()
            || !landmark.y.is_finite()
            || !(0.0..=1.0).contains(&landmark.x)
            || !(0.0..=1.0).contains(&landmark.y)
        {
            return invalid("face landmark coordinates must be finite within 0..=1");
        }
    }
    Ok(())
}

fn validate_face_vector_ref(reference: &FaceVectorRef) -> Result<(), SidecarError> {
    validate_relative_path("face vector relative_path", &reference.relative_path)?;
    validate_name("face vector format", &reference.format)?;
    validate_name("face vector checksum", &reference.checksum)?;
    validate_name("face vector channels", &reference.channels)?;
    validate_name("face vector data_version", &reference.data_version)?;
    if reference.dimension == 0 {
        return invalid("face vector dimension must be > 0");
    }
    Ok(())
}

fn validate_face_matte(matte: &ArtifactReference) -> Result<(), SidecarError> {
    validate_artifact(matte)?;
    if matte.width == 0 || matte.height == 0 {
        return invalid("face matte resolution must be non-zero");
    }
    Ok(())
}

/// Loud validation of the source-level face section. Every cross-reference
/// (detection ↔ embedding ↔ cluster ↔ person) must be consistent; unknown
/// targets, duplicate IDs and out-of-range values are rejected, never dropped.
pub fn validate_face_analysis(analysis: &FaceAnalysis) -> Result<(), SidecarError> {
    if analysis.version != FACE_SCHEMA_VERSION {
        return invalid(format!(
            "unsupported face.version {} (expected {FACE_SCHEMA_VERSION})",
            analysis.version
        ));
    }
    validate_face_identity(&analysis.identity)?;
    validate_metadata_timestamp(&analysis.created_at)
        .map_err(|e| SidecarError::Invalid(format!("face created_at invalid: {e}")))?;
    validate_face_error(analysis.error.as_deref())?;

    if analysis.detections.len() > MAX_FACE_DETECTIONS {
        return invalid(format!(
            "face detections exceed limit of {MAX_FACE_DETECTIONS}"
        ));
    }
    if analysis.embeddings.len() > MAX_FACE_EMBEDDINGS {
        return invalid(format!(
            "face embeddings exceed limit of {MAX_FACE_EMBEDDINGS}"
        ));
    }
    if analysis.clusters.len() > MAX_FACE_CLUSTERS {
        return invalid(format!("face clusters exceed limit of {MAX_FACE_CLUSTERS}"));
    }
    if analysis.persons.len() > MAX_FACE_PERSONS {
        return invalid(format!("face persons exceed limit of {MAX_FACE_PERSONS}"));
    }

    let mut detection_ids = BTreeSet::new();
    for detection in &analysis.detections {
        validate_face_id("face detection id", &detection.id)?;
        if !detection_ids.insert(detection.id.as_str()) {
            return invalid(format!("duplicate face detection id `{}`", detection.id));
        }
        validate_face_bbox(&detection.bbox)?;
        if !detection.score.is_finite() || !(0.0..=1.0).contains(&detection.score) {
            return invalid(format!(
                "face detection `{}` score must be finite within 0..=1",
                detection.id
            ));
        }
        validate_face_landmarks(&detection.landmarks)?;
        if let Some(matte) = &detection.matte {
            validate_face_matte(matte)?;
        }
        validate_face_error(detection.error.as_deref())?;
        if let Some(embedding_id) = &detection.embedding_id {
            validate_face_id("face embedding_id", embedding_id)?;
        }
    }

    let mut embedding_by_id = BTreeMap::new();
    for embedding in &analysis.embeddings {
        validate_face_id("face embedding id", &embedding.id)?;
        if embedding_by_id
            .insert(embedding.id.as_str(), embedding.detection_id.as_str())
            .is_some()
        {
            return invalid(format!("duplicate face embedding id `{}`", embedding.id));
        }
        validate_face_id("face embedding detection_id", &embedding.detection_id)?;
        if !detection_ids.contains(embedding.detection_id.as_str()) {
            return invalid(format!(
                "face embedding `{}` references unknown detection `{}`",
                embedding.id, embedding.detection_id
            ));
        }
        validate_face_vector_ref(&embedding.vector)?;
        validate_face_error(embedding.error.as_deref())?;
    }
    // Detection → embedding links must resolve and be reciprocal: a detection
    // may point at the embedding that was computed for it, nothing else.
    for detection in &analysis.detections {
        let Some(embedding_id) = &detection.embedding_id else {
            continue;
        };
        match embedding_by_id.get(embedding_id.as_str()) {
            Some(detection_id) if *detection_id == detection.id => {}
            Some(_) => {
                return invalid(format!(
                    "face detection `{}` embedding_id `{embedding_id}` belongs to another detection",
                    detection.id
                ));
            }
            None => {
                return invalid(format!(
                    "face detection `{}` references unknown embedding `{embedding_id}`",
                    detection.id
                ));
            }
        }
    }

    let mut cluster_ids = BTreeSet::new();
    for cluster in &analysis.clusters {
        validate_face_id("face cluster id", &cluster.id)?;
        if !cluster_ids.insert(cluster.id.as_str()) {
            return invalid(format!("duplicate face cluster id `{}`", cluster.id));
        }
        if cluster.detection_ids.len() > MAX_FACE_CLUSTER_MEMBERS {
            return invalid(format!(
                "face cluster `{}` exceeds member limit of {MAX_FACE_CLUSTER_MEMBERS}",
                cluster.id
            ));
        }
        let mut members = BTreeSet::new();
        for detection_id in &cluster.detection_ids {
            validate_face_id("face cluster detection id", detection_id)?;
            if !members.insert(detection_id.as_str()) {
                return invalid(format!(
                    "face cluster `{}` lists detection `{detection_id}` twice",
                    cluster.id
                ));
            }
            if !detection_ids.contains(detection_id.as_str()) {
                return invalid(format!(
                    "face cluster `{}` references unknown detection `{detection_id}`",
                    cluster.id
                ));
            }
        }
    }

    let mut person_ids = BTreeSet::new();
    let mut assigned_clusters = BTreeSet::new();
    for person in &analysis.persons {
        validate_face_id("face person id", &person.id)?;
        if !person_ids.insert(person.id.as_str()) {
            return invalid(format!("duplicate face person id `{}`", person.id));
        }
        validate_face_name("face person name", &person.name)?;
        let mut clusters = BTreeSet::new();
        for cluster_id in &person.cluster_ids {
            validate_face_id("face person cluster id", cluster_id)?;
            if !clusters.insert(cluster_id.as_str()) {
                return invalid(format!(
                    "face person `{}` lists cluster `{cluster_id}` twice",
                    person.id
                ));
            }
            if !cluster_ids.contains(cluster_id.as_str()) {
                return invalid(format!(
                    "face person `{}` references unknown cluster `{cluster_id}`",
                    person.id
                ));
            }
            if !assigned_clusters.insert(cluster_id.as_str()) {
                return invalid(format!(
                    "face cluster `{cluster_id}` is assigned to more than one person"
                ));
            }
        }
    }
    Ok(())
}

fn validate_face_identity(identity: &FaceIdentity) -> Result<(), SidecarError> {
    validate_name("face source.content_hash", &identity.source.content_hash)?;
    validate_name("face decode.decoder", &identity.decode.decoder)?;
    validate_name("face decode.version", &identity.decode.version)?;
    if identity.geometry.width == 0 || identity.geometry.height == 0 {
        return invalid("face geometry dimensions must be non-zero");
    }
    if !(1..=8).contains(&identity.geometry.orientation) {
        return invalid("face geometry orientation must be between 1 and 8");
    }
    if !identity.geometry.pixel_aspect_ratio.is_finite()
        || identity.geometry.pixel_aspect_ratio <= 0.0
    {
        return invalid("face geometry pixel_aspect_ratio must be finite and > 0");
    }
    validate_face_model("face detection_model", &identity.detection_model)?;
    validate_face_model("face embedding_model", &identity.embedding_model)?;
    if identity.inference_resolution.width == 0 || identity.inference_resolution.height == 0 {
        return invalid("face inference_resolution must be non-zero");
    }
    validate_face_name("face preprocessing.name", &identity.preprocessing.name)?;
    validate_face_name(
        "face preprocessing.version",
        &identity.preprocessing.version,
    )?;
    validate_face_name("face rescaling_method", &identity.rescaling_method)?;
    validate_face_name("face clustering.method", &identity.clustering.method)?;
    if identity.clustering.version == 0 {
        return invalid("face clustering.version must be >= 1");
    }
    for (key, value) in &identity.clustering.parameters {
        validate_name("face clustering parameter key", key)?;
        validate_name("face clustering parameter value", value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        load_sidecar, migrate_json, save_sidecar, DecodeFingerprint, SidecarDocument,
        SourceIdentity,
    };
    use std::collections::BTreeMap;
    use std::fs;

    fn source() -> SourceIdentity {
        SourceIdentity {
            relative_name: "IMG_0001.ARW".into(),
            content_hash: "blake3:abc".into(),
            byte_length: 42,
            modified_at: None,
            raw_format: "ARW".into(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "libraw".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 6000,
                height: 4000,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            extras: Extras::new(),
        }
    }

    fn model(name: &str) -> ModelIdentity {
        ModelIdentity {
            name: name.into(),
            version: "1.0".into(),
            hash: FACE_PENDING_MODEL_HASH.into(),
            extras: Extras::new(),
        }
    }

    fn vector_ref() -> FaceVectorRef {
        FaceVectorRef {
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "aa".repeat(32),
            dimension: 512,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }
    }

    fn analysis() -> FaceAnalysis {
        let detection = FaceDetection {
            id: "face-1".into(),
            bbox: FaceBoundingBox {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.4,
            },
            score: 0.99,
            landmarks: vec![
                FaceLandmark {
                    name: "left_eye".into(),
                    x: 0.2,
                    y: 0.3,
                },
                FaceLandmark {
                    name: "right_eye".into(),
                    x: 0.35,
                    y: 0.3,
                },
            ],
            embedding_id: Some("emb-1".into()),
            matte: Some(ArtifactReference {
                relative_path: "IMG_0001.ARW.lumina.zdata".into(),
                format: "zdata-mask".into(),
                checksum: "bb".repeat(32),
                width: 1024,
                height: 1024,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            }),
            status: FaceArtifactStatus::Valid,
            error: None,
            extras: Extras::new(),
        };
        let embedding = FaceEmbedding {
            id: "emb-1".into(),
            detection_id: "face-1".into(),
            vector: vector_ref(),
            status: FaceArtifactStatus::Valid,
            error: None,
            extras: Extras::new(),
        };
        FaceAnalysis {
            version: FACE_SCHEMA_VERSION,
            identity: FaceIdentity {
                source: SourceFingerprint {
                    content_hash: "blake3:abc".into(),
                    byte_length: 42,
                    extras: Extras::new(),
                },
                decode: source().decode_fingerprint,
                geometry: source().geometry_fingerprint,
                detection_model: model("scrfd"),
                embedding_model: model("arcface"),
                inference_resolution: Resolution {
                    width: 640,
                    height: 640,
                    extras: Extras::new(),
                },
                preprocessing: Preprocessing {
                    name: "landmark_align".into(),
                    version: "1".into(),
                    parameters: BTreeMap::from([("mean".into(), "0.5".into())]),
                    extras: Extras::new(),
                },
                rescaling_method: "identity".into(),
                coordinate_system: CoordinateSystem::SourceOriented,
                clustering: FaceClusteringIdentity {
                    method: "dbscan_cosine".into(),
                    version: 1,
                    parameters: BTreeMap::from([("eps".into(), "0.4".into())]),
                    extras: Extras::new(),
                },
                extras: Extras::new(),
            },
            detections: vec![detection],
            embeddings: vec![embedding],
            clusters: vec![FaceCluster {
                id: "cluster-1".into(),
                detection_ids: vec!["face-1".into()],
                extras: Extras::new(),
            }],
            persons: vec![FacePerson {
                id: "person-1".into(),
                name: "Alex".into(),
                confirmed: true,
                cluster_ids: vec!["cluster-1".into()],
                extras: Extras::new(),
            }],
            created_at: "2026-09-16T08:00:00Z".into(),
            status: FaceArtifactStatus::Valid,
            error: None,
            extras: Extras::new(),
        }
    }

    fn document_with_face() -> SidecarDocument {
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.face = Some(analysis());
        document
    }

    #[test]
    fn face_section_is_absent_by_default_and_serializes_absent() {
        let document = SidecarDocument::new(source(), "pipeline-1");
        assert!(document.face.is_none());
        let json = document.to_json().unwrap();
        assert!(!json.contains("\"face\""), "absent face must not serialize");
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert!(decoded.face.is_none());
    }

    #[test]
    fn face_roundtrip_is_lossless_and_byte_stable() {
        let document = document_with_face();
        let json = document.to_json().unwrap();
        assert!(json.contains("face-1"));
        assert!(json.contains("arcface"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, document);
        assert_eq!(decoded.to_json().unwrap(), json);
    }

    #[test]
    fn face_unknown_fields_roundtrip_as_extras() {
        let document = document_with_face();
        let mut value: serde_json::Value =
            serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["face"]["future_face"] = serde_json::Value::from(1);
        value["face"]["identity"]["detection_model"]["future_model"] =
            serde_json::Value::from("kept");
        value["face"]["detections"][0]["future_detection"] = serde_json::Value::from(true);
        let decoded = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        let reencoded: serde_json::Value =
            serde_json::from_str(&decoded.to_json().unwrap()).unwrap();
        assert_eq!(reencoded["face"]["future_face"], serde_json::Value::from(1));
        assert_eq!(
            reencoded["face"]["identity"]["detection_model"]["future_model"],
            serde_json::Value::from("kept")
        );
        assert_eq!(
            reencoded["face"]["detections"][0]["future_detection"],
            serde_json::Value::from(true)
        );
    }

    #[test]
    fn face_legacy_documents_load_without_face_and_migrate_without_loss() {
        // Current-schema JSON without the additive key.
        let legacy = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let document = SidecarDocument::from_json(legacy).unwrap();
        assert!(document.face.is_none());
        assert!(!document.to_json().unwrap().contains("\"face\""));
        // Explicit v1 → v2 migration preserves the legacy document.
        let v1 = legacy.replace("\"schema_version\":2", "\"schema_version\":1");
        let migrated = migrate_json(&v1).unwrap();
        let decoded = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(decoded.schema_version, crate::SCHEMA_VERSION);
        assert!(decoded.face.is_none());
    }

    #[test]
    fn face_file_roundtrip_is_atomic_and_reloadable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("IMAGE.lumina.json");
        let document = document_with_face();
        save_sidecar(&path, &document).unwrap();
        let loaded = load_sidecar(&path).unwrap();
        assert_eq!(loaded, document);
        // Recovery: a crashed temporary is swept, the valid sidecar survives.
        let temp = directory.path().join(".IMAGE.lumina.json.tmp-crash");
        fs::write(&temp, b"{\"partial\": true}").unwrap();
        let file = fs::OpenOptions::new().write(true).open(&temp).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        let reloaded = load_sidecar(&path).unwrap();
        assert_eq!(reloaded, document);
        assert!(!temp.exists());
    }

    #[test]
    fn face_validation_rejects_unknown_version() {
        let mut face = analysis();
        face.version = 2;
        assert!(validate_face_analysis(&face).is_err());
    }

    #[test]
    fn face_validation_rejects_inconsistent_references() {
        // Detection points at an embedding of another detection.
        let mut face = analysis();
        face.embeddings[0].detection_id = "face-other".into();
        assert!(validate_face_analysis(&face).is_err());
        // Detection points at an unknown embedding.
        let mut face = analysis();
        face.detections[0].embedding_id = Some("emb-missing".into());
        assert!(validate_face_analysis(&face).is_err());
        // Cluster references an unknown detection.
        let mut face = analysis();
        face.clusters[0].detection_ids = vec!["face-missing".into()];
        assert!(validate_face_analysis(&face).is_err());
        // Person references an unknown cluster.
        let mut face = analysis();
        face.persons[0].cluster_ids = vec!["cluster-missing".into()];
        assert!(validate_face_analysis(&face).is_err());
        // Duplicate detection ids are rejected, never merged.
        let mut face = analysis();
        let duplicate = face.detections[0].clone();
        face.detections.push(duplicate);
        assert!(validate_face_analysis(&face).is_err());
    }

    #[test]
    fn face_validation_rejects_out_of_range_geometry_and_scores() {
        let cases: [fn(&mut FaceAnalysis); 13] = [
            |face| face.detections[0].score = 1.5,
            |face| face.detections[0].score = f32::NAN,
            |face| face.detections[0].bbox.width = 0.0,
            |face| face.detections[0].bbox.x = 0.9,
            |face| face.detections[0].landmarks[0].x = 1.5,
            |face| face.detections[0].landmarks[0].name.clear(),
            |face| face.detections[0].error = Some("   ".into()),
            |face| face.embeddings[0].vector.dimension = 0,
            |face| face.embeddings[0].vector.relative_path = "/abs/vector.bin".into(),
            |face| face.detections[0].matte.as_mut().unwrap().width = 0,
            |face| face.identity.clustering.version = 0,
            |face| face.identity.detection_model.hash = "dummy".into(),
            |face| face.created_at = "not-a-timestamp".into(),
        ];
        for mutate in cases {
            let mut face = analysis();
            mutate(&mut face);
            assert!(
                validate_face_analysis(&face).is_err(),
                "face contract violation must be rejected loudly"
            );
        }
    }

    #[test]
    fn face_accepts_real_sha256_and_pending_model_hash() {
        let mut face = analysis();
        face.identity.detection_model.hash = format!("{}{}", FACE_SHA256_PREFIX, "ab".repeat(32));
        face.identity.embedding_model.hash = FACE_PENDING_MODEL_HASH.into();
        validate_face_analysis(&face).expect("real pin + pending are both legal");
        face.identity.embedding_model.hash = format!("{}{}", FACE_SHA256_PREFIX, "AB".repeat(32));
        assert!(
            validate_face_analysis(&face).is_err(),
            "uppercase hex rejected"
        );
    }
}
