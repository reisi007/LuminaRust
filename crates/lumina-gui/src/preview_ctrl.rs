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
//! Native only: wasm32 has no background threads or native file IO (documented
//! capability boundary in `feature/platform/capability-matrix.md`).

#[cfg(test)]
use lumina_core::preview_cache::decode_webp;
use lumina_core::preview_cache::{
    encode_webp_lossless, prefetch_window, LruPreviewCache, PreviewDiskCache, PreviewKey,
    PreviewKind,
};
use lumina_core::{render_frame, ImageFrame, RenderContext};
use lumina_sidecar::EditRecipe;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

/// Maximum neighbor decode/preview attempts before the cell keeps a *visible*
/// failure state instead of retrying forever (mirrors
/// [`crate::filmstrip::THUMBNAIL_MAX_ATTEMPTS`]).
pub const PREVIEW_MAX_ATTEMPTS: u32 = 3;

/// A request to prepare one neighbor's screen/1:1 preview.
///
/// The field set is deliberately lightweight so the main thread enqueues jobs
/// without any file I/O: `probe_id` is a stable source identity (canonical path)
/// used for dedup/in-flight tracking; the **worker** computes the authoritative
/// [`PreviewKey`] + digest by reading the source, so a changed source/render is
/// reflected in a new digest (stale detection) without a main-thread read.
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
}

/// Cheap staleness fingerprint of a source + its sidecar, captured by the
/// worker when it prepares a preview and re-validated on the UI thread at
/// enqueue / neighbor-preview time (A3).
///
/// This follows the pipeline's fast-fingerprint rule: the cheap mtime/len pair
/// is the UI-side staleness gate; the **authoritative** validation for every
/// disk/RAM hit remains the full content/recipe key computed by the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreviewStamp {
    pub source_mtime: (i64, u32),
    pub source_len: u64,
    pub sidecar_mtime: (i64, u32),
}

/// Capture the cheap fingerprint of `path` (mtime seconds/nanos + — for the
/// source — byte length). A missing/unreadable file yields the default stamp,
/// which never equals a previously recorded live stamp, so the entry staleness
/// check re-renders instead of serving a frame of a vanished source.
fn file_stamp(path: &std::path::Path, with_len: bool) -> ((i64, u32), u64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return ((-1, 0), 0);
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs() as i64, d.subsec_nanos()))
        .unwrap_or((-1, 0));
    (mtime, if with_len { meta.len() } else { 0 })
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
    pub digest: String,
    pub probe_id: String,
    pub name: String,
    /// Cheap source/sidecar fingerprint at prepare time (A3 staleness gate).
    pub stamp: PreviewStamp,
    pub outcome: PreviewOutcome,
}

/// Decode + render + downscale + WebP-encode a neighbor on the background
/// worker. Returns the decoded frame so the UI thread can use it immediately,
/// and stores the encoded WebP to the source's own `.lumina/previews` tier.
fn worker_preview(job: PreviewJob) -> Result<PreviewResult, String> {
    let bytes = std::fs::read(&job.source).map_err(|e| format!("{}: {e}", job.source.display()))?;
    let decoded = if crate::is_raw_name(&job.name) {
        lumina_raw::decode_bytes(&bytes, &job.name)
            .map_err(|e| e.to_string())?
            .frame
    } else {
        ImageFrame::decode(&bytes).map_err(|e| e.to_string())?
    };

    // Build the render input: 1:1 previews keep the full decoded frame (no
    // downscaling), Screen previews are reduced to the target long edge.
    let frame = if job.kind == PreviewKind::OneToOne {
        decoded
    } else {
        downscale_to_target(&decoded, job.target)
    };

    // Render with the neighbor's own recipe (its sidecar, if any): the worker
    // — not the UI thread — reads the sidecar, keeping the main thread free of
    // per-neighbor file I/O on navigation.
    //
    // B7: a render failure must never be silently replaced by the un-rendered
    // base frame — that would show a wrong (recipe-less) neighbor preview with
    // no visible indication. Any error propagates up as a `Failed` outcome so
    // the cell keeps a visible error state (no silent fallback, Agents.md).
    let recipe = load_neighbor_recipe(&job.source, &job.virtual_copy);
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
    };
    let rendered = render_frame(&frame, &context)
        .map(|o| o.frame)
        .map_err(|e| format!("render {}: {e}", job.name))?;

    // Build the authoritative key from the source content hash + recipe.
    let content_hash = blake3::hash(&bytes).to_hex().to_string();
    let key = PreviewKey {
        source_content_hash: content_hash,
        decode_context: "decode-v1".to_owned(),
        pipeline_version: env!("CARGO_PKG_VERSION").to_owned(),
        virtual_copy_id: job.virtual_copy.clone(),
        render_key: render_key_of(&recipe, (rendered.width, rendered.height)),
        kind: job.kind,
        width: rendered.width,
        height: rendered.height,
        encode: Default::default(),
    };
    let digest = key.digest();

    let webp = encode_webp_lossless(&rendered).map_err(|e| e.to_string())?;
    // Disk tier is rooted at the *source's own* folder (`.lumina/previews`),
    // like `DiskFolderCache` — a whole sidecar bundle moves together. The write
    // happens on the worker, never the UI thread.
    if let Some(folder) = job.source.parent() {
        if let Ok(disk) = PreviewDiskCache::in_folder(folder) {
            if let Err(e) = disk.store(&digest, &webp) {
                // Disk write failure is only diagnosed, not fatal — the RAM LRU
                // still serves the hit this session.
                log::warn!("preview disk store failed for {}: {e}", job.name);
            }
        }
    }

    // A3: cheap source+sidecar fingerprint at prepare time — the UI-side
    // staleness gate on later navigation (an mtime/len change → re-render).
    let (src_mtime, src_len) = file_stamp(&job.source, true);
    let (side_mtime, _) = file_stamp(&lumina_sidecar::sidecar_path_for(&job.source), false);
    Ok(PreviewResult {
        digest,
        probe_id: job.probe_id.clone(),
        name: job.name,
        stamp: PreviewStamp {
            source_mtime: src_mtime,
            source_len: src_len,
            sidecar_mtime: side_mtime,
        },
        outcome: PreviewOutcome::Ready(rendered),
    })
}

/// Downscale `frame` so it fits the target long edge (returns unchanged when
/// already within).
fn downscale_to_target(frame: &ImageFrame, target: (u32, u32)) -> ImageFrame {
    let long = frame.width.max(frame.height);
    let max_edge = target.0.max(target.1);
    if max_edge == 0 || long <= max_edge {
        return frame.clone();
    }
    frame.downscale(max_edge)
}

/// Deterministic render digest for the neighbor recipe (content + target). Used
/// as the render-key component of the [`PreviewKey`]; a recipe change therefore
/// produces a new key → the cached entry is stale.
fn render_key_of(recipe: &EditRecipe, target: (u32, u32)) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"preview-render");
    hasher.update(&target.0.to_le_bytes());
    hasher.update(&target.1.to_le_bytes());
    if let Ok(bytes) = serde_json::to_vec(recipe) {
        hasher.update(&bytes);
    }
    hasher.finalize().to_hex().to_string()
}

/// Load the recipe of a neighbor's virtual copy from its sidecar (worker side).
/// A missing sidecar or virtual copy yields the default recipe — the neighbor
/// preview then reflects the develop state exactly like a fresh source.
fn load_neighbor_recipe(source: &std::path::Path, virtual_copy: &str) -> EditRecipe {
    let sidecar = lumina_sidecar::sidecar_path_for(source);
    match lumina_sidecar::load_sidecar(&sidecar) {
        Ok(document) => document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == virtual_copy)
            .map(|copy| copy.recipe.clone())
            .unwrap_or_default(),
        Err(_) => EditRecipe::default(),
    }
}

/// Shared priority-ordered job queue backed by a condvar so workers block when
/// idle and drain by priority (lowest `priority` first) instead of FIFO.
#[derive(Default)]
pub struct PreviewQueue {
    jobs: Mutex<Vec<PreviewJob>>,
    notify: Condvar,
}

impl PreviewQueue {
    pub fn push(&self, job: PreviewJob) {
        self.jobs.lock().expect("preview queue poisoned").push(job);
        self.notify.notify_one();
    }

    /// Block until a job is available, then return the highest-priority one.
    fn pop(&self) -> PreviewJob {
        let mut jobs = self.jobs.lock().expect("preview queue poisoned");
        loop {
            if jobs.is_empty() {
                jobs = self.notify.wait(jobs).expect("preview queue poisoned");
                continue;
            }
            let idx = jobs
                .iter()
                .enumerate()
                .min_by_key(|(_, job)| job.priority)
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
    in_flight: BTreeMap<String, ()>,
    /// probe_id -> the last successfully produced digest for that probe. This
    /// is what [`Self::needs_job`] and [`Self::neighbor_preview`] consult; it is
    /// the *availability/veraltung* anchor (A1/A3/A4) — there is no probe-keyed
    /// "permanently done" set.
    probe_digests: BTreeMap<String, String>,
    /// probe_id -> the source/sidecar fingerprint captured when the digest above
    /// was produced. The UI thread compares it cheaply (mtime/len) before
    /// serving an entry (A3: source/recipe change → stale → re-render).
    probe_stamps: BTreeMap<String, PreviewStamp>,
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
                let job = queue.pop();
                log::trace!("preview worker {i}: preparing {}", job.name);
                let probe = job.probe_id.clone();
                let result = match worker_preview(job) {
                    Ok(result) => result,
                    Err(message) => PreviewResult {
                        digest: String::new(),
                        probe_id: probe,
                        name: String::new(),
                        stamp: PreviewStamp::default(),
                        outcome: PreviewOutcome::Failed(message),
                    },
                };
                let _ = tx.send(result);
            });
        }
        let controller = Self {
            lru: LruPreviewCache::default(),
            queue: Arc::clone(&queue),
            result_rx,
            attempts: BTreeMap::new(),
            failed: BTreeMap::new(),
            pending_failed: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            probe_digests: BTreeMap::new(),
            probe_stamps: BTreeMap::new(),
            planned_kind: None,
            active_probe_id: None,
            directory: None,
        };
        (controller, queue)
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

    /// Whether the current source/sidecar fingerprint differs from the one
    /// captured when the probe's preview was prepared (A3 staleness gate). A
    /// changed source or recipe must re-render, never silently serve the old
    /// frame. Returns `false` when nothing was prepared yet.
    pub fn probe_is_stale(&self, probe_id: &str, source: &std::path::Path) -> bool {
        let Some(recorded) = self.probe_stamps.get(probe_id) else {
            return false;
        };
        let (src_mtime, src_len) = file_stamp(source, true);
        let (side_mtime, _) = file_stamp(&lumina_sidecar::sidecar_path_for(source), false);
        src_mtime != recorded.source_mtime
            || src_len != recorded.source_len
            || side_mtime != recorded.sidecar_mtime
    }

    /// Enqueue one neighbor job (see [`plan_window_jobs`]). Returns `false`
    /// when it was skipped (in flight / retries exhausted / current frame
    /// already available).
    ///
    /// A3: if the source or its sidecar changed since the probe's preview was
    /// prepared, the old entry is invalidated first so a re-render happens
    /// instead of dedup-ing against a stale digest.
    pub fn enqueue(&mut self, job: PreviewJob) -> bool {
        let probe = job.probe_id.clone();
        if self.probe_is_stale(&probe, &job.source) {
            self.invalidate_probe(&probe);
        }
        if !self.needs_job(&probe) {
            return false;
        }
        self.in_flight.insert(probe.clone(), ());
        self.queue.push(job);
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

    /// Drain completed results. On `Ready`, the decoded frame is inserted into
    /// the RAM LRU under the authoritative digest and the probe's current
    /// digest is recorded. A changed source/recipe lands under a *new* digest —
    /// the old entry stays stale (never silently shown) and the probe becomes
    /// eligible for a lazy re-render via [`Self::needs_job`] (A3).
    pub fn poll(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            let probe = result.probe_id.clone();
            match result.outcome {
                PreviewOutcome::Ready(frame) => {
                    if !result.digest.is_empty() {
                        log::trace!("neighbor preview ready: {}", result.name);
                        self.lru.insert(result.digest.clone(), frame);
                        // The probe's latest authoritative digest (used by
                        // `needs_job`, `neighbor_preview` and `probe_state`).
                        self.probe_digests
                            .insert(probe.clone(), result.digest.clone());
                        // Cheap source/sidecar fingerprint for the A3 staleness
                        // gate on later navigation.
                        self.probe_stamps.insert(probe.clone(), result.stamp);
                        // Promotion: when the *active* image's own preview lands,
                        // pin its RAM entry so it can never be evicted (SOLL:
                        // „das aktive Bild wird nie evictet").
                        if self.active_probe_id.as_deref() == Some(probe.as_str()) {
                            self.lru.set_active(&result.digest);
                        }
                    }
                    self.in_flight.remove(&probe);
                    self.failed.remove(&probe);
                    self.pending_failed.remove(&probe);
                }
                PreviewOutcome::Failed(message) => {
                    log::warn!("neighbor preview failed for {}: {message}", result.name);
                    self.in_flight.remove(&probe);
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
        self.probe_stamps.remove(probe_id);
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn neighbor_preview(
        &mut self,
        probe_id: &str,
        source: &std::path::Path,
    ) -> Result<Option<ImageFrame>, String> {
        // A3: never serve a frame whose source/sidecar changed since prepare —
        // that would be a silent stale display. The stale entry is invalidated
        // and reported as a miss so the lazy re-render path applies.
        if self.probe_is_stale(probe_id, source) {
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
        self.probe_stamps.clear();
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
mod tests {
    use super::*;

    fn probe(len: usize) -> Vec<String> {
        (0..len).map(|i| format!("probe-{i}")).collect()
    }

    #[test]
    fn plan_window_honours_priority_and_edges() {
        let sources: Vec<PathBuf> = (0..8).map(|i| PathBuf::from(format!("p{i}"))).collect();
        let names: Vec<String> = (0..8).map(|i| format!("p{i}.cr3")).collect();
        let jobs = plan_window_jobs(
            &probe(8),
            &sources,
            &names,
            3,
            (800, 600),
            PreviewKind::Screen,
        );
        let priorities: Vec<u8> = jobs.iter().map(|j| j.priority).collect();
        assert_eq!(priorities, vec![0, 1, 2, 3, 4, 5]);
        let probes: Vec<&str> = jobs.iter().map(|j| j.probe_id.as_str()).collect();
        assert_eq!(
            probes,
            vec!["probe-4", "probe-5", "probe-2", "probe-6", "probe-1", "probe-7"]
        );

        // No wrap at the start.
        let jobs = plan_window_jobs(
            &probe(8),
            &sources,
            &names,
            0,
            (800, 600),
            PreviewKind::Screen,
        );
        assert_eq!(jobs.len(), 4);
    }

    #[test]
    fn needs_job_dedups_in_flight_and_retries_are_bounded() {
        // Build a controller without threads to exercise bookkeeping only.
        let ctrl = PreviewController {
            lru: LruPreviewCache::default(),
            queue: Arc::new(PreviewQueue::default()),
            result_rx: mpsc::channel::<PreviewResult>().1,
            attempts: BTreeMap::new(),
            failed: BTreeMap::new(),
            pending_failed: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            probe_digests: BTreeMap::new(),
            planned_kind: None,
            active_probe_id: None,
            probe_stamps: BTreeMap::new(),
            directory: None,
        };
        let mut ctrl = ctrl;
        // Fake an in-flight entry.
        ctrl.in_flight.insert("k".into(), ());
        assert!(!ctrl.needs_job("k"));
        ctrl.in_flight.remove("k");
        assert!(ctrl.needs_job("k"));
        // A resident current frame means no job is needed.
        ctrl.probe_digests.insert("k".into(), "d-k".into());
        ctrl.lru.insert("d-k", make_frame(1, 1, 1));
        assert!(!ctrl.needs_job("k"), "resident current frame → no job");
        // Eviction makes the (otherwise "done") probe eligible again (A3).
        ctrl.lru.remove_entry("d-k");
        assert!(
            ctrl.needs_job("k"),
            "evicted probe must be re-planned, not permanently done (A3)"
        );
        // Recipe/source change (invalidate) also re-plans (A3).
        ctrl.lru.insert("d-k", make_frame(1, 1, 2));
        ctrl.invalidate_probe("k");
        assert!(ctrl.needs_job("k"), "invalidated probe re-renders (A3)");
        // Exhaust retries → no more jobs.
        for i in 0..PREVIEW_MAX_ATTEMPTS {
            ctrl.attempts.insert("k".into(), i + 1);
        }
        assert!(!ctrl.needs_job("k"));

        // Drain failures surfaces the pending log message (no silent fallback) while the visible badge stays.
        ctrl.failed.insert("k".into(), "boom".into());
        ctrl.pending_failed.insert("k".into(), "boom".into());
        assert_eq!(
            ctrl.drain_failures(),
            vec![("k".to_owned(), "boom".to_owned())]
        );
        assert!(
            ctrl.drain_failures().is_empty(),
            "pending drain is consuming"
        );
        assert_eq!(
            ctrl.failure("k"),
            Some("boom"),
            "visible badge persists after log drain (A2)"
        );
    }

    /// A *miss* must be visible as a "needs job / wird vorbereitet" state and
    /// never as a silently wrong image: before any work, the controller reports
    /// `needs_job` and the RAM LRU holds nothing for the probe.
    #[test]
    fn miss_is_visible_not_silent() {
        // A miss must be visible as a "needs job / wird vorbereitet" state and
        // never as a silently wrong image. Constructed directly (no threads).
        let mut ctrl = PreviewController {
            lru: LruPreviewCache::default(),
            queue: Arc::new(PreviewQueue::default()),
            result_rx: mpsc::channel::<PreviewResult>().1,
            attempts: BTreeMap::new(),
            failed: BTreeMap::new(),
            pending_failed: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            probe_digests: BTreeMap::new(),
            planned_kind: None,
            active_probe_id: None,
            probe_stamps: BTreeMap::new(),
            directory: None,
        };
        assert!(ctrl.lru().is_empty());
        assert!(ctrl.needs_job("probe-0"));
        assert!(!ctrl.lru().contains("probe-0"));
        // Enqueueing marks it in flight → the caller knows to show "wird
        // vorbereitet" instead of a stale/wrong frame.
        assert!(ctrl.enqueue(PreviewJob {
            probe_id: "probe-0".into(),
            source: PathBuf::from("/nonexistent/for-unit-test.png"),
            name: "for-unit-test.png".into(),
            virtual_copy: "vc-original".into(),
            target: (64, 64),
            kind: PreviewKind::Screen,
            priority: 0,
        }));
        assert!(
            !ctrl.needs_job("probe-0"),
            "in flight: preparing state shown"
        );
        ctrl.set_active("probe-0");
        assert_eq!(ctrl.active_probe_id(), Some("probe-0"));
    }

    #[test]
    fn webp_roundtrip_via_helpers() {
        let f = make_frame(3, 3, 42);
        let webp = encode_webp_pub(&f).unwrap();
        let back = decode_webp_pub(&webp).unwrap();
        assert_eq!(back.pixels, f.pixels);
    }

    #[test]
    fn lru_holds_seven_and_pins_active() {
        let mut lru = LruPreviewCache::default();
        lru.insert("active", make_frame(1, 1, 1));
        lru.set_active("active");
        for i in 0..8 {
            lru.insert(format!("n{i}"), make_frame(1, 1, i as u8));
        }
        assert!(lru.contains("active"), "pinned active must survive");
        assert_eq!(lru.len(), 7, "max 7 slots (active + 6)");
    }

    /// End-to-end worker test: a real source on disk is decoded + rendered +
    /// WebP-encoded by a background worker thread, delivered back via `poll`
    /// into the RAM LRU, and persisted to the source's `.lumina/previews`
    /// disk tier — without any UI-thread decode.
    #[test]
    fn worker_roundtrip_decodes_renders_and_stores_disk_tier() {
        use lumina_core::ImageFileFormat;
        use std::time::{Duration, Instant};

        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("neighbor.png");
        // A 8x8 RGBA8 frame with a distinct alpha sample → PNG source on disk.
        let png = make_frame(8, 8, 77).encode(ImageFileFormat::Png).unwrap();
        std::fs::write(&source, &png).unwrap();

        let (mut ctrl, _queue) = PreviewController::spawn(1);
        ctrl.enqueue(PreviewJob {
            probe_id: "neighbor".into(),
            source: source.clone(),
            name: "neighbor.png".into(),
            virtual_copy: "vc-original".into(),
            target: (8, 8),
            kind: PreviewKind::Screen,
            priority: 0,
        });

        // Wait (bounded) for the worker result to land in the RAM LRU.
        let deadline = Instant::now() + Duration::from_secs(10);
        while ctrl.lru().is_empty() && Instant::now() < deadline {
            ctrl.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        ctrl.poll();
        assert!(
            !ctrl.lru().is_empty(),
            "worker must deliver the rendered neighbor into the RAM LRU"
        );
        assert!(
            !ctrl.needs_job("neighbor"),
            "a finished job must not be re-enqueued unconditionally"
        );
        assert!(
            ctrl.current_probes().contains(&"neighbor".to_owned()),
            "a successful prepare leaves a resident current preview (A3)"
        );
        assert_eq!(
            ctrl.probe_state("neighbor"),
            PreviewProbeState::Ready,
            "ready preview is visible per cell (A2)"
        );

        // A1: a navigation to this neighbor now hits the RAM LRU (no decode).
        let hit = ctrl
            .neighbor_preview("neighbor", &source)
            .expect("RAM lookup succeeds")
            .expect("neighbor preview is resident after prep");
        assert_eq!((hit.width, hit.height), (8, 8), "RAM cache-hit (A1)");

        // A4: evicting the RAM entry still serves the neighbor from the disk
        // tier (the worker persisted the WebP) — no full source re-render.
        let digest = ctrl.probe_digest("neighbor").unwrap().to_owned();
        ctrl.lru.remove_entry(&digest);
        assert!(
            ctrl.neighbor_preview("neighbor", &source)
                .expect("disk lookup succeeds")
                .is_some(),
            "evicted-but-on-disk neighbor still hits (A4)"
        );
        assert_eq!(
            ctrl.probe_state("neighbor"),
            PreviewProbeState::Stale,
            "evicted (RAM) but on-disk preview is visible as stale/veraltet (A2)"
        );

        // Disk tier: `.lumina/previews/*.preview.webp` next to the source.
        let previews = dir.path().join(".lumina").join("previews");
        assert!(previews.is_dir(), "disk tier must be created by the worker");
        let stored: Vec<_> = std::fs::read_dir(&previews)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(stored.len(), 1, "exactly one preview file expected");
        let webp = std::fs::read(&stored[0]).unwrap();
        let back = decode_webp_pub(&webp).unwrap();
        assert_eq!(
            (back.width, back.height),
            (8, 8),
            "stored WebP keeps the downscaled geometry"
        );
    }

    /// A3: a source/recipe change must make a previously-cached neighbor stale
    /// and trigger a lazy re-render — never silently show the old frame and
    /// never stay blocked by a permanently-"done" probe.
    #[test]
    fn invalidate_probe_marks_stale_and_triggers_reread() {
        let mut ctrl = PreviewController {
            lru: LruPreviewCache::default(),
            queue: Arc::new(PreviewQueue::default()),
            result_rx: mpsc::channel::<PreviewResult>().1,
            attempts: BTreeMap::new(),
            failed: BTreeMap::new(),
            pending_failed: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            probe_digests: BTreeMap::new(),
            planned_kind: None,
            active_probe_id: None,
            probe_stamps: BTreeMap::new(),
            directory: None,
        };
        // Simulate a successfully prepared neighbor under digest "d1".
        ctrl.lru.insert("d1", make_frame(2, 2, 1));
        ctrl.probe_digests.insert("p".into(), "d1".into());
        assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Ready);
        assert!(!ctrl.needs_job("p"));

        // The recipe/source changed → the UI calls `invalidate_probe`.
        ctrl.invalidate_probe("p");
        assert_eq!(
            ctrl.probe_state("p"),
            PreviewProbeState::Miss,
            "stale frame dropped; not silently available (A2)"
        );
        assert!(
            ctrl.needs_job("p"),
            "a changed recipe/source must re-render, not stay done (A3)"
        );
        assert!(
            !ctrl.lru().contains("d1"),
            "stale frame removed from the RAM LRU (A3)"
        );

        // In-flight → loading; ready → visible as ready.
        ctrl.in_flight.insert("p".into(), ());
        assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Loading);
        ctrl.in_flight.remove("p");
        ctrl.lru.insert("d2", make_frame(2, 2, 2));
        ctrl.probe_digests.insert("p".into(), "d2".into());
        assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Ready);
    }

    /// A2: an exhausted retry budget is a visible per-cell `Failed` state, not
    /// a silent gray cell.
    #[test]
    fn exhausted_retries_surface_visible_failure_state() {
        let mut ctrl = PreviewController {
            lru: LruPreviewCache::default(),
            queue: Arc::new(PreviewQueue::default()),
            result_rx: mpsc::channel::<PreviewResult>().1,
            attempts: BTreeMap::new(),
            failed: BTreeMap::new(),
            pending_failed: BTreeMap::new(),
            in_flight: BTreeMap::new(),
            probe_digests: BTreeMap::new(),
            planned_kind: None,
            active_probe_id: None,
            probe_stamps: BTreeMap::new(),
            directory: None,
        };
        ctrl.failed.insert("p".into(), "decode exploded".into());
        assert_eq!(ctrl.probe_state("p"), PreviewProbeState::Failed);
        assert_eq!(ctrl.failure("p"), Some("decode exploded"));
    }
}
