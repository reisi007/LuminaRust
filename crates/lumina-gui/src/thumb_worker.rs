//! R2-MODSWITCH-1 F7: the background thumbnail thread pool, the cache-aware
//! worker and the main-thread result drain.
//!
//! Extracted from `lib.rs` (file-size ratchet) while adding the F7 cache-aware
//! path: the scheduler (`crate::filmstrip_frame::ensure_thumbnail`) now only
//! performs a metadata-only probe and enqueues a job; the worker owns every
//! disk read and every decode:
//!
//! * `cached == true` — the scheduler saw a standard preview record on disk; the
//!   worker still reads and validates the source, then accepts those bytes only
//!   when the record carries the exact same source-content hash.
//! * `cached == false` — the worker decodes the source, downscales, renders the
//!   default recipe and stores a source-identified standard preview.
//!
//! A cached-but-unreadable preview is a *visible* failure
//! ([`ThumbnailOutcome::Failed`], bounded retry), never a silent fallback. A
//! metadata hit whose bytes vanished between probe and load falls through to a
//! fresh render instead of showing a wrong/blank cell.

use super::*;
use crate::filmstrip::{downscale_rgba, THUMBNAIL_MAX_DIM};
use crate::thumb_cache::THUMB_VIRTUAL_COPY;
use log::{debug, info, trace, warn};
use lumina_core::cache::disk::DiskFolderCache;
use lumina_core::cache::PreviewKind;
use lumina_core::{prepare_source_base, render_frame, StageWork};

/// A request to generate a filmstrip thumbnail for one source on the dedicated
/// background thread pool.
///
/// The channel carrying these is **unbounded** (`std::sync::mpsc::channel`), so
/// the pool never drops a job under load.
pub(crate) struct ThumbnailJob {
    pub(crate) source: PathBuf,
    pub(crate) name: String,
    /// Stable thumbnail key (canonicalized absolute path) the result is filed
    /// under — never the bare filename (REVIEW-GUI-THUMB-1).
    pub(crate) key: String,
    /// Persisted sidecar+bundle content identity captured at enqueue time. The
    /// main thread discards a result if this state changed while the worker ran.
    pub(crate) source_identity: crate::source_actions::SidecarBundleIdentity,
    /// R2-MODSWITCH-1 F7: folder cache handle built by the metadata-only
    /// scheduler probe, so the worker reuses the once-per-folder
    /// `create_dir_all`/settings handle instead of rebuilding it per cell.
    pub(crate) cache: Option<DiskFolderCache>,
    /// R2-MODSWITCH-1 F7: the scheduler saw a standard preview record. The
    /// worker accepts it only after exact source-content identity validation.
    pub(crate) cached: bool,
    /// R3-LOG-1: enqueue instant, so the main-thread drain can report the
    /// enqueue→ready wall time (queue + decode + texture-free delivery).
    pub(crate) enqueued_at: std::time::Instant,
}

/// The outcome of a [`ThumbnailJob`]. A worker failure is always delivered as
/// [`ThumbnailOutcome::Failed`] so the main thread can show a visible error and
/// schedule a bounded retry instead of leaving a gray placeholder for the rest
/// of the session (REVIEW-GUI-THUMB-2, no silent fallback).
pub(crate) enum ThumbnailOutcome {
    Ready(ImageFrame),
    Failed(String),
}

/// The rendered preview pixels produced by a [`ThumbnailJob`]. The texture
/// itself is created on the main thread (it needs the `egui::Context`).
pub(crate) struct ThumbnailResult {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) source: PathBuf,
    pub(crate) source_identity: crate::source_actions::SidecarBundleIdentity,
    /// Identity of the exact sidecar/bundle inputs the worker used. `None` on a
    /// failure, where no pixels can be inserted.
    pub(crate) prepared_identity: Option<crate::source_actions::SidecarBundleIdentity>,
    pub(crate) outcome: ThumbnailOutcome,
    /// R3-LOG-1: forwarded from the job so the drain reports enqueue→ready ms.
    pub(crate) enqueued_at: std::time::Instant,
}

/// Cache-aware decode + downscale + default-recipe render, on the worker thread.
#[cfg(test)]
pub(crate) fn decode_thumbnail_frame(job: &ThumbnailJob) -> Result<ImageFrame, String> {
    decode_thumbnail_frame_with_identity(job).map(|(frame, _)| frame)
}

fn decode_thumbnail_frame_with_identity(
    job: &ThumbnailJob,
) -> Result<(ImageFrame, crate::source_actions::SidecarBundleIdentity), String> {
    // Read once, then validate the sidecar against the exact bytes that may be
    // decoded below. The persistent filename-only record is never authoritative:
    // only a record carrying this source-content hash may serve a cache hit.
    let bytes =
        std::fs::read(&job.source).map_err(|error| format!("{}: {error}", job.source.display()))?;
    let source_image = crate::source_actions::FileContentIdentity::from_bytes(&bytes);
    let source_content_hash = match &source_image {
        crate::source_actions::FileContentIdentity::Hashed(hash) => hash.as_str(),
        _ => unreachable!("in-memory source bytes always have a content hash"),
    };
    let snapshot = crate::source_actions::read_sidecar_recipe_snapshot_for_bytes(
        &job.source,
        THUMB_VIRTUAL_COPY,
        &bytes,
    )?;
    let recipe = snapshot.recipe;
    let has_source_actions = !recipe.source_actions.is_empty();
    let identity_without_actions = crate::source_actions::SidecarBundleIdentity::with_source_image(
        snapshot.document_identity.clone(),
        source_image.clone(),
        None,
    );
    // Source-action thumbnails remain RAM-only because this cache has no
    // recipe/artifact component. Recipe-free thumbnails may reuse persistent
    // bytes only when their exact source-content identity still matches.
    if job.cached && !has_source_actions {
        if let Some(cache) = &job.cache {
            match cache.load_preview_with_source_hash(
                &job.name,
                THUMB_VIRTUAL_COPY,
                PreviewKind::Standard,
                source_content_hash,
            ) {
                Ok(Some(bytes)) => {
                    // A cached-but-corrupt preview is a visible error, not a
                    // silent miss (REVIEW-GUI-THUMB-2 semantics).
                    return ImageFrame::decode(&bytes)
                        .map(|frame| (frame, identity_without_actions))
                        .map_err(|error| format!("cached preview unreadable: {error}"));
                }
                Ok(None) => {
                    // Legacy, source-mismatched, or externally pruned records
                    // are misses. Render from the validated current bytes.
                    debug!("thumbnail cache miss after metadata hit for {}", job.name);
                }
                Err(error) => {
                    debug!("thumbnail cache read failed for {}: {error}", job.name);
                }
            }
        }
    }
    let frame = if is_raw_name(&job.name) {
        lumina_raw::decode_bytes(&bytes, &job.name)
            .map_err(|error| error.to_string())?
            .frame
    } else {
        ImageFrame::decode(&bytes).map_err(|error| error.to_string())?
    };
    let zdata = lumina_sidecar::zdata_path_for(&job.source);
    let source_actions = crate::source_actions::resolve_source_actions(&recipe, &zdata, &frame)
        .map_err(|error| error.to_string())?;
    let prepared_identity = crate::source_actions::SidecarBundleIdentity::with_source_image(
        snapshot.document_identity,
        source_image.clone(),
        source_actions.bundle_identity().cloned(),
    );
    let prepared = if source_actions.is_empty() {
        frame
    } else {
        let mut source_work = StageWork::default();
        prepare_source_base(&frame, source_actions.artifacts(), &mut source_work)
            .map_err(|error| format!("source actions for {}: {error}", job.name))?
    };
    // Source actions precede the thumbnail downscale, preserving their native
    // full-frame dimensions and compositing semantics.
    let (small, w, h) = downscale_rgba(
        &prepared.pixels,
        prepared.width,
        prepared.height,
        THUMBNAIL_MAX_DIM,
    );
    let small_frame = ImageFrame::new(w, h, small).map_err(|error| error.to_string())?;
    // Preserve the historical default-recipe thumbnail when no source action is
    // active. For action fixtures, clone the referenced recipe but clear the
    // already-applied link so Core does not composite the same action twice.
    let mut action_recipe = recipe.clone();
    action_recipe.source_actions.clear();
    let default_recipe = EditRecipe::default();
    let recipe = if has_source_actions {
        &action_recipe
    } else {
        &default_recipe
    };
    let context = RenderContext {
        recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    // GUI-SRCACC-1: a source-action thumbnail must never silently fall back to
    // its pre-repair small frame. Any core render failure is a visible worker
    // error; action-aware frames are also not written to the recipe-blind cache.
    let preview = render_frame(&small_frame, &context)
        .map(|output| output.frame)
        .map_err(|error| format!("render {}: {error}", job.name))?;
    if !has_source_actions {
        let png = preview
            .encode(ImageFileFormat::Png)
            .map_err(|error| error.to_string())?;
        let cache = job
            .cache
            .clone()
            .or_else(|| DiskFolderCache::for_image(&job.source).ok());
        if let Some(cache) = cache {
            let _ = cache.store_preview_with_source_hash(
                &job.name,
                THUMB_VIRTUAL_COPY,
                PreviewKind::Standard,
                source_content_hash,
                &png,
            );
        }
    }
    Ok((preview, prepared_identity))
}

/// Worker entry point: never drops a job silently; failures travel back to the
/// main thread as [`ThumbnailOutcome::Failed`] (REVIEW-GUI-THUMB-2).
fn worker_thumbnail(job: ThumbnailJob) -> ThumbnailResult {
    let (outcome, prepared_identity) = match decode_thumbnail_frame_with_identity(&job) {
        Ok((frame, identity)) => (ThumbnailOutcome::Ready(frame), Some(identity)),
        Err(message) => (ThumbnailOutcome::Failed(message), None),
    };
    ThumbnailResult {
        key: job.key,
        name: job.name,
        source: job.source,
        source_identity: job.source_identity,
        prepared_identity,
        outcome,
        enqueued_at: job.enqueued_at,
    }
}

/// Spin up the dedicated thumbnail thread pool. The pool size is the available
/// parallelism clamped to `[2, 8]`; workers share one (mutex-guarded) job
/// receiver and send results back over an unbounded channel the main thread
/// drains every frame via [`LuminaApp::poll_thumbnails`].
pub(crate) fn spawn_thumbnail_pool() -> (mpsc::Sender<ThumbnailJob>, mpsc::Receiver<ThumbnailResult>)
{
    let (job_tx, job_rx) = mpsc::channel::<ThumbnailJob>();
    let (result_tx, result_rx) = mpsc::channel::<ThumbnailResult>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let pool_size = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8);
    for i in 0..pool_size {
        let rx = Arc::clone(&job_rx);
        let tx = result_tx.clone();
        thread::spawn(move || loop {
            let job = match rx.lock().expect("thumbnail job receiver poisoned").recv() {
                Ok(job) => job,
                Err(_) => break, // all senders gone → shut down
            };
            trace!("thumbnail worker {}: decoding {}", i, job.name);
            // Always reports back: failures arrive as
            // ThumbnailOutcome::Failed so the main thread can show them and
            // retry in a bounded way (REVIEW-GUI-THUMB-2).
            let result = worker_thumbnail(job);
            trace!("thumbnail worker {}: finished {}", i, result.name);
            let _ = tx.send(result);
        });
    }
    info!("thumbnail thread pool started with {} workers", pool_size);
    (job_tx, result_rx)
}

impl LuminaApp {
    /// Drain completed thumbnails from the background pool and build their
    /// textures on the main thread. Runs every frame *regardless of pointer
    /// state*; the reset of the per-frame diagnostic counters happens here,
    /// before any scheduling work of this frame.
    pub(crate) fn poll_thumbnails(&mut self, ctx: &egui::Context) {
        self.frame_thumb_enqueued = 0;
        self.frame_thumbs_ready = 0;
        while let Ok(result) = self.thumbnail_rx.try_recv() {
            if !self.thumbnails.accepts_result(
                &result.key,
                &result.source,
                &result.source_identity,
                result.prepared_identity.as_ref(),
            ) {
                debug!(
                    "discarding stale thumbnail result for {} after artifact change",
                    result.name
                );
                continue;
            }
            match result.outcome {
                ThumbnailOutcome::Ready(frame) => {
                    let enqueue_to_ready_ms =
                        timing::Stopwatch::at(result.enqueued_at).elapsed_ms();
                    let tex = self.make_thumbnail_texture(ctx, &frame, &result.key);
                    self.thumbnails.insert(&result.key, tex);
                    // R3-LOG-1: report the enqueue→ready wall time per key.
                    timing::emit(|| timing::thumbnail_ready_line(&result.key, enqueue_to_ready_ms));
                }
                ThumbnailOutcome::Failed(message) => {
                    // Visible failure state + bounded retry instead of a gray
                    // placeholder for the rest of the session
                    // (REVIEW-GUI-THUMB-2, no silent fallback).
                    warn!("thumbnail failed for {}: {message}", result.name);
                    self.thumbnails.mark_failed(&result.key, message);
                }
            }
            self.frame_thumbs_ready += 1;
            ctx.request_repaint();
        }
    }
}
