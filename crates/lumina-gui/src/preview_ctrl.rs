//! Native neighbor-preview controller (PREVIEW-CACHE-FEATURE).
//!
//! Orchestrates the hybrid neighbor preview cache defined in
//! `feature/quality/preview-cache.md` on the GUI side:
//!
//! - the active image stays a GPU/CPU texture (out of scope here),
//! - the neighbors in the asymmetric +4/−2 window are rendered/encoded to WebP
//!   **on dedicated background worker threads** (never the UI `IdleQueue`),
//!   stored on disk under `.lumina/previews/` and in the RAM LRU, so the next
//!   image switch has no visible decode/render stall.
//!
//! The heavy lifting (key, window, RAM LRU, WebP encode/decode, disk tier) lives
//! in `lumina_core::preview_cache` and is fully platform-neutral / unit-tested
//! there. This module is the native join point: it owns the worker pool, the
//! priority-ordered dispatch, the per-key in-flight/miss/retry bookkeeping (no
//! silent fallback) and the hand-off of decoded frames to the UI thread.
//!
//! The controller owns background threads and native file IO (capability
//! boundary documented in `feature/platform/capability-matrix.md`).

use lumina_cache::preview_disk::PreviewDiskCache;
#[cfg(test)]
use lumina_core::preview_cache::{decode_webp, encode_webp_lossless};
use lumina_core::preview_cache::{prefetch_window, LruPreviewCache, PreviewKind};
use lumina_core::{DenoisePolicy, ImageFrame};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

#[cfg(test)]
#[path = "preview_ctrl_tests.rs"]
mod preview_ctrl_tests;

/// Maximum neighbor decode/preview attempts before the cell keeps a *visible*
/// failure state instead of retrying forever (mirrors
/// [`crate::filmstrip::THUMBNAIL_MAX_ATTEMPTS`]).
pub const PREVIEW_MAX_ATTEMPTS: u32 = 3;

/// A request to prepare one neighbor's screen/1:1 preview.
///
/// Planning stays lightweight. `probe_id` is a stable source identity (canonical
/// path) used for dedup; the controller captures the exact input identity when
/// enqueueing, and the worker independently returns the identity it prepared.
pub struct PreviewJob {
    /// Stable source identity (canonical absolute path). Never a bare filename.
    pub probe_id: String,
    pub source: PathBuf,
    pub name: String,
    pub virtual_copy: String,
    /// Target resolution (Screen fit: pane size; 1:1: full source).
    pub target: (u32, u32),
    pub kind: PreviewKind,
    /// Priority rank for the worker pool (0 = highest).
    pub priority: u8,
    /// R3-DENOISE-1: the app's denoise fallback policy, propagated from the
    /// active preview so the worker falls back (`Warn`) or aborts (`Strict`)
    /// identically for an active `denoise_ai` stage — never one path silently
    /// failing where the other falls back.
    pub denoise_policy: DenoisePolicy,
}

/// What a neighbor worker produced.
pub enum PreviewOutcome {
    /// A decoded RGBA8 frame (already rendered + downscaled to target).
    Ready(ImageFrame),
    /// The worker could not prepare the preview. The message is shown visibly.
    Failed(String),
}

/// Result travelling back to the UI thread.
pub struct PreviewResult {
    /// The authoritative digest (from the worker's key), or empty on failure.
    pub(crate) digest: String,
    pub(crate) probe_id: String,
    pub(crate) name: String,
    pub(crate) source: PathBuf,
    pub(crate) virtual_copy: String,
    /// Identity captured before the worker was queued.
    pub(crate) enqueued_identity: crate::source_actions::NeighborInputIdentity,
    /// Identity derived from the exact source/sidecar/zdata inputs the worker
    /// used. `None` on failure, where no frame can be inserted.
    pub(crate) prepared_identity: Option<crate::source_actions::NeighborInputIdentity>,
    pub(crate) outcome: PreviewOutcome,
}

struct QueuedPreviewJob {
    job: PreviewJob,
    enqueued_identity: crate::source_actions::NeighborInputIdentity,
}

/// Shared priority-ordered job queue backed by a condvar so workers block when
/// idle and drain by priority (lowest `priority` first) instead of FIFO.
#[derive(Default)]
pub struct PreviewQueue {
    jobs: Mutex<Vec<QueuedPreviewJob>>,
    notify: Condvar,
}

impl PreviewQueue {
    #[cfg(test)]
    fn push(&self, job: PreviewJob) {
        let identity =
            crate::source_actions::NeighborInputIdentity::capture(&job.source, &job.virtual_copy);
        self.push_with_identity(job, identity);
    }

    fn push_with_identity(
        &self,
        job: PreviewJob,
        enqueued_identity: crate::source_actions::NeighborInputIdentity,
    ) {
        self.jobs
            .lock()
            .expect("preview queue poisoned")
            .push(QueuedPreviewJob {
                job,
                enqueued_identity,
            });
        self.notify.notify_one();
    }

    /// Block until a job is available, then return the highest-priority one.
    fn pop(&self) -> QueuedPreviewJob {
        let mut jobs = self.jobs.lock().expect("preview queue poisoned");
        loop {
            if jobs.is_empty() {
                jobs = self.notify.wait(jobs).expect("preview queue poisoned");
                continue;
            }
            let idx = jobs
                .iter()
                .enumerate()
                .min_by_key(|(_, queued)| queued.job.priority)
                .map(|(idx, _)| idx)
                .unwrap();
            return jobs.swap_remove(idx);
        }
    }
}

/// Visibility state of one neighbor's preview (A2: „wird vorbereitet / Veraltet /
/// Fehler" must be visible per cell, never only in logs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewProbeState {
    /// No preview has been requested yet (nothing in flight, nothing cached).
    Miss,
    /// A job is running on a worker („wird vorbereitet").
    Loading,
    /// A valid preview for the *current* key is available in the RAM LRU.
    Ready,
    /// A preview was produced for an earlier key but is no longer available /
    /// the source/recipe changed — it must lazily re-render („Veraltet",
    /// never silently shown).
    Stale,
    /// The retry budget is exhausted; the cell keeps a visible error.
    Failed,
}

/// UI-side controller of the neighbor preview cache.
///
/// Owns the RAM LRU (from `lumina_core`), the per-key in-flight/failure/attempt
/// bookkeeping, and the priority dispatch to the worker pool. The UI thread
/// calls [`Self::enqueue`] when the active image changes and [`Self::poll`]
/// every frame to collect results. There is deliberately **no silent
/// fallback**: a miss stays a visible "wird vorbereitet" state; failures are
/// surfaced via [`Self::drain_failures`] and retried up to
/// [`PREVIEW_MAX_ATTEMPTS`].
///
/// A3: dedup / availability is tracked **by the authoritative key digest**, not
/// by a permanently-"done" probe id. A probe is considered available only while
/// its current digest's frame is still in the RAM LRU. A recipe/source change
/// (which changes the digest) or an eviction therefore makes
/// [`Self::needs_job`] return `true` again — the neighbor is lazily re-rendered
/// instead of being stuck in a permanently-done state. [`Self::invalidate_probe`]
/// forces this for a changed recipe/source on the UI side.
pub struct PreviewController {
    lru: LruPreviewCache,
    queue: Arc<PreviewQueue>,
    result_rx: mpsc::Receiver<PreviewResult>,
    /// probe_id -> attempts so far (also marks in-flight while > 0).
    attempts: BTreeMap<String, u32>,
    failed: BTreeMap<String, String>,
    /// Pending failures not yet drained for logging (keeps `failed` persistent for the visible badge, A2).
    pending_failed: BTreeMap<String, String>,
    in_flight: BTreeMap<String, crate::source_actions::NeighborInputIdentity>,
    /// probe_id -> the last successfully produced digest for that probe. This
    /// is what [`Self::needs_job`] and [`Self::neighbor_preview`] consult; it is
    /// the *availability/veraltung* anchor (A1/A3/A4) — there is no probe-keyed
    /// "permanently done" set.
    probe_digests: BTreeMap<String, String>,
    /// probe_id -> exact source/document/artifact identity captured when the
    /// latest accepted result (success or visible failure) was recorded.
    probe_identities: BTreeMap<String, crate::source_actions::NeighborInputIdentity>,
    /// The preview kind last announced via [`Self::plan_kind`] (A6).
    planned_kind: Option<PreviewKind>,
    active_probe_id: Option<String>,
    /// Directory the cached entries belong to; switching it clears the cache so
    /// previews of a previous folder can neither linger nor grow without bound
    /// (mirrors `ThumbnailManager::ensure_directory`). Relisting the *same*
    /// directory (e.g. a navigation via `open_file`) keeps the warm RAM LRU so
    /// a change-of-active can be served as an instant cache hit (A1).
    directory: Option<String>,
}

impl PreviewController {
    fn with_queue(queue: Arc<PreviewQueue>, result_rx: mpsc::Receiver<PreviewResult>) -> Self {
        Self {
            lru: LruPreviewCache::default(),
            queue,
            result_rx,
            attempts: BTreeMap::new(),
            failed: BTreeMap::new(),
            pending_failed: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            probe_digests: BTreeMap::new(),
            probe_identities: BTreeMap::new(),
            planned_kind: None,
            active_probe_id: None,
            directory: None,
        }
    }

    /// Spawn `pool_size` workers sharing `queue`. Results arrive on
    /// [`Self::poll`].
    pub fn spawn(pool_size: usize) -> (Self, Arc<PreviewQueue>) {
        let queue = Arc::new(PreviewQueue::default());
        let (result_tx, result_rx) = mpsc::channel::<PreviewResult>();
        let queue_worker = Arc::clone(&queue);
        for i in 0..pool_size.max(1) {
            let queue = Arc::clone(&queue_worker);
            let tx = result_tx.clone();
            std::thread::spawn(move || loop {
                let queued = queue.pop();
                log::trace!("preview worker {i}: preparing {}", queued.job.name);
                let probe = queued.job.probe_id.clone();
                let name = queued.job.name.clone();
                let source = queued.job.source.clone();
                let virtual_copy = queued.job.virtual_copy.clone();
                let enqueued_identity = queued.enqueued_identity.clone();
                let result = match crate::preview_jobs::worker_preview_with_identity(
                    queued.job,
                    enqueued_identity.clone(),
                ) {
                    Ok(result) => result,
                    Err(message) => PreviewResult {
                        digest: String::new(),
                        probe_id: probe,
                        name,
                        source,
                        virtual_copy,
                        enqueued_identity,
                        prepared_identity: None,
                        outcome: PreviewOutcome::Failed(message),
                    },
                };
                let _ = tx.send(result);
            });
        }
        (Self::with_queue(Arc::clone(&queue), result_rx), queue)
    }

    #[cfg(test)]
    pub(crate) fn manual() -> (Self, mpsc::Sender<PreviewResult>) {
        let (result_tx, result_rx) = mpsc::channel();
        let controller = Self::with_queue(Arc::new(PreviewQueue::default()), result_rx);
        (controller, result_tx)
    }

    /// The RAM LRU (read access for drawing / diagnostics).
    pub fn lru(&self) -> &LruPreviewCache {
        &self.lru
    }

    /// Whether a job for `probe_id` should be enqueued.
    ///
    /// A job is needed when the probe is **not** in flight, the bounded retry
    /// budget is not exhausted, **and** no preview for its *current* key is
    /// still available in the RAM LRU. Availability is measured by the
    /// authoritative key digest (A3), never by a permanently-"done" probe id:
    /// a recipe/source change (new digest) or an LRU eviction therefore makes a
    /// neighbor eligible again and it is lazily re-rendered.
    pub fn needs_job(&self, probe_id: &str) -> bool {
        if self.in_flight.contains_key(probe_id) {
            return false;
        }
        if self.attempts.get(probe_id).copied().unwrap_or(0) >= PREVIEW_MAX_ATTEMPTS {
            return false;
        }
        match self.probe_digests.get(probe_id) {
            // A current frame is still resident in the RAM LRU → nothing to do.
            Some(digest) if self.lru.contains(digest) => false,
            // No digest yet, or the digest's frame was evicted / the recipe or
            // source changed → prepare (or re-prepare) lazily.
            _ => true,
        }
    }

    /// Whether the current source/document/artifact content differs from the
    /// identity captured for the latest accepted result. Returns `false` when
    /// nothing has been accepted yet.
    #[cfg(test)]
    pub fn probe_is_stale(&self, probe_id: &str, source: &std::path::Path) -> bool {
        self.probe_identity_is_stale(
            probe_id,
            &crate::source_actions::NeighborInputIdentity::capture(source, "vc-original"),
        )
    }

    fn probe_identity_is_stale(
        &self,
        probe_id: &str,
        current: &crate::source_actions::NeighborInputIdentity,
    ) -> bool {
        self.probe_identities
            .get(probe_id)
            .is_some_and(|recorded| recorded != current)
    }

    /// Enqueue one neighbor job. The deterministic input identity is captured
    /// before dispatch and travels with the queued job through its result.
    pub fn enqueue(&mut self, job: PreviewJob) -> bool {
        let probe = job.probe_id.clone();
        let identity =
            crate::source_actions::NeighborInputIdentity::capture(&job.source, &job.virtual_copy);
        if self.probe_identity_is_stale(&probe, &identity)
            || self
                .in_flight
                .get(&probe)
                .is_some_and(|in_flight| in_flight != &identity)
        {
            self.invalidate_probe(&probe);
        }
        if !self.needs_job(&probe) {
            return false;
        }
        self.in_flight.insert(probe, identity.clone());
        self.queue.push_with_identity(job, identity);
        true
    }

    /// Mark `probe_id` as the active image (its RAM entry becomes pinned when a
    /// digest is available). Called when a neighbor becomes the current image.
    pub fn set_active(&mut self, probe_id: &str) {
        self.active_probe_id = Some(probe_id.to_owned());
    }

    /// The currently active probe id, if any (used to skip the neighbor badge on
    /// the active image — it is never displayed via the neighbor cache).
    #[must_use]
    pub fn active_probe_id(&self) -> Option<&str> {
        self.active_probe_id.as_deref()
    }

    /// Drain completed results. A result is accepted only when its enqueue
    /// identity still matches the controller's in-flight token and the exact
    /// source/document/artifact content on disk. Stale pixels and stale
    /// failures are discarded before either can become visible.
    pub fn poll(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            let probe = result.probe_id.clone();
            let Some(expected) = self.in_flight.get(&probe).cloned() else {
                log::debug!("discarding neighbor result for unknown probe {}", probe);
                continue;
            };
            if result.enqueued_identity != expected {
                log::debug!("discarding superseded neighbor result for {}", probe);
                continue;
            }
            let current = crate::source_actions::NeighborInputIdentity::capture(
                &result.source,
                &result.virtual_copy,
            );
            if current != expected {
                log::debug!("discarding stale neighbor result for {}", probe);
                self.invalidate_probe(&probe);
                continue;
            }
            if result
                .prepared_identity
                .as_ref()
                .is_some_and(|prepared| prepared != &current)
            {
                log::debug!("discarding neighbor result with raced inputs for {}", probe);
                self.invalidate_probe(&probe);
                continue;
            }

            self.in_flight.remove(&probe);
            self.probe_identities.insert(probe.clone(), current);
            match result.outcome {
                PreviewOutcome::Ready(frame) => {
                    if !result.digest.is_empty() {
                        log::trace!("neighbor preview ready: {}", result.name);
                        self.lru.insert(result.digest.clone(), frame);
                        self.probe_digests
                            .insert(probe.clone(), result.digest.clone());
                        if self.active_probe_id.as_deref() == Some(probe.as_str()) {
                            self.lru.set_active(&result.digest);
                        }
                    }
                    self.failed.remove(&probe);
                    self.pending_failed.remove(&probe);
                }
                PreviewOutcome::Failed(message) => {
                    let attempts = self.attempts.get(&probe).copied().unwrap_or(0) + 1;
                    self.attempts.insert(probe.clone(), attempts);
                    self.failed.insert(probe.clone(), message.clone());
                    self.pending_failed.insert(probe, message);
                }
            }
        }
    }

    /// Invalidate a probe so it is lazily re-rendered (A3). Called by the UI
    /// when a source or recipe change should make a previously-cached neighbor
    /// preview stale and re-prepare it — the stale frame is dropped and never
    /// shown.
    pub fn invalidate_probe(&mut self, probe_id: &str) {
        if let Some(digest) = self.probe_digests.remove(probe_id) {
            // Drop the stale frame from the RAM LRU (a private copy may still
            // be on disk under the old digest — it is simply not the current
            // key, so it can never be served as a hit).
            self.lru.remove_entry(&digest);
        }
        self.probe_digests.remove(probe_id);
        self.probe_identities.remove(probe_id);
        self.in_flight.remove(probe_id);
        self.attempts.remove(probe_id);
        self.failed.remove(probe_id);
        self.pending_failed.remove(probe_id);
    }

    /// A6: announce the preview kind/resolution the next scheduling round will
    /// use. When it differs from what was planned before (e.g. switching the
    /// zoom mode to 1:1), every previously-cached neighbor is stale for the new
    /// key — all probes are invalidated so they are lazily re-rendered instead
    /// of serving a wrong-resolution frame.
    pub fn plan_kind(&mut self, kind: PreviewKind) {
        if self.planned_kind != Some(kind) {
            let probes: Vec<String> = self.probe_digests.keys().cloned().collect();
            for probe in probes {
                self.invalidate_probe(&probe);
            }
            self.planned_kind = Some(kind);
        }
    }

    /// A1/A4: return the neighbor's preview for the *current* key without
    /// decoding/rendering — first from the RAM LRU (A1: cache-hit without
    /// decode), then from the disk tier (A4, by the previously stored digest).
    /// `None` is a genuine miss: the caller keeps the visible „wird vorbereitet"
    /// state and the ordinary lazy render path applies.
    pub fn neighbor_preview(
        &mut self,
        probe_id: &str,
        source: &std::path::Path,
    ) -> Result<Option<ImageFrame>, String> {
        // Never serve a frame after any source/document/artifact content change.
        // The exact identity is rechecked at the paint boundary, so preserving
        // mtime/length cannot make an old frame look current.
        let current = crate::source_actions::NeighborInputIdentity::capture(source, "vc-original");
        if self.probe_identity_is_stale(probe_id, &current) {
            self.invalidate_probe(probe_id);
            return Ok(None);
        }
        let Some(digest) = self.probe_digests.get(probe_id).cloned() else {
            return Ok(None);
        };
        // A1 — RAM LRU hit (no decode).
        if let Some(frame) = self.lru.get(&digest) {
            return Ok(Some(frame));
        }
        // A4 — disk tier hit (decode of a *cached* WebP, no full source render).
        if let Some(folder) = source.parent() {
            if let Ok(disk) = PreviewDiskCache::in_folder(folder) {
                if let Some(frame) = disk.load(&digest).map_err(|e| e.to_string())? {
                    return Ok(Some(frame));
                }
            }
        }
        Ok(None)
    }

    /// A2: per-cell visibility state of a neighbor's preview („wird
    /// vorbereitet / Veraltet / Fehler" visible, never only logged).
    pub fn probe_state(&self, probe_id: &str) -> PreviewProbeState {
        if let Some(message) = self.failed.get(probe_id) {
            // The retry budget is exhausted; the message is shown visibly.
            let _ = message;
            return PreviewProbeState::Failed;
        }
        if self.in_flight.contains_key(probe_id) {
            return PreviewProbeState::Loading;
        }
        match self.probe_digests.get(probe_id) {
            Some(digest) if self.lru.contains(digest) => PreviewProbeState::Ready,
            Some(_) => PreviewProbeState::Stale,
            None => PreviewProbeState::Miss,
        }
    }

    /// The visible failure message for a probe, if the retry budget is
    /// exhausted (used by the cell UI, A2).
    pub fn failure(&self, probe_id: &str) -> Option<&str> {
        self.failed.get(probe_id).map(String::as_str)
    }

    /// Take all *pending* worker failures since the last call (probe_id,
    /// message) for logging. The visible `Failed` badge stays via `failed` /
    /// `probe_state` until the probe succeeds or is invalidated — draining does
    /// not clear the visible badge (A2), only the pending log queue.
    pub fn drain_failures(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.pending_failed)
            .into_iter()
            .collect()
    }

    /// Discard all state when the browsed directory changes (new source set).
    ///
    /// Relisting the **same** directory (e.g. `open_file` to a neighbor during
    /// navigation) keeps the warm RAM LRU and in-flight/attempt bookkeeping so
    /// a change-of-active can be served as an instant LRU/disk cache hit (A1).
    pub fn ensure_directory(&mut self, directory: &str) {
        if self.directory.as_deref() == Some(directory) {
            return;
        }
        self.lru.clear();
        self.in_flight.clear();
        self.failed.clear();
        self.pending_failed.clear();
        self.attempts.clear();
        self.probe_digests.clear();
        self.probe_identities.clear();
        self.planned_kind = None;
        self.active_probe_id = None;
        self.directory = Some(directory.to_owned());
        // Drain results that belong to the previous folder so no stale frame is
        // applied to the new source set on the next `poll`.
        while self.result_rx.try_recv().is_ok() {}
    }

    /// Unconditional reset (tests / initial construction).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.directory = None;
        self.ensure_directory("");
    }

    /// Test/diagnostic accessors.
    #[cfg(test)]
    pub fn in_flight_probes(&self) -> Vec<String> {
        self.in_flight.keys().cloned().collect()
    }

    /// Probe ids that currently have a *resident* preview (A3: availability is
    /// measured by the key digest, not by a permanently-"done" flag).
    #[cfg(test)]
    pub fn current_probes(&self) -> Vec<String> {
        self.probe_digests
            .iter()
            .filter(|(_, d)| self.lru.contains(d))
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// The latest produced digest for `probe_id` (test/diagnostic).
    #[cfg(test)]
    pub fn probe_digest(&self, probe_id: &str) -> Option<&str> {
        self.probe_digests.get(probe_id).map(String::as_str)
    }
}

/// Compute the ordered neighbor jobs for the asymmetric +4/−2 window.
///
/// Pure helper (testable) — the caller supplies the active index and each
/// entry's (probe_id, source, name). Returns jobs in priority order. The
/// neighbor's recipe is resolved inside the worker (sidecar), never here.
pub fn plan_window_jobs(
    probe_ids: &[String],
    sources: &[PathBuf],
    names: &[String],
    active: usize,
    target: (u32, u32),
    kind: PreviewKind,
    denoise_policy: DenoisePolicy,
) -> Vec<PreviewJob> {
    let window = prefetch_window(active, probe_ids.len());
    window
        .into_iter()
        .map(|slot| PreviewJob {
            probe_id: probe_ids[slot.index].clone(),
            source: sources[slot.index].clone(),
            name: names[slot.index].clone(),
            virtual_copy: "vc-original".to_owned(),
            target,
            kind,
            priority: slot.priority,
            denoise_policy,
        })
        .collect()
}

/// Build an [`ImageFrame`] from pixel data (test helper).
#[cfg(test)]
pub fn make_frame(w: u32, h: u32, value: u8) -> ImageFrame {
    ImageFrame::new(w, h, vec![value; w as usize * h as usize * 4]).unwrap()
}

/// Decode a WebP back into a frame (delegates to core; test helper).
#[cfg(test)]
pub fn decode_webp_pub(bytes: &[u8]) -> Result<ImageFrame, lumina_core::CoreError> {
    decode_webp(bytes)
}

/// Encode a frame to WebP (delegates to core; test helper).
#[cfg(test)]
pub fn encode_webp_pub(frame: &ImageFrame) -> Result<Vec<u8>, lumina_core::CoreError> {
    encode_webp_lossless(frame)
}

#[cfg(test)]
mod tests;
