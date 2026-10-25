//! `face` (LRPAR-G12-FACE-IMPL-20 / S4 CLI slice): the persisted face
//! analysis status/analyze command, its argument type and its identity and
//! artifact-evidence helpers.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, face slice). Pure move: bodies are
//! byte-identical; the `#[cfg]` attributes and the `#[cfg(feature = "onnx-rt")]`
//! imports moved verbatim; only the visibility tokens, the `use` line and this
//! header are new.

use super::*;
// ===========================================================================
// LRPAR-G12-FACE-IMPL-20 / S4: `face`.
//
// SOLL: `feature/decisions/LRPAR-G12-FACE-20.md` §2.3/§4/§6. `--status`
// re-evaluates the persisted source-level analysis against the live
// source/decode/model context (visible valid/stale/missing/corrupt); `--analyze`
// runs the real ONNX face engine loudly (`try_load_face_engine`) — without the
// `onnx-rt` capability the command refuses with exit 1 and writes nothing (no
// silent stub fallback). Person names are never synthesized.
// ===========================================================================

#[derive(Debug, Args)]
pub(crate) struct FaceArgs {
    #[arg(long)]
    pub(crate) input: PathBuf,
    /// Report the persisted analysis status and exit (read-only; the default
    /// when `--analyze` is absent).
    #[arg(long)]
    pub(crate) status: bool,
    /// Run the real face engine and persist the analysis.
    #[arg(long)]
    pub(crate) analyze: bool,
    /// Re-analyze even when a valid, identity-matching analysis already exists.
    #[arg(long)]
    pub(crate) force: bool,
    /// Real detection model artifact (`.onnx`); falls back to
    /// `LUMINA_FACE_DETECT_MODEL_PATH`.
    #[arg(long)]
    pub(crate) detector: Option<PathBuf>,
    /// Real embedding model artifact (`.onnx`); falls back to
    /// `LUMINA_FACE_EMBED_MODEL_PATH`.
    #[arg(long)]
    pub(crate) embedder: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

fn face_status_str(status: FaceArtifactStatus) -> &'static str {
    match status {
        FaceArtifactStatus::Valid => "valid",
        FaceArtifactStatus::Stale => "stale",
        FaceArtifactStatus::Missing => "missing",
        FaceArtifactStatus::Corrupt => "corrupt",
    }
}

/// Real evidence about the binary face artifacts a persisted analysis
/// references, derived from the `.lumina.zdata` records themselves — never
/// from mere existence.
///
/// This is the shared `lumina_sidecar::face_artifact_evidence` contract (FACE-20
/// §3.2), used verbatim by CLI and GUI, so both classify the same sidecar
/// identically:
///
/// - an analysis with no persisted embedding references, a missing bundle, a
///   missing `face_embedding` record or an unreadable file →
///   [`FaceArtifactEvidence::Missing`]
/// - a present record whose record checksum (BLAKE3 over the canonical
///   `encoding_version || dimension || f32` stream) differs from the persisted
///   `FaceVectorRef.checksum`, or whose dimension differs, or a bundle that is
///   not loadable → `Present { checksum_matches: false }` (classified
///   `corrupt`)
/// - every referenced record verified → `Present { checksum_matches: true }`
pub(super) fn face_artifact_evidence(
    bundle_root: &Path,
    analysis: &FaceAnalysis,
) -> FaceArtifactEvidence {
    lumina_sidecar::face_artifact_evidence(bundle_root, analysis)
}

/// Stable machine label for the artifact evidence reported by `face --status`
/// (mirrors the GUI's evidence classification).
fn face_evidence_str(evidence: FaceArtifactEvidence) -> &'static str {
    match evidence {
        FaceArtifactEvidence::Missing => "missing",
        FaceArtifactEvidence::Present {
            checksum_matches: true,
        } => "checksum-verified",
        FaceArtifactEvidence::Present {
            checksum_matches: false,
        } => "checksum-mismatch",
    }
}

/// Bundle root (directory holding the source and its `.lumina.json`/`.zdata`),
/// used to resolve the analysis' relative artifact references.
fn face_bundle_root(input: &Path) -> &Path {
    input.parent().unwrap_or_else(|| Path::new("."))
}

/// Builds the live face identity (source/decode/geometry + the shipped
/// candidate model suite + clustering identity) for the status comparison.
fn face_current_identity(
    document: &SidecarDocument,
) -> Result<lumina_sidecar::FaceIdentity, CliError> {
    face_identity(
        &FaceModelSuite::candidate(),
        SourceFingerprint {
            content_hash: document.source.content_hash.clone(),
            byte_length: document.source.byte_length,
            extras: Default::default(),
        },
        document.source.decode_fingerprint.clone(),
        document.source.geometry_fingerprint.clone(),
        FaceClusteringParams::default().to_identity(),
        &FaceInferenceOptions::default(),
    )
    .map_err(|error| CliError::Message(format!("face identity error: {error}")))
}

pub(super) fn face(args: FaceArgs) -> Result<(), CliError> {
    if args.status && args.analyze {
        return Err(CliError::Usage(
            "--status and --analyze are mutually exclusive".into(),
        ));
    }
    let path = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&path)?;
    if args.analyze {
        face_analyze(&args, &mut document, &path)
    } else {
        face_status(&args, &document)
    }
}

fn face_status(args: &FaceArgs, document: &SidecarDocument) -> Result<(), CliError> {
    let Some(analysis) = document.face.as_ref() else {
        return emit(
            args.json,
            serde_json::json!({
                "command": "face",
                "action": "status",
                "input": args.input,
                "status": "no-analysis",
                "detections": 0,
            }),
            "face analysis: no analysis",
        );
    };
    let current = face_current_identity(document)?;
    // M2: real evidence from the record checksums (BLAKE3 per referenced
    // `face_embedding` record), identical to the GUI classification. A
    // persisted `valid` analysis whose
    // references are absent/empty/tampered must never be reported `valid`.
    let evidence = face_artifact_evidence(face_bundle_root(&args.input), analysis);
    let status = if analysis.status != FaceArtifactStatus::Valid {
        analysis.status
    } else {
        face_artifact_status(&current, &analysis.identity, evidence)
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "face",
            "action": "status",
            "input": args.input,
            "status": face_status_str(status),
            "detections": analysis.detections.len(),
            "embeddings": analysis.embeddings.len(),
            "clusters": analysis.clusters.len(),
            "persons": analysis.persons.len(),
            "created_at": analysis.created_at,
            "artifact_evidence": face_evidence_str(evidence),
        }),
        &format!("face analysis: {}", face_status_str(status)),
    )?;
    if status == FaceArtifactStatus::Corrupt {
        return Err(CliError::Message(
            "face analysis is corrupt; explicit `--analyze` required (never automatic)".into(),
        ));
    }
    Ok(())
}

pub(super) fn face_analyze(
    args: &FaceArgs,
    document: &mut SidecarDocument,
    _sidecar: &Path,
) -> Result<(), CliError> {
    let current = face_current_identity(document)?;
    // No automatic re-inference: a valid, identity-matching analysis with
    // verified artifact references is kept unless `--force`. M2: the check
    // uses the same real BLAKE3 evidence as `--status`, so an analysis whose
    // referenced vectors are missing/empty/tampered is re-run instead of being
    // reported as an unchanged valid result.
    if !args.force {
        if let Some(existing) = document.face.as_ref() {
            let evidence = face_artifact_evidence(face_bundle_root(&args.input), existing);
            if existing.status == FaceArtifactStatus::Valid
                && face_artifact_status(&current, &existing.identity, evidence)
                    == FaceArtifactStatus::Valid
            {
                return emit(
                    args.json,
                    serde_json::json!({
                        "command": "face",
                        "action": "analyze",
                        "input": args.input,
                        "status": "valid",
                        "changed": false,
                    }),
                    "face analysis already valid (use --force to re-run)",
                );
            }
        }
    }

    let detector = args
        .detector
        .clone()
        .or_else(|| std::env::var_os("LUMINA_FACE_DETECT_MODEL_PATH").map(PathBuf::from))
        .ok_or_else(|| {
            CliError::Message(
                "face --analyze requires --detector or LUMINA_FACE_DETECT_MODEL_PATH".into(),
            )
        })?;
    let embedder = args
        .embedder
        .clone()
        .or_else(|| std::env::var_os("LUMINA_FACE_EMBED_MODEL_PATH").map(PathBuf::from))
        .ok_or_else(|| {
            CliError::Message(
                "face --analyze requires --embedder or LUMINA_FACE_EMBED_MODEL_PATH".into(),
            )
        })?;
    let suite = FaceModelSuite::candidate();
    let options = FaceInferenceOptions::default();
    let engine = try_load_face_engine(&detector, &suite, &embedder, &options)
        .map_err(|error| CliError::Message(format!("face engine load failed: {error}")))?;

    match engine {
        FaceOnnxEngine::RuntimeDisabled => Err(CliError::Message(
            "face analysis unavailable: `onnx-rt` is not compiled into this build; \
             no analysis was produced (no silent stub fallback)"
                .into(),
        )),
        #[cfg(feature = "onnx-rt")]
        FaceOnnxEngine::OnnxRuntime { detector, embedder } => {
            let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
            let (frame, _raw) = decode_input(&args.input, &bytes)?;
            let detections = detector
                .detect(&frame)
                .map_err(|error| CliError::Message(format!("face detection failed: {error}")))?;
            let vectors = embedder
                .embed(&frame, &detections)
                .map_err(|error| CliError::Message(format!("face embedding failed: {error}")))?;
            if vectors.len() != detections.len() {
                return Err(CliError::Message(format!(
                    "face embedder returned {} vectors for {} detections",
                    vectors.len(),
                    detections.len()
                )));
            }
            let raw_vectors: Vec<Vec<f32>> = vectors
                .iter()
                .map(|vector| vector.values().to_vec())
                .collect();
            let labels = cluster_embeddings(&raw_vectors, &FaceClusteringParams::default())
                .map_err(|error| CliError::Message(format!("face clustering failed: {error}")))?;
            let detection_ids: Vec<String> = detections.iter().map(detected_face_id).collect();
            let clusters = clusters_from_labels(&detection_ids, &labels).map_err(|error| {
                CliError::Message(format!("face cluster build failed: {error}"))
            })?;
            // FACE-20 §3.2: persist the normalized vectors as `face_embedding`
            // records in the shared bundle *before* the JSON sidecar records
            // their references, so a failure can never leave a dangling
            // reference. The record checksum is the persisted reference
            // checksum (BLAKE3 over the canonical raw vector stream).
            let zdata_path = zdata_path_for(&args.input);
            let relative_path = zdata_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| CliError::Message("face bundle path is not valid UTF-8".into()))?
                .to_string();
            let mut records = Vec::with_capacity(detections.len());
            let mut embeddings = Vec::with_capacity(detections.len());
            for (index, vector) in vectors.iter().enumerate() {
                let detection_id = detection_ids
                    .get(index)
                    .expect("one vector per detection, checked above");
                let embedding_id = embedding_id_for(detection_id);
                let dimension = vector.dimension() as u32;
                let record = SidecarFaceEmbeddingArtifact {
                    id: embedding_id.clone(),
                    dimension,
                    values: vector.values().to_vec(),
                };
                embeddings.push(FaceEmbeddingRecord {
                    detection_index: index,
                    vector: vector.clone(),
                    reference: lumina_sidecar::FaceVectorRef {
                        relative_path: relative_path.clone(),
                        format: "lumina-zdata".into(),
                        checksum: record.checksum(),
                        dimension,
                        channels: "f32".into(),
                        data_version: "1".into(),
                        extras: Default::default(),
                    },
                });
                records.push(record);
            }
            if !records.is_empty() {
                save_face_embeddings(&zdata_path, records).map_err(|error| {
                    CliError::Message(format!("face vector persistence failed: {error}"))
                })?;
            }
            let output = FaceAnalysisOutput {
                identity: current,
                created_at: now_rfc3339_utc(),
                detections,
                embeddings,
                clusters,
                persons: vec![],
            };
            let analysis = output.into_sidecar().map_err(|error| {
                CliError::Message(format!("face analysis build failed: {error}"))
            })?;
            document.face = Some(analysis);
            document.validate()?;
            save_sidecar(_sidecar, document)?;
            let analysis = document.face.as_ref().expect("just assigned");
            info!(
                "face analysis persisted: detections={} embeddings={} clusters={}",
                analysis.detections.len(),
                analysis.embeddings.len(),
                analysis.clusters.len()
            );
            emit(
                args.json,
                serde_json::json!({
                    "command": "face",
                    "action": "analyze",
                    "input": args.input,
                    "status": "valid",
                    "changed": true,
                    "detections": analysis.detections.len(),
                    "clusters": analysis.clusters.len(),
                    "embeddings_persisted": !analysis.embeddings.is_empty(),
                }),
                "face analysis written",
            )
        }
    }
}
