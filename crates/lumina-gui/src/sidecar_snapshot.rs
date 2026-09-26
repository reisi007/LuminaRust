//! Exact sidecar snapshots shared by GUI stand-in workers.
//!
//! A recipe is not meaningful without the source bytes it was persisted for.
//! Keeping the bounded read, schema parse, and source-fingerprint check here
//! gives neighbor and thumbnail workers one fail-closed identity boundary.

use std::io::Read;
use std::path::Path;

use lumina_sidecar::{EditRecipe, SidecarDocument, MAX_SIDECAR_BYTES};

use super::{
    source_fingerprint, source_fingerprint_matches, FileContentIdentity, SidecarBundleIdentity,
    SourceActionIdentity,
};

/// A bounded, exact sidecar snapshot shared by identity capture and workers.
#[derive(Debug)]
pub(crate) struct SidecarRecipeSnapshot {
    pub(crate) recipe: EditRecipe,
    pub(crate) document_identity: FileContentIdentity,
    /// MASK-LOCAL-P0: stand-in workers must refuse a non-neutral local mask
    /// state rather than rendering a global-only neighbour/thumbnail.
    pub(crate) has_local_adjustments: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SidecarSnapshotError {
    message: String,
    identity: FileContentIdentity,
}

fn read_sidecar_recipe_snapshot_detailed(
    source: &Path,
    virtual_copy: &str,
    source_bytes: Option<&[u8]>,
) -> Result<SidecarRecipeSnapshot, SidecarSnapshotError> {
    let path = lumina_sidecar::sidecar_path_for(source);
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SidecarRecipeSnapshot {
                recipe: EditRecipe::default(),
                document_identity: FileContentIdentity::Missing,
                has_local_adjustments: false,
            });
        }
        Err(error) => {
            return Err(SidecarSnapshotError {
                message: format!("sidecar {}: {error}", path.display()),
                identity: FileContentIdentity::Unavailable(error.to_string()),
            });
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file
        .by_ref()
        .take(MAX_SIDECAR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
    {
        return Err(SidecarSnapshotError {
            message: format!("sidecar {}: {error}", path.display()),
            identity: FileContentIdentity::Unavailable(error.to_string()),
        });
    }
    if bytes.len() > MAX_SIDECAR_BYTES {
        return Err(SidecarSnapshotError {
            message: format!(
                "sidecar exceeds size limit of {MAX_SIDECAR_BYTES} bytes: `{}`",
                path.display()
            ),
            identity: match FileContentIdentity::from_path(&path) {
                FileContentIdentity::Hashed(content_hash) => FileContentIdentity::Invalid {
                    content_hash,
                    classification: "size-limit".into(),
                },
                identity => identity,
            },
        });
    }
    let document_identity = FileContentIdentity::from_bytes(&bytes);
    let json = match std::str::from_utf8(&bytes) {
        Ok(json) => json,
        Err(_) => {
            return Err(SidecarSnapshotError {
                message: format!("sidecar is not valid UTF-8: `{}`", path.display()),
                identity: FileContentIdentity::invalid_from_bytes(&bytes, "invalid-utf8"),
            });
        }
    };
    let document = match SidecarDocument::from_json(json) {
        Ok(document) => document,
        Err(error) => {
            return Err(SidecarSnapshotError {
                message: format!("sidecar {}: {error}", path.display()),
                identity: FileContentIdentity::invalid_from_bytes(&bytes, "invalid-schema"),
            });
        }
    };
    // A sidecar recipe is usable only for the exact source bytes it was written
    // for. This check is shared by neighbor and thumbnail stand-ins; accepting a
    // recipe here would let a same-path replacement inherit stale pixels even
    // when its dimensions (and therefore the cheap decode shape) are unchanged.
    //
    // THUMB-HASH-PERF-35: without `source_bytes` this used to
    // `std::fs::read` the whole source on the UI thread per visible cell per
    // frame. `source_fingerprint_of` returns the identical
    // `SourceFingerprint` from the `(path, mtime, ctime, len)`-memoized
    // whole-file identity, so the fail-closed comparison is unchanged in value
    // and in failure class — a path that cannot be stat'ed or read still
    // returns the same loud "could not read source" error.
    let live_source = if let Some(bytes) = source_bytes {
        source_fingerprint(bytes)
    } else {
        match crate::source_identity::source_fingerprint_of(source) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return Err(SidecarSnapshotError {
                    message: format!(
                        "source identity conflict: could not read source `{}` while validating sidecar `{}`: {error}",
                        source.display(),
                        path.display(),
                    ),
                    identity: document_identity,
                });
            }
        }
    };
    if !source_fingerprint_matches(&document.source, &live_source) {
        return Err(SidecarSnapshotError {
            message: format!(
                "source identity conflict: source `{}` (hash {}, byte length {}) does not match sidecar `{}` (hash {}, byte length {}); sidecar recipe was not used",
                source.display(),
                live_source.content_hash,
                live_source.byte_length,
                path.display(),
                document.source.content_hash,
                document.source.byte_length,
            ),
            identity: document_identity,
        });
    }
    let Some(copy) = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == virtual_copy)
    else {
        return Err(SidecarSnapshotError {
            message: format!(
                "sidecar `{}` has no virtual copy `{virtual_copy}`",
                path.display()
            ),
            identity: FileContentIdentity::invalid_from_bytes(
                &bytes,
                &format!("missing-virtual-copy:{virtual_copy}"),
            ),
        });
    };
    Ok(SidecarRecipeSnapshot {
        recipe: copy.recipe.clone(),
        document_identity,
        has_local_adjustments: copy.mask_layers.iter().any(|layer| {
            if !layer.visible {
                return false;
            }
            match layer.effective_local_adjustments() {
                Ok(Some(adjustments)) => !adjustments.is_neutral(),
                Ok(None) => false,
                Err(_) => true,
            }
        }),
    })
}

#[cfg(test)]
pub(crate) fn read_sidecar_recipe_snapshot(
    source: &Path,
    virtual_copy: &str,
) -> Result<SidecarRecipeSnapshot, String> {
    read_sidecar_recipe_snapshot_detailed(source, virtual_copy, None).map_err(|error| error.message)
}

/// Validate the sidecar against the exact source bytes a worker will decode.
/// This closes the read/validate/render gap on same-path source replacement.
pub(crate) fn read_sidecar_recipe_snapshot_for_bytes(
    source: &Path,
    virtual_copy: &str,
    source_bytes: &[u8],
) -> Result<SidecarRecipeSnapshot, String> {
    read_sidecar_recipe_snapshot_detailed(source, virtual_copy, Some(source_bytes))
        .map_err(|error| error.message)
}

pub(super) fn persisted_action_identity(
    source: &Path,
    virtual_copy: &str,
) -> (SidecarBundleIdentity, Vec<SourceActionIdentity>) {
    let source_image = FileContentIdentity::from_path(source);
    match read_sidecar_recipe_snapshot_detailed(source, virtual_copy, None) {
        Ok(snapshot) => {
            let identities = snapshot
                .recipe
                .source_actions
                .iter()
                .map(|spec| SourceActionIdentity {
                    id: spec.artifact.id.clone(),
                    checksum: spec.artifact.checksum.clone(),
                })
                .collect::<Vec<_>>();
            let bundle = (!identities.is_empty())
                .then(|| FileContentIdentity::from_path(&lumina_sidecar::zdata_path_for(source)));
            (
                SidecarBundleIdentity::with_source_image(
                    snapshot.document_identity,
                    source_image,
                    bundle,
                ),
                identities,
            )
        }
        Err(error) => (
            SidecarBundleIdentity::with_source_image(
                error.identity,
                source_image,
                Some(FileContentIdentity::from_path(
                    &lumina_sidecar::zdata_path_for(source),
                )),
            ),
            Vec::new(),
        ),
    }
}

pub(crate) fn sidecar_bundle_identity(source: &Path, virtual_copy: &str) -> SidecarBundleIdentity {
    persisted_action_identity(source, virtual_copy).0
}
