//! LRPAR-G09-CULL-25 / Heuristik-Slice: deterministic Stage-1 assisted culling.
//!
//! SOLL: `feature/decisions/LRPAR-G09-CULL-25.md` (§2 Vorschlags-Sichtung,
//! §4 Stufe 1, §5 Persistenz, §7 Folge-Task 2). KI-Culling in LuminaRust is
//! **assisted culling only**: this crate computes one reasoned recommendation
//! per source image (`keep` / `review` / `reject-kandidat` plus a documented
//! score and machine-readable reason codes). It **never** writes `rating`,
//! `flag`, `color_label` or any other recipe field, and it never inspects the
//! filesystem on its own.
//!
//! ## Scope of this slice
//!
//! - Pure Rust, no ONNX, no weights, no randomness, no wall clock, no threads.
//!   Stage 2 (optional ONNX quality/aesthetic model) is *only* an interface and
//!   capability gate here ([`stage2`]); it is never silently substituted by the
//!   heuristic and the heuristic is never silently replaced by it.
//! - Signals (Stage 1, §4): sharpness/blur, clipping/exposure (reusing the
//!   existing [`lumina_core::analyze_tone_with_histogram`] path — no second
//!   algorithm for the same measurement), a pixel noise estimate, and
//!   duplicate/series similarity that only ever compares images the caller
//!   passed in one explicit selection (`analyze_selection`). There is no
//!   folder-crossing scan and no automatic directory traversal.
//! - Explicitly **not** in Stage 1: face/eye signals (LRPAR-G12-FACE-20).
//!
//! ## Score scale (documented, deterministic)
//!
//! [`ProposalCore::score`] is a `0..=1` **keep-worthiness**: `1.0` = strong
//! keep, `0.0` = strong reject candidate. It is a weighted blend of three
//! component scores, each itself in `0..=1` and monotone in its raw measure:
//!
//! ```text
//! base = (w_sharpness * sharpness + w_exposure * exposure + w_noise * (1 - noise)) / (sum w)
//! score = clamp(base - duplicate_penalty, 0, 1)
//! ```
//!
//! Defaults ([`CullConfig`]): `w_sharpness = 0.6`, `w_exposure = 0.25`,
//! `w_noise = 0.15`. The recommendation is then a pure threshold decision on
//! the score: `score <= reject_threshold` → `reject-kandidat`,
//! `score >= keep_threshold` → `keep`, otherwise `review`. Raising the score
//! can never lower the recommendation category (property-tested).
//!
//! ## Baseline reason-code registry (open registry — schema validates shape)
//!
//! The sidecar validates reason codes only for shape (`[a-z0-9_]`, §5). These
//! are the codes this Stage-1 analyzer may emit; the registry stays open, so a
//! newer analyzer may add codes without a schema bump:
//!
//! | code | meaning | signal |
//! | --- | --- | --- |
//! | [`REASON_SHARPNESS_LOW`] | `sharpness_low` | structural detail below the configured score threshold |
//! | [`REASON_MOTION_BLUR_SUSPECT`] | `motion_blur_suspect` | low sharpness **and** strongly anisotropic gradients (one direction smeared) |
//! | [`REASON_EXPOSURE_CLIPPED`] | `exposure_clipped` | shadow + highlight clipping fraction above threshold (tone histogram path) |
//! | [`REASON_EXPOSURE_UNDEREXPOSED`] | `exposure_underexposed` | mean luminance below the dark threshold |
//! | [`REASON_EXPOSURE_OVEREXPOSED`] | `exposure_overexposed` | mean luminance above the bright threshold |
//! | [`REASON_NOISE_HIGH_ISO`] | `noise_high_iso` | Immerkaer pixel-noise estimate above threshold (optionally ISO-modulated) |
//! | [`REASON_DUPLICATE_GROUP`] | `duplicate_group` | near-duplicate of another image in the explicit selection |
//! | [`REASON_SERIES_GROUP`] | `series_group` | burst/series resemblance of another image in the explicit selection |
//!
//! All codes are listed in [`BASELINE_REASON_CODES`].
//!
//! ## Determinism
//!
//! Identical inputs (pixels, fingerprints, config, `created_at`) produce
//! bitwise-identical analyses and byte-identical sidecar section JSON on the
//! same build. `created_at` is always supplied by the caller
//! ([`lumina_sidecar::now_rfc3339_utc`] in production, a literal in tests) so
//! the analyzer itself is wall-clock free.
//!
//! ## Persistence
//!
//! [`sidecar_io`] fills and reads [`lumina_sidecar::CullingSection`] through the
//! existing API: writing is validated and loud, reading distinguishes
//! `no proposal` / `valid` / `stale` / `unusable` explicitly. A stale or
//! missing section is a visible state — never an invented recommendation and
//! never a hidden re-run.

use thiserror::Error;

pub mod analyze;
pub mod config;
pub mod identity;
pub mod score;
pub mod sidecar_io;
pub mod signals;
pub mod similarity;
pub mod stage2;
// This module holds real `proptest` properties.
#[cfg(test)]
mod props;

pub use analyze::{analyze_heuristic, analyze_selection, CullSourceInput, HeuristicAnalysis};
pub use config::CullConfig;
pub use identity::{
    heuristic_analyzer, heuristic_identity, heuristic_preprocessing, identity_mismatches,
    IdentityMismatch, HEURISTIC_ANALYZER_KIND, HEURISTIC_ANALYZER_NAME, HEURISTIC_ANALYZER_VERSION,
    HEURISTIC_PREPROCESSING_NAME, HEURISTIC_PREPROCESSING_VERSION,
};
pub use score::{
    base_score, exposure_component, noise_component, noise_score, propose, sharpness_score,
    ProposalCore,
};
pub use sidecar_io::{
    clear_culling, evaluate_culling, evaluate_section, load_culling, record_culling, save_culling,
    CullingReadState,
};
pub use similarity::{
    group_similar, histogram_distance, perceptual_hash, similarity_signature, SimilarityGroup,
    SimilarityKind, SimilaritySignature,
};
pub use stage2::{
    analyze_stage2, QualityModelBackend, Stage2Availability, Stage2UnavailableReason,
};

/// Baseline reason code: structural sharpness below the configured threshold.
pub const REASON_SHARPNESS_LOW: &str = "sharpness_low";
/// Baseline reason code: low sharpness plus anisotropic gradients (motion blur).
pub const REASON_MOTION_BLUR_SUSPECT: &str = "motion_blur_suspect";
/// Baseline reason code: shadow/highlight clipping above threshold.
pub const REASON_EXPOSURE_CLIPPED: &str = "exposure_clipped";
/// Baseline reason code: mean luminance below the dark threshold.
pub const REASON_EXPOSURE_UNDEREXPOSED: &str = "exposure_underexposed";
/// Baseline reason code: mean luminance above the bright threshold.
pub const REASON_EXPOSURE_OVEREXPOSED: &str = "exposure_overexposed";
/// Baseline reason code: pixel noise estimate above threshold.
pub const REASON_NOISE_HIGH_ISO: &str = "noise_high_iso";
/// Baseline reason code: near-duplicate within the explicit selection.
pub const REASON_DUPLICATE_GROUP: &str = "duplicate_group";
/// Baseline reason code: burst/series resemblance within the explicit selection.
pub const REASON_SERIES_GROUP: &str = "series_group";

/// The complete baseline Stage-1 reason-code registry documented in the module
/// header. Open registry: consumers must tolerate unknown codes.
pub const BASELINE_REASON_CODES: &[&str] = &[
    REASON_SHARPNESS_LOW,
    REASON_MOTION_BLUR_SUSPECT,
    REASON_EXPOSURE_CLIPPED,
    REASON_EXPOSURE_UNDEREXPOSED,
    REASON_EXPOSURE_OVEREXPOSED,
    REASON_NOISE_HIGH_ISO,
    REASON_DUPLICATE_GROUP,
    REASON_SERIES_GROUP,
];

/// Errors of the culling analysis and its sidecar binding.
///
/// Failures are loud: a missing model, a stale proposal or a malformed frame is
/// reported as a visible state/error, never replaced by a guessed result.
#[derive(Debug, Error)]
pub enum CullError {
    /// The input frame had zero width or height (cannot be analyzed).
    #[error("culling analysis requires a non-empty frame, got {width}x{height}")]
    EmptyFrame {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
    },
    /// A configuration value was outside its documented domain.
    #[error("invalid culling configuration: {0}")]
    InvalidConfig(String),
    /// A reason code or proposal was outside the sidecar/analyzer contract.
    #[error("invalid culling proposal: {0}")]
    InvalidProposal(String),
    /// Stage-2 ONNX analysis is not available; there is no silent fallback.
    #[error("stage-2 culling analysis unavailable: {reason:?}")]
    Stage2Unavailable {
        /// Why Stage 2 cannot run.
        reason: Stage2UnavailableReason,
    },
    /// A shared `lumina-core` measurement step failed (e.g. downscale).
    #[error("culling measurement failed: {0}")]
    Core(#[from] lumina_core::CoreError),
    /// A sidecar read/write/serialization step failed.
    #[error("culling sidecar error: {0}")]
    Sidecar(#[from] lumina_sidecar::SidecarError),
}
