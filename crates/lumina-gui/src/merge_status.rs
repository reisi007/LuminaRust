//! LRPAR-G13-MERGE-15 — visible merge-bundle status (decision §Persistenz).
//!
//! Extracted from `merge_gui.rs` (file-size ratchet): the read-only status
//! view over a persisted merge bundle (no rewrite, no merge logic).

use std::path::Path;

use lumina_merge::bundle::{
    classify_merge_document, merge_checksum, stored_merge_artifact, stored_merge_recipe,
    MergeDocumentKind,
};
use lumina_sidecar::{load_sidecar, sidecar_path_for};

use crate::i18n::Str;

/// Visible status of a persisted merge bundle (decision §Persistenz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeArtifactStatus {
    /// No merge bundle at the target (nothing to report).
    NoBundle,
    /// Source hashes, decode contexts and the DNG checksum match.
    Ok,
    /// The bundle exists but an input changed (only `force` re-merges).
    Stale,
    /// A bundle part is gone (DNG or sidecar).
    Missing,
    /// The sidecar is not a merge envelope / unreadable / the stored recipe is
    /// invalid (a hard, loud state — never overwritten silently).
    Unsupported,
}

impl MergeArtifactStatus {
    /// Visible status text (status line + panel).
    pub fn text(self) -> &'static str {
        match self {
            MergeArtifactStatus::NoBundle => Str::MergeStatusNone.t(),
            MergeArtifactStatus::Ok => Str::MergeStatusOk.t(),
            MergeArtifactStatus::Stale => Str::MergeStatusStale.t(),
            MergeArtifactStatus::Missing => Str::MergeStatusMissing.t(),
            MergeArtifactStatus::Unsupported => Str::MergeStatusUnsupported.t(),
        }
    }
}

/// Reads the visible status of a merge bundle at `dng_path` (decision
/// §Persistenz). Pure I/O + validation; never rewrites anything.
pub fn merge_bundle_status(dng_path: &Path) -> (MergeArtifactStatus, String) {
    let sidecar_path = sidecar_path_for(dng_path);
    if !sidecar_path.exists() {
        return if dng_path.exists() {
            (
                MergeArtifactStatus::Missing,
                format!(
                    "merge DNG `{}` exists without its sidecar",
                    dng_path.display()
                ),
            )
        } else {
            (MergeArtifactStatus::NoBundle, String::new())
        };
    }
    let document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(error) => {
            return (
                MergeArtifactStatus::Unsupported,
                format!("merge sidecar unreadable: {error}"),
            )
        }
    };
    match classify_merge_document(&document) {
        MergeDocumentKind::Merge => {}
        MergeDocumentKind::Standard => {
            return (
                MergeArtifactStatus::Unsupported,
                "sidecar is a standard sidecar (no merge envelope)".into(),
            )
        }
        MergeDocumentKind::Unknown(kind) => {
            return (
                MergeArtifactStatus::Unsupported,
                format!("unknown document type `{kind}`"),
            )
        }
    }
    let recipe = match stored_merge_recipe(&document) {
        Ok(recipe) => recipe,
        Err(reason) => return (MergeArtifactStatus::Unsupported, reason),
    };
    let artifact = match stored_merge_artifact(&document) {
        Ok(artifact) => artifact,
        Err(reason) => return (MergeArtifactStatus::Unsupported, reason),
    };
    let Ok(dng_bytes) = std::fs::read(dng_path) else {
        return (MergeArtifactStatus::Missing, "merge DNG is gone".into());
    };
    if merge_checksum(&dng_bytes) != artifact.checksum {
        return (
            MergeArtifactStatus::Stale,
            "merge DNG checksum changed on disk".into(),
        );
    }
    // Source hashes are relative to the bundle directory.
    let bundle_dir = dng_path.parent().unwrap_or_else(|| Path::new("."));
    for source in &recipe.sources {
        let source_path = bundle_dir.join(&source.path);
        let Ok(bytes) = std::fs::read(&source_path) else {
            return (
                MergeArtifactStatus::Missing,
                format!("source `{}` is missing", source.path),
            );
        };
        if merge_checksum(&bytes) != source.content_hash {
            return (
                MergeArtifactStatus::Stale,
                format!("source `{}` changed (hash mismatch)", source.path),
            );
        }
    }
    (MergeArtifactStatus::Ok, String::new())
}
