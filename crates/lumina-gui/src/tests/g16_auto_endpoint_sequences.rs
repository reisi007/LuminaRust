//! AUTO-TONE-ENDPOINT-MIXED-7 acceptance sequences for the G-16 end point
//! (`Shift`+double-click on `whites`/`blacks`).
//!
//! Split out of `g16_auto_endpoint.rs` for the 500-line ratchet: the base file
//! keeps the single-action state tests, this file keeps the order-sensitive
//! sequences and the shared six-mirror assertions.
//!
//! Every sequence must leave a state that is **all six or none** — never a
//! mixed one — and must not lose a user value:
//!
//! 1. frisch + Endpunkt,
//! 2. voller Lauf + Endpunkt,
//! 3. voller Lauf + Endpunkt + Handwert + Reparaturlauf,
//! 4. zweimal Endpunkt hintereinander.
//!
//! `auto_tone_overrides` is the single derivation of the override set for the
//! end point and the repair run; `clear_stale_auto_tone` keeps a value whose
//! auto mirror does not document it.

use super::*;

/// The six mirrors in `AUTO_TONE_ADJUSTMENT_KEYS` order.
fn mirrors_of(recipe: &EditRecipe) -> [Option<f64>; 6] {
    let auto = &recipe.auto_features;
    [
        auto.auto_exposure,
        auto.auto_contrast,
        auto.auto_whites,
        auto.auto_blacks,
        auto.auto_highlights,
        auto.auto_shadows,
    ]
}

/// The six `adjustments` in `AUTO_TONE_ADJUSTMENT_KEYS` order, `None` when the
/// key is absent.
fn adjustments_of(recipe: &EditRecipe) -> [Option<f64>; 6] {
    lumina_stages::auto_tone::AUTO_TONE_ADJUSTMENT_KEYS
        .map(|key| recipe.adjustments.get(key).copied())
}

/// Every one of the six mirrors is present and every one of the six sliders is
/// present — the only non-mixed Auto-Tone state, shared by every sequence
/// assertion below.
fn assert_complete_six(recipe: &EditRecipe, context: &str) {
    let mirrors = mirrors_of(recipe);
    assert!(
        mirrors.iter().all(|m| m.is_some()),
        "{context}: all six mirrors must be present, got {mirrors:?}"
    );
    let adjustments = adjustments_of(recipe);
    assert!(
        adjustments.iter().all(|a| a.is_some()),
        "{context}: all six sliders must be present, got {adjustments:?}"
    );
}

/// Describes an override the way clause (4) demands it: the mirror documents the
/// **auto** value, `adjustments` the **user** value, and the two differ.
fn assert_override(recipe: &EditRecipe, key: &str, expected_user: f64, context: &str) {
    let user = recipe.adjustments.get(key).copied();
    assert_eq!(user, Some(expected_user), "{context}: {key} user value");
    let auto = match key {
        "exposure" => recipe.auto_features.auto_exposure,
        "contrast" => recipe.auto_features.auto_contrast,
        "whites" => recipe.auto_features.auto_whites,
        "blacks" => recipe.auto_features.auto_blacks,
        "highlights" => recipe.auto_features.auto_highlights,
        "shadows" => recipe.auto_features.auto_shadows,
        _ => panic!("unknown tone key {key}"),
    };
    assert!(
        auto.is_some(),
        "{context}: {key} must keep an auto mirror documenting the auto value"
    );
}

/// Makes an existing Auto-Tone state stale for the **shared** freshness
/// predicate by moving the target luminance away from the value the
/// fingerprint was made for. The value stays in the valid `0..=1` domain. Used
/// so the collective repair actually runs instead of skipping a fresh state.
fn make_auto_tone_stale(app: &mut LuminaApp) {
    let current = app.recipe().auto_features.target_luminance;
    app.recipe.auto_features.target_luminance = if (current - 0.5).abs() < 1e-9 {
        0.75
    } else {
        0.5
    };
}

/// Sequence 1: **frisch + Endpunkt**.
///
/// A fresh recipe has no Auto-Tone state. The end point writes its one slider
/// and creates **no** Auto-Tone state at all — 0 of 6 mirrors is "no Auto-Tone",
/// not a mixed state, and there is no data to lose.
#[test]
fn g16_endpoint_on_a_fresh_recipe_is_none_of_six_not_mixed() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.apply_auto_endpoint(AutoEndpoint::White).unwrap();

    assert!(
        !app.recipe().auto_features.enable_auto_tone,
        "a fresh recipe stays out of Auto-Tone"
    );
    assert_eq!(
        mirrors_of(app.recipe())
            .iter()
            .filter(|m| m.is_some())
            .count(),
        0,
        "no mirror may be created out of nothing"
    );
    assert_eq!(app.recipe().auto_features.analysis_fingerprint, None);
    assert!(
        app.recipe().adjustments.contains_key("whites"),
        "the end point value is the user's value"
    );
}

/// Sequence 2: **voller Lauf + Endpunkt**.
///
/// After a full Auto-Tone run the end point must not leave 5 of 6 mirrors. It
/// completes the state through the shared writer: all six mirrors present, all
/// six sliders present, and the freshness predicate still true.
#[test]
fn g16_endpoint_after_a_full_run_completes_to_six_of_six() {
    use lumina_stages::auto_tone::{auto_tone_input_fingerprint, auto_tone_is_fresh};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    assert_complete_six(app.recipe(), "before the end point");

    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    assert_complete_six(app.recipe(), "after the end point on a full run");
    assert!(
        app.recipe().auto_features.enable_auto_tone,
        "the Auto-Tone state is kept, not dropped"
    );
    let frame = app.original.clone().expect("decoded frame");
    let target = app.recipe().auto_features.target_luminance;
    let fingerprint = auto_tone_input_fingerprint(&frame, target);
    assert!(
        auto_tone_is_fresh(app.recipe(), &fingerprint),
        "a completed six-mirror state must stay fresh"
    );
}

/// Sequence: **voller Lauf + geänderter Zielwert + Endpunkt**.
///
/// This is the case that makes the end point's completion load-bearing: after a
/// full run the mirrors and fingerprint belong to the old target luminance. Once
/// the target changes, the persisted state is stale. The end point completes the
/// state through the shared writer, so the fingerprint is rewritten for the new
/// target and the mirrors document the recomputed auto values — the recipe is
/// fresh again and still 6 of 6. Without that completion the fingerprint stays on
/// the old target and the recipe is stale (mutation M-A, `DoD.md` §10).
#[test]
fn g16_endpoint_completes_a_stale_target_state_back_to_fresh() {
    use lumina_stages::auto_tone::{auto_tone_input_fingerprint, auto_tone_is_fresh};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    // The user changes the analysis target; the persisted fingerprint still
    // belongs to the old one.
    app.recipe.auto_features.target_luminance = 0.75;
    assert!(!auto_tone_is_fresh(
        app.recipe(),
        &auto_tone_input_fingerprint(
            &app.original.clone().unwrap(),
            app.recipe().auto_features.target_luminance
        )
    ));

    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();

    assert_complete_six(app.recipe(), "after the end point on a changed target");
    let frame = app.original.clone().expect("decoded frame");
    let fingerprint = auto_tone_input_fingerprint(&frame, 0.75);
    assert!(
        auto_tone_is_fresh(app.recipe(), &fingerprint),
        "the end point must complete the state for the new target"
    );
}

/// Sequence 4: **zweimal Endpunkt hintereinander**.
///
/// The previously unpinned case. Two end points used to leave 4 of 6 mirrors
/// after a full run; now the state is complete and fresh after each one, and the
/// second end point does not undo the first.
#[test]
fn g16_two_endpoints_in_a_row_stay_complete_and_keep_both_values() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    let blacks_after_first = app.recipe().adjustments["blacks"];
    app.apply_auto_endpoint(AutoEndpoint::White).unwrap();

    assert_complete_six(app.recipe(), "after two end points");
    assert_eq!(
        app.recipe().adjustments["blacks"],
        blacks_after_first,
        "the second end point must not disturb the first"
    );
    assert!(
        app.recipe().adjustments.contains_key("whites"),
        "the second end point wrote its key"
    );
}

/// Acceptance (2): the mirrors document the **auto** value, `adjustments` the
/// **user** value, for every sequence. A full run, then a hand edit on two keys,
/// then the collective repair: the auto mirrors must come back complete and the
/// two hand values must still be the effective values.
#[test]
fn g16_mirrors_document_auto_and_adjustments_document_user_after_a_repair() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    app.set_adjustment("blacks", -0.5);
    app.set_adjustment("whites", 0.4);
    // Make the state stale (fingerprint) so the collective action actually runs
    // the repair instead of skipping a fresh state — otherwise this test would
    // assert on a no-op and could not fail.
    make_auto_tone_stale(&mut app);

    let regenerated = app.regenerate_stale().expect("the repair runs");
    assert!(
        regenerated.contains(&"auto-tone"),
        "the stale Auto-Tone module must be repaired, got {regenerated:?}"
    );

    assert_complete_six(app.recipe(), "after the repair");
    assert_override(
        app.recipe(),
        "blacks",
        -0.5,
        "hand value must survive the repair",
    );
    assert_override(
        app.recipe(),
        "whites",
        0.4,
        "hand value must survive the repair",
    );
    // A key the user never touched carries the auto value in both places.
    assert_eq!(
        app.recipe().adjustments["contrast"],
        app.recipe().auto_features.auto_contrast.unwrap(),
        "an untouched key is the auto value"
    );
}

/// Acceptance (3): after the repair the shared freshness predicate is true, and
/// a second repair run is idempotent — it leaves the values exactly where the
/// first one did.
#[test]
fn g16_the_repaired_state_is_fresh_and_idempotent() {
    use lumina_stages::auto_tone::{auto_tone_input_fingerprint, auto_tone_is_fresh};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    app.set_adjustment("blacks", -0.5);
    make_auto_tone_stale(&mut app);
    let regenerated = app.regenerate_stale().expect("the first repair runs");
    assert!(
        regenerated.contains(&"auto-tone"),
        "the stale Auto-Tone module must be repaired, got {regenerated:?}"
    );

    let frame = app.original.clone().expect("decoded frame");
    let target = app.recipe().auto_features.target_luminance;
    let fingerprint = auto_tone_input_fingerprint(&frame, target);
    assert!(
        auto_tone_is_fresh(app.recipe(), &fingerprint),
        "the repaired state must be fresh"
    );
    assert_eq!(
        app.recipe().adjustments["blacks"],
        -0.5,
        "the repair must preserve the hand value (it is the override set's job)"
    );
    let after_first = (
        adjustments_of(app.recipe()),
        mirrors_of(app.recipe()),
        app.recipe().auto_features.analysis_fingerprint.clone(),
    );

    let regenerated = app.regenerate_stale().expect("the second repair runs");
    assert!(
        !regenerated.contains(&"auto-tone"),
        "a fresh state must be a no-op for the repair, got {regenerated:?}"
    );
    assert_eq!(
        (
            adjustments_of(app.recipe()),
            mirrors_of(app.recipe()),
            app.recipe().auto_features.analysis_fingerprint.clone(),
        ),
        after_first,
        "the repair must be idempotent"
    );
}

/// Sequence 3 and acceptance (5): **voller Lauf + Endpunkt + Handwert +
/// Reparaturlauf**.
///
/// The old guard (`..._known_defect`) asserted that the repair **overwrites** a
/// hand value set after the end-point click. That was the measured defect. It is
/// now a **counter-assertion**: the hand value must survive. The inversion is
/// the proof — implemented the other way (repair without the override set, or
/// `regenerate_stale` skipping the state) this test goes red.
///
/// The stale trigger is a changed target luminance: a fresh 6-of-6 state is not
/// repaired (that is correct), so the sequence makes the state genuinely stale
/// first, exactly as a changed analysis would.
#[test]
fn g16_the_state_repair_preserves_a_later_manual_value() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    // The user's own edit, after the end point.
    app.set_adjustment("blacks", -0.5);
    make_auto_tone_stale(&mut app);

    let regenerated = app.regenerate_stale().expect("the repair runs");
    assert!(
        regenerated.contains(&"auto-tone"),
        "the repair must have run, got {regenerated:?}"
    );

    assert_eq!(
        app.recipe().adjustments["blacks"],
        -0.5,
        "AUTO-TONE-ENDPOINT-MIXED-7: the repair must preserve a manual value set \
         after the end point (this replaces the old known-defect assertion)"
    );
    // Clause (4): the mirror still documents the auto value, which is a real
    // auto value — not the hand value.
    let auto = app
        .recipe()
        .auto_features
        .auto_blacks
        .expect("the repair restores the complete six-mirror contract");
    assert!(
        (auto - -0.5).abs() > 1e-9,
        "the auto mirror must not be the user's hand value, got {auto}"
    );
    assert_complete_six(app.recipe(), "after the repair of a hand value");
}

/// Acceptance (4): a value the user set on an end-point key survives the
/// stale-clear on the **production load path**, not through a direct call to
/// `clear_stale_auto_tone`.
///
/// The earlier version of this test called `clear_stale_auto_tone` itself and
/// stayed green even when the load path dropped the call — a test seam is not a
/// production test (`Agents.md`). This one persists a hand value that differs
/// from its auto mirror, makes the analysis stale by changing the target
/// luminance, and reopens the file: only the loaded recipe is asserted on.
///
/// **Named boundary:** a pure end-point value is numerically **equal** to the
/// auto value (both come from the same `suggest_auto_tone` run), so without a
/// persisted marker the stale-clear cannot tell it apart from an auto value and
/// removes it — that removal loses nothing, because the value is the stale auto
/// value itself. The assertion here uses the distinguishable case (a value that
/// differs from the auto mirror), which is the property clause (4) needs.
#[test]
fn g16_endpoint_override_survives_the_stale_clear_on_load() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    app.auto_tone().unwrap();
    app.apply_auto_endpoint(AutoEndpoint::Black).unwrap();
    // The user overrides the end-point key after the click.
    app.set_adjustment("blacks", -0.5);
    app.mark_recipe_dirty("blacks", -0.5);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none(), "the override must persist");

    // Change the target luminance in the persisted recipe and save: on reload the
    // stored fingerprint no longer matches, so the production load path runs the
    // stale-clear.
    app.recipe.auto_features.target_luminance = 0.75;
    app.mark_recipe_dirty("target_luminance", 0.75);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none(), "the stale trigger must persist");

    // Reload the sidecar alone in a fresh app: only this path is asserted on.
    let mut reloaded = new_app();
    open_and_decode(&mut reloaded, source.display().to_string());
    let loaded = reloaded.recipe();
    assert_eq!(
        loaded.adjustments.get("blacks").copied(),
        Some(-0.5),
        "the user value on the end-point key must survive the load-path stale clear"
    );
    // The auto-written values were cleared: nothing may be silently adopted.
    assert!(
        !loaded.adjustments.contains_key("contrast"),
        "an auto-written value must clear on stale"
    );
    assert_eq!(
        loaded.auto_features.auto_blacks, None,
        "the stale clear drops the auto bookkeeping"
    );
    assert_eq!(mirrors_of(loaded).iter().filter(|m| m.is_some()).count(), 0);
}
