//! R2-MODSWITCH-1 F8 (Release 1.0): the asynchronous folder scan.
//!
//! These headless tests drive the **production** async path explicitly
//! ([`LuminaApp::begin_scan`]/[`LuminaApp::poll_scan`]) — the `list_directory`
//! convenience method runs the same engine synchronously under `cfg(test)` so
//! the existing listing suite needs no event loop. Both share `run_scan`, so
//! there is no second scan implementation.
//!
//! Pinned here:
//! 1. the scan runs off the caller's thread and lands asynchronously,
//! 2. a visible loading status is set while it is in flight,
//! 3. a superseded scan (rapid tree clicks) is dropped, latest-wins,
//! 4. a disconnected worker is surfaced loudly, never a silent stall.

use super::*;

fn write_png(path: &Path) {
    std::fs::write(
        path,
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap(),
    )
    .unwrap();
}

/// Poll the async scan until it lands (bounded), returning whether it applied.
fn pump_scan(app: &mut LuminaApp) -> bool {
    for _ in 0..2000 {
        if app.poll_scan() {
            return true;
        }
        if !app.scan_pending() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    false
}

/// The folder scan is handed to a worker thread; the caller returns before the
/// listing exists and applies it on a later `poll_scan`.
#[test]
fn scan_runs_on_a_worker_and_lands_asynchronously() {
    let directory = tempfile::tempdir().unwrap();
    write_png(&directory.path().join("a.png"));
    write_png(&directory.path().join("b.png"));
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    // `set_directory` itself is synchronous under test, so start a fresh async
    // scan on the same folder to exercise the worker path.
    app.begin_scan(false);
    assert!(
        app.scan_pending(),
        "an in-flight scan must be visible as pending"
    );
    assert!(
        app.status().contains("Scanning folder"),
        "the loading status must be visible while the scan runs, got {:?}",
        app.status()
    );
    assert!(pump_scan(&mut app), "the async scan must land");
    assert!(!app.scan_pending(), "pending must clear once applied");
    assert_eq!(app.entries().len(), 2, "both images must be listed");
}

/// A superseded scan (a second request before the first landed) is dropped:
/// the newest generation wins and the older listing never reaches the grid.
#[test]
fn superseded_scan_is_dropped_latest_wins() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    write_png(&first.path().join("old.png"));
    write_png(&second.path().join("new1.png"));
    write_png(&second.path().join("new2.png"));

    let mut app = new_app();
    app.set_directory(first.path().display().to_string());
    app.begin_scan(false);
    let first_generation = app.scan_generation;
    // Redirect before the first scan lands.
    app.set_directory(second.path().display().to_string());
    app.begin_scan(false);
    assert!(
        app.scan_generation > first_generation,
        "the second request must advance the generation"
    );
    assert!(pump_scan(&mut app), "the newest scan must land");
    let names: Vec<&str> = app.entries().iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"new1.png") && names.contains(&"new2.png"),
        "the newest folder must win, got {names:?}"
    );
    assert!(
        !names.contains(&"old.png"),
        "the superseded listing must never appear"
    );
}

/// A disconnected worker surfaces loudly instead of leaving the grid pending.
#[test]
fn disconnected_scan_worker_is_loud_not_a_silent_stall() {
    let mut app = new_app();
    // Fabricate an in-flight scan state whose sender is already gone: dropping
    // the sender disconnects the channel without sending a result.
    let (tx, rx) = std::sync::mpsc::channel::<crate::library_scan::ScanResult>();
    drop(tx);
    app.scan_rx = Some(rx);
    app.scan_pending = true;
    app.status = "Scanning folder…".into();
    assert!(!app.poll_scan(), "a dead worker cannot apply a listing");
    assert!(
        !app.scan_pending(),
        "the pending flag must clear so the UI is not stuck"
    );
    assert!(
        app.status().contains("not readable"),
        "the stall must be visible as an error status, got {:?}",
        app.status()
    );
}

/// The async worker and the synchronous test seam produce the same listing
/// (one scan engine, no drift).
#[test]
fn async_and_sync_scans_agree() {
    let directory = tempfile::tempdir().unwrap();
    write_png(&directory.path().join("one.png"));
    write_png(&directory.path().join("two.png"));

    let mut async_app = new_app();
    async_app.set_directory(directory.path().display().to_string());
    async_app.begin_scan(true);
    assert!(pump_scan(&mut async_app));
    let async_names: Vec<String> = async_app.entries().iter().map(|e| e.name.clone()).collect();

    let mut sync_app = new_app();
    sync_app.set_directory(directory.path().display().to_string());
    sync_app.scan_directory_blocking(true);
    let sync_names: Vec<String> = sync_app.entries().iter().map(|e| e.name.clone()).collect();

    assert_eq!(
        async_names, sync_names,
        "the worker and synchronous scan must agree"
    );
}
