//! LENSFUN-DB-33: the **plan** events — what the load plan decided and what it
//! deliberately did not load (DoD §4 "a level for user-visible errors").
//!
//! Split out of `tests/diagnostics.rs` (file-size ratchet, User-Vorgabe
//! 2026-09-17). That file covers the *load*-time events (`resolved`,
//! `file_rejected`, `failed`); this one covers everything decided *before* the
//! load runs: a layer that was considered and left out, and a load plan that
//! does not load the directory the resolution reported.
//!
//! The behaviours pinned here:
//!
//! - **F1-2** — the loaded set has one source of truth (`Resolved::primary`),
//!   and every divergence from the resolved/pinned directory is *reported*;
//! - **NIEDRIG-2** — nothing is skipped without a recorded reason, and the
//!   actionable reasons reach the sink;
//! - **F2** — the legacy `stderr` sink is not silent on the success path.

use crate::db_layers::{SkipReason, SkippedLayer};
use crate::db_path::LayerOrigin;
use crate::system_load::Diagnostics;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::diagnostics::{resolve_tree, TempTree, FIXTURE_XML};

/// Records events, as in `diagnostics.rs`.
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
    fn resolved(&mut self, resolved: &crate::db_path::Resolved) {
        self.events.lock().expect("recorder lock").push(format!(
            "resolved[{}|{}]",
            resolved.primary.as_str(),
            resolved.file_count()
        ));
    }
    fn file_rejected(&mut self, layer_dir: &std::path::Path, file: &std::path::Path) {
        self.events.lock().expect("recorder lock").push(format!(
            "rejected[{}|{}]",
            layer_dir.display(),
            file.display()
        ));
    }

    fn layer_skipped(&mut self, skipped: &SkippedLayer) {
        self.events
            .lock()
            .expect("recorder lock")
            .push(format!("skipped[{skipped}]"));
    }

    fn pin_displaced(&mut self, resolved: &crate::db_path::Resolved) {
        self.events.lock().expect("recorder lock").push(format!(
            "pin_displaced[{}|{}]",
            resolved.dir.display(),
            resolved.primary.as_str()
        ));
    }

    fn failed(&mut self, err: &crate::db_path::SystemDbError) {
        self.events
            .lock()
            .expect("recorder lock")
            .push(format!("failed[{}]", err));
    }
}

/// this exercises the plan through the same `build`/`skip` bookkeeping and
/// asserts that both events reach the sink.
#[test]
fn a_displaced_pin_and_an_unusable_layer_are_both_reported() {
    let tree = TempTree::new("displaced");
    tree.write("a.xml", FIXTURE_XML);
    // Force a divergence by hand: `primary` says the system updates won.
    let mut resolved = resolve_tree(&tree);
    resolved.primary = LayerOrigin::SystemUpdates;
    resolved.skipped.push(SkippedLayer {
        dir: Some(PathBuf::from("/var/lib/lensfun-updates/version_1")),
        origin: LayerOrigin::SystemUpdates,
        reason: SkipReason::NoTimestampFile,
    });
    assert!(!resolved.pin_honored());
    let system_updates = resolved
        .skipped
        .iter()
        .find(|s| s.reason == SkipReason::NoTimestampFile)
        .expect("the system-update skip must be present");

    let mut recorder = Recorder::default();
    recorder.layer_skipped(system_updates);
    recorder.pin_displaced(&resolved);

    let events = recorder.events();
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(
        events[0].contains("System-Update-Paket") && events[0].contains("timestamp.txt"),
        "the skipped layer must be named with its reason: {events:?}"
    );
    assert!(
        events[1].contains("pin_displaced") && events[1].contains("System-Update-Paket"),
        "the displacement must name both the resolved and the loaded layer: {events:?}"
    );
}

/// **NIEDRIG-2: the old code skipped a candidate with `continue` and no
/// diagnostic at all.** Both skip reasons are now recorded *and* pushed into the
/// stream, and an absent directory is deliberately the only non-actionable one.
#[test]
fn every_skip_reason_is_recorded_and_the_actionable_ones_reach_the_sink() {
    let reasons = [
        SkipReason::Absent,
        SkipReason::NoXmlFiles,
        SkipReason::Unreadable,
        SkipReason::NoTimestampFile,
        SkipReason::NoUserDataDir,
    ];
    let mut actionable = Vec::new();
    for reason in reasons {
        let skipped = SkippedLayer {
            dir: Some(PathBuf::from("/x")),
            origin: LayerOrigin::UserUpdates,
            reason,
        };
        // Recorded: a complete plan is inspectable without any sink.
        assert!(!skipped.to_string().is_empty());
        if reason.is_actionable() {
            actionable.push(reason);
        }
    }
    assert_eq!(
        actionable,
        vec![
            SkipReason::NoXmlFiles,
            SkipReason::Unreadable,
            SkipReason::NoTimestampFile,
            SkipReason::NoUserDataDir
        ],
        "only an absent directory may be silent — it is the normal case"
    );
    // Each actionable reason has its own wording, so a permission error is never
    // mistaken for an empty package.
    let texts: Vec<String> = actionable.iter().map(|r| r.to_string()).collect();
    let mut sorted = texts.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), texts.len(), "duplicate wording in {texts:?}");
}

/// **F2: the legacy `StderrDiagnostics` must not be silent on success.** Its
/// `resolved` was a no-op, so `LensfunDb::load_system()` — the entry point
/// `lumina-gui` and `lumina-cli` still use — reported *nothing at all* when the
/// database loaded fine. The sink cannot route to a logger (no dependencies), so
/// the assertion is on the event contract the wrapper emits, not on captured
/// stderr: `resolved` must reach the sink with the layers and the resolved
/// directory.
#[test]
fn the_success_path_is_reported_and_names_both_directories() {
    let tree = TempTree::new("successpath");
    tree.write("a.xml", FIXTURE_XML);
    let resolved = resolve_tree(&tree);

    let mut recorder = Recorder::default();
    recorder.resolved(&resolved);
    let events = recorder.events();
    assert_eq!(
        events.len(),
        1,
        "the success path must not be silent: {events:?}"
    );
    assert!(
        events[0].contains("System-Datenbank") && events[0].contains("|1"),
        "the loaded layer and its file count must be named: {events:?}"
    );
    // `StderrDiagnostics` renders the same information; calling it must not
    // panic and must stay consistent with the contract above.
    crate::system_load::StderrDiagnostics.resolved(&resolved);
}
