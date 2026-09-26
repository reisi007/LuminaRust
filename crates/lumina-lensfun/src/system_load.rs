//! FFI-backed loading of the resolved Lensfun profile database (LENSFUN-DB-33).
//!
//! Split out of `ffi`/`lib.rs` (file-size ratchet, User-Vorgabe 2026-09-17) and
//! out of [`crate::db_path`], which owns the *pure* resolution and the load plan.
//! This module executes that plan against liblensfun and reports what happened.
//!
//! Contract (SOLL: `feature/platform/capability-matrix.md`, „Lensfun-Profil-
//! Datenbank (plattformabhängige Auflösung, LENSFUN-DB-33)“):
//!
//! - the C library's `lf_db_load()` is deliberately **not** used — its search
//!   path is compiled into the shared library and can be influenced by neither
//!   the operator override nor a test. Instead every layer of the plan is passed
//!   to `lf_db_load_file()` one file at a time, in the deterministic order
//!   [`db_path`](crate::db_path) produced;
//! - **every** event a caller may need to log goes through a caller-supplied
//!   [`Diagnostics`] sink. `lumina-lensfun` has no dependencies and therefore
//!   no logger; emitting to `stderr` itself would be invisible to a
//!   Finder-launched GUI and would repeat on every render (a cache miss logs
//!   nothing, so the lookup runs again). See [`report_once`];
//! - a rejected individual file is a **warning event**, never a silent drop;
//!   only a database where *no* file could be loaded at all is a hard
//!   [`SystemDbError`];
//! - a layer that was considered and left out, and a load plan that does **not**
//!   load the directory the resolution pinned, are both reported — see
//!   [`Diagnostics::layer_skipped`] and [`Diagnostics::pin_displaced`].

use crate::db_error::{MissReason, ProbeMiss, SystemDbError};
use crate::db_layers::SkippedLayer;
use crate::db_path::Resolved;
use crate::ffi::{self, lensfun_global_lock, lf_db_destroy, lf_db_load_file, lf_db_new, Corrector};
use std::path::Path;

/// Where a database load reports what it did.
///
/// Implement it to route into the host's logger at the right level. The five
/// methods map onto the levels a caller needs:
///
/// - [`resolved`](Diagnostics::resolved) — info: which directories were loaded;
/// - [`file_rejected`](Diagnostics::file_rejected) — warn: one profile file was
///   skipped, the database is still usable;
/// - [`layer_skipped`](Diagnostics::layer_skipped) — warn: a database directory
///   that exists is empty, unreadable or undated, so it is not loaded;
/// - [`pin_displaced`](Diagnostics::pin_displaced) — warn: the directory
///   reported by the resolution is **not** the one that is loaded;
/// - [`failed`](Diagnostics::failed) — error: no database at all.
pub trait Diagnostics {
    /// The load plan was executed. Reports every layer and its file count.
    fn resolved(&mut self, resolved: &Resolved);

    /// `file` inside `layer_dir` was rejected by liblensfun and is **not** part
    /// of the loaded database.
    fn file_rejected(&mut self, layer_dir: &Path, file: &Path);

    /// A database directory that exists was left out of the plan.
    ///
    /// Only *actionable* reasons arrive here — an absent directory is the
    /// normal case and is deliberately not pushed (see
    /// [`crate::db_layers::SkipReason::is_actionable`]), though it stays
    /// inspectable in [`Resolved::skipped`].
    fn layer_skipped(&mut self, skipped: &SkippedLayer);

    /// The load plan does **not** load `resolved.dir`.
    ///
    /// `resolved.primary` names what is loaded instead. Without this event a
    /// caller would quote the resolution's directory while a completely
    /// different database is in memory — the silent contradiction
    /// LENSFUN-DB-33 exists to remove. Impossible for an operator override,
    /// which pins the system layer.
    fn pin_displaced(&mut self, resolved: &Resolved);

    /// The load failed; `err` names every probed location.
    fn failed(&mut self, err: &SystemDbError);
}

/// Discards everything. Use when the caller has its own visibility.
///
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
/// [`LensfunDb::load_system`] — the entry point `lumina-gui` and `lumina-cli`
/// still use — constructs a fresh instance of this per call, so there is no
/// de-duplication state to carry. Without the `stderr` line the success path
/// would be completely silent, and a database failure would be a silent,
/// byte-identical no-op. Two remaining limits are stated in
/// `feature/platform/capability-matrix.md` and are **not** solvable from inside
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

/// A [`Diagnostics`] that emits each distinct message **at most once**.
///
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

/// Handle to the system Lensfun database, loaded once.
///
/// The destructor deletes its `lfLens` objects, which decrements lensfun's
/// global regex refcount (and may `regfree` the shared regexes) — it must not
/// race with a concurrent load/search, hence the same global lock.
pub struct LensfunDb {
    pub(crate) db: *mut ffi::lfDatabase,
}

impl Drop for LensfunDb {
    fn drop(&mut self) {
        let _guard = lensfun_global_lock();
        // Safety: `db` came from `lf_db_new()` and is destroyed exactly once.
        unsafe { lf_db_destroy(self.db) }
    }
}

impl LensfunDb {
    /// Load a Lensfun database from one XML file (instead of the system search
    /// paths). Returns `None` if the database cannot be initialised or the file
    /// does not exist or fails to parse.
    ///
    /// Used by the hermetic fixture tests, which must not depend on whatever
    /// the machine has installed. Thread safety: serialized behind
    /// [`lensfun_global_lock`], like [`Self::load_system`].
    pub fn load_file(path: &std::path::Path) -> Option<LensfunDb> {
        // Runs before any C allocation, so `?` cannot leak the handle.
        let c_path = to_c_path(path)?;
        let _guard = lensfun_global_lock();
        unsafe {
            let db = lf_db_new();
            if db.is_null() {
                return None;
            }
            let err = lf_db_load_file(db, c_path.as_ptr());
            if err != 0 {
                lf_db_destroy(db);
                return None;
            }
            Some(LensfunDb { db })
        }
    }

    /// Resolve the **platform-dependent** profile database location, load it,
    /// and report every step to `diag` (LENSFUN-DB-33).
    ///
    /// This is the normative entry point. On failure it returns the named
    /// [`SystemDbError`] *and* calls [`Diagnostics::failed`], so a caller that
    /// only logs the returned value and a caller that logs the event see the
    /// same information. It never degrades to a silent, byte-identical no-op.
    ///
    /// Reporting order: the plan's deviations (`layer_skipped`,
    /// `pin_displaced`) are emitted **before** the load, so a displaced pin is
    /// visible even when the load then fails.
    ///
    /// Thread safety: lensfun 0.3.4's database path is not thread-safe (global
    /// lazy regex compilation, see `LENSFUN_GLOBAL_LOCK`); the wrapper
    /// serializes it, so concurrent calls from several threads are safe.
    pub fn resolve_system_with(diag: &mut impl Diagnostics) -> Result<LensfunDb, SystemDbError> {
        let resolved = match crate::db_path::resolve() {
            Ok(resolved) => resolved,
            Err(err) => {
                diag.failed(&err);
                return Err(err);
            }
        };
        for skipped in resolved.skipped.iter().filter(|s| s.reason.is_actionable()) {
            diag.layer_skipped(skipped);
        }
        if !resolved.pin_honored() {
            diag.pin_displaced(&resolved);
        }
        match Self::load_layers(&resolved, diag) {
            Ok(db) => {
                diag.resolved(&resolved);
                Ok(db)
            }
            Err(err) => {
                diag.failed(&err);
                Err(err)
            }
        }
    }

    /// [`Self::resolve_system_with`] reporting to `stderr`, unlevelled text.
    ///
    /// Prefer [`Self::resolve_system_with`] with a caller-supplied sink: this
    /// default is invisible to a GUI launched from Finder and repeats per
    /// render. See [`StderrDiagnostics`].
    pub fn resolve_system() -> Result<LensfunDb, SystemDbError> {
        Self::resolve_system_with(&mut StderrDiagnostics)
    }

    /// Load the system Lensfun database, reporting to `diag`, or `None` on
    /// failure.
    ///
    /// This is the `Option`-shaped entry point for call sites that cannot
    /// propagate an error (the current `lumina-gui` / `lumina-cli` lookups).
    /// **The caller must supply a long-lived [`report_once`] sink** and treat a
    /// `None` as "lens correction unavailable", not as "no profile matched" —
    /// those are different conditions and only this one is reported.
    pub fn load_system_with(diag: &mut impl Diagnostics) -> Option<LensfunDb> {
        Self::resolve_system_with(diag).ok()
    }

    /// Load the system Lensfun database, reporting unlevelled text to `stderr`.
    ///
    /// Kept so existing call sites keep compiling, and **not** silent: the
    /// success path, every deviation and every failure produce a `stderr` line.
    /// What it cannot do is route into the host's logger, de-duplicate, or be
    /// seen at all by a Dock/Finder-launched app — those are the caller's
    /// obligations. Migrate callers to [`Self::load_system_with`] with a
    /// long-lived [`report_once`] sink. Open obligation, recorded in
    /// `feature/platform/capability-matrix.md`.
    pub fn load_system() -> Option<LensfunDb> {
        Self::load_system_with(&mut StderrDiagnostics)
    }

    /// Executes the whole load plan, one layer at a time, so a partially
    /// readable database still yields the profiles it does have.
    ///
    /// Takes [`lensfun_global_lock`] **itself**: lensfun 0.3.4 compiles/uses
    /// process-global regexes on this path, so a caller that forgets the lock
    /// races on the same `regex_t` — a SIGSEGV under glibc, which is what
    /// [`crate::tests::concurrent_db_load_and_search_is_safe`] guards. Encoding
    /// the lock in the signature (instead of as a documented precondition) is
    /// what makes that unrepresentable; the function is therefore **safe**,
    /// while the raw `lf_*` calls inside it stay in `unsafe` blocks.
    pub(crate) fn load_layers(
        resolved: &Resolved,
        diag: &mut impl Diagnostics,
    ) -> Result<LensfunDb, SystemDbError> {
        let _guard = lensfun_global_lock();
        // Safety: the guard above serializes every caller of liblensfun's global
        // regex state; `db` comes from `lf_db_new()` and is destroyed on exactly
        // one of the two exits below, so no handle leaks and none is freed twice.
        let (db, loaded) = unsafe {
            let db = lf_db_new();
            if db.is_null() {
                return Err(Self::miss(resolved, MissReason::Unreadable));
            }
            let mut loaded = 0usize;
            for layer in &resolved.layers {
                for file in &layer.files {
                    match to_c_path(file) {
                        // Never a silent drop: one corrupt file must still say so.
                        Some(path) if lf_db_load_file(db, path.as_ptr()) == 0 => loaded += 1,
                        _ => diag.file_rejected(&layer.dir, file),
                    }
                }
            }
            (db, loaded)
        };
        if loaded == 0 {
            // Safety: `db` is the live handle from `lf_db_new()` above; the
            // `_guard` at the top of this function is still held, so the
            // destructor cannot race with a concurrent load/search.
            unsafe { lf_db_destroy(db) };
            return Err(Self::miss(resolved, MissReason::AllFilesRejected));
        }
        Ok(LensfunDb { db })
    }

    /// Wrap a rejection of the *resolved* location in the public error.
    ///
    /// The location was found (see [`crate::db_path`]) but its content could not
    /// be loaded, so the reason is about the files, not about a missing directory.
    fn miss(resolved: &Resolved, reason: MissReason) -> SystemDbError {
        SystemDbError::NotFound {
            misses: vec![ProbeMiss {
                dir: resolved.dir.clone(),
                source: resolved.source,
                reason,
            }],
        }
    }

    /// Build a lens corrector for the given camera/lens, or `None` if no
    /// matching, non-identity profile is found.
    ///
    /// `lens_name` is an optional human-readable lens description; when `None`
    /// only the camera is used to pick a lens. `width`/`height` are the image
    /// dimensions the correction is computed for (must be > 0).
    #[allow(clippy::too_many_arguments)]
    pub fn for_camera(
        &self,
        make: &str,
        model: &str,
        lens_name: Option<&str>,
        width: u32,
        height: u32,
        focal_length: f32,
        aperture: f32,
        distance: f32,
    ) -> Option<Corrector> {
        Corrector::for_camera(
            self,
            make,
            model,
            lens_name,
            width,
            height,
            focal_length,
            aperture,
            distance,
        )
    }
}

/// Borrow a path as a NUL-terminated C string without lossy conversion, so a
/// non-UTF-8 file name is rejected rather than silently rewritten.
fn to_c_path(path: &Path) -> Option<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}
