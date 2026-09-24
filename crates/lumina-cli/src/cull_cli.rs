//! Live-source guards and source-level persistence for the `cull` CLI slice.

use super::{io_error, CliError};
use lumina_cull::{
    evaluate_culling, heuristic_identity, record_culling, save_culling, HeuristicAnalysis,
};
use lumina_sidecar::{
    load_sidecar, now_rfc3339_utc, sidecar_path_for, SidecarDocument, SourceFingerprint,
};
use std::{fs, path::Path};

/// Fingerprint of the exact source bytes used by a decode, never the sidecar's
/// possibly stale source identity.
pub(super) fn source_fingerprint(bytes: &[u8]) -> SourceFingerprint {
    SourceFingerprint {
        content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        byte_length: bytes.len() as u64,
        extras: Default::default(),
    }
}

fn ensure_sidecar_source_matches(
    input: &Path,
    document: &SidecarDocument,
    live: &SourceFingerprint,
) -> Result<(), CliError> {
    if live.content_hash == document.source.content_hash
        && live.byte_length == document.source.byte_length
    {
        return Ok(());
    }
    Err(CliError::Message(format!(
        "source identity conflict for `{}`: live source hash/byte length does not match \
         SidecarDocument.source; refusing culling write (live hash={}, bytes={}; \
         sidecar hash={}, bytes={})",
        input.display(),
        live.content_hash,
        live.byte_length,
        document.source.content_hash,
        document.source.byte_length,
    )))
}

/// Persists one analyzed item only when the bytes used for the analysis, a
/// fresh pre-write read, and the sidecar source identity all agree.
pub(super) fn persist_one(
    input: &Path,
    analyzed_source: &SourceFingerprint,
    analysis: &HeuristicAnalysis,
    force: bool,
) -> Result<serde_json::Value, CliError> {
    let current_bytes = fs::read(input).map_err(|error| io_error(input, error))?;
    let live_source = source_fingerprint(&current_bytes);
    if &live_source != analyzed_source {
        return Err(CliError::Message(format!(
            "source identity conflict for `{}`: source changed during cull analysis; \
             refusing culling write",
            input.display()
        )));
    }

    let path = sidecar_path_for(input);
    let mut document = load_sidecar(&path)?;
    ensure_sidecar_source_matches(input, &document, &live_source)?;
    let source = live_source;
    let decode = document.source.decode_fingerprint.clone();
    let geometry = document.source.geometry_fingerprint.clone();
    let current = heuristic_identity(
        source.clone(),
        decode.clone(),
        geometry.clone(),
        analysis.core.analysis_resolution.clone(),
    );
    // No automatic re-computation: an existing valid, identity-matching
    // proposal is kept untouched unless `--force`. The source guard above is
    // unconditional, so force can never bless a globally conflicted sidecar.
    if !force
        && matches!(
            evaluate_culling(&document, &current),
            lumina_cull::CullingReadState::Valid(_)
        )
    {
        return Ok(serde_json::json!({
            "input": input,
            "status": "current",
            "proposal": analysis.proposal(),
            "score": analysis.score(),
            "reasons": analysis.reasons(),
        }));
    }
    let section = analysis
        .core
        .to_section(source, decode, geometry, &now_rfc3339_utc())?;
    record_culling(&mut document, section)?;
    save_culling(&path, &document)?;
    Ok(serde_json::json!({
        "input": input,
        "status": "analyzed",
        "proposal": analysis.proposal(),
        "score": analysis.score(),
        "reasons": analysis.reasons(),
        "diagnostics": analysis.diagnostics,
        "similar_group": analysis.similar_group,
        "similar_redundant": analysis.similar_redundant,
    }))
}
