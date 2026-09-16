//! Stage-2 (optional ONNX model) interface and capability gate.
//!
//! This module is **not** an ONNX implementation. It pins the calibrated
//! interface a later ONNX slice (`lumina-onnx`, F-078 licence check, model hash
//! pinning) must satisfy and — crucially — encodes the decision that a missing
//! Stage 2 is a hard, visible state: [`analyze_stage2`] never falls back to the
//! heuristic, and the heuristic path never silently runs Stage 2. The caller
//! chooses the stage explicitly and shows "no proposal" when the requested
//! stage is unavailable (decisions §4: `RuntimeDisabled` → „kein Vorschlag").

use lumina_sidecar::{CullingAnalyzer, CullingAnalyzerKind, Preprocessing, Resolution};

use crate::analyze::CullSourceInput;
use crate::config::CullConfig;
use crate::score::{propose, ProposalCore};
use crate::CullError;

/// Capability state of the optional Stage-2 runtime, evaluated by the caller
/// (native CLI/desktop). Mirrors the AI-mask capability model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage2Availability {
    /// An `onnx-rt` backend is present and the model is licence-checked/pinned.
    Available,
    /// The ONNX runtime is disabled/absent — "no proposal", never a fallback.
    RuntimeDisabled,
    /// The runtime exists but the pinned model artifact is missing.
    ModelMissing,
    /// F-078 licence check has not cleared the model for use.
    LicenceUnverified,
}

impl Stage2Availability {
    /// Whether Stage 2 may run at all.
    #[must_use]
    pub fn is_available(self) -> bool {
        matches!(self, Stage2Availability::Available)
    }
}

/// Why Stage-2 analysis could not run. Reported verbatim; never swallowed into
/// a heuristic result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage2UnavailableReason {
    /// The caller requested Stage 2 without providing a backend.
    NoBackend,
    /// The ONNX runtime is disabled/absent.
    RuntimeDisabled,
    /// The pinned model artifact is missing.
    ModelMissing,
    /// The model has no cleared licence (F-078).
    LicenceUnverified,
}

/// Raw output of a Stage-2 model evaluation (score + reason codes). The model
/// writes no recipe state.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelOutput {
    /// Model keep-worthiness `0..=1` on the documented scale.
    pub score: f32,
    /// Machine-readable reason codes (open registry).
    pub reasons: Vec<String>,
}

/// The Stage-2 backend contract. Implementations live in `lumina-onnx` (or the
/// caller) and must return an `onnx` [`CullingAnalyzer`] with a pinned
/// `sha256:<hex>` model hash (or the explicit `pending-integration` marker
/// while the licence gate is open).
pub trait QualityModelBackend {
    /// Analyzer/model identity declared by the backend.
    fn identity(&self) -> CullingAnalyzer;
    /// Resolution the model runs at (part of the persisted identity).
    fn analysis_resolution(&self) -> Resolution;
    /// Preprocessing identity (input transform + version).
    fn preprocessing(&self) -> Preprocessing;
    /// Runs the model over one decoded source image.
    fn analyze(&self, input: &CullSourceInput<'_>) -> Result<ModelOutput, CullError>;
}

/// Runs a Stage-2 analysis or fails loudly.
///
/// - `availability != Available` → [`CullError::Stage2Unavailable`] with the
///   matching reason, regardless of any backend (no heuristic fallback);
/// - `Available` without a backend → `NoBackend` (still loud);
/// - `Available` with a backend → the model output, validated (score range,
///   reason shape, `onnx` kind + hash pin) before it can reach the sidecar.
pub fn analyze_stage2(
    input: &CullSourceInput<'_>,
    config: &CullConfig,
    availability: Stage2Availability,
    backend: Option<&dyn QualityModelBackend>,
) -> Result<ProposalCore, CullError> {
    config.validate()?;
    if let Some(reason) = match availability {
        Stage2Availability::Available => None,
        Stage2Availability::RuntimeDisabled => Some(Stage2UnavailableReason::RuntimeDisabled),
        Stage2Availability::ModelMissing => Some(Stage2UnavailableReason::ModelMissing),
        Stage2Availability::LicenceUnverified => Some(Stage2UnavailableReason::LicenceUnverified),
    } {
        return Err(CullError::Stage2Unavailable { reason });
    }
    let Some(backend) = backend else {
        return Err(CullError::Stage2Unavailable {
            reason: Stage2UnavailableReason::NoBackend,
        });
    };
    let analyzer = backend.identity();
    if analyzer.kind != CullingAnalyzerKind::Onnx {
        return Err(CullError::InvalidProposal(
            "stage-2 backend identity must use kind `onnx`".into(),
        ));
    }
    let output = backend.analyze(input)?;
    let core = ProposalCore {
        proposal: propose(f64::from(output.score), config),
        score: output.score,
        reasons: output.reasons,
        analyzer,
        analysis_resolution: backend.analysis_resolution(),
        preprocessing: backend.preprocessing(),
    };
    core.validate()?;
    Ok(core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::ImageFrame;
    use lumina_sidecar::{CullProposal, CULLING_PENDING_MODEL_HASH};
    use std::collections::BTreeMap;

    struct FakeBackend {
        kind: CullingAnalyzerKind,
        hash: Option<String>,
        score: f32,
    }

    impl QualityModelBackend for FakeBackend {
        fn identity(&self) -> CullingAnalyzer {
            CullingAnalyzer {
                kind: self.kind,
                name: "fake-model".into(),
                version: "1".into(),
                model_hash: self.hash.clone(),
                extras: Default::default(),
            }
        }

        fn analysis_resolution(&self) -> Resolution {
            Resolution {
                width: 64,
                height: 64,
                extras: Default::default(),
            }
        }

        fn preprocessing(&self) -> Preprocessing {
            Preprocessing {
                name: "fake_pre".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Default::default(),
            }
        }

        fn analyze(&self, _input: &CullSourceInput<'_>) -> Result<ModelOutput, CullError> {
            Ok(ModelOutput {
                score: self.score,
                reasons: vec!["model_keep".into()],
            })
        }
    }

    fn frame() -> ImageFrame {
        ImageFrame::new(
            2,
            2,
            vec![
                10, 10, 10, 255, 20, 20, 20, 255, 30, 30, 30, 255, 40, 40, 40, 255,
            ],
        )
        .expect("exact buffer")
    }

    fn onnx_backend(score: f32) -> FakeBackend {
        FakeBackend {
            kind: CullingAnalyzerKind::Onnx,
            hash: Some(CULLING_PENDING_MODEL_HASH.into()),
            score,
        }
    }

    #[test]
    fn unavailable_stages_never_fall_back_to_the_heuristic() {
        let frame = frame();
        let input = CullSourceInput {
            frame: &frame,
            iso: None,
        };
        let config = CullConfig::default();
        let backend = onnx_backend(0.9);
        for (availability, reason) in [
            (
                Stage2Availability::RuntimeDisabled,
                Stage2UnavailableReason::RuntimeDisabled,
            ),
            (
                Stage2Availability::ModelMissing,
                Stage2UnavailableReason::ModelMissing,
            ),
            (
                Stage2Availability::LicenceUnverified,
                Stage2UnavailableReason::LicenceUnverified,
            ),
        ] {
            let error = analyze_stage2(&input, &config, availability, Some(&backend))
                .expect_err("unavailable stage must fail loudly");
            assert!(matches!(
                error,
                CullError::Stage2Unavailable { reason: actual } if actual == reason
            ));
        }
        let error = analyze_stage2(&input, &config, Stage2Availability::Available, None)
            .expect_err("available without a backend is still loud");
        assert!(matches!(
            error,
            CullError::Stage2Unavailable {
                reason: Stage2UnavailableReason::NoBackend
            }
        ));
    }

    #[test]
    fn available_backend_yields_a_validated_proposal() {
        let frame = frame();
        let input = CullSourceInput {
            frame: &frame,
            iso: None,
        };
        let core = analyze_stage2(
            &input,
            &CullConfig::default(),
            Stage2Availability::Available,
            Some(&onnx_backend(0.95)),
        )
        .expect("stage-2 proposal");
        assert_eq!(core.proposal, CullProposal::Keep);
        assert_eq!(core.analyzer.kind, CullingAnalyzerKind::Onnx);
        assert_eq!(core.analysis_resolution.width, 64);
        assert_eq!(core.reasons, vec!["model_keep".to_string()]);
    }

    #[test]
    fn non_onnx_backend_identity_and_bad_score_are_rejected() {
        let frame = frame();
        let input = CullSourceInput {
            frame: &frame,
            iso: None,
        };
        let config = CullConfig::default();
        let heuristic = FakeBackend {
            kind: CullingAnalyzerKind::Heuristic,
            hash: None,
            score: 0.5,
        };
        assert!(analyze_stage2(
            &input,
            &config,
            Stage2Availability::Available,
            Some(&heuristic)
        )
        .is_err());
        assert!(analyze_stage2(
            &input,
            &config,
            Stage2Availability::Available,
            Some(&onnx_backend(1.5))
        )
        .is_err());
    }
}
