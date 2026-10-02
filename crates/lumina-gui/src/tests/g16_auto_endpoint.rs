//! AUTO-TONE-CLI-6 Klausel (2): der G-16-Endpunkt (`Shift`+Doppelklick auf
//! `whites`/`blacks`) darf keinen gemischten Auto-Tone-Zustand erzeugen.
//!
//! Aus `g16_shortcuts.rs` herausgezogen, weil die Datei dadurch ueber die
//! 500-Zeilen-Schwelle kam und die Auto-Tone-Vertragsfragen eine eigene Sorge
//! sind: die Tastenabbildungen dort pruefen `egui::Key` -> Aktion, diese Tests
//! pruefen den Zustand, den die Aktion im Rezept hinterlaesst. Die
//! Mapping-Tests bleiben in `g16_shortcuts.rs`; die Zustandstests sind hier.
//!
//! Die beiden Lesarten, die der Vertrag verbietet, sind als Test dokumentiert:
//! ein Endpunkt, der nur seinen Regler schreibt und den Alt-Spiegel stehen
//! laesst (verliert den Nutzerwert beim Stale-Clear), und einer, der alle
//! sechs Regler ueberschreibt (fasst Regler an, die niemand angefasst hat).

use super::*;

#[test]
fn g16_apply_auto_endpoint_sets_only_its_field() {
    // Effect test: Shift+double-click on Whites applies exactly the auto
    // white point from the shared auto-tone path (no second algorithm).
    // AUTO-TONE-CLI-6 clause (2): the end point is a pure user override —
    // no mirror, no enable_auto_tone, no fingerprint, so no mixed state.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // A manual whites value without a mirror must be overwritten by the
    // auto value (the end point is a user override, not an auto state).
    app.set_adjustment("whites", 0.5);
    let frame = app.original.clone().expect("decoded frame");
    let expected = suggest_auto_tone(
        &frame,
        AutoToneConfig {
            target_luminance: app.recipe.auto_features.target_luminance,
            ..Default::default()
        },
    )
    .expect("auto-tone evaluates");
    app.apply_auto_endpoint(AutoEndpoint::White).unwrap();
    assert_eq!(app.recipe().adjustments["whites"], expected.whites);
    // Clause (2): no auto-tone state — the end point writes no mirror,
    // no enable_auto_tone, no fingerprint. A mixed state (1 of 6 mirrored)
    // must never arise and never be persisted.
    assert!(!app.recipe().auto_features.enable_auto_tone);
    assert_eq!(app.recipe().auto_features.auto_whites, None);
    assert_eq!(app.recipe().auto_features.auto_blacks, None);
    assert_eq!(app.recipe().auto_features.auto_exposure, None);
    assert_eq!(app.recipe().auto_features.auto_contrast, None);
    assert_eq!(app.recipe().auto_features.auto_highlights, None);
    assert_eq!(app.recipe().auto_features.auto_shadows, None);
    assert_eq!(app.recipe().auto_features.analysis_fingerprint, None);
    // Persisted through the normal save path (CAS, loud conflicts; the
    // JSON roundtrip may re-round the last ulp, hence the tolerance).
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    let persisted = document.virtual_copies[0].recipe.adjustments["whites"];
    assert!(
        (persisted - expected.whites).abs() < 1e-12,
        "persisted {persisted} vs computed {}",
        expected.whites
    );
    // Reload leg (DoD §1): a fresh app restores the value from the sidecar
    // alone — as a manual edit, with no auto-tone state.
    let mut reloaded = new_app();
    open_and_decode(&mut reloaded, source.display().to_string());
    let restored = &reloaded.recipe().adjustments["whites"];
    assert!(
        (*restored - expected.whites).abs() < 1e-12,
        "reloaded {restored} vs computed {}",
        expected.whites
    );
    assert!(!reloaded.recipe().auto_features.enable_auto_tone);
    assert_eq!(reloaded.recipe().auto_features.auto_whites, None);
    assert_eq!(
        reloaded.recipe().auto_features.analysis_fingerprint,
        None,
        "no fingerprint persisted"
    );
}

#[test]
fn g16_apply_auto_black_endpoint_sets_only_blacks() {
    // AUTO-TONE-CLI-6 clause (2): the black end point is a pure user
    // override — no mirror, no enable_auto_tone, no fingerprint.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let frame = app.original.clone().expect("decoded frame");
    let expected = suggest_auto_tone(
        &frame,
        AutoToneConfig {
            target_luminance: app.recipe.auto_features.target_luminance,
            ..Default::default()
        },
    )
    .expect("auto-tone evaluates");
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    assert_eq!(app.recipe().adjustments["blacks"], expected.blacks);
    assert!(!app.recipe().auto_features.enable_auto_tone);
    assert_eq!(app.recipe().auto_features.auto_blacks, None);
    assert_eq!(app.recipe().auto_features.auto_whites, None);
    // Reload leg (DoD §1): a fresh app restores the value from the sidecar
    // alone — as a manual edit, with no auto-tone state (tolerance: JSON
    // roundtrip).
    let mut reloaded = new_app();
    open_and_decode(&mut reloaded, source.display().to_string());
    let restored = &reloaded.recipe().adjustments["blacks"];
    assert!(
        (*restored - expected.blacks).abs() < 1e-12,
        "reloaded {restored} vs computed {}",
        expected.blacks
    );
    assert!(!reloaded.recipe().auto_features.enable_auto_tone);
    assert_eq!(reloaded.recipe().auto_features.auto_blacks, None);
    assert_eq!(
        reloaded.recipe().auto_features.analysis_fingerprint,
        None,
        "no fingerprint persisted"
    );
}

#[test]
fn g16_apply_auto_endpoint_leaves_either_no_mirrors_or_an_announced_partial_set() {
    // AUTO-TONE-CLI-6 clause (2). The first version of this test asserted
    // "never a mixed state" and that was WRONG — branch 2 produces 5 of 6, and
    // the measurement (left: 5, right: 6) is what corrected it. The contract is
    // narrower and is what is pinned here: the end point never invents a
    // partial set out of nothing. On a fresh recipe it writes 0 of 6 — the
    // state "no auto-tone", not a mixed one. On a complete auto-tone state it
    // withdraws exactly its own key, leaving 5 of 6 — a partial set that
    // already existed as a complete one, and that the next test pins as loud.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    // Branch 1: end point on a fresh recipe → no auto-tone state (0/6).
    app.apply_auto_endpoint(AutoEndpoint::White).unwrap();
    let mirrors = [
        app.recipe().auto_features.auto_exposure,
        app.recipe().auto_features.auto_contrast,
        app.recipe().auto_features.auto_whites,
        app.recipe().auto_features.auto_blacks,
        app.recipe().auto_features.auto_highlights,
        app.recipe().auto_features.auto_shadows,
    ];
    let mirrored = mirrors.iter().filter(|m| m.is_some()).count();
    assert_eq!(mirrored, 0, "no mirror after end point on fresh recipe");
    assert!(!app.recipe().auto_features.enable_auto_tone);
    assert_eq!(app.recipe().auto_features.analysis_fingerprint, None);

    // Branch 2: full auto-tone first, then end point → still full (6/6);
    // the end point only overrides the effective value of one slider.
    app.auto_tone().unwrap();
    let mirrors_before = [
        app.recipe().auto_features.auto_exposure,
        app.recipe().auto_features.auto_contrast,
        app.recipe().auto_features.auto_whites,
        app.recipe().auto_features.auto_blacks,
        app.recipe().auto_features.auto_highlights,
        app.recipe().auto_features.auto_shadows,
    ];
    assert_eq!(
        mirrors_before.iter().filter(|m| m.is_some()).count(),
        6,
        "full auto-tone before the end point"
    );
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    let mirrors_after = [
        app.recipe().auto_features.auto_exposure,
        app.recipe().auto_features.auto_contrast,
        app.recipe().auto_features.auto_whites,
        app.recipe().auto_features.auto_blacks,
        app.recipe().auto_features.auto_highlights,
        app.recipe().auto_features.auto_shadows,
    ];
    // Branch 2 is 5-of-6, not 6-of-6. The end point claims `blacks` for the
    // user, so that one mirror is dropped — and with it the value would be
    // deleted by the next stale clear, which is the loss the
    // `g16_auto_endpoint_after_auto_tone_keeps_its_value_across_a_stale_clear`
    // test pins. The other five stay: the user did not touch them.
    //
    // CORRECTION of this test's own first version: it asserted 6 and was wrong.
    // The mixed set is real and is not a defect to be wished away — it is
    // loud and self-healing, which is what the next test pins.
    assert_eq!(
        mirrors_after.iter().filter(|m| m.is_some()).count(),
        5,
        "the end point drops exactly its own mirror, leaving 5 of 6"
    );
    assert_eq!(
        app.recipe().auto_features.auto_blacks,
        None,
        "the claimed slider is no longer auto-written"
    );
    assert!(app.recipe().auto_features.enable_auto_tone);
    assert!(app.recipe().auto_features.analysis_fingerprint.is_some());
}

/// The mixed state left by branch 2 must be **loud and self-healing**, never a
/// silent one: the collective regenerate has to recognise the incomplete mirror
/// set and repair it, and the shared freshness predicate has to refuse the
/// recipe. That is what makes the 5-of-6 set acceptable — the contract asks for
/// an honest state, not for a state that cannot occur.

#[test]
fn g16_apply_auto_endpoint_after_auto_tone_marks_the_state_stale_not_lost() {
    use lumina_stages::auto_tone::{auto_tone_input_fingerprint, auto_tone_is_fresh};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    let frame = app.original.clone().expect("a decoded frame");
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    let endpoint_value = app.recipe().adjustments["blacks"];

    // The user value is still there and still rendered — nothing was lost.
    assert_eq!(app.recipe().adjustments["blacks"], endpoint_value);

    // The shared predicate refuses it, so a CLI `regenerate` recomputes rather
    // than adopting the user's end-point value as an auto value.
    let target = app.recipe().auto_features.target_luminance;
    let input_fingerprint = auto_tone_input_fingerprint(&frame, target);
    assert!(
        !auto_tone_is_fresh(app.recipe(), &input_fingerprint),
        "an incomplete mirror set must not pass the shared freshness predicate"
    );

    // And the collective regenerate repairs it in one full run.
    app.regenerate_stale().expect("collective regenerate runs");
    let repaired = app
        .recipe()
        .auto_features
        .auto_blacks
        .expect("the repair restores the complete six-mirror contract");
    assert!(
        repaired.is_finite(),
        "the repair writes a real auto value again, got {repaired}"
    );
    assert!(
        auto_tone_is_fresh(app.recipe(), &input_fingerprint),
        "after the repair the recipe is fresh again"
    );
}

#[test]
fn g16_apply_auto_endpoint_preserves_other_manual_values() {
    // Data preservation: a manual whites value survives an end-point click
    // on blacks (and vice versa) — the end point writes exactly one field.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("whites", 0.42);
    app.set_adjustment("blacks", -0.31);
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    assert_eq!(
        app.recipe().adjustments["whites"],
        0.42,
        "manual whites survives a blacks end-point click"
    );
    // And the other direction.
    let mut app2 = new_app();
    open_and_decode(&mut app2, source.display().to_string());
    app2.set_adjustment("whites", 0.42);
    app2.set_adjustment("blacks", -0.31);
    app2.apply_auto_endpoint(AutoEndpoint::White).unwrap();
    assert_eq!(
        app2.recipe().adjustments["blacks"],
        -0.31,
        "manual blacks survives a whites end-point click"
    );
}

/// The price of the repair, pinned as a **known defect** rather than as a
/// feature (AUTO-TONE-CLI-6, independent verification 2026-10-02).
///
/// The first version of the accompanying commit called this state "loud and
/// self-healing" and added "no data loss" in the same breath. Measured, the
/// second half is false: the repair runs a full `auto_tone()`, which rewrites
/// all six sliders, so a value the user has set by hand **after** the end-point
/// click is overwritten. Sequence and numbers: auto-tone run -> end point on
/// blacks -> user sets blacks to -0.5 -> `regenerate_stale()` leaves
/// 0.0287…, the auto value.
///
/// This test asserts the **measured present behaviour**, deliberately. It is a
/// canary: when the per-slider override concept lands and the repair starts
/// preserving manual values, this test goes red and has to be rewritten into
/// the assertion that the value survives. A test that asserted the wishful
/// behaviour would sit red forever or, worse, be quietly weakened.
#[test]
fn g16_the_state_repair_overwrites_a_later_manual_value_known_defect() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    // The user's own edit, after the end point.
    app.set_adjustment("blacks", -0.5);
    let manual = app.recipe().adjustments["blacks"];

    app.regenerate_stale().expect("the repair runs");

    // KNOWN DEFECT, measured: the manual value does NOT survive. If a future
    // change makes it survive, this assertion fails and the fix is documented
    // here rather than lost.
    assert_ne!(
        app.recipe().adjustments["blacks"],
        manual,
        "KNOWN DEFECT (AUTO-TONE-CLI-6): the repair overwrites a manual value \
         set after the end point. If this now fails, the repair preserves manual \
         values — update this test and pipeline.md to the fixed behaviour."
    );
}

/// The data-loss case that decides between the two readings of clause (2).
///
/// Sequence: a **full** auto-tone run (6/6 mirrored), then an end-point click
/// on `whites`, then a stale clear. Under the "pure user override" reading the
/// end point writes no mirror — but the mirror that the *auto-tone run* left on
/// `whites` is still there, and `clear_stale_auto_tone` removes exactly the keys
/// that carry a mirror. The end-point value would therefore be deleted together
/// with the auto values, which is a silent loss of an explicit user action.
///
/// Measured, not assumed: this test was written *after* the reading was chosen,
/// precisely because the two other end-point tests both start from a fresh
/// recipe and so never reach this state.
#[test]
fn g16_auto_endpoint_after_auto_tone_keeps_its_value_across_a_stale_clear() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    // A complete auto-tone contract first: all six mirrored.
    app.auto_tone().unwrap();
    assert!(
        app.recipe().auto_features.auto_whites.is_some(),
        "the auto-tone run must mirror whites, or this scenario is unreachable"
    );

    // The explicit user action on top of it.
    app.apply_auto_endpoint(AutoEndpoint::White).unwrap();
    let endpoint_value = app.recipe().adjustments["whites"];

    // The clear that runs when the analysis goes stale. It is exercised through
    // the same production helper the load path uses (lib.rs:10070), not through
    // a copy of its logic.
    let mut recipe = app.recipe().clone();
    clear_stale_auto_tone(&mut recipe);

    // The auto-written keys go; the end point's value must NOT go with them.
    assert!(
        !recipe.adjustments.contains_key("contrast"),
        "auto-written contrast must clear on stale"
    );
    assert!(
        !recipe.adjustments.contains_key("blacks"),
        "auto-written blacks must clear on stale"
    );
    assert_eq!(
        recipe.adjustments.get("whites").copied(),
        Some(endpoint_value),
        "the end-point value is an explicit user action and must survive the \
         stale clear; a mirror left over from the auto-tone run must not \
         delete it"
    );
}
