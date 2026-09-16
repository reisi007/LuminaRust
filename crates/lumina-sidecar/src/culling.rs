//! LRPAR-G09-CULL-25: source-level `culling` proposal schema (Release 2.5).
//!
//! SOLL: `feature/decisions/LRPAR-G09-CULL-25.md` §5 „Persistenz-Scope
//! (Sidecar-first)". KI-Culling in LuminaRust is *assisted culling* only: the
//! system may compute and show one reasoned recommendation per source image
//! (`keep` / `review` / `reject-kandidat` plus a score and machine-readable
//! reason codes). It must **never** write `rating`, `flag`, `color_label` or
//! any other recipe field — adopting a proposal is an explicit user action per
//! virtual copy through the normal save/render path.
//!
//! Persistence rules (Agents.md, `feature/architecture/sidecar.md`):
//!
//! - The proposal lives on the **source level** (shared analysis, like the
//!   shared mask matte); rating/flag/label stay per virtual copy.
//! - An absent `culling` section is the valid "no proposal" state and is not
//!   an error; `status` distinguishes a persisted but no-longer-usable
//!   proposal (`stale`/`missing`/`corrupt`).
//! - Scores are small, so they stay inline in the JSON. No float arrays are
//!   persisted here; there are no absolute paths.
//! - Validation is loud: unknown proposal/status values fail deserialization,
//!   out-of-range scores and malformed reason codes are rejected, never
//!   clipped or silently dropped.
//!
//! Scope: schema + validation only. The deterministic heuristic analysis
//! (`lumina-core`), CLI, GUI and the optional ONNX stage are later slices
//! (§7).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    invalid, validate_metadata_timestamp, validate_name, DecodeFingerprint, Extras,
    GeometryFingerprint, Preprocessing, Resolution, SidecarError, SourceFingerprint,
};

/// Current culling-proposal schema version. Foreign versions are rejected
/// loudly (pre-MVP: no back-compat obligation, but always versioned).
pub const CULLING_SCHEMA_VERSION: u8 = 1;

/// F-078 gate marker for the optional ONNX stage: until licence-checked,
/// hash-pinned weights exist, the analyzer carries `pending-integration`
/// instead of a `sha256:<hex>` pin.
pub const CULLING_PENDING_MODEL_HASH: &str = "pending-integration";
/// Prefix of the SHA-256 hash contract (`sha256:<64 lowercase hex>`).
pub const CULLING_SHA256_PREFIX: &str = "sha256:";
/// Hex length of a SHA-256 digest.
pub const CULLING_HASH_HEX_LEN: usize = 64;

/// Bounds against hostile/degenerate documents.
pub const MAX_CULLING_REASONS: usize = 64;
pub const MAX_CULLING_REASON_CHARS: usize = 64;
pub const MAX_CULLING_ERROR_CHARS: usize = 1024;

/// The three normative recommendation values. Persisted lowercase
/// (`keep`/`review`/`reject-kandidat`); unknown values fail deserialization
/// loudly instead of being guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CullProposal {
    Keep,
    Review,
    #[serde(rename = "reject-kandidat")]
    RejectCandidate,
}

/// Status of a persisted culling proposal. `stale`/`missing`/`corrupt` are
/// visible states; there is no silent re-computation as the only option and
/// no invented recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CullingStatus {
    Valid,
    Stale,
    Missing,
    Corrupt,
}

/// Analysis backend of the proposal. Stage 1 is the model-free, deterministic
/// heuristic in `lumina-core`; stage 2 is the optional local ONNX model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CullingAnalyzerKind {
    Heuristic,
    Onnx,
}

/// Analyzer/model identity of the proposal. The heuristic carries no model
/// hash (and must not carry one); the ONNX analyzer requires a hash pin or
/// the explicit `pending-integration` gate marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CullingAnalyzer {
    pub kind: CullingAnalyzerKind,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_hash: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Reproducible identity of one culling analysis: source content hash,
/// decode/geometry context, analyzer/model identity, analysis resolution and
/// preprocessing. A change to any part makes the proposal `stale` — visible,
/// with an explicit refresh, never a hidden re-run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CullingIdentity {
    pub source: SourceFingerprint,
    pub decode: DecodeFingerprint,
    pub geometry: GeometryFingerprint,
    pub analyzer: CullingAnalyzer,
    pub analysis_resolution: Resolution,
    pub preprocessing: Preprocessing,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Source-level culling proposal (`SidecarDocument::culling`). Additive;
/// absent is the valid "no proposal" state and serializes back absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CullingSection {
    pub version: u8,
    pub proposal: CullProposal,
    /// Deterministic recommendation score `0..=1` on the documented scale.
    pub score: f32,
    /// Machine-readable reason codes (open registry, e.g. `sharpness_low`,
    /// `motion_blur_suspect`, `exposure_clipped`, `noise_high_iso`,
    /// `duplicate_group`). Codes are validated for shape, not against a
    /// closed list, so a newer analyzer can add codes without a schema bump.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    pub identity: CullingIdentity,
    pub created_at: String,
    pub status: CullingStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

fn validate_culling_name(field: &str, value: &str) -> Result<(), SidecarError> {
    validate_name(field, value)?;
    if value.chars().any(char::is_control) {
        return invalid(format!("{field} must not contain control characters"));
    }
    Ok(())
}

fn validate_culling_error(error: Option<&str>) -> Result<(), SidecarError> {
    if let Some(text) = error {
        if text.trim().is_empty() {
            return invalid("culling error text must not be empty or whitespace-only");
        }
        if text.chars().count() > MAX_CULLING_ERROR_CHARS {
            return invalid(format!(
                "culling error text exceeds limit of {MAX_CULLING_ERROR_CHARS} characters"
            ));
        }
    }
    Ok(())
}

/// Loud `sha256:<64 lowercase hex>` contract (ONNX model hash).
pub fn validate_culling_sha256(field: &str, value: &str) -> Result<(), SidecarError> {
    let Some(hex) = value.strip_prefix(CULLING_SHA256_PREFIX) else {
        return invalid(format!(
            "{field} must be `{CULLING_SHA256_PREFIX}<{CULLING_HASH_HEX_LEN} lowercase hex>`"
        ));
    };
    if hex.len() != CULLING_HASH_HEX_LEN
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return invalid(format!(
            "{field} must be `{CULLING_SHA256_PREFIX}<{CULLING_HASH_HEX_LEN} lowercase hex>`"
        ));
    }
    Ok(())
}

/// A machine-readable reason code: lowercase `[a-z0-9_]`, non-empty and
/// within the length cap. Unknown codes are legal (open registry); malformed
/// codes are rejected loudly.
pub fn validate_culling_reason(reason: &str) -> Result<(), SidecarError> {
    if reason.is_empty()
        || reason.chars().count() > MAX_CULLING_REASON_CHARS
        || !reason
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return invalid(format!(
            "culling reason code `{reason}` must be non-empty lowercase [a-z0-9_] within \
             {MAX_CULLING_REASON_CHARS} characters"
        ));
    }
    Ok(())
}

fn validate_culling_analyzer(analyzer: &CullingAnalyzer) -> Result<(), SidecarError> {
    validate_culling_name("culling analyzer.name", &analyzer.name)?;
    validate_culling_name("culling analyzer.version", &analyzer.version)?;
    match (analyzer.kind, analyzer.model_hash.as_deref()) {
        (CullingAnalyzerKind::Heuristic, None) => Ok(()),
        (CullingAnalyzerKind::Heuristic, Some(_)) => {
            invalid("culling heuristic analyzer must not carry a model_hash")
        }
        (CullingAnalyzerKind::Onnx, None) => {
            invalid("culling onnx analyzer requires a model_hash pin")
        }
        (CullingAnalyzerKind::Onnx, Some(hash)) => {
            if hash == CULLING_PENDING_MODEL_HASH {
                Ok(())
            } else {
                validate_culling_sha256("culling analyzer.model_hash", hash)
            }
        }
    }
}

fn validate_culling_identity(identity: &CullingIdentity) -> Result<(), SidecarError> {
    validate_name("culling source.content_hash", &identity.source.content_hash)?;
    validate_name("culling decode.decoder", &identity.decode.decoder)?;
    validate_name("culling decode.version", &identity.decode.version)?;
    if identity.geometry.width == 0 || identity.geometry.height == 0 {
        return invalid("culling geometry dimensions must be non-zero");
    }
    if !(1..=8).contains(&identity.geometry.orientation) {
        return invalid("culling geometry orientation must be between 1 and 8");
    }
    if !identity.geometry.pixel_aspect_ratio.is_finite()
        || identity.geometry.pixel_aspect_ratio <= 0.0
    {
        return invalid("culling geometry pixel_aspect_ratio must be finite and > 0");
    }
    validate_culling_analyzer(&identity.analyzer)?;
    if identity.analysis_resolution.width == 0 || identity.analysis_resolution.height == 0 {
        return invalid("culling analysis_resolution must be non-zero");
    }
    validate_culling_name("culling preprocessing.name", &identity.preprocessing.name)?;
    validate_culling_name(
        "culling preprocessing.version",
        &identity.preprocessing.version,
    )?;
    Ok(())
}

/// Loud validation of the source-level culling section.
pub fn validate_culling_section(section: &CullingSection) -> Result<(), SidecarError> {
    if section.version != CULLING_SCHEMA_VERSION {
        return invalid(format!(
            "unsupported culling.version {} (expected {CULLING_SCHEMA_VERSION})",
            section.version
        ));
    }
    if !section.score.is_finite() || !(0.0..=1.0).contains(&section.score) {
        return invalid("culling.score must be finite within 0..=1");
    }
    if section.reasons.len() > MAX_CULLING_REASONS {
        return invalid(format!(
            "culling reasons exceed limit of {MAX_CULLING_REASONS}"
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for reason in &section.reasons {
        validate_culling_reason(reason)?;
        if !seen.insert(reason) {
            return invalid(format!("duplicate culling reason `{reason}`"));
        }
    }
    validate_culling_identity(&section.identity)?;
    validate_metadata_timestamp(&section.created_at)
        .map_err(|e| SidecarError::Invalid(format!("culling created_at invalid: {e}")))?;
    validate_culling_error(section.error.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        load_sidecar, migrate_json, save_sidecar, DecodeFingerprint, SidecarDocument,
        SourceIdentity,
    };
    use std::collections::BTreeMap;

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

    fn section() -> CullingSection {
        CullingSection {
            version: CULLING_SCHEMA_VERSION,
            proposal: CullProposal::Review,
            score: 0.35,
            reasons: vec!["sharpness_low".into(), "duplicate_group".into()],
            identity: CullingIdentity {
                source: SourceFingerprint {
                    content_hash: "blake3:abc".into(),
                    byte_length: 42,
                    extras: Extras::new(),
                },
                decode: source().decode_fingerprint,
                geometry: source().geometry_fingerprint,
                analyzer: CullingAnalyzer {
                    kind: CullingAnalyzerKind::Heuristic,
                    name: "lumina-quality".into(),
                    version: "1".into(),
                    model_hash: None,
                    extras: Extras::new(),
                },
                analysis_resolution: Resolution {
                    width: 2048,
                    height: 1365,
                    extras: Extras::new(),
                },
                preprocessing: Preprocessing {
                    name: "srgb_rgba8".into(),
                    version: "1".into(),
                    parameters: BTreeMap::new(),
                    extras: Extras::new(),
                },
                extras: Extras::new(),
            },
            created_at: "2026-09-16T08:00:00Z".into(),
            status: CullingStatus::Valid,
            error: None,
            extras: Extras::new(),
        }
    }

    fn document_with_culling() -> SidecarDocument {
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.culling = Some(section());
        document
    }

    #[test]
    fn absent_section_is_valid_no_proposal_and_serializes_absent() {
        let document = SidecarDocument::new(source(), "pipeline-1");
        assert!(document.culling.is_none());
        let json = document.to_json().unwrap();
        assert!(
            !json.contains("\"culling\""),
            "no proposal must not serialize a section"
        );
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert!(decoded.culling.is_none());
    }

    #[test]
    fn culling_roundtrip_is_lossless_and_byte_stable() {
        let document = document_with_culling();
        let json = document.to_json().unwrap();
        assert!(json.contains("sharpness_low"));
        assert!(json.contains("\"review\""));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, document);
        assert_eq!(decoded.to_json().unwrap(), json);
    }

    #[test]
    fn culling_legacy_documents_load_without_section() {
        let legacy = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let document = SidecarDocument::from_json(legacy).unwrap();
        assert!(document.culling.is_none());
        assert!(!document.to_json().unwrap().contains("\"culling\""));
    }

    #[test]
    fn explicit_v1_to_v2_migration_preserves_document_without_culling() {
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut legacy: serde_json::Value =
            serde_json::from_str(&document.to_json().unwrap()).unwrap();
        legacy["schema_version"] = serde_json::Value::from(1);
        let migrated = migrate_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
        let decoded = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(decoded.schema_version, crate::SCHEMA_VERSION);
        assert!(decoded.culling.is_none());
    }

    #[test]
    fn culling_file_roundtrip_is_atomic_and_reloadable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("IMAGE.lumina.json");
        let document = document_with_culling();
        save_sidecar(&path, &document).unwrap();
        assert_eq!(load_sidecar(&path).unwrap(), document);
    }

    #[test]
    fn culling_recovery_sweeps_crashed_temporary_and_keeps_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("IMAGE.lumina.json");
        let document = document_with_culling();
        save_sidecar(&path, &document).unwrap();
        let temp = directory.path().join(".IMAGE.lumina.json.tmp-crash");
        std::fs::write(&temp, b"{\"partial\": true}").unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&temp).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        assert_eq!(load_sidecar(&path).unwrap(), document);
        assert!(!temp.exists(), "orphaned temporary must be swept");
    }

    #[test]
    fn unknown_proposal_or_status_is_a_loud_parse_error() {
        for (pointer, value) in [
            ("/culling/proposal", serde_json::Value::from("maybe")),
            ("/culling/status", serde_json::Value::from("done")),
            (
                "/culling/identity/analyzer/kind",
                serde_json::Value::from("cloud"),
            ),
        ] {
            let mut json: serde_json::Value =
                serde_json::from_str(&document_with_culling().to_json().unwrap()).unwrap();
            *json.pointer_mut(pointer).unwrap() = value;
            let err = SidecarDocument::from_json(&serde_json::to_string(&json).unwrap())
                .expect_err("unknown enum value rejected");
            assert!(matches!(err, SidecarError::Json(_)), "got: {err}");
        }
    }

    #[test]
    fn culling_validation_matrix_rejects_every_deviation() {
        let cases: [fn(&mut CullingSection); 12] = [
            |s| s.version = 2,
            |s| s.score = 1.5,
            |s| s.score = f32::NAN,
            |s| s.reasons = vec!["Sharpness Low".into()],
            |s| s.reasons = vec!["ok".into(), "ok".into()],
            |s| s.reasons = vec![format!("r{}", "x".repeat(MAX_CULLING_REASON_CHARS))],
            |s| s.error = Some("  ".into()),
            |s| s.created_at = "not-a-timestamp".into(),
            |s| s.identity.geometry.width = 0,
            |s| s.identity.analysis_resolution.height = 0,
            |s| s.identity.analyzer.version.clear(),
            |s| {
                s.identity.analyzer.kind = CullingAnalyzerKind::Onnx;
                s.identity.analyzer.model_hash = Some("dummy".into());
            },
        ];
        for mutate in cases {
            let mut value = section();
            mutate(&mut value);
            assert!(
                validate_culling_section(&value).is_err(),
                "culling contract violation must be rejected loudly"
            );
        }
        validate_culling_section(&section()).expect("sample is valid");
    }

    #[test]
    fn analyzer_kind_and_hash_pairing_is_enforced() {
        // Heuristic must not pretend to have a model.
        let mut heuristic = section();
        heuristic.identity.analyzer.model_hash =
            Some(format!("{CULLING_SHA256_PREFIX}{}", "ab".repeat(32)));
        assert!(validate_culling_section(&heuristic).is_err());
        // ONNX requires a pin (or the explicit pending marker).
        let mut onnx = section();
        onnx.identity.analyzer.kind = CullingAnalyzerKind::Onnx;
        assert!(validate_culling_section(&onnx).is_err());
        onnx.identity.analyzer.model_hash = Some(CULLING_PENDING_MODEL_HASH.into());
        validate_culling_section(&onnx).expect("pending marker is legal");
        onnx.identity.analyzer.model_hash =
            Some(format!("{CULLING_SHA256_PREFIX}{}", "ab".repeat(32)));
        validate_culling_section(&onnx).expect("real pin is legal");
    }

    /// D1-Nacharbeit (Schema-Verifizierung): every enum variant must survive a
    /// serde roundtrip on its documented wire form. The earlier matrices only
    /// exercised the `heuristic`/`valid`/`review` defaults, so this table
    /// covers the previously untested variants: all `CullProposal`s
    /// (`keep`/`review`/`reject-kandidat`), all `CullingStatus`s
    /// (`valid`/`stale`/`missing`/`corrupt`) and both `CullingAnalyzerKind`s
    /// (`heuristic`/`onnx`).
    #[test]
    fn serde_roundtrip_covers_every_enum_variant() {
        for (variant, wire) in [
            (CullProposal::Keep, "\"keep\""),
            (CullProposal::Review, "\"review\""),
            (CullProposal::RejectCandidate, "\"reject-kandidat\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, wire, "CullProposal wire form");
            let decoded: CullProposal = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, variant);
        }
        for (variant, wire) in [
            (CullingStatus::Valid, "\"valid\""),
            (CullingStatus::Stale, "\"stale\""),
            (CullingStatus::Missing, "\"missing\""),
            (CullingStatus::Corrupt, "\"corrupt\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, wire, "CullingStatus wire form");
            let decoded: CullingStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, variant);
        }
        for (variant, wire) in [
            (CullingAnalyzerKind::Heuristic, "\"heuristic\""),
            (CullingAnalyzerKind::Onnx, "\"onnx\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, wire, "CullingAnalyzerKind wire form");
            let decoded: CullingAnalyzerKind = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, variant);
        }
    }

    /// D1-Nacharbeit: the full source-level section must roundtrip losslessly
    /// for every documented proposal/status pair, including the ONNX analyzer
    /// (with the explicit `pending-integration` and real hash pins). The
    /// section is a nested struct, so this pins key placement and ordering in
    /// addition to the enum wire forms.
    #[test]
    fn serde_roundtrip_matrix_covers_every_proposal_and_status() {
        for proposal in [
            CullProposal::Keep,
            CullProposal::Review,
            CullProposal::RejectCandidate,
        ] {
            for status in [
                CullingStatus::Valid,
                CullingStatus::Stale,
                CullingStatus::Missing,
                CullingStatus::Corrupt,
            ] {
                let mut value = section();
                value.proposal = proposal;
                value.status = status;
                value.error = matches!(
                    status,
                    CullingStatus::Stale | CullingStatus::Missing | CullingStatus::Corrupt
                )
                .then(|| "artifact no longer usable".to_string());
                validate_culling_section(&value)
                    .unwrap_or_else(|e| panic!("{proposal:?}/{status:?} must be valid: {e}"));
                let json = serde_json::to_string(&value).unwrap();
                let decoded: CullingSection = serde_json::from_str(&json).unwrap();
                assert_eq!(decoded, value);
                assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
                // The status enum serializes at its documented pointer.
                let raw: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert!(raw["status"].is_string());
            }
        }
    }

    #[test]
    fn onnx_analyzer_roundtrips_with_both_legal_hash_pins() {
        for hash in [
            CULLING_PENDING_MODEL_HASH.to_string(),
            format!("{CULLING_SHA256_PREFIX}{}", "ab".repeat(32)),
        ] {
            let mut value = section();
            value.identity.analyzer = CullingAnalyzer {
                kind: CullingAnalyzerKind::Onnx,
                name: "lumina-cull-onnx".into(),
                version: "2".into(),
                model_hash: Some(hash.clone()),
                extras: Extras::new(),
            };
            validate_culling_section(&value).expect("legal ONNX pin");
            let json = serde_json::to_string(&value).unwrap();
            let decoded: CullingSection = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, value);
            assert_eq!(decoded.identity.analyzer.kind, CullingAnalyzerKind::Onnx);
            assert_eq!(
                decoded.identity.analyzer.model_hash.as_deref(),
                Some(hash.as_str())
            );
        }
    }

    #[test]
    fn unknown_fields_and_open_reason_registry_roundtrip() {
        let mut value = section();
        value.reasons.push("future_signal".into());
        value
            .extras
            .insert("future_culling".into(), serde_json::Value::from(1));
        validate_culling_section(&value).expect("unknown reason codes are legal");
        let json = serde_json::to_string(&value).unwrap();
        let decoded: CullingSection = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, value);
    }
}
