//! Monotone component scores, the documented proposal scale and the
//! sidecar-ready proposal payload.
//!
//! Scale (see the crate header): `score 0..=1` is keep-worthiness. Every
//! component is itself `0..=1`, monotone in its raw measure, and the final
//! recommendation is a monotone threshold decision on the score.

use lumina_sidecar::{
    validate_culling_reason, validate_culling_section, validate_culling_sha256, CullProposal,
    CullingAnalyzer, CullingAnalyzerKind, CullingIdentity, CullingSection, DecodeFingerprint,
    GeometryFingerprint, Preprocessing, Resolution, SourceFingerprint, CULLING_PENDING_MODEL_HASH,
};
use serde::{Deserialize, Serialize};

use crate::config::CullConfig;
use crate::signals::ExposureMetrics;
use crate::CullError;

/// Factor applied to [`CullConfig::clip_fraction_threshold`] for the exposure
/// component's clip penalty: the penalty reaches `1.0` at
/// `clip_fraction_threshold * EXPOSURE_CLIP_SCALE` (`0.05` by default).
pub const EXPOSURE_CLIP_SCALE: f64 = 10.0;

/// Monotone sharpness score `1 - exp(-tenengrad / scale)` in `0..=1`.
/// Strictly increasing for `tenengrad > 0`; `0.0` for a flat plane.
#[must_use]
pub fn sharpness_score(tenengrad: f64, scale: f64) -> f64 {
    if !tenengrad.is_finite() || tenengrad <= 0.0 || scale <= 0.0 {
        return 0.0;
    }
    (1.0 - (-tenengrad / scale).exp()).clamp(0.0, 1.0)
}

/// Monotone noise score `1 - exp(-sigma / scale)` in `0..=1` where a higher
/// value means **more** noise. Strictly increasing for `sigma > 0`.
#[must_use]
pub fn noise_score(sigma: f64, scale: f64) -> f64 {
    if !sigma.is_finite() || sigma <= 0.0 || scale <= 0.0 {
        return 0.0;
    }
    (1.0 - (-sigma / scale).exp()).clamp(0.0, 1.0)
}

/// Inverted noise score (component `1.0` = clean).
#[must_use]
pub fn noise_component(sigma: f64, scale: f64) -> f64 {
    1.0 - noise_score(sigma, scale)
}

/// Exposure component in `0..=1` (higher = better exposed), monotone
/// decreasing in the clipping fraction and in the extreme-mean penalty.
#[must_use]
pub fn exposure_component(metrics: &ExposureMetrics, config: &CullConfig) -> f64 {
    let scale = (config.clip_fraction_threshold * EXPOSURE_CLIP_SCALE).max(f64::EPSILON);
    let clip_penalty = (metrics.clip_fraction / scale).clamp(0.0, 1.0);

    let mean = metrics.mean;
    let mean_penalty = if mean < config.underexposed_mean_threshold {
        (config.underexposed_mean_threshold - mean) / config.underexposed_mean_threshold
    } else if mean > config.overexposed_mean_threshold {
        (mean - config.overexposed_mean_threshold) / (1.0 - config.overexposed_mean_threshold)
    } else {
        0.0
    }
    .clamp(0.0, 1.0);

    ((1.0 - clip_penalty) * (1.0 - mean_penalty)).clamp(0.0, 1.0)
}

/// Weighted base score (before any similar-group penalty): a convex blend of
/// sharpness, exposure and inverted noise, normalized by the weight sum.
#[must_use]
pub fn base_score(
    sharpness: f64,
    exposure: f64,
    noise_component_value: f64,
    config: &CullConfig,
) -> f64 {
    let sum = config.weight_sharpness + config.weight_exposure + config.weight_noise;
    if !sum.is_finite() || sum <= 0.0 {
        return 0.0;
    }
    let blended = (config.weight_sharpness * sharpness
        + config.weight_exposure * exposure
        + config.weight_noise * noise_component_value)
        / sum;
    blended.clamp(0.0, 1.0)
}

/// Monotone threshold decision: raising the score can never lower the
/// recommendation category.
#[must_use]
pub fn propose(score: f64, config: &CullConfig) -> CullProposal {
    if score <= config.reject_threshold {
        CullProposal::RejectCandidate
    } else if score >= config.keep_threshold {
        CullProposal::Keep
    } else {
        CullProposal::Review
    }
}

/// The sidecar-ready proposal payload produced by any analyzer (Stage 1
/// heuristic or Stage 2 model): recommendation, score, reason codes and the
/// analyzer/preprocessing identity. It never carries recipe state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalCore {
    /// Recommendation `keep` / `review` / `reject-kandidat`.
    pub proposal: CullProposal,
    /// Keep-worthiness score `0..=1`.
    pub score: f32,
    /// Machine-readable reason codes (baseline registry in the crate header).
    pub reasons: Vec<String>,
    /// Analyzer identity (kind/name/version/model hash).
    pub analyzer: CullingAnalyzer,
    /// Resolution the analysis ran at.
    pub analysis_resolution: Resolution,
    /// Preprocessing identity (input transform + version).
    pub preprocessing: Preprocessing,
}

impl ProposalCore {
    /// Loud validation of the proposal payload before it reaches the sidecar:
    /// score range, reason-code shape/uniqueness and the analyzer hash
    /// pairing. A malformed proposal is rejected, never clipped.
    pub fn validate(&self) -> Result<(), CullError> {
        if !self.score.is_finite() || !(0.0..=1.0).contains(&self.score) {
            return Err(CullError::InvalidProposal(format!(
                "score must be finite within 0..=1, got {}",
                self.score
            )));
        }
        let mut seen = std::collections::BTreeSet::new();
        for reason in &self.reasons {
            validate_culling_reason(reason)
                .map_err(|e| CullError::InvalidProposal(e.to_string()))?;
            if !seen.insert(reason) {
                return Err(CullError::InvalidProposal(format!(
                    "duplicate reason code `{reason}`"
                )));
            }
        }
        // Analyzer/hash pairing mirrors the sidecar schema loudly: a heuristic
        // must not claim a model, an ONNX analyzer must carry a pin (or the
        // explicit pending marker).
        match (self.analyzer.kind, self.analyzer.model_hash.as_deref()) {
            (CullingAnalyzerKind::Heuristic, None) => {}
            (CullingAnalyzerKind::Heuristic, Some(_)) => {
                return Err(CullError::InvalidProposal(
                    "heuristic analyzer must not carry a model_hash".into(),
                ));
            }
            (CullingAnalyzerKind::Onnx, None) => {
                return Err(CullError::InvalidProposal(
                    "onnx analyzer requires a model_hash pin".into(),
                ));
            }
            (CullingAnalyzerKind::Onnx, Some(hash)) => {
                if hash != CULLING_PENDING_MODEL_HASH {
                    validate_culling_sha256("analyzer.model_hash", hash)
                        .map_err(|e| CullError::InvalidProposal(e.to_string()))?;
                }
            }
        }
        Ok(())
    }

    /// Builds the reproducible identity for this proposal. Changing any part
    /// makes a persisted proposal `stale` (visible), never silently re-run.
    #[must_use]
    pub fn identity(
        &self,
        source: SourceFingerprint,
        decode: DecodeFingerprint,
        geometry: GeometryFingerprint,
    ) -> CullingIdentity {
        CullingIdentity {
            source,
            decode,
            geometry,
            analyzer: self.analyzer.clone(),
            analysis_resolution: self.analysis_resolution.clone(),
            preprocessing: self.preprocessing.clone(),
            extras: Default::default(),
        }
    }

    /// Fills a validated [`CullingSection`] (status `valid`) for persistence on
    /// the source level. `created_at` is caller-supplied (`now_rfc3339_utc` in
    /// production) so the analyzer stays wall-clock free.
    pub fn to_section(
        &self,
        source: SourceFingerprint,
        decode: DecodeFingerprint,
        geometry: GeometryFingerprint,
        created_at: &str,
    ) -> Result<CullingSection, CullError> {
        let section = CullingSection {
            version: lumina_sidecar::CULLING_SCHEMA_VERSION,
            proposal: self.proposal,
            score: self.score,
            reasons: self.reasons.clone(),
            identity: self.identity(source, decode, geometry),
            created_at: created_at.to_string(),
            status: lumina_sidecar::CullingStatus::Valid,
            error: None,
            extras: Default::default(),
        };
        validate_culling_section(&section)
            .map_err(|e| CullError::InvalidProposal(e.to_string()))?;
        Ok(section)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{heuristic_analyzer, heuristic_preprocessing};
    use lumina_sidecar::CULLING_PENDING_MODEL_HASH;
    use std::collections::BTreeMap;

    fn core(score: f32, reasons: &[&str]) -> ProposalCore {
        ProposalCore {
            proposal: CullProposal::Review,
            score,
            reasons: reasons.iter().map(|reason| (*reason).to_string()).collect(),
            analyzer: heuristic_analyzer(),
            analysis_resolution: Resolution {
                width: 100,
                height: 100,
                extras: Default::default(),
            },
            preprocessing: heuristic_preprocessing(),
        }
    }

    fn source() -> SourceFingerprint {
        SourceFingerprint {
            content_hash: "blake3:x".into(),
            byte_length: 1,
            extras: Default::default(),
        }
    }

    fn decode() -> DecodeFingerprint {
        DecodeFingerprint {
            decoder: "d".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Default::default(),
        }
    }

    fn geometry() -> GeometryFingerprint {
        GeometryFingerprint {
            width: 100,
            height: 100,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Default::default(),
        }
    }

    #[test]
    fn to_section_is_deterministic_and_valid() {
        let value = core(0.42, &["sharpness_low"]);
        let first = value
            .to_section(source(), decode(), geometry(), "2026-09-16T00:00:00Z")
            .expect("section");
        let second = value
            .to_section(source(), decode(), geometry(), "2026-09-16T00:00:00Z")
            .expect("section");
        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );
        assert_eq!(first.status, lumina_sidecar::CullingStatus::Valid);
    }

    #[test]
    fn out_of_range_score_and_malformed_reasons_are_rejected() {
        assert!(core(1.5, &[]).validate().is_err());
        assert!(core(-0.1, &[]).validate().is_err());
        assert!(core(f32::NAN, &[]).validate().is_err());
        assert!(core(0.5, &["Sharpness Low"]).validate().is_err());
        assert!(core(0.5, &["ok", "ok"]).validate().is_err());
        assert!(core(0.5, &["sharpness_low"]).validate().is_ok());
    }

    #[test]
    fn analyzer_hash_pairing_is_enforced() {
        let mut heuristic = core(0.5, &[]);
        heuristic.analyzer.model_hash = Some("dummy".into());
        assert!(heuristic.validate().is_err());

        let mut onnx = core(0.5, &[]);
        onnx.analyzer.kind = CullingAnalyzerKind::Onnx;
        assert!(onnx.validate().is_err());
        onnx.analyzer.model_hash = Some(CULLING_PENDING_MODEL_HASH.into());
        assert!(onnx.validate().is_ok());
        onnx.analyzer.model_hash = Some("sha256:not-hex".into());
        assert!(onnx.validate().is_err());
    }

    #[test]
    fn component_mappings_are_monotone_and_clamped() {
        assert_eq!(sharpness_score(-1.0, 0.02), 0.0);
        assert_eq!(sharpness_score(0.0, 0.02), 0.0);
        assert!(sharpness_score(0.02, 0.02) > sharpness_score(0.01, 0.02));
        assert_eq!(noise_score(-1.0, 0.05), 0.0);
        assert!(noise_score(0.1, 0.05) > noise_score(0.05, 0.05));
        assert!(sharpness_score(f64::INFINITY, 0.02) <= 1.0);
    }
}
