//! DNG staging and serialized publication for the shared merge bundle.

use std::io::Write;
use std::path::Path;

use lumina_sidecar::{SidecarDocument, SidecarWriteLock};

use super::MergeRunError;

/// Stage a DNG in its target directory and sync its contents before publication.
/// This potentially expensive work must happen outside the sidecar lock.
pub(super) fn stage_dng(
    dng_path: &Path,
    dng_bytes: &[u8],
) -> Result<tempfile::NamedTempFile, MergeRunError> {
    let parent = dng_path.parent().unwrap_or_else(|| Path::new("."));
    let filename = dng_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "artifact".into());
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| {
            MergeRunError::Failed(format!("cannot stage `{}`: {error}", dng_path.display()))
        })?;
    let temporary_path = temporary.path().to_path_buf();
    temporary.write_all(dng_bytes).map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot stage `{}`: {error}",
            temporary_path.display()
        ))
    })?;
    temporary.flush().map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot stage `{}`: {error}",
            temporary_path.display()
        ))
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot stage `{}`: {error}",
            temporary_path.display()
        ))
    })?;
    Ok(temporary)
}

/// Commit the sidecar before renaming its staged DNG into place. The caller
/// holds the cooperative lock from the final ownership re-check through both
/// publications; a failed sidecar save leaves the DNG target untouched.
pub(super) fn publish_staged_dng(
    dng_path: &Path,
    temporary: tempfile::NamedTempFile,
    sidecar_lock: &SidecarWriteLock,
    document: &SidecarDocument,
) -> Result<(), MergeRunError> {
    if let Err(error) = sidecar_lock.save(document) {
        return Err(MergeRunError::Failed(format!(
            "sidecar write failed: {error}"
        )));
    }
    temporary.persist(dng_path).map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot publish `{}`: {}",
            dng_path.display(),
            error.error
        ))
    })?;
    Ok(())
}
