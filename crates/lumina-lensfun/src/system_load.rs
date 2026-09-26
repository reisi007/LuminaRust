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
pub use crate::db_sinks::{ReportOnce, SilentDiagnostics, StderrDiagnostics};
use crate::ffi::{self, lensfun_global_lock, lf_db_destroy, lf_db_load_file, lf_db_new, Corrector};
use std::path::{Path, PathBuf};

#[cfg(test)]
std::thread_local! {
    static LOAD_LAYERS_LOCK_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Test-only: how deep *this thread* is inside `load_layers`, i.e. whether it
/// currently holds the global lock.
///
/// A caller-supplied [`Diagnostics`] sink must never run under that lock (the
/// mutex is not reentrant, so a re-entrant sink self-deadlocks — finding
/// NIEDRIG-4). Proving that from a test needs an exact signal:
///
/// - `Mutex::try_lock` also fails when an **unrelated parallel test** holds the
///   mutex, so asserting on it is flaky (and did flake here, before this became
///   thread-local);
/// - a process-global counter has the same problem in reverse: other threads'
///   loads would show up as false positives.
///
/// A **thread-local depth** has neither problem: the sink is invoked on the same
/// thread that ran the load, so a non-zero value there is proof and only there.
#[cfg(test)]
pub(crate) fn load_layers_holds_the_lock() -> bool {
    LOAD_LAYERS_LOCK_DEPTH.with(std::cell::Cell::get) > 0
}

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

/// Handle to the system Lensfun database, loaded once.
///
/// The destructor deletes its `lfLens` objects, which decrements lensfun's
/// global regex refcount (and may `regfree` the shared regexes) — it must not
/// race with a concurrent load/search, hence the same global lock.
pub struct LensfunDb {
    pub(crate) db: *mut ffi::lfDatabase,
}

impl std::fmt::Debug for LensfunDb {
    /// Only the pointer identity — `lfDatabase` is an opaque C type and the
    /// loaded profile set is reported through [`Diagnostics::resolved`], not
    /// through this.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LensfunDb").finish_non_exhaustive()
    }
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
    /// # This is a wiring, not a second implementation
    ///
    /// The whole sequence lives in [`report_with`], and *this* function is the
    /// only caller that feeds it the real process environment and the real
    /// filesystem probe. That is deliberate: an earlier revision kept a
    /// byte-identical copy of the emission order here while the tests drove
    /// `report_with`, so deleting `diag.pin_displaced(&resolved)` or the whole
    /// `layer_skipped` loop from *this* function left every test green
    /// (finding MITTEL-3, round 4). The tests now cover the code production
    /// runs, and the proof is that a mutation here goes red — see
    /// `tests::production_seam`.
    ///
    /// Thread safety: lensfun 0.3.4's database path is not thread-safe (global
    /// lazy regex compilation, see `LENSFUN_GLOBAL_LOCK`); the wrapper
    /// serializes it, so concurrent calls from several threads are safe.
    pub fn resolve_system_with(diag: &mut impl Diagnostics) -> Result<LensfunDb, SystemDbError> {
        report_with(
            &crate::db_path::EnvValues::from_process(),
            &crate::db_path::FsProbe,
            diag,
        )
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
    /// long-lived [`report_once`] sink.
    ///
    /// **Closed 2026-09-26 (`LENSFUN-CALLER-37`):** both product callers were
    /// migrated — `lumina-gui` with its own `LogDiagnostics` on the app's `log`
    /// facade, `lumina-cli` with a process-lifetime `ReportOnce`. This function
    /// now has **no** product caller and survives only as a convenience for
    /// tests and external users. Recorded in
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
    /// what makes forgetting it unrepresentable; the function is therefore
    /// **safe**, while the raw `lf_*` calls inside it stay in `unsafe` blocks.
    ///
    /// # The sink is never called while the lock is held (NIEDRIG-4)
    ///
    /// `LENSFUN_GLOBAL_LOCK` is a plain `std::sync::Mutex` and therefore **not
    /// reentrant**. A caller-supplied [`Diagnostics`] impl is arbitrary
    /// host-tenant code: routing it into the host's logger is harmless, but a
    /// sink that re-enters this crate — to look up another profile, to log
    /// through a helper that touches the database — would self-deadlock. So the
    /// rejections are **collected** during the load and handed to `diag` only
    /// after the guard is dropped. That is why `file_rejected` can be delayed
    /// relative to the load it describes, and why the order guarantee is
    /// "all rejections, then the outcome", not "rejection immediately after the
    /// failing call".
    pub(crate) fn load_layers(
        resolved: &Resolved,
        diag: &mut impl Diagnostics,
    ) -> Result<LensfunDb, SystemDbError> {
        // `Vec`, not a `BTreeSet`: duplicate names are legitimate (two layers
        // may hold same-named files) and must be reported as often as they
        // happen.
        let mut rejected: Vec<(PathBuf, PathBuf)> = Vec::new();
        let outcome = {
            #[cfg(test)]
            LOAD_LAYERS_LOCK_DEPTH.with(|d| d.set(d.get() + 1));
            let _guard = lensfun_global_lock();
            // Safety: the guard above serializes every caller of liblensfun's
            // global regex state; `db` comes from `lf_db_new()` and is destroyed
            // on exactly one of the two exits below, so no handle leaks and none
            // is freed twice.
            let (db, loaded) = unsafe {
                let db = lf_db_new();
                if db.is_null() {
                    return Err(Self::miss(resolved, MissReason::Unreadable));
                }
                let mut loaded = 0usize;
                for layer in &resolved.layers {
                    for file in &layer.files {
                        match to_c_path(file) {
                            // Never a silent drop: one corrupt file must still
                            // say so — recorded now, reported after the lock.
                            Some(path) if lf_db_load_file(db, path.as_ptr()) == 0 => loaded += 1,
                            _ => rejected.push((layer.dir.clone(), file.clone())),
                        }
                    }
                }
                (db, loaded)
            };
            if loaded == 0 {
                // Safety: `db` is the live handle from `lf_db_new()` above; the
                // guard is still held here, so the destructor cannot race with a
                // concurrent load/search.
                unsafe { lf_db_destroy(db) };
                Err(Self::miss(resolved, MissReason::AllFilesRejected))
            } else {
                Ok(LensfunDb { db })
            }
        }; // <- the guard is dropped here, before any sink call
        #[cfg(test)]
        LOAD_LAYERS_LOCK_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));

        for (layer_dir, file) in &rejected {
            diag.file_rejected(layer_dir, file);
        }
        outcome
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

/// The load sequence, with the environment values and the directory probe
/// injected. **This is the single implementation** of the documented emission
/// order; [`LensfunDb::resolve_system_with`] is the caller that feeds it the
/// real process environment and the real [`FsProbe`](crate::db_path::FsProbe).
///
/// # Why the values are parameters
///
/// The only alternative to injecting them is `std::env::set_var`, which is
/// undefined behaviour next to a concurrent `getenv` in any other thread — and
/// no test-local lock can make that safe. Injection is therefore not a test-only
/// convenience: it is what lets the *production* code be the tested code.
///
/// # The documented emission order
///
/// The normative list is in `feature/platform/capability-matrix.md`; in short:
/// resolution failure → `failed` only; then every *actionable* skipped layer; then
/// `pin_displaced` if the plan does not load the resolved directory; then the
/// load (whose `file_rejected` events come after the lock is released — see
/// [`LensfunDb::load_layers`]); then exactly one outcome event. Steps 2 and 3
/// deliberately precede the load, so a displaced pin stays visible even when the
/// load then fails. Pinned by
/// `tests::production_seam::production_emits_pin_displaced_and_layer_skipped_before_the_load`.
pub fn report_with(
    env: &crate::db_path::EnvValues,
    probe: &impl crate::db_path::Probe,
    diag: &mut impl Diagnostics,
) -> Result<LensfunDb, SystemDbError> {
    let resolved = match env.resolve(probe) {
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
    match LensfunDb::load_layers(&resolved, diag) {
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

/// Borrow a path as a NUL-terminated C string without lossy conversion, so a
/// non-UTF-8 file name is rejected rather than silently rewritten.
fn to_c_path(path: &Path) -> Option<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}
