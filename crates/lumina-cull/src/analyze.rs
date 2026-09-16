//! The Stage-1 heuristic analyzer: per-image signals plus the explicit-selection
//! duplicate/series pass.
//!
//! `analyze_heuristic` is a pure function of the pixels (plus optional ISO and
//! the config); `analyze_selection` additionally compares signatures **only**
//! within the passed slice. Neither touches the filesystem, wall clock or
//! randomness.

use std::collections::BTreeSet;

use lumina_core::{downscale_bilinear, ImageFrame};
use lumina_sidecar::{CullProposal, Resolution};
use serde::{Deserialize, Serialize};

use crate::config::CullConfig;
use crate::identity::{heuristic_analyzer, heuristic_preprocessing};
use crate::score::{
    base_score, exposure_component, noise_score, propose, sharpness_score, ProposalCore,
};
use crate::signals::{blur_3x3, exposure_metrics, gradient_stats, luma_plane, noise_sigma};
use crate::similarity::{
    group_similar, similarity_signature, SimilarityCandidate, SimilarityGroup, SimilarityKind,
    SimilaritySignature,
};
use crate::{
    CullError, REASON_DUPLICATE_GROUP, REASON_EXPOSURE_CLIPPED, REASON_EXPOSURE_OVEREXPOSED,
    REASON_EXPOSURE_UNDEREXPOSED, REASON_MOTION_BLUR_SUSPECT, REASON_NOISE_HIGH_ISO,
    REASON_SERIES_GROUP, REASON_SHARPNESS_LOW,
};

/// One source image offered to the analyzer. `iso` is optional EXIF metadata;
/// when absent the noise reason uses the neutral threshold.
#[derive(Debug, Clone, Copy)]
pub struct CullSourceInput<'a> {
    /// Decoded RGBA8 frame (already EXIF-oriented by the caller; geometry is
    /// recorded separately in the persisted identity).
    pub frame: &'a ImageFrame,
    /// Optional ISO sensitivity from EXIF (modulates the noise reason only).
    pub iso: Option<u32>,
}

/// Deterministic raw measures behind one analysis (for tests, reports and
/// tuning). Serialized with the analysis so a CLI `--json` slice can surface
/// *why* a recommendation was made.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnalysisDiagnostics {
    /// Structural sharpness component `0..=1` (higher = sharper).
    pub sharpness_score: f64,
    /// Exposure component `0..=1` (higher = better exposed).
    pub exposure_score: f64,
    /// Noise score `0..=1` (higher = noisier).
    pub noise_score: f64,
    /// Immerkaer noise sigma on the analysis frame.
    pub noise_sigma: f64,
    /// Tenengrad structural energy on the blurred analysis frame.
    pub tenengrad: f64,
    /// Directional sharpness (weakest gradient direction) used for the score.
    pub directional_sharpness: f64,
    /// Variance of the 4-neighbour Laplacian.
    pub laplacian_variance: f64,
    /// Gradient anisotropy `0..=1` (motion-blur indicator).
    pub anisotropy: f64,
    /// Combined shadow + highlight clipping fraction.
    pub clip_fraction: f64,
    /// Darkest-bin fraction.
    pub shadow_clip_fraction: f64,
    /// Brightest-bin fraction.
    pub highlight_clip_fraction: f64,
    /// Mean luminance of the analysis frame.
    pub mean_luminance: f64,
    /// Median luminance of the analysis frame.
    pub median_luminance: f64,
    /// Analysis frame width (equals the persisted analysis resolution).
    pub analysis_width: u32,
    /// Analysis frame height (equals the persisted analysis resolution).
    pub analysis_height: u32,
}

/// One analyzed source image: the sidecar-ready proposal core, its
/// diagnostics, and its membership in a similar group of the selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeuristicAnalysis {
    /// Proposal payload (recommendation, score, reasons, identity).
    pub core: ProposalCore,
    /// Raw deterministic measures.
    pub diagnostics: AnalysisDiagnostics,
    /// Index into [`SelectionAnalysis::groups`] when this image belongs to a
    /// similar group within the explicit selection.
    pub similar_group: Option<usize>,
    /// True when this image is a redundant (non-best) group member.
    pub similar_redundant: bool,
}

impl HeuristicAnalysis {
    /// Recommendation (`keep` / `review` / `reject-kandidat`).
    #[must_use]
    pub fn proposal(&self) -> CullProposal {
        self.core.proposal
    }

    /// Keep-worthiness `0..=1` (after any group penalty).
    #[must_use]
    pub fn score(&self) -> f32 {
        self.core.score
    }

    /// Machine-readable reason codes.
    #[must_use]
    pub fn reasons(&self) -> &[String] {
        &self.core.reasons
    }
}

/// Result of [`analyze_selection`]: one analysis per input plus the similar
/// groups found within that explicit selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionAnalysis {
    /// Per-input analyses, in input order.
    pub images: Vec<HeuristicAnalysis>,
    /// Similar groups (empty when the selection has no near-duplicates/series).
    pub groups: Vec<SimilarityGroup>,
}

/// Analyzes a single source image. No similar-group pass runs here, so
/// `duplicate_group`/`series_group` are only emitted by [`analyze_selection`].
pub fn analyze_heuristic(
    input: &CullSourceInput<'_>,
    config: &CullConfig,
) -> Result<HeuristicAnalysis, CullError> {
    analyze_one(input, config).map(|(analysis, _)| analysis)
}

/// Analyzes an explicit selection and attaches duplicate/series reasons and the
/// redundant-member penalty. The selection boundary is the caller's; this
/// function never expands it.
pub fn analyze_selection(
    inputs: &[CullSourceInput<'_>],
    config: &CullConfig,
) -> Result<SelectionAnalysis, CullError> {
    config.validate()?;
    let mut images = Vec::with_capacity(inputs.len());
    let mut candidates = Vec::with_capacity(inputs.len());
    for input in inputs {
        let (analysis, signature) = analyze_one(input, config)?;
        candidates.push(SimilarityCandidate {
            signature,
            intrinsic_score: f64::from(analysis.core.score),
        });
        images.push(analysis);
    }

    let groups = group_similar(&candidates, config);
    for (group_index, group) in groups.iter().enumerate() {
        let reason = match group.kind {
            SimilarityKind::Duplicate => REASON_DUPLICATE_GROUP,
            SimilarityKind::Series => REASON_SERIES_GROUP,
        };
        for &member in &group.members {
            let analysis = &mut images[member];
            analysis.similar_group = Some(group_index);
            insert_reason(&mut analysis.core.reasons, reason);
            if member != group.best {
                analysis.similar_redundant = true;
                let penalized = (f64::from(analysis.core.score) - config.duplicate_score_penalty)
                    .clamp(0.0, 1.0) as f32;
                analysis.core.score = penalized;
                analysis.core.proposal = propose(f64::from(penalized), config);
            }
            analysis.core.validate()?;
        }
    }

    Ok(SelectionAnalysis { images, groups })
}

/// Shared per-image pass: returns the analysis and the content signature so
/// [`analyze_selection`] never recomputes the downscale/luma work.
fn analyze_one(
    input: &CullSourceInput<'_>,
    config: &CullConfig,
) -> Result<(HeuristicAnalysis, SimilaritySignature), CullError> {
    config.validate()?;
    if input.frame.width == 0 || input.frame.height == 0 {
        return Err(CullError::EmptyFrame {
            width: input.frame.width,
            height: input.frame.height,
        });
    }
    let analysis_frame = downscale_bilinear(input.frame, config.analysis_max_width)?;
    if analysis_frame.width == 0 || analysis_frame.height == 0 {
        return Err(CullError::EmptyFrame {
            width: analysis_frame.width,
            height: analysis_frame.height,
        });
    }

    let plane = luma_plane(&analysis_frame)?;
    let blurred = blur_3x3(&plane);
    let gradients = gradient_stats(&blurred);
    let sigma = noise_sigma(&plane, &blurred);
    let exposure = exposure_metrics(&analysis_frame)?;

    let sharpness = sharpness_score(gradients.directional_sharpness, config.sharpness_scale);
    let noise = noise_score(sigma, config.noise_scale);
    let exposure_score = exposure_component(&exposure, config);
    let base = base_score(sharpness, exposure_score, 1.0 - noise, config);

    let reasons = collect_reasons(sharpness, &gradients, &exposure, noise, input.iso, config);
    // The stored `f32` score is the single source of truth for the
    // recommendation, so `proposal == propose(score)` holds exactly (no
    // f64→f32 rounding across a threshold).
    let score = base as f32;
    let proposal = propose(f64::from(score), config);
    let core = ProposalCore {
        proposal,
        score,
        reasons,
        analyzer: heuristic_analyzer(),
        analysis_resolution: Resolution {
            width: analysis_frame.width,
            height: analysis_frame.height,
            extras: Default::default(),
        },
        preprocessing: heuristic_preprocessing(),
    };
    core.validate()?;

    let signature = similarity_signature(&analysis_frame)?;
    let diagnostics = AnalysisDiagnostics {
        sharpness_score: sharpness,
        exposure_score,
        noise_score: noise,
        noise_sigma: sigma,
        tenengrad: gradients.tenengrad,
        directional_sharpness: gradients.directional_sharpness,
        laplacian_variance: gradients.laplacian_variance,
        anisotropy: gradients.anisotropy,
        clip_fraction: exposure.clip_fraction,
        shadow_clip_fraction: exposure.shadow_clip_fraction,
        highlight_clip_fraction: exposure.highlight_clip_fraction,
        mean_luminance: exposure.mean,
        median_luminance: exposure.median,
        analysis_width: analysis_frame.width,
        analysis_height: analysis_frame.height,
    };
    Ok((
        HeuristicAnalysis {
            core,
            diagnostics,
            similar_group: None,
            similar_redundant: false,
        },
        signature,
    ))
}

/// Effective `noise_high_iso` threshold for an optional ISO value.
pub(crate) fn noise_threshold(iso: Option<u32>, config: &CullConfig) -> f64 {
    match iso {
        Some(value) if value >= CullConfig::HIGH_ISO => config.noise_score_threshold_high_iso,
        Some(value) if value < CullConfig::LOW_ISO => config.noise_score_threshold_low_iso,
        _ => config.noise_score_threshold,
    }
}

fn collect_reasons(
    sharpness: f64,
    gradients: &crate::signals::GradientStats,
    exposure: &crate::signals::ExposureMetrics,
    noise: f64,
    iso: Option<u32>,
    config: &CullConfig,
) -> Vec<String> {
    let mut codes: BTreeSet<&'static str> = BTreeSet::new();
    if sharpness < config.sharpness_low_threshold {
        codes.insert(REASON_SHARPNESS_LOW);
    }
    if sharpness < config.motion_blur_sharpness_threshold
        && gradients.anisotropy >= config.motion_blur_anisotropy_threshold
    {
        codes.insert(REASON_MOTION_BLUR_SUSPECT);
    }
    if exposure.clip_fraction > config.clip_fraction_threshold {
        codes.insert(REASON_EXPOSURE_CLIPPED);
    }
    if exposure.mean < config.underexposed_mean_threshold {
        codes.insert(REASON_EXPOSURE_UNDEREXPOSED);
    }
    if exposure.mean > config.overexposed_mean_threshold {
        codes.insert(REASON_EXPOSURE_OVEREXPOSED);
    }
    if noise >= noise_threshold(iso, config) {
        codes.insert(REASON_NOISE_HIGH_ISO);
    }
    codes.into_iter().map(String::from).collect()
}

/// Inserts a reason code keeping the vector sorted and unique (deterministic
/// serialization order).
fn insert_reason(reasons: &mut Vec<String>, code: &str) {
    if reasons.iter().any(|existing| existing == code) {
        return;
    }
    reasons.push(code.to_string());
    reasons.sort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CullError;

    fn frame(width: u32, height: u32, value: u8) -> ImageFrame {
        let pixels = (0..(width * height))
            .flat_map(|_| [value, value, value, 255])
            .collect();
        ImageFrame::new(width, height, pixels).expect("exact buffer")
    }

    #[test]
    fn empty_frames_and_invalid_config_are_loud() {
        let empty = ImageFrame::new(0, 0, vec![]).unwrap();
        let input = CullSourceInput {
            frame: &empty,
            iso: None,
        };
        assert!(matches!(
            analyze_heuristic(&input, &CullConfig::default()),
            Err(CullError::EmptyFrame { .. })
        ));

        let config = CullConfig {
            keep_threshold: 0.1,
            reject_threshold: 0.9,
            ..CullConfig::default()
        };
        let valid = frame(8, 8, 128);
        let input = CullSourceInput {
            frame: &valid,
            iso: None,
        };
        assert!(matches!(
            analyze_heuristic(&input, &config),
            Err(CullError::InvalidConfig(_))
        ));
    }

    #[test]
    fn iso_selects_the_documented_noise_threshold() {
        let config = CullConfig::default();
        assert_eq!(noise_threshold(None, &config), config.noise_score_threshold);
        assert_eq!(
            noise_threshold(Some(CullConfig::HIGH_ISO), &config),
            config.noise_score_threshold_high_iso
        );
        assert_eq!(
            noise_threshold(Some(800), &config),
            config.noise_score_threshold
        );
        assert_eq!(
            noise_threshold(Some(CullConfig::LOW_ISO - 1), &config),
            config.noise_score_threshold_low_iso
        );
    }
}
