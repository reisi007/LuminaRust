//! Decode-policy state tests: immediate navigation, latest-wins handoff, and
//! disconnected-worker cleanup.

use std::path::Path;

use eframe::egui;
use lumina_core::{ImageFileFormat, ImageFrame};

use super::super::{DecodeRequestResult, LuminaApp};

fn new_app() -> LuminaApp {
    LuminaApp::new(egui::Context::default())
}

fn write_png(path: &Path, rgb: [u8; 3]) {
    let bytes = ImageFrame::new(1, 1, vec![rgb[0], rgb[1], rgb[2], 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn settle_decode(app: &mut LuminaApp) {
    for _ in 0..120_000 {
        app.poll_decode();
        if !app.decode_pending() {
            assert!(
                app.pending_load_path.is_none(),
                "a settled request must clear its pending path"
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("background decode did not settle");
}

fn load_a() -> (tempfile::TempDir, LuminaApp) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("a.png");
    write_png(&source, [10, 20, 30]);
    let mut app = new_app();
    app.accept_dropped_file(&source, || panic!("path drop must not read bytes"));
    settle_decode(&mut app);
    (directory, app)
}

#[test]
fn ordinary_open_file_keeps_immediate_directory_navigation() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("ordinary.png");
    write_png(&source, [40, 80, 120]);
    let mut app = new_app();

    app.open_file(source.display().to_string());

    let source = source.display().to_string();
    assert_eq!(app.directory, directory.path().display().to_string());
    assert!(app
        .entries
        .iter()
        .any(|entry| entry.path.display().to_string() == source));
    assert_eq!(app.filmstrip_selection(), vec![source.clone()]);
    assert_eq!(app.filmstrip_anchor.as_deref(), Some(source.as_str()));
    assert!(app.auto_load_attempted);
    let generation = app.decode_generation;
    settle_decode(&mut app);
    assert_eq!(app.decode_generation, generation, "no auto-load duplicate");
}

#[test]
fn direct_begin_load_path_remains_non_navigating() {
    let (dir_a, mut app) = load_a();
    let source_a = app.path.clone();
    let directory = app.directory.clone();
    let entries = app.entries.len();
    let selection = app.filmstrip_selection.clone();
    let anchor = app.filmstrip_anchor.clone();
    let dir_b = tempfile::tempdir().unwrap();
    let source_b = dir_b.path().join("direct.png");
    write_png(&source_b, [80, 40, 20]);

    app.begin_load_path(source_b.display().to_string());

    assert_eq!(app.directory, directory);
    assert_eq!(app.entries.len(), entries);
    assert_eq!(app.filmstrip_selection, selection);
    assert_eq!(app.filmstrip_anchor, anchor);
    settle_decode(&mut app);
    assert_eq!(app.path, source_b.display().to_string());
    assert_eq!(app.directory, dir_a.path().display().to_string());
    assert_eq!(app.filmstrip_selection, selection);
    assert_eq!(app.filmstrip_anchor, anchor);
    assert_eq!(app.filmstrip_anchor.as_deref(), Some(source_a.as_str()));
}

#[test]
fn deferred_drop_supersedes_inflight_immediate_open() {
    let (_dir_a, mut app) = load_a();
    let immediate_dir = tempfile::tempdir().unwrap();
    let immediate = immediate_dir.path().join("immediate.png");
    write_png(&immediate, [70, 90, 110]);
    app.open_file(immediate.display().to_string());
    assert_eq!(app.directory, immediate_dir.path().display().to_string());
    assert_eq!(
        app.pending_load_path.as_deref(),
        Some(immediate.to_str().expect("UTF-8 fixture path"))
    );

    let deferred_dir = tempfile::tempdir().unwrap();
    let deferred = deferred_dir.path().join("deferred.png");
    write_png(&deferred, [140, 100, 60]);
    app.accept_dropped_file(&deferred, || panic!("path drop must not read bytes"));

    assert_eq!(
        app.pending_load_path.as_deref(),
        Some(deferred.to_str().expect("UTF-8 fixture path")),
        "the newest deferred request replaces the immediate target"
    );
    assert_eq!(app.directory, immediate_dir.path().display().to_string());
    settle_decode(&mut app);
    assert_eq!(app.path, deferred.display().to_string());
    assert_eq!(app.directory, deferred_dir.path().display().to_string());
    assert!(app.error().is_none());
}

#[test]
fn superseded_deferred_worker_cannot_adopt_its_directory() {
    let (dir_a, mut app) = load_a();
    let dir_b = tempfile::tempdir().unwrap();
    let source_b = dir_b.path().join("stale.png");
    write_png(&source_b, [120, 80, 40]);

    app.accept_dropped_file(&source_b, || panic!("path drop must not read bytes"));
    let stale_rx = app.decode_rx.take().expect("B worker receiver");
    let source_a = dir_a.path().join("a.png").display().to_string();
    app.open_file_deferred(source_a.clone());
    let current_rx = app.decode_rx.take().expect("newest worker receiver");

    app.decode_rx = Some(stale_rx);
    for _ in 0..120_000 {
        app.poll_decode();
        if !app.decode_pending() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!app.decode_pending(), "the stale result must be drained");
    assert_eq!(
        app.pending_load_path.as_deref(),
        Some(source_a.as_str()),
        "a stale result must not clear the newest request"
    );
    assert_eq!(app.path, source_a, "A remains the decoded source");
    assert_eq!(app.directory, dir_a.path().display().to_string());

    app.decode_rx = Some(current_rx);
    settle_decode(&mut app);
    assert_eq!(app.path, source_a);
    assert_eq!(app.directory, dir_a.path().display().to_string());
    assert!(app.error().is_none());
}

#[test]
fn disconnected_decode_worker_clears_pending_and_is_loud() {
    let (tx, rx) = std::sync::mpsc::channel::<DecodeRequestResult>();
    drop(tx);
    let mut app = new_app();
    app.pending_load_path = Some("disconnected.png".into());
    app.decode_rx = Some(rx);

    app.poll_decode();

    assert!(!app.decode_pending());
    assert!(app.pending_load_path.is_none());
    assert!(
        app.error()
            .is_some_and(|error| error.contains("worker disconnected")),
        "a dead worker must not leave a silent decoding stall"
    );
}
