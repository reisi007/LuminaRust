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
//! **Exact since `JSON-FLOAT-ROUNDTRIP` (owner decision `JSON-ROUNDTRIP`,
//! 2026-10-02, variant (a)).** This workspace now builds `serde_json` 1.0.151
//! **with** `float_roundtrip`. Every `f64` the serializer writes is read back
//! bit-identically, so [`apply_auto_tone_result`]'s reuse branch takes values
//! that are identical to what the algorithm computed — no 1-ULP drift, no
//! re-serialisation of a shifted value, no byte change between two runs over
//! the same sidecar.
//!
//! The mechanism was re-measured in this slice, not carried over from the
//! 2026-09-26 numbers:
//!
//! | measurement | feature **off** | feature **on** |
//! |---|---|---|
//! | 200,000 `f64` splitmix64 draw in `[-10, 10]` | 15,801 lossy (**7.90 %**) | **0** |
//! | maximum deviation in that run | exactly 1 ULP | — |
//! | 43 tracked JSON documents, 5236 float literals | 0 lossy | **0** |
//!
//! Re-measured in this slice — and the rule quoted here is **withdrawn at
//! the source (DoD §9)**, because the verification round refuted both of
//! its halves. The 1-ULP loss hits a `u64` **significand above 2^53** (or
//! an inexact `10^|exp|` for `|exp| > 22`), **not** only values whose
//! shortest representation needs ≥ 17 significant digits. Over 2,000,000
//! samples of the same splitmix64 draw against a real feature-off build of
//! `serde_json` 1.0.151: at ≤ 15 significant digits **nothing** lost (0 of
//! 180,915); **16-digit significands above 2^53 lost as well** (63,278 of
//! 1,302,251 = 4.86 % in the pinned draw, 34.25 % in the verification
//! draw); 17-digit ones lost most (93,658 of 516,833 = 18.12 %). And a
//! **hand-set** literal can sit inside the class: `-9.936999806814127`
//! (16 digits, significand above 2^53) lost a bit. What survives of this
//! paragraph is the measured part: the computed auto-tone mirrors were hit
//! ~2.5× as often as a uniform draw (18.3 % vs 7.9 %), and none of the
//! 5,236 literals in the 43 tracked documents lost a bit in either build.
//! That zero is an observation about those files, not a consequence of
//! "hand-set" or "≤ 16 digits" — the boundary is pinned in
//! `src/tests/auto_tone_float.rs`.
//!
//! **Corrections to the earlier text, withdrawn at the source (DoD §9):**
//! the serializer is no longer `ryu` but `zmij` (`ser.rs:1716`
//! `write_f64` → `zmij::Buffer::format_finite`); and the feature is
//! dependency-free (`float_roundtrip = []`), so **`Cargo.lock` did not change**.
//! Migration plan with all measured bytes: `feature/quality/
//! json-float-roundtrip-plan.md`; the pinned boundary lives in
//! `src/tests/auto_tone_float.rs`, which still carries the two values that
//! lost a bit in the feature-off build.
//!
//! # MCP-PARITY-B: what moved and what stayed
//!
//! The writer itself ([`apply_auto_tone_result`]), the analysis
// fingerprint, the six-key table, the all-or-nothing reuse rule and the
// `regenerate --module auto-tone` freshness predicate now live in
// `crates/lumina-stages/src/auto_tone.rs`, because `lumina-mcp` reaches them
// through `lumina_regenerate op="auto_tone"` and a second copy could not stay
// consistent with the single writer. What stays here is the CLI-only layering
// of `process --auto-tone`: the preset and explicit-slider layers *below* the
// writer, which depend on `ProcessArgs` and therefore cannot move.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lumina_core::{ImageFrame, SourceActionArtifact};
use lumina_sidecar::{EditRecipe, Preset};

use lumina_stages::pipeline::resolve_source_actions;

use crate::{io_error, CliError, ProcessArgs};

/// AUTO-TONE-ANALYSIS-INPUT-8: the CLI's Auto-Tone **measurement domain**.
///
/// `process --auto-tone` must measure the frame after SourceActions and after
/// Crop (without Adjustments) — the image the user is actually editing, per the
/// owner decision `AUTO-DOMAIN` (2026-10-02), not the raw decode. This type
/// resolves the recipe's source actions **once** and hands out both the bundle
/// path and the analysis frame, so the Auto-Tone measurement and the render
/// below it can never see different artifacts.
///
/// `recipe.source_actions` is replaced **wholesale** by the preset layer
/// (`apply_preset_layer`: `*recipe = preset.recipe`), so the resolution is
/// deliberately taken **after** it and before the auto layer. The auto layer and
/// the explicit-slider layer touch only `adjustments`/`auto_features`, never
/// `source_actions`, so one resolution there is valid for the analysis frame
/// *and* for the render context below.
///
/// **MEASURED 2026-10-02, both directions** (a wrong position here is silent in
/// one direction and a hard failure in the other):
///
/// * source action in the **sidecar**, preset without one — resolving before
///   the preset aborts `process --auto-tone --preset` with exit 1 even though
///   the effective recipe carries no retouche at all;
/// * source action in the **preset**, sidecar without one — resolving before the
///   preset returns an empty artifact list, renders the un-retouched decode
///   with exit 0 and still persists the preset's `source_actions`, i.e. a
///   silent drop (F-042-N1: "reported loudly, never silently dropped").
///
/// Pinned by `auto_tone_with_a_preset_without_source_actions_does_not_fail_on_the_sidecars_retouche`
/// and `auto_tone_with_a_preset_carrying_source_actions_reports_the_missing_bundle_loudly`
/// in `tests/source_action_cli_e2e.rs`.
///
/// It lives here, next to the preset/explicit layers of the same `process`
/// ordering, because it is the *input* of the auto layer — the same reason
/// `apply_preset_layer` and `apply_explicit_slider_values` are here (all three
/// were extracted from `process_selected` for the file-size ratchet).
pub(crate) struct AutoToneDomain {
    zdata_path: PathBuf,
    source_actions: Vec<SourceActionArtifact>,
}

impl AutoToneDomain {
    /// Resolves the source actions for `recipe`. Loud on a missing, corrupt or
    /// checksum-mismatched bundle: the render that would have used it must not
    /// silently continue on the un-retouched decode.
    pub(crate) fn resolve(input: &Path, recipe: &EditRecipe) -> Result<Self, CliError> {
        let zdata_path = lumina_sidecar::zdata_path_for(input);
        let source_actions = resolve_source_actions(recipe, &zdata_path)?;
        Ok(Self {
            zdata_path,
            source_actions,
        })
    }

    /// The `.lumina.zdata` bundle path of the loaded source (mask planes and
    /// source actions share it).
    pub(crate) fn zdata_path(&self) -> &Path {
        &self.zdata_path
    }

    /// The resolved source-action artifacts for the render context.
    pub(crate) fn source_actions(&self) -> &[SourceActionArtifact] {
        &self.source_actions
    }

    /// The Auto-Tone analysis input frame for `recipe` — the documented
    /// production path (post SourceActions + spot heals + crop, no Adjustments),
    /// shared verbatim with the GUI and `regenerate`.
    pub(crate) fn analysis_frame(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<ImageFrame, CliError> {
        Ok(lumina_stages::auto_tone::auto_analysis_frame(
            frame,
            recipe,
            &self.source_actions,
        )?)
    }
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
