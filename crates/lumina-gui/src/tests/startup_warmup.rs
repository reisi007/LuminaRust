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
