//! Versioned, portable domain types for a Lumina sidecar.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[cfg(feature = "zdata")]
mod zdata;
#[cfg(feature = "zdata")]
pub use zdata::{load_zdata, save_zdata, zdata_path_for, MaskTile, ZDataContainer, ZDataError};

pub const FORMAT: &str = "lumina-sidecar";
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_SIDECAR_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_VIRTUAL_COPIES: usize = 10_000;
pub type Extras = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodeFingerprint {
    pub decoder: String,
    pub version: String,
    pub parameters: BTreeMap<String, String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryFingerprint {
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub pixel_aspect_ratio: f32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisFingerprint {
    pub algorithm: String,
    pub version: String,
    pub input_fingerprint: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub content_hash: String,
    pub byte_length: u64,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub relative_name: String,
    pub content_hash: String,
    pub byte_length: u64,
    pub modified_at: Option<String>,
    pub raw_format: String,
    pub orientation: u8,
    pub decode_fingerprint: DecodeFingerprint,
    pub geometry_fingerprint: GeometryFingerprint,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub source: SourceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactReference {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub name: String,
    pub version: String,
    pub hash: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preprocessing {
    pub name: String,
    pub version: String,
    pub parameters: BTreeMap<String, String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateSystem {
    SourceOriented,
    ModelInput,
    Normalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskStatus {
    Valid,
    Stale,
    Missing,
    Corrupt,
    Pending,
}

/// The operation performed by a mask definition. `Source` is the default so
/// schema-1 definitions that predate operational masks remain readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MaskOperation {
    #[default]
    Source,
    Union,
    Intersect,
    Subtract,
    Invert,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskDefinition {
    pub id: String,
    pub name: String,
    pub source_fingerprint: SourceFingerprint,
    pub decode_context: DecodeFingerprint,
    pub geometry_context: GeometryFingerprint,
    pub model: ModelIdentity,
    pub inference_resolution: Resolution,
    pub preprocessing: Preprocessing,
    pub rescaling_method: String,
    pub rescaling_parameters: BTreeMap<String, String>,
    pub coordinate_system: CoordinateSystem,
    pub status: MaskStatus,
    pub created_at: String,
    pub generator_version: String,
    pub error_text: Option<String>,
    pub artifact: Option<ArtifactReference>,
    #[serde(default)]
    pub operation: MaskOperation,
    pub references: Vec<MaskReference>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskReference {
    pub copy_id: String,
    pub mask_id: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskLayer {
    pub id: String,
    pub mask: MaskReference,
    pub inverted: bool,
    pub feather: f32,
    pub blur: f32,
    pub density: f32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditRecipe {
    #[serde(default = "default_recipe_version")]
    pub recipe_version: String,
    pub adjustments: BTreeMap<String, f64>,
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub auto_features: AutoFeatures,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

fn default_recipe_version() -> String {
    "1".into()
}

impl Default for EditRecipe {
    fn default() -> Self {
        Self {
            recipe_version: default_recipe_version(),
            adjustments: BTreeMap::new(),
            options: BTreeMap::new(),
            auto_features: AutoFeatures::default(),
            extras: Extras::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoFeatures {
    #[serde(default)]
    pub enable_auto_tone: bool,
    #[serde(default)]
    pub match_total_exposure: bool,
    #[serde(default = "default_target_luminance")]
    pub target_luminance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_exposure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_contrast: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_exposure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
}

impl Default for AutoFeatures {
    fn default() -> Self {
        Self {
            enable_auto_tone: false,
            match_total_exposure: false,
            target_luminance: default_target_luminance(),
            auto_exposure: None,
            auto_contrast: None,
            matched_exposure: None,
            analysis_fingerprint: None,
        }
    }
}

fn default_target_luminance() -> f64 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub recipe: EditRecipe,
    pub recorded_at: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub recipe: EditRecipe,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRecord {
    pub id: String,
    pub relative_path: String,
    pub format: String,
    pub exported_at: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualCopy {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub recipe: EditRecipe,
    pub mask_library: Vec<MaskDefinition>,
    pub mask_layers: Vec<MaskLayer>,
    pub history: Vec<HistoryEntry>,
    pub export_records: Vec<ExportRecord>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarDocument {
    pub format: String,
    pub schema_version: u32,
    pub source: SourceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
    pub pipeline_version: String,
    pub virtual_copies: Vec<VirtualCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_virtual_copies: Vec<VirtualCopy>,
    pub presets: Vec<Preset>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Error, PartialEq)]
pub enum SidecarError {
    #[error("sidecar file is missing: {0}")]
    Missing(String),
    #[error("sidecar I/O failed while {operation} `{path}`: {message}")]
    Io {
        operation: String,
        path: String,
        message: String,
    },
    #[error("invalid sidecar JSON: {0}")]
    Json(String),
    #[error("invalid sidecar: {0}")]
    Invalid(String),
    #[error("sidecar changed concurrently: {0}")]
    Conflict(String),
    #[error("XMP is not supported by Lumina sidecar schema v1")]
    XmpUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    Unchanged,
    SourceChanged,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    Available,
    Missing,
}

/// Returns the sidecar path immediately next to `source`.
pub fn sidecar_path_for(source: &Path) -> PathBuf {
    let filename = source
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    source.with_file_name(format!("{filename}.lumina.json"))
}

pub fn load_sidecar(path: &Path) -> Result<SidecarDocument, SidecarError> {
    let json = fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SidecarError::Missing(path.display().to_string())
        } else {
            io_error("reading", path, error)
        }
    })?;
    SidecarDocument::from_json(&json)
}

/// Validates before writing and replaces the destination only after the complete
/// temporary file has been flushed and synced. Output and sidecar are not a
/// two-file transaction; crash recovery for that pair remains future work.
pub fn save_sidecar(path: &Path, document: &SidecarDocument) -> Result<(), SidecarError> {
    let json = document.to_json()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let filename = filename.as_deref().unwrap_or("sidecar");
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error("creating temporary file", parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    let result = (|| {
        temporary
            .write_all(json.as_bytes())
            .map_err(|error| io_error("writing temporary file", &temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error("flushing temporary file", &temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error("syncing temporary file", &temporary_path, error))?;
        temporary
            .persist(path)
            .map_err(|error| io_error("renaming temporary file", path, error.error))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| io_error("syncing sidecar directory", parent, error))?;
        Ok(())
    })();
    result
}

pub fn save_sidecar_if_unchanged(
    path: &Path,
    document: &SidecarDocument,
    expected_revision: Option<&str>,
) -> Result<String, SidecarError> {
    if let Some(expected) = expected_revision {
        if path.exists() {
            let current = document_revision(&load_sidecar(path)?)?;
            if current != expected {
                return Err(SidecarError::Conflict(path.display().to_string()));
            }
        }
    }
    save_sidecar(path, document)?;
    document_revision(document)
}

pub fn document_revision(document: &SidecarDocument) -> Result<String, SidecarError> {
    let json = document.to_json()?;
    Ok(blake3::hash(json.as_bytes()).to_hex().to_string())
}

pub fn source_status(path: &Path, source: &SourceIdentity) -> Result<SourceStatus, SidecarError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceStatus::Missing)
        }
        Err(error) => return Err(io_error("reading source", path, error)),
    };
    let metadata =
        fs::metadata(path).map_err(|error| io_error("reading source metadata", path, error))?;
    let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    if metadata.len() == source.byte_length && hash == source.content_hash {
        Ok(SourceStatus::Unchanged)
    } else {
        Ok(SourceStatus::SourceChanged)
    }
}

pub fn artifact_status(bundle_root: &Path, artifact: &ArtifactReference) -> ArtifactStatus {
    bundle_root
        .join(&artifact.relative_path)
        .is_file()
        .then_some(ArtifactStatus::Available)
        .unwrap_or(ArtifactStatus::Missing)
}

pub fn xmp_supported() -> bool {
    false
}

pub fn migrate_json(json: &str) -> Result<String, SidecarError> {
    let document = SidecarDocument::from_json(json)?;
    document.to_json()
}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> SidecarError {
    SidecarError::Io {
        operation: operation.into(),
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

impl SidecarDocument {
    pub fn new(source: SourceIdentity, pipeline_version: impl Into<String>) -> Self {
        Self {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            source,
            analysis_fingerprint: None,
            pipeline_version: pipeline_version.into(),
            virtual_copies: vec![VirtualCopy {
                id: "vc-original".into(),
                name: "Original".into(),
                is_default: true,
                recipe: EditRecipe::default(),
                mask_library: vec![],
                mask_layers: vec![],
                history: vec![],
                export_records: vec![],
                extras: Extras::new(),
            }],
            deleted_virtual_copies: vec![],
            presets: vec![],
            extras: Extras::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, SidecarError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| SidecarError::Json(e.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Self, SidecarError> {
        if json.len() > MAX_SIDECAR_BYTES {
            return Err(SidecarError::Invalid("sidecar exceeds size limit".into()));
        }
        let mut value: Value =
            serde_json::from_str(json).map_err(|e| SidecarError::Json(e.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| SidecarError::Invalid("missing schema_version".into()))?;
        if version == 0 {
            value["schema_version"] = Value::from(SCHEMA_VERSION);
        } else if version != u64::from(SCHEMA_VERSION) {
            return Err(SidecarError::Invalid("unsupported schema_version".into()));
        }
        let document: Self =
            serde_json::from_value(value).map_err(|e| SidecarError::Json(e.to_string()))?;
        document.validate()?;
        Ok(document)
    }

    pub fn duplicate_virtual_copy(
        &mut self,
        source_id: &str,
        new_id: impl Into<String>,
        new_name: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let mut copy = self
            .virtual_copies
            .iter()
            .find(|copy| copy.id == source_id)
            .cloned()
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{source_id}`")))?;
        copy.id = new_id.into();
        copy.name = new_name.into();
        copy.is_default = false;
        if self
            .virtual_copies
            .iter()
            .any(|candidate| candidate.id == copy.id)
            || self
                .deleted_virtual_copies
                .iter()
                .any(|candidate| candidate.id == copy.id)
        {
            return invalid(format!("duplicate virtual copy id `{}`", copy.id));
        }
        self.virtual_copies.push(copy);
        self.validate()
    }

    pub fn rename_virtual_copy(
        &mut self,
        id: &str,
        name: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let copy = self
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{id}`")))?;
        copy.name = name.into();
        self.validate()
    }

    pub fn sort_virtual_copies(&mut self) {
        self.virtual_copies
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    }

    pub fn delete_virtual_copy(&mut self, id: &str) -> Result<(), SidecarError> {
        if id == "vc-original" {
            return invalid("the default virtual copy cannot be deleted");
        }
        let index = self
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{id}`")))?;
        self.deleted_virtual_copies
            .push(self.virtual_copies.remove(index));
        self.validate()
    }

    pub fn restore_virtual_copy(&mut self, id: &str) -> Result<(), SidecarError> {
        let index = self
            .deleted_virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown deleted virtual copy `{id}`")))?;
        self.virtual_copies
            .push(self.deleted_virtual_copies.remove(index));
        self.validate()
    }

    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.format != FORMAT {
            return invalid("format must be `lumina-sidecar`");
        }
        if self.schema_version != SCHEMA_VERSION {
            return invalid("unsupported schema_version");
        }
        validate_name("pipeline_version", &self.pipeline_version)?;
        validate_name("source.relative_name", &self.source.relative_name)?;
        validate_relative_path("source.relative_name", &self.source.relative_name)?;
        if !(1..=8).contains(&self.source.orientation) {
            return invalid("source.orientation must be between 1 and 8");
        }
        if self.virtual_copies.is_empty() {
            return invalid("at least one virtual copy is required");
        }
        if self.virtual_copies.len() + self.deleted_virtual_copies.len() > MAX_VIRTUAL_COPIES {
            return invalid("sidecar exceeds virtual copy limit");
        }
        let mut copy_ids = BTreeSet::new();
        let mut defaults = 0;
        for copy in &self.virtual_copies {
            validate_name("virtual copy id", &copy.id)?;
            validate_name("virtual copy name", &copy.name)?;
            validate_name("recipe_version", &copy.recipe.recipe_version)?;
            if !copy_ids.insert(&copy.id) {
                return invalid(format!("duplicate virtual copy id `{}`", copy.id));
            }
            defaults += usize::from(copy.is_default);
            let mut mask_ids = BTreeSet::new();
            for mask in &copy.mask_library {
                validate_name("mask id", &mask.id)?;
                validate_name("mask name", &mask.name)?;
                validate_name("mask rescaling_method", &mask.rescaling_method)?;
                validate_name("mask generator_version", &mask.generator_version)?;
                if !mask_ids.insert(&mask.id) {
                    return invalid(format!(
                        "duplicate mask id `{}` in copy `{}`",
                        mask.id, copy.id
                    ));
                }
                if let Some(a) = &mask.artifact {
                    validate_artifact(a)?;
                }
                let arity_is_valid = match mask.operation {
                    MaskOperation::Source => mask.references.is_empty(),
                    MaskOperation::Invert => mask.references.len() == 1,
                    MaskOperation::Subtract => mask.references.len() == 2,
                    MaskOperation::Union | MaskOperation::Intersect => mask.references.len() >= 2,
                };
                if !arity_is_valid {
                    return invalid(format!(
                        "mask `{}/{}` operation `{}` has invalid input arity ({})",
                        copy.id,
                        mask.id,
                        serde_json::to_string(&mask.operation).unwrap_or_default(),
                        mask.references.len()
                    ));
                }
                for reference in &mask.references {
                    validate_name("mask reference copy_id", &reference.copy_id)?;
                    validate_name("mask reference mask_id", &reference.mask_id)?;
                }
            }
            for export in &copy.export_records {
                validate_name("export id", &export.id)?;
                validate_relative_path("export relative_path", &export.relative_path)?;
                validate_name("export format", &export.format)?;
            }
            validate_unique_ids("mask layer", &copy.mask_layers, |layer| &layer.id)?;
            validate_unique_ids("history entry", &copy.history, |entry| &entry.id)?;
            validate_unique_ids("export record", &copy.export_records, |export| &export.id)?;
            for layer in &copy.mask_layers {
                validate_name("mask layer id", &layer.id)?;
            }
            for entry in &copy.history {
                validate_name("history entry id", &entry.id)?;
            }
        }
        for copy in &self.deleted_virtual_copies {
            validate_name("deleted virtual copy id", &copy.id)?;
            validate_name("deleted virtual copy name", &copy.name)?;
            validate_name("recipe_version", &copy.recipe.recipe_version)?;
            if !copy_ids.insert(&copy.id) {
                return invalid(format!("duplicate virtual copy id `{}`", copy.id));
            }
        }
        validate_unique_ids("preset", &self.presets, |preset| &preset.id)?;
        for preset in &self.presets {
            validate_name("preset id", &preset.id)?;
        }
        for copy in &self.virtual_copies {
            for layer in &copy.mask_layers {
                let target = self
                    .virtual_copies
                    .iter()
                    .find(|candidate| candidate.id == layer.mask.copy_id)
                    .ok_or_else(|| {
                        SidecarError::Invalid(format!(
                            "mask layer `{}` references unknown copy `{}`",
                            layer.id, layer.mask.copy_id
                        ))
                    })?;
                if !target
                    .mask_library
                    .iter()
                    .any(|mask| mask.id == layer.mask.mask_id)
                {
                    return invalid(format!(
                        "mask layer `{}` references unknown mask `{}/{}`",
                        layer.id, layer.mask.copy_id, layer.mask.mask_id
                    ));
                }
            }
        }
        if defaults != 1 {
            return invalid("exactly one default virtual copy is required");
        }
        if !self
            .virtual_copies
            .iter()
            .any(|c| c.id == "vc-original" && c.is_default)
        {
            return invalid("`vc-original` must be the default virtual copy");
        }
        self.validate_mask_graph(&copy_ids)
    }

    fn validate_mask_graph(&self, copy_ids: &BTreeSet<&String>) -> Result<(), SidecarError> {
        let mut nodes = BTreeSet::new();
        let mut edges = BTreeMap::<(String, String), Vec<(String, String)>>::new();
        for copy in &self.virtual_copies {
            for mask in &copy.mask_library {
                let node = (copy.id.clone(), mask.id.clone());
                nodes.insert(node.clone());
                for reference in &mask.references {
                    edges
                        .entry(node.clone())
                        .or_default()
                        .push((reference.copy_id.clone(), reference.mask_id.clone()));
                }
            }
        }
        for (from, targets) in &edges {
            for target in targets {
                if !copy_ids.contains(&target.0) || !nodes.contains(target) {
                    return invalid(format!(
                        "mask `{}/{}' references unknown mask `{}/{}`",
                        from.0, from.1, target.0, target.1
                    ));
                }
                if from == target {
                    return invalid(format!("mask `{}/{}' references itself", from.0, from.1));
                }
            }
        }
        fn visit(
            node: &(String, String),
            edges: &BTreeMap<(String, String), Vec<(String, String)>>,
            visiting: &mut BTreeSet<(String, String)>,
            visited: &mut BTreeSet<(String, String)>,
        ) -> Result<(), SidecarError> {
            if visiting.contains(node) {
                return invalid(format!(
                    "mask graph contains a cycle at `{}/{}`",
                    node.0, node.1
                ));
            }
            if !visited.insert(node.clone()) {
                return Ok(());
            }
            visiting.insert(node.clone());
            if let Some(targets) = edges.get(node) {
                for target in targets {
                    visit(target, edges, visiting, visited)?;
                }
            }
            visiting.remove(node);
            Ok(())
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for node in nodes {
            visit(&node, &edges, &mut visiting, &mut visited)?;
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}
fn validate_name(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.trim().is_empty() {
        invalid(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_unique_ids<T>(
    kind: &str,
    values: &[T],
    id: impl Fn(&T) -> &String,
) -> Result<(), SidecarError> {
    let mut ids = BTreeSet::new();
    for value in values {
        let value_id = id(value);
        if !ids.insert(value_id) {
            return invalid(format!("duplicate {kind} id `{value_id}`"));
        }
    }
    Ok(())
}

fn validate_relative_path(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || value.starts_with("//")
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return invalid(format!("{field} must be a safe portable relative path"));
    }
    Ok(())
}
fn validate_artifact(a: &ArtifactReference) -> Result<(), SidecarError> {
    validate_relative_path("artifact relative_path", &a.relative_path)?;
    validate_name("artifact format", &a.format)?;
    validate_name("artifact checksum", &a.checksum)?;
    validate_name("artifact channels", &a.channels)?;
    validate_name("artifact data_version", &a.data_version)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> SourceIdentity {
        SourceIdentity {
            relative_name: "IMG_0001.ARW".into(),
            content_hash: "sha256:x".into(),
            byte_length: 42,
            modified_at: None,
            raw_format: "ARW".into(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "test".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 10,
                height: 20,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            extras: Extras::new(),
        }
    }
    fn mask(id: &str) -> MaskDefinition {
        MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "sha256:x".into(),
                byte_length: 42,
                extras: Extras::new(),
            },
            decode_context: source().decode_fingerprint.clone(),
            geometry_context: source().geometry_fingerprint.clone(),
            model: ModelIdentity {
                name: "model".into(),
                version: "1".into(),
                hash: "sha256:model".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width: 10,
                height: 20,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "standard".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "bilinear".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Valid,
            created_at: "2026-01-01T00:00:00Z".into(),
            generator_version: "generator-1".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            extras: Extras::new(),
        }
    }
    #[test]
    fn complete_roundtrip() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "scene-analysis".into(),
            version: "2.1".into(),
            input_fingerprint: "sha256:analysis-input".into(),
            extras: Extras::from([("analysis_extra".into(), Value::from(true))]),
        });
        d.extras
            .insert("future_root".into(), Value::from("preserved"));

        let mut source_mask = mask("a");
        source_mask
            .source_fingerprint
            .extras
            .insert("future_source_fingerprint".into(), Value::from(7));
        source_mask
            .decode_context
            .parameters
            .insert("quality".into(), "high".into());
        source_mask.geometry_context.pixel_aspect_ratio = 1.25;
        source_mask
            .model
            .extras
            .insert("future_model".into(), Value::from("kept"));
        source_mask.inference_resolution.width = 512;
        source_mask
            .preprocessing
            .parameters
            .insert("mean".into(), "0.5".into());
        source_mask.rescaling_method = "lanczos".into();
        source_mask
            .rescaling_parameters
            .insert("radius".into(), "3".into());
        source_mask.coordinate_system = CoordinateSystem::Normalized;
        source_mask.status = MaskStatus::Corrupt;
        source_mask.created_at = "2026-02-03T04:05:06Z".into();
        source_mask.generator_version = "segmenter-2.4".into();
        source_mask.error_text = Some("model output checksum mismatch".into());
        source_mask.artifact = Some(ArtifactReference {
            relative_path: "masks/a.zdata".into(),
            format: "zdata-mask".into(),
            checksum: "sha256:artifact".into(),
            width: 512,
            height: 256,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        });
        source_mask
            .extras
            .insert("future_mask".into(), Value::from("kept"));
        d.virtual_copies[0].mask_library.push(source_mask);
        d.virtual_copies.push(VirtualCopy {
            id: "vc-bw".into(),
            name: "B&W".into(),
            is_default: false,
            recipe: EditRecipe {
                recipe_version: "1".into(),
                adjustments: BTreeMap::from([("exposure".into(), 1.25)]),
                options: BTreeMap::from([("profile".into(), "neutral".into())]),
                auto_features: AutoFeatures::default(),
                extras: Extras::from([("future_recipe".into(), Value::from(42))]),
            },
            mask_library: vec![],
            mask_layers: vec![MaskLayer {
                id: "layer".into(),
                mask: MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "a".into(),
                    extras: Extras::new(),
                },
                inverted: false,
                feather: 0.0,
                blur: 0.0,
                density: 1.0,
                extras: Extras::new(),
            }],
            history: vec![HistoryEntry {
                id: "h".into(),
                recipe: EditRecipe {
                    recipe_version: "1".into(),
                    adjustments: BTreeMap::from([("contrast".into(), -0.4)]),
                    options: BTreeMap::from([("source".into(), "preset".into())]),
                    auto_features: AutoFeatures::default(),
                    extras: Extras::new(),
                },
                recorded_at: Some("2026-02-03T04:05:06Z".into()),
                extras: Extras::from([("future_history".into(), Value::from(true))]),
            }],
            export_records: vec![ExportRecord {
                id: "e".into(),
                relative_path: "exports/out.jpg".into(),
                format: "jpeg".into(),
                exported_at: Some("2026-02-03T04:06:06Z".into()),
                extras: Extras::from([("future_export".into(), Value::from("kept"))]),
            }],
            extras: Extras::from([("future_copy".into(), Value::from(true))]),
        });
        d.presets.push(Preset {
            id: "preset-1".into(),
            name: "Monochrome Contrast".into(),
            recipe: EditRecipe {
                recipe_version: "1".into(),
                adjustments: BTreeMap::from([("highlights".into(), -0.75)]),
                options: BTreeMap::from([("curve".into(), "film".into())]),
                auto_features: AutoFeatures::default(),
                extras: Extras::new(),
            },
            extras: Extras::new(),
        });
        let json = d.to_json().unwrap();
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn empty_sidecar_roundtrip() {
        let d = SidecarDocument::new(source(), "pipeline-1");
        let json = d.to_json().unwrap();
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn auto_features_roundtrip_with_result_and_fingerprint() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let features = &mut d.virtual_copies[0].recipe.auto_features;
        features.enable_auto_tone = true;
        features.match_total_exposure = true;
        features.target_luminance = 0.42;
        features.auto_exposure = Some(1.25);
        features.auto_contrast = Some(-0.2);
        features.matched_exposure = Some(0.5);
        features.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint: "tone-rgba8-rec709:abc".into(),
            extras: Extras::new(),
        });
        let json = d.to_json().unwrap();
        assert!(json.contains("auto_exposure"));
        assert!(json.contains("tone-rgba8-rec709:abc"));
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn sidecar_path_keeps_full_source_name() {
        assert_eq!(
            sidecar_path_for(Path::new("/photos/IMG_0001.ARW")),
            PathBuf::from("/photos/IMG_0001.ARW.lumina.json")
        );
    }

    #[test]
    fn file_roundtrip_and_missing_case() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("photo.png");
        let path = sidecar_path_for(&source_path);
        let document = SidecarDocument::new(source(), "pipeline-1");
        save_sidecar(&path, &document).unwrap();
        assert_eq!(load_sidecar(&path).unwrap(), document);
        assert!(matches!(
            load_sidecar(&directory.path().join("missing.json")),
            Err(SidecarError::Missing(_))
        ));
    }

    #[test]
    fn corrupt_json_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("photo.png.lumina.json");
        std::fs::write(&path, b"{not-json").unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
    }

    #[test]
    fn unknown_fields_roundtrip() {
        let json = r#"{"format":"lumina-sidecar","schema_version":1,"pipeline_version":"p","source":{"relative_name":"x.raw","content_hash":"h","byte_length":1,"modified_at":null,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0},"future":42},"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[],"future_copy":true}],"presets":[],"future_root":"kept"}"#;
        let d = SidecarDocument::from_json(json).unwrap();
        let out = d.to_json().unwrap();
        assert!(out.contains("future_root"));
        assert!(out.contains("future_copy"));
        assert!(out.contains("\"future\": 42"));
    }

    #[test]
    fn schema_version_one_missing_operation_defaults_to_source() {
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].mask_library.push(mask("legacy"));
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["virtual_copies"][0]["mask_library"][0]
            .as_object_mut()
            .unwrap()
            .remove("operation");

        let legacy_json = serde_json::to_string(&value).unwrap();
        let decoded = SidecarDocument::from_json(&legacy_json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].mask_library[0].operation,
            MaskOperation::Source
        );
        let roundtripped: Value = serde_json::from_str(&decoded.to_json().unwrap()).unwrap();
        assert_eq!(
            roundtripped["virtual_copies"][0]["mask_library"][0]["operation"],
            "source"
        );
    }
    #[test]
    fn unsafe_paths_are_rejected() {
        for path in [
            "../outside",
            "a/../../x",
            "/tmp/x",
            "C:\\x",
            "C:/x",
            "\\\\server\\share\\x",
            "a\\b",
            "a/./b",
        ] {
            let mut d = SidecarDocument::new(source(), "p");
            d.source.relative_name = path.into();
            assert!(d.validate().is_err(), "{path}");
        }
    }
    #[test]
    fn default_and_original_are_exact() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].id = "other".into();
        assert!(d.validate().is_err());
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies.push(d.virtual_copies[0].clone());
        assert!(d.validate().is_err());
    }
    #[test]
    fn mask_cycles_and_invalid_targets_are_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut a = mask("a");
        let mut b = mask("b");
        a.operation = MaskOperation::Invert;
        b.operation = MaskOperation::Invert;
        a.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "b".into(),
            extras: Extras::new(),
        });
        b.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "a".into(),
            extras: Extras::new(),
        });
        d.virtual_copies[0].mask_library = vec![a, b];
        let error = d.validate().unwrap_err().to_string();
        assert!(error.contains("cycle"));
        d.virtual_copies[0].mask_library[0].references[0].mask_id = "missing".into();
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unknown mask"));
        d.virtual_copies[0].mask_library[0].references.clear();
        d.virtual_copies[0].mask_library[0].operation = MaskOperation::Source;
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "missing".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("mask layer"));
    }

    #[test]
    fn valid_cross_copy_mask_reference_is_accepted() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("source-mask"));
        d.virtual_copies.push(VirtualCopy {
            id: "vc-target".into(),
            name: "Target".into(),
            is_default: false,
            recipe: EditRecipe::default(),
            mask_library: vec![MaskDefinition {
                operation: MaskOperation::Invert,
                references: vec![MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "source-mask".into(),
                    extras: Extras::new(),
                }],
                ..mask("derived")
            }],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        });
        assert!(d.validate().is_ok());
    }

    #[test]
    fn cross_copy_mask_cycle_is_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(MaskDefinition {
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc-target".into(),
                mask_id: "target-mask".into(),
                extras: Extras::new(),
            }],
            ..mask("source-mask")
        });
        d.virtual_copies.push(VirtualCopy {
            id: "vc-target".into(),
            name: "Target".into(),
            is_default: false,
            recipe: EditRecipe::default(),
            mask_library: vec![MaskDefinition {
                operation: MaskOperation::Invert,
                references: vec![MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "source-mask".into(),
                    extras: Extras::new(),
                }],
                ..mask("target-mask")
            }],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("cycle"));
    }

    #[test]
    fn direct_mask_self_reference_is_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(MaskDefinition {
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "self".into(),
                extras: Extras::new(),
            }],
            ..mask("self")
        });
        let error = d.validate().unwrap_err().to_string();
        assert!(error.contains("references itself"));
    }

    #[test]
    fn mask_identity_fields_roundtrip() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut definition = mask("identity");
        definition.rescaling_method = "lanczos".into();
        definition
            .rescaling_parameters
            .insert("filter_radius".into(), "3".into());
        definition.generator_version = "segmenter-2.4".into();
        d.virtual_copies[0].mask_library.push(definition);
        let json = d.to_json().unwrap();
        assert!(json.contains("rescaling_method"));
        assert!(json.contains("generator_version"));
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn collection_ids_must_be_nonempty_and_unique() {
        let mut d = SidecarDocument::new(source(), "p");
        d.presets = vec![
            Preset {
                id: "preset".into(),
                name: "One".into(),
                recipe: EditRecipe::default(),
                extras: Extras::new(),
            },
            Preset {
                id: "preset".into(),
                name: "Two".into(),
                recipe: EditRecipe::default(),
                extras: Extras::new(),
            },
        ];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate preset id"));

        d.presets.clear();
        d.virtual_copies[0].mask_library.push(mask("layer-mask"));
        let layer = MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "layer-mask".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
        };
        d.virtual_copies[0].mask_layers = vec![layer.clone(), layer];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate mask layer id"));

        d.virtual_copies[0].mask_layers.clear();
        let history = HistoryEntry {
            id: "history".into(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras: Extras::new(),
        };
        d.virtual_copies[0].history = vec![history.clone(), history];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate history entry id"));

        d.virtual_copies[0].history.clear();
        let export = ExportRecord {
            id: "export".into(),
            relative_path: "exports/out.jpg".into(),
            format: "jpeg".into(),
            exported_at: None,
            extras: Extras::new(),
        };
        d.virtual_copies[0].export_records = vec![export.clone(), export];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate export record id"));
    }

    #[test]
    fn ids_must_be_nonempty() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].id.clear();
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("virtual copy id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("mask"));
        d.virtual_copies[0].mask_library[0].id.clear();
        assert!(d.validate().unwrap_err().to_string().contains("mask id"));

        let mut d = SidecarDocument::new(source(), "p");
        let mut referenced_mask = mask("referenced");
        referenced_mask.operation = MaskOperation::Invert;
        referenced_mask.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "mask".into(),
            extras: Extras::new(),
        });
        referenced_mask.references[0].copy_id.clear();
        d.virtual_copies[0].mask_library.push(referenced_mask);
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("mask reference copy_id"));

        let mut d = SidecarDocument::new(source(), "p");
        let mut referenced_mask = mask("referenced");
        referenced_mask.operation = MaskOperation::Invert;
        referenced_mask.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "mask".into(),
            extras: Extras::new(),
        });
        referenced_mask.references[0].mask_id.clear();
        d.virtual_copies[0].mask_library.push(referenced_mask);
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("mask reference mask_id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: String::new(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "mask".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
        });
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("mask layer id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].history.push(HistoryEntry {
            id: String::new(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras: Extras::new(),
        });
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("history entry id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].export_records.push(ExportRecord {
            id: String::new(),
            relative_path: "exports/out.jpg".into(),
            format: "jpeg".into(),
            exported_at: None,
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("export id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.presets.push(Preset {
            id: String::new(),
            name: "Preset".into(),
            recipe: EditRecipe::default(),
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("preset id"));
    }

    #[test]
    fn virtual_copy_lifecycle_preserves_independent_recipe() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 1.0);
        d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
            .unwrap();
        d.rename_virtual_copy("vc-copy", "Renamed").unwrap();
        d.virtual_copies.swap(0, 1);
        d.delete_virtual_copy("vc-copy").unwrap();
        assert_eq!(d.virtual_copies.len(), 1);
        d.restore_virtual_copy("vc-copy").unwrap();
        assert_eq!(d.virtual_copies[1].name, "Renamed");
        d.virtual_copies[1]
            .recipe
            .adjustments
            .insert("exposure".into(), -1.0);
        assert_ne!(
            d.virtual_copies[0].recipe.adjustments["exposure"],
            d.virtual_copies[1].recipe.adjustments["exposure"]
        );
        assert_eq!(
            d,
            SidecarDocument::from_json(&d.to_json().unwrap()).unwrap()
        );
    }

    #[test]
    fn migration_unknown_fields_and_incompatible_version() {
        let d = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        value["virtual_copies"][0]["recipe"]
            .as_object_mut()
            .unwrap()
            .remove("recipe_version");
        let migrated = migrate_json(&serde_json::to_string(&value).unwrap()).unwrap();
        let decoded = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(decoded.virtual_copies[0].recipe.recipe_version, "1");
        value["schema_version"] = Value::from(99);
        assert!(SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).is_err());
    }

    #[test]
    fn atomic_compare_and_swap_and_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let d = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &d, None).unwrap();
        let mut changed = d.clone();
        changed.virtual_copies[0].name = "Changed".into();
        assert!(save_sidecar_if_unchanged(&path, &changed, Some("wrong")).is_err());
        save_sidecar_if_unchanged(&path, &changed, Some(&revision)).unwrap();
        std::fs::write(
            directory.path().join(".image.lumina.json.tmp-crash"),
            b"partial",
        )
        .unwrap();
        assert_eq!(
            load_sidecar(&path).unwrap().virtual_copies[0].name,
            "Changed"
        );
    }

    #[test]
    fn source_and_artifact_conflicts_are_visible() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.raw");
        let bytes = b"source";
        std::fs::write(&source_path, bytes).unwrap();
        let mut identity = source();
        identity.byte_length = bytes.len() as u64;
        identity.content_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
        assert_eq!(
            source_status(&source_path, &identity).unwrap(),
            SourceStatus::Unchanged
        );
        std::fs::write(&source_path, b"changed").unwrap();
        assert_eq!(
            source_status(&source_path, &identity).unwrap(),
            SourceStatus::SourceChanged
        );
        std::fs::remove_file(&source_path).unwrap();
        assert_eq!(
            source_status(&source_path, &identity).unwrap(),
            SourceStatus::Missing
        );
        let artifact = ArtifactReference {
            relative_path: "masks/a.zdata".into(),
            format: "zdata".into(),
            checksum: "hash".into(),
            width: 1,
            height: 1,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        assert_eq!(
            artifact_status(directory.path(), &artifact),
            ArtifactStatus::Missing
        );
    }

    #[test]
    fn xmp_is_explicitly_unsupported() {
        assert!(!xmp_supported());
        assert!(matches!(
            SidecarError::XmpUnsupported,
            SidecarError::XmpUnsupported
        ));
    }
}
