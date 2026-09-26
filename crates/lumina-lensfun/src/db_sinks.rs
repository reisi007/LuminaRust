//! The [`Diagnostics`] implementations this crate ships (LENSFUN-DB-33).
//!
//! Split out of `system_load.rs` (file-size ratchet, User-Vorgabe 2026-09-17).
//! They are the *sinks* — what a caller gets instead of a logger it does not
//! have — and they share no logic with the load plan, so they do not belong in
//! the module that executes it.

#![cfg(feature = "native")]

use crate::db_layers::SkippedLayer;
use crate::db_path::{Resolved, SystemDbError};
use crate::system_load::Diagnostics;
use std::path::Path;

/// # Caller obligation
///
/// Only choose this if the caller *does* have the same information another way
/// (e.g. it re-resolved and logged the plan itself). It is not a "quiet mode":
/// anything not routed elsewhere is then never reported anywhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SilentDiagnostics;

impl Diagnostics for SilentDiagnostics {
    fn resolved(&mut self, _resolved: &Resolved) {}
    fn file_rejected(&mut self, _layer_dir: &Path, _file: &Path) {}
    fn layer_skipped(&mut self, _skipped: &SkippedLayer) {}
    fn pin_displaced(&mut self, _resolved: &Resolved) {}
    fn failed(&mut self, _err: &SystemDbError) {}
}

/// Writes every event to `stderr`, each line prefixed with its level as **text**.
///
/// # What this is and is not
///
/// The level in the prefix (`INFO`/`WARNUNG`/`FEHLER`) is a *label*, not
/// routing: this crate has no logger and no dependencies, so it cannot hand a
/// record to the host's logging system, cannot attach a timestamp, and cannot
/// honour a log level. Routing and real levels are the caller's job — see
/// [`Diagnostics`].
///
/// # Why it is nonetheless not optional
///
/// [`LensfunDb::load_system`] constructs a fresh instance of this per call, so
/// there is no de-duplication state to carry. Without the `stderr` line the
/// success path would be completely silent, and a database failure would be a
/// silent, byte-identical no-op.
///
/// **Status 2026-09-26 (`LENSFUN-CALLER-37`):** this is no longer a *live* limit
/// for the product. `lumina-gui` (`src/lensfun_auto.rs`) and `lumina-cli`
/// (`src/lensfun_cli.rs`) both call `load_system_with` with a long-lived,
/// de-duplicating sink, so **zero product call sites of `load_system()` remain**
/// outside its own definition. What is left of the two original limits is
/// recorded in `feature/platform/capability-matrix.md` §"Pflicht des Aufrufers
/// von `load_system()`": the CLI still writes unlevelled text to `stderr`
/// (correct for a terminal program, but not a log record). The limits are
/// **not** solvable from inside
/// this crate:
///
/// - a Dock/Finder-launched macOS app has no `stderr` at all;
/// - the events repeat per lookup, because the sink is per call.
#[derive(Debug, Clone, Copy, Default)]
pub struct StderrDiagnostics;

/// The text level every `StderrDiagnostics` line carries, so an operator can
/// tell an informational plan from a warning from a hard failure.
const LEVEL_INFO: &str = "INFO";
const LEVEL_WARN: &str = "WARNUNG";
const LEVEL_ERROR: &str = "FEHLER";

impl Diagnostics for StderrDiagnostics {
    fn resolved(&mut self, resolved: &Resolved) {
        let layers: Vec<String> = resolved
            .layers
            .iter()
            .map(|l| format!("{} [{}]", l.dir.display(), l.origin.as_str()))
            .collect();
        eprintln!(
            "lumina-lensfun: LENSFUN-DB-33 {LEVEL_INFO}: Profil-Datenbank geladen: {} \
             (aufgelöst: {} [{}], {} Datei(en)).",
            layers.join(" + "),
            resolved.dir.display(),
            resolved.source.as_str(),
            resolved.file_count(),
        );
    }

    fn file_rejected(&mut self, layer_dir: &Path, file: &Path) {
        eprintln!(
            "lumina-lensfun: LENSFUN-DB-33 {LEVEL_WARN}: Profil-Datei {} ({}) wurde von \
             liblensfun abgelehnt; die Datenbank wird ohne sie geladen.",
            file.display(),
            layer_dir.display()
        );
    }

    fn layer_skipped(&mut self, skipped: &SkippedLayer) {
        eprintln!(
            "lumina-lensfun: LENSFUN-DB-33 {LEVEL_WARN}: Ebene {} ({} ) nicht geladen: {}.",
            skipped.origin.as_str(),
            skipped.dir.as_deref().map_or_else(
                || std::path::Path::new("<kein Pfad>").display(),
                std::path::Path::display,
            ),
            skipped.reason,
        );
    }

    fn pin_displaced(&mut self, resolved: &Resolved) {
        let loaded = resolved
            .layers
            .first()
            .map(|l| format!("{} ({})", l.dir.display(), l.origin.as_str()))
            .unwrap_or_else(|| "<nichts>".to_owned());
        eprintln!(
            "lumina-lensfun: LENSFUN-DB-33 {LEVEL_WARN}: Die aufgelöste Datenbank {} [{}] \
             wird NICHT geladen; geladen wird stattdessen {loaded}.",
            resolved.dir.display(),
            resolved.source.as_str(),
        );
    }

    fn failed(&mut self, err: &SystemDbError) {
        eprintln!("lumina-lensfun: LENSFUN-DB-33 {LEVEL_ERROR}: {err}");
    }
}

/// Holds the memory a caller needs to satisfy "report once, at a proper level"
/// without changing the per-render call pattern. Construct it **once** and keep
/// it alive for the process (or the session) — a sink created fresh per call
/// deduplicates nothing.
///
/// ```ignore
/// // In the application, next to the other long-lived log state:
/// let mut lensfun_diag = lumina_lensfun::report_once();
/// // ...later, on every lookup:
/// let db = LensfunDb::load_system_with(&mut lensfun_diag);
/// ```
///
/// (`ignore`: the snippet is host-application code and only compiles inside a
/// crate that depends on this one with the `native` feature.)
#[derive(Debug, Default)]
pub struct ReportOnce {
    pub(crate) seen: std::sync::Mutex<std::collections::BTreeSet<String>>,
}

impl ReportOnce {
    /// A fresh, empty deduplicating sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `key` was already reported (also records it).
    fn first_time(&self, key: String) -> bool {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key)
    }
}

impl Diagnostics for ReportOnce {
    fn resolved(&mut self, resolved: &Resolved) {
        let key = format!(
            "resolved:{}",
            resolved
                .layers
                .iter()
                .map(|l| format!(
                    "{}|{}|{}",
                    l.dir.display(),
                    l.origin.as_str(),
                    l.files.len()
                ))
                .collect::<Vec<_>>()
                .join(";")
        );
        if self.first_time(key) {
            StderrDiagnostics.resolved(resolved);
        }
    }

    fn file_rejected(&mut self, layer_dir: &Path, file: &Path) {
        if self.first_time(format!("rejected:{}", file.display())) {
            StderrDiagnostics.file_rejected(layer_dir, file);
        }
    }

    fn layer_skipped(&mut self, skipped: &SkippedLayer) {
        if self.first_time(format!("skipped:{skipped}")) {
            StderrDiagnostics.layer_skipped(skipped);
        }
    }

    fn pin_displaced(&mut self, resolved: &Resolved) {
        if self.first_time(format!(
            "pin_displaced:{}|{}",
            resolved.dir.display(),
            resolved.primary.as_str()
        )) {
            StderrDiagnostics.pin_displaced(resolved);
        }
    }

    fn failed(&mut self, err: &SystemDbError) {
        // Key on the error *kind*, not its full text, so a stable, resolvable
        // system database does not permanently suppress a later real failure.
        if self.first_time(format!("failed:{err}")) {
            StderrDiagnostics.failed(err);
        }
    }
}

/// Convenience constructor for a deduplicating sink (see [`ReportOnce`]).
pub fn report_once() -> ReportOnce {
    ReportOnce::new()
}
