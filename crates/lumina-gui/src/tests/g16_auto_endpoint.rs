//! G-16 auto tone end point state tests (`Shift`+double-click on
//! `whites`/`blacks`): the state a single action leaves in the recipe.
//!
//! Split out of `g16_shortcuts.rs` for the 500-line ratchet — the keyboard
//! mappings stay there, the recipe state is its own concern. The order-sensitive
//! acceptance sequences (frisch/voller Lauf/zweimal Endpunkt, the repair and the
//! load-path stale clear) live in `g16_auto_endpoint_sequences.rs`; this file
//! keeps the single-action tests.
//!
//! AUTO-TONE-ENDPOINT-MIXED-7: on a **fresh** recipe the end point is a pure
//! user override (0 of 6 mirrors = "no Auto-Tone", not a mixed state). On an
//! **existing** Auto-Tone state it is completed to 6 of 6 by
//! `apply_auto_endpoint` — see the sequence file.

use super::*;
use crate::tests::support::{captured_logs, clear_captured_logs};

/// `DoD.md` §7.4 for the two Auto-Tone user actions: every new user action
/// carries a log level, with evidence.
///
/// **Why this test exists:** the third verification round passed the task, and
/// the §7 checklist still had a hole. `apply_auto_endpoint` logs at `info!`
/// (`lib.rs`) and `auto_tone` goes through `instrument_gui_action!`, but a
/// `grep` over the test tree showed **15** references to
/// `apply_auto_endpoint` and **none** of them in a log assertion — the level
/// was asserted nowhere. A log line that nobody reads is not evidence that the
/// action is traceable in `RUST_LOG=trace` runs, which is the F-103-N6
/// obligation.
///
/// The end point is the interesting half: it is the action a user performs by
/// hand, and it is the one whose state was recently changed. Its line names the
/// key and the value, so a support session can tell which end point was used.
#[test]
fn g16_auto_tone_actions_log_their_level_and_the_key() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);

    // The end point: one `info!` line naming the key it wrote.
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    clear_captured_logs();
    app.apply_auto_endpoint(AutoEndpoint::White).unwrap();
    let endpoint_lines = captured_logs()
        .into_iter()
        .filter(|line| line.contains("apply_auto_endpoint"))
        .collect::<Vec<_>>();
    assert_eq!(
        endpoint_lines.len(),
        1,
        "the end point must log exactly one line, got {endpoint_lines:?}"
    );
    assert!(
        endpoint_lines[0].starts_with("INFO"),
        "the end point logs at INFO, got {}",
        endpoint_lines[0]
    );
    assert!(
        endpoint_lines[0].contains("whites"),
        "the line names the key it wrote, got {}",
        endpoint_lines[0]
    );

    // The full run: the instrumented action logs its own line. `AutoTone` is a
    // `GuiAction`, so this is a different line shape than the end point's.
    clear_captured_logs();
    app.auto_tone().unwrap();
    let auto_lines = captured_logs();
    assert!(
        auto_lines.iter().any(|line| line.contains("auto_tone")),
        "the full Auto Tone run must be traceable, got {auto_lines:?}"
    );
}

#[test]
fn g16_apply_auto_endpoint_sets_only_its_field() {
    // Effect test: Shift+double-click on Whites applies exactly the auto
    // white point from the shared auto-tone path (no second algorithm).
    // AUTO-TONE-ENDPOINT-MIXED-7: on a fresh recipe the end point is a pure
    // user override — no mirror, no enable_auto_tone, no fingerprint, so no
    // partial AUTO-TONE-2 contract can arise out of nothing.
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
    // No auto-tone state: the end point writes no mirror, no
    // enable_auto_tone, no fingerprint — a fresh recipe stays "no auto-tone".
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
    // AUTO-TONE-ENDPOINT-MIXED-7: on a fresh recipe the black end point is a
    // pure user override — no mirror, no enable_auto_tone, no fingerprint.
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
fn g16_apply_auto_endpoint_preserves_other_manual_values() {
    // Data preservation: a manual whites value survives an end-point click
    // on blacks (and vice versa) — on a fresh recipe the end point writes
    // exactly one field.
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
