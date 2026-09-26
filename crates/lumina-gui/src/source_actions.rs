//! GUI-SRCACC-1: strict persisted source-action resolution.

use std::collections::BTreeMap;
use std::path::Path;

use lumina_core::{ImageFrame, MaskPlane, SourceActionArtifact};
use lumina_sidecar::{
    load_zdata, validate_source_action_spec, EditRecipe, RepairRegionArtifact,
    SourceActionArtifactRef, SourceFingerprint, SourceIdentity,
};

#[path = "sidecar_snapshot.rs"]
mod sidecar_snapshot;
use sidecar_snapshot::persisted_action_identity;
#[cfg(test)]
pub(crate) use sidecar_snapshot::read_sidecar_recipe_snapshot;
pub(crate) use sidecar_snapshot::{
    read_sidecar_recipe_snapshot_for_bytes, sidecar_bundle_identity,
};

use crate::{GenerativeArtifacts, GenerativeRoleStatus, LuminaApp, Str};

/// Stable identity of one successfully resolved source action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceActionIdentity {
    pub(crate) id: String,
    pub(crate) checksum: String,
}

/// Content identity: exact bytes plus a stable failure class for invalid input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileContentIdentity {
    Missing,
    Unavailable(String),
    Hashed(String),
    Invalid {
        content_hash: String,
        classification: String,
    },
}

impl FileContentIdentity {
    /// Exact whole-file content identity of `path`.
    ///
    /// THUMB-HASH-PERF-35: the expensive BLAKE3 pass is memoized on
    /// `(path, mtime, ctime, len)` in [`crate::source_identity`] — an unchanged
    /// file is hashed once instead of once per visible cell per frame. Only the
    /// *recomputation* is skipped: the returned value is bit-identical to the
    /// pre-cache hash, a genuinely changed file always misses the memo (the key
    /// carries the kernel-maintained `ctime`, so even a same-length rewrite
    /// whose mtime was restored invalidates), and a missing / unreadable /
    /// permission-denied path keeps its own class and is never stored. Normative
    /// contract and the platform caveat: `feature/platform/cli-gui-wasm.md`
    /// § *Quell-Identitäts-Cache im UI-Thread*.
    pub(crate) fn from_path(path: &Path) -> Self {
        match crate::source_identity::content_hash(path) {
            Ok(content_hash) => Self::Hashed(content_hash),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unavailable(error.to_string()),
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self::Hashed(format!("blake3:{}", blake3::hash(bytes).to_hex()))
    }

    fn invalid_from_bytes(bytes: &[u8], classification: &str) -> Self {
        Self::Invalid {
            content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
            classification: classification.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarBundleIdentity {
    source_image: FileContentIdentity,
    document: FileContentIdentity,
    source_action_bundle: Option<FileContentIdentity>,
}

impl SidecarBundleIdentity {
    pub(crate) fn with_source_image(
        document: FileContentIdentity,
        source_image: FileContentIdentity,
        source_action_bundle: Option<FileContentIdentity>,
    ) -> Self {
        Self {
            source_image,
            document,
            source_action_bundle,
        }
    }

    /// Whether the persisted recipe/bundle identity includes a source-action
    /// bundle. Such thumbnails must never consume the legacy recipe-blind disk
    /// entry, including on a cold folder index.
    pub(crate) fn has_source_action_bundle(&self) -> bool {
        self.source_action_bundle.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NeighborInputIdentity {
    source: FileContentIdentity,
    persisted: SidecarBundleIdentity,
    source_actions: Vec<SourceActionIdentity>,
}

impl NeighborInputIdentity {
    pub(crate) fn capture(source: &Path, virtual_copy: &str) -> Self {
        let (persisted, source_actions) = persisted_action_identity(source, virtual_copy);
        Self {
            source: FileContentIdentity::from_path(source),
            persisted,
            source_actions,
        }
    }

    pub(crate) fn from_worker(
        source: &[u8],
        document: FileContentIdentity,
        resolved: &ResolvedSourceActions,
    ) -> Self {
        let source_identity = FileContentIdentity::from_bytes(source);
        Self {
            source: source_identity.clone(),
            persisted: SidecarBundleIdentity::with_source_image(
                document,
                source_identity,
                resolved.bundle_identity().cloned(),
            ),
            source_actions: resolved.identities().to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ResolvedSourceActions {
    artifacts: Vec<SourceActionArtifact>,
    identities: Vec<SourceActionIdentity>,
    bundle_identity: Option<FileContentIdentity>,
}

impl ResolvedSourceActions {
    pub(crate) fn artifacts(&self) -> &[SourceActionArtifact] {
        &self.artifacts
    }

    pub(crate) fn identities(&self) -> &[SourceActionIdentity] {
        &self.identities
    }

    pub(crate) fn artifact_checksums(&self) -> Vec<String> {
        self.identities
            .iter()
            .map(|identity| identity.checksum.clone())
            .collect()
    }

    pub(crate) fn bundle_identity(&self) -> Option<&FileContentIdentity> {
        self.bundle_identity.as_ref()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct SourceActionResolveError(String);

fn error(message: impl Into<String>) -> SourceActionResolveError {
    SourceActionResolveError(message.into())
}

pub(crate) fn source_fingerprint(bytes: &[u8]) -> SourceFingerprint {
    SourceFingerprint {
        content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        byte_length: bytes.len() as u64,
        extras: BTreeMap::new(),
    }
}

pub(crate) fn source_fingerprint_matches(
    stored: &SourceIdentity,
    live: &SourceFingerprint,
) -> bool {
    stored.content_hash == live.content_hash && stored.byte_length == live.byte_length
}

pub(crate) fn resolve_repair_region(
    reference: &SourceActionArtifactRef,
    region: RepairRegionArtifact,
    source: &ImageFrame,
) -> Result<SourceActionArtifact, SourceActionResolveError> {
    if region.id != reference.id {
        return Err(error(format!(
            "source action `{}` artifact identity mismatch: bundle returned `{}`",
            reference.id, region.id
        )));
    }
    if region.width != source.width || region.height != source.height {
        return Err(error(format!(
            "source action `{}` dimensions {}x{} do not match source frame {}x{}",
            reference.id, region.width, region.height, source.width, source.height
        )));
    }
    region.validate().map_err(|err| {
        error(format!(
            "source action `{}` has invalid repair-region data: {err}",
            reference.id
        ))
    })?;
    let plane = MaskPlane::new(region.width, region.height, region.region).map_err(|err| {
        error(format!(
            "source action `{}` has an invalid region plane: {err}",
            reference.id
        ))
    })?;
    let replacement =
        ImageFrame::new(region.width, region.height, region.replacement).map_err(|err| {
            error(format!(
                "source action `{}` has an invalid replacement image: {err}",
                reference.id
            ))
        })?;
    Ok(SourceActionArtifact {
        region: plane,
        replacement,
    })
}

pub(crate) fn resolve_source_actions(
    recipe: &EditRecipe,
    zdata_path: &Path,
    source: &ImageFrame,
) -> Result<ResolvedSourceActions, SourceActionResolveError> {
    if recipe.source_actions.is_empty() {
        return Ok(ResolvedSourceActions::default());
    }
    for spec in &recipe.source_actions {
        validate_source_action_spec(spec, zdata_path).map_err(error)?;
    }
    let container = load_zdata(zdata_path).map_err(|err| {
        error(format!(
            "could not read source-action bundle `{}`: {err}",
            zdata_path.display()
        ))
    })?;
    let bundle_identity = FileContentIdentity::from_bytes(container.to_bytes());
    let mut artifacts = Vec::with_capacity(recipe.source_actions.len());
    let mut identities = Vec::with_capacity(recipe.source_actions.len());
    for spec in &recipe.source_actions {
        let region = container.repair_region(&spec.artifact.id).map_err(|err| {
            error(format!(
                "source action `{}` artifact missing or corrupt in bundle: {err}",
                spec.artifact.id
            ))
        })?;
        let checksum = region.checksum();
        if checksum != spec.artifact.checksum {
            return Err(error(format!(
                "source action `{}` checksum mismatch: recipe and bundle disagree (stale or corrupted artifact)",
                spec.artifact.id
            )));
        }
        artifacts.push(resolve_repair_region(&spec.artifact, region, source)?);
        identities.push(SourceActionIdentity {
            id: spec.artifact.id.clone(),
            checksum,
        });
    }
    Ok(ResolvedSourceActions {
        artifacts,
        identities,
        bundle_identity: Some(bundle_identity),
    })
}

impl LuminaApp {
    pub(crate) fn sidecar_resolution_pending(&self) -> bool {
        if self.document.is_some() || self.path.trim().is_empty() {
            return false;
        }
        lumina_sidecar::sidecar_path_for(Path::new(&self.path)).exists()
    }

    pub(crate) fn resolve_current_source_actions(
        &self,
        source: &ImageFrame,
    ) -> Result<ResolvedSourceActions, SourceActionResolveError> {
        if self.sidecar_resolution_pending() {
            let sidecar = lumina_sidecar::sidecar_path_for(Path::new(&self.path));
            return Err(error(format!(
                "could not load source sidecar `{}` for source-action resolution",
                sidecar.display()
            )));
        }
        if self.recipe.source_actions.is_empty() {
            return Ok(ResolvedSourceActions::default());
        }
        if self.path.trim().is_empty() {
            return Err(error(format!(
                "source action bundle unavailable: loaded image `{}` has no file path",
                self.source_name
            )));
        }
        let zdata_path = lumina_sidecar::zdata_path_for(Path::new(&self.path));
        resolve_source_actions(&self.recipe, &zdata_path, source)
    }

    pub(crate) fn invalidate_source_action_preview(&mut self) {
        self.preview = None;
        self.render_key = None;
        self.tone_analysis = None;
        self.preview_histogram = None;
        self.render_mask_layers.clear();
        self.preview_is_draft = false;
        self.preview_roi = None;
        self.preview_render_src = None;
        self.draft_original = None;
        self.base_stage_cache.clear();
        self.source_hash_memo = None;
        self.last_stage_work = None;
        self.generative_artifacts = GenerativeArtifacts::default();
        self.generative_role_status = [GenerativeRoleStatus::Missing; 2];
        self.generative_memo = None;
        self.face_crop_textures.clear();
        self.mask_overlay_texture = None;
        self.thumbnails.invalidate_all();
        if let Some(controller) = self.preview_ctrl.as_mut() {
            controller.reset();
        }
        self.texture = None;
        self.texture_identity = None;
        self.navigator_texture = None;
        self.navigator_texture_key = None;
        self.navigator_overview = None;
        self.navigator_overview_key = None;
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.status = Str::Error.t().into();
        #[cfg(feature = "gpu")]
        {
            self.gpu_present_frame = None;
            self.vram_fresh = false;
            self.vram_mask_is_evaluated = false;
            self.gpu_stage_gate = None;
            self.gpu_route_fallback = None;
            self.vram_render_refusal = None;
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.clear_source_action_artifacts();
            }
        }
    }
}
