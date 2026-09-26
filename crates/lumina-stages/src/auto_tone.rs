//! AUTO-TONE-CLI-6: the single Auto-Tone write path, now shared by the CLI and
//! the MCP server (`lumina regenerate --module auto-tone` /
//! `lumina_regenerate`) — moved here by MCP-PARITY-B.
//!
//! # The contract (unchanged)
//!
//! * **One writer in the CLI and MCP paths.** [`apply_auto_tone_result`] is the
//!   only function in `lumina-cli` and `lumina-stages` that writes the six
//!   AUTO-TONE-2 sliders, the six `auto_features` mirrors and the analysis
//!   `AnalysisFingerprint`. The three
//!   callers — `process --auto-tone` (CLI), `lumina regenerate --module
//!   auto-tone` (CLI) and `op="auto_tone"` (MCP) — go through it. The
//!   thirteen-field write itself lives in the module-private
//!   [`write_auto_tone_state`], and [`mirrored_values`] is the module's only
//!   mirror reader, so a second, slimmer copy cannot be introduced from
//!   another file without first widening this module's private surface — and
//!   the source scan in `lumina-cli/src/tests/auto_tone_writer.rs` fails before
//!   that. **Scope, stated honestly:** that structural scan covers
//!   `lumina-cli` and `lumina-stages` — **not** `lumina-gui`, which writes the
//!   same mirrors and fingerprint from its own pre-existing path that
//!   MCP-PARITY-B deliberately does not touch. The invariant is therefore "one
//!   writer across the CLI and MCP paths", not "one writer in the workspace".
//! * **All six or none.** Persisted values are reused only when the analysis
//!   fingerprint matches **and** all six mirrors are present; otherwise all
//!   six are recomputed. A partial (e.g. 2-of-6) state can neither be created
//!   nor persisted.
//! * **Reuse reads mirrors only.** `recipe.adjustments` is never a source in
//!   the reuse branch, so a user value can never be adopted as an auto value.
//! * **Freshness is presence-based, not value-based** ([`auto_tone_is_fresh`]).
//!
//! # Why it moved
//!
//! The freshness predicate *is* the `regenerate --module auto-tone` decision.
//! If the MCP server had its own copy, a recipe the CLI called fresh could be
//! silently overwritten through the tool (or the reverse), which is precisely
//! the "second logic" the MCP-PARITY slices forbid.

use crate::error::StageError;
use lumina_core::{
    suggest_auto_tone, tone_fingerprint, AutoToneConfig, AutoToneResult, ImageFrame,
};
use lumina_sidecar::{AnalysisFingerprint, AutoFeatures, EditRecipe};
use std::collections::BTreeMap;

/// The six AUTO-TONE-2 slider keys, in their normative order
/// (`feature/architecture/pipeline.md` § Auto-Tone, `AUTO_TONE_ADJUSTMENT_KEYS`).
/// Domains are validated by `lumina-core` (`exposure` `-10..=10`, the other
/// five `-1..=1`); the slider mathematics itself is **not** part of
/// AUTO-TONE-CLI-6.
pub const AUTO_TONE_ADJUSTMENT_KEYS: [&str; 6] = [
    "exposure",
    "contrast",
    "whites",
    "blacks",
    "highlights",
    "shadows",
];

/// Analysis-fingerprint identity of the tone analysis. Shared with the
/// freshness predicate, so a foreign algorithm is never mistaken for ours.
const FINGERPRINT_ALGORITHM: &str = "tone-rgba8-rec709";
const FINGERPRINT_VERSION: &str = "1";

/// The Auto-Tone configuration of one run. Only the target luminance is
/// parameterized; every bound stays at the `lumina-core` default (the slider
/// mathematics is out of scope for AUTO-TONE-CLI-6).
fn auto_tone_config(target_luminance: f64) -> AutoToneConfig {
    AutoToneConfig {
        target_luminance,
        ..Default::default()
    }
}

/// The analysis fingerprint both the reuse decision and the freshness
/// predicate are made against. `--target-luminance` therefore binds the
/// fingerprint: a changed target invalidates the persisted auto values.
pub fn auto_tone_input_fingerprint(frame: &ImageFrame, target_luminance: f64) -> String {
    tone_fingerprint(frame, auto_tone_config(target_luminance))
}

/// Where the six slider values of one [`apply_auto_tone_result`] call came from.
/// Reported so callers and tests can distinguish "reused the complete
/// persisted contract" from "recomputed from pixels".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoToneSource {
    /// The persisted mirrors were reused verbatim (fingerprint matched **and**
    /// all six mirrors were present).
    Reused,
    /// All six values were recomputed with `suggest_auto_tone`.
    Computed,
}

/// The outcome of [`apply_auto_tone_result`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoToneOutcome {
    pub source: AutoToneSource,
}

/// Whether a caller may reuse the persisted values instead of recomputing
/// them. `regenerate --module auto-tone` is an explicit regeneration request
/// and always recomputes; `process --auto-tone` reuses a complete, matching
/// contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedAutoTone {
    /// Reuse the persisted mirrors iff fingerprint matches and all six are
    /// present, otherwise recompute all six.
    ReuseIfComplete,
    /// Never reuse; always recompute all six from the frame.
    AlwaysRecompute,
}

/// The six slider values in [`AUTO_TONE_ADJUSTMENT_KEYS`] order.
type SliderValues = [f64; 6];

/// The six computed values of a fresh `suggest_auto_tone` run, in
/// [`AUTO_TONE_ADJUSTMENT_KEYS`] order.
fn computed_values(result: &AutoToneResult) -> SliderValues {
    [
        result.exposure,
        result.contrast,
        result.whites,
        result.blacks,
        result.highlights,
        result.shadows,
    ]
}

/// The six mirrors as **one set** — `None` as soon as a single mirror is
/// missing. This is the all-or-nothing half of the reuse contract: a partial
/// set is not a value set, and `mirrored_values` is the only reader of the
/// mirrors in this module.
fn mirrored_values(auto: &AutoFeatures) -> Option<SliderValues> {
    Some([
        *auto.auto_exposure.as_ref()?,
        *auto.auto_contrast.as_ref()?,
        *auto.auto_whites.as_ref()?,
        *auto.auto_blacks.as_ref()?,
        *auto.auto_highlights.as_ref()?,
        *auto.auto_shadows.as_ref()?,
    ])
}

/// The complete persisted set, or `None` for a missing mirror **or** a
/// fingerprint that does not belong to the current frame/target. Both halves
/// are required — a matching fingerprint with five mirrors is not a contract.
fn reusable_values(auto: &AutoFeatures, input_fingerprint: &str) -> Option<SliderValues> {
    let stored = auto.analysis_fingerprint.as_ref()?;
    if stored.algorithm != FINGERPRINT_ALGORITHM || stored.input_fingerprint != input_fingerprint {
        return None;
    }
    mirrored_values(auto)
}

/// Writes the full AUTO-TONE-2 result into `recipe`: six sliders plus the six
/// `auto_features` mirrors and the analysis fingerprint, as one group.
///
/// **This is the only place in the workspace that writes those thirteen
/// fields.** It is module-private on purpose: [`apply_auto_tone_result`] is the
/// only public entry point, so a second, slimmer copy cannot be introduced
/// from another file without deleting that surface (and the source-scan test
/// `there_is_exactly_one_auto_tone_write_path` in
/// `lumina-cli/src/tests/auto_tone_writer.rs` fails first).
///
/// `preset_overrides` are the slider values of the `--preset` recipe. The
/// preset is the second layer (auto first, preset second, explicit CLI last):
/// where the preset sets a key explicitly, the **preset** value lands in
/// `recipe.adjustments`; where the preset is silent, the **auto** value does.
/// The mirrors and the fingerprint always document the auto values.
fn write_auto_tone_state(
    recipe: &mut EditRecipe,
    values: &SliderValues,
    input_fingerprint: &str,
    target_luminance: f64,
    preset_overrides: Option<&BTreeMap<String, f64>>,
) {
    for (key, value) in AUTO_TONE_ADJUSTMENT_KEYS.iter().zip(values.iter()) {
        let effective = preset_overrides
            .and_then(|overrides| overrides.get(*key))
            .copied()
            .unwrap_or(*value);
        recipe.adjustments.insert((*key).into(), effective);
    }
    let auto = &mut recipe.auto_features;
    auto.enable_auto_tone = true;
    auto.target_luminance = target_luminance;
    auto.auto_exposure = Some(values[0]);
    auto.auto_contrast = Some(values[1]);
    auto.auto_whites = Some(values[2]);
    auto.auto_blacks = Some(values[3]);
    auto.auto_highlights = Some(values[4]);
    auto.auto_shadows = Some(values[5]);
    auto.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: FINGERPRINT_ALGORITHM.into(),
        version: FINGERPRINT_VERSION.into(),
        input_fingerprint: input_fingerprint.into(),
        extras: BTreeMap::new(),
    });
}

/// Writes the complete AUTO-TONE-2 result into `recipe` — the single write
/// path shared by `process --auto-tone`, `lumina regenerate --module auto-tone`
/// and `lumina_regenerate op="auto_tone"`.
///
/// The value source is decided by [`PersistedAutoTone`]; either way all six
/// sliders, all six mirrors and the fingerprint are written together, so a
/// partial contract is never persisted.
pub fn apply_auto_tone_result(
    recipe: &mut EditRecipe,
    frame: &ImageFrame,
    target_luminance: f64,
    persisted: PersistedAutoTone,
    preset_overrides: Option<&BTreeMap<String, f64>>,
) -> Result<AutoToneOutcome, StageError> {
    let input_fingerprint = auto_tone_input_fingerprint(frame, target_luminance);
    let reusable = match persisted {
        PersistedAutoTone::ReuseIfComplete => {
            reusable_values(&recipe.auto_features, &input_fingerprint)
        }
        PersistedAutoTone::AlwaysRecompute => None,
    };
    let (values, source) = match reusable {
        Some(values) => (values, AutoToneSource::Reused),
        None => (
            computed_values(&suggest_auto_tone(
                frame,
                auto_tone_config(target_luminance),
            )?),
            AutoToneSource::Computed,
        ),
    };
    write_auto_tone_state(
        recipe,
        &values,
        &input_fingerprint,
        target_luminance,
        preset_overrides,
    );
    Ok(AutoToneOutcome { source })
}

/// The freshness predicate of `lumina regenerate` for the `auto-tone` module:
/// `enable_auto_tone`, all six mirrors, all six sliders **present** and a
/// fingerprint that matches the current frame/target.
///
/// Presence, not value equality, is the contract: a slider the user overrode
/// (key present, value effective) keeps the recipe fresh, because
/// regenerating would silently destroy that override. A deliberately changed
/// state is therefore a *removed* slider or mirror, or a non-matching
/// fingerprint. See `feature/architecture/pipeline.md` § Auto-Tone.
pub fn auto_tone_is_fresh(recipe: &EditRecipe, input_fingerprint: &str) -> bool {
    let auto = &recipe.auto_features;
    auto.enable_auto_tone
        && mirrored_values(auto).is_some()
        && AUTO_TONE_ADJUSTMENT_KEYS
            .iter()
            .all(|key| recipe.adjustments.contains_key(*key))
        && auto.analysis_fingerprint.as_ref().is_some_and(|stored| {
            stored.algorithm == FINGERPRINT_ALGORITHM
                && stored.input_fingerprint == input_fingerprint
        })
}
