//! R3-WARMUP-1 (Release 1.0, User-Entscheidung 2026-09-19): the one-shot
//! cold-start warmup.
//!
//! The R3-SWITCH-1 measurement showed the *first* Library switch at
//! 2886.8 ms (cold) while every later switch was 0.2–24.5 ms: the folder
//! preview index, the grid thumbnails and the first decode/full render were
//! all still pending when the user switched. This module front-loads exactly
//! that work right after app start, on the *existing* background paths:
//!
//! 1. the metadata-only folder preview index ([`crate::thumb_cache`],
//!    memoized per folder),
//! 2. thumbnails for the leading entries through the dedicated
//!    `thumb_worker` pool (bounded — the first grid screen, never the whole
//!    folder),
//! 3. the background decode of the first image (`begin_load_path`, the
//!    PERF-GUI-7 path), and
//! 4. a committed full render of that image through the existing
//!    `finish_decode`/`render_full` debounce path.
//!
//! **No new thread pool, no new architecture, no persistence change.** The
//! warmup only decides *when* the pre-existing work runs. It never writes a
//! recipe or a sidecar.
//!
//! The frame loop drives it while the UI is idle (no pointer down), so the
//! app stays interactive. Progress is visible through the existing status line
//! and overlay toast (`Str` is at its file-size ceiling, so the message is a
//! constant like the R2-JANK-1 F4 pending label — no silent work).
//!
//! Arming happens once in the native entry point
//! ([`LuminaApp::schedule_startup_warmup`] via `apply_startup_config`), which
//! keeps headless/kittest harnesses that build the app directly from picking
//! up the warmup implicitly. The state is one-shot: after the first successful
//! run (or a loud decode failure) it never runs again.

use super::*;
use log::trace;

/// Visible, non-blocking warmup progress line (status + toast). Constant
/// instead of a `Str` variant: `i18n.rs` is at its committed size ceiling
/// (Ratchet), same precedent as the R2-JANK-1 F4 pending label.
pub(crate) const WARMUP_PROGRESS: &str = "Warming up library previews…";

/// How many leading entries the warmup enqueues thumbnails for. Bounded on
/// purpose: the first grid screen, never an unbounded folder walk.
const WARMUP_THUMB_LIMIT: usize = 12;

/// What one warmup run did. Test-visible scheduling facts (never recipe or
/// sidecar state), so a headless test can pin the actual work.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct WarmupReport {
    /// The folder preview index was probed (built once if cold).
    pub(crate) index_built: bool,
    /// Number of thumbnail worker jobs enqueued (cache hits/in-flight = 0).
    pub(crate) thumbs_enqueued: usize,
    /// The warmup started the background decode itself (false when the
    /// startup auto-load had already begun it).
    pub(crate) decode_started: bool,
    /// A committed full render was armed through the debounce path (only when
    /// a source was loaded but not yet rendered).
    pub(crate) render_armed: bool,
}

/// One-shot warmup state. Session-only, never persisted.
#[derive(Default)]
pub(crate) struct WarmupState {
    /// `schedule_startup_warmup` was called (native app start).
    armed: bool,
    /// The warmup ran; never again.
    done: bool,
    report: WarmupReport,
}

impl WarmupState {
    /// Arm the one-shot warmup (idempotent; a completed warmup stays done).
    fn arm(&mut self) {
        if !self.done {
            self.armed = true;
        }
    }

    /// Whether the warmup is armed and still waiting to run.
    fn pending(&self) -> bool {
        self.armed && !self.done
    }
}

impl LuminaApp {
    /// R3-WARMUP-1: arm the one-shot startup warmup. Called once by the native
    /// entry point right after the startup config is applied; headless tests
    /// call it explicitly (harnesses that build the app directly do not, so
    /// their goldens stay free of warmup progress).
    pub fn schedule_startup_warmup(&mut self) {
        self.warmup.arm();
    }

    /// R3-WARMUP-1: run the pending warmup once the app is idle and has
    /// something to warm. Returns `true` when this call performed the warmup.
    ///
    /// Cheap on the UI thread by construction: the folder index is a memoized
    /// metadata probe, thumbnails are enqueued on the worker pool, the decode
    /// runs on its own background thread and the render is only *armed* for the
    /// existing debounce path. No blocking work happens here.
    pub(crate) fn maybe_run_startup_warmup(&mut self, ctx: &egui::Context) -> bool {
        if !self.warmup.pending() {
            return false;
        }
        // Idle only: never compete with an active pointer interaction.
        if ctx.input(|i| i.pointer.any_down()) {
            return false;
        }
        // Wait for the first listing / loaded source: a workdir may be opened
        // after startup, and an in-memory load has no folder to index.
        if self.entries.is_empty() && self.original.is_none() {
            return false;
        }
        if let Some(first) = self.entries.first().cloned() {
            // 1. Folder preview index (metadata-only, memoized, R3-LOG-1 timed).
            let folder = first
                .path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            let name = first.name.clone();
            let _ = self.thumbnail_cache.probe(&folder, &name);
            self.warmup.report.index_built = true;
            // 2. Thumbnails for the leading entries on the existing pool.
            let leading: Vec<FileBrowserEntry> = self
                .entries
                .iter()
                .take(WARMUP_THUMB_LIMIT)
                .cloned()
                .collect();
            let mut enqueued = 0;
            for entry in &leading {
                if self.ensure_thumbnail(ctx, entry) {
                    enqueued += 1;
                }
            }
            self.warmup.report.thumbs_enqueued = enqueued;
            // 3. Background decode of the first image when nothing is loaded or
            //    already decoding (the scan auto-load usually wins this race).
            if self.original.is_none() && self.decode_rx.is_none() && self.path.trim().is_empty() {
                trace!("GUI warmup: starting first-image background decode");
                self.begin_load_path(first.path.display().to_string());
                self.warmup.report.decode_started = true;
            }
        }
        // 4. Full render safety net: a source that is loaded but not yet
        //    rendered is committed through the existing debounce path. The
        //    normal auto-load already renders in `finish_decode`, so this is
        //    never a second render.
        if self.original.is_some() && self.render_key.is_none() && !self.pending_full_render {
            self.pending_full_render = true;
            self.warmup.report.render_armed = true;
        }
        // Visible, non-blocking progress — never a silent state.
        self.status = WARMUP_PROGRESS.to_string();
        let now = ctx.input(|i| i.time);
        self.show_toast(WARMUP_PROGRESS.to_string(), now);
        self.warmup.done = true;
        trace!("GUI warmup: cold-start work scheduled (see report)");
        true
    }

    /// R3-WARMUP-1: whether the one-shot warmup is still armed and unrun.
    #[cfg(test)]
    pub(crate) fn warmup_pending(&self) -> bool {
        self.warmup.pending()
    }

    /// R3-WARMUP-1: the scheduling facts of the completed warmup (all-default
    /// until it ran).
    #[cfg(test)]
    pub(crate) fn warmup_report(&self) -> WarmupReport {
        self.warmup.report
    }
}
