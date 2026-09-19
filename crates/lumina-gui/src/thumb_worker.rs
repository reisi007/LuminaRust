//! R2-MODSWITCH-1 F7: the background thumbnail thread pool, the cache-aware
//! worker and the main-thread result drain.
//!
//! Extracted from `lib.rs` (file-size ratchet) while adding the F7 cache-aware
//! path: the scheduler (`crate::filmstrip_frame::ensure_thumbnail`) now only
//! performs a metadata-only probe and enqueues a job; the worker owns every
//! disk read and every decode:
//!
//! * `cached == true` — the scheduler saw a standard preview on disk; the
//!   worker loads and decodes it instead of re-decoding the source.
//! * `cached == false` — the worker decodes the source, downscales, renders the
//!   default recipe and stores the standard preview (the former miss path).
//!
//! A cached-but-unreadable preview is a *visible* failure
//! ([`ThumbnailOutcome::Failed`], bounded retry), never a silent fallback. A
//! metadata hit whose bytes vanished between probe and load falls through to a
//! fresh render instead of showing a wrong/blank cell.

use super::*;
use crate::filmstrip::{downscale_rgba, THUMBNAIL_MAX_DIM};
use log::{debug, info, trace, warn};
use lumina_core::cache::disk::DiskFolderCache;
use lumina_core::cache::PreviewKind;
use lumina_core::render_frame;

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
    /// R2-MODSWITCH-1 F7: folder cache handle built by the metadata-only
    /// scheduler probe, so the worker reuses the once-per-folder
    /// `create_dir_all`/settings handle instead of rebuilding it per cell.
    pub(crate) cache: Option<DiskFolderCache>,
    /// R2-MODSWITCH-1 F7: the scheduler saw a cached standard preview; load and
    /// decode it instead of re-decoding the source.
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
    pub(crate) outcome: ThumbnailOutcome,
    /// R3-LOG-1: forwarded from the job so the drain reports enqueue→ready ms.
    pub(crate) enqueued_at: std::time::Instant,
}

/// Cache-aware decode + downscale + default-recipe render, on the worker thread.
fn decode_thumbnail_frame(job: &ThumbnailJob) -> Result<ImageFrame, String> {
    if job.cached {
        if let Some(cache) = &job.cache {
            match cache.load_preview(&job.name, "vc-original", PreviewKind::Standard) {
                Ok(Some(bytes)) => {
                    // A cached-but-corrupt preview is a visible error, not a
                    // silent miss (REVIEW-GUI-THUMB-2 semantics).
                    return ImageFrame::decode(&bytes)
                        .map_err(|error| format!("cached preview unreadable: {error}"));
                }
                Ok(None) => {
                    // The metadata hit went stale between probe and load (e.g.
                    // an external prune) — render fresh rather than show a
                    // wrong/blank cell.
                    debug!("thumbnail cache miss after metadata hit for {}", job.name);
                }
                Err(error) => {
                    debug!("thumbnail cache read failed for {}: {error}", job.name);
                }
            }
        }
    }
    let bytes =
        std::fs::read(&job.source).map_err(|error| format!("{}: {error}", job.source.display()))?;
    let frame = if is_raw_name(&job.name) {
        lumina_raw::decode_bytes(&bytes, &job.name)
            .map_err(|error| error.to_string())?
            .frame
    } else {
        ImageFrame::decode(&bytes).map_err(|error| error.to_string())?
    };
    let (small, w, h) = downscale_rgba(&frame.pixels, frame.width, frame.height, THUMBNAIL_MAX_DIM);
    let small_frame = ImageFrame::new(w, h, small).map_err(|error| error.to_string())?;
    let context = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    // Default-recipe render for display; a render failure falls back to the
    // plain downscaled frame (documented display-only preview path).
    let preview = render_frame(&small_frame, &context)
        .map(|o| o.frame)
        .unwrap_or(small_frame);
    let png = preview
        .encode(ImageFileFormat::Png)
        .map_err(|error| error.to_string())?;
    let cache = job
        .cache
        .clone()
        .or_else(|| DiskFolderCache::for_image(&job.source).ok());
    if let Some(cache) = cache {
        let _ = cache.store_preview(&job.name, "vc-original", PreviewKind::Standard, &png);
    }
    Ok(preview)
}

/// Worker entry point: never drops a job silently; failures travel back to the
/// main thread as [`ThumbnailOutcome::Failed`] (REVIEW-GUI-THUMB-2).
fn worker_thumbnail(job: ThumbnailJob) -> ThumbnailResult {
    let outcome = match decode_thumbnail_frame(&job) {
        Ok(frame) => ThumbnailOutcome::Ready(frame),
        Err(message) => ThumbnailOutcome::Failed(message),
    };
    ThumbnailResult {
        key: job.key,
        name: job.name,
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
