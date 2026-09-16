//! LRPAR-G14-DENOISE-IMPL-20: core pipeline stage for the optional
//! `recipe.adjustments.denoise_ai` KI-Denoise stage (Release 2.0).
//!
//! SOLL: `feature/architecture/pipeline.md` §F-096a and the decision
//! `feature/decisions/LRPAR-G14-DENOISE-20.md` (§2 ordering, §5 persistence,
//! §6 status model).
//!
//! This module is model-free and I/O-free: it consumes a caller-resolved,
//! deterministic RGB8 artifact (the ONNX/CLI slice loads and verifies it from
//! the `.lumina.zdata` bundle) and implements
//!
//! * the deterministic `strength`/`preserve_detail` blend,
//! * the canonical, seam-free tile assembly used by a tiled producer,
//! * the visible §6 status classification (`unavailable`/`stale`/`missing`/
//!   `corrupt`/`ready`/`inactive`),
//! * the loud `Strict` refusal or the visible `Warn` fallback to the manual
//!   F-096 noise reduction — never a silent no-op.
//!
//! Ordering (normative): `… → colour → DenoiseAI → NoiseReduction (F-096) →
//! Sharpening (F-095) → …`. The stage is applied inside the `Adjustments`
//! sub-stage, immediately before the manual noise reduction, so the manual NR
//! stays the visible fallback anchor.

use log::warn;
use serde::{Deserialize, Serialize};

use lumina_sidecar::DenoiseAi;

use crate::{CoreError, ImageFrame};

/// Canonical RGB8 payload encoding version, mirrored by
/// `lumina_sidecar::RGB_ENCODING_VERSION`. Any change invalidates persisted
/// `denoise_rgb` records visibly.
pub const DENOISE_RGB_ENCODING_VERSION: u32 = 1;

/// Fixed detail-measurement scale in 8-bit luminance units. The local detail
/// weight is `clamp(|Y - box_mean_3x3(Y)| / DENOISE_DETAIL_SCALE, 0, 1)`; the
/// constant is part of the documented blend contract, so changing it is a
/// visible behaviour change.
pub const DENOISE_DETAIL_SCALE: f32 = 32.0;

/// A full-frame denoised RGB8 result (no alpha). The render pipeline keeps the
/// source alpha; only the three colour channels are replaced/blended.
///
/// `pixels` is row-major, `width * height * 3` bytes. The canonical raw stream
/// (`encoding_version || width || height || pixels`) is byte-identical to the
/// sidecar's `DenoiseRgbArtifact`, so [`DenoiseRgbArtifact::checksum`] equals
/// the zdata record checksum and the recipe's `DenoiseArtifactRef.checksum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenoiseRgbArtifact {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl DenoiseRgbArtifact {
    /// Builds an artifact, validating the `width * height * 3` invariant
    /// loudly (overflow-safe).
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, CoreError> {
        let artifact = Self {
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// Loud dimension/pixel-count validation.
    pub fn validate(&self) -> Result<(), CoreError> {
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|value| value.checked_mul(3));
        if expected != Some(self.pixels.len()) {
            return Err(CoreError::Denoise {
                status: DenoiseStageStatus::Corrupt.as_str().into(),
                reason: format!(
                    "denoise artifact {}x{} needs {} RGB bytes, got {}",
                    self.width,
                    self.height,
                    expected.unwrap_or(0),
                    self.pixels.len()
                ),
            });
        }
        Ok(())
    }

    /// Canonical pre-compression encoding. Identical to the sidecar zdata
    /// `denoise_rgb` payload and the core checksum contract.
    fn encode_raw(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(12 + self.pixels.len());
        raw.extend_from_slice(&DENOISE_RGB_ENCODING_VERSION.to_le_bytes());
        raw.extend_from_slice(&self.width.to_le_bytes());
        raw.extend_from_slice(&self.height.to_le_bytes());
        raw.extend_from_slice(&self.pixels);
        raw
    }

    /// BLAKE3 hex digest of the canonical uncompressed RGB stream. The recipe
    /// stores exactly this value in `DenoiseArtifactRef.checksum`.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }
}

/// Visible status of the KI-Denoise stage (decision §6). `Inactive` covers the
/// identity cases (field `None`, `enabled == false`, `strength == 0`), `Ready`
/// means a matching, verified artifact was supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenoiseStageStatus {
    /// Field absent or a defined identity (`enabled == false`/`strength == 0`).
    Inactive,
    /// A matching, checksum-verified artifact is available.
    Ready,
    /// No usable model/weights (`pending-integration`, no local model path).
    Unavailable,
    /// Source/decode/model/input-spec changed since the artifact was produced.
    Stale,
    /// The referenced artifact record/file is absent.
    Missing,
    /// The artifact exists but is unusable (checksum/dimension/format).
    Corrupt,
}

impl DenoiseStageStatus {
    /// Stable lower-case status string, used in errors, logs and reports.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DenoiseStageStatus::Inactive => "inactive",
            DenoiseStageStatus::Ready => "ready",
            DenoiseStageStatus::Unavailable => "unavailable",
            DenoiseStageStatus::Stale => "stale",
            DenoiseStageStatus::Missing => "missing",
            DenoiseStageStatus::Corrupt => "corrupt",
        }
    }
}

/// How a non-`Ready` active stage is handled (analogy: `MaskPolicy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenoisePolicy {
    /// Abort the render loudly ([`CoreError::Denoise`]).
    Strict,
    /// Log `warn!` and fall through to the manual F-096 noise reduction
    /// (or identity) — an explicitly surfaced fallback, never a silent one.
    Warn,
}

/// The identity fields the §6 validity rule compares. The caller fills one
/// value from the live source/decode/model context (`current`) and one from
/// what the persisted artifact was produced with (`recorded`). All fields are
/// opaque strings; comparison is exact.
///
/// Persistence (LRPAR-G14-DENOISE-IMPL-20, B2): `DenoiseAi`/`DenoiseRgbArtifact`
/// carry no producer provenance of their own, so the recorded identity is
/// persisted in [`DenoiseAi::extras`] via [`set_denoise_producer_provenance`]
/// and read back with [`denoise_producer_provenance`] (the flattened `extras`
/// make this a roundtrip-legal, additive location — no schema change, no
/// migration). A missing/garbage provenance reads as `None` and must then be
/// treated as `stale` by the caller, never as silently valid.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DenoiseIdentity {
    pub source_content_hash: String,
    pub decode_fingerprint: String,
    pub model_name: String,
    pub model_version: String,
    pub model_hash: String,
    pub input_spec_digest: String,
    pub artifact_checksum: String,
}

/// Key under which [`DenoiseIdentity`] is persisted in [`DenoiseAi::extras`].
/// `DenoiseAi` flattens `extras` into the recipe JSON (`serde(flatten)`), so
/// this key is additive and survives a JSON roundtrip without a schema change.
pub const DENOISE_PRODUCER_PROVENANCE_KEY: &str = "producer_identity";

/// Reads the persisted producer provenance from [`DenoiseAi::extras`], if the
/// producing (inference) slice recorded one.
///
/// A missing key, a non-object value or a malformed object yields `None` — the
/// caller then has no proof of validity and must classify the artifact as
/// `stale`/`unavailable`, never as silently valid. This function is pure and
/// never fails loudly on a malformed persisted value: the loud signal is the
/// resulting non-`ready` status.
#[must_use]
pub fn denoise_producer_provenance(denoise: &DenoiseAi) -> Option<DenoiseIdentity> {
    serde_json::from_value(denoise.extras.get(DENOISE_PRODUCER_PROVENANCE_KEY)?.clone()).ok()
}

/// Persists the producer provenance into [`DenoiseAi::extras`]
/// ([`DENOISE_PRODUCER_PROVENANCE_KEY`], roundtrip-legal via the flattened
/// `extras`).
pub fn set_denoise_producer_provenance(denoise: &mut DenoiseAi, identity: &DenoiseIdentity) {
    // `DenoiseIdentity` is seven `String`s, so both conversions are infallible;
    // the explicit match keeps the contract honest without an `unwrap`/`expect`.
    if let Ok(value) = serde_json::to_value(identity) {
        denoise
            .extras
            .insert(DENOISE_PRODUCER_PROVENANCE_KEY.to_string(), value);
    }
}

/// Classifies the §6 status of an active `denoise_ai` stage. Pure and
/// deterministic; no I/O, no model.
///
/// Order: identity → `Inactive`; `pending-integration` → `Unavailable`;
/// absent artifact → `Missing`; checksum mismatch → `Corrupt`; any other
/// identity-field mismatch → `Stale`; otherwise `Ready`.
///
/// Two comparisons feed the `Stale` verdict (§6), so the **full** recorded
/// identity is read, not just source/decode:
/// * the recipe's requested model/input-spec must match the live `current`
///   context (an edited recipe request invalidates the artifact), and
/// * the persisted producer `recorded` identity must equal `current` on every
///   field (source hash, decode fingerprint, model name/version/hash,
///   input-spec digest and artifact checksum). An absent/empty provenance
///   therefore mismatches and is loudly `stale`, never silently `ready`.
#[must_use]
pub fn resolve_denoise_status(
    denoise: &DenoiseAi,
    current: &DenoiseIdentity,
    recorded: &DenoiseIdentity,
    artifact_present: bool,
) -> DenoiseStageStatus {
    if denoise.is_identity() {
        return DenoiseStageStatus::Inactive;
    }
    if denoise.model.model_hash == lumina_sidecar::DENOISE_PENDING_MODEL_HASH {
        return DenoiseStageStatus::Unavailable;
    }
    if !artifact_present {
        return DenoiseStageStatus::Missing;
    }
    // The recipe stores the artifact checksum; the caller resolved the actual
    // record checksum into `current`. A mismatch is corruption, not staleness.
    if let Some(reference) = &denoise.artifact {
        if !reference.checksum.is_empty() && current.artifact_checksum != reference.checksum {
            return DenoiseStageStatus::Corrupt;
        }
    }
    // §6: the recipe request (model + input-spec) must agree with the live
    // context, otherwise the requested model is not the one available.
    let requested_model_matches = current.model_name == denoise.model.name
        && current.model_version == denoise.model.version
        && current.model_hash == denoise.model.model_hash;
    let requested_input_spec_matches = current.input_spec_digest == denoise.input_spec_digest;
    // §6: the persisted producer provenance must equal the live context on all
    // identity fields (B2 — every `recorded` field is used, none is dead).
    let recorded_matches = recorded == current;
    if !requested_model_matches || !requested_input_spec_matches || !recorded_matches {
        return DenoiseStageStatus::Stale;
    }
    DenoiseStageStatus::Ready
}

/// Caller-resolved KI-Denoise stage state: the §6 status, an optional
/// matching artifact and the fallback policy.
#[derive(Debug, Clone)]
pub struct DenoiseStageInput<'a> {
    pub status: DenoiseStageStatus,
    pub artifact: Option<&'a DenoiseRgbArtifact>,
    /// Human-readable explanation for a non-`Ready` status (empty otherwise).
    pub reason: String,
    pub policy: DenoisePolicy,
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
        }
    }

    /// Selects the fallback policy ([`DenoisePolicy`]).
    #[must_use]
    pub fn with_policy(mut self, policy: DenoisePolicy) -> Self {
        self.policy = policy;
        self
    }
}

/// Outcome of [`apply_denoise_stage`], for CLI/GUI status reporting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenoiseOutcome {
    /// Whether the artifact was actually blended into the frame.
    pub applied: bool,
    /// The status the stage resolved to.
    pub status: DenoiseStageStatus,
}

/// Applies the KI-Denoise stage to `frame` in place.
///
/// * Recipe absent or identity (`enabled == false`/`strength == 0`) → identity,
///   no model/artifact/error.
/// * Active + `Ready` + matching artifact → deterministic blend (see
///   [`apply_denoise_blend`]).
/// * Active + any non-ready state → [`CoreError::Denoise`] under
///   [`DenoisePolicy::Strict`], or a `warn!` plus identity under
///   [`DenoisePolicy::Warn`] (the manual F-096 stage then runs normally).
pub fn apply_denoise_stage(
    frame: &mut ImageFrame,
    recipe: Option<&DenoiseAi>,
    input: &DenoiseStageInput<'_>,
) -> Result<DenoiseOutcome, CoreError> {
    let Some(denoise) = recipe else {
        return Ok(DenoiseOutcome {
            applied: false,
            status: DenoiseStageStatus::Inactive,
        });
    };
    if denoise.is_identity() {
        return Ok(DenoiseOutcome {
            applied: false,
            status: DenoiseStageStatus::Inactive,
        });
    }
    let status = match input.status {
        // A caller that resolved nothing while the stage is active: exactly the
        // `unavailable` case (no model/weights/path). Never a silent no-op.
        DenoiseStageStatus::Inactive => DenoiseStageStatus::Unavailable,
        other => other,
    };
    if status != DenoiseStageStatus::Ready {
        let reason = if input.reason.is_empty() {
            "no denoise artifact resolved for an active denoise_ai stage".to_string()
        } else {
            input.reason.clone()
        };
        return fail_or_fallback(input, status, reason);
    }
    let Some(artifact) = input.artifact else {
        return fail_or_fallback(
            input,
            DenoiseStageStatus::Corrupt,
            "denoise_ai resolved `ready` but carried no artifact".to_string(),
        );
    };
    apply_denoise_blend(frame, artifact, denoise.strength, denoise.preserve_detail)?;
    Ok(DenoiseOutcome {
        applied: true,
        status: DenoiseStageStatus::Ready,
    })
}

fn fail_or_fallback(
    input: &DenoiseStageInput<'_>,
    status: DenoiseStageStatus,
    reason: String,
) -> Result<DenoiseOutcome, CoreError> {
    match input.policy {
        DenoisePolicy::Strict => Err(CoreError::Denoise {
            status: status.as_str().into(),
            reason,
        }),
        DenoisePolicy::Warn => {
            let status_str = status.as_str();
            warn!(
                "denoise_ai is {status_str}: {reason}; falling back to manual noise reduction \
                 (F-096) / identity — visible, not silent"
            );
            Ok(DenoiseOutcome {
                applied: false,
                status,
            })
        }
    }
}

/// Deterministic `strength`/`preserve_detail` blend of `artifact` into
/// `frame`. The frame dimensions must match the artifact dimensions exactly
/// (a mismatch is a loud `corrupt` error — the artifact does not describe this
/// frame). Alpha is preserved; `strength == 0` returns without touching pixels.
pub fn apply_denoise_blend(
    frame: &mut ImageFrame,
    artifact: &DenoiseRgbArtifact,
    strength: f32,
    preserve_detail: f32,
) -> Result<(), CoreError> {
    artifact.validate()?;
    if artifact.width != frame.width || artifact.height != frame.height {
        return Err(CoreError::Denoise {
            status: DenoiseStageStatus::Corrupt.as_str().into(),
            reason: format!(
                "denoise artifact {}x{} does not match frame {}x{}",
                artifact.width, artifact.height, frame.width, frame.height
            ),
        });
    }
    if strength == 0.0 {
        return Ok(());
    }
    let w = frame.width as usize;
    let h = frame.height as usize;
    // Immutable source snapshot so the per-pixel write can read its neighbours.
    let source = frame.pixels.clone();
    let luminance: Vec<f32> = source
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
        .collect();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let mut sum = 0.0_f32;
            for dy in -1i32..=1 {
                let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                for dx in -1i32..=1 {
                    let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    sum += luminance[yy * w + xx];
                }
            }
            let detail =
                ((luminance[y * w + x] - sum / 9.0).abs() / DENOISE_DETAIL_SCALE).clamp(0.0, 1.0);
            let alpha = strength * (1.0 - preserve_detail * detail);
            if alpha == 0.0 {
                continue;
            }
            let j = (y * w + x) * 3;
            for c in 0..3 {
                let base = source[i + c] as f32;
                let denoised = artifact.pixels[j + c] as f32;
                frame.pixels[i + c] =
                    (base + alpha * (denoised - base)).round().clamp(0.0, 255.0) as u8;
            }
            // `frame.pixels[i + 3]` (alpha) is intentionally untouched.
        }
    }
    Ok(())
}

/// One produced tile of a tiled KI-Denoise run: a row-major RGB8 block placed
/// at `(x, y)` in the full frame.
#[derive(Debug, Clone, Copy)]
pub struct DenoiseTile<'a> {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
}

/// Deterministically assembles a full-frame RGB8 artifact from overlapping
/// tiles so tile seams are invisible by construction.
///
/// Each pixel is the normalized weighted mean of every covering tile,
/// `weight = min(distance to the nearest tile edge) + 1`, which is a
/// deterministic distance-to-edge feather. A constant field reconstructs
/// exactly (any convex combination of equal values is that value), so no seam
/// can appear; a smooth signal crossfades monotonically. Tiles must stay
/// inside the frame and cover it completely — an uncovered pixel is a loud
/// error, never a silent black/zero result. Tile size, overlap and this
/// weighting are part of `input_spec_digest`: changing them invalidates
/// persisted artifacts visibly.
pub fn assemble_denoise_tiles(
    width: u32,
    height: u32,
    tiles: &[DenoiseTile<'_>],
) -> Result<DenoiseRgbArtifact, CoreError> {
    if width == 0 || height == 0 {
        return Err(CoreError::Denoise {
            status: DenoiseStageStatus::Corrupt.as_str().into(),
            reason: "denoise tile assembly needs non-zero frame dimensions".into(),
        });
    }
    let w = width as usize;
    let h = height as usize;
    let mut accum = vec![0.0_f32; w * h * 3];
    let mut weights = vec![0.0_f32; w * h];
    for tile in tiles {
        if tile.width == 0 || tile.height == 0 {
            return Err(CoreError::Denoise {
                status: DenoiseStageStatus::Corrupt.as_str().into(),
                reason: "denoise tile has zero dimensions".into(),
            });
        }
        let expected = tile.width as usize * tile.height as usize * 3;
        if tile.pixels.len() != expected {
            return Err(CoreError::Denoise {
                status: DenoiseStageStatus::Corrupt.as_str().into(),
                reason: format!(
                    "denoise tile {}x{} needs {expected} RGB bytes, got {}",
                    tile.width,
                    tile.height,
                    tile.pixels.len()
                ),
            });
        }
        if tile.x as usize + tile.width as usize > w || tile.y as usize + tile.height as usize > h {
            return Err(CoreError::Denoise {
                status: DenoiseStageStatus::Corrupt.as_str().into(),
                reason: format!(
                    "denoise tile {}x{} at ({}, {}) exceeds frame {w}x{h}",
                    tile.width, tile.height, tile.x, tile.y
                ),
            });
        }
        let tw = tile.width as usize;
        for ty in 0..tile.height as usize {
            for tx in 0..tw {
                let px = tile.x as usize + tx;
                let py = tile.y as usize + ty;
                // Distance to the nearest tile edge (0 on the border).
                let edge = tx
                    .min(ty)
                    .min(tw - 1 - tx)
                    .min(tile.height as usize - 1 - ty);
                let weight = (edge + 1) as f32;
                let src = (ty * tw + tx) * 3;
                let dst = (py * w + px) * 3;
                for c in 0..3 {
                    accum[dst + c] += weight * tile.pixels[src + c] as f32;
                }
                weights[py * w + px] += weight;
            }
        }
    }
    let mut pixels = vec![0u8; w * h * 3];
    for index in 0..w * h {
        let weight = weights[index];
        if weight <= 0.0 {
            return Err(CoreError::Denoise {
                status: DenoiseStageStatus::Missing.as_str().into(),
                reason: format!(
                    "denoise tiles do not cover pixel ({}, {})",
                    index % w,
                    index / w
                ),
            });
        }
        for c in 0..3 {
            pixels[index * 3 + c] = (accum[index * 3 + c] / weight).round().clamp(0.0, 255.0) as u8;
        }
    }
    DenoiseRgbArtifact::new(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{
        DenoiseAi, DenoiseArtifactKind, DenoiseArtifactRef, DenoiseModelIdentity, Extras,
        DENOISE_AI_VERSION, DENOISE_PENDING_MODEL_HASH,
    };

    fn sha256(fill: u8) -> String {
        format!("sha256:{}", format!("{fill:02x}").repeat(32))
    }

    fn artifact(width: u32, height: u32, rgb: [u8; 3]) -> DenoiseRgbArtifact {
        let pixels = rgb
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 3)
            .collect();
        DenoiseRgbArtifact::new(width, height, pixels).unwrap()
    }

    fn denoise(strength: f32, preserve_detail: f32) -> DenoiseAi {
        DenoiseAi {
            version: DENOISE_AI_VERSION,
            enabled: true,
            model: DenoiseModelIdentity {
                name: "fixture-srgb".into(),
                version: "1".into(),
                model_hash: sha256(0x11),
                extras: Extras::new(),
            },
            input_spec_digest: sha256(0x22),
            strength,
            preserve_detail,
            artifact: Some(DenoiseArtifactRef {
                kind: DenoiseArtifactKind::DenoiseRgb,
                relative_path: "IMG.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: artifact(2, 2, [0, 0, 0]).checksum(),
                width: 2,
                height: 2,
                channels: "rgb8".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            }),
            extras: Extras::new(),
        }
    }

    fn gradient_frame(w: u32, h: u32) -> ImageFrame {
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = (i * 7 % 256) as u8;
            px[1] = (i * 13 % 256) as u8;
            px[2] = (i * 29 % 256) as u8;
            px[3] = 255;
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }

    #[test]
    fn absent_and_identity_recipe_stages_are_no_ops_without_artifact() {
        let mut frame = gradient_frame(4, 4);
        let original = frame.clone();
        let input = DenoiseStageInput::inactive();
        // Absent field.
        let outcome = apply_denoise_stage(&mut frame, None, &input).unwrap();
        assert!(!outcome.applied);
        assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
        assert_eq!(frame, original);
        // enabled=false.
        let mut disabled = denoise(0.5, 0.0);
        disabled.enabled = false;
        let outcome = apply_denoise_stage(&mut frame, Some(&disabled), &input).unwrap();
        assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
        // strength=0.
        let zero = denoise(0.0, 0.5);
        let outcome = apply_denoise_stage(&mut frame, Some(&zero), &input).unwrap();
        assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
        assert_eq!(frame, original, "identity must leave pixels untouched");
    }

    #[test]
    fn strength_zero_is_strict_identity_with_a_ready_artifact() {
        let mut frame = gradient_frame(2, 2);
        let original = frame.clone();
        let zero = denoise(0.0, 1.0);
        let ready = artifact(2, 2, [0, 0, 0]);
        let outcome =
            apply_denoise_stage(&mut frame, Some(&zero), &DenoiseStageInput::ready(&ready))
                .unwrap();
        assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
        assert!(!outcome.applied);
        assert_eq!(frame, original, "strength:0 is byte-identical identity");
    }

    #[test]
    fn strength_one_blend_replaces_rgb_and_preserves_alpha_and_determinism() {
        let mut frame = gradient_frame(2, 2);
        let mut expected = frame.clone();
        for px in expected.pixels.as_chunks_mut::<4>().0.iter_mut() {
            px[0] = 5;
            px[1] = 6;
            px[2] = 7;
        }
        let denoised = artifact(2, 2, [5, 6, 7]);
        let recipe = denoise(1.0, 0.0);
        let outcome = apply_denoise_stage(
            &mut frame,
            Some(&recipe),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        assert!(outcome.applied);
        assert_eq!(outcome.status, DenoiseStageStatus::Ready);
        assert_eq!(frame, expected);
        // Determinism: an identical second run is byte-identical.
        let mut again = gradient_frame(2, 2);
        apply_denoise_stage(
            &mut again,
            Some(&recipe),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        assert_eq!(frame, again);
    }

    #[test]
    fn partial_strength_moves_pixels_towards_the_artifact_monotonically() {
        let denoised = artifact(3, 3, [0, 0, 0]);
        let mut previous: u8 = gradient_frame(3, 3).pixels[0];
        for strength in [0.25_f32, 0.5, 1.0] {
            let mut candidate = gradient_frame(3, 3);
            let recipe = denoise(strength, 0.0);
            apply_denoise_stage(
                &mut candidate,
                Some(&recipe),
                &DenoiseStageInput::ready(&denoised),
            )
            .unwrap();
            assert!(
                candidate.pixels[0] <= previous,
                "higher strength must not move away from the denoised value"
            );
            previous = candidate.pixels[0];
        }
    }

    #[test]
    fn preserve_detail_protects_high_detail_pixels() {
        // A frame with a strong luminance edge: detail weight must reduce the
        // denoise amount on the edge relative to a flat frame.
        let mut flat = ImageFrame::new(3, 3, [100, 100, 100, 255].repeat(9)).unwrap();
        let mut edge = flat.clone();
        for (i, px) in edge.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let value = if i % 2 == 0 { 10 } else { 240 };
            px[0] = value;
            px[1] = value;
            px[2] = value;
        }
        let denoised = artifact(3, 3, [0, 0, 0]);
        let recipe = denoise(1.0, 1.0);
        apply_denoise_stage(
            &mut flat,
            Some(&recipe),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        apply_denoise_stage(
            &mut edge,
            Some(&recipe),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        // Flat center pixel is fully denoised to 0; a high-detail pixel keeps
        // more of its original value (is denoised less).
        assert_eq!(flat.pixels[4 * 4], 0);
        assert!(
            edge.pixels[4 * 4] > 0,
            "detail protection must retain source value on an edge"
        );
    }

    fn artifact_as_frame(artifact: &DenoiseRgbArtifact) -> ImageFrame {
        let mut pixels = Vec::with_capacity(artifact.width as usize * artifact.height as usize * 4);
        for rgb in artifact.pixels.as_chunks::<3>().0 {
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        ImageFrame::new(artifact.width, artifact.height, pixels).unwrap()
    }

    #[test]
    fn psnr_gate_approaches_the_artifact_with_strength_and_is_infinite_at_zero() {
        let frame = gradient_frame(8, 6);
        let denoised = artifact(8, 6, [0, 0, 0]);
        let target = artifact_as_frame(&denoised);
        let mut previous = crate::psnr(&frame, &target);
        for strength in [0.25_f32, 0.5, 0.75, 1.0] {
            let mut candidate = frame.clone();
            let recipe = denoise(strength, 0.0);
            apply_denoise_stage(
                &mut candidate,
                Some(&recipe),
                &DenoiseStageInput::ready(&denoised),
            )
            .unwrap();
            let distance = crate::psnr(&candidate, &target);
            assert!(
                distance >= previous - 1e-6,
                "higher strength must not move away from the denoised target \
                 ({distance} < {previous})"
            );
            previous = distance;
        }
        assert!(
            previous.is_infinite() || previous > 40.0,
            "full strength must reach the artifact, got {previous} dB"
        );
        // strength:0 is strict identity → infinite PSNR against the source.
        let mut identity = frame.clone();
        let zero = denoise(0.0, 0.0);
        apply_denoise_stage(
            &mut identity,
            Some(&zero),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        assert_eq!(crate::psnr(&frame, &identity), f64::INFINITY);
    }

    #[test]
    fn dimension_mismatch_is_a_loud_corrupt_error() {
        let mut frame = gradient_frame(2, 2);
        let wrong = artifact(3, 3, [0, 0, 0]);
        let recipe = denoise(0.5, 0.0);
        let error =
            apply_denoise_stage(&mut frame, Some(&recipe), &DenoiseStageInput::ready(&wrong))
                .unwrap_err();
        assert!(matches!(
            error,
            CoreError::Denoise { ref status, .. } if status == "corrupt"
        ));
    }

    #[test]
    fn active_stage_without_resolution_is_loud_under_strict() {
        let mut frame = gradient_frame(2, 2);
        let recipe = denoise(0.5, 0.0);
        let error = apply_denoise_stage(&mut frame, Some(&recipe), &DenoiseStageInput::inactive())
            .unwrap_err();
        assert!(matches!(
            error,
            CoreError::Denoise { ref status, .. } if status == "unavailable"
        ));
    }

    #[test]
    fn warn_policy_surfaces_every_non_ready_status_without_pixel_change() {
        for status in [
            DenoiseStageStatus::Unavailable,
            DenoiseStageStatus::Stale,
            DenoiseStageStatus::Missing,
            DenoiseStageStatus::Corrupt,
        ] {
            let mut frame = gradient_frame(2, 2);
            let original = frame.clone();
            let recipe = denoise(0.5, 0.0);
            let input = DenoiseStageInput::non_ready(status, "test reason")
                .with_policy(DenoisePolicy::Warn);
            let outcome = apply_denoise_stage(&mut frame, Some(&recipe), &input).unwrap();
            assert_eq!(outcome.status, status);
            assert!(!outcome.applied);
            assert_eq!(frame, original, "fallback leaves the manual NR its input");
        }
    }

    #[test]
    fn staleness_is_classified_per_identity_field() {
        let recipe = denoise(0.5, 0.0);
        let current = DenoiseIdentity {
            source_content_hash: "src".into(),
            decode_fingerprint: "decode".into(),
            model_name: "fixture-srgb".into(),
            model_version: "1".into(),
            model_hash: sha256(0x11),
            input_spec_digest: sha256(0x22),
            artifact_checksum: recipe.artifact.as_ref().unwrap().checksum.clone(),
        };
        // The persisted producer provenance matches the live context.
        let recorded = current.clone();
        assert_eq!(
            resolve_denoise_status(&recipe, &current, &recorded, true),
            DenoiseStageStatus::Ready
        );
        // Each identity field, changed independently in the live context, is
        // stale — `model_version` is mutated too (B3).
        let mutations: [fn(&mut DenoiseIdentity); 6] = [
            |c| c.source_content_hash = "other-source".into(),
            |c| c.decode_fingerprint = "other-decode".into(),
            |c| c.model_name = "other".into(),
            |c| c.model_version = "other".into(),
            |c| c.model_hash = sha256(0x33),
            |c| c.input_spec_digest = sha256(0x44),
        ];
        for mutate in mutations {
            let mut candidate = current.clone();
            mutate(&mut candidate);
            assert_eq!(
                resolve_denoise_status(&recipe, &candidate, &recorded, true),
                DenoiseStageStatus::Stale
            );
        }
        // B2: the producer `recorded` identity is read on **every** field, so a
        // drifted persisted provenance is stale even when the live context and
        // the recipe request are unchanged.
        for mutate in mutations {
            let mut drift = recorded.clone();
            mutate(&mut drift);
            assert_eq!(
                resolve_denoise_status(&recipe, &current, &drift, true),
                DenoiseStageStatus::Stale,
                "a drifted producer provenance must be stale, never silently ready"
            );
        }
        // Missing provenance (all-default `recorded`) can never be `ready`.
        assert_eq!(
            resolve_denoise_status(&recipe, &current, &DenoiseIdentity::default(), true),
            DenoiseStageStatus::Stale
        );
        // Checksum mismatch is corrupt, not stale; missing artifact is missing.
        let mut corrupt = current.clone();
        corrupt.artifact_checksum = "00".repeat(32);
        assert_eq!(
            resolve_denoise_status(&recipe, &corrupt, &recorded, true),
            DenoiseStageStatus::Corrupt
        );
        assert_eq!(
            resolve_denoise_status(&recipe, &current, &recorded, false),
            DenoiseStageStatus::Missing
        );
        // pending-integration is unavailable even with a perfect artifact.
        let mut pending = recipe.clone();
        pending.model.model_hash = DENOISE_PENDING_MODEL_HASH.into();
        assert_eq!(
            resolve_denoise_status(&pending, &current, &recorded, true),
            DenoiseStageStatus::Unavailable
        );
    }

    /// B2: the producer identity has a real persistence location — the
    /// flattened `DenoiseAi.extras` — and survives a JSON roundtrip losslessly.
    #[test]
    fn producer_provenance_roundtrips_through_extras() {
        let mut recipe = denoise(0.5, 0.0);
        let identity = DenoiseIdentity {
            source_content_hash: "blake3:src".into(),
            decode_fingerprint: "libraw:1:raw".into(),
            model_name: "fixture-srgb".into(),
            model_version: "1".into(),
            model_hash: sha256(0x11),
            input_spec_digest: sha256(0x22),
            artifact_checksum: recipe.artifact.as_ref().unwrap().checksum.clone(),
        };
        assert!(
            denoise_producer_provenance(&recipe).is_none(),
            "no provenance before it was recorded"
        );
        set_denoise_producer_provenance(&mut recipe, &identity);
        assert_eq!(
            denoise_producer_provenance(&recipe).as_ref(),
            Some(&identity)
        );
        // The flattened `extras` make the new key additive: a JSON roundtrip is
        // loss-free and needs no schema change or migration.
        let json = serde_json::to_string(&recipe).unwrap();
        assert!(json.contains(DENOISE_PRODUCER_PROVENANCE_KEY), "{json}");
        let decoded: DenoiseAi = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, recipe);
        assert_eq!(
            denoise_producer_provenance(&decoded).as_ref(),
            Some(&identity)
        );
        // A malformed persisted value reads as absent (the caller then
        // classifies `stale`), never as a silently valid identity.
        let mut garbage = recipe.clone();
        garbage.extras.insert(
            DENOISE_PRODUCER_PROVENANCE_KEY.into(),
            serde_json::Value::from("not-an-identity"),
        );
        assert!(denoise_producer_provenance(&garbage).is_none());
    }

    #[test]
    fn checksum_is_canonical_and_matches_the_sidecar_layout() {
        let artifact = artifact(2, 2, [10, 20, 30]);
        let mut canonical = Vec::new();
        canonical.extend_from_slice(&1u32.to_le_bytes());
        canonical.extend_from_slice(&2u32.to_le_bytes());
        canonical.extend_from_slice(&2u32.to_le_bytes());
        canonical.extend_from_slice(&artifact.pixels);
        assert_eq!(canonical, artifact.encode_raw());
        assert_eq!(
            artifact.checksum(),
            blake3::hash(&canonical).to_hex().to_string()
        );
        // Stable across calls and independent of any artifact id.
        assert_eq!(artifact.checksum(), artifact.checksum());
    }

    #[test]
    fn tile_assembly_of_constant_tiles_is_seamless_and_deterministic() {
        // Two overlapping horizontal tiles, same constant colour: the output
        // must be exactly that constant with no seam line.
        let tile_a = [40u8, 50, 60].repeat(4 * 2); // 4 wide x 2 high
        let tile_b = [40u8, 50, 60].repeat(4 * 2);
        let tiles = [
            DenoiseTile {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
                pixels: &tile_a,
            },
            DenoiseTile {
                x: 2,
                y: 0,
                width: 4,
                height: 2,
                pixels: &tile_b,
            },
        ];
        let assembled = assemble_denoise_tiles(6, 2, &tiles).unwrap();
        assert_eq!(assembled.width, 6);
        assert_eq!(assembled.height, 2);
        assert!(
            assembled
                .pixels
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| *p == [40, 50, 60]),
            "constant tiles must reconstruct a constant, seam-free frame"
        );
        // Determinism.
        let again = assemble_denoise_tiles(6, 2, &tiles).unwrap();
        assert_eq!(assembled, again);
    }

    #[test]
    fn tile_assembly_crossfades_without_discontinuity() {
        // A dark tile overlapping a bright tile: the transition must be
        // monotone between the two constants (no seam step beyond the range).
        let dark = [0u8; 3].repeat(4);
        let bright = [255u8; 3].repeat(4);
        let tiles = [
            DenoiseTile {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
                pixels: &dark,
            },
            DenoiseTile {
                x: 2,
                y: 0,
                width: 4,
                height: 1,
                pixels: &bright,
            },
        ];
        let assembled = assemble_denoise_tiles(6, 1, &tiles).unwrap();
        let row: Vec<u8> = assembled
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[0])
            .collect();
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "row must be monotone");
    }

    #[test]
    fn uncovered_pixels_and_out_of_bounds_tiles_are_loud() {
        let tile = [0u8; 3].repeat(2);
        let missing_coverage = [DenoiseTile {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            pixels: &tile[..3],
        }];
        let error = assemble_denoise_tiles(2, 2, &missing_coverage).unwrap_err();
        assert!(matches!(
            error,
            CoreError::Denoise { ref status, .. } if status == "missing"
        ));
        let out_of_bounds = [DenoiseTile {
            x: 1,
            y: 0,
            width: 2,
            height: 1,
            pixels: &tile,
        }];
        let error = assemble_denoise_tiles(2, 2, &out_of_bounds).unwrap_err();
        assert!(matches!(
            error,
            CoreError::Denoise { ref status, .. } if status == "corrupt"
        ));
    }
}
