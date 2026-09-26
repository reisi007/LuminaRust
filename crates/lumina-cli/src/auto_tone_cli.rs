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
use std::path::Path;

use lumina_sidecar::{EditRecipe, Preset};

use crate::{io_error, CliError, ProcessArgs};

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
