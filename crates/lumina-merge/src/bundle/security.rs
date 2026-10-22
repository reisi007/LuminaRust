//! Ownership and integrity gate for existing merge output bundles.

use super::*;

/// An envelope marker alone is not ownership evidence. Existing targets must
/// carry a valid recipe and artifact reference that agree with the target and
/// sidecar source identity; if the DNG exists, its bytes must match the stored
/// checksum before `force` can proceed.
pub(super) fn check_existing_bundle(
    gate: BundleGate<'_>,
) -> Result<Option<MergeRunOutcome>, MergeRunError> {
    let BundleGate {
        command,
        mode,
        force,
        sidecar_path,
        dng_path,
        digest,
        residual_px,
        residual_warning,
    } = gate;
    let Some(document) = claim_merge_target(sidecar_path, dng_path)? else {
        return Ok(None);
    };

    let (stored_recipe, stored_artifact) =
        validate_merge_bundle_anchor(&document, sidecar_path, dng_path)?;
    let dng_bytes = match fs::read(dng_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && force => {
            // A missing DNG has no bytes to overwrite. A validated recipe and
            // artifact anchor may be used to rebuild it; a present but damaged
            // DNG is never force-repairable because its ownership cannot be
            // distinguished from a transplanted sidecar.
            info!("{command}: force rebuilds the missing DNG from its intact merge sidecar");
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MergeRunError::Missing(format!(
                "merge DNG `{}` is gone (its sidecar `{}` remains); \
                 regenerate the bundle explicitly with force",
                dng_path.display(),
                sidecar_path.display()
            )))
        }
        Err(error) => {
            return Err(MergeRunError::Missing(format!(
                "cannot read merge DNG `{}` ({error}); refusing to overwrite it",
                dng_path.display()
            )))
        }
    };
    let dng_bytes = dng_bytes.expect("present DNG bytes are set above");
    if dng_bytes.len() as u64 != document.source.byte_length
        || merge_checksum(&dng_bytes) != stored_artifact.checksum
    {
        return Err(stale_bundle_error(
            sidecar_path,
            "merge DNG checksum or byte length does not match its artifact reference; \
             force cannot repair a present, damaged target",
        ));
    }

    if force {
        info!("{command}: force regenerates the intact existing merge bundle");
        return Ok(None);
    }
    if stored_recipe.digest() != digest {
        return Err(MergeRunError::Stale(format!(
            "sources, decode context or alignment changed since `{}` \
             was written (digest mismatch); re-merge explicitly with force",
            sidecar_path.display()
        )));
    }
    info!("{command}: bundle already current (digest match, nothing rewritten)");
    Ok(Some(MergeRunOutcome {
        mode,
        dng_path: dng_path.to_path_buf(),
        sidecar_path: sidecar_path.to_path_buf(),
        digest: digest.to_string(),
        checksum: stored_artifact.checksum,
        cached: true,
        residual_px,
        residual_warning,
    }))
}

fn claim_merge_target(
    sidecar_path: &Path,
    dng_path: &Path,
) -> Result<Option<SidecarDocument>, MergeRunError> {
    if !sidecar_path.exists() {
        if dng_path.exists() {
            return Err(MergeRunError::Missing(format!(
                "merge DNG `{}` exists without a merge sidecar `{}`; refusing to \
                 overwrite a target that cannot be validated as a coherent merge \
                 bundle (force does not override this) — move it aside or choose \
                 another output",
                dng_path.display(),
                sidecar_path.display()
            )));
        }
        return Ok(None);
    }
    let document = load_sidecar(sidecar_path).map_err(|error| {
        MergeRunError::Missing(format!(
            "existing sidecar `{}` is unreadable ({error}); refusing to overwrite \
             it (force does not override this) — move it aside or choose another \
             output",
            sidecar_path.display()
        ))
    })?;
    match classify_merge_document(&document) {
        MergeDocumentKind::Merge => Ok(Some(document)),
        MergeDocumentKind::Standard => Err(MergeRunError::Stale(format!(
            "existing sidecar `{}` is a standard sidecar (no merge envelope); \
             refusing to overwrite it — force only regenerates a coherent merge bundle",
            sidecar_path.display()
        ))),
        MergeDocumentKind::Unknown(kind) => Err(MergeRunError::Stale(format!(
            "existing sidecar `{}` has unknown document type `{kind}`; refusing to \
             overwrite it — force only regenerates a coherent merge bundle",
            sidecar_path.display()
        ))),
    }
}

/// Validates all source-identity fields that the merge format can bind to its
/// artifact and generation rules. The historical LibRaw version has no
/// independent authenticated anchor; only its non-empty shape is verifiable.
fn validate_merge_bundle_anchor(
    document: &SidecarDocument,
    sidecar_path: &Path,
    dng_path: &Path,
) -> Result<(MergeRecipe, ArtifactReference), MergeRunError> {
    let stored_recipe = stored_merge_recipe(document)
        .map_err(|reason| stale_bundle_error(sidecar_path, &reason))?;
    let stored_artifact = stored_merge_artifact(document)
        .map_err(|reason| stale_bundle_error(sidecar_path, &reason))?;
    let expected_name = dng_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| stale_bundle_error(sidecar_path, "target has no portable file name"))?;
    let source = &document.source;
    let checksum = stored_artifact.checksum.strip_prefix("blake3:");
    let canonical_checksum = checksum.is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    let expected_geometry = format!(
        "{}x{}",
        source.geometry_fingerprint.width, source.geometry_fingerprint.height
    );
    let expected_decode_parameters =
        std::collections::BTreeMap::from([("geometry".to_owned(), expected_geometry)]);
    let expected_decode_extras = std::collections::BTreeMap::from([(
        "orientation_applied".to_owned(),
        serde_json::Value::String("true".to_owned()),
    )]);
    if stored_recipe.output.file != expected_name
        || stored_artifact.relative_path != expected_name
        || source.relative_name != expected_name
        || document.pipeline_version != MERGE_PIPELINE_VERSION
        || source.raw_format != "DNG"
        || source.orientation != 1
        || source.modified_at.is_some()
        || source.content_hash != stored_artifact.checksum
        || !canonical_checksum
        || source.decode_fingerprint.decoder != "libraw"
        || source.decode_fingerprint.version.trim().is_empty()
        || source.decode_fingerprint.parameters != expected_decode_parameters
        || source.decode_fingerprint.extras != expected_decode_extras
        || stored_artifact.format != MERGE_DNG_FORMAT
        || stored_artifact.channels != MERGE_DNG_CHANNELS
        || stored_artifact.data_version != MERGE_DNG_DATA_VERSION
        || stored_artifact.width != source.geometry_fingerprint.width
        || stored_artifact.height != source.geometry_fingerprint.height
        || source.geometry_fingerprint.orientation != 1
        || source.geometry_fingerprint.pixel_aspect_ratio != 1.0
        || !source.geometry_fingerprint.extras.is_empty()
        || !source.extras.is_empty()
        || source.byte_length == 0
    {
        return Err(stale_bundle_error(
            sidecar_path,
            "merge recipe, artifact reference and sidecar source identity are incoherent",
        ));
    }
    Ok((stored_recipe, stored_artifact))
}

fn stale_bundle_error(sidecar_path: &Path, reason: &str) -> MergeRunError {
    MergeRunError::Stale(format!(
        "existing sidecar `{}` {reason}; force cannot repair an incoherent bundle — \
         move the files aside or choose another output",
        sidecar_path.display()
    ))
}
