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

/// R5-WARN-2: `DenoiseStageInput` lives in its own submodule so the
/// over-500-line `denoise.rs` keeps its ratchet after the `quiet` field.
mod stage_input;
pub use stage_input::DenoiseStageInput;

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
            if !input.quiet {
                warn!(
                    "denoise_ai is {status_str}: {reason}; falling back to manual noise reduction \
                     (F-096) / identity — visible, not silent"
                );
            }
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
mod tests;
