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
//!
//! # The one value this sink produces *for* the load
//!
//! Everything else a caller observes here is produced **by the caller** or
//! **de-duplicated away**: a caller-side counter can be advanced without the work
//! happening, and a repeated outcome adds no record. The two *outcome* events are
//! the exception — [`Diagnostics::resolved`] and [`Diagnostics::failed`] are the
//! only callbacks `lumina_lensfun` makes once per completed load, and the load
//! makes them itself, after the resolution ran and (on the success branch) after
//! at least one profile file went through `lf_db_load_file`. The sink counts them
//! ([`note_load_outcome`], read by [`lensfun_load_outcomes`]), which is what lets
//! a test tell a performed load apart from an attempt that stopped in front of
//! it — the difference a caller-side counter cannot see.

use log::{debug, error, warn};
use lumina_lensfun::db_layers::SkippedLayer;
use lumina_lensfun::db_path::{Resolved, SystemDbError};
use lumina_lensfun::system_load::Diagnostics;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

thread_local! {
    /// How many database loads reported **their outcome** to the sink on this
    /// thread. Read through [`lensfun_load_outcomes`], which documents why the
    /// count is per thread and what it does and does not bind.
    static LOAD_OUTCOMES: Cell<u64> = const { Cell::new(0) };
}

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
        note_load_outcome();
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
        note_load_outcome();
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

/// Count one **load outcome** reported to the sink on this thread.
///
/// Called from exactly the two outcome methods and from nowhere else. The
/// deviation events must not be counted: `layer_skipped`/`pin_displaced` are
/// emitted **before** the load and `file_rejected` **after** it but still before
/// the outcome, so all three can fire (or not) without a load having produced an
/// outcome — exactly the distinction a caller-side attempt counter loses.
///
/// The increment is deliberately the **first** statement of each method: the
/// de-duplication below may suppress the record, and it must not suppress the
/// fact.
fn note_load_outcome() {
    LOAD_OUTCOMES.with(|loads| loads.set(loads.get() + 1));
}

/// How many database loads this thread's sink has reported the **outcome** of
/// (see [`LOAD_OUTCOMES`], written by [`note_load_outcome`]).
///
/// # What this binds that the attempts counter cannot
///
/// `super::lensfun_auto::LOOKUP_ATTEMPTS` is incremented by the **caller**,
/// immediately beside `load_system_with`, so it proves "the call site was
/// reached" and nothing more. Measured 2026-09-27: a memo that loads once and
/// then returns before `load_system_with` while the `fetch_add` keeps advancing
/// left the whole `lumina-gui` lib suite (903 tests) green.
///
/// This count is produced by the **load** instead.
/// `lumina_lensfun::system_load::report_with` makes exactly one of
/// [`Diagnostics::resolved`] / [`Diagnostics::failed`] per `load_system_with`,
/// and reaches the success branch only after at least one profile file was
/// accepted by `lf_db_load_file`. "One more outcome on this thread" is therefore
/// a fact about the load, and every memo in `lumina-gui` — in
/// `ensure_lensfun_cache` or in [`with_diagnostics`] — skips both methods and
/// freezes it.
///
/// # Why per thread
///
/// The load runs synchronously on the calling thread and the sink is invoked on
/// that same thread, so a thread-local attributes an outcome to the attempt that
/// caused it. A process-wide counter could not do that: `lumina-gui` runs its
/// tests in parallel, so another test's load could advance a shared counter and
/// make a frozen one look alive. Same reason
/// `lumina_lensfun::system_load::load_layers_holds_the_lock` is thread-local.
///
/// # What it still does not cover
///
/// Two memos *below* the sink — inside `lumina_lensfun::system_load::report_with`
/// — stay invisible here, and both were measured rather than assumed:
///
/// * one that **replays the outcome without loading** (calls `resolved` from a
///   cache and returns) keeps the count rising without a single FFI load. The
///   count cannot distinguish it, because from the sink's side the event looks
///   identical;
/// * one that memoises **between the load and `for_camera`** never reaches the
///   outcome stage for later lookups at all, so it needs no replay.
///
/// A memo below the sink that merely *skips* the outcome emission **is** caught:
/// the count stops rising and the binding goes red. That was the first draft of
/// this paragraph and it had it backwards.
///
/// `lumina-lensfun` is outside this file's reach, so both limits are named
/// rather than papered over.
///
/// Only the `failed` branch is pinned **machine-independently**, by
/// `a_reported_miss_advances_the_load_count_and_a_deviation_does_not`; the
/// `resolved` increment is exercised by the production test, which on a host
/// *with* a database takes the `resolved` branch and on a host without one the
/// `failed` branch. Removing the `resolved` increment keeps the
/// machine-independent test green — the binding still holds on each host through
/// whichever branch that host runs, but "both branches, everywhere" would be
/// the wrong claim.
#[cfg(test)]
pub(crate) fn lensfun_load_outcomes() -> u64 {
    LOAD_OUTCOMES.with(Cell::get)
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
///
/// A count that has to survive such a memo cannot be taken by the caller at all;
/// it has to come from **below** the call, from code the load itself runs. That
/// is [`lensfun_load_outcomes`], fed by the sink's own outcome methods.
pub(crate) fn with_diagnostics<R>(f: impl FnOnce(&mut LogDiagnostics) -> R) -> R {
    static SINK: OnceLock<Mutex<LogDiagnostics>> = OnceLock::new();
    let mut guard = SINK
        .get_or_init(|| Mutex::new(LogDiagnostics::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}
