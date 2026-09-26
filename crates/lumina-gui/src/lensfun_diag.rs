//! LENSFUN-CALLER-37 (GUI half): how the Lensfun system-database lookup is
//! reported.
//!
//! # Why the GUI needs its own [`Diagnostics`] impl
//!
//! `lumina-lensfun` is dependency-free by design and ships two sinks:
//! `StderrDiagnostics` and `ReportOnce` (the de-duplicating wrapper the CLI
//! uses). **Neither is usable here**, and the reason is not stylistic:
//!
//! * both write **unlevelled text labels** to `stderr`. A Dock/Finder-launched
//!   macOS app has no `stderr` at all, so "no Lensfun database" would be
//!   *literally invisible* — the failure becomes a silent, pixel-wise no-op.
//!   Routing into the app's own [`log`] facade is the only way the record
//!   reaches the log file, the console and any attached observer.
//! * the level in the text prefix (`INFO`/`WARNUNG`/`FEHLER`) is a **label**,
//!   not a level. Nothing can filter on it, and DoD §4 requires a real level.
//!
//! The sink itself and the de-duplication contract are documented at
//! [`LogDiagnostics`]; the one call site is
//! [`LuminaApp::ensure_lensfun_cache`](super::LuminaApp::ensure_lensfun_cache)
//! in [`super::lensfun_auto`].

use log::{debug, error, warn};
use lumina_lensfun::db_layers::SkippedLayer;
use lumina_lensfun::db_path::{Resolved, SystemDbError};
use lumina_lensfun::system_load::Diagnostics;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// The GUI's Lensfun diagnostics sink: real log levels, de-duplicated per
/// process.
///
/// # Not a "quiet mode"
///
/// Every event that is not de-duplicated is logged. The set only suppresses an
/// **identical** event from repeating, which is what keeps a per-render lookup
/// from producing one `error` line per frame. Nothing is swallowed: a first
/// occurrence of each distinct outcome always reaches the log.
#[derive(Debug, Default)]
pub(crate) struct LogDiagnostics {
    seen: BTreeSet<String>,
}

impl LogDiagnostics {
    /// A fresh, empty sink.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether `key` is new, recording it either way.
    ///
    /// The set is the sink's own state and needs no lock here: the sink is
    /// handed out through [`with_diagnostics`] under a `Mutex`, so every
    /// mutation happens while that guard is held.
    fn first_time(&mut self, key: String) -> bool {
        self.seen.insert(key)
    }
}

impl Diagnostics for LogDiagnostics {
    fn resolved(&mut self, resolved: &Resolved) {
        let layers: Vec<String> = resolved
            .layers
            .iter()
            .map(|layer| format!("{} [{}]", layer.dir.display(), layer.origin.as_str()))
            .collect();
        let key = format!(
            "resolved:{}",
            resolved
                .layers
                .iter()
                .map(|layer| format!(
                    "{}|{}|{}",
                    layer.dir.display(),
                    layer.origin.as_str(),
                    layer.files.len()
                ))
                .collect::<Vec<_>>()
                .join(";")
        );
        if self.first_time(key) {
            debug!(
                "lensfun auto: system profile database loaded: {} (resolved: {} [{}], {} file(s))",
                layers.join(" + "),
                resolved.dir.display(),
                resolved.source.as_str(),
                resolved.file_count(),
            );
        }
    }

    fn file_rejected(&mut self, layer_dir: &Path, file: &Path) {
        if self.first_time(format!("rejected:{}", file.display())) {
            warn!(
                "lensfun auto: {} in {} was rejected by liblensfun and is NOT part of the \
                 loaded database",
                file.display(),
                layer_dir.display(),
            );
        }
    }

    fn layer_skipped(&mut self, skipped: &SkippedLayer) {
        if self.first_time(format!("skipped:{skipped}")) {
            warn!("lensfun auto: database layer skipped: {skipped}");
        }
    }

    fn pin_displaced(&mut self, resolved: &Resolved) {
        if self.first_time(format!(
            "pin_displaced:{}|{}",
            resolved.dir.display(),
            resolved.primary.as_str()
        )) {
            let loaded = resolved
                .layers
                .first()
                .map(|layer| format!("{} ({})", layer.dir.display(), layer.origin.as_str()))
                .unwrap_or_else(|| "<nothing>".to_owned());
            warn!(
                "lensfun auto: the resolved database is NOT the loaded one: resolved {} [{}], \
                 but {loaded} is loaded",
                resolved.dir.display(),
                resolved.source.as_str(),
            );
        }
    }

    fn failed(&mut self, err: &SystemDbError) {
        // Key on the error *text* (its kind plus every probed location), not on
        // "something failed once", so a later real failure is never suppressed.
        if self.first_time(format!("failed:{err}")) {
            error!(
                "lensfun auto: no system Lensfun database ({err}); the EXIF auto-correction \
                 stays off and the manual model applies"
            );
        }
    }
}

/// Run `f` with the process-lifetime Lensfun diagnostics sink.
///
/// The sink is handed to `f` rather than returned: a `MutexGuard` cannot
/// outlive its guard, so a `&'static mut` return type would be a lie.
///
/// # What a caller counts, and where
///
/// This helper owns **where** the sink lives, never **what** a call did. A
/// caller that must record "this really happened" — the G-06 cache counts the
/// database loads it performed in `super::lensfun_auto::LOOKUP_ATTEMPTS` — has
/// to do that counting *inside* `f`, next to the action it counts.
///
/// Doing it out here would be actively wrong: an add after `f` returns also
/// counts an `f` that returned early **without doing the work**, and an early
/// return is exactly what a "have we already asked?" memo looks like. Writing
/// that memo into [`LogDiagnostics`]'s own `seen` set is the natural place for
/// it, and it would then be reported as a lookup that never happened.
pub(crate) fn with_diagnostics<R>(f: impl FnOnce(&mut LogDiagnostics) -> R) -> R {
    static SINK: OnceLock<Mutex<LogDiagnostics>> = OnceLock::new();
    let mut guard = SINK
        .get_or_init(|| Mutex::new(LogDiagnostics::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}
