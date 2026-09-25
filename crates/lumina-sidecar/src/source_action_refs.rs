//! Shared validation and loading helpers for persisted source-action bundles.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{SourceActionSpec, SOURCE_ACTION_VERSION};

/// F-042-N1: reference to a repair-region artifact stored in the sidecar's
/// `.lumina.zdata` bundle. `id` is the record id inside the bundle,
/// `relative_path` is the portable (never absolute) bundle file name, and
/// `checksum` is the BLAKE3 checksum of the artifact bytes. The existing
/// `ArtifactReference` is intentionally *not* reused here: it has no record
/// `id` field and carries mask-specific metadata (`channels`, `data_version`)
/// that a repair region does not need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceActionArtifactRef {
    pub id: String,
    pub relative_path: String,
    pub checksum: String,
}

/// Validate a persisted source-action reference against the adjacent bundle.
///
/// The link is intentionally narrower than a generic artifact path: it must be
/// one safe portable `.lumina.zdata` filename, and that filename must exactly
/// equal the bundle selected for loading. Keeping this rule here makes the GUI
/// and CLI fail closed before either resolver touches container bytes.
pub fn validate_source_action_bundle_reference(
    reference: &SourceActionArtifactRef,
    zdata_path: &Path,
) -> Result<(), String> {
    if reference.id.is_empty() {
        return Err("source action artifact id must not be empty".into());
    }
    if reference.checksum.is_empty() {
        return Err(format!(
            "source action `{}` checksum must not be empty",
            reference.id
        ));
    }
    let relative = reference.relative_path.as_str();
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.starts_with('\\')
        || relative.contains('\\')
        || relative.contains(':')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!(
            "source action `{}` relative_path must be one safe portable bundle filename",
            reference.id
        ));
    }
    if !relative.ends_with(".lumina.zdata") {
        return Err(format!(
            "source action `{}` relative_path must name a .lumina.zdata bundle",
            reference.id
        ));
    }
    let actual_name = zdata_path.file_name().and_then(|name| name.to_str());
    if actual_name != Some(relative) {
        return Err(format!(
            "source action `{}` bundle path mismatch: recipe references `{relative}`, loaded `{}`",
            reference.id,
            zdata_path.display()
        ));
    }
    Ok(())
}

/// Validate the version and bundle link of one persisted source action.
pub fn validate_source_action_spec(
    spec: &SourceActionSpec,
    zdata_path: &Path,
) -> Result<(), String> {
    if spec.version != SOURCE_ACTION_VERSION {
        return Err(format!(
            "source action `{}` has unsupported version {}",
            spec.artifact.id, spec.version
        ));
    }
    validate_source_action_bundle_reference(&spec.artifact, zdata_path)
}

/// Validate all source-action links before opening the adjacent bundle.
#[cfg(feature = "zdata")]
pub fn load_validated_source_action_bundle(
    recipe: &crate::EditRecipe,
    zdata_path: &Path,
) -> Result<crate::ZDataContainer, String> {
    for spec in &recipe.source_actions {
        validate_source_action_spec(spec, zdata_path)?;
    }
    load_zdata(zdata_path).map_err(|error| {
        format!(
            "could not read source-action bundle `{}`: {error}",
            zdata_path.display()
        )
    })
}

#[cfg(feature = "zdata")]
use crate::load_zdata;
