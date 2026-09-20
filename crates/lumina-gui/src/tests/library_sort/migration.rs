//! LRPAR-G09-SORT-09 (2026-09-20): the sort-order file moved into the folder's
//! deletable `.lumina/` cache directory.
//!
//! Coverage for the one-time, loud migration of the legacy
//! `<folder>/lumina-sort.json` (next to the images) into
//! `<folder>/.lumina/lumina-sort.json`, the normal write path and the
//! "no relic next to the images" guarantee.
//!
//! Split out of `tests/library_sort.rs` (module `tests::library_sort`) so both
//! files stay within the 500-line ratchet; the shared fixtures/helpers live in
//! the parent module and are pulled in via `use super::*`.

use super::*;

/// Process-wide capture of the `info!` migration line: installs one logger for
/// the library test binary and records every `INFO`-and-below message so the
/// one-time migration is proven to be loud (DoD §4), not only functional.
static MIGRATION_LOGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static LOGGER_INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();

struct CaptureLogger;

impl log::Log for CaptureLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            MIGRATION_LOGS
                .lock()
                .expect("migration log mutex")
                .push(format!("{}: {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

fn init_log_capture() {
    LOGGER_INIT.get_or_init(|| {
        let _ = log::set_boxed_logger(Box::new(CaptureLogger));
        log::set_max_level(log::LevelFilter::Info);
    });
}

/// Legacy (pre-2026-09-20) location directly next to the images.
fn legacy_file(dir: &Path) -> PathBuf {
    dir.join("lumina-sort.json")
}

/// The legacy file next to the images is migrated once into `.lumina/`: content
/// preserved, mode + order restored, legacy file retired.
#[test]
fn legacy_sort_file_next_to_images_is_migrated_once() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    std::fs::write(
        legacy_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"custom","order":["b.cr3","a.cr3"]}"#,
    )
    .unwrap();

    let mut app = new_app();
    scan(&mut app, dir.path());

    assert_eq!(app.library_sort(), LibrarySort::Custom);
    assert_eq!(visible_names(&app), vec!["b.cr3", "a.cr3"]);
    assert_eq!(
        app.library_sort_order().to_vec(),
        vec!["b.cr3".to_string(), "a.cr3".to_string()]
    );
    // The value now lives in `.lumina/`; the legacy file is retired, so no
    // second copy is silently kept behind.
    assert!(
        sort_file(dir.path()).is_file(),
        "the migrated order must live in `.lumina/`"
    );
    assert!(
        !legacy_file(dir.path()).exists(),
        "the legacy file must be retired after migration"
    );
    let raw = std::fs::read_to_string(sort_file(dir.path())).unwrap();
    assert!(raw.contains("\"b.cr3\""), "{raw}");
    assert!(
        !raw.contains(&dir.path().display().to_string()),
        "the migrated order stays portable: {raw}"
    );

    // One-time: a reload reads `.lumina/` and leaves the legacy path empty.
    let mut reopened = new_app();
    scan(&mut reopened, dir.path());
    assert_eq!(reopened.library_sort(), LibrarySort::Custom);
    assert_eq!(visible_names(&reopened), vec!["b.cr3", "a.cr3"]);
    assert!(!legacy_file(dir.path()).exists());
}

/// The one-time migration is loud: an `INFO` log line names the legacy and the
/// new location (DoD §4: user-visible action logs at least `info!`).
#[test]
fn legacy_migration_is_logged_at_info() {
    init_log_capture();
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    std::fs::write(
        legacy_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"custom","order":["a.cr3"]}"#,
    )
    .unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    let logs = MIGRATION_LOGS.lock().expect("migration log mutex");
    assert!(
        logs.iter().any(|line| line.contains("migrated legacy")),
        "the migration must log loudly, captured: {logs:?}"
    );
}

/// A normal sort/reorder writes only to `.lumina/lumina-sort.json` and never
/// leaves a relic next to the images.
#[test]
fn normal_save_writes_only_the_cache_path() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    let b = stub_raw(dir.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    app.reorder_library_entry(&b.display().to_string(), &a.display().to_string())
        .unwrap();
    assert!(sort_file(dir.path()).is_file(), "the order is cached");
    assert!(
        !legacy_file(dir.path()).exists(),
        "a save must never create a file next to the images"
    );
}

/// When both locations exist, the active `.lumina/` file wins; the legacy relic
/// is ignored (loudly logged), never merged or silently preferred.
#[test]
fn active_cache_file_wins_over_legacy_relic() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    write_sort_file(
        dir.path(),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"custom","order":["b.cr3","a.cr3"]}"#,
    );
    std::fs::write(
        legacy_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"custom","order":["a.cr3","b.cr3"]}"#,
    )
    .unwrap();

    let mut app = new_app();
    scan(&mut app, dir.path());

    assert_eq!(app.library_sort(), LibrarySort::Custom);
    assert_eq!(
        visible_names(&app),
        vec!["b.cr3", "a.cr3"],
        "the `.lumina/` file is authoritative"
    );
}
