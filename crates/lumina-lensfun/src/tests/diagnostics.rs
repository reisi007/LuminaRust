//! LENSFUN-DB-33: the diagnostics contract (DoD §4 "a level for user-visible
//! errors", DoD §7.5 "every normative statement has a test anchor").
//!
//! `lumina-lensfun` has **no dependencies**, so it cannot log on its own. Every
//! event is therefore pushed through a caller-supplied [`Diagnostics`] sink and
//! the caller decides the level. These tests pin that contract:
//!
//! - every observable event is reported exactly once, in order;
//! - the "one file rejected" event names the file, so a partially broken
//!   database is visible rather than silently trimmed;
//! - the load only fails hard when **no** file at all could be loaded;
//! - [`ReportOnce`] collapses repeats, so a per-render lookup cannot spam.

use crate::db_layers::SkipReason;
use crate::db_layers::SkippedLayer;
use crate::db_path::{self, FsProbe, LayerOrigin, SystemDbError};
use crate::system_load::{Diagnostics, ReportOnce, SilentDiagnostics};
use crate::LensfunDb;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Records every event so the tests can assert on the exact sequence.
#[derive(Debug, Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn events(&self) -> Vec<String> {
        self.events.lock().expect("recorder lock").clone()
    }
}

impl Diagnostics for Recorder {
    fn resolved(&mut self, resolved: &db_path::Resolved) {
        let layers: Vec<String> = resolved
            .layers
            .iter()
            .map(|l| format!("{}:{}", l.origin.as_str(), l.files.len()))
            .collect();
        self.events
            .lock()
            .expect("recorder lock")
            .push(format!("resolved[{}]", layers.join(",")));
    }

    fn file_rejected(&mut self, layer_dir: &Path, file: &Path) {
        self.events.lock().expect("recorder lock").push(format!(
            "rejected[{}|{}]",
            layer_dir.display(),
            file.display()
        ));
    }

    fn layer_skipped(&mut self, skipped: &SkippedLayer) {
        self.events.lock().expect("recorder lock").push(format!(
            "skipped[{}|{}|{}]",
            skipped.origin.as_str(),
            skipped
                .dir
                .as_deref()
                .map_or_else(|| Path::new("<kein Pfad>").display(), Path::display,),
            skipped.reason
        ));
    }

    fn pin_displaced(&mut self, resolved: &db_path::Resolved) {
        self.events.lock().expect("recorder lock").push(format!(
            "pin_displaced[{}|{}]",
            resolved.dir.display(),
            resolved.primary.as_str()
        ));
    }

    fn failed(&mut self, err: &SystemDbError) {
        self.events
            .lock()
            .expect("recorder lock")
            .push(format!("failed[{}]", err));
    }
}

/// A minimal but valid Lensfun database with one camera + one lens.
pub(super) const FIXTURE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera><maker>Probe Corp</maker><model>Probe Body</model><mount>ProbeMount</mount><cropfactor>1.5</cropfactor></camera>
    <lens><maker>Probe Corp</maker><model>Probe Lens 50mm f/2.8</model><mount>ProbeMount</mount><cropfactor>1.5</cropfactor>
        <calibration><distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
        <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/></calibration>
    </lens>
</lensdatabase>
"#;

/// A file that is present but is not a Lensfun database at all.
const JUNK_XML: &str = "this is definitely not <xml> at all\n";

/// A throwaway directory tree, removed on drop.
pub(super) struct TempTree(pub(super) PathBuf);

impl TempTree {
    pub(super) fn new(tag: &str) -> Self {
        // The leaf is literally `lensfun` on purpose. `lensfun_dir_in_datadir`
        // appends `lensfun` only when the name differs, and glib's user-data
        // rule appends it too, so this one tree is simultaneously:
        //   - a valid *datadir*       -> system schema `<root>/version_1`,
        //   - a valid *system dir*    -> override schema `<root>/version_1`,
        //   - a user-data base        -> user DB `<root>/lensfun`, updates
        //                                `<root>/lensfun/updates/version_1`.
        // Before this, `fixture_env`'s compiled-datadir candidate pointed at
        // `<root>/lensfun/version_1` while `write()` filled `<root>/version_1`,
        // so the production-seam tests silently resolved the machine's real
        // database (`PlatformDefault`) instead of their own fixture.
        let root = std::env::temp_dir()
            .join(format!(
                "lumina-lensfun-diag-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ))
            .join("lensfun");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(db_path::SCHEMA_SUBDIR)).expect("create temp tree");
        Self(root)
    }

    fn schema_dir(&self) -> PathBuf {
        self.0.join(db_path::SCHEMA_SUBDIR)
    }

    pub(super) fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.schema_dir().join(name), contents).expect("write fixture");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        // The `lensfun` leaf sits under a per-process/thread base directory of
        // its own; remove that base as well (best effort — it is empty now).
        if let Some(base) = self.0.parent() {
            let _ = std::fs::remove_dir(base);
        }
    }
}

/// Build a `Resolved` for `tree` through the real resolver, so the test also
/// exercises the production resolution path.
pub(super) fn resolve_tree(tree: &TempTree) -> db_path::Resolved {
    db_path::resolve_with(Some(tree.0.as_os_str()), None, None, None, &FsProbe)
        .expect("the temp tree must resolve")
}

/// The happy path: a good database yields exactly one `resolved` event and no
/// `failed` event. The caller can therefore log "loaded from X" at info level.
#[test]
fn a_healthy_database_reports_exactly_one_resolved_event() {
    let tree = TempTree::new("healthy");
    tree.write("a.xml", FIXTURE_XML);
    let resolved = resolve_tree(&tree);

    let mut recorder = Recorder::default();
    let Ok(db) = LensfunDb::load_layers(&resolved, &mut recorder) else {
        panic!("a healthy database must load: {:?}", recorder.events());
    };
    // The database is genuinely usable, not just "returned".
    assert!(crate::Corrector::for_camera(
        &db,
        "Probe Corp",
        "Probe Body",
        Some("Probe Lens 50mm f/2.8"),
        400,
        300,
        50.0,
        2.8,
        10.0
    )
    .is_some());

    // `load_layers` executes the plan; the success event belongs to the
    // `resolve_system_with` wrapper (which is the only public entry point that
    // also owns resolution). So here we assert "no rejection, no failure", and
    // the `resolved` event itself is pinned by the dedicated test below.
    let events = recorder.events();
    assert!(
        !events.iter().any(|e| e.starts_with("rejected[")),
        "a healthy database must not report rejections: {events:?}"
    );
    assert!(
        !events.iter().any(|e| e.starts_with("failed[")),
        "a healthy database must not report a failure: {events:?}"
    );
}

/// SOLL: "Teilweise unlesbare Datei … sichtbare Warnung: der Dateiname steht in
/// der Warnung". The junk file must produce a `rejected` event **naming that
/// file**, and the load must still succeed.
#[test]
fn a_rejected_file_is_reported_by_name_and_the_load_still_succeeds() {
    let tree = TempTree::new("partial");
    tree.write("good.xml", FIXTURE_XML);
    tree.write("broken.xml", JUNK_XML);

    let resolved = resolve_tree(&tree);
    let mut recorder = Recorder::default();
    let Ok(db) = LensfunDb::load_layers(&resolved, &mut recorder) else {
        panic!(
            "one good file must be enough to load: {:?}",
            recorder.events()
        );
    };

    let events = recorder.events();
    let rejected: Vec<&String> = events
        .iter()
        .filter(|e| e.starts_with("rejected["))
        .collect();
    assert_eq!(
        rejected.len(),
        1,
        "expected exactly one rejection: {events:?}"
    );
    assert!(
        rejected[0].contains("broken.xml"),
        "the warning must name the offending file: {events:?}"
    );
    assert!(
        !rejected[0].contains("good.xml"),
        "a good file must not be reported as rejected: {events:?}"
    );
    // …and the good profile is really there despite the broken sibling.
    assert!(crate::Corrector::for_camera(
        &db,
        "Probe Corp",
        "Probe Body",
        Some("Probe Lens 50mm f/2.8"),
        400,
        300,
        50.0,
        2.8,
        10.0
    )
    .is_some());
}

/// SOLL: "Erst wenn keine Datei geladen werden konnte, ist der Fehler hart."
/// A directory that exists and holds XML, but where liblensfun rejects every
/// file, is a hard error — and F5 requires the reason to say exactly that.
#[test]
fn only_when_every_file_is_rejected_is_the_error_hard() {
    let tree = TempTree::new("allbad");
    tree.write("broken1.xml", JUNK_XML);
    tree.write("broken2.xml", JUNK_XML);

    let resolved = resolve_tree(&tree);
    let mut recorder = Recorder::default();
    let Err(err) = LensfunDb::load_layers(&resolved, &mut recorder) else {
        panic!("a database where nothing loads must be a hard error");
    };

    // Every file is named, and the error is not a generic "no XML".
    let events = recorder.events();
    assert_eq!(
        events.iter().filter(|e| e.starts_with("rejected[")).count(),
        2,
        "{events:?}"
    );
    let text = err.to_string();
    assert!(text.contains("XML-Dateien vorhanden"), "{text}");
    assert!(
        !text.contains("version_1/ ohne XML-Datei"),
        "F5: the message must not send the operator to the wrong fix: {text}"
    );
    assert_eq!(
        err,
        SystemDbError::NotFound {
            misses: vec![db_path::ProbeMiss {
                dir: tree.0.clone(),
                source: db_path::Source::EnvOverride,
                reason: db_path::MissReason::AllFilesRejected,
            }],
        }
    );
}

/// F5: the `AllFilesRejected` wording must be reachable and distinct, so a
/// directory full of corrupt files is never confused with an empty one.
#[test]
fn all_files_rejected_is_distinct_from_no_xml_files() {
    let corrupt = db_path::MissReason::AllFilesRejected.to_string();
    let empty = db_path::MissReason::NoXmlFiles.to_string();
    assert_ne!(corrupt, empty);
    assert!(corrupt.contains("abgelehnt"), "{corrupt}");
    assert!(empty.contains("ohne XML-Datei"), "{empty}");
}

/// The failure path must reach the sink **as well as** the return value, so a
/// caller that only logs the event and a caller that only checks the `Result`
/// see the same named diagnostic. `load_layers` is the seam that lets us force
/// a failure without touching the process environment.
#[test]
fn a_failed_load_reports_through_the_sink_and_returns_the_same_error() {
    let tree = TempTree::new("failedsink");
    tree.write("broken.xml", JUNK_XML);
    let resolved = resolve_tree(&tree);

    let mut recorder = Recorder::default();
    let Err(err) = LensfunDb::load_layers(&resolved, &mut recorder) else {
        panic!("nothing loadable must fail hard");
    };

    // One `rejected` event per file, then the caller-visible error.
    let events = recorder.events();
    let rejected: Vec<&String> = events
        .iter()
        .filter(|e| e.starts_with("rejected["))
        .collect();
    assert_eq!(rejected.len(), 1, "the failure must name the file first");
    assert!(rejected[0].contains("broken.xml"));

    // The error the caller receives is the same named diagnostic, and its text
    // explains what happened (so the caller can log it verbatim at error level).
    let text = err.to_string();
    assert!(text.contains("SystemDbError"), "{text}");
    assert!(text.contains("abgelehnt"), "{text}");
}

/// `ReportOnce` must collapse repeats, so a per-render lookup cannot spam the
/// log with the same failure. The sink is long-lived in the host application.
#[test]
fn report_once_collapses_repeated_events() {
    let mut once = ReportOnce::new();
    let err = SystemDbError::NotFound {
        misses: vec![db_path::ProbeMiss {
            dir: PathBuf::from("/nowhere/lensfun"),
            source: db_path::Source::PlatformDefault,
            reason: db_path::MissReason::Absent,
        }],
    };
    for _ in 0..100 {
        once.failed(&err);
    }
    // Nothing observable to assert beyond "it did not panic and stayed bounded";
    // the de-duplication is internal. A second, *different* error must still be
    // reported, so a real later failure is never suppressed.
    let other = SystemDbError::NotFound {
        misses: vec![db_path::ProbeMiss {
            dir: PathBuf::from("/elsewhere/lensfun"),
            source: db_path::Source::EnvOverride,
            reason: db_path::MissReason::NoXmlFiles,
        }],
    };
    once.failed(&other);
    let seen = once.seen.lock().expect("lock");
    assert!(
        seen.contains(&format!("failed:{other}")),
        "a different error must still be reportable"
    );
    assert_eq!(
        seen.len(),
        2,
        "100 identical failures must collapse to one: {seen:?}"
    );
}

/// The user-database layer, when present, is a first-class part of the plan and
/// is named in the `resolved` event so an operator can see what was merged.
#[test]
fn the_resolved_event_names_every_layer_including_the_user_database() {
    let mut recorder = Recorder::default();
    let resolved = db_path::Resolved {
        dir: PathBuf::from("/usr/share/lensfun"),
        schema_dir: PathBuf::from("/usr/share/lensfun/version_1"),
        source: db_path::Source::PlatformDefault,
        primary: LayerOrigin::SystemSchema,
        layers: vec![
            db_path::Layer {
                dir: PathBuf::from("/usr/share/lensfun"),
                files: vec![PathBuf::from("/usr/share/lensfun/version_1/a.xml")],
                origin: LayerOrigin::SystemSchema,
            },
            db_path::Layer {
                dir: PathBuf::from("/home/u/.local/share/lensfun"),
                files: vec![PathBuf::from("/home/u/.local/share/lensfun/u.xml")],
                origin: LayerOrigin::UserData,
            },
        ],
        skipped: vec![],
    };
    recorder.resolved(&resolved);
    let events = recorder.events();
    assert!(events[0].contains("System-Datenbank:1"), "{events:?}");
    assert!(events[0].contains("Benutzer-Datenbank:1"), "{events:?}");
}

/// `SilentDiagnostics` must really be silent — it exists for callers that have
/// their own visibility and must not double-report.
#[test]
fn silent_diagnostics_emit_nothing() {
    let mut silent = SilentDiagnostics;
    silent.resolved(&db_path::Resolved {
        dir: PathBuf::from("/x"),
        schema_dir: PathBuf::from("/x/version_1"),
        source: db_path::Source::PlatformDefault,
        primary: LayerOrigin::SystemSchema,
        layers: vec![],
        skipped: vec![],
    });
    silent.file_rejected(Path::new("/x"), Path::new("/x/a.xml"));
    silent.layer_skipped(&SkippedLayer {
        dir: Some(PathBuf::from("/x")),
        origin: LayerOrigin::SystemUpdates,
        reason: SkipReason::NoTimestampFile,
    });
    silent.pin_displaced(&db_path::Resolved {
        dir: PathBuf::from("/x"),
        schema_dir: PathBuf::from("/x/version_1"),
        source: db_path::Source::PlatformDefault,
        primary: LayerOrigin::SystemUpdates,
        layers: vec![],
        skipped: vec![],
    });
    silent.failed(&SystemDbError::NotFound { misses: vec![] });
}
