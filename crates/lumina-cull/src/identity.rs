//! Analyzer identity and staleness comparison (decisions §5).
//!
//! The Stage-1 analyzer identity is fixed: `heuristic`, name
//! `lumina-cull-heuristic`, version `1`, **no model hash** (the sidecar schema
//! rejects a heuristic analyzer that pretends to have a model). A persisted
//! proposal is usable only while source hash, decode/geometry parameters,
//! analyzer identity, analysis resolution and preprocessing all match the
//! current values; any deviation is a visible `stale`, never a hidden re-run.

use lumina_sidecar::{
    CullingAnalyzer, CullingAnalyzerKind, CullingIdentity, DecodeFingerprint, GeometryFingerprint,
    Preprocessing, Resolution, SourceFingerprint,
};
use std::collections::BTreeMap;

/// Stable Stage-1 analyzer name (persisted in the sidecar identity).
pub const HEURISTIC_ANALYZER_NAME: &str = "lumina-cull-heuristic";
/// Stage-1 analyzer version. **Bump on any scale/threshold/signal change.**
pub const HEURISTIC_ANALYZER_VERSION: &str = "1";
/// Stage-1 analyzer kind (model-free).
pub const HEURISTIC_ANALYZER_KIND: CullingAnalyzerKind = CullingAnalyzerKind::Heuristic;
/// Stable Stage-1 preprocessing name (RGBA8 sRGB + bilinear downscale).
pub const HEURISTIC_PREPROCESSING_NAME: &str = "srgb_rgba8_downscale_bilinear";
/// Stage-1 preprocessing version.
pub const HEURISTIC_PREPROCESSING_VERSION: &str = "1";

/// The fixed Stage-1 analyzer identity (heuristic, no model hash).
#[must_use]
pub fn heuristic_analyzer() -> CullingAnalyzer {
    CullingAnalyzer {
        kind: HEURISTIC_ANALYZER_KIND,
        name: HEURISTIC_ANALYZER_NAME.to_string(),
        version: HEURISTIC_ANALYZER_VERSION.to_string(),
        model_hash: None,
        extras: Default::default(),
    }
}

/// The fixed Stage-1 preprocessing identity.
#[must_use]
pub fn heuristic_preprocessing() -> Preprocessing {
    Preprocessing {
        name: HEURISTIC_PREPROCESSING_NAME.to_string(),
        version: HEURISTIC_PREPROCESSING_VERSION.to_string(),
        parameters: BTreeMap::new(),
        extras: Default::default(),
    }
}

/// Builds the reproducible identity for a Stage-1 analysis.
#[must_use]
pub fn heuristic_identity(
    source: SourceFingerprint,
    decode: DecodeFingerprint,
    geometry: GeometryFingerprint,
    analysis_resolution: Resolution,
) -> CullingIdentity {
    CullingIdentity {
        source,
        decode,
        geometry,
        analyzer: heuristic_analyzer(),
        analysis_resolution,
        preprocessing: heuristic_preprocessing(),
        extras: Default::default(),
    }
}

/// Which documented identity component differs between a stored proposal and
/// the current source/analyzer context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdentityMismatch {
    /// Source content hash differs (the pixels are not the analyzed ones).
    SourceContentHash,
    /// Source byte length differs.
    SourceByteLength,
    /// Decoder/version/parameters differ.
    Decode,
    /// Geometry (width/height/orientation/aspect) differs.
    Geometry,
    /// Analyzer (kind/name/version/model hash) differs.
    Analyzer,
    /// Analysis resolution differs.
    AnalysisResolution,
    /// Preprocessing identity differs.
    Preprocessing,
}

/// Documented identity fields that must match; additive `extras` maps are
/// intentionally *not* part of the comparison so a benign unknown field can
/// never fake a stale state (and vice versa, a known field change always
/// invalidates).
#[must_use]
pub fn identity_mismatches(
    stored: &CullingIdentity,
    current: &CullingIdentity,
) -> Vec<IdentityMismatch> {
    let mut mismatches = Vec::new();
    if stored.source.content_hash != current.source.content_hash {
        mismatches.push(IdentityMismatch::SourceContentHash);
    }
    if stored.source.byte_length != current.source.byte_length {
        mismatches.push(IdentityMismatch::SourceByteLength);
    }
    if stored.decode.decoder != current.decode.decoder
        || stored.decode.version != current.decode.version
        || stored.decode.parameters != current.decode.parameters
    {
        mismatches.push(IdentityMismatch::Decode);
    }
    if stored.geometry.width != current.geometry.width
        || stored.geometry.height != current.geometry.height
        || stored.geometry.orientation != current.geometry.orientation
        || stored.geometry.pixel_aspect_ratio != current.geometry.pixel_aspect_ratio
    {
        mismatches.push(IdentityMismatch::Geometry);
    }
    if stored.analyzer.kind != current.analyzer.kind
        || stored.analyzer.name != current.analyzer.name
        || stored.analyzer.version != current.analyzer.version
        || stored.analyzer.model_hash != current.analyzer.model_hash
    {
        mismatches.push(IdentityMismatch::Analyzer);
    }
    if stored.analysis_resolution.width != current.analysis_resolution.width
        || stored.analysis_resolution.height != current.analysis_resolution.height
    {
        mismatches.push(IdentityMismatch::AnalysisResolution);
    }
    if stored.preprocessing.name != current.preprocessing.name
        || stored.preprocessing.version != current.preprocessing.version
        || stored.preprocessing.parameters != current.preprocessing.parameters
    {
        mismatches.push(IdentityMismatch::Preprocessing);
    }
    mismatches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(hash: &str) -> SourceFingerprint {
        SourceFingerprint {
            content_hash: hash.into(),
            byte_length: 42,
            extras: Default::default(),
        }
    }

    fn decode() -> DecodeFingerprint {
        DecodeFingerprint {
            decoder: "libraw".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Default::default(),
        }
    }

    fn geometry() -> GeometryFingerprint {
        GeometryFingerprint {
            width: 6000,
            height: 4000,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Default::default(),
        }
    }

    fn resolution() -> Resolution {
        Resolution {
            width: 2048,
            height: 1365,
            extras: Default::default(),
        }
    }

    #[test]
    fn heuristic_analyzer_is_model_free() {
        let analyzer = heuristic_analyzer();
        assert_eq!(analyzer.kind, CullingAnalyzerKind::Heuristic);
        assert!(analyzer.model_hash.is_none());
        assert_eq!(analyzer.name, HEURISTIC_ANALYZER_NAME);
        assert_eq!(analyzer.version, HEURISTIC_ANALYZER_VERSION);
    }

    #[test]
    fn mismatches_are_detected_per_component() {
        let base = heuristic_identity(source("blake3:a"), decode(), geometry(), resolution());
        assert!(identity_mismatches(&base, &base).is_empty());

        let mut changed = base.clone();
        changed.source.content_hash = "blake3:b".into();
        assert_eq!(
            identity_mismatches(&base, &changed),
            vec![IdentityMismatch::SourceContentHash]
        );

        let mut changed = base.clone();
        changed.source.byte_length = 43;
        assert_eq!(
            identity_mismatches(&base, &changed),
            vec![IdentityMismatch::SourceByteLength]
        );

        let mut changed = base.clone();
        changed.geometry.orientation = 6;
        assert_eq!(
            identity_mismatches(&base, &changed),
            vec![IdentityMismatch::Geometry]
        );

        let mut changed = base.clone();
        changed.analyzer.version = "2".into();
        assert_eq!(
            identity_mismatches(&base, &changed),
            vec![IdentityMismatch::Analyzer]
        );

        let mut changed = base.clone();
        changed.analysis_resolution.width = 1024;
        assert_eq!(
            identity_mismatches(&base, &changed),
            vec![IdentityMismatch::AnalysisResolution]
        );

        let mut changed = base.clone();
        changed.preprocessing.version = "2".into();
        assert_eq!(
            identity_mismatches(&base, &changed),
            vec![IdentityMismatch::Preprocessing]
        );
    }

    #[test]
    fn additive_extras_are_not_part_of_the_identity() {
        let base = heuristic_identity(source("blake3:a"), decode(), geometry(), resolution());
        let mut with_extras = base.clone();
        with_extras
            .source
            .extras
            .insert("future_unknown".into(), serde_json::Value::from(true));
        assert!(identity_mismatches(&base, &with_extras).is_empty());
    }
}
