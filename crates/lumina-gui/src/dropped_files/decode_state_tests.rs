//! Decode-policy state tests: immediate navigation, latest-wins handoff, and
//! disconnected-worker cleanup.

use std::path::{Path, PathBuf};

use eframe::egui;
use lumina_core::{ImageFileFormat, ImageFrame};

use super::super::{library_scan, DecodeRequestResult, LuminaApp, PendingDirectoryOpen};

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

fn write_three_pngs(directory: &Path) -> [PathBuf; 3] {
    [
        directory.join("b0.png"),
        directory.join("b1.png"),
        directory.join("b2.png"),
    ]
    .into_iter()
    .zip([[10, 20, 30], [40, 50, 60], [70, 80, 90]])
    .map(|(path, rgb)| {
        write_png(&path, rgb);
        path
    })
    .collect::<Vec<_>>()
    .try_into()
    .unwrap()
}

fn arm_async_target(app: &mut LuminaApp, directory: &Path, active: &Path) {
    app.directory = directory.display().to_string();
    app.path = active.display().to_string();
    app.pending_directory_open = Some(PendingDirectoryOpen {
        directory: directory.display().to_string(),
        active_path: active.display().to_string(),
        scan_generation: None,
    });
}

#[test]
fn deferred_cross_directory_neighbors_wait_for_async_listing() {
    let (_dir_a, mut app) = load_a();
    let a_entry = app.entries[0].path.clone();
    let a_key = app.entries[0].thumb_key.clone();
    let dir_b = tempfile::tempdir().unwrap();
    let paths = write_three_pngs(dir_b.path());
    let active = &paths[1];

    // This is the state produced by a successful deferred B decode. Drive the
    // production worker explicitly; `list_directory()` is synchronous only in
    // cfg(test) and would hide the pending-listing window this test owns.
    arm_async_target(&mut app, dir_b.path(), active);
    let generation = app.begin_scan(false);

    assert!(app.scan_pending());
    assert_eq!(
        app.pending_directory_open.as_ref().unwrap().scan_generation,
        Some(generation)
    );
    assert!(app.status().contains("Scanning folder"));
    assert_eq!(app.entries[0].path, a_entry, "A stays listed while B scans");
    assert!(
        app.preview_ctrl
            .as_ref()
            .is_none_or(|ctrl| ctrl.in_flight_probes().is_empty()),
        "no neighbors use A's listing"
    );
    let bytes = std::fs::read(active).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    app.apply_decoded_frame(&frame, 1, None, "b1.png", &bytes, false, None);
    assert!(
        app.status().contains("Scanning folder"),
        "a decoded frame must not replace the pending scan status"
    );

    let mut applied = false;
    for _ in 0..2_000 {
        if app.poll_scan() {
            applied = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(applied, "the production scan must land");
    assert!(!app.scan_pending());
    assert!(!app.status().contains("Scanning folder"));
    assert_eq!(app.frame_previews_enqueued, 2);

    let expected: Vec<String> = [&paths[0], &paths[2]]
        .iter()
        .map(|path| path.canonicalize().unwrap().to_string_lossy().into_owned())
        .collect();
    let mut probes = app
        .preview_ctrl
        .as_ref()
        .expect("B listing schedules its neighbor controller")
        .in_flight_probes();
    probes.sort();
    assert_eq!(probes, expected, "only B -1/+1 may be scheduled");
    assert!(!probes.contains(&a_key));
}

#[test]
fn disconnected_scan_clears_pending_target_without_scheduling_a() {
    let (_dir_a, mut app) = load_a();
    let dir_b = tempfile::tempdir().unwrap();
    let paths = write_three_pngs(dir_b.path());
    arm_async_target(&mut app, dir_b.path(), &paths[1]);
    let (tx, rx) = std::sync::mpsc::channel::<library_scan::ScanResult>();
    drop(tx);
    app.scan_rx = Some(rx);
    app.scan_pending = true;
    app.status = library_scan::SCAN_PROGRESS.into();

    assert!(!app.poll_scan());
    assert!(!app.scan_pending());
    assert!(app.pending_directory_open.is_none());
    assert!(
        app.preview_ctrl
            .as_ref()
            .is_none_or(|ctrl| ctrl.in_flight_probes().is_empty()),
        "a failed scan cannot schedule A"
    );
    assert!(!app.status().contains("Scanning folder"));
    assert!(app
        .entries
        .iter()
        .all(|entry| !entry.path.starts_with(dir_b.path())));
}

#[test]
fn stale_scan_does_not_schedule_pending_target_neighbors() {
    let (_dir_a, mut app) = load_a();
    let dir_b = tempfile::tempdir().unwrap();
    let paths = write_three_pngs(dir_b.path());
    arm_async_target(&mut app, dir_b.path(), &paths[1]);
    let old_generation = app.scan_generation + 1;
    app.pending_directory_open.as_mut().unwrap().scan_generation = Some(old_generation);
    app.scan_generation = old_generation + 1;
    let (tx, rx) = std::sync::mpsc::channel::<library_scan::ScanResult>();
    tx.send(library_scan::ScanResult {
        directory: dir_b.path().to_path_buf(),
        entries: Vec::new(),
        generation: old_generation,
    })
    .unwrap();
    app.scan_rx = Some(rx);
    app.scan_pending = true;
    app.status = library_scan::SCAN_PROGRESS.into();

    assert!(!app.poll_scan());
    assert!(app.pending_directory_open.is_none());
    assert!(
        app.preview_ctrl
            .as_ref()
            .is_none_or(|ctrl| ctrl.in_flight_probes().is_empty()),
        "stale completion cannot schedule A"
    );
    assert!(app
        .entries
        .iter()
        .all(|entry| !entry.path.starts_with(dir_b.path())));
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
fn pathless_read_failure_supersedes_inflight_path_decode() {
    let (_dir_a, mut app) = load_a();
    let source_a = app.path.clone();
    let bytes_a = app.source_bytes.clone().expect("A bytes");
    let recipe_before = app.recipe().clone();
    let dir_b = tempfile::tempdir().unwrap();
    let source_b = dir_b.path().join("b.png");
    write_png(&source_b, [200, 100, 50]);

    app.accept_dropped_file(&source_b, || panic!("path drop must not read bytes"));
    assert!(app.decode_pending(), "B decode must be in flight");
    let in_flight_generation = app.decode_generation;

    // Drop-event latest-wins: this failed pathless drop is the newest drop
    // event, so it invalidates the in-flight B decode (generation bump +
    // receiver replaced) and the older worker must never land after it.
    app.accept_dropped_file(Path::new(""), || Err("usb unplugged".to_string()));

    assert!(
        app.error()
            .is_some_and(|error| error.contains("usb unplugged")),
        "the failed drop must report loudly"
    );
    assert!(
        !app.decode_pending(),
        "the failed drop supersedes the B worker"
    );
    assert!(app.pending_load_path.is_none());
    assert!(
        app.decode_generation > in_flight_generation,
        "the failed drop must invalidate the in-flight generation"
    );

    // Give the superseded B worker a chance to land: its receiver is gone, so
    // its send is a silent no-op and `poll_decode` has nothing to apply.
    for _ in 0..200 {
        app.poll_decode();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(app.path, source_a);
    assert_eq!(app.source_name, "a.png");
    assert_eq!(app.source_bytes.as_deref(), Some(bytes_a.as_slice()));
    assert_eq!(app.recipe(), &recipe_before);
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
