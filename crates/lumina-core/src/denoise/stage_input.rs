//! R5-WARN-2 (file-size extraction): the caller-resolved KI-Denoise stage
//! input. Extracted from `denoise.rs` so the (over-500-line) module keeps its
//! ratchet after the `quiet` field was added.

use super::*;

/// Caller-resolved KI-Denoise stage state: the §6 status, an optional
/// matching artifact and the fallback policy.
#[derive(Debug, Clone)]
pub struct DenoiseStageInput<'a> {
    pub status: DenoiseStageStatus,
    pub artifact: Option<&'a DenoiseRgbArtifact>,
    /// Human-readable explanation for a non-`Ready` status (empty otherwise).
    pub reason: String,
    pub policy: DenoisePolicy,
    /// R5-WARN-2: suppress the per-call fallback `warn!`. The GUI sets this
    /// after it has already surfaced the same (status, reason) once, so a
    /// per-tick render loop cannot repeat the identical line. The CLI leaves it
    /// `false` (one command = one visible warning).
    pub quiet: bool,
}

impl Default for DenoiseStageInput<'_> {
    fn default() -> Self {
        Self::inactive()
    }
}

impl<'a> DenoiseStageInput<'a> {
    /// The stage is not resolved by the caller. With an active recipe this is
    /// treated as `Unavailable` (loud under `Strict`), never as a silent no-op.
    #[must_use]
    pub fn inactive() -> Self {
        Self {
            status: DenoiseStageStatus::Inactive,
            artifact: None,
            reason: String::new(),
            policy: DenoisePolicy::Strict,
            quiet: false,
        }
    }

    /// A matching, verified artifact.
    #[must_use]
    pub fn ready(artifact: &'a DenoiseRgbArtifact) -> Self {
        Self {
            status: DenoiseStageStatus::Ready,
            artifact: Some(artifact),
            reason: String::new(),
            policy: DenoisePolicy::Strict,
            quiet: false,
        }
    }

    /// A visible non-ready state (unavailable/stale/missing/corrupt).
    #[must_use]
    pub fn non_ready(status: DenoiseStageStatus, reason: impl Into<String>) -> Self {
        Self {
            status,
            artifact: None,
            reason: reason.into(),
            policy: DenoisePolicy::Strict,
            quiet: false,
        }
    }

    /// Selects the fallback policy ([`DenoisePolicy`]).
    #[must_use]
    pub fn with_policy(mut self, policy: DenoisePolicy) -> Self {
        self.policy = policy;
        self
    }
}
