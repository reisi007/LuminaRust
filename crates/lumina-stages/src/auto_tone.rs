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
//!   that. **Scope, corrected 2026-09-30 (AUTO-TONE-CLI-6 clause 1):** that
//!   structural scan covers `lumina-cli` and `lumina-stages`. It does **not**
//!   cover `lumina-gui`, but the GUI is no longer a second writer either —
//!   `LuminaApp::auto_tone` calls [`apply_auto_tone_result`] like every other
//!   front end. The wording this replaces said the GUI "writes the same mirrors
//!   and fingerprint from its own pre-existing path"; that was true when
//!   MCP-PARITY-B wrote it and stopped being true with clause (1). The GUI
//!   side is pinned by its own check instead
//!   (`lumina-gui/src/tests/auto_tone_contract.rs`): the fingerprint-algorithm
//!   literal must occur exactly once across the four contract-writing crates.
//!   The invariant is "one writer, workspace-wide".
//! * **All six or none.** Persisted values are reused only when the analysis
//!   fingerprint matches **and** all six mirrors are present; otherwise all
//!   six are recomputed. A partial (e.g. 2-of-6) state can neither be created
//!   nor persisted.
//! * **Reuse reads mirrors only.** `recipe.adjustments` is never a source in
//!   the reuse branch, so a user value can never be adopted as an auto value.
//! * **A mirror that does not document the slider marks a user claim, not a
//!   partial contract** (AUTO-TONE-ENDPOINT-MIXED-7).
//!   [`auto_tone_overrides`] is the single derivation that turns a missing
//!   mirror (a key never auto-written) or a mirror that differs from the
//!   effective value (a slider edited by hand after the auto write) into the
//!   user values a repair run must keep. The writer still never *creates* a
//!   partial set: every caller reaches this module through
//!   `apply_auto_tone_result`, which always writes all six sliders and all six
//!   mirrors.
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
    apply_spot_heals_from_recipe, prepare_source_base, suggest_auto_tone, tone_fingerprint,
    AutoToneConfig, AutoToneResult, ImageFrame, SourceActionArtifact, StageWork,
};
use lumina_sidecar::{AnalysisFingerprint, AutoFeatures, EditRecipe};
use std::collections::BTreeMap;

/// AUTO-TONE-ANALYSIS-INPUT-8: the **analysis input frame** of Auto-Tone.
///
/// The owner's decision (`AUTO-DOMAIN`, 2026-10-02) fixes the domain Auto-Tone
/// measures: the frame **after SourceActions and after Crop**, *without*
/// Adjustments — the image the user is actually editing. This is the single
/// production seam that builds it, so the GUI (`LuminaApp`), the CLI
/// (`process --auto-tone`, `regenerate --module auto-tone`) and the MCP server
/// all measure the **same** pixels.
///
/// # Why this exact domain
///
/// The owner's wording is „nach wegretuschieren UND nach Zuschnitt": the retouch
/// (artifact `SourceActions` plus the recipe's spot heals) and the framing
/// (crop/rotation/mirror) are part of the edited image, so both must influence
/// the Auto values. Adjustments are deliberately **excluded** — Auto-Tone writes
/// the very exposure/contrast/… sliders that Adjustments applies, so measuring a
/// frame that already carries them would feed the previous Auto result back into
/// the next (a feedback loop, not a measurement).
///
/// # The chosen production path
///
/// ```text
/// source
///   -> prepare_source_base          (artifact SourceActions, strict)
///   -> apply_spot_heals_from_recipe (recipe spot_removals = "wegretuschieren")
///   -> apply_crop_stage             (recipe.geometry: crop/rotation/mirror)
/// ```
///
/// It reuses the **runtime render stages verbatim** — no second crop or heal
/// implementation — and stops exactly where Adjustments would begin. The result
/// is what `apply_auto_tone_result` below is measured on, and its fingerprint
/// (`auto_tone_input_fingerprint`) therefore identifies this domain by content.
///
/// # Named boundary
///
/// The lens (F-098) and perspective (F-099) stages stay **out** of the analysis
/// domain (the owner named retouch and crop; those two are resampling stages
/// with their own artifact inputs). The crop stage consequently runs with
/// `use_content_default = false`: the maximum-content default rect is defined on
/// the lens/perspective-corrected canvas and would be wrong on this un-corrected
/// frame. An **explicit** `recipe.geometry.crop` is applied, rotation and
/// mirroring too; only the implicit content default is out of scope. Generative
/// expand and auto-fill are out for the same reason (model-produced canvases).
///
/// Loud, never a silent fallback: an invalid source action or a spot-heal mode
/// the portable core cannot apply is a [`StageError`], exactly as on the render
/// path.
pub fn auto_analysis_frame(
    source: &ImageFrame,
    recipe: &EditRecipe,
    source_actions: &[SourceActionArtifact],
) -> Result<ImageFrame, StageError> {
    let mut work = StageWork::default();
    let mut frame = prepare_source_base(source, source_actions, &mut work)?;
    apply_spot_heals_from_recipe(&mut frame, recipe)?;
    frame.apply_crop_stage(recipe.geometry.as_ref(), false)?;
    Ok(frame)
}

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
///
/// AUTO-TONE-CLI-6: `pub` because the GUI writes an endpoint fingerprint of the
/// same identity, and a second hand-copied literal is exactly the divergence
/// these two constants exist to prevent.
pub const FINGERPRINT_ALGORITHM: &str = "tone-rgba8-rec709";
pub const FINGERPRINT_VERSION: &str = "1";

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

/// The per-slider **user overrides** a recompute must preserve, derived from the
/// recipe's own persisted state — no new field, no migration
/// (AUTO-TONE-ENDPOINT-MIXED-7).
///
/// A tone key in [`AUTO_TONE_ADJUSTMENT_KEYS`] is an override exactly when its
/// `auto_features` mirror does **not** document its effective value in
/// `recipe.adjustments`:
///
/// * the mirror is **missing** (`None`) while the slider is present — the key
///   was never auto-written (an end point on a fresh recipe, or a value that a
///   stale-clear kept because it was a user override);
/// * the mirror is present but records a **different** value — the slider was
///   edited by hand after the auto write.
///
/// Where the mirror and the slider agree, the slider *is* the auto value and no
/// override is produced. The result feeds the `preset_overrides` slot of
/// [`apply_auto_tone_result`], which writes the effective value where an
/// override exists and the **auto value** into the mirror either way — clause
/// (4) of AUTO-TONE-CLI-6.
///
/// This is the single source of the override set for the two GUI callers: the
/// G-16 end point (`LuminaApp::apply_auto_endpoint`) and the repair run
/// (`regenerate_stale` → `LuminaApp::auto_tone`).
pub fn auto_tone_overrides(recipe: &EditRecipe) -> BTreeMap<String, f64> {
    let auto = &recipe.auto_features;
    let mirrors = [
        ("exposure", auto.auto_exposure),
        ("contrast", auto.auto_contrast),
        ("whites", auto.auto_whites),
        ("blacks", auto.auto_blacks),
        ("highlights", auto.auto_highlights),
        ("shadows", auto.auto_shadows),
    ];
    let mut overrides = BTreeMap::new();
    for (key, mirror) in mirrors {
        let Some(value) = recipe.adjustments.get(key).copied() else {
            continue;
        };
        let is_override = match mirror {
            None => true,
            Some(auto_value) => value != auto_value,
        };
        if is_override {
            overrides.insert(key.to_string(), value);
        }
    }
    overrides
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
