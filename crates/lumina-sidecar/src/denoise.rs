//! LRPAR-G14-DENOISE-20: additive `recipe.adjustments.denoise_ai` schema
//! (Release 2.0).
//!
//! SOLL: `feature/decisions/LRPAR-G14-DENOISE-20.md` §5
//! („Rezept-Stufen-Vorschlag + Persistenz"). KI-Denoise is an optional,
//! additive stage for release 2.0 — it never replaces the manual F-096
//! noise reduction and never silently changes the render result:
//!
//! - The field is additive in schema v2. An absent key (or `None`) is
//!   identity: MVP recipes render byte-identically and need no migration.
//! - `strength == 0` and `enabled == false` are identity as well (no model,
//!   no inference, no error).
//! - The denoised RGB result lives in the binary sidecar
//!   (`<original>.lumina.zdata`) and is referenced only: no uncompressed
//!   float/pixel array is ever inlined in the JSON.
//! - Every deviation (unknown version, out-of-range strength, malformed
//!   model hash/digest, unsafe path) is rejected loudly — never clipped,
//!   defaulted or reinterpreted.
//!
//! Field-shape decision (documented deviation from the §5 sketch): the
//! decision's minimal `artifact` sketch (`path`, `sha256`, `kind`) is
//! superseded by the normative portable-artifact contract of
//! `feature/architecture/sidecar.md` § Persistenzregeln / `Agents.md`, which
//! requires relative path, format, checksum, resolution, channel type and
//! data version for every binary artifact reference. This module therefore
//! carries the full reference plus `kind = "denoise_rgb"` (the decision's
//! kind value). The checksum is the BLAKE3 digest over the uncompressed RGB
//! stream (determinism rule of §5), named `checksum` for consistency with the
//! existing `ArtifactReference`/`GenerativeArtifactRef` links.
//!
//! Scope: schema + validation only. The pipeline stage, zdata payload codec,
//! ONNX wiring and CLI/GUI are later slices (§8).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{invalid, validate_name, validate_relative_path, Extras, SidecarError};

/// Current denoise-recipe schema version. Foreign versions are rejected
/// loudly (pre-MVP: no back-compat obligation, but always versioned).
pub const DENOISE_AI_VERSION: u8 = 1;

/// The persisted artefact kind of a denoised RGB payload (§5).
pub const DENOISE_ARTIFACT_KIND: &str = "denoise_rgb";

/// F-078 gate marker: until licence-checked, hash-pinned weights exist, the
/// manifest carries `pending-integration` instead of a `sha256:<hex>` pin.
pub const DENOISE_PENDING_MODEL_HASH: &str = "pending-integration";
/// Prefix of the SHA-256 hash contract (`sha256:<64 lowercase hex>`).
pub const DENOISE_SHA256_PREFIX: &str = "sha256:";
/// Hex length of a SHA-256 digest.
pub const DENOISE_HASH_HEX_LEN: usize = 64;

/// Discriminator of the persisted artifact. Only `denoise_rgb` is defined;
/// any other value is a loud parse error (never silently reinterpreted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenoiseArtifactKind {
    DenoiseRgb,
}

/// Portable reference to a denoised RGB payload in the sidecar's
/// `.lumina.zdata` bundle (relative path, format, BLAKE3 checksum over the
/// uncompressed stream, resolution, channel type, data version). Absolute
/// paths are forbidden; a moved bundle stays valid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DenoiseArtifactRef {
    pub kind: DenoiseArtifactKind,
    pub relative_path: String,
    pub format: String,
    pub checksum: String,
    pub width: u32,
    pub height: u32,
    pub channels: String,
    pub data_version: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Reproducible denoiser identity (name/version/hash). `model_hash` is either
/// a real `sha256:<64 hex>` pin or the explicit `pending-integration` F-078
/// gate marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DenoiseModelIdentity {
    pub name: String,
    pub version: String,
    pub model_hash: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Additive `recipe.adjustments.denoise_ai` stage. `None` on `EditRecipe` is
/// identity; a present value with `enabled == false` or `strength == 0` is
/// identity as well (see [`DenoiseAi::is_identity`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DenoiseAi {
    pub version: u8,
    pub enabled: bool,
    pub model: DenoiseModelIdentity,
    /// Digest of resolution/tile/overlap/normalisation/tensor names
    /// (`sha256:<64 lowercase hex>`). Any change invalidates persisted
    /// artefacts visibly.
    pub input_spec_digest: String,
    /// Blending strength `0..=1` (`0` = identity, no inference needed).
    pub strength: f32,
    /// Edge/detail protection `0..=1`.
    pub preserve_detail: f32,
    /// Persisted denoised RGB artefact; `None` before the (explicit)
    /// inference run means `missing` — never a silent heuristic substitute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<DenoiseArtifactRef>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

impl DenoiseAi {
    /// `true` when this stage is a no-op: disabled or zero strength. Such a
    /// state requires neither a model nor an artefact and is never an error.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        !self.enabled || self.strength == 0.0
    }

    /// Loud validation; see [`validate_denoise_ai`].
    pub fn validate(&self) -> Result<(), SidecarError> {
        validate_denoise_ai(self)
    }
}

/// Loud `sha256:<64 lowercase hex>` contract (model hash and input-spec
/// digest).
pub fn validate_denoise_sha256(field: &str, value: &str) -> Result<(), SidecarError> {
    let Some(hex) = value.strip_prefix(DENOISE_SHA256_PREFIX) else {
        return invalid(format!(
            "{field} must be `{DENOISE_SHA256_PREFIX}<{DENOISE_HASH_HEX_LEN} lowercase hex>`"
        ));
    };
    if hex.len() != DENOISE_HASH_HEX_LEN
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return invalid(format!(
            "{field} must be `{DENOISE_SHA256_PREFIX}<{DENOISE_HASH_HEX_LEN} lowercase hex>`"
        ));
    }
    Ok(())
}

fn validate_denoise_model_hash(field: &str, value: &str) -> Result<(), SidecarError> {
    if value == DENOISE_PENDING_MODEL_HASH {
        return Ok(());
    }
    validate_denoise_sha256(field, value)
}

fn validate_denoise_artifact(artifact: &DenoiseArtifactRef) -> Result<(), SidecarError> {
    validate_relative_path("denoise_ai artifact relative_path", &artifact.relative_path)?;
    validate_name("denoise_ai artifact format", &artifact.format)?;
    validate_name("denoise_ai artifact checksum", &artifact.checksum)?;
    validate_name("denoise_ai artifact channels", &artifact.channels)?;
    validate_name("denoise_ai artifact data_version", &artifact.data_version)?;
    if artifact.width == 0 || artifact.height == 0 {
        return invalid("denoise_ai artifact resolution must be non-zero");
    }
    Ok(())
}

/// Loud validation of the denoise stage: version pin, complete model
/// identity, hash/digest contracts and bounded strengths. Every deviation is
/// rejected — never clipped or completed with a default.
pub fn validate_denoise_ai(denoise: &DenoiseAi) -> Result<(), SidecarError> {
    if denoise.version != DENOISE_AI_VERSION {
        return invalid(format!(
            "unsupported denoise_ai.version {} (expected {DENOISE_AI_VERSION})",
            denoise.version
        ));
    }
    validate_name("denoise_ai.model.name", &denoise.model.name)?;
    validate_name("denoise_ai.model.version", &denoise.model.version)?;
    validate_denoise_model_hash("denoise_ai.model.model_hash", &denoise.model.model_hash)?;
    validate_denoise_sha256("denoise_ai.input_spec_digest", &denoise.input_spec_digest)?;
    if !denoise.strength.is_finite() || !(0.0..=1.0).contains(&denoise.strength) {
        return invalid("denoise_ai.strength must be finite within 0..=1");
    }
    if !denoise.preserve_detail.is_finite() || !(0.0..=1.0).contains(&denoise.preserve_detail) {
        return invalid("denoise_ai.preserve_detail must be finite within 0..=1");
    }
    if let Some(artifact) = &denoise.artifact {
        validate_denoise_artifact(artifact)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        load_sidecar, migrate_json, save_sidecar, EditRecipe, Extras, SidecarDocument,
        SourceIdentity,
    };
    use std::collections::BTreeMap;

    fn sha256(fill: u8) -> String {
        format!(
            "{DENOISE_SHA256_PREFIX}{}",
            format!("{fill:02x}").repeat(32)
        )
    }

    fn denoise() -> DenoiseAi {
        DenoiseAi {
            version: DENOISE_AI_VERSION,
            enabled: true,
            model: DenoiseModelIdentity {
                name: "nafnet-srgb".into(),
                version: "1.0".into(),
                model_hash: DENOISE_PENDING_MODEL_HASH.into(),
                extras: Extras::new(),
            },
            input_spec_digest: sha256(0xab),
            strength: 0.5,
            preserve_detail: 0.5,
            artifact: Some(DenoiseArtifactRef {
                kind: DenoiseArtifactKind::DenoiseRgb,
                relative_path: "IMG_0001.ARW.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: "cd".repeat(32),
                width: 6000,
                height: 4000,
                channels: "rgb8".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            }),
            extras: Extras::new(),
        }
    }

    fn document_with_denoise() -> SidecarDocument {
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].recipe.denoise_ai = Some(denoise());
        document
    }

    fn source() -> SourceIdentity {
        SourceIdentity {
            relative_name: "IMG_0001.ARW".into(),
            content_hash: "blake3:abc".into(),
            byte_length: 42,
            modified_at: None,
            raw_format: "ARW".into(),
            orientation: 1,
            decode_fingerprint: crate::DecodeFingerprint {
                decoder: "libraw".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_fingerprint: crate::GeometryFingerprint {
                width: 6000,
                height: 4000,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            extras: Extras::new(),
        }
    }

    #[test]
    fn absent_field_is_identity_and_serializes_absent() {
        let recipe = EditRecipe::default();
        assert!(recipe.denoise_ai.is_none());
        let json = serde_json::to_string(&recipe).unwrap();
        assert!(
            !json.contains("denoise_ai"),
            "identity must not serialize the key"
        );
        // A present identity value is a no-op too.
        let mut identity = denoise();
        identity.enabled = false;
        assert!(identity.is_identity());
        let mut zero = denoise();
        zero.strength = 0.0;
        assert!(zero.is_identity());
    }

    #[test]
    fn denoise_ai_roundtrips_nested_under_adjustments() {
        let document = document_with_denoise();
        let json = document.to_json().unwrap();
        assert!(json.contains("\"denoise_ai\""), "nested in adjustments");
        assert!(json.contains(DENOISE_ARTIFACT_KIND));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, document);
        assert_eq!(decoded.to_json().unwrap(), json);
    }

    #[test]
    fn legacy_recipe_without_denoise_ai_loads_as_none() {
        let legacy = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{"exposure":0.5}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let document = SidecarDocument::from_json(legacy).unwrap();
        assert!(document.virtual_copies[0].recipe.denoise_ai.is_none());
        let out = document.to_json().unwrap();
        assert!(!out.contains("denoise_ai"));
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["exposure"],
            0.5
        );
    }

    #[test]
    fn explicit_v1_to_v2_migration_preserves_legacy_recipe_without_denoise_ai() {
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut legacy: serde_json::Value =
            serde_json::from_str(&document.to_json().unwrap()).unwrap();
        legacy["schema_version"] = serde_json::Value::from(1);
        let migrated = migrate_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
        let decoded = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(decoded.schema_version, crate::SCHEMA_VERSION);
        assert!(decoded.virtual_copies[0].recipe.denoise_ai.is_none());
    }

    #[test]
    fn denoise_ai_file_roundtrip_is_atomic_and_reloadable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("IMAGE.lumina.json");
        let document = document_with_denoise();
        save_sidecar(&path, &document).unwrap();
        assert_eq!(load_sidecar(&path).unwrap(), document);
    }

    #[test]
    fn denoise_recovery_sweeps_crashed_temporary_and_keeps_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("IMAGE.lumina.json");
        let document = document_with_denoise();
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
    fn unknown_artifact_kind_is_a_loud_parse_error() {
        let mut value: serde_json::Value =
            serde_json::from_str(&document_with_denoise().to_json().unwrap()).unwrap();
        value["virtual_copies"][0]["recipe"]["adjustments"]["denoise_ai"]["artifact"]["kind"] =
            serde_json::Value::from("upscale_rgb");
        let err = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap())
            .expect_err("unknown kind rejected");
        assert!(matches!(err, SidecarError::Json(_)), "got: {err}");
    }

    #[test]
    fn denoise_validation_matrix_rejects_every_deviation() {
        let cases: [fn(&mut DenoiseAi); 12] = [
            |d| d.version = 2,
            |d| d.model.name.clear(),
            |d| d.model.version = "  ".into(),
            |d| d.model.model_hash = "dummy".into(),
            |d| d.model.model_hash = format!("{DENOISE_SHA256_PREFIX}{}", "AB".repeat(32)),
            |d| d.input_spec_digest = "not-a-digest".into(),
            |d| d.strength = 1.5,
            |d| d.strength = f32::NAN,
            |d| d.preserve_detail = -0.1,
            |d| d.artifact.as_mut().unwrap().relative_path = "/abs/out.bin".into(),
            |d| d.artifact.as_mut().unwrap().width = 0,
            |d| d.artifact.as_mut().unwrap().checksum.clear(),
        ];
        for mutate in cases {
            let mut value = denoise();
            mutate(&mut value);
            assert!(
                validate_denoise_ai(&value).is_err(),
                "denoise contract violation must be rejected loudly"
            );
        }
        validate_denoise_ai(&denoise()).expect("sample is valid");
    }

    #[test]
    fn unknown_fields_roundtrip_and_absent_artifact_is_allowed() {
        let mut value = denoise();
        value.artifact = None;
        value
            .extras
            .insert("future_denoise".into(), serde_json::Value::from(7));
        validate_denoise_ai(&value).expect("artifact is optional");
        let json = serde_json::to_string(&value).unwrap();
        let decoded: DenoiseAi = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, value);
        assert!(!json.contains("\"artifact\""));
    }
}
