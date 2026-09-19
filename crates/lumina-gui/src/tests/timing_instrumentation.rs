//! R3-LOG-1 (Release 1.0): headless tests for the wall-clock switch/decode/
//! render instrumentation in [`crate::timing`].
//!
//! Split out of `timing.rs` so the module itself stays within the strict
//! 500-line rule for new files (no new baseline entry). Every instrumented call
//! site is asserted to fire and carry the measured values; the delta math is
//! pinned against an injected clock (DoD §2).

use super::*;
#[cfg(feature = "gpu")]
use crate::timing::take_vram_refusal_warns;
use crate::timing::{format_ms, take_timing_log, Stopwatch};
use std::time::{Duration, Instant};

fn app() -> LuminaApp {
    let mut app = new_app();
    app.load_bytes(png(), "timing-test.png").unwrap();
    app
}

/// The injected clock pins the delta math exactly (no scheduler race).
#[test]
fn stopwatch_delta_is_exact_with_injected_clock() {
    let t0 = Instant::now();
    let stopwatch = Stopwatch::at(t0);
    assert_eq!(
        stopwatch.elapsed_ms_at(t0 + Duration::from_millis(1500)),
        1500.0
    );
    assert_eq!(
        stopwatch.elapsed_ms_at(t0 + Duration::from_micros(250)),
        0.25
    );
    // Out-of-order instants must never produce a negative delta.
    assert_eq!(Stopwatch::delta_ms(t0 + Duration::from_millis(5), t0), 0.0);
    assert_eq!(format_ms(1500.0), "1500.0");
    assert_eq!(format_ms(7.25), "7.2");
}

/// set_module records the event with the module name; the first painted
/// frame closes the delta exactly once.
#[test]
fn module_switch_event_and_first_paint_fire_once() {
    let mut app = app();
    let _ = take_timing_log();

    app.set_module(Module::Library);
    app.note_first_paint_after_switch();
    let log = take_timing_log();
    assert_eq!(
        log.len(),
        2,
        "one event line + one first-paint line: {log:?}"
    );
    assert!(
        log[0].contains("module switch event") && log[0].contains("Library"),
        "{}",
        log[0]
    );
    assert!(
        log[1].contains("module switch first paint")
            && log[1].contains("Library")
            && log[1].contains("switch_to_paint_ms="),
        "{}",
        log[1]
    );

    // No pending switch = no second first-paint line.
    app.note_first_paint_after_switch();
    assert!(take_timing_log().is_empty());
}

/// A switch into the same module still re-arms the event (the user pressed
/// the bar/shortcut; the delta is measured per event, not per state change).
#[test]
fn repeated_set_module_records_each_event() {
    let mut app = app();
    let _ = take_timing_log();
    app.set_module(Module::Develop);
    app.note_first_paint_after_switch();
    app.set_module(Module::Develop);
    app.note_first_paint_after_switch();
    let log = take_timing_log();
    assert_eq!(log.len(), 4, "two events, two first paints: {log:?}");
}

/// Decode start/finish carries path, ms and resolution; failure drops the
/// anchor too.
#[test]
fn decode_timing_reports_path_ms_and_resolution() {
    let mut app = app();
    let _ = take_timing_log();
    app.note_decode_start("photo.arw");
    app.note_decode_finish(6032, 4024);
    let log = take_timing_log();
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].contains("decode start") && log[0].contains("photo.arw"));
    assert!(
        log[1].contains("decode done")
            && log[1].contains("photo.arw")
            && log[1].contains("decode_ms=")
            && log[1].contains("resolution=6032x4024"),
        "{}",
        log[1]
    );

    app.note_decode_start("broken.arw");
    app.note_decode_failed();
    assert!(
        take_timing_log()
            .iter()
            .any(|line| line.contains("decode failed") && line.contains("broken.arw")),
        "a failed decode must still report its duration"
    );
}

/// The committed full render reports its wall time and output dimensions.
#[test]
fn full_render_line_reports_ms_and_output_dims() {
    let mut app = app();
    let _ = take_timing_log();
    app.render().unwrap();
    let log = take_timing_log();
    assert!(
        log.iter().any(|line| line.contains("full render done")
            && line.contains("render_ms=")
            && line.contains("output=2x1")),
        "{log:?}"
    );
}

/// The CPU texture upload reports its target + byte count; the F3 GPU-present
/// skip reports the bytes saved instead of uploading.
#[test]
fn texture_upload_and_f3_skip_report_bytes() {
    let ctx = egui::Context::default();
    let mut app = app();
    let _ = take_timing_log();
    // First upload creates the retained handle (navigator needs it).
    app.update_cpu_texture(&ctx, false);
    let first = take_timing_log();
    assert!(
        first.iter().any(
            |line| line.contains("texture upload target=navigator-handle")
                && line.contains("bytes=8")
        ),
        "2x1 RGBA = 8 bytes: {first:?}"
    );

    // A new identity with the CPU path active uploads to the preview target.
    app.preview_generation += 1;
    let _ = take_timing_log();
    app.update_cpu_texture(&ctx, false);
    let uploaded = take_timing_log();
    assert!(
        uploaded
            .iter()
            .any(|line| line.contains("texture upload target=preview") && line.contains("bytes=8")),
        "2x1 RGBA = 8 bytes: {uploaded:?}"
    );

    // A new identity with the GPU present active must skip the CPU upload and
    // name the saved bytes.
    app.preview_generation += 1;
    let _ = take_timing_log();
    app.update_cpu_texture(&ctx, true);
    let skipped = take_timing_log();
    assert!(
        skipped
            .iter()
            .any(|line| line.contains("texture upload skipped")
                && line.contains("target=preview")
                && line.contains("saved_bytes=8")),
        "{skipped:?}"
    );
}

/// R3-ROUTING-1: a persistent known refusal warns once, then only traces;
/// a *new* stage warns again.
#[cfg(feature = "gpu")]
#[test]
fn vram_refusal_warns_once_per_state_change() {
    let mut app = app();
    let _ = take_vram_refusal_warns();
    let geometry = "geometry (dimension-changing output)";
    let generative = "generative_edit (artifact-blind VRAM present)";

    assert!(app.note_vram_refusal(geometry), "first occurrence is new");
    assert!(
        !app.note_vram_refusal(geometry),
        "a repeat of the same stage must not warn again"
    );
    assert!(
        !app.note_vram_refusal(geometry),
        "still the same persistent stage"
    );
    assert_eq!(take_vram_refusal_warns(), 1, "one warn for one state");

    assert!(
        app.note_vram_refusal(generative),
        "a different stage is a new state"
    );
    assert_eq!(take_vram_refusal_warns(), 1, "one warn for the new state");
    assert!(
        app.note_vram_refusal(geometry),
        "returning to a previous stage is a state change again"
    );
    assert_eq!(take_vram_refusal_warns(), 1);
}

/// F7 (R2-MODSWITCH-1) fires for real: an armed full render in the switch frame
/// is deferred exactly once, then committed on the next frame. The counter is
/// the test-visible anchor for the `trace!` scheduler line.
#[test]
fn module_switch_deferral_fires_and_is_counted() {
    use crate::render_schedule::take_module_switch_deferrals;

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    std::fs::write(&source, png()).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    let ctx = egui::Context::default();
    app.schedule_render(&ctx);

    let _ = take_module_switch_deferrals();
    app.set_adjustment("exposure", 1.0);
    app.set_module(Module::Library);
    app.schedule_render(&ctx);
    assert_eq!(
        take_module_switch_deferrals(),
        1,
        "the switch frame must defer the armed full render exactly once"
    );
    assert!(
        app.pending_full_render,
        "the deferral keeps the render armed"
    );
    app.schedule_render(&ctx);
    assert_eq!(
        take_module_switch_deferrals(),
        0,
        "the committed frame must not defer again"
    );
}

/// R3-LOG-1 (MITTEL-1): every real module-switch entry point routes through
/// `set_module`, so the switch event is recorded (and a first paint armed) —
/// not only the module-bar button. Covers all seven former direct assignments.
#[test]
fn all_module_switch_entry_points_record_the_event() {
    let mut app = app();
    let _ = take_timing_log();

    // 1. Survey→Library (`toggle_compare_mode(Survey)`).
    app.toggle_compare_mode(CompareMode::Survey);
    app.note_first_paint_after_switch();
    assert!(
        take_timing_log()
            .iter()
            .any(|line| line.contains("module switch event") && line.contains("Library")),
        "Survey must record a Library switch event"
    );

    // 2–4. `set_library_view` Grid / Compare / People.
    for view in [LibraryView::Grid, LibraryView::Compare, LibraryView::People] {
        let _ = take_timing_log();
        app.set_library_view(view);
        app.note_first_paint_after_switch();
        let log = take_timing_log();
        assert!(
            log.iter()
                .any(|line| line.contains("module switch event") && line.contains("Library")),
            "set_library_view({view:?}) must record a Library switch event: {log:?}"
        );
        assert!(
            log.iter()
                .any(|line| line.contains("module switch first paint")),
            "set_library_view({view:?}) must arm the first-paint delta"
        );
    }

    // 5–6. Cmd/Ctrl+Shift+I / +E.
    for (action, module) in [
        (ImportExportAction::Import, "Library"),
        (ImportExportAction::Export, "Export"),
    ] {
        let _ = take_timing_log();
        app.apply_import_export_action(action);
        app.note_first_paint_after_switch();
        let log = take_timing_log();
        assert!(
            log.iter().any(|line| line.contains("module switch event")
                && line.contains(&format!("module={module}"))),
            "{action:?} must record a {module} switch event: {log:?}"
        );
    }

    // 7. Library-grid double-click → Develop.
    let _ = take_timing_log();
    app.open_grid_entry_in_develop(String::new());
    app.note_first_paint_after_switch();
    assert!(
        take_timing_log()
            .iter()
            .any(|line| line.contains("module switch event") && line.contains("Develop")),
        "the grid double-click must record a Develop switch event"
    );
}

/// F1 (R2-JANK-1) fires for real: a second draft tick inside the 16 ms budget
/// is throttled (the frame-budget path, test-visible anchor for the `trace!`).
#[test]
fn draft_tick_throttle_fires_inside_the_budget() {
    use crate::render_tick::take_draft_tick_throttles;

    let mut app = app();
    app.render().unwrap();
    let _ = take_draft_tick_throttles();

    // First tick at t=1.0 renders; a second at t=1.005 is inside the budget.
    app.render_draft_tick_at([64, 48], 1.0);
    assert_eq!(take_draft_tick_throttles(), 0, "the first tick renders");
    app.render_draft_tick_at([64, 48], 1.005);
    assert_eq!(
        take_draft_tick_throttles(),
        1,
        "a tick inside the 16 ms budget must be throttled"
    );
    app.render_draft_tick_at([64, 48], 1.020);
    assert_eq!(
        take_draft_tick_throttles(),
        0,
        "past the budget the tick renders again"
    );
}
