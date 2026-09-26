//! AUTO-TONE-CLI-6: the single Auto-Tone write path of the CLI.
//!
//! Extracted from `process_selected` (file-size ratchet: `main.rs` must not
//! grow) and normative in
//! `feature/architecture/pipeline.md` § Auto-Tone, subsection
//! „Ein Schreibpfad, Vorrangordnung und Wiederverwendung“.
//!
//! # The contract
//!
//! * **One writer.** [`apply_auto_tone_result`] is the only function in the
//!   CLI crate that writes the six AUTO-TONE-2 sliders, the six
//!   `auto_features` mirrors and the analysis `AnalysisFingerprint`. Both
//!   callers — `process_selected` (`process --auto-tone`) and
//!   `lumina regenerate --module auto-tone` — go through it. The
//!   thirteen-field write itself lives in the module-private
//!   [`write_auto_tone_state`], and [`mirrored_values`] is the module's only
//!   mirror reader, so a second, slimmer copy cannot be introduced from
//!   another file without first widening this module's private surface — and
//!   the source scan in `src/tests/auto_tone_writer.rs` fails before that.
//! * **All six or none.** Persisted values are reused only when the analysis
//!   fingerprint matches **and** all six mirrors are present; otherwise all
//!   six are recomputed. A partial (e.g. 2-of-6) state can neither be created
//!   nor persisted.
//! * **Reuse reads mirrors only.** `recipe.adjustments` is never a source in
//!   the reuse branch, so a user value can never be adopted as an auto value.
//! * **`adjustments` is the effective value, the mirrors are the auto value.**
//!   Preset and explicit CLI values override `adjustments` only; the mirrors
//!   keep documenting what the algorithm computed.
//! * **Freshness is presence-based, not value-based.** [`auto_tone_is_fresh`]
//!   asks for the six sliders *present*, the six mirrors *present* and a
//!   matching fingerprint — never for value equality. A value-based predicate
//!   would mark every user override stale (the mirrors keep the auto value,
//!   `adjustments` the effective one), and the next `regenerate` would silently
//!   destroy that override. "Deliberately changed" therefore means: a *removed*
//!   slider, a *removed* mirror, or a non-matching fingerprint.
//!
//! # The `f64` JSON round-trip boundary (AUTO-TONE-CLI-6, clause 9)
//!
//! This workspace builds `serde_json` 1.0.151 **without** `float_roundtrip`.
//! The feature governs only the **parser**; the serializer (ryu) always writes
//! the shortest exactly-re-readable decimal, so the text in a sidecar is
//! *always* correct — the **parser** loses the last bit. The loss therefore
//! happens on **load**, and the file on disk looks innocent. The two halves are
//! pinned separately in `src/tests/auto_tone_float.rs` so the diagnosis points
//! at the parser and not at the serializer.
//!
//! Measured over the six sliders' domain `[-10, 10]` (2026-09-26):
//!
//! | measurement | result |
//! |---|---|
//! | 200,000 `f64` on a linear grid (step 1e-4), `[-10, 10]` | **14,900 (7.45 %)** do not survive the round trip |
//! | 200,000 `f64` from a deterministic 64-bit LCG, `[-10, 10]` | 15,804 (7.90 %) — the rate is sampling-dependent |
//! | **240 real auto-tone values** from 40 deterministic fixtures | **44 (18.33 %)** are lossy |
//! | maximum deviation in every measurement | **exactly 1 ULP** |
//!
//! Real auto-tone values are ~2.5x more often affected than a uniform draw
//! because they land almost on a binary fraction plus a tiny remainder. Two
//! verified single cases: `-0.20253906249999998` reads back as
//! `-0.2025390625`, `0.013476562499999997` as `0.013476562499999995`.
//!
//! **Consequence for [`apply_auto_tone_result`]:** the reuse branch takes the
//! persisted values *from the loaded sidecar*, so for those values the reused
//! number is not bit-identical to what the algorithm computed. This is the
//! documented, measured state — the completeness contract (six sliders, six
//! mirrors, fingerprint, presence-based freshness) is unaffected; only the last
//! decimal bit can differ.
//!
//! **It converges and does not accumulate** (measured over 40 fixtures × 3
//! runs on the same sidecar): run 1 → run 2 drifted **44 of 240** slider values
//! by at most 1 ULP; run 2 → run 3 drifted **0 of 240**. Run 1 writes the exact
//! text, run 2 reads it (1 ULP off) and writes the shortest representation of
//! *that* value, and from run 2 on the persisted text is a fixed point of the
//! parser. Freshness already holds from run 1, so `regenerate` never overwrites
//! it.
//!
//! **Rendered bytes:** over the same 40 fixtures the rendered PNG was
//! byte-identical between runs 1 and 2 and between runs 2 and 3 in **40 of 40**
//! cases. That is an **observation over 40 fixtures, not a proof** — a 1-ULP
//! exposure difference can in principle move a `u8` sample. The Auto-Tone byte
//! goldens in `src/tests/` therefore use a fixture whose six values survive the
//! round trip *exactly*, so the golden stays an independent quantity; the real
//! case is pinned separately in `src/tests/auto_tone_float.rs`.
//!
//! **Measured side effect at the byte level:** because run 2 writes the shorter
//! text of the 1-ULP-shifted value, the sidecar **bytes** change between runs 1
//! and 2 for the 27 of 40 family members that drift (44 of 240 slider
//! literals). Any digest formed over the serialized bytes (blake3, e.g.
//! `LocalAdjustments::digest`, `mask_layers_digest`) is therefore affected as
//! soon as a loaded document is re-serialized. A digest change on a real mask
//! layer was **not** measured here — the Auto-Tone fixtures carry no mask and
//! this slice does not touch `lumina-sidecar`; the point is recorded as a
//! measured byte change because it is what weighs the `JSON-FLOAT-ROUNDTRIP`
//! decision, whose acceptance explicitly demands proof that no recipe digest
//! changes unintentionally.
//!
//! **Why this is not fixed here:** enabling `float_roundtrip` would change
//! **every** existing persisted sidecar byte — all recipe digests,
//! `mask_layers_digest`, render identities and goldens would have to be
//! re-verified, and every already-written file would yield different `f64`
//! values on the next load. That is a separate, deliberately open decision
//! (task `JSON-FLOAT-ROUNDTRIP`, Release 1.5: enable vs. deliberately keep,
//! with a migration/golden plan). The contract this slice freezes is the
//! measured boundary itself.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use lumina_core::{
    suggest_auto_tone, tone_fingerprint, AutoToneConfig, AutoToneResult, ImageFrame,
};
use lumina_sidecar::{AnalysisFingerprint, AutoFeatures, EditRecipe, Preset};

use crate::{io_error, CliError, ProcessArgs};

/// The six AUTO-TONE-2 sliders (adjustment keys) written by Auto-Tone, in
/// their contract order. Shared by the writer ([`write_auto_tone_state`]),
/// the preset override lookup and the freshness predicate
/// ([`auto_tone_is_fresh`]), so a value written by `process --auto-tone` and
/// one written by `regenerate --module auto-tone` are recognized as the same
/// complete contract. Domains are validated by `lumina-core`
/// (`exposure` `-10..=10`, the other five `-1..=1`); the slider mathematics
/// itself is **not** part of AUTO-TONE-CLI-6.
pub(crate) const AUTO_TONE_ADJUSTMENT_KEYS: [&str; 6] = [
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
pub(crate) fn auto_tone_input_fingerprint(frame: &ImageFrame, target_luminance: f64) -> String {
    tone_fingerprint(frame, auto_tone_config(target_luminance))
}

/// Where the six slider values of one `apply_auto_tone_result` call came from.
/// Reported so callers and tests can distinguish "reused the complete
/// persisted contract" from "recomputed from pixels".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoToneSource {
    /// The persisted mirrors were reused verbatim (fingerprint matched **and**
    /// all six mirrors were present).
    Reused,
    /// All six values were recomputed with `suggest_auto_tone`.
    Computed,
}

/// The outcome of [`apply_auto_tone_result`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoToneOutcome {
    pub(crate) source: AutoToneSource,
}

/// Whether a caller may reuse the persisted values instead of recomputing
/// them. `regenerate --module auto-tone` is an explicit regeneration request
/// and always recomputes; `process --auto-tone` reuses a complete, matching
/// contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistedAutoTone {
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
/// **This is the only place in the CLI crate that writes those thirteen
/// fields.** It is module-private on purpose: `apply_auto_tone_result` is the
/// only public entry point, so a second, slimmer copy cannot be introduced
/// from another file without deleting that surface (and the source-scan test
/// `there_is_exactly_one_auto_tone_write_path` in
/// `src/tests/auto_tone_writer.rs` fails first).
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
/// path shared by `process --auto-tone` and `lumina regenerate --module
/// auto-tone`.
///
/// The value source is decided by [`PersistedAutoTone`]; either way all six
/// sliders, all six mirrors and the fingerprint are written together, so a
/// partial contract is never persisted.
pub(crate) fn apply_auto_tone_result(
    recipe: &mut EditRecipe,
    frame: &ImageFrame,
    target_luminance: f64,
    persisted: PersistedAutoTone,
    preset_overrides: Option<&BTreeMap<String, f64>>,
) -> Result<AutoToneOutcome, CliError> {
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
pub(crate) fn auto_tone_is_fresh(recipe: &EditRecipe, input_fingerprint: &str) -> bool {
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

/// The second layer of the `process` slider ordering: the `--preset` recipe
/// replaces the whole recipe, and its own slider values are returned so the
/// Auto-Tone writer can let the preset win where the preset is not silent.
///
/// The pre-preset `auto_features` (mirrors, fingerprint, matching state) are
/// restored verbatim when `auto_requested` — those belong to the document, not
/// to the preset, exactly as before AUTO-TONE-CLI-6.
pub(crate) fn apply_preset_layer(
    recipe: &mut EditRecipe,
    path: &Path,
    auto_requested: bool,
) -> Result<BTreeMap<String, f64>, CliError> {
    let json = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let preset: Preset =
        serde_json::from_str(&json).map_err(|error| CliError::Preset(error.to_string()))?;
    let auto_features = recipe.auto_features.clone();
    let overrides = if auto_requested {
        preset.recipe.adjustments.clone()
    } else {
        BTreeMap::new()
    };
    *recipe = preset.recipe;
    if auto_requested {
        recipe.auto_features = auto_features;
    }
    Ok(overrides)
}

/// The last layer of the `process` slider ordering: the explicit CLI values
/// win over both the auto layer and the preset, for **all six** sliders. The
/// `auto_features` mirrors keep the auto value — `recipe.adjustments` carries
/// the effective value from here on.
pub(crate) fn apply_explicit_slider_values(recipe: &mut EditRecipe, args: &ProcessArgs) {
    for (key, value) in [
        ("exposure", args.exposure),
        ("contrast", args.contrast),
        ("whites", args.whites),
        ("blacks", args.blacks),
        ("highlights", args.highlights),
        ("shadows", args.shadows),
    ] {
        if let Some(value) = value {
            recipe.adjustments.insert(key.into(), value);
        }
    }
}
