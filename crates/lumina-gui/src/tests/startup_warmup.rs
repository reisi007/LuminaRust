//! R3-WARMUP-1 (Release 1.0): the one-shot cold-start warmup tests.
//!
//! Split from `tests/scheduling.rs` so both files stay within the strict
//! 500-line rule for new files (file-size ratchet). The warmup must build the
//! folder index, enqueue the first-screen thumbnails and kick off the
//! first-image decode on the existing background paths — with visible
//! progress — exactly once, and it must reuse an auto-load already in flight.

use super::*;
use crate::warmup::WARMUP_PROGRESS;

/// R3-WARMUP-1: the one-shot cold-start warmup builds the folder index,
/// enqueues the first-screen thumbnails and kicks off the first-image decode —
/// all on the existing background paths, with visible progress — and never
/// runs a second time.
#[test]
fn startup_warmup_builds_index_thumbnails_and_decode_once() {
    let directory = tempfile::tempdir().unwrap();
    // Fabricated RAW entries (extension-only scan): no startup auto-load runs,
    // so the warmup itself must start the decode.
    let mut app = new_app();
    app.directory = directory.path().display().to_string();
    app.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let first = app.entries[0].path.display().to_string();
    let ctx = egui::Context::default();

    // Unarmed warmup is inert.
    assert!(!app.maybe_run_startup_warmup(&ctx));
    assert!(app.warmup_report().thumbs_enqueued == 0);

    app.schedule_startup_warmup();
    assert!(app.warmup_pending());
    assert!(
        app.maybe_run_startup_warmup(&ctx),
        "the armed warmup must run once entries exist"
    );
    let report = app.warmup_report();
    assert!(report.index_built, "the folder preview index must be built");
    assert_eq!(
        report.thumbs_enqueued, 2,
        "both leading entries get a worker job"
    );
    assert!(report.decode_started, "the warmup starts the first decode");
    assert!(app.decode_rx.is_some(), "the decode must be in flight");
    assert_eq!(app.pending_load_path.as_deref(), Some(first.as_str()));

    // Visible, non-blocking progress: status + overlay toast.
    assert_eq!(app.status(), WARMUP_PROGRESS);
    assert!(app.toast_visible(0.0), "the warmup toast must be visible");

    // Exactly once — the armed flag is discharged.
    assert!(!app.warmup_pending());
    assert!(!app.maybe_run_startup_warmup(&ctx));
}

/// R3-WARMUP-1: with a real startup auto-load already running, the warmup
/// reuses it (no duplicate decode) and the first image ends up fully rendered.
#[test]
fn startup_warmup_reuses_auto_load_and_leaves_first_image_rendered() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    app.schedule_startup_warmup();
    app.set_directory(directory.path().display().to_string());
    let ctx = egui::Context::default();

    assert!(
        app.maybe_run_startup_warmup(&ctx),
        "warmup runs after the listing"
    );
    assert!(
        !app.warmup_report().decode_started,
        "the scan auto-load already started the decode; the warmup reuses it"
    );
    assert!(app.decode_rx.is_some());
    drain_auto_load(&mut app);
    assert!(
        app.error().is_none(),
        "decode must succeed: {:?}",
        app.error()
    );
    assert_eq!(app.path, source.display().to_string());
    assert!(
        app.render_key.is_some(),
        "the first image must be fully rendered (finish_decode/render path)"
    );
}

/// R3-WARMUP-1: a loaded-but-unrendered source is committed through the
/// existing full-render debounce path (the safety net), never a second render.
#[test]
fn startup_warmup_arms_debounce_render_for_unrendered_source() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.entries = vec![raw_entry(directory.path(), "a.cr3")];
    app.load_bytes(png(), "warmup.png").unwrap();
    // Simulate a source that is loaded but not yet committed.
    app.render_key = None;
    app.pending_full_render = false;
    app.schedule_startup_warmup();
    let ctx = egui::Context::default();
    // Baseline scheduler run: pins `last_scheduled_module` so the warmup-armed
    // render is not mistaken for the F7 module-switch deferral.
    app.schedule_render(&ctx);

    assert!(app.maybe_run_startup_warmup(&ctx));
    assert!(
        app.warmup_report().render_armed,
        "the warmup must arm the debounce render"
    );
    assert!(app.pending_full_render);
    // The existing scheduler commits it.
    app.schedule_render(&ctx);
    assert!(
        !app.pending_full_render,
        "the debounce path commits the render"
    );
    assert!(app.render_key.is_some(), "the full render must complete");
    assert!(app.error().is_none());
}

/// R4-SWITCH-2-Vorbereitung (Release 1.0): Warmup-Nachweis ohne Wanduhr.
///
/// Die R4-Messung zeigte den ersten Library-Paint bei 4286,1 ms, weil Ordner-
/// Index, Thumbnails und Decode/Render beim Switch noch ausstanden — und das
/// Warmup erst NACH dem Switch lief. Dieser Test fährt die Kaltstart-Sequenz
/// zweimal (Warmup an/aus) und belegt relativ statt absolut: in der gewärmten
/// App ist die Switch-Arbeit VOR dem Switch getan (Index-Build-Zähler > 0,
/// Decode in flight), in der kalten erst DANACH (Zähler 0 vor, 1 nach dem
/// ersten Thumbnail-Lauf). Beide Switches emittieren die vollständige
/// Timing-Trace (Event + First-Paint mit `switch_to_paint_ms=`). Bewusst kein
/// ms-Budget: Wanduhr flakt auf CI (R3-LOG-1 begründet nur Traces, keine Gates).
#[test]
fn warmup_frontloads_first_library_switch_work() {
    use crate::timing::take_timing_log;

    // Identical cold-start listings (fabricated RAW entries: extension-only
    // scan, no startup auto-load — the warmup itself must start the decode).
    let directory = tempfile::tempdir().unwrap();
    let dir_string = directory.path().display().to_string();
    let mut warm = new_app();
    warm.directory = dir_string.clone();
    warm.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let mut cold = new_app();
    cold.directory = dir_string;
    cold.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let ctx = egui::Context::default();

    // Cold: nothing front-loaded before the first Library switch.
    assert_eq!(cold.thumbnail_cache.builds(), 0);
    assert!(cold.decode_rx.is_none());

    // Warm: the armed one-shot warmup does the switch's work up front.
    warm.schedule_startup_warmup();
    assert!(warm.maybe_run_startup_warmup(&ctx));
    let report = warm.warmup_report();
    assert!(report.index_built);
    assert!(report.decode_started);
    assert!(warm.decode_rx.is_some());
    let warm_builds = warm.thumbnail_cache.builds();
    assert!(warm_builds >= 1, "the warmup must probe the folder index");

    // Both switches emit the complete timing trace (no silent switch).
    for app in [&mut warm, &mut cold] {
        let _ = take_timing_log();
        app.set_module(Module::Library);
        app.note_first_paint_after_switch();
        let log = take_timing_log();
        assert_eq!(log.len(), 2, "one event + one first paint: {log:?}");
        assert!(log[0].contains("module switch event") && log[0].contains("Library"));
        assert!(
            log[1].contains("module switch first paint")
                && log[1].contains("Library")
                && log[1].contains("switch_to_paint_ms=")
        );
        let raw = log[1].rsplit("switch_to_paint_ms=").next().unwrap();
        assert!(raw.contains('.'), "one-decimal format: {}", log[1]);
        let ms: f64 = raw.parse().expect("parseable milliseconds");
        assert!(ms >= 0.0 && ms.is_finite());
    }

    // Warmed: the switch reuses the warm index (no second build). Cold: the
    // same thumbnail run builds it only now (after the switch).
    let indices: Vec<usize> = (0..warm.entries().len()).collect();
    warm.ensure_thumbnail_priority(&ctx, &indices, 0..indices.len());
    assert_eq!(warm.thumbnail_cache.builds(), warm_builds);
    let indices: Vec<usize> = (0..cold.entries().len()).collect();
    cold.ensure_thumbnail_priority(&ctx, &indices, 0..indices.len());
    assert_eq!(cold.thumbnail_cache.builds(), 1);
}

/// R4-SWITCH-2: the warmup's arming and its deferral gates are traced, so a
/// warmup that ran late (the R4 run showed it 37 s after start, after the
/// first Library switch) is explainable from the trace instead of an
/// uninstrumented gap. The deferral line fires once per reason, never per
/// frame.
#[test]
fn warmup_arming_and_deferral_are_traced() {
    use crate::timing::take_timing_log;

    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    let ctx = egui::Context::default();
    let _ = take_timing_log();

    app.schedule_startup_warmup();
    let log = take_timing_log();
    assert!(
        log.iter().any(|line| line.contains("warmup armed")),
        "arming must be traced: {log:?}"
    );
    assert!(app.warmup_pending());

    // No listing/source yet: deferred loudly, exactly once across frames.
    assert!(!app.maybe_run_startup_warmup(&ctx));
    assert!(!app.maybe_run_startup_warmup(&ctx));
    let log = take_timing_log();
    let deferred: Vec<&String> = log
        .iter()
        .filter(|line| line.contains("warmup deferred"))
        .collect();
    assert_eq!(
        deferred.len(),
        1,
        "the deferral is traced once per reason: {log:?}"
    );
    assert!(deferred[0].contains("no listing yet"), "{:?}", deferred[0]);
    assert!(
        app.warmup_pending(),
        "a deferral must not discharge the warmup"
    );

    // Once entries exist the still-armed warmup proceeds (and discharges).
    app.entries = vec![raw_entry(directory.path(), "a.cr3")];
    assert!(app.maybe_run_startup_warmup(&ctx));
    assert!(!app.warmup_pending());
}
