//! Shared eframe application for the native and browser MVP.

// REVIEW-GUI-WASM-FOLLOWUP: `filmstrip` (background thumbnail pool, disk-cache
// probes) and `viewport` (filmstrip/navigator windowing math) are native-only
// capabilities — the WASM filmstrip is a static capability hint. Gating the
// modules themselves keeps the wasm32 build free of dead-code warnings.
#[cfg(not(target_arch = "wasm32"))]
mod filmstrip;
mod i18n;
// F-009: file-backed user presets (`<name>.lumina-preset.json`) are a native
// capability; wasm32 keeps the in-memory create/apply flow only.
#[cfg(not(target_arch = "wasm32"))]
mod presets;
// PREVIEW-CACHE-FEATURE: the neighbor-preview controller (worker pool + RAM/disk
// LRU) is a native capability (background threads + native file IO).
#[cfg(not(target_arch = "wasm32"))]
mod preview_ctrl;
mod slider;
mod theme;
#[cfg(not(target_arch = "wasm32"))]
mod viewport;

use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::cache::disk::DiskFolderCache;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::cache::PreviewKind;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::MaskPolicy;
// REVIEW-GUI-WASM-FOLLOWUP: `export_image`/`ExportOptions` (Export module) and
// `rasterize_prompt` (mask overlay) are only reachable on native.
use lumina_core::{
    analyze_tone, match_total_exposure_masked, prepare_source_base, render_frame_from_base,
    suggest_auto_tone, tone_fingerprint, AutoToneConfig, CacheStage, ImageFileFormat, ImageFrame,
    MaskContext, MaskLayerResult, MaskPlane, OutputSpec, RenderContext, RenderKey, StageFrameCache,
    StageWork,
};
// PERF-FILMSTRIP only (native thumbnail worker); unused on wasm32.
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::render_frame;
#[cfg(not(target_arch = "wasm32"))]
use lumina_core::{export_image, masks::rasterize_prompt, ExportOptions};
use lumina_raw::RawError;
#[cfg(not(target_arch = "wasm32"))]
use lumina_sidecar::{
    load_zdata, zdata_path_for, ArtifactStatus, BrushMark, BrushMarkSign, CoordinateSystem,
    DecodeFingerprint, GeometryFingerprint, HistoryEntry, MaskDefinition, MaskLayer, MaskOperation,
    MaskPrompt, MaskReference, MaskStatus, ModelIdentity, Point2, Preprocessing, PromptTransform,
    Resolution, SidecarDocument, SourceFingerprint, SourceIdentity, SourceStatus,
};
use lumina_sidecar::{
    AnalysisFingerprint, ColorGrading, ColorGradingRange, Crop, CurveChannels, CurvePoint, Curves,
    EditRecipe, Effects, Geometry, Grain, HslAdjustments, HslChannel, LensCorrection,
    NoiseReduction, Perspective, Presence, Preset, Sharpening, Vignette,
};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::Value;
use slider::{identity_spec, lr_slider, percent_spec, SliderAction, SliderSpec};
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeSet;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{mpsc, Arc, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;

// REVIEW-GUI-WASM-FOLLOWUP: every `debug!` call site sits behind a native
// thumbnail/decode path, so the import is gated with them.
#[cfg(not(target_arch = "wasm32"))]
use log::debug;
use log::{error, info, trace, warn};
use theme::apply_lightroom_dark;

#[cfg(not(target_arch = "wasm32"))]
use filmstrip::{downscale_rgba, ThumbnailManager, THUMBNAIL_MAX_DIM};
use i18n::Str;

/// Work which may be performed when the GUI has no interactive input.
///
/// Queueing is deliberately separate from mask status: a missing/pending mask
/// is never inserted here implicitly.  The caller must enqueue it as the
/// result of an explicit user action (or a future CLI/GUI command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleTask {
    MaskInference {
        mask_id: String,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Thumbnail {
        source: PathBuf,
        name: String,
    },
}

/// Top-level module selected in the module bar (Library / Develop / Export).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Library,
    Develop,
    Export,
}

/// Maps a Lightroom-style module-switch keyboard shortcut to its target module.
///
/// This is a pure function so the mapping can be unit-tested without an
/// [`egui::Context`]. The mapping mirrors Lightroom's module keys:
///
/// * `G` switches to `Library` (Grid).
/// * `D` switches to `Develop`.
/// * `E` is Lightroom's "Loupe" shortcut. Lumina has no separate Loupe module,
///   so `E` is treated as an alias for `Library` (documented here so the alias
///   is intentional and not a silent fallback).
///
/// Keys that are not module shortcuts — in particular the existing `Y`
/// Before/After toggle and `Esc` eyedropper-cancel — return `None` and keep
/// their own, separate handling.
pub fn module_for_key(key: egui::Key) -> Option<Module> {
    match key {
        egui::Key::G => Some(Module::Library),
        egui::Key::D => Some(Module::Develop),
        egui::Key::E => Some(Module::Library),
        _ => None,
    }
}

/// Active interactive masking tool (F-103-N4). `None` means the preview accepts
/// the ordinary click/eyedropper interactions; any other variant arms the
/// preview for a drag gesture that builds a [`MaskPrompt`] for the selected
/// mask.  The tool only chooses *how* the drag is interpreted; persistence goes
/// through the existing sidecar paths.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskTool {
    #[default]
    None,
    Brush,
    LinearGradient,
    Radial,
}

/// Preview zoom behaviour (Lightroom-like). `Fit` is object-contain (the
/// previous default); the absolute modes resolve to an effective scale derived
/// from the pane each frame so they survive window resizes. `Custom` is set by
/// scroll-wheel / `+/-` zoom and pins an explicit relative-to-fit multiplier
/// that is no longer re-derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    Fit,
    OneToOne,
    TwoHundred,
    FitWidth,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedIdleTask {
    id: u64,
    priority: u8,
    task: IdleTask,
}

/// Small, bounded priority queue for work that is safe to defer until idle.
#[derive(Debug, Clone)]
pub struct IdleQueue {
    capacity: usize,
    next_id: u64,
    tasks: Vec<QueuedIdleTask>,
}

impl IdleQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next_id: 0,
            tasks: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Maximum number of queued tasks before [`enqueue`](Self::enqueue) starts
    /// dropping jobs.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Enqueue only explicit requests. Returns a stable cancellation handle.
    pub fn enqueue(&mut self, task: IdleTask, priority: u8) -> Option<u64> {
        if self.tasks.len() >= self.capacity {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.tasks.push(QueuedIdleTask { id, priority, task });
        Some(id)
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        self.tasks.len() != before
    }

    /// Takes the highest-priority task. Equal priorities retain FIFO order
    /// (REVIEW-GUI-N4): ties are broken by the monotonically increasing
    /// enqueue id, so the *first*-enqueued task of a priority class wins.
    /// (`Iterator::max_by_key` alone would pick the *last* maximum — LIFO.)
    pub fn pop_next(&mut self) -> Option<(u64, IdleTask)> {
        let index = self
            .tasks
            .iter()
            .enumerate()
            .min_by_key(|(_, task)| (std::cmp::Reverse(task.priority), task.id))?
            .0;
        let task = self.tasks.remove(index);
        Some((task.id, task.task))
    }
}

/// A request to generate a filmstrip thumbnail for one source on the dedicated
/// background thread pool.
///
/// The channel carrying these is **unbounded** (`std::sync::mpsc::channel`), so
/// the pool never drops a job under load — the original bottleneck was the
/// bounded `IdleQueue` (capacity 32) that silently dropped thumbnails and
/// logged "thumbnail queue full; will retry ... on a later frame" while the UI
/// froze generating them synchronously on the main thread (M5 Pro: unusable
/// switching). Thumbnails are now decoded/downscaled/rendered on worker threads
/// and the resulting pixels are uploaded to a texture on the main thread.
#[cfg(not(target_arch = "wasm32"))]
struct ThumbnailJob {
    source: PathBuf,
    name: String,
    /// Stable thumbnail key (canonicalized absolute path) the result is filed
    /// under — never the bare filename (REVIEW-GUI-THUMB-1).
    key: String,
}

/// The outcome of a [`ThumbnailJob`]. A worker failure is always delivered as
/// [`ThumbnailOutcome::Failed`] so the main thread can show a visible error and
/// schedule a bounded retry instead of leaving a gray placeholder for the rest
/// of the session (REVIEW-GUI-THUMB-2, no silent fallback).
#[cfg(not(target_arch = "wasm32"))]
enum ThumbnailOutcome {
    Ready(ImageFrame),
    Failed(String),
}

/// The rendered (downscaled + default-recipe-rendered) preview pixels produced
/// by a [`ThumbnailJob`]. The worker computes the frame, caches the PNG to disk
/// and sends the pixels; the texture itself is created on the main thread (it
/// needs the `egui::Context`) from these pixels.
#[cfg(not(target_arch = "wasm32"))]
struct ThumbnailResult {
    key: String,
    name: String,
    outcome: ThumbnailOutcome,
}

/// Decode + downscale + default-recipe-render a source on a background worker
/// thread (PERF-FILMSTRIP). Returns the rendered frame so the main thread can
/// build the `egui` texture (it needs the `Context`). Errors are returned
/// visibly to the worker caller, never swallowed into `None`.
#[cfg(not(target_arch = "wasm32"))]
fn decode_thumbnail_frame(source: &Path, name: &str) -> Result<ImageFrame, String> {
    let bytes = std::fs::read(source).map_err(|error| format!("{}: {error}", source.display()))?;
    let frame = if is_raw_name(name) {
        lumina_raw::decode_bytes(&bytes, name)
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
    };
    // Default-recipe render for display; a render failure falls back to the
    // plain downscaled frame (documented display-only preview path).
    let preview = render_frame(&small_frame, &context)
        .map(|o| o.frame)
        .unwrap_or(small_frame);
    let png = preview
        .encode(ImageFileFormat::Png)
        .map_err(|error| error.to_string())?;
    if let Ok(cache) = DiskFolderCache::for_image(source) {
        let _ = cache.store_preview(name, "vc-original", PreviewKind::Standard, &png);
    }
    Ok(preview)
}

/// Worker entry point: never drops a job silently; failures travel back to the
/// main thread as [`ThumbnailOutcome::Failed`] (REVIEW-GUI-THUMB-2).
#[cfg(not(target_arch = "wasm32"))]
fn worker_thumbnail(job: ThumbnailJob) -> ThumbnailResult {
    let outcome = match decode_thumbnail_frame(&job.source, &job.name) {
        Ok(frame) => ThumbnailOutcome::Ready(frame),
        Err(message) => ThumbnailOutcome::Failed(message),
    };
    ThumbnailResult {
        key: job.key,
        name: job.name,
        outcome,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GuiError {
    #[error("{0}")]
    Core(#[from] lumina_core::CoreError),
    #[error("{0}")]
    Sidecar(#[from] lumina_sidecar::SidecarError),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Raw(#[from] RawError),
}

pub struct LuminaApp {
    original: Option<ImageFrame>,
    preview: Option<ImageFrame>,
    source_bytes: Option<Vec<u8>>,
    source_is_raw: bool,
    raw_orientation: u8,
    camera_white_balance: Option<[f32; 4]>,
    source_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: String,
    #[cfg(not(target_arch = "wasm32"))]
    directory: String,
    #[cfg(not(target_arch = "wasm32"))]
    entries: Vec<FileBrowserEntry>,
    recipe: EditRecipe,
    texture: Option<egui::TextureHandle>,
    /// R2-GUIMOD-02: identity of the pixels currently held by
    /// [`Self::texture`] — `(preview generation, before_after, [w, h])`.
    /// The CPU present path re-uploads only when this differs from what would
    /// be displayed, instead of rebuilding a full-screen [`egui::ColorImage`]
    /// and re-creating the texture on every repaint (mousemoves over panels
    /// used to pay a full-frame memcpy + upload per frame).
    #[cfg(not(target_arch = "wasm32"))]
    texture_identity: Option<(u64, bool, [usize; 2])>,
    /// R2-GUIMOD-02: bumped whenever `self.preview` receives new content.
    /// Part of [`Self::texture_identity`]; together with the Before/After flag
    /// and pixel size it gates the CPU texture upload to actual content
    /// changes. A monotonic counter is deliberate: draft and full renders of
    /// the same source can produce identical render keys while their pixels
    /// differ (masks are skipped for drafts), so key-based identity would be
    /// unsound.
    ///
    /// INVARIANT: every site that assigns `self.preview` MUST bump this
    /// counter — otherwise the CPU present path keeps serving the previous
    /// upload (`texture_identity` still matches). The only production
    /// assignment today is `render_from`; a new source clears `preview`
    /// implicitly by resetting render state before the next render bumps the
    /// generation again.
    #[cfg(not(target_arch = "wasm32"))]
    preview_generation: u64,
    status: String,
    error: Option<String>,
    render_key: Option<RenderKey>,
    tone_analysis: Option<lumina_core::ToneAnalysis>,
    /// Effective mask layers of the last [`Self::render`] (F-041): the
    /// measurement domain of `Match Total Exposure` is the rendered preview
    /// weighted by these planes. Empty on wasm32 (no mask context, documented
    /// post-MVP state) and whenever the render produced no layers.
    render_mask_layers: Vec<MaskLayerResult>,
    #[cfg(not(target_arch = "wasm32"))]
    document: Option<SidecarDocument>,
    #[cfg(not(target_arch = "wasm32"))]
    virtual_copy_id: String,
    #[cfg(not(target_arch = "wasm32"))]
    selected_mask_id: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    mask_name_input: String,
    #[cfg(not(target_arch = "wasm32"))]
    mask_tool: MaskTool,
    /// Normalized brush radius (0..=1 in source space). Driven by a slider.
    #[cfg(not(target_arch = "wasm32"))]
    brush_radius: f32,
    /// When true, brush marks use the negative (eraser) sign.
    #[cfg(not(target_arch = "wasm32"))]
    brush_eraser: bool,
    /// Marks accumulated during an in-progress brush drag (cleared on release).
    #[cfg(not(target_arch = "wasm32"))]
    pending_brush_marks: Vec<BrushMark>,
    /// Drag start/current normalized points for gradient/radial gestures.
    #[cfg(not(target_arch = "wasm32"))]
    drag_start: Option<Point2>,
    #[cfg(not(target_arch = "wasm32"))]
    drag_current: Option<Point2>,
    /// True while a mask-tool drag is in progress (drives the live overlay).
    #[cfg(not(target_arch = "wasm32"))]
    drawing: bool,
    preset_name: String,
    preset_fields: BTreeMap<String, bool>,
    preset_relative_exposure: bool,
    /// F-009: user-global presets directory; `None` means the platform config
    /// base could not be determined and file presets are shown as unavailable
    /// (no silent fallback directory).
    #[cfg(not(target_arch = "wasm32"))]
    presets_dir: Option<std::path::PathBuf>,
    /// F-009: current snapshot of the presets directory. Failed files stay in
    /// the list with their error text instead of being skipped silently.
    #[cfg(not(target_arch = "wasm32"))]
    preset_entries: Vec<presets::PresetEntry>,
    idle_queue: IdleQueue,
    /// PERF-FILMSTRIP: dedicated background thread pool for filmstrip
    /// thumbnails. `thumbnail_tx` enqueues jobs (unbounded mpsc, no capacity
    /// gate); `thumbnail_rx` delivers rendered frames to be textured on the
    /// main thread. Native-only (no threads on wasm).
    #[cfg(not(target_arch = "wasm32"))]
    thumbnail_tx: mpsc::Sender<ThumbnailJob>,
    #[cfg(not(target_arch = "wasm32"))]
    thumbnail_rx: mpsc::Receiver<ThumbnailResult>,
    /// Active top-level module (Library / Develop / Export).
    active_module: Module,
    /// Export module UI state (F-103-N5). The target path is chosen via a
    /// native save dialog; the format/quality drive the shared export path.
    /// Native-only: the wasm Export module is a capability hint
    /// (REVIEW-GUI-WASM-FOLLOWUP).
    #[cfg(not(target_arch = "wasm32"))]
    export_path: String,
    #[cfg(not(target_arch = "wasm32"))]
    export_format: ImageFileFormat,
    #[cfg(not(target_arch = "wasm32"))]
    export_quality: u8,
    /// Before/After toggle state. Never mutates the recipe.
    before_after: bool,
    /// White-balance eyedropper armed state.
    wb_pick_mode: bool,
    /// Generated filmstrip thumbnail textures. Native-only (REVIEW-GUI-
    /// WASM-FOLLOWUP): the wasm filmstrip is a static placeholder.
    #[cfg(not(target_arch = "wasm32"))]
    thumbnails: ThumbnailManager,
    // ---- PERF-GUI-* (CPU interactivity quick-wins, no GPU) ----
    /// True while the preview shows a low-resolution draft (rendered from
    /// `draft_original` during a slider drag); cleared on the full render.
    preview_is_draft: bool,
    /// Source downscaled to draft resolution, cached on load so draft renders
    /// never re-allocate during a slider drag (PERF-GUI-3 "zero alloc").
    draft_original: Option<ImageFrame>,
    /// PERF-GUI-1: RAM cache of prepared base stages (`prepare_source_base`
    /// output: post decode/source-actions/ROI-crop, pre-adjustment), keyed by
    /// `CacheStage::Base` digests. Recipe-blind keys mean every slider change
    /// reuses the cached demosaiced base and re-renders only the adjustment
    /// stage downstream; a new source clears it in [`Self::apply_decoded_frame`].
    base_stage_cache: StageFrameCache,
    /// PERF-GUI-1: memoized blake3 content hash of `source_bytes`. Hashing the
    /// whole RAW file per render tick was part of the old hot path; it is now
    /// computed once per loaded source and invalidated with it.
    source_hash_memo: Option<String>,
    /// PERF-GUI-1: stage work counters of the last completed render
    /// (diagnostics + cache-hit tests). `None` until the first staged render.
    last_stage_work: Option<StageWork>,
    /// Long-edge cap (px) for the cached draft source (viewport resolution).
    draft_max_dim: u32,
    /// Timestamp (egui `ctx.input(|i| i.time)`) of the last frame that still
    /// had pending edits; drives the 150 ms idle debounce (PERF-GUI-3/4).
    last_edit_time: f64,
    /// Preview zoom factor (1.0 = fit). >1 zooms into the centre; the render
    /// path crops to the visible source bounding box (ROI, PERF-GUI-5).
    preview_zoom: f32,
    /// Active zoom mode driving `preview_zoom` (see [`ZoomMode`]). Non-`Custom`
    /// modes re-derive `preview_zoom` from the pane each frame (so they survive
    /// resizes); `Custom` pins an explicit relative-to-fit multiplier.
    zoom_mode: ZoomMode,
    /// Screen-space pan offset (px) from the centred position, applied when the
    /// preview is zoomed beyond fit so the image can be dragged (hand tool).
    preview_pan: egui::Vec2,
    /// Source ROI `(x, y, w, h)` of the currently displayed texture. `None`
    /// means the whole frame. Pointer→source mapping accounts for this crop so
    /// the WB eyedropper and mask tools stay accurate while zoomed/panned.
    preview_roi: Option<[u32; 4]>,
    /// Cached geometry from the last [`Self::draw_preview`] so the next frame's
    /// [`Self::sync_zoom`] can derive absolute zoom modes correctly.
    ///
    /// `preview_base_fit_scale` is the object-contain fit of the pane against
    /// the **un-cropped source dimensions**, never against the currently
    /// displayed texture: at zoom > 1 that texture is an ROI crop whose fit
    /// scale depends on the zoom itself, so deriving absolute modes (100% /
    /// 200% / Fit Width) from it oscillates frame-by-frame
    /// (REVIEW-GUI-ZOOMLOOP-1).
    preview_base_fit_scale: f32,
    preview_pane_w: f32,
    preview_pane_h: f32,
    /// Un-cropped source dimensions backing the displayed texture (cached so
    /// `sync_zoom` can compute Fit-Width and the base fit without borrowing
    /// `self.original`).
    preview_src_w: f32,
    preview_src_h: f32,
    /// Effective on-screen scale (screen px per source px) for the zoom readout.
    preview_effective_scale: f32,
    /// Whether the left thumbnail navigator rail is open. Native-only (the
    /// navigator rail is a native module; REVIEW-GUI-WASM-FOLLOWUP).
    #[cfg(not(target_arch = "wasm32"))]
    navigator_open: bool,
    /// Library module: expanded folder-tree nodes, keyed by absolute path.
    #[cfg(not(target_arch = "wasm32"))]
    open_folders: BTreeSet<String>,
    /// Library module: lazy per-folder children cache, filled via `read_dir`
    /// the first time a folder node is expanded.
    #[cfg(not(target_arch = "wasm32"))]
    folder_children: BTreeMap<String, Vec<String>>,
    /// Library module: depth-limited RAW file count per folder node
    /// (display only; computed once per folder).
    #[cfg(not(target_arch = "wasm32"))]
    folder_raw_counts: BTreeMap<String, usize>,
    /// Library module: current thumbnail cell size (px) for the center grid,
    /// driven by a toolbar slider (Lightroom-like resizable library thumbs).
    #[cfg(not(target_arch = "wasm32"))]
    library_thumb_size: f32,
    /// Develop history section: currently selected (last restored) history
    /// entry id of the active virtual copy.
    #[cfg(not(target_arch = "wasm32"))]
    history_selected: Option<String>,
    /// PERF-GUI-7: receiver for a background RAW/raster decode. `Some` while a
    /// decode is in flight on a worker thread; native-only (no threads on wasm).
    #[cfg(not(target_arch = "wasm32"))]
    decode_rx: Option<std::sync::mpsc::Receiver<DecodeResult>>,
    /// REVIEW-GUI-N1: revision (BLAKE3 over the JSON) of the on-disk sidecar
    /// that the in-memory `document` lineage is based on. `None` means no
    /// sidecar file existed when this lineage started (fresh document). Passed
    /// to the compare-and-swap write in [`Self::save_sidecar`] so an
    /// externally modified sidecar surfaces as a visible conflict instead of
    /// being silently overwritten; refreshed after every successful save.
    #[cfg(not(target_arch = "wasm32"))]
    sidecar_revision: Option<String>,
    /// True while an edit (slider drag, presence change, etc.) needs a
    /// full-quality render. Drives the debounced full render after a pointer
    /// drag settles (PERF-GUI-3/4). Cleared once the full render runs.
    pending_full_render: bool,
    /// Set once the directory-auto-load has begun a background decode, so
    /// `list_directory` never re-triggers it every time the directory is
    /// rescanned (the decode is async and `original` stays `None` until it
    /// finishes). Left unset while no RAW entry exists, so a later scan of a
    /// now-populated directory can still auto-load. Native-only (directory
    /// auto-load is a native file-system capability).
    #[cfg(not(target_arch = "wasm32"))]
    auto_load_attempted: bool,
    /// GUI-60FPS-1: optional GPU context for the native desktop. `None` on wasm
    /// or when no adapter is bound (CPU fallback remains fully functional).
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    gpu: Option<lumina_gpu::GpuContext>,
    /// GUI-60FPS-1 H1: persistent R16 mask plane (Vec<u16> u16-LE, row-major,
    /// `width × height`) backing the interactive brush. Kept CPU-side so each
    /// dirty 512² tile can be (re-)stamped incrementally via
    /// `lumina_core::mask_tiles::stamp_brush_mark` and then uploaded with
    /// `queue.write_texture` (`bytemuck::cast_slice` → `&[u8]`). Only dirty tiles
    /// are uploaded per stroke (no whole-plane rewrite, no dummy zeros).
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    brush_mask_plane: Option<Vec<u16>>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    brush_mask_plane_dims: Option<(u32, u32)>,
    /// GUI-WGPU-PRESENT-1: the eframe wgpu renderer's shared state. When
    /// present, `lumina-gpu` was constructed on the *same* Device/Queue
    /// (see `attach_wgpu_render_state`), so the VRAM overlay composite can be
    /// registered as an egui user texture and presented without any CPU
    /// readback.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Offscreen target the VRAM overlay pass composites into; registered once
    /// as an egui user texture and re-created only when dimensions change.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    present_target: Option<PresentTarget>,
    /// True while the VRAM output corresponds to the current recipe/source:
    /// set right after a successful `render_to_vram`, cleared by every edit
    /// ([`Self::mark_dirty`]) so a stale tone result can never be presented.
    /// R2-GUIMOD-01: also cleared by every completed **full-quality** CPU
    /// render (`render_from` on the non-draft path) — otherwise the debounced
    /// full render after a drag would compute sharp pixels that are then never
    /// shown because the gate kept presenting the superseded VRAM draft.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    vram_fresh: bool,
    /// R2-GUIMOD-05: memoized `unsupported_gpu_stages(&self.recipe)` verdict,
    /// keyed by the [`RenderKey`] of the render it was computed for. The gate
    /// used to rebuild this `Vec<String>` (with `format!` allocations) every
    /// frame although recipe/render identity rarely changes. `None` while no
    /// key-backed verdict is stored; queried without a render key (dirty
    /// preview) deliberately bypasses the memo because the recipe may have
    /// drifted since the last render.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    gpu_stage_gate: Option<(RenderKey, bool)>,
    /// True while the VRAM mask texture carries the pipeline-*evaluated* layer
    /// planes (pushed after a full render) rather than only live brush stamps —
    /// then the shader overlay already shows what the CPU overlay would paint.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    vram_mask_is_evaluated: bool,
    /// The egui user-texture id + size of the GPU-presented preview for THIS
    /// frame (recomputed in `update_texture`, consumed in `draw_preview`).
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    gpu_present_frame: Option<(egui::TextureId, [usize; 2])>,
    /// R2-GUIMOD-06: visible (non-stderr) feedback for the GPU→CPU routing
    /// fallback. `Some(reason)` when a GPU context is available and usable but
    /// the recipe references stages the VRAM tone path cannot evaluate, so the
    /// preview is computed on the CPU — a silent fallback before this fix.
    /// `None` while the GPU present path is usable (or when no GPU context
    /// exists at all, in which case there is no "fallback" to report). Surfaced
    /// as a status badge in the preview HUD; it never affects rendered pixels.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    gpu_route_fallback: Option<String>,
    /// GUI-SCROLL-200-1: per-frame diagnostic counters for `LUMINA_PERF_LOG=1`.
    /// `frame_thumb_enqueued` counts worker jobs enqueued (or cached previews
    /// loaded) this frame, `frame_thumbs_ready` counts worker results applied.
    /// Both are reset at the start of [`Self::update`]; a scroll spike while
    /// thumbnail jobs run shows up as large values in the same frame that
    /// exceeds the 16.7 ms budget.
    #[cfg(not(target_arch = "wasm32"))]
    frame_thumb_enqueued: usize,
    #[cfg(not(target_arch = "wasm32"))]
    frame_thumbs_ready: usize,
    /// PREVIEW-CACHE-FEATURE: neighbor-preview controller (worker pool + RAM/disk
    /// LRU + prefetch window). Lazy-created on first navigation so unit tests
    /// that never schedule neighbors stay thread-free.
    #[cfg(not(target_arch = "wasm32"))]
    preview_ctrl: Option<preview_ctrl::PreviewController>,
    /// PREVIEW-CACHE-FEATURE: per-frame counters for the neighbor-preview work
    /// (LUMINA_PERF_LOG diagnostics).
    #[cfg(not(target_arch = "wasm32"))]
    frame_previews_enqueued: usize,
    #[cfg(not(target_arch = "wasm32"))]
    frame_previews_ready: usize,
}

/// GUI-WGPU-PRESENT-1: offscreen present target + its egui registration.
///
/// The overlay pass composites the VRAM tone output and mask plane into
/// `texture`; `texture` is registered with the eframe wgpu renderer as a user
/// texture (`register_native_texture`) so `painter().image(id, ..)` draws it
/// directly on screen. Re-created only when the VRAM dimensions change; the
/// old registration is freed to avoid leaking GPU-side bind groups.
#[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
struct PresentTarget {
    texture: eframe::wgpu::Texture,
    #[allow(dead_code)]
    view: eframe::wgpu::TextureView,
    id: egui::TextureId,
    dims: (u32, u32),
}

/// GUI-WGPU-PRESENT-1: hand the eframe wgpu renderer's shared state to the app.
///
/// Called from `run_native`'s builder with `CreationContext::wgpu_render_state`.
/// When present, [`lumina_gpu::GpuContext`] resources are re-based onto that
/// device/queue so VRAM textures are shareable with the presenting surface.
/// R2-GUIMOD-09: this function is also the single construction point for the
/// context — `LuminaApp::new` deliberately leaves `gpu` empty so startup
/// performs at most one adapter/device request. When no renderer state is
/// handed over, a standalone context is created here (same capability as the
/// old eager constructor) so non-present GPU paths keep working; headless
/// callers that never invoke this function simply stay CPU-only.
/// No-op under wasm32 or without the `gpu` feature (CPU present path stays).
#[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
pub fn attach_wgpu_render_state(
    app: &mut LuminaApp,
    state: Option<eframe::egui_wgpu::RenderState>,
) {
    app.wgpu_render_state = state.clone();
    // R2-GUIMOD-09: this is now the ONLY place that constructs a
    // `GpuContext` during startup. `LuminaApp::new` leaves `gpu` empty, so a
    // shared-device context is built exactly once here instead of paying a
    // standalone init first and discarding it moments later.
    // Re-base the GPU context onto the renderer's device/queue so VRAM
    // textures share the presenting surface's device (the whole point of the
    // migration). If that fails we fall back to a standalone context and log
    // loudly — no silent capability downgrade.
    if let Some(rs) = &state {
        match lumina_gpu::GpuContext::from_parts(
            rs.instance.clone(),
            rs.adapter.clone(),
            rs.device.clone(),
            rs.queue.clone(),
        ) {
            Ok(ctx) => {
                log::info!(
                    "GPU present path: sharing eframe wgpu device ({})",
                    rs.adapter.get_info().name
                );
                app.gpu = Some(ctx);
            }
            Err(err) => {
                log::warn!(
                    "GPU present path: shared-device context unavailable ({err}); \
                     falling back to the CPU present upload"
                );
                // Preserve the historical capability of the eager standalone
                // context for non-present GPU paths (`render_to_vram`), but
                // create it here so startup still performs only ONE adapter/
                // device request.
                app.gpu = lumina_gpu::GpuContext::new().ok();
            }
        }
    } else {
        // No renderer state was handed over (headless harnesses, tests). The
        // historical eager constructor would have produced a standalone
        // context; keep that capability available at the single construction
        // point without any extra init cost when it is never used.
        app.gpu = lumina_gpu::GpuContext::new().ok();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct FileBrowserEntry {
    path: PathBuf,
    name: String,
    /// Stable thumbnail key: canonicalized absolute path. Identical filenames
    /// in different folders must never share a thumbnail cell
    /// (REVIEW-GUI-THUMB-1).
    thumb_key: String,
    has_sidecar: bool,
    source_status: SourceStatus,
    conflict: bool,
    virtual_copies: usize,
    missing_models: usize,
}

/// REVIEW-GUI-THUMB-1: stable thumbnail cache key. The canonicalized absolute
/// path guarantees that the same filename in two folders maps to different
/// entries; a canonicalize failure (e.g. a missing file) falls back to the
/// lossy path string, which is still folder-scoped.
#[cfg(not(target_arch = "wasm32"))]
fn thumbnail_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

#[cfg(not(target_arch = "wasm32"))]
impl FileBrowserEntry {
    fn status_label(&self) -> &'static str {
        if self.conflict {
            Str::StatusConflict.t()
        } else if self.is_offline() {
            Str::StatusOffline.t()
        } else if self.has_sidecar {
            Str::Sidecar.t()
        } else {
            Str::StatusWithout.t()
        }
    }
    fn is_offline(&self) -> bool {
        matches!(self.source_status, SourceStatus::Missing)
    }
    /// Stable thumbnail key (canonicalized absolute path). Read-only access for
    /// headless scheduling tests/diagnostics (GUI-SCROLL-200-1).
    pub fn thumb_key(&self) -> &str {
        &self.thumb_key
    }
}

/// PERF-GUI-7: result of a background RAW/raster decode. Carries the decoded
/// frame plus the metadata needed to apply it on the main thread (and to load
/// the matching sidecar). `Err` carries the (path, message) so the GUI can show
/// the decode failure without blocking the worker thread.
#[cfg(not(target_arch = "wasm32"))]
struct DecodedFrame {
    path: String,
    name: String,
    bytes: Vec<u8>,
    frame: ImageFrame,
    orientation: u8,
    camera_white_balance: Option<[f32; 4]>,
    source_is_raw: bool,
}

#[cfg(not(target_arch = "wasm32"))]
type DecodeResult = Result<DecodedFrame, (String, String)>;

/// Draw method of one Develop section: `fn(&mut LuminaApp, &mut egui::Ui)`.
/// Factored out so [`LuminaApp::DEVELOP_SECTIONS`] stays readable.
type DevelopSectionDraw = fn(&mut LuminaApp, &mut egui::Ui);

impl LuminaApp {
    /// Set the active top-level module (Library / Develop / Export). Used by the
    /// headless snapshot tests (F-103-N9) to render a specific module; this is a
    /// pure state assignment with no recipe/sidecar side effects.
    pub fn set_module(&mut self, module: Module) {
        trace!("GUI interaction: set_module {:?}", module);
        self.active_module = module;
    }

    pub fn new(_ctx: egui::Context) -> Self {
        // PERF-FILMSTRIP: spin up the dedicated thumbnail thread pool. The pool
        // size is the available parallelism clamped to [2, 8] (M5 Pro reports 12
        // logical cores, so this lands at 8 workers; for small machines it never
        // drops below 2). Workers share one (mutex-guarded) job receiver and
        // send results back over an unbounded channel the main thread drains
        // every frame via `poll_thumbnails`.
        #[cfg(not(target_arch = "wasm32"))]
        let (thumbnail_tx, thumbnail_rx) = {
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
                thread::spawn(move || {
                    loop {
                        let job = match rx.lock().expect("thumbnail job receiver poisoned").recv() {
                            Ok(job) => job,
                            Err(_) => break, // all senders gone → shut down
                        };
                        trace!("thumbnail worker {}: decoding {}", i, job.name);
                        // Always reports back: failures arrive as
                        // ThumbnailOutcome::Failed so the main thread can show
                        // them and retry in a bounded way (REVIEW-GUI-THUMB-2).
                        let result = worker_thumbnail(job);
                        trace!("thumbnail worker {}: finished {}", i, result.name);
                        let _ = tx.send(result);
                    }
                });
            }
            info!("thumbnail thread pool started with {} workers", pool_size);
            (job_tx, result_rx)
        };
        Self {
            original: None,
            preview: None,
            source_bytes: None,
            source_is_raw: false,
            raw_orientation: 1,
            camera_white_balance: None,
            source_name: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            path: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            directory: ".".into(),
            #[cfg(not(target_arch = "wasm32"))]
            entries: Vec::new(),
            recipe: EditRecipe::default(),
            texture: None,
            // R2-GUIMOD-02: no CPU pixels uploaded yet (see `texture_identity`).
            #[cfg(not(target_arch = "wasm32"))]
            texture_identity: None,
            #[cfg(not(target_arch = "wasm32"))]
            preview_generation: 0,
            status: Str::ReadyForImage.t().into(),
            error: None,
            render_key: None,
            tone_analysis: None,
            render_mask_layers: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            document: None,
            #[cfg(not(target_arch = "wasm32"))]
            virtual_copy_id: "vc-original".into(),
            #[cfg(not(target_arch = "wasm32"))]
            selected_mask_id: None,
            #[cfg(not(target_arch = "wasm32"))]
            mask_name_input: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            mask_tool: MaskTool::None,
            #[cfg(not(target_arch = "wasm32"))]
            brush_radius: 0.05,
            #[cfg(not(target_arch = "wasm32"))]
            brush_eraser: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_brush_marks: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            drag_start: None,
            #[cfg(not(target_arch = "wasm32"))]
            drag_current: None,
            #[cfg(not(target_arch = "wasm32"))]
            drawing: false,
            preset_name: String::new(),
            preset_fields: BTreeMap::from([
                ("exposure".into(), true),
                ("contrast".into(), true),
                ("highlights".into(), false),
                ("shadows".into(), false),
            ]),
            preset_relative_exposure: false,
            #[cfg(not(target_arch = "wasm32"))]
            presets_dir: presets::default_presets_dir(),
            // F-009: initial directory scan so saved presets survive restarts.
            // A scan error surfaces through the entry list, never silently.
            #[cfg(not(target_arch = "wasm32"))]
            preset_entries: presets::default_presets_dir()
                .as_deref()
                .map(presets::scan_presets_dir)
                .unwrap_or_default(),
            idle_queue: IdleQueue::new(32),
            #[cfg(not(target_arch = "wasm32"))]
            thumbnail_tx,
            #[cfg(not(target_arch = "wasm32"))]
            thumbnail_rx,
            active_module: Module::Develop,
            #[cfg(not(target_arch = "wasm32"))]
            export_path: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            export_format: ImageFileFormat::Png,
            #[cfg(not(target_arch = "wasm32"))]
            export_quality: 90,
            before_after: false,
            wb_pick_mode: false,
            #[cfg(not(target_arch = "wasm32"))]
            thumbnails: ThumbnailManager::new(),
            preview_is_draft: false,
            draft_original: None,
            base_stage_cache: StageFrameCache::new(BASE_STAGE_CACHE_MAX_BYTES),
            source_hash_memo: None,
            last_stage_work: None,
            draft_max_dim: 1280,
            last_edit_time: 0.0,
            preview_zoom: 1.0,
            zoom_mode: ZoomMode::Fit,
            preview_pan: egui::Vec2::ZERO,
            preview_roi: None,
            preview_base_fit_scale: 1.0,
            preview_pane_w: 800.0,
            preview_pane_h: 600.0,
            preview_src_w: 1.0,
            preview_src_h: 1.0,
            preview_effective_scale: 1.0,
            // Navigator defaults to collapsed (hidden) and is revealed via the
            // "Navigator" toggle button in the preview toolbar (Lightroom-like).
            #[cfg(not(target_arch = "wasm32"))]
            navigator_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            open_folders: BTreeSet::new(),
            #[cfg(not(target_arch = "wasm32"))]
            folder_children: BTreeMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            folder_raw_counts: BTreeMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            library_thumb_size: 132.0,
            #[cfg(not(target_arch = "wasm32"))]
            history_selected: None,
            #[cfg(not(target_arch = "wasm32"))]
            decode_rx: None,
            #[cfg(not(target_arch = "wasm32"))]
            sidecar_revision: None,
            pending_full_render: false,
            #[cfg(not(target_arch = "wasm32"))]
            auto_load_attempted: false,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            // R2-GUIMOD-09: deliberately `None` here. Constructing a standalone
            // `GpuContext` performs a blocking adapter/device request that
            // `attach_wgpu_render_state` immediately replaced with the
            // renderer-shared context — two full GPU inits per startup. The
            // context is now created exactly once, inside
            // [`attach_wgpu_render_state`] (native entry point wires it right
            // after construction; headless tests stay GPU-free).
            gpu: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            wgpu_render_state: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            present_target: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            vram_fresh: false,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            gpu_stage_gate: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            vram_mask_is_evaluated: false,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            gpu_present_frame: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            // R2-GUIMOD-06: no routing fallback until a present decision runs.
            gpu_route_fallback: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            brush_mask_plane: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            brush_mask_plane_dims: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_thumb_enqueued: 0,
            #[cfg(not(target_arch = "wasm32"))]
            frame_thumbs_ready: 0,
            #[cfg(not(target_arch = "wasm32"))]
            // PREVIEW-CACHE-FEATURE: lazy — no worker pool until the first
            // neighbor prefetch (keeps headless tests thread-free).
            preview_ctrl: None,
            #[cfg(not(target_arch = "wasm32"))]
            frame_previews_enqueued: 0,
            #[cfg(not(target_arch = "wasm32"))]
            frame_previews_ready: 0,
        }
    }

    pub fn recipe(&self) -> &EditRecipe {
        &self.recipe
    }

    /// Monotonic counter of how many times `self.preview` received new content
    /// (bumped in `render_from`). Exposed read-only for headless integration
    /// tests (F-103-N9 interaction tests) to assert that an edit re-renders.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_generation(&self) -> u64 {
        self.preview_generation
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_file(&mut self, path: impl Into<String>) {
        let p = path.into();
        trace!("GUI interaction: open_file {}", p);
        // REVIEW-GUI-PATHDESYNC-1: `self.path` is NOT committed here. The decode
        // runs asynchronously; adopting the new path before `finish_decode`
        // would let Save Recipe / Export / mask fingerprints write the still-
        // loaded image-A state under the new path B (phantom sidecar) — and on
        // a failed decode the path would point at a file that never loaded.
        // `finish_decode` commits the path only after a successful decode, so
        // every write path stays consistent with original/document/recipe.
        // Populate the file browser with the directory containing the opened file.
        if let Some(parent) = Path::new(&p).parent() {
            self.directory = parent.display().to_string();
        }
        self.list_directory();
        // PERF-GUI-7: decode off the main thread so switching files never
        // blocks the UI; the decoded frame is delivered via `decode_rx` and
        // applied in `update()`/`poll_decode()`.
        self.begin_load_path(p);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_directory(&mut self, directory: impl Into<String>) {
        self.directory = directory.into();
        info!("directory set: {}", self.directory);
        self.list_directory();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn list_directory(&mut self) {
        let directory = std::path::PathBuf::from(self.directory.trim());
        debug!("listing directory: {}", directory.display());
        // REVIEW-GUI-THUMB-1: drop cached thumbnails of a previous folder so
        // they neither resurface nor accumulate unboundedly across a session.
        self.thumbnails
            .ensure_directory(&directory.to_string_lossy());
        // PREVIEW-CACHE-FEATURE: a *directory change* invalidates the neighbor
        // cache state (RAM LRU, in-flight, failures) — stale entries of another
        // folder must neither resurface nor ever be shown. Relisting the same
        // directory during navigation keeps the warm LRU so a change-of-active
        // is served as an instant cache hit (A1).
        if let Some(ctrl) = self.preview_ctrl.as_mut() {
            ctrl.ensure_directory(self.directory.trim());
        }
        self.entries.clear();
        match std::fs::read_dir(&directory) {
            Ok(dir_entries) => {
                for entry in dir_entries.flatten() {
                    if let Some(scanned) = Self::scan_entry(&entry.path()) {
                        self.entries.push(scanned);
                    }
                }
                // Also pick up orphan sidecars whose source file is missing.
                // After deleting the source, read_dir won't list it, but the
                // .lumina.json sidecar still exists on disk.
                if let Ok(sidecar_entries) = std::fs::read_dir(&directory) {
                    for entry in sidecar_entries.flatten() {
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.ends_with(".lumina.json") {
                                if let Some(source_name) = name.strip_suffix(".lumina.json") {
                                    let source_path = directory.join(source_name);
                                    if !self.entries.iter().any(|e| e.path == source_path) {
                                        if let Some(scanned) = Self::scan_entry(&source_path) {
                                            self.entries.push(scanned);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                self.entries.sort_by(|a, b| a.name.cmp(&b.name));
                self.status = Str::ImagesInDirectory.format_arg(&self.entries.len().to_string());
                // PERF-GUI-6: when no specific file was requested (e.g. the user
                // picked a directory, not a single image) and nothing is loaded
                // yet, auto-load the first RAW entry so the Develop module shows
                // an image immediately — no manual click required.
                //
                // Robustness guards:
                // * `!self.auto_load_attempted` — run the auto-load at most once
                //   per session so rescanning the directory never restarts a
                //   decode that is already in flight.
                // * `self.decode_rx.is_none()` — a decode is already pending
                //   (async), so we must not start a second one; `original` stays
                //   `None` until the in-flight decode's `finish_decode` runs.
                // * `is_raw_name(&e.name)` — RAW-only, so jpg/png/webp never
                //   enter the Develop preview.
                // If no RAW entry exists yet we deliberately leave
                // `auto_load_attempted` unset so a later, now-populated scan can
                // still auto-load.
                if !self.auto_load_attempted
                    && self.path.is_empty()
                    && self.original.is_none()
                    && self.decode_rx.is_none()
                {
                    if let Some(first) = self
                        .entries
                        .iter()
                        .find(|e| is_raw_name(&e.name))
                        .map(|e| e.path.clone())
                    {
                        debug!(
                            "auto-loading first raw entry after list_directory: {}",
                            first.display()
                        );
                        self.begin_load_path(first.display().to_string());
                        self.auto_load_attempted = true;
                    }
                }
            }
            Err(error) => {
                self.status = Str::DirectoryNotReadable.format_arg(&error.to_string());
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn entries(&self) -> &[FileBrowserEntry] {
        &self.entries
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_entry(path: &Path) -> Option<FileBrowserEntry> {
        if !is_supported_image(path) {
            return None;
        }
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        let has_sidecar = sidecar_path.is_file();
        let mut virtual_copies = 0usize;
        let mut missing_models = 0usize;
        let source_status = if path.is_file() {
            match lumina_sidecar::load_sidecar(&sidecar_path) {
                Ok(document) => {
                    virtual_copies = document.virtual_copies.len();
                    let bundle_root = path.parent().unwrap_or_else(|| Path::new("."));
                    for copy in &document.virtual_copies {
                        for mask in &copy.mask_library {
                            let artifact_missing = mask.artifact.as_ref().is_some_and(|artifact| {
                                lumina_sidecar::artifact_status(bundle_root, artifact)
                                    != ArtifactStatus::Available
                            });
                            if matches!(
                                mask.status,
                                MaskStatus::Missing
                                    | MaskStatus::Pending
                                    | MaskStatus::Stale
                                    | MaskStatus::Corrupt
                            ) || artifact_missing
                            {
                                missing_models += 1;
                            }
                        }
                    }
                    lumina_sidecar::source_status(path, &document.source)
                        .unwrap_or(SourceStatus::Unchanged)
                }
                Err(_) => SourceStatus::Unchanged,
            }
        } else {
            SourceStatus::Missing
        };
        let conflict = has_sidecar && !matches!(source_status, SourceStatus::Unchanged);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        Some(FileBrowserEntry {
            path: path.to_path_buf(),
            name,
            thumb_key: thumbnail_key(path),
            has_sidecar,
            source_status,
            conflict,
            virtual_copies,
            missing_models,
        })
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn idle_queue(&self) -> &IdleQueue {
        &self.idle_queue
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn preview(&self) -> Option<&ImageFrame> {
        self.preview.as_ref()
    }
    /// REVIEW-GUI-N5: whether the current preview is a low-resolution draft
    /// (slider drag in flight). Consumers of the preview pixels (histogram,
    /// exposure matching) must check this so a draft is never silently
    /// measured as if it were the final render.
    pub fn preview_is_draft(&self) -> bool {
        self.preview_is_draft
    }
    pub fn render_key(&self) -> Option<&RenderKey> {
        self.render_key.as_ref()
    }

    /// PERF-GUI-1: number of cached base-stage frames (diagnostics/tests).
    pub fn base_stage_cache_len(&self) -> usize {
        self.base_stage_cache.len()
    }

    /// PERF-GUI-1: drops every cached base-stage frame. Pure memory-pressure
    /// hygiene — the next render rebuilds the base from the decoded source,
    /// which changes no pixels (cache-miss is a performance event, never a
    /// fallback).
    pub fn clear_preview_stage_cache(&mut self) {
        self.base_stage_cache.clear();
    }

    /// PERF-GUI-1: stage work counters of the last completed render.
    pub fn last_stage_work(&self) -> Option<StageWork> {
        self.last_stage_work
    }
    pub fn tone_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        self.tone_analysis
    }

    pub fn create_preset(&self, name: impl Into<String>) -> Result<Preset, GuiError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::PresetNameEmpty.t().to_string()));
        }
        let mut recipe = EditRecipe::default();
        for (field, selected) in &self.preset_fields {
            if *selected {
                if let Some(value) = self.recipe.adjustments.get(field) {
                    recipe.adjustments.insert(field.clone(), *value);
                }
            }
        }
        if self.preset_relative_exposure {
            if !self.recipe.auto_features.enable_auto_tone {
                return Err(GuiError::Io(
                    Str::RelativeExposureRequiresAutoTone.t().to_string(),
                ));
            }
            recipe
                .options
                .insert("exposure_semantics".into(), "relative".into());
        } else {
            recipe
                .options
                .insert("exposure_semantics".into(), "absolute".into());
        }
        Ok(Preset {
            id: format!("preset-{}", blake3::hash(name.as_bytes()).to_hex()),
            name,
            recipe,
            extras: BTreeMap::new(),
        })
    }

    pub fn apply_preset(&mut self, preset: &Preset) -> Result<(), GuiError> {
        trace!("GUI interaction: apply_preset {}", preset.name);
        if preset
            .recipe
            .options
            .get("exposure_semantics")
            .map(String::as_str)
            == Some("relative")
            && !self.recipe.auto_features.enable_auto_tone
        {
            return Err(GuiError::Io(
                Str::RelativeExposureRequiresAutoTone.t().to_string(),
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let previous = self.recipe.clone();
        for (key, value) in &preset.recipe.adjustments {
            let value = if key == "exposure"
                && preset
                    .recipe
                    .options
                    .get("exposure_semantics")
                    .map(String::as_str)
                    == Some("relative")
            {
                self.recipe.adjustments.get(key).copied().unwrap_or(0.0) + value
            } else {
                *value
            };
            self.recipe.adjustments.insert(key.clone(), value);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = &mut self.document {
            if let Some(copy) = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == self.virtual_copy_id)
            {
                let id = format!("history-{}", copy.history.len() + 1);
                copy.history.push(HistoryEntry {
                    id,
                    recipe: previous,
                    recorded_at: None,
                    extras: BTreeMap::new(),
                });
            }
        }
        self.render()
    }

    /// Lightroom-style non-destructive history restore: copies the stored
    /// recipe state of a history step of the active virtual copy into the
    /// session recipe and re-renders. Nothing is persisted until the user
    /// presses Save Recipe / Sidecar.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn restore_history(&mut self, entry_id: &str) -> Result<(), GuiError> {
        let document = self
            .document
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
        let recipe = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .and_then(|copy| copy.history.iter().find(|entry| entry.id == entry_id))
            .map(|entry| entry.recipe.clone())
            .ok_or_else(|| GuiError::Io(Str::HistoryEntryMissing.t().to_string()))?;
        trace!("GUI interaction: history restore {}", entry_id);
        self.history_selected = Some(entry_id.to_string());
        self.recipe = recipe;
        self.render()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn duplicate_virtual_copy(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<(), GuiError> {
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        document.duplicate_virtual_copy(&self.virtual_copy_id, id, name)?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Switch the active virtual copy (REVIEW-GUI-VCSWITCH-1).
    ///
    /// Switching adopts the target copy's stored recipe. Session state that
    /// belonged to the previous copy is reset: the history selection and any
    /// in-progress mask-tool gesture. Unsaved edits of the previous copy are
    /// **discarded** by design (the stored recipe is authoritative); this is
    /// made visible through a distinct status message plus a `warn!` log —
    /// never silently.
    ///
    /// Errors (no sidecar, unknown id) are returned to the caller; UI call
    /// sites must surface them via `show_error` instead of discarding them.
    pub fn select_virtual_copy(&mut self, id: &str) -> Result<(), GuiError> {
        let Some(document) = &self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        // Dirty check against the copy we are leaving, BEFORE adopting the new
        // recipe.
        let discarded_unsaved = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .is_some_and(|previous| previous.recipe != self.recipe);
        let previous_id = self.virtual_copy_id.clone();
        self.virtual_copy_id = copy.id.clone();
        self.recipe = copy.recipe.clone();
        self.selected_mask_id = copy
            .mask_layers
            .first()
            .map(|layer| layer.mask.mask_id.clone());
        // Per-copy session state resets (REVIEW-GUI-VCSWITCH-1): a history
        // selection or an in-progress drag of the previous copy must never
        // leak into the newly selected one.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.history_selected = None;
            self.pending_brush_marks.clear();
            self.drag_start = None;
            self.drag_current = None;
            self.drawing = false;
        }
        if discarded_unsaved {
            warn!(
                "virtual-copy switch from `{previous_id}` to `{}` discarded unsaved edits",
                self.virtual_copy_id
            );
        }
        // The status is set *after* `render` because a successful render
        // overwrites it ("Preview current"); on a render failure the error
        // path keeps its own visible state.
        let outcome = self.render();
        if outcome.is_ok() {
            self.status = if discarded_unsaved {
                format!(
                    "Switched to copy `{}` — unsaved edits of `{previous_id}` were discarded",
                    self.virtual_copy_id
                )
            } else {
                format!("Switched to copy `{}`", self.virtual_copy_id)
            };
        }
        outcome
    }

    /// Select a mask from the active copy's library and make it the active layer.
    /// The matte is only referenced; no payload is copied or modified.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn select_mask(&mut self, mask_id: &str) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let document = self.document.as_mut().expect("document was ensured");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
            return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
        }
        if let Some(layer) = copy.mask_layers.first_mut() {
            layer.mask = MaskReference {
                copy_id: copy.id.clone(),
                mask_id: mask_id.into(),
                extras: BTreeMap::new(),
            };
        } else {
            copy.mask_layers.push(MaskLayer {
                id: "layer-1".into(),
                mask: MaskReference {
                    copy_id: copy.id.clone(),
                    mask_id: mask_id.into(),
                    extras: BTreeMap::new(),
                },
                inverted: false,
                feather: 0.0,
                blur: 0.0,
                density: 1.0,
                extras: BTreeMap::new(),
            });
        }
        self.selected_mask_id = Some(mask_id.into());
        self.render_key = None;
        self.status = Str::MaskSelected.format_arg(mask_id);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn selected_mask_id(&self) -> Option<&str> {
        self.selected_mask_id.as_deref()
    }

    /// Create a pending library entry. Inference is deliberately not started here.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_mask(&mut self, name: impl Into<String>) -> Result<String, GuiError> {
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let id = format!("mask-{}", blake3::hash(name.as_bytes()).to_hex());
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        let source_hash = self
            .source_bytes
            .as_ref()
            .map(|b| format!("blake3:{}", blake3::hash(b).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        let source_byte_length = self.source_bytes.as_ref().map_or(0, |b| b.len() as u64);
        let document = self.document.as_mut().expect("document was ensured");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if copy.mask_library.iter().any(|mask| mask.id == id) {
            return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
        }
        copy.mask_library.push(MaskDefinition {
            id: id.clone(),
            name,
            source_fingerprint: SourceFingerprint {
                content_hash: source_hash,
                byte_length: source_byte_length,
                extras: BTreeMap::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "pending".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_context: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            model: ModelIdentity {
                name: "unavailable".into(),
                version: "pending".into(),
                hash: "pending".into(),
                extras: BTreeMap::new(),
            },
            inference_resolution: Resolution {
                width: frame.width,
                height: frame.height,
                extras: BTreeMap::new(),
            },
            preprocessing: Preprocessing {
                name: "pending".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Pending,
            created_at: "pending".into(),
            generator_version: env!("CARGO_PKG_VERSION").into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            extras: BTreeMap::new(),
        });
        self.select_mask(&id)?;
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rename_mask(&mut self, mask_id: &str, name: impl Into<String>) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let copy = self.active_copy_mut()?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.name = name;
        self.status = Str::MaskRenamed.t().into();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_inverted(&mut self, inverted: bool) -> Result<(), GuiError> {
        let layer = self.active_layer_mut()?;
        layer.inverted = inverted;
        // REVIEW-GUI-MASKRENDER-1: layer edits change the evaluated matte, so
        // the preview must actually re-render — route through `mark_dirty`
        // (which also schedules the debounced render), not just invalidate
        // the key.
        self.mark_dirty();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_feather(&mut self, feather: f32) -> Result<(), GuiError> {
        if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
            return Err(GuiError::Io(Str::FeatheringMustBeBetween.t().to_string()));
        }
        self.active_layer_mut()?.feather = feather;
        // REVIEW-GUI-MASKRENDER-1: see `set_mask_inverted`.
        self.mark_dirty();
        Ok(())
    }

    /// Store a local adjustment as declarative layer metadata. Applying it to pixels
    /// requires the not-yet-implemented masked core pipeline; it is never baked in.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_local_adjustment(&mut self, key: &str, value: f64) -> Result<(), GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") || !value.is_finite()
        {
            return Err(GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()));
        }
        self.active_layer_mut()?
            .extras
            .insert(format!("adjustment_{key}"), Value::from(value));
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn offer_mask_recalculation(&mut self) -> Result<bool, GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        let offered = !matches!(mask.status, MaskStatus::Valid);
        self.status = if offered {
            Str::MaskStaleRecalc.t()
        } else {
            Str::MaskCurrentNoRecalc.t()
        }
        .into();
        Ok(offered)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn mark_mask_for_recalculation(&mut self) -> Result<(), GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.status = MaskStatus::Pending;
        mask.error_text = Some(Str::ExplicitRecalcRequested.t().to_string());
        let queued = self.idle_queue.enqueue(
            IdleTask::MaskInference {
                mask_id: mask_id.clone(),
            },
            100,
        );
        if queued.is_none() {
            return Err(GuiError::Io(Str::IdleQueueFull.t().to_string()));
        }
        self.status = Str::RecalcRequested.t().into();
        Ok(())
    }

    // ---- F-103-N4: interactive mask tools (Brush / Linear / Radial) ----

    /// Visible reason shown when a source-coordinate tool is refused because
    /// active recipe geometry changes the mapping between the displayed
    /// (post-geometry) preview and the source frame (REVIEW-GUI-MASKGEO-1).
    #[cfg(not(target_arch = "wasm32"))]
    const GEOMETRY_TOOL_BLOCKED: &str = "Mask and white-balance tools are unavailable while Crop, Rotation, Mirror or Perspective is active — drawn/picked source coordinates would land transformed-wrong. Reset the geometry to use them.";

    /// True while recipe geometry changes the mapping between the displayed
    /// (post-geometry) preview frame and the un-decoded source frame
    /// (REVIEW-GUI-MASKGEO-1).
    ///
    /// The interactive tools map pointer positions to *source* coordinates
    /// (`to_normalized` + ROI). With `Crop`/`rotation`/mirroring — and equally
    /// with a non-neutral `Perspective`, which changes the output bounds — that
    /// mapping is no longer identity, so brush/gradient/radial prompts and WB
    /// picks would silently land at transformed-wrong positions. Until the
    /// core applies geometry to mask planes (documented F-041 alignment limit)
    /// the honest behaviour is to refuse these tools visibly instead of
    /// writing wrong data.
    #[cfg(not(target_arch = "wasm32"))]
    fn geometry_blocks_source_mapping(&self) -> bool {
        let geometry_active = self.recipe.geometry.as_ref().is_some_and(|g| {
            g.crop.is_some()
                || g.rotation_degrees.abs() > f32::EPSILON
                || g.mirror_horizontal
                || g.mirror_vertical
        });
        let perspective_active = self.recipe.perspective.as_ref().is_some_and(|p| {
            p.vertical != 0.0
                || p.horizontal != 0.0
                || p.rotation != 0.0
                || p.scale != 1.0
                || p.aspect_ratio != 1.0
                || p.shift_x != 0.0
                || p.shift_y != 0.0
        });
        geometry_active || perspective_active
    }

    /// Arm or disarm an interactive masking tool. Disarming returns the preview
    /// to its ordinary click/eyedropper behaviour and cancels any in-progress
    /// drag.
    ///
    /// REVIEW-GUI-MASKGEO-1: arming is refused (visibly, tool stays `None`)
    /// while recipe geometry is active — see
    /// [`Self::geometry_blocks_source_mapping`]. No silent fallback into
    /// transformed-wrong marks.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_tool(&mut self, tool: MaskTool) {
        if tool != MaskTool::None && self.geometry_blocks_source_mapping() {
            warn!("mask tool {tool:?} refused while recipe geometry is active");
            self.mask_tool = MaskTool::None;
            self.pending_brush_marks.clear();
            self.drag_start = None;
            self.drag_current = None;
            self.drawing = false;
            self.status = Self::GEOMETRY_TOOL_BLOCKED.into();
            return;
        }
        self.mask_tool = tool;
        self.pending_brush_marks.clear();
        self.drag_start = None;
        self.drag_current = None;
        self.drawing = false;
    }

    /// Set the normalized brush radius. Rejected (no state change) if not finite
    /// or outside the open-closed `(0, 1]` range.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_brush_radius(&mut self, radius: f32) -> Result<(), GuiError> {
        if !radius.is_finite() || !(0.0..=1.0).contains(&radius) || radius <= 0.0 {
            return Err(GuiError::Io(
                "Brush radius must be finite and within (0, 1]".into(),
            ));
        }
        self.brush_radius = radius;
        Ok(())
    }

    /// Toggle the brush eraser (negative) sign.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_brush_eraser(&mut self, eraser: bool) {
        self.brush_eraser = eraser;
    }

    /// Set the blur of the selected mask layer (0..=1).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_blur(&mut self, blur: f32) -> Result<(), GuiError> {
        if !blur.is_finite() || !(0.0..=1.0).contains(&blur) {
            return Err(GuiError::Io("Blur must be between 0 and 1".into()));
        }
        self.active_layer_mut()?.blur = blur;
        // REVIEW-GUI-MASKRENDER-1: see `set_mask_inverted`.
        self.mark_dirty();
        Ok(())
    }

    /// Set the density of the selected mask layer (0..=1).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_mask_density(&mut self, density: f32) -> Result<(), GuiError> {
        if !density.is_finite() || !(0.0..=1.0).contains(&density) {
            return Err(GuiError::Io("Density must be between 0 and 1".into()));
        }
        self.active_layer_mut()?.density = density;
        // REVIEW-GUI-MASKRENDER-1: see `set_mask_inverted`.
        self.mark_dirty();
        Ok(())
    }

    /// Returns the active virtual copy's source dimensions, used as the brush
    /// prompt resolution and overlay rasterization size.
    #[cfg(not(target_arch = "wasm32"))]
    fn image_dims(&self) -> Result<(u32, u32), GuiError> {
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        Ok((frame.width, frame.height))
    }

    /// Ensure a mask is selected; create a default one if the active copy has
    /// none yet so a drawn prompt always has a home.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_selected_mask(&mut self) -> Result<String, GuiError> {
        if let Some(id) = self.selected_mask_id.clone() {
            return Ok(id);
        }
        let count = self
            .document
            .as_ref()
            .and_then(|d| {
                d.virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .map_or(0, |c| c.mask_library.len());
        self.create_mask(format!("Mask {}", count + 1))
    }

    /// Persist a finished [`MaskPrompt`] onto the selected mask and write the
    /// sidecar. A hand-drawn prompt mask is complete without a model — the
    /// geometric rasterizer (F-079) supplies the matte — so it is marked
    /// `Valid` (the file browser would otherwise report a phantom "missing
    /// model"). No silent fallback: a missing sidecar/document is a hard error.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_mask_prompt(&mut self, prompt: MaskPrompt) -> Result<(), GuiError> {
        let mask_id = self.ensure_selected_mask()?;
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|mask| mask.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.prompt = Some(prompt);
        mask.status = MaskStatus::Valid;
        mask.error_text = None;
        self.render_key = None;
        self.save_sidecar();
        self.status = Str::MaskPromptSaved.format_arg(&mask_id);
        Ok(())
    }

    /// Finalize a brush stroke. An empty stroke is a hard error and writes
    /// nothing (no silent fallback). Every mark is validated against the F-079
    /// prompt rules before persistence.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_brush_stroke(&mut self, marks: Vec<BrushMark>) -> Result<(), GuiError> {
        if marks.is_empty() {
            return Err(GuiError::Io("A brush mask needs at least one mark".into()));
        }
        for mark in &marks {
            if !mark.x.is_finite()
                || !mark.y.is_finite()
                || !mark.radius.is_finite()
                || !(0.0..=1.0).contains(&mark.x)
                || !(0.0..=1.0).contains(&mark.y)
                || !(0.0..=1.0).contains(&mark.radius)
                || mark.radius <= 0.0
            {
                return Err(GuiError::Io(
                    "Brush marks must have finite normalized coordinates within 0..=1 and a positive radius".into(),
                ));
            }
        }
        let (w, h) = self.image_dims()?;
        let prompt = MaskPrompt::Brush {
            marks,
            resolution: (w, h),
            transformation: PromptTransform::default(),
        };
        self.apply_mask_prompt(prompt)
    }

    /// Build a linear-gradient prompt from a drag (start→end, normalized 0..=1).
    ///
    /// Behaviour (documented): both endpoints are clamped to `0..=1` before use,
    /// so a drag that leaves the image still yields a well-defined angle from
    /// the clamped segment. The drag *direction* sets `angle_deg`
    /// (`atan2(dy, dx)`, normalized to `[0, 360)`); `start`/`end` are the matte
    /// values (1.0 → 0.0) along that axis, matching the F-079 geometric
    /// rasterizer (`start` + t·(end−start) across the normalized projection).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn gradient_prompt_from_drag(a: Point2, b: Point2) -> MaskPrompt {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        let dx = (b.x - a.x) as f64;
        let dy = (b.y - a.y) as f64;
        let mut angle = dy.atan2(dx).to_degrees();
        if angle < 0.0 {
            angle += 360.0;
        }
        MaskPrompt::Gradient {
            angle_deg: angle as f32,
            start: 1.0,
            end: 0.0,
            transformation: PromptTransform::default(),
        }
    }

    /// Finalize a linear-gradient drag. A zero-length drag (the two endpoints
    /// coincide within tolerance) is rejected.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_gradient(&mut self, a: Point2, b: Point2) -> Result<(), GuiError> {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        if (b.x - a.x).abs() < 1e-4 && (b.y - a.y).abs() < 1e-4 {
            return Err(GuiError::Io("Drag a gradient across the image".into()));
        }
        self.apply_mask_prompt(Self::gradient_prompt_from_drag(a, b))
    }

    /// Build a radial-gradient (ellipse) prompt from a drag. The drag defines
    /// the ellipse bounding box: `center` is the segment midpoint and `radii`
    /// are half the absolute (clamped) deltas, clamped to `(0, 1]`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ellipse_prompt_from_drag(a: Point2, b: Point2) -> MaskPrompt {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        let center = Point2 {
            x: (a.x + b.x) / 2.0,
            y: (a.y + b.y) / 2.0,
        };
        let rx = ((b.x - a.x).abs() / 2.0).clamp(1e-3, 1.0);
        let ry = ((b.y - a.y).abs() / 2.0).clamp(1e-3, 1.0);
        MaskPrompt::Ellipse {
            center,
            radii: Point2 { x: rx, y: ry },
            transformation: PromptTransform::default(),
        }
    }

    /// Finalize a radial-gradient drag. A zero-size drag (both radii below
    /// tolerance) is rejected.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_radial(&mut self, a: Point2, b: Point2) -> Result<(), GuiError> {
        let rx = ((b.x - a.x).abs() / 2.0).clamp(1e-3, 1.0);
        let ry = ((b.y - a.y).abs() / 2.0).clamp(1e-3, 1.0);
        if rx < 1e-4 && ry < 1e-4 {
            return Err(GuiError::Io("Drag a radial mask across the image".into()));
        }
        self.apply_mask_prompt(Self::ellipse_prompt_from_drag(a, b))
    }

    /// Finish the in-progress mask-tool drag, dispatching to the right commit
    /// based on the active tool. Errors are surfaced as visible [`GuiError`]s.
    #[cfg(not(target_arch = "wasm32"))]
    fn finish_drawing(&mut self) {
        let tool = self.mask_tool;
        let start = self.drag_start;
        let end = self.drag_current;
        let marks = std::mem::take(&mut self.pending_brush_marks);
        self.drawing = false;
        self.drag_start = None;
        self.drag_current = None;
        let result = match tool {
            MaskTool::None => return,
            MaskTool::Brush => self.commit_brush_stroke(marks),
            MaskTool::LinearGradient => match (start, end) {
                (Some(a), Some(b)) => self.commit_gradient(a, b),
                _ => Ok(()),
            },
            MaskTool::Radial => match (start, end) {
                (Some(a), Some(b)) => self.commit_radial(a, b),
                _ => Ok(()),
            },
        };
        if let Err(error) = result {
            self.show_error(error);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_document_loaded(&mut self) -> Result<(), GuiError> {
        if self.document.is_none() {
            let frame = self
                .original
                .as_ref()
                .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
                .clone();
            self.document = Some(SidecarDocument::new(
                self.source_identity(&frame),
                "raster-mvp-1",
            ));
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn active_copy_mut(&mut self) -> Result<&mut lumina_sidecar::VirtualCopy, GuiError> {
        self.document
            .as_mut()
            .and_then(|d| {
                d.virtual_copies
                    .iter_mut()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn active_layer_mut(&mut self) -> Result<&mut MaskLayer, GuiError> {
        self.active_copy_mut()?
            .mask_layers
            .first_mut()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_adjustment_to_selection(
        paths: &[std::path::PathBuf],
        key: &str,
        value: f64,
    ) -> Result<usize, GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") {
            return Err(GuiError::Io(Str::UnknownAdjustment.format_arg(key)));
        }
        let mut changed = 0;
        for path in paths {
            let sidecar_path = lumina_sidecar::sidecar_path_for(path);
            let mut document = lumina_sidecar::load_sidecar(&sidecar_path)?;
            let Some(copy) = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.is_default)
            else {
                continue;
            };
            copy.recipe.adjustments.insert(key.into(), value);
            copy.history.push(HistoryEntry {
                id: format!("selection-{changed}"),
                recipe: copy.recipe.clone(),
                recorded_at: None,
                extras: BTreeMap::new(),
            });
            lumina_sidecar::save_sidecar(&sidecar_path, &document)?;
            changed += 1;
        }
        Ok(changed)
    }

    pub fn load_bytes(&mut self, bytes: Vec<u8>, name: impl Into<String>) -> Result<(), GuiError> {
        let name = name.into();
        let source_is_raw = is_raw_name(&name);
        let (frame, orientation, camera_white_balance) = if source_is_raw {
            let image = lumina_raw::decode_bytes(&bytes, &name)?;
            let wb = image.metadata.camera_white_balance;
            let camera_white_balance = if wb.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                warn!(
                    "As-Shot white balance invalid {:?} for {} — dropping to None (recipe WB remains, image will load)",
                    wb, name
                );
                None
            } else {
                Some(wb)
            };
            (
                image.frame,
                image.metadata.orientation,
                camera_white_balance,
            )
        } else {
            (ImageFrame::decode(&bytes)?, 1, None)
        };
        // PERF-GUI-7: shared post-decode setup (also used by the async path).
        self.apply_decoded_frame(
            &frame,
            orientation,
            camera_white_balance,
            &name,
            &bytes,
            source_is_raw,
        );
        if let Err(e) = self.render() {
            error!("render after load failed for {}: {e}", self.source_name);
            self.show_error(e);
        }
        Ok(())
    }

    /// PERF-GUI-7: shared post-decode setup used by both the synchronous
    /// `load_bytes` (byte drops / tests) and the asynchronous `finish_decode`
    /// (background file decode). Sets the source frame, clears the sidecar
    /// document, resets the recipe and — crucially — caches the draft
    /// (viewport-resolution) source once per load so draft renders during a
    /// slider drag never re-allocate (PERF-GUI-3 "zero alloc during
    /// interaction").
    fn apply_decoded_frame(
        &mut self,
        frame: &ImageFrame,
        orientation: u8,
        camera_white_balance: Option<[f32; 4]>,
        name: &str,
        bytes: &[u8],
        source_is_raw: bool,
    ) {
        self.source_name = name.to_string();
        self.source_bytes = Some(bytes.to_vec());
        self.source_is_raw = source_is_raw;
        self.raw_orientation = orientation;
        self.camera_white_balance = camera_white_balance;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.document = None;
            self.virtual_copy_id = "vc-original".into();
            self.selected_mask_id = None;
            // REVIEW-GUI-N1: a new image starts a fresh sidecar lineage.
            self.sidecar_revision = None;
            // REVIEW-GUI-N3: per-image session state must never leak from the
            // previous file into this one.
            self.history_selected = None;
            self.pending_brush_marks.clear();
            self.drag_start = None;
            self.drag_current = None;
            self.drawing = false;
        }
        // REVIEW-GUI-N3 (viewport + interaction state): a new image opens at
        // fit with no pan, no zoom ROI, no Before/After hold, no armed WB
        // eyedropper and no stale render bookkeeping — otherwise image B
        // opened in an 8× crop of image A.
        self.preview_zoom = 1.0;
        self.zoom_mode = ZoomMode::Fit;
        self.preview_pan = egui::Vec2::ZERO;
        self.preview_roi = None;
        self.before_after = false;
        self.wb_pick_mode = false;
        self.render_mask_layers.clear();
        self.render_key = None;
        self.tone_analysis = None;
        self.pending_full_render = false;
        self.last_edit_time = 0.0;
        self.original = Some(frame.clone());
        self.recipe = EditRecipe::default();
        self.error = None;
        self.preview_is_draft = false;
        // PERF-GUI-1: a new source identity invalidates every cached stage at
        // once (the coarsest invalidation level of the stage DAG). Recipe
        // changes never reach this point — they keep the base cache.
        self.base_stage_cache.clear();
        self.source_hash_memo = None;
        self.last_stage_work = None;
        // PERF-GUI-3: cache a downscaled source for fast draft renders.
        self.draft_original = Some(frame.downscale(self.draft_max_dim));
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        {
            // H1: invalidate the persistent R16 brush plane — a new source size needs
            // a fresh zeroed plane; stale dimensions would mis-align tile uploads.
            self.brush_mask_plane = None;
            self.brush_mask_plane_dims = None;
            // R2-GUI-FOLLOWUP: a source switch must not reuse VRAM/present state
            // from the previous image. `vram_fresh = false` drops any stale
            // VRAM tone result and forces a fresh full render through the
            // present gate; `gpu_stage_gate = None` clears the memoized
            // `unsupported_gpu_stages` verdict so it is recomputed against the
            // new recipe/source identity instead of serving a long-gone verdict.
            self.vram_fresh = false;
            self.gpu_stage_gate = None;
        }
        self.status = Str::Loaded.format_arg(&self.source_name);
        info!(
            "loaded image {} (raw={}, camera_white_balance={:?})",
            self.source_name, source_is_raw, self.camera_white_balance
        );
    }

    pub fn set_adjustment(&mut self, name: &str, value: f64) {
        trace!(
            "GUI interaction: set_adjustment {}={} (before render)",
            name,
            value
        );
        self.recipe.adjustments.insert(name.into(), value);
        // PERF-GUI-1 stepwise invalidation: an adjustment is downstream of the
        // base stage, so only the render identity and the derived tone panel
        // are invalidated here. The cached demosaiced base
        // (`base_stage_cache`) stays — its `CacheStage::Base` digest is
        // recipe-blind, so the next render hits it and recomputes exactly the
        // Adjustments(+geometry/masks) stages.
        self.render_key = None;
        self.tone_analysis = None;
        // Coalesce: the slider drag renders a draft live; the full render is
        // deferred to pointer release (PERF-GUI-3/4).
        self.pending_full_render = true;
        self.status = Str::ChangePending.t().into();
        self.error = None;
        // GFX-SLIDER-VRAM-FRESH: an adjustment is an edit, exactly like
        // `mark_dirty` — the VRAM tone result no longer matches the recipe and
        // must never be presented until the drag path re-renders it. `mark_dirty`
        // cleared `vram_fresh` here, but `set_adjustment` (the interactive slider
        // path) did not, so a stale VRAM frame could keep being presented after a
        // slider change that did not immediately re-run `render_to_vram` (e.g.
        // in headless tests / programmatic `set_adjustment` with no pointer drag).
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        {
            self.vram_fresh = false;
            self.vram_mask_is_evaluated = false;
        }
    }

    /// Set a single Presence field (`texture`, `clarity` or `dehaze`). The value
    /// is stored in the normative `-1..=1` domain (shown as `-100..+100`); the
    /// sidecar validation is the source of truth for the domain, so out-of-range
    /// values are stored as given and rejected on save rather than silently
    /// clamped (no silent fallback).
    pub fn set_presence(&mut self, field: &str, value: f64) {
        let mut presence = self.recipe.presence.unwrap_or(Presence {
            version: 1,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
        });
        match field {
            "texture" => presence.texture = value as f32,
            "clarity" => presence.clarity = value as f32,
            "dehaze" => presence.dehaze = value as f32,
            _ => return,
        }
        self.recipe.presence = Some(presence);
        trace!("GUI interaction: set_presence {}={}", field, value);
        self.mark_dirty();
    }

    pub fn auto_tone(&mut self) -> Result<(), GuiError> {
        let Some(frame) = &self.original else {
            return Ok(());
        };
        let config = AutoToneConfig {
            target_luminance: self.recipe.auto_features.target_luminance,
            ..Default::default()
        };
        let result = suggest_auto_tone(frame, config)?;
        self.recipe
            .adjustments
            .insert("exposure".into(), result.exposure);
        self.recipe
            .adjustments
            .insert("contrast".into(), result.contrast);
        self.recipe.auto_features.enable_auto_tone = true;
        self.recipe.auto_features.auto_exposure = Some(result.exposure);
        self.recipe.auto_features.auto_contrast = Some(result.contrast);
        self.recipe.auto_features.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint: tone_fingerprint(frame, config),
            extras: BTreeMap::new(),
        });
        self.render()
    }

    pub fn match_total_exposure(&mut self, target: f64) -> Result<(), GuiError> {
        // REVIEW-GUI-N5: never measure a draft. If the preview is currently a
        // low-resolution drag draft, commit the pending full-quality render
        // first so the measurement domain is the final visible render.
        if self.preview_is_draft {
            self.render_full([0, 0], None)?;
        }
        let Some(frame) = &self.preview else {
            return Ok(());
        };
        debug_assert!(
            !self.preview_is_draft,
            "measurement must run on the full render, never a draft"
        );
        // F-041: measure the final visible domain — the rendered preview
        // (post crop/geometry, same frame that is displayed) weighted by the
        // effective mask planes of the last render. wasm32 renders without
        // mask layers, so the empty slice keeps the raster measurement.
        let mask_planes: Vec<MaskPlane> = self
            .render_mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        let value = match_total_exposure_masked(frame, target, &mask_planes)?;
        let exposure = self
            .recipe
            .adjustments
            .get("exposure")
            .copied()
            .unwrap_or(0.0)
            + value;
        self.recipe.adjustments.insert("exposure".into(), exposure);
        self.recipe.auto_features.match_total_exposure = true;
        self.recipe.auto_features.target_luminance = target;
        self.recipe.auto_features.matched_exposure = Some(value);
        self.render()
    }

    pub fn reset(&mut self) {
        self.recipe = EditRecipe::default();
        if self.original.is_some() {
            let _ = self.render();
        }
    }

    pub fn render(&mut self) -> Result<(), GuiError> {
        self.render_full([0, 0], None)
    }

    /// Full-resolution render of the committed source. Used on load, after a
    /// slider drag settles (mouse-up / idle) and by every explicit re-render.
    /// `viewport` is the preview pane size (used for ROI clamping / logging);
    /// `roi` is an optional `(x, y, w, h)` crop (source pixels) when zoomed in.
    pub fn render_full(
        &mut self,
        _viewport: [u32; 2],
        roi: Option<[u32; 4]>,
    ) -> Result<(), GuiError> {
        let Some(original) = self.original.take() else {
            self.status = Str::NoImageLoaded.t().into();
            return Ok(());
        };
        // Derive the ROI from the zoom factor and pan offset when no explicit
        // crop was given (PERF-GUI-5, REVIEW-GUI-PANROI-1); the full render
        // always honours masks.
        let roi = roi.or_else(|| {
            Self::roi_from_zoom(
                original.width,
                original.height,
                self.preview_zoom,
                self.preview_pan,
                self.preview_pane_w,
                self.preview_pane_h,
            )
        });
        self.preview_is_draft = false;
        self.pending_full_render = false;
        let result = self.render_from(&original, true, roi);
        self.original = Some(original);
        result
    }

    /// Cheap preview render used while a slider is being dragged: renders the
    /// cached downscaled draft source (`draft_original`) at viewport resolution
    /// instead of re-processing the full 45 MP original on every pointer tick.
    /// Mask planes are skipped because they are full-resolution and would not
    /// align with the downscaled source. Falls back to the full original when no
    /// draft is cached yet. `viewport`/`roi` mirror [`Self::render_full`].
    pub fn render_draft(
        &mut self,
        _viewport: [u32; 2],
        roi: Option<[u32; 4]>,
    ) -> Result<(), GuiError> {
        // Take the pre-allocated draft source so `render_from` borrows a local
        // value rather than `self` — zero allocation while dragging. Fall back to
        // a clone of the full original only when no draft is cached yet.
        let mut took_draft = true;
        let source = if let Some(d) = self.draft_original.take() {
            d
        } else {
            match &self.original {
                Some(o) => {
                    took_draft = false;
                    o.clone()
                }
                None => {
                    self.status = Str::NoImageLoaded.t().into();
                    return Ok(());
                }
            }
        };
        let roi = roi.or_else(|| {
            Self::roi_from_zoom(
                source.width,
                source.height,
                self.preview_zoom,
                self.preview_pan,
                self.preview_pane_w,
                self.preview_pane_h,
            )
        });
        self.preview_is_draft = true;
        let result = self.render_from(&source, false, roi);
        if took_draft {
            self.draft_original = Some(source);
        }
        result
    }

    /// Compute the source ROI `(x, y, w, h)` that is visible in the preview
    /// pane for a zoom factor `> 1.0` (PERF-GUI-5, REVIEW-GUI-PANROI-1).
    ///
    /// The visible window follows the pan offset: the drawn image centre sits
    /// at `pane.center() + pan`, so the source point currently behind the pane
    /// centre is the image centre shifted by `-pan / scale` where
    /// `scale = fit(w, h) * zoom` is the on-screen scale (screen points per
    /// source pixel). The returned rect is that window expanded by
    /// [`PREVIEW_ROI_MARGIN`] so the hand tool always has off-screen content
    /// to drag without an immediate re-render, and it is clamped to the image
    /// bounds — which is what makes borders/corners reachable at any zoom.
    ///
    /// Returns `None` at fit/zoom-out or when the window already covers the
    /// whole frame, so the entire image is rendered.
    fn roi_from_zoom(
        w: u32,
        h: u32,
        zoom: f32,
        pan: egui::Vec2,
        pane_w: f32,
        pane_h: f32,
    ) -> Option<[u32; 4]> {
        if zoom <= 1.0 || w == 0 || h == 0 {
            return None;
        }
        let (w, h) = (f64::from(w), f64::from(h));
        let (pane_w, pane_h) = (f64::from(pane_w), f64::from(pane_h));
        let fit = (pane_w / w).min(pane_h / h);
        if fit <= 0.0 {
            return None;
        }
        let scale = f64::from(zoom) * fit;
        // Visible window in source pixels, with margin for panning headroom.
        let vw = pane_w / scale * PREVIEW_ROI_MARGIN;
        let vh = pane_h / scale * PREVIEW_ROI_MARGIN;
        if vw >= w || vh >= h {
            return None;
        }
        let zw = (vw.floor() as u32).clamp(1, w as u32);
        let zh = (vh.floor() as u32).clamp(1, h as u32);
        // Source point under the pane centre (window centre), clamped so the
        // window never leaves the frame.
        let cx = w / 2.0 - f64::from(pan.x) / scale;
        let cy = h / 2.0 - f64::from(pan.y) / scale;
        let x = ((cx - vw / 2.0).floor() as i64).clamp(0, (w as u32 - zw) as i64) as u32;
        let y = ((cy - vh / 2.0).floor() as i64).clamp(0, (h as u32 - zh) as i64) as u32;
        Some([x, y, zw, zh])
    }

    /// Switch the preview zoom mode. Non-`Custom` modes re-derive `preview_zoom`
    /// from the current pane each frame (so they survive resizes); switching
    /// always re-centres the pan and triggers a re-render so the ROI crop
    /// matches the on-screen zoom.
    pub fn set_zoom_mode(&mut self, mode: ZoomMode) {
        trace!("GUI interaction: set_zoom_mode {:?}", mode);
        self.zoom_mode = mode;
        self.preview_pan = egui::Vec2::ZERO;
        self.mark_dirty();
        // PREVIEW-CACHE-FEATURE (A6): a zoom-mode change to 1:1 (or back to Fit)
        // changes the neighbor preview kind/resolution; the current source's
        // cached neighbors no longer match the new key, so the +4/−2 window is
        // re-planned at the new resolution. Only native (the neighbor cache is a
        // native capability); no-op while nothing is loaded.
        #[cfg(not(target_arch = "wasm32"))]
        if !self.path.is_empty() && self.original.is_some() {
            let active = self.path.clone();
            self.schedule_neighbor_previews(&active);
        }
    }

    /// Scroll-wheel / keyboard continuous zoom (relative-to-fit multiplier).
    /// Pins the mode to `Custom` so the next frame does not re-derive
    /// `preview_zoom`.
    pub fn zoom_step(&mut self, factor: f32) {
        let next = (self.preview_zoom * factor).clamp(0.05, 32.0);
        trace!(
            "GUI interaction: zoom_step factor={:.3} -> {:.3}",
            factor,
            next
        );
        self.preview_zoom = next;
        self.zoom_mode = ZoomMode::Custom;
        self.mark_dirty();
    }

    /// Re-derive `preview_zoom` (and reset pan) for non-`Custom` modes using
    /// the pane geometry and **un-cropped source dimensions** cached by the
    /// previous [`Self::draw_preview`] (REVIEW-GUI-ZOOMLOOP-1: deriving from
    /// the ROI-cropped texture's fit scale made 100%/200%/Fit-Width wrong and
    /// oscillate frame-by-frame). Called once per frame before the render logic
    /// so the ROI crop matches the on-screen zoom even on the frame a mode
    /// button/shortcut is pressed.
    fn sync_zoom(&mut self) {
        use ZoomMode::*;
        if self.zoom_mode == Custom {
            return;
        }
        // Fit of the pane against the un-cropped source, not the current
        // (possibly ROI-cropped) texture.
        let fit = self.preview_base_fit_scale.max(1e-6);
        let src_w = self.preview_src_w.max(1.0);
        self.preview_pan = egui::Vec2::ZERO;
        self.preview_zoom = match self.zoom_mode {
            Fit => 1.0,
            OneToOne => 1.0 / fit,
            TwoHundred => 2.0 / fit,
            FitWidth => (self.preview_pane_w / src_w) / fit,
            Custom => 1.0,
        };
    }

    /// Core render used by both [`Self::render_full`] and [`Self::render_draft`].
    /// `with_masks` enables the sidecar mask planes (skipped for the draft,
    /// whose source is downscaled and therefore misaligned with full-res masks).
    /// `roi` optionally crops the source to the visible region before the render
    /// (PERF-GUI-5).
    ///
    /// PERF-GUI-1 (staged invalidation): the pipeline runs as
    /// `base → Adjustments → geometry → Masks`, and the **base stage** (post
    /// decode/source-actions/ROI-crop, pre-adjustment) is cached in RAM keyed
    /// by its recipe-blind [`CacheStage::Base`] digest. A slider change only
    /// nulls the final-render identity (`set_adjustment`/`mark_dirty`), so the
    /// next `render_from` hits that entry and re-executes exactly the stages
    /// downstream of it — the crop, the source-action head and the full-file
    /// blake3 hash are skipped on every interactive tick. A cache miss simply
    /// rebuilds the base from the decoded source (performance event, never a
    /// fallback); a new source clears the cache in [`Self::apply_decoded_frame`].
    fn render_from(
        &mut self,
        source: &ImageFrame,
        #[cfg_attr(target_arch = "wasm32", allow(unused_variables))] with_masks: bool,
        roi: Option<[u32; 4]>,
    ) -> Result<(), GuiError> {
        // PERF-GUI-5: crop to the visible ROI (when zoomed) before rendering so
        // the full frame is never processed for a magnified view.
        //
        // REVIEW-GUI-N6: the recorded `preview_roi` must describe the pixels
        // that were *actually* rendered — it feeds the pointer→source mapping
        // of the WB eyedropper / mask tools. Core's `crop_region` clamps
        // oversized rects instead of failing, so the effective (clamped) rect
        // is computed here and recorded; when the crop genuinely fails, the
        // full frame is rendered and `preview_roi` is cleared instead of
        // silently keeping the rejected request.
        let effective_roi = roi.map(|[x, y, w, h]| {
            let x = x.min(source.width.saturating_sub(1));
            let y = y.min(source.height.saturating_sub(1));
            let w = w.min(source.width - x);
            let h = h.min(source.height - y);
            [x, y, w, h]
        });
        // Identity inputs shared by the base-stage key and the final render key.
        // The source hash is memoized per loaded file (PERF-GUI-1): hashing the
        // whole RAW file used to run on EVERY preview tick.
        let source_hash = self.resolved_source_hash();
        let decode_version = if self.source_is_raw {
            lumina_raw::libraw_decode_version()
        } else {
            env!("CARGO_PKG_VERSION").into()
        };
        let copy_id = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.virtual_copy_id.clone()
            }
            #[cfg(target_arch = "wasm32")]
            {
                "vc-original".to_owned()
            }
        };
        let mask_hashes = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.document
                    .as_ref()
                    .and_then(|d| {
                        d.virtual_copies
                            .iter()
                            .find(|c| c.id == self.virtual_copy_id)
                    })
                    .map(|c| {
                        c.mask_library
                            .iter()
                            .filter_map(|m| m.artifact.as_ref().map(|a| a.checksum.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            #[cfg(target_arch = "wasm32")]
            {
                Vec::new()
            }
        };
        // Base-stage identity (recipe-blind): source identity + decoder +
        // virtual copy + ROI window + resulting frame geometry. Two recipes
        // that differ only in exposure/color share this digest, which is what
        // makes a slider drag hit the cached demosaiced base.
        let (base_w, base_h) = match effective_roi {
            Some([_, _, w, h]) => (w, h),
            None => (source.width, source.height),
        };
        let base_digest = RenderKey::new(
            source_hash.clone(),
            decode_version.clone(),
            "raster-mvp-1",
            copy_id.clone(),
            &EditRecipe::default(),
            Vec::new(),
            OutputSpec {
                profile: "sRGB".into(),
                width: base_w,
                height: base_h,
                format: "rgba8".into(),
            },
        )
        .with_base_roi(effective_roi)
        .stage_digest(CacheStage::Base);

        // ---- Base stage (cacheable head of the pipeline) ----
        let mut work = StageWork::default();
        let mut crop_failed = false;
        let base_frame = match self.base_stage_cache.get(&base_digest) {
            Some(hit) => {
                work.base_cache_hit = true;
                trace!(
                    "GUI render: base stage cache HIT ({base_w}x{base_h}, roi={effective_roi:?})"
                );
                hit
            }
            None => {
                let cropped = match effective_roi {
                    Some([x, y, w, h]) => source.crop_region(x, y, w, h).ok(),
                    None => None,
                };
                if effective_roi.is_some() && cropped.is_none() {
                    crop_failed = true;
                    warn!(
                        "ROI crop {roi:?} failed; rendering the full frame and clearing preview_roi"
                    );
                }
                let cropped_source: &ImageFrame = match &cropped {
                    Some(f) => f,
                    None => source,
                };
                let prepared = prepare_source_base(cropped_source, &[], &mut work)?;
                // Cache ONLY entries whose bytes match their digest identity
                // exactly. A (defensively handled) failed ROI crop fell back
                // to the full frame; caching it under the requested window's
                // key could later serve mismatched geometry — it stays
                // uncached instead (no silent fallback into the cache).
                if !crop_failed {
                    self.base_stage_cache
                        .insert(base_digest.clone(), prepared.clone());
                }
                work.base_cache_hit = false;
                trace!(
                    "GUI render: base stage cache MISS — rebuilt ({base_w}x{base_h}, roi={effective_roi:?})"
                );
                prepared
            }
        };

        // Mask artifact planes loaded from the optional `.lumina.zdata` sidecar
        // (native only).  Missing or unreadable zdata is not a hard error:
        // affected layers are reported through the `MaskPolicy::Warn` path.
        #[cfg(not(target_arch = "wasm32"))]
        let masks_context = if with_masks {
            let planes = self.load_mask_planes();
            match &self.document {
                Some(document) => document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .map(|_| MaskContext {
                        copies: &document.virtual_copies,
                        active_copy_id: &self.virtual_copy_id,
                        planes,
                        policy: MaskPolicy::Warn,
                    }),
                None => None,
            }
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let masks_context: Option<MaskContext<'_>> = None;
        // ---- Downstream stages: Adjustments → geometry → Masks ----
        let output = render_frame_from_base(
            base_frame,
            &RenderContext {
                recipe: &self.recipe,
                camera_white_balance: self.camera_white_balance,
                source_actions: &[],
                masks: masks_context,
                lensfun: None,
            },
            &mut work,
        )?;
        let preview = output.frame;
        let mask_warnings = output.mask_warnings;
        // REVIEW-CORE-DIGEST-WIRING: this preview key deliberately stays on the
        // neutral `RenderKey::new` defaults instead of attaching the `with_*`
        // builders, because neither builder input exists at this site:
        // - No `with_export_options`: the render target is a plain in-memory
        //   RGBA8 frame (`format: "rgba8"`) displayed as a texture; it is never
        //   encoded here, so there are no encoder parameters to identify. The
        //   core digest distinguishes that state explicitly (`None` differs
        //   from every attached `Some(_)`), and the real export path
        //   (`export_to`) re-renders from the original via `export_image`
        //   without consulting any cache keyed by this preview key.
        // - No `with_source_action_hashes`: the `RenderContext` above passes
        //   `source_actions: &[]`, so no repair-region pixels were applied and
        //   the empty hash list truthfully describes exactly these pixels.
        //   Recipe-referenced artifact checksums must not be mixed in here —
        //   that would claim repair content this frame does not contain.
        self.render_key = Some(RenderKey::new(
            source_hash,
            decode_version,
            "raster-mvp-1",
            copy_id,
            &self.recipe,
            mask_hashes,
            OutputSpec {
                profile: "sRGB".into(),
                width: preview.width,
                height: preview.height,
                format: "rgba8".into(),
            },
        ));
        self.tone_analysis = Some(analyze_tone(&preview));
        self.preview = Some(preview);
        // R2-GUIMOD-02: new preview content — any CPU-present identity cached
        // in `texture_identity` is now stale and will re-upload once.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.preview_generation += 1;
        }
        // R2-GUIMOD-01 (MVP-blocking): a completed **full-quality** CPU render
        // supersedes whatever sits in VRAM. During a drag the VRAM result was
        // rendered from the *draft* source; after mouse-up the debounced full
        // render used to leave `vram_fresh` set, so the gate went on
        // presenting the soft draft and the freshly computed sharp pixels were
        // never shown. Draft renders (which run *after* `render_to_vram` in
        // the same tick, by design feeding the VRAM present path) must keep
        // the flag — hence the `preview_is_draft` discriminator.
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        if !self.preview_is_draft {
            self.vram_fresh = false;
        }
        // Record the crop this texture represents so pointer→source mapping in
        // `draw_preview` (WB eyedropper / mask tools) stays accurate when
        // zoomed. The *effective* (clamped) rect is recorded, and only when
        // the crop actually succeeded — a failed crop fell back to the full
        // frame, and recording the rejected request would corrupt every
        // subsequent coordinate mapping (REVIEW-GUI-N6).
        self.preview_roi = if effective_roi.is_some() && !crop_failed {
            effective_roi
        } else {
            None
        };
        self.render_mask_layers = output.mask_layers;
        // GUI-WGPU-PRESENT-1 / GPU-STAGE-1: make the *pipeline-evaluated* mask
        // coverage visible in the GPU present composite by pushing the combined
        // effective planes into the VRAM mask texture. Failures are loud but
        // never break the CPU preview path.
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        {
            self.vram_mask_is_evaluated = false;
            if !self.render_mask_layers.is_empty() {
                let planes: Vec<lumina_core::MaskPlane> = self
                    .render_mask_layers
                    .iter()
                    .map(|layer| layer.plane.clone())
                    .collect();
                match lumina_gpu::combine_mask_planes(&planes) {
                    Ok(Some(combined))
                        if combined.width
                            == self.preview.as_ref().map(|p| p.width).unwrap_or(0)
                            && combined.height
                                == self.preview.as_ref().map(|p| p.height).unwrap_or(0) =>
                    {
                        if let Some(gpu) = self.gpu.as_ref() {
                            if gpu.is_available()
                                && gpu.ensure_vram(combined.width, combined.height).is_ok()
                            {
                                match gpu.upload_mask_plane(
                                    combined.width,
                                    combined.height,
                                    &combined.values,
                                ) {
                                    Ok(()) => self.vram_mask_is_evaluated = true,
                                    Err(err) => {
                                        warn!("gpu evaluated-mask upload failed: {err}");
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!("gpu evaluated-mask combination failed: {err}");
                    }
                }
            }
        }
        self.error = None;
        self.last_stage_work = Some(work);
        self.status = if !mask_warnings.is_empty() {
            let layers: Vec<&str> = mask_warnings
                .iter()
                .filter_map(|w| w.split('`').nth(1))
                .collect();
            if layers.is_empty() {
                Str::MaskUnavailable.t().to_string()
            } else {
                Str::MaskUnavailableLayer.format_arg(&layers.join(", "))
            }
        } else {
            Str::PreviewCurrent.t().to_string()
        };
        Ok(())
    }

    /// PERF-GUI-1: content hash of the currently loaded source bytes, computed
    /// at most once per loaded file. The memo is cleared together with
    /// `source_bytes` in [`Self::apply_decoded_frame`]; callers therefore never
    /// re-hash the (potentially ~50 MB) RAW file per interactive render tick.
    fn resolved_source_hash(&mut self) -> String {
        if let Some(hash) = &self.source_hash_memo {
            return hash.clone();
        }
        let hash = self
            .source_bytes
            .as_ref()
            .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        self.source_hash_memo = Some(hash.clone());
        hash
    }

    /// Loads mask artifact planes from the optional `.lumina.zdata` sidecar for
    /// the active virtual copy (native only).  Missing/unreadable zdata yields
    /// an empty map; affected layers are handled by the `MaskPolicy::Warn`
    /// path in [`render_frame`].
    ///
    /// KONSISTENZ (REVIEW-CLI-N1): tile records are addressed by the composite
    /// id [`Self::zdata_tile_record_id`] (`"{copy_id}/{mask_id}"`) so two
    /// virtual copies that happen to share a mask id never share a matte.
    /// Containers written before that convention carry the bare `mask_id`;
    /// those records stay readable through an explicitly logged legacy lookup
    /// (documented read compatibility — not a silent fallback).
    #[cfg(not(target_arch = "wasm32"))]
    fn load_mask_planes(&self) -> BTreeMap<(String, String), MaskPlane> {
        let mut planes = BTreeMap::new();
        let Some(document) = &self.document else {
            return planes;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|c| c.id == self.virtual_copy_id)
        else {
            return planes;
        };
        let zdata_path = zdata_path_for(Path::new(&self.path));
        if !zdata_path.exists() {
            return planes;
        }
        let Ok(container) = load_zdata(&zdata_path) else {
            return planes;
        };
        for mask in copy
            .mask_library
            .iter()
            .filter(|m| matches!(m.status, MaskStatus::Valid))
        {
            let tile = match container.tile(&Self::zdata_tile_record_id(&copy.id, &mask.id), 0, 0) {
                Ok(tile) => Some(tile),
                Err(_) => match container.tile(&mask.id, 0, 0) {
                    Ok(tile) => {
                        debug!(
                            "mask plane copy `{}` / mask `{}` loaded under the legacy bare \
                             mask-id zdata key",
                            copy.id, mask.id
                        );
                        Some(tile)
                    }
                    Err(_) => None,
                },
            };
            if let Some(tile) = tile {
                if let Ok(plane) = MaskPlane::new(tile.width, tile.height, tile.values) {
                    planes.insert((copy.id.clone(), mask.id.clone()), plane);
                }
            }
        }
        planes
    }

    /// Composite zdata tile-record id shared with the CLI (REVIEW-CLI-N1):
    /// `"{copy_id}/{mask_id}"`. The field order mirrors the
    /// `(copy_id, mask_id)` planes key of `MaskContext`.
    #[cfg(not(target_arch = "wasm32"))]
    fn zdata_tile_record_id(copy_id: &str, mask_id: &str) -> String {
        format!("{copy_id}/{mask_id}")
    }

    fn show_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        self.status = Str::Error.t().into();
        self.error = Some(message);
    }

    /// REVIEW-GUI-WASM-FOLLOWUP: the texture upload is driven by the native
    /// preview-area path only.
    ///
    /// R2-GUIMOD-02: the CPU upload used to run on **every** repaint —
    /// `ColorImage::from_rgba_unmultiplied` (full-frame memcpy) plus
    /// `ctx.load_texture` (full texture re-upload) even when neither the
    /// preview nor the Before/After toggle had changed (e.g. mousemoves over
    /// panels). The upload now happens only when the displayed content
    /// identity changes; the [`egui::TextureHandle`] itself is retained and
    /// updated in place (`handle.set`) so the egui texture id stays stable.
    /// Pixel output is unchanged: identical RGBA bytes, identical options.
    #[cfg(not(target_arch = "wasm32"))]
    fn update_texture(&mut self, ctx: &egui::Context) {
        // GUI-WGPU-PRESENT-1: when the wgpu renderer shares its device with
        // `lumina-gpu` and the VRAM content is fresh, present straight from
        // VRAM (overlay composite → registered user texture). No CPU readback,
        // no `ColorImage` upload. Every fallback condition below drops to the
        // historical CPU upload, which remains fully functional.
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        {
            self.gpu_present_frame = None;
            // R2-GUIMOD-06: record whether the GPU present path was taken or the
            // preview was routed to the CPU. `gpu_present_if_ready` returns the
            // texture only when every present condition (including the
            // GPU-eligible recipe check) holds; a `None` here while a GPU context
            // is bound may mean the render was silently routed to CPU.
            match self.gpu_present_if_ready() {
                Some((id, size)) => {
                    self.gpu_present_frame = Some((id, size));
                    self.gpu_route_fallback = None;
                }
                None => {
                    self.gpu_route_fallback = self.routing_fallback_reason();
                }
            }
        }
        // Before/After shows the original (never the recipe) so the toggle can
        // never mutate the recipe — it only swaps which frame is displayed.
        let frame = if self.before_after {
            self.original.as_ref()
        } else {
            self.preview.as_ref()
        };
        if let Some(frame) = frame {
            let size = [frame.width as usize, frame.height as usize];
            let identity = (self.preview_generation, self.before_after, size);
            if self.texture_identity != Some(identity) {
                // Build the full-frame image while `frame` still borrows
                // `self`; the handle mutation below needs `&mut self.texture`.
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
                if let Some(handle) = self.texture.as_mut() {
                    handle.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ctx.load_texture(
                        "lumina-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                self.texture_identity = Some(identity);
            }
        }
    }

    /// GPU-present eligibility + composite + registration (GUI-WGPU-PRESENT-1).
    ///
    /// Returns the registered texture id and pixel size when the preview can
    /// be presented readback-free this frame. Deliberately conservative: any
    /// condition that would change visible pixels beyond the documented F-043
    /// tolerance keeps the CPU present path (no silent divergence):
    /// - Before/After displays the original — never in VRAM;
    /// - zoomed ROI previews crop on the CPU — geometry must not jump;
    /// - recipes with GPU-unsupported stages would render tone-only in VRAM
    ///   (the documented GPU-STAGE-1 Restrisiko) — CPU pixels are exact;
    /// - stale VRAM (`vram_fresh == false`) after any edit **or** after any
    ///   completed full-quality CPU render (R2-GUIMOD-01);
    /// - R2-GUIMOD-01 belt-and-braces: for a non-draft preview the VRAM
    ///   dimensions must match the preview dimensions — a full-resolution CPU
    ///   result whose geometry differs from the (draft-sized) VRAM content
    ///   must win even if a stale freshness flag ever slipped through.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn gpu_present_if_ready(&mut self) -> Option<(egui::TextureId, [usize; 2])> {
        if !self.vram_fresh || self.before_after || self.preview_roi.is_some() {
            return None;
        }
        let dims = self.gpu.as_ref()?.vram_dimensions()?;
        if !self.gpu.as_ref()?.is_available() {
            return None;
        }
        // R2-GUIMOD-01: geometry cross-check (see doc above). For drafts the
        // VRAM tone output *is* the draft-source render the interactive path
        // wants to present, so the check applies only to full-quality
        // previews.
        if !self.vram_content_matches_displayed_preview(dims) {
            return None;
        }
        // The GUI binds no source-action artifacts; a recipe referencing them
        // would lose the compositing on the GPU tone-only path → CPU route.
        // R2-GUIMOD-05: memoized per render key instead of rebuilding a
        // `Vec<String>` every frame.
        if self.recipe_has_unsupported_gpu_stages() {
            return None;
        }
        let render_state = self.wgpu_render_state.clone()?;
        // Composite output+mask into our present target (GPU-GPU, no readback).
        if let Err(err) = self.ensure_present_target(&render_state, dims) {
            log::warn!("gpu present target unavailable: {err}");
            return None;
        }
        let texture = self.present_target.as_ref()?.texture.clone();
        let id = self.present_target.as_ref()?.id;
        if let Err(err) = self.gpu.as_ref()?.copy_vram_to_texture(&texture) {
            log::warn!("gpu overlay present failed: {err}");
            return None;
        }
        Some((id, [dims.0 as usize, dims.1 as usize]))
    }

    /// R2-GUIMOD-01: does the VRAM content describe the pixels currently
    /// displayed? For a **full-quality** preview only exact dimension equality
    /// proves that preview and VRAM tone result show the same crop of the same
    /// source; any mismatch keeps the (exact) CPU present path. A displayed
    /// *draft* is exempt by design: the interactive drag path presents exactly
    /// the draft-source VRAM render it just produced.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn vram_content_matches_displayed_preview(&self, dims: (u32, u32)) -> bool {
        if self.preview_is_draft {
            return true;
        }
        match self.preview.as_ref() {
            Some(preview) => preview.width == dims.0 && preview.height == dims.1,
            None => false,
        }
    }

    /// R2-GUIMOD-05: `!lumina_gpu::unsupported_gpu_stages(&self.recipe)
    /// .is_empty()`, memoized against the current render key. The verdict can
    /// only change when the recipe/source/copy identity changes — exactly what
    /// replaces the render key — so the per-frame `Vec<String>` rebuild with
    /// its `format!` allocations collapses to one call per render.
    ///
    /// While no render key exists (dirty preview) the memo is deliberately
    /// bypassed *and* not populated: the recipe may drift between edits
    /// without ever producing an intermediate key, so a `None`-keyed entry
    /// could serve a verdict for a long-gone recipe.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn recipe_has_unsupported_gpu_stages(&mut self) -> bool {
        let cached = self
            .gpu_stage_gate
            .as_ref()
            .filter(|(key, _)| Some(key) == self.render_key.as_ref())
            .map(|(_, has_unsupported)| *has_unsupported);
        match cached {
            Some(verdict) => verdict,
            None => {
                let has_unsupported = !lumina_gpu::unsupported_gpu_stages(&self.recipe).is_empty();
                // Memoize only against a concrete key (see doc above).
                self.gpu_stage_gate = self.render_key.clone().map(|key| (key, has_unsupported));
                has_unsupported
            }
        }
    }

    /// R2-GUIMOD-06: classify *why* the GPU present path was not taken this
    /// frame, so the silent GPU→CPU routing can be shown as a visible badge.
    ///
    /// Returns `Some(reason)` only when a GPU context exists **and** is usable
    /// (`is_available`) **and** the recipe cannot be fully evaluated on the VRAM
    /// tone path — i.e. the preview is being computed on the CPU for a
    /// capability reason rather than because of an editorial state (stale VRAM,
    /// Before/After toggle, zoomed ROI, or a missing present target). In all
    /// other cases there is no "fallback" to report and `None` is returned.
    ///
    /// Reuses the memoized [`Self::recipe_has_unsupported_gpu_stages`] verdict,
    /// so calling it every frame is cheap once the render key is stable.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn routing_fallback_reason(&mut self) -> Option<String> {
        let gpu = self.gpu.as_ref()?;
        if !gpu.is_available() {
            return None;
        }
        if self.recipe_has_unsupported_gpu_stages() {
            Some(Str::CpuFallbackUnsupportedStages.t().to_string())
        } else {
            None
        }
    }

    /// Create or resize the offscreen present target and keep it registered as
    /// an egui user texture with the eframe wgpu renderer.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn ensure_present_target(
        &mut self,
        render_state: &eframe::egui_wgpu::RenderState,
        dims: (u32, u32),
    ) -> Result<(), String> {
        if let Some(existing) = &self.present_target {
            if existing.dims == dims {
                return Ok(());
            }
            // Dimensions changed: free the old registration before replacing.
            render_state.renderer.write().free_texture(&existing.id);
            self.present_target = None;
        }
        let texture = lumina_gpu::shaders::create_output_texture(
            &render_state.device,
            dims.0,
            dims.1,
            "lumina-gui-present-target",
        );
        let view = texture.create_view(&eframe::wgpu::TextureViewDescriptor::default());
        let id = render_state.renderer.write().register_native_texture(
            &render_state.device,
            &view,
            eframe::wgpu::FilterMode::Linear,
        );
        self.present_target = Some(PresentTarget {
            texture,
            view,
            id,
            dims,
        });
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn source_identity(&self, frame: &ImageFrame) -> SourceIdentity {
        SourceIdentity {
            relative_name: if self.source_name.is_empty() {
                "dropped-image".into()
            } else {
                self.source_name.clone()
            },
            content_hash: self
                .source_bytes
                .as_ref()
                .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
                .unwrap_or_else(|| "blake3:unknown".into()),
            byte_length: self
                .source_bytes
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64),
            modified_at: None,
            raw_format: Path::new(&self.source_name)
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("raster")
                .to_ascii_uppercase(),
            orientation: self.raw_orientation,
            decode_fingerprint: DecodeFingerprint {
                decoder: decoder_identity(self.source_is_raw).into(),
                version: if self.source_is_raw {
                    lumina_raw::libraw_decode_version()
                } else {
                    env!("CARGO_PKG_VERSION").into()
                },
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            extras: BTreeMap::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    // ---- PERF-GUI-7: asynchronous (off-main-thread) file decode ----
    //
    // `begin_load_path` starts a background decode and returns immediately so
    // switching files never freezes the UI. The decoded frame is delivered via
    // `decode_rx` and applied on the main thread by `poll_decode` (driven from
    // `update`). `is_supported_image` keeps the RAW-only / raster filter.
    /// Start a background decode of `path`. The previous preview stays on screen
    /// until the decoded frame arrives; failures are surfaced via `show_error`.
    #[cfg(not(target_arch = "wasm32"))]
    fn begin_load_path(&mut self, path: String) {
        if path.trim().is_empty() {
            return;
        }
        info!("decoding (background) {}", path);
        self.status = format!(
            "Decoding {}",
            Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
        );
        // PREVIEW-CACHE-FEATURE (A1/A4): if the target being navigated to was
        // already prepared as a neighbor preview (change-of-active, same session
        // or earlier prefetch), paint it immediately — RAM-LRU or disk hit —
        // so the first frame shows no decode/render wait. The full-resolution
        // decode below still runs in the background and `finish_decode` replaces
        // this with the full render; a miss keeps the standard loading path.
        self.paint_cached_neighbor_preview(&path);
        let (tx, rx) = std::sync::mpsc::channel();
        self.decode_rx = Some(rx);
        std::thread::spawn(move || {
            let result: DecodeResult = (|| {
                let p = std::path::PathBuf::from(&path);
                let bytes = std::fs::read(&p)
                    .map_err(|e| (path.clone(), format!("{}: {}", p.display(), e)))?;
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image")
                    .to_string();
                let source_is_raw = is_raw_name(&name);
                let (frame, orientation, camera_white_balance) = if source_is_raw {
                    let image = lumina_raw::decode_bytes(&bytes, &name)
                        .map_err(|e| (path.clone(), e.to_string()))?;
                    let wb = image.metadata.camera_white_balance;
                    let camera_white_balance = if wb.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                        warn!(
                            "As-Shot white balance invalid {:?} for {} — dropping to None",
                            wb, name
                        );
                        None
                    } else {
                        Some(wb)
                    };
                    (
                        image.frame,
                        image.metadata.orientation,
                        camera_white_balance,
                    )
                } else {
                    (
                        ImageFrame::decode(&bytes).map_err(|e| (path.clone(), e.to_string()))?,
                        1,
                        None,
                    )
                };
                Ok(DecodedFrame {
                    path,
                    name,
                    bytes,
                    frame,
                    orientation,
                    camera_white_balance,
                    source_is_raw,
                })
            })();
            let _ = tx.send(result);
        });
    }

    /// Apply a completed background decode: set the source, then restore the
    /// sidecar recipe for that path (mirroring the old synchronous `load_path`).
    #[cfg(not(target_arch = "wasm32"))]
    fn finish_decode(&mut self, result: DecodeResult) {
        match result {
            Ok(frame) => {
                self.path = frame.path.clone();
                // PREVIEW-CACHE-FEATURE: the active image just changed — plan
                // the +4/−2 neighbor window around it (lazy, on workers).
                let active_path = self.path.clone();
                self.schedule_neighbor_previews(&active_path);
                self.apply_decoded_frame(
                    &frame.frame,
                    frame.orientation,
                    frame.camera_white_balance,
                    &frame.name,
                    &frame.bytes,
                    frame.source_is_raw,
                );
                if let Err(e) = self.render() {
                    error!("render after load failed for {}: {e}", self.source_name);
                    self.show_error(e);
                }
                let path = std::path::PathBuf::from(self.path.trim());
                if let Ok(document) =
                    lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&path))
                {
                    // REVIEW-GUI-N1: remember the revision this document was
                    // loaded from — the save path compares against it (CAS).
                    self.sidecar_revision = lumina_sidecar::document_revision(&document).ok();
                    // REVIEW-GUI-N2: resolve the copy by identity, never
                    // positionally. `apply_decoded_frame` reset the session to
                    // the default copy id ("vc-original"); prefer that id,
                    // then the document's default copy, then the first entry —
                    // and adopt whichever id actually resolved so subsequent
                    // edits/saves target the same copy even when the JSON
                    // array was reordered.
                    let resolved = document
                        .virtual_copies
                        .iter()
                        .find(|copy| copy.id == self.virtual_copy_id)
                        .or_else(|| document.virtual_copies.iter().find(|copy| copy.is_default))
                        .or_else(|| document.virtual_copies.first())
                        .cloned();
                    if let Some(copy) = resolved {
                        let candidate = copy.recipe.clone();
                        self.virtual_copy_id = copy.id.clone();
                        self.selected_mask_id = copy
                            .mask_layers
                            .first()
                            .map(|layer| layer.mask.mask_id.clone());
                        self.document = Some(document);
                        let config = AutoToneConfig {
                            target_luminance: candidate.auto_features.target_luminance,
                            ..Default::default()
                        };
                        let fingerprint =
                            tone_fingerprint(self.original.as_ref().expect("loaded frame"), config);
                        let valid = candidate
                            .auto_features
                            .analysis_fingerprint
                            .as_ref()
                            .is_some_and(|stored| is_current_tone_analysis(stored, &fingerprint));
                        self.recipe = candidate;
                        let stale_auto_tone = self.recipe.auto_features.enable_auto_tone && !valid;
                        if stale_auto_tone {
                            clear_stale_auto_tone(&mut self.recipe);
                            self.status = Str::AutoToneStale.t().into();
                        }
                        if let Err(error) = self.render() {
                            self.show_error(error);
                        } else if stale_auto_tone {
                            self.status = Str::AutoToneStale.t().into();
                        }
                    } else {
                        // A sidecar without any virtual copy cannot be
                        // rendered from; surface it instead of silently
                        // keeping an unrelated recipe.
                        warn!("sidecar for {} has no virtual copies", path.display());
                        self.show_error(GuiError::Io(Str::VirtualCopyNotFound.t().to_string()));
                    }
                }
            }
            Err((path, message)) => {
                // REVIEW-GUI-PATHDESYNC-1: a failed decode must NOT adopt the
                // new path — original/document/recipe still belong to the
                // previously loaded image, so writes would otherwise produce a
                // phantom sidecar under a path that never loaded. Surface the
                // failure visibly instead.
                error!("background decode failed for {path}: {message}");
                self.show_error(GuiError::Io(format!("{path}: {message}")));
            }
        }
    }

    /// Drain any completed background decode (PERF-GUI-7). Called every frame
    /// from `update` so the UI stays responsive while decoding.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_decode(&mut self) {
        let Some(rx) = &self.decode_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.decode_rx = None;
                self.finish_decode(result);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.decode_rx = None;
            }
        }
    }

    // ---- PREVIEW-CACHE-FEATURE: neighbor-preview prefetch (native) ----
    //
    // The active image is always the full GPU/CPU texture; the neighbors in the
    // +4/−2 window are prepared as WebP previews on background workers and kept
    // in a RAM LRU + disk tier (see `feature/quality/preview-cache.md` and the
    // `lumina_core::preview_cache` primitives). A miss is a *visible*
    // preparation state, never a silently wrong/upscaled fallback.

    /// Drain completed neighbor-preview results and request a repaint when work
    /// arrived. Non-blocking; called every frame from `update`.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_neighbor_previews(&mut self, ctx: &egui::Context) {
        let Some(ctrl) = self.preview_ctrl.as_mut() else {
            return;
        };
        let before = ctrl.lru().len();
        ctrl.poll();
        let ready = ctrl.lru().len().saturating_sub(before);
        self.frame_previews_ready += ready;
        // PREVIEW-CACHE-FEATURE: worker failures are never swallowed — they are
        // surfaced visibly (the neighbor-preview cell UI shows them via the
        // probe → message mapping) and logged here for the current slice.
        let mut failure_count = 0;
        for (probe, message) in ctrl.drain_failures() {
            warn!("neighbor preview failed for {probe}: {message}");
            failure_count += 1;
        }
        // A2: a ready frame or a (visible) failure changes per-cell badges — the
        // next frame must redraw the navigator cells.
        if ready > 0 || failure_count > 0 {
            ctx.request_repaint();
        }
    }

    /// Plan and enqueue the asymmetric +4/−2 neighbor-preview window around the
    /// currently active image `active_path`. The worker pool is spawned lazily
    /// on first navigation so headless tests stay thread-free. The authoritative
    /// state of each neighbor (content hash, sidecar recipe) is resolved inside
    /// the workers, never on the UI thread.
    #[cfg(not(target_arch = "wasm32"))]
    fn schedule_neighbor_previews(&mut self, active_path: &str) -> usize {
        if self.entries.is_empty() {
            return 0;
        }
        let canonical = Path::new(active_path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(active_path))
            .to_string_lossy()
            .into_owned();
        let Some(active) = self.entries.iter().position(|e| e.thumb_key == canonical) else {
            return 0;
        };
        // Pre-build arrays before touching `preview_ctrl` so the borrows stay
        // disjoint (no `self` field overlap in the borrow checker).
        let probe_ids: Vec<String> = self.entries.iter().map(|e| e.thumb_key.clone()).collect();
        let sources: Vec<PathBuf> = self.entries.iter().map(|e| e.path.clone()).collect();
        let names: Vec<String> = self.entries.iter().map(|e| e.name.clone()).collect();
        let preview_ctrl = self.preview_ctrl.get_or_insert_with(|| {
            // Pool clamped to a small dedicated size (the SOLL mandates a fixed
            // small pool; thumbnails keep their own pool). The disk tier is
            // rooted per-job at the source's own `.lumina/previews` folder.
            let pool_size = thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(2, 4);
            let (ctrl, _queue) = preview_ctrl::PreviewController::spawn(pool_size);
            ctrl
        });
        // A6: inherit the folder / display option — a 1:1 zoom plans neighbors
        // at 1:1 resolution, otherwise the (default) Screen preview is used.
        // The worker keeps the decoded frame at full resolution for `OneToOne`
        // (no downscaling), so the target here only drives the Screen path.
        let (kind, target) = if self.zoom_mode == ZoomMode::OneToOne {
            (lumina_core::preview_cache::PreviewKind::OneToOne, (0, 0))
        } else {
            (
                lumina_core::preview_cache::PreviewKind::Screen,
                (self.draft_max_dim, self.draft_max_dim),
            )
        };
        // A6: when the kind/resolution changes (e.g. zoom → 1:1) the previously
        // prepared neighbors are stale for the new key and are lazily re-rendered.
        preview_ctrl.plan_kind(kind);
        let jobs =
            preview_ctrl::plan_window_jobs(&probe_ids, &sources, &names, active, target, kind);
        let mut enqueued = 0;
        for job in jobs {
            if preview_ctrl.enqueue(job) {
                enqueued += 1;
            }
        }
        self.frame_previews_enqueued += enqueued;
        preview_ctrl.set_active(&canonical);
        enqueued
    }

    /// PREVIEW-CACHE-FEATURE (A1/A4): paint a cached neighbor preview for the
    /// path being navigated to, so the first frame of a change-of-active shows
    /// no decode/render wait. Serves first from the RAM LRU, then from the disk
    /// tier; a miss is a genuine miss (the standard lazy loading path applies —
    /// never a silently wrong/upscaled image).
    #[cfg(not(target_arch = "wasm32"))]
    fn paint_cached_neighbor_preview(&mut self, path: &str) {
        let canonical = Path::new(path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .into_owned();
        let source = PathBuf::from(path);
        // Ask the controller for the cached frame; the borrow on `preview_ctrl`
        // ends here so the assignment to `self.preview` below is allowed.
        let cached = self.preview_ctrl.as_mut().and_then(|ctrl| {
            match ctrl.neighbor_preview(&canonical, &source) {
                Ok(Some(frame)) => Some(frame),
                Ok(None) => None,
                Err(message) => {
                    // A cache read failure is surfaced, not silently swallowed
                    // (no silent fallback): it still proceeds with the real decode,
                    // but the neighbour-preview error is logged visibly.
                    log::warn!("neighbor preview cache read failed for {path}: {message}");
                    None
                }
            }
        });
        if let Some(frame) = cached {
            log::debug!("neighbor preview cache-hit, painting immediately: {path}");
            self.preview = Some(frame);
            self.preview_generation += 1;
            // Force `update_texture` to (re-)upload the new pixels this frame.
            self.texture_identity = None;
        }
    }

    /// PREVIEW-CACHE-FEATURE (A2): a visible badge (label + color) for a source's
    /// neighbor-preview state, or `None` when no state applies (e.g. the probe
    /// was consumed/active). Maps the controller's per-probe state to a cell
    /// overlay so „wird vorbereitet / Veraltet / Fehler" is never only a log.
    #[cfg(not(target_arch = "wasm32"))]
    fn neighbor_preview_badge(&self, probe_id: &str) -> Option<(String, egui::Color32)> {
        let ctrl = self.preview_ctrl.as_ref()?;
        // The active image is never displayed via the neighbor cache — skip the
        // badge there (SOLL: the active image stays a full texture).
        if ctrl.active_probe_id() == Some(probe_id) {
            return None;
        }
        let (label, color) = match ctrl.probe_state(probe_id) {
            preview_ctrl::PreviewProbeState::Miss => return None,
            preview_ctrl::PreviewProbeState::Loading => (
                "wird vorbereitet…".to_owned(),
                egui::Color32::from_rgb(0x44, 0x66, 0x88),
            ),
            preview_ctrl::PreviewProbeState::Ready => (
                "Vorschau bereit".to_owned(),
                egui::Color32::from_rgb(0x2e, 0x7d, 0x32),
            ),
            preview_ctrl::PreviewProbeState::Stale => (
                "Veraltet".to_owned(),
                egui::Color32::from_rgb(0xb0, 0x8a, 0x00),
            ),
            preview_ctrl::PreviewProbeState::Failed => (
                format!("Fehler: {}", ctrl.failure(probe_id).unwrap_or("unbekannt")),
                egui::Color32::from_rgb(0xb0, 0x2a, 0x2a),
            ),
        };
        Some((label, color))
    }

    /// Persist the active virtual copy's recipe into the sidecar.
    ///
    /// REVIEW-GUI-SAVEMSG-1: the "Sidecar saved" status is set **only** on
    /// success; a failed write keeps the error visible instead of being
    /// overwritten by a success message.
    ///
    /// REVIEW-GUI-N1: the write goes through the compare-and-swap API
    /// [`lumina_sidecar::save_sidecar_if_unchanged`] with the revision read
    /// from disk immediately before, so an externally modified sidecar is
    /// reported as a conflict instead of being silently overwritten.
    /// Additionally, `document.source` of an already-loaded document is kept
    /// as loaded — recomputing it from the live bytes would silently launder a
    /// source/conflict state (the fresh identity is only set for documents
    /// newly created in this session).
    #[cfg(not(target_arch = "wasm32"))]
    fn save_sidecar(&mut self) {
        if self.path.trim().is_empty() {
            self.show_error(Str::SaveNeedsLocalPath.t());
            return;
        }
        let path = std::path::PathBuf::from(self.path.trim());
        let Some(frame) = &self.original else {
            self.show_error(Str::NoImageLoaded.t());
            return;
        };
        let sidecar_path = lumina_sidecar::sidecar_path_for(&path);
        // REVIEW-GUI-N1: compare-and-swap against the revision this document
        // lineage was loaded from (`self.sidecar_revision`, captured at
        // load time and refreshed after each successful save). An external
        // modification since then therefore surfaces as a visible conflict
        // instead of being silently overwritten. `None` expects the file to
        // not exist yet (fresh document); if a file appeared in the meantime,
        // the CAS refuses visibly rather than clobbering it.
        let expected_revision = self.sidecar_revision.clone();
        let mut document = self
            .document
            .take()
            .unwrap_or_else(|| SidecarDocument::new(self.source_identity(frame), "raster-mvp-1"));
        // REVIEW-GUI-N1: the identity of an already-loaded document stays
        // exactly as loaded — recomputing it here from the live bytes would
        // silently launder an externally changed source (conflict laundering).
        // A document newly created above already carries the current identity
        // via `SidecarDocument::new(self.source_identity(frame), ..)`.
        let Some(copy) = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            self.show_error(Str::VirtualCopyNotFound.t());
            self.document = Some(document);
            return;
        };
        copy.recipe = self.recipe.clone();
        match lumina_sidecar::save_sidecar_if_unchanged(
            &sidecar_path,
            &document,
            expected_revision.as_deref(),
        ) {
            Ok(new_revision) => {
                self.status = Str::SidecarSaved.t().into();
                self.sidecar_revision = Some(new_revision);
                self.document = Some(document);
                self.list_directory();
                // `list_directory` overwrites the status with its scan result;
                // restore the success message so the final state is honest.
                self.status = Str::SidecarSaved.t().into();
            }
            Err(save_error) => {
                error!("sidecar save failed for {}: {save_error}", path.display());
                self.show_error(save_error);
                // Keep the document so the failed edit is not lost; the
                // conflict stays visible until resolved.
                self.document = Some(document);
            }
        }
    }

    // ---- F-103-N5: Export module -------------------------------------------
    //
    // The export path is byte-identical to the CLI: it renders the current
    // recipe through the *same* `lumina_core::export_image` function (render +
    // encode) and writes the artifact through the *same* `lumina_sidecar::
    // write_atomically` helper. No encode logic is duplicated in the GUI.

    /// Resolve the effective export target and enforce the non-destructive
    /// write guards (REVIEW-GUI-EXPORT-1). The format extension is applied
    /// **first**, then the final path is checked against the loaded source and
    /// its persistent artefacts (`<source>.lumina.json` sidecar and
    /// `<source>.lumina.zdata` mask bundle) so an export can never overwrite
    /// the original or its sidecar data — e.g. target `/d/photo` with format
    /// PNG must be refused when `/d/photo.png` is the loaded source, which a
    /// pre-extension check would miss. Pure helper, unit-tested headless.
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_export_target(
        source: &str,
        output: PathBuf,
        extension: &str,
    ) -> Result<PathBuf, GuiError> {
        let output = output.with_extension(extension);
        if source.trim().is_empty() {
            return Ok(output);
        }
        let source = Path::new(source);
        let sidecar = lumina_sidecar::sidecar_path_for(source);
        let zdata = lumina_sidecar::zdata_path_for(source);
        // `zdata_path_for` requires the sidecar crate's `zdata` feature, which
        // the GUI enables for every native target (this helper is
        // `not(wasm32)`-gated together with the whole export module).
        let protected: Vec<(&Path, &str)> = vec![
            (source, "the original image"),
            (sidecar.as_path(), "its sidecar"),
            (zdata.as_path(), "its mask bundle"),
        ];
        for (protected_path, kind) in &protected {
            if Self::paths_resolve_equal_symmetric(protected_path, &output)
                .map_err(|error| GuiError::Io(error.to_string()))?
            {
                return Err(GuiError::Io(format!(
                    "export target {} resolves to {}, {}; refusing to overwrite it",
                    output.display(),
                    kind,
                    protected_path.display()
                )));
            }
        }
        Ok(output)
    }

    /// Same-path check that tolerates either side not existing yet
    /// (`lumina_sidecar::paths_resolve_equal` canonicalizes its first argument,
    /// which fails ENOENT for a sidecar/zdata artefact that was never written).
    /// Existing paths are canonicalized directly; a missing path is resolved
    /// against its parent directory so name collisions are still caught.
    #[cfg(not(target_arch = "wasm32"))]
    fn paths_resolve_equal_symmetric(a: &Path, b: &Path) -> std::io::Result<bool> {
        let resolve = |path: &Path| -> std::io::Result<std::path::PathBuf> {
            if path.exists() {
                std::fs::canonicalize(path)
            } else {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                Ok(std::fs::canonicalize(parent)?.join(path.file_name().unwrap_or_default()))
            }
        };
        Ok(resolve(a)? == resolve(b)?)
    }

    /// Export the currently loaded image to `output` using the shared render +
    /// encode chain. The output file extension is forced to the format's
    /// canonical extension so the chosen format (not a typed extension) is
    /// authoritative — mirroring the CLI's `output.with_extension(...)`.
    ///
    /// The original source is never overwritten: if `self.path` is set and
    /// resolves equal to `output`, the export is rejected as a [`GuiError`]
    /// (no silent fallback). The artifact is written atomically; the declarative
    /// recipe/sidecar is left untouched by the export itself (the user saves
    /// the recipe explicitly via "Save Recipe / Sidecar").
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_to(&mut self, output: PathBuf) -> Result<(), GuiError> {
        let Some(original) = self.original.as_ref() else {
            return Err(GuiError::Io(
                "No image loaded; open or drop an image first".into(),
            ));
        };
        let format = self.export_format;
        let quality = self.export_quality;
        // Apply the format extension FIRST, then enforce the non-destructive
        // write guards against the resolved target (REVIEW-GUI-EXPORT-1).
        let output = Self::resolve_export_target(
            &self.path,
            output,
            self.export_format.default_extension(),
        )?;
        let options = ExportOptions {
            format,
            quality,
            dither: false,
            ..Default::default()
        };
        options.validate().map_err(GuiError::Core)?;
        // Build the identical render context used by `render()` (what the user
        // currently sees) so the export matches the preview.
        let masks_context = {
            let planes = self.load_mask_planes();
            self.document.as_ref().and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                    .map(|_| MaskContext {
                        copies: &document.virtual_copies,
                        active_copy_id: &self.virtual_copy_id,
                        planes,
                        policy: MaskPolicy::Warn,
                    })
            })
        };
        let context = RenderContext {
            recipe: &self.recipe,
            camera_white_balance: self.camera_white_balance,
            source_actions: &[],
            masks: masks_context,
            lensfun: None,
        };
        let encoded = export_image(original, &context, options).map_err(GuiError::Core)?;
        lumina_sidecar::write_atomically(&output, &encoded).map_err(GuiError::Sidecar)?;
        self.error = None;
        self.status = format!(
            "Exported {} ({:?} @ q{})",
            output.display(),
            format,
            quality
        );
        Ok(())
    }

    /// Suggested export file name derived from the source name and the selected
    /// format (e.g. `photo.jpg` from `photo.png`). Used to prefill the save
    /// dialog and the path field.
    #[cfg(not(target_arch = "wasm32"))]
    fn suggested_export_name(&self) -> String {
        let base = if self.source_name.is_empty() {
            "export".to_string()
        } else {
            self.source_name.clone()
        };
        let stem = Path::new(&base)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "export".to_string());
        format!("{}.{}", stem, self.export_format.default_extension())
    }

    /// Draw the Export module controls (native only). Under wasm32 the module is
    /// shown as a clear capability hint instead (no file-system export).
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_export_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Export.t());
        ui.label(Str::ExportTarget.t());
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.export_path);
            if ui.button(Str::ExportChoose.t()).clicked() {
                let suggested = self.suggested_export_name();
                if let Some(path) = rfd::FileDialog::new().set_file_name(&suggested).save_file() {
                    self.export_path = path.display().to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(Str::ExportFormatLabel.t());
            egui::ComboBox::from_label("")
                .selected_text(format_label(self.export_format))
                .show_ui(ui, |ui| {
                    for (candidate, label) in [
                        (ImageFileFormat::Png, "PNG"),
                        (ImageFileFormat::Jpeg, "JPEG"),
                        (ImageFileFormat::WebP, "WebP"),
                    ] {
                        if ui
                            .selectable_label(self.export_format == candidate, label)
                            .clicked()
                        {
                            self.export_format = candidate;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(Str::ExportQualityLabel.t());
            let mut quality = self.export_quality as f64;
            if ui
                .add(egui::Slider::new(&mut quality, 1.0..=100.0).show_value(true))
                .changed()
            {
                self.export_quality = quality as u8;
            }
        });
        if self.export_format == ImageFileFormat::Png {
            ui.label(Str::ExportQualityUnused.t());
        }
        ui.horizontal(|ui| {
            if ui.button(Str::ExportUseSuggested.t()).clicked() {
                self.export_path = self.suggested_export_name();
            }
            if ui.button(Str::ExportRun.t()).clicked() {
                let path = PathBuf::from(self.export_path.trim());
                if path.as_os_str().is_empty() {
                    self.show_error(GuiError::Io("Choose an export target first".into()));
                } else if let Err(error) = self.export_to(path) {
                    error!("export failed to {}: {error}", self.export_path.trim());
                    self.show_error(error);
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        ui.label(&self.status);
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui) {
        // Clone the texture handle so the borrow of `self` does not outlive the
        // block — several helpers below (`handle_mask_tool_drag`,
        // `draw_mask_overlay`, `pick_white_balance_at`, `mark_dirty`) need
        // `&mut self` and would otherwise conflict with `&self.texture`.
        //
        // GUI-WGPU-PRESENT-1: when the frame was presented readback-free from
        // VRAM (`gpu_present_frame`), the painted image is that registered user
        // texture instead of the CPU `ColorImage`; its size (full frame) feeds
        // the same geometry math. The CPU handle stays the fallback and is
        // always present once any CPU render ran (warm-up before the very first
        // render still shows the empty-state label).
        if let Some(texture) = self.texture.clone() {
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            let gpu_present = self.gpu_present_frame;
            #[cfg(not(all(not(target_arch = "wasm32"), feature = "gpu")))]
            #[allow(clippy::infallible_destructuring_match)]
            let gpu_present: Option<(egui::TextureId, [usize; 2])> = None;
            // The preview pane is laid out somewhere inside the window, so its
            // origin is not (0,0). `available_size()` returns only the
            // dimensions, so building the draw rect relative to (0,0) places the
            // image at the window origin instead of centering it in the pane,
            // producing the misalignment/clipping seen in the screenshot.
            //
            // Use the true available rectangle (with the correct origin) and
            // center the image inside it.
            let pane = ui.available_rect_before_wrap();
            let (tw, th) = match gpu_present {
                Some((_, size)) => (size[0] as f32, size[1] as f32),
                None => {
                    let size = texture.size();
                    (size[0] as f32, size[1] as f32)
                }
            };
            // Un-cropped source dimensions backing the texture (the texture
            // itself is an ROI crop at zoom > 1, so its own fit scale depends
            // on the zoom and must never feed back into the zoom derivation —
            // REVIEW-GUI-ZOOMLOOP-1).
            let (src_w, src_h) = self
                .original
                .as_ref()
                .map(|o| (o.width as f32, o.height as f32))
                .or_else(|| {
                    self.draft_original
                        .as_ref()
                        .map(|d| (d.width as f32, d.height as f32))
                })
                .unwrap_or((tw, th));
            if src_w > 0.0 && src_h > 0.0 {
                // Object-contain fit of the pane against the un-cropped source
                // (not capped, so small images fill the pane / large images are
                // downscaled, Lightroom-like).
                self.preview_base_fit_scale = (pane.width() / src_w).min(pane.height() / src_h);
                self.preview_src_w = src_w;
                self.preview_src_h = src_h;
            }
            // Cache geometry so the next frame's sync_zoom() derives absolute
            // zoom modes (100% / 200% / Fit Width) from stable, un-cropped
            // values.
            self.preview_pane_w = pane.width();
            self.preview_pane_h = pane.height();

            // On-screen scale in screen points per SOURCE pixel. The
            // ROI-cropped texture is drawn at this same scale; `roi_from_zoom`
            // sizes the crop to fill the pane exactly at it.
            let mut scale = self.preview_base_fit_scale * self.preview_zoom;
            let mut draw = egui::vec2(tw * scale, th * scale);
            let mut center = pane.center() + self.preview_pan;
            let mut rect = egui::Rect::from_center_size(center, draw);

            // Scroll-wheel zoom around the cursor (Lightroom-like). Only while
            // the pointer hovers the preview so other scroll areas are
            // unaffected. egui 0.36 removed `InputState::raw_scroll_delta`; the
            // raw per-frame wheel delta is summed from the `MouseWheel` events.
            let scroll = ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                        _ => None,
                    })
                    .sum::<f32>()
            });
            let pointer = ui.input(|i| i.pointer.interact_pos());
            if scroll != 0.0 {
                if let Some(p) = pointer {
                    if rect.contains(p) {
                        let srect_w = rect.width().max(1e-6);
                        let srect_h = rect.height().max(1e-6);
                        let fx = ((p.x - rect.min.x) / srect_w).clamp(0.0, 1.0);
                        let fy = ((p.y - rect.min.y) / srect_h).clamp(0.0, 1.0);
                        let factor = if scroll > 0.0 { 1.1 } else { 1.0 / 1.1 };
                        self.preview_zoom = (self.preview_zoom * factor).clamp(0.05, 32.0);
                        self.zoom_mode = ZoomMode::Custom;
                        let new_scale = self.preview_base_fit_scale * self.preview_zoom;
                        let new_draw = egui::vec2(tw * new_scale, th * new_scale);
                        let new_center =
                            p - egui::vec2(fx * new_draw.x, fy * new_draw.y) + new_draw / 2.0;
                        self.preview_pan = new_center - pane.center();
                        // Recompute for the placement below.
                        scale = new_scale;
                        draw = new_draw;
                        center = new_center;
                        rect = egui::Rect::from_center_size(center, draw);
                        self.mark_dirty();
                    }
                }
            }

            // Whether the (zoomed) image overflows the pane on either axis — only
            // then is panning meaningful.
            let pan_eligible = draw.x > pane.width() + 0.5 || draw.y > pane.height() + 0.5;

            #[cfg(not(target_arch = "wasm32"))]
            let armed = self.mask_tool != MaskTool::None;
            #[cfg(not(target_arch = "wasm32"))]
            let pick = self.wb_pick_mode;
            #[cfg(target_arch = "wasm32")]
            let armed = false;
            #[cfg(target_arch = "wasm32")]
            let pick = false;

            // A mask tool arms the preview for a drag gesture; the WB eyedropper
            // keeps a plain click; otherwise a zoomed image drags to pan (hand
            // tool). Pan never conflicts with an armed mask tool or the picker.
            let sense = if armed {
                egui::Sense::drag()
            } else if pick {
                egui::Sense::click()
            } else if pan_eligible {
                egui::Sense::drag()
            } else {
                egui::Sense::click()
            };
            let response = ui.allocate_rect(rect, sense);

            // Pan while zoomed (only when no mask tool and not picking).
            if !armed && !pick && pan_eligible {
                let delta = response.drag_delta();
                if delta != egui::Vec2::ZERO {
                    if response.drag_started() {
                        self.zoom_mode = ZoomMode::Custom;
                        trace!("GUI interaction: preview pan start");
                    }
                    center += delta;
                    // REVIEW-GUI-PANROI-1: a pan moves the visible window
                    // inside the source, so the ROI-cropped texture must be
                    // re-derived from the new offset. Marking dirty here arms
                    // the PERF-GUI-3/4 hot path: while the pointer stays down
                    // the next frame renders a cheap draft from the new pan
                    // (coalesced to one draft per moved frame), and once the
                    // pointer is released the debounced full render commits
                    // the final ROI — including the clamped borders that
                    // `preview_pan` alone could never reach before.
                    self.mark_dirty();
                }
            }

            // Clamp the centre so a zoomed image always covers the pane (no empty
            // gutters) and a smaller-than-pane image stays centred (no panning).
            //
            // Order-independent guard: at fit the computed draw size can, by
            // floating-point rounding, come out a few micro-pixels larger than
            // the pane (`draw.x ≈ pane.width() + ε`), which would make
            // `pane.left() + draw.x / 2.0` (the `clamp` *min*) larger than
            // `pane.right() - draw.x / 2.0` (the `clamp` *max*) and panic
            // `f32::clamp`. Swap the bounds when they invert instead of passing
            // them through, and clamp against the corrected [lo, hi] so the
            // centre is pinned to the pane centre (where `lo ≈ hi ≈ centre`).
            if draw.x <= pane.width() {
                center.x = pane.center().x;
            } else {
                let mut lo = pane.left() + draw.x / 2.0;
                let mut hi = pane.right() - draw.x / 2.0;
                if lo > hi {
                    std::mem::swap(&mut lo, &mut hi);
                }
                center.x = center.x.clamp(lo, hi);
            }
            if draw.y <= pane.height() {
                center.y = pane.center().y;
            } else {
                let mut lo = pane.top() + draw.y / 2.0;
                let mut hi = pane.bottom() - draw.y / 2.0;
                if lo > hi {
                    std::mem::swap(&mut lo, &mut hi);
                }
                center.y = center.y.clamp(lo, hi);
            }
            self.preview_pan = center - pane.center();
            let rect = egui::Rect::from_center_size(center, draw);
            self.preview_effective_scale = scale;

            // GUI-WGPU-PRESENT-1: the GPU-presented frame is a registered
            // user texture — draw it via the painter directly (identical rect,
            // full UVs). Otherwise the historical CPU `Image` widget.
            if let Some((present_id, _)) = gpu_present {
                ui.painter().image(
                    present_id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                ui.put(rect, egui::Image::from_texture(&texture));
            }

            // WB eyedropper needs the source-coordinate mapping, which is part
            // of the native (non-wasm) capability set.
            #[cfg(not(target_arch = "wasm32"))]
            if pick && response.clicked() {
                // REVIEW-GUI-MASKGEO-1: the picker may have been armed before
                // geometry was edited; refuse the pick visibly instead of
                // sampling transformed-wrong source pixels, and disarm so the
                // stale mode does not linger.
                if self.geometry_blocks_source_mapping() {
                    self.wb_pick_mode = false;
                    self.status = Self::GEOMETRY_TOOL_BLOCKED.into();
                } else if let Some(pos) = response.interact_pointer_pos() {
                    let (nx, ny) = Self::to_normalized(
                        pos,
                        rect,
                        self.preview_roi,
                        self.image_dims().unwrap_or((1, 1)),
                    );
                    self.pick_white_balance_at(nx as f64, ny as f64);
                }
            }
            if pick {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
                    egui::StrokeKind::Middle,
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            self.handle_mask_tool_drag(&response, rect);
            // Mask overlay is painted over the full-frame rect (accounting for the
            // current ROI crop) so it lines up with the zoomed/panned view.
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (full_w, full_h) = self.image_dims().unwrap_or((1, 1));
                let roi = self.preview_roi.unwrap_or([0, 0, full_w, full_h]);
                let from_min = egui::pos2(
                    rect.min.x - roi[0] as f32 * scale,
                    rect.min.y - roi[1] as f32 * scale,
                );
                let full_rect = egui::Rect::from_min_size(
                    from_min,
                    egui::vec2(full_w as f32 * scale, full_h as f32 * scale),
                );
                self.draw_mask_overlay(ui, full_rect);
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(Str::NoImage.t());
            });
        }
    }

    /// Map a pointer position to normalized (0..=1) *full-frame* source
    /// coordinates. The displayed rect may be only a zoomed/panned sub-crop of
    /// the source (see [`Self::preview_roi`]); `roi`/`full` translate the local
    /// rect fraction into absolute source space so the WB eyedropper and mask
    /// tools stay accurate at any zoom/offset.
    #[cfg(not(target_arch = "wasm32"))]
    fn to_normalized(
        pos: egui::Pos2,
        rect: egui::Rect,
        roi: Option<[u32; 4]>,
        full: (u32, u32),
    ) -> (f32, f32) {
        // Guard the pointer→source division against a zero-width/height rect
        // (e.g. a momentarily empty texture) so we never divide by zero and
        // produce NaN/Infinity into the normalized coordinates.
        let rw = rect.width().max(1e-6);
        let rh = rect.height().max(1e-6);
        let fx = ((pos.x - rect.min.x) / rw).clamp(0.0, 1.0);
        let fy = ((pos.y - rect.min.y) / rh).clamp(0.0, 1.0);
        let roi = roi.unwrap_or([0, 0, full.0, full.1]);
        let nx = (roi[0] as f32 + fx * roi[2] as f32) / full.0 as f32;
        let ny = (roi[1] as f32 + fy * roi[3] as f32) / full.1 as f32;
        (nx, ny)
    }

    /// Drive an interactive mask-tool drag on the preview widget.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_mask_tool_drag(&mut self, response: &egui::Response, rect: egui::Rect) {
        if self.mask_tool == MaskTool::None || self.wb_pick_mode {
            return;
        }
        // REVIEW-GUI-MASKGEO-1 defense in depth: the tool can only be armed via
        // `set_mask_tool`, which already refuses while geometry is active — but
        // geometry could be *edited* mid-session, so re-check before mapping
        // any pointer position into source coordinates.
        if self.geometry_blocks_source_mapping() {
            return;
        }
        let Some(pos) = response.interact_pointer_pos() else {
            return;
        };
        let (nx, ny) = Self::to_normalized(
            pos,
            rect,
            self.preview_roi,
            self.image_dims().unwrap_or((1, 1)),
        );
        if response.drag_started() {
            self.drawing = true;
            self.drag_start = Some(Point2 { x: nx, y: ny });
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                self.pending_brush_marks.clear();
                self.pending_brush_marks.push(BrushMark {
                    x: nx,
                    y: ny,
                    radius: self.brush_radius,
                    sign: if self.brush_eraser {
                        BrushMarkSign::Negative
                    } else {
                        BrushMarkSign::Positive
                    },
                });
                #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
                self.gpu_upload_brush_tile(nx, ny);
            }
        } else if response.dragged() {
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                if let Some(last) = self.pending_brush_marks.last() {
                    let dist = ((last.x - nx).powi(2) + (last.y - ny).powi(2)).sqrt();
                    if dist > self.brush_radius * 0.5 {
                        self.pending_brush_marks.push(BrushMark {
                            x: nx,
                            y: ny,
                            radius: self.brush_radius,
                            sign: if self.brush_eraser {
                                BrushMarkSign::Negative
                            } else {
                                BrushMarkSign::Positive
                            },
                        });
                        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
                        self.gpu_upload_brush_tile(nx, ny);
                    }
                }
            }
        }
        if response.drag_stopped() {
            self.finish_drawing();
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn gpu_upload_brush_tile(&mut self, nx: f32, ny: f32) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if !gpu.is_available() {
            return;
        }
        let Ok((w, h)) = self.image_dims() else {
            return;
        };
        // Ensure the persistent R16 plane exists and matches the current source dims.
        let dims_changed = self.brush_mask_plane_dims != Some((w, h));
        if dims_changed || self.brush_mask_plane.is_none() {
            let len = (w as usize).saturating_mul(h as usize);
            self.brush_mask_plane = Some(vec![0u16; len]);
            self.brush_mask_plane_dims = Some((w, h));
            // Also ensure VRAM mask texture is sized for this source; stale
            // dimensions are handled lazily in `ensure_vram` at render time.
            if let Err(e) = gpu.ensure_vram(w, h) {
                warn!("gpu ensure_vram({}x{}) failed: {}", w, h, e);
            }
        }
        let Some(plane) = self.brush_mask_plane.as_mut() else {
            return;
        };
        let sign = if self.brush_eraser {
            lumina_sidecar::BrushMarkSign::Negative
        } else {
            lumina_sidecar::BrushMarkSign::Positive
        };
        let tiles = lumina_gpu::tiling::dirty_tiles_for_brush_mark(nx, ny, self.brush_radius, w, h);
        // Persistent plane: stamp once, then upload only dirty 512² tiles.
        // `stamp_brush_mark` is the canonical per-pixel kernel from
        // `lumina_core::mask_tiles` (byte-identical to `rasterize_prompt` Brush).
        lumina_core::mask_tiles::stamp_brush_mark(plane, w, h, nx, ny, self.brush_radius, sign);
        for tile in tiles {
            let x0 = tile.tx * lumina_gpu::tiling::TILE_SIZE;
            let y0 = tile.ty * lumina_gpu::tiling::TILE_SIZE;
            let tw = (lumina_gpu::tiling::TILE_SIZE)
                .min(w.saturating_sub(x0))
                .max(1);
            let th = (lumina_gpu::tiling::TILE_SIZE)
                .min(h.saturating_sub(y0))
                .max(1);
            // Extract this tile's u16 row-major subregion from the persistent plane
            // and upload as u8 LE bytes via `bytemuck::cast_slice` (no per-pixel copy).
            let mut tile_u16 = Vec::with_capacity((tw * th) as usize);
            for row in 0..th {
                let src_y = y0 + row;
                let src_start = (src_y * w + x0) as usize;
                let src_end = src_start + tw as usize;
                if src_end <= plane.len() {
                    tile_u16.extend_from_slice(&plane[src_start..src_end]);
                }
            }
            if tile_u16.len() != (tw * th) as usize {
                warn!(
                    "brush tile slice length mismatch {} vs {}x{}",
                    tile_u16.len(),
                    tw,
                    th
                );
                continue;
            }
            let tile_bytes: &[u8] = bytemuck::cast_slice(&tile_u16);
            if let Err(e) = gpu.upload_mask_tile(x0, y0, tw, th, tile_bytes) {
                warn!(
                    "gpu_upload_brush_tile upload failed at tile {}x{} ({}x{}): {}",
                    x0, y0, tw, th, e
                );
            } else {
                trace!(
                    "gpu_upload_brush_tile stamped ({:.3},{:.3}) r={:.3} -> tile ({},{}) {}x{}",
                    nx,
                    ny,
                    self.brush_radius,
                    x0,
                    y0,
                    tw,
                    th
                );
            }
        }
    }

    /// Draw the currently relevant mask as a translucent overlay on the preview:
    /// the in-progress drag (live) or the selected mask's saved prompt. The
    /// F-079 geometric rasterizer produces the matte; it is painted as a
    /// translucent tint over the source rect so the user sees exactly what the
    /// pipeline will evaluate.
    ///
    /// GUI-WGPU-PRESENT-1: when the preview is presented readback-free from
    /// VRAM, the overlay shader *already* composites the VRAM mask plane into
    /// the presented texture — painting the CPU tint here would double-tint.
    /// The CPU overlay therefore only runs for content that lives exclusively
    /// on the CPU side:
    ///
    /// - gradient/radial prompts while drawing (never stamped into VRAM tiles),
    /// - any overlay when the frame was CPU-presented.
    ///
    /// Live brush strokes and pipeline-evaluated planes
    /// (`vram_mask_is_evaluated`, pushed after each full render via
    /// `combine_mask_planes` + `upload_mask_plane`) are shown by the GPU
    /// composite instead — same tint strength, same u16 coverage domain.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_mask_overlay(&mut self, ui: &mut egui::Ui, full_rect: egui::Rect) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        if self.gpu_present_frame.is_some() {
            let live_brush_in_vram = self.drawing && self.mask_tool == MaskTool::Brush;
            if live_brush_in_vram || self.vram_mask_is_evaluated {
                trace!(
                    "draw_mask_overlay: gpu present composites the vram mask \
                     (live_brush={live_brush_in_vram}, evaluated={})",
                    self.vram_mask_is_evaluated
                );
                return;
            }
            // Gradient/radial prompts have no VRAM representation — fall
            // through to the CPU overlay below.
        }
        let Some(prompt) = self.current_overlay_prompt() else {
            return;
        };
        let (w, h) = self.image_dims().unwrap_or((1, 1));
        // Cap the rasterization so live drags stay smooth on large sources.
        let max_dim = 1024u32;
        let (rw, rh) = if w.max(h) > max_dim {
            let s = max_dim as f32 / w.max(h) as f32;
            (
                (w as f32 * s).round().max(1.0) as u32,
                (h as f32 * s).round().max(1.0) as u32,
            )
        } else {
            (w, h)
        };
        let Ok(plane) = rasterize_prompt(&prompt, rw, rh) else {
            return;
        };
        let mut pixels = vec![0u8; plane.values.len() * 4];
        for (i, value) in plane.values.iter().enumerate() {
            let alpha = (*value as f32 / u16::MAX as f32 * 0.45 * 255.0) as u8;
            pixels[i * 4..i * 4 + 4].copy_from_slice(&[80, 160, 255, alpha]);
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [plane.width as usize, plane.height as usize],
            &pixels,
        );
        let texture =
            ui.ctx()
                .load_texture("lumina-mask-overlay", image, egui::TextureOptions::NEAREST);
        ui.painter().image(
            texture.id(),
            full_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    /// The prompt to display in the overlay: the live in-progress gesture while
    /// drawing, otherwise the selected mask's saved prompt (if any).
    #[cfg(not(target_arch = "wasm32"))]
    fn current_overlay_prompt(&self) -> Option<MaskPrompt> {
        if self.drawing && self.mask_tool != MaskTool::None {
            let (start, end) = (self.drag_start?, self.drag_current?);
            return match self.mask_tool {
                MaskTool::Brush => {
                    if self.pending_brush_marks.is_empty() {
                        None
                    } else {
                        let (w, h) = self.image_dims().unwrap_or((1, 1));
                        Some(MaskPrompt::Brush {
                            marks: self.pending_brush_marks.clone(),
                            resolution: (w, h),
                            transformation: PromptTransform::default(),
                        })
                    }
                }
                MaskTool::LinearGradient => Some(Self::gradient_prompt_from_drag(start, end)),
                MaskTool::Radial => Some(Self::ellipse_prompt_from_drag(start, end)),
                MaskTool::None => None,
            };
        }
        let id = self.selected_mask_id.as_ref()?;
        let document = self.document.as_ref()?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)?;
        let mask = copy.mask_library.iter().find(|mask| mask.id == *id)?;
        mask.prompt.clone()
    }

    /// Histogram of the *currently displayed* render state (original while
    /// Before/After is held, otherwise the last preview).
    fn current_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        if self.before_after {
            self.original.as_ref().map(analyze_tone)
        } else {
            self.tone_analysis
        }
    }

    fn draw_histogram(&self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading(Str::Histogram.t());
        // REVIEW-GUI-N5: a draft preview's histogram is measured from the
        // low-resolution drag render — it must say so instead of posing as
        // the final render state.
        if self.preview_is_draft {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Draft preview — histogram reflects the low-res draft until the full render completes",
            );
        }
        if let Some(analysis) = self.current_analysis() {
            ui.label(format!(
                "Mean {:.3}  Median {:.3}",
                analysis.mean, analysis.median
            ));
            ui.label(format!(
                "P01 {:.3}  P99 {:.3}  ({} Samples)",
                analysis.p01, analysis.p99, analysis.sample_count
            ));
            let width = ui.available_width().max(40.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, egui::Color32::from_gray(35));
            let left = rect.left() + rect.width() * analysis.p01 as f32;
            let right = rect.left() + rect.width() * analysis.p99 as f32;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left, rect.top()),
                    egui::pos2(right.max(left + 1.0), rect.bottom()),
                ),
                2.0,
                egui::Color32::LIGHT_GRAY,
            );
        } else {
            ui.label(Str::NotCurrent.t());
        }
    }

    fn mark_dirty(&mut self) {
        // PERF-GUI-1 stepwise invalidation: like `set_adjustment`, this drops
        // only the final-render identity and its derived panel state. The
        // base-stage cache survives — its keys cover source/decode/ROI/source-
        // action identity and are recipe-blind, so geometry/optics/mask edits
        // reuse the cached base as well (they run downstream of it in the
        // documented pipeline order). A new SOURCE clears the cache in
        // `apply_decoded_frame`; nothing here can ever serve stale pixels.
        self.render_key = None;
        self.tone_analysis = None;
        self.error = None;
        // An edit occurred: a full-quality render will be needed (debounced on
        // pointer release / idle, PERF-GUI-3/4).
        self.pending_full_render = true;
        // GUI-WGPU-PRESENT-1: the VRAM tone result no longer matches the
        // recipe — never present it until the drag path re-renders it.
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        {
            self.vram_fresh = false;
            self.vram_mask_is_evaluated = false;
        }
    }

    /// Documented default for a flat adjustment key (identity is 0; WB Kelvin 6500).
    pub fn default_for_adjustment(key: &str) -> f64 {
        match key {
            "wb_temperature" => 6500.0,
            _ => 0.0,
        }
    }

    /// Reset exactly one flat adjustment to its documented default. Never resets
    /// the whole recipe (that is [`Self::reset`]).
    pub fn reset_single_adjustment(&mut self, key: &str) {
        trace!("GUI interaction: reset_single_adjustment {}", key);
        let default = Self::default_for_adjustment(key);
        self.recipe.adjustments.insert(key.to_owned(), default);
        self.mark_dirty();
        self.status = format!("Reset {key}");
    }

    /// A small sample RGBA PNG for headless snapshot / integration tests
    /// (F-103-N9). Pure helper with no app side effects; the bytes decode via
    /// [`Self::load_bytes`].
    pub fn sample_image_png() -> Vec<u8> {
        ImageFrame::new(
            4,
            3,
            vec![
                20, 30, 40, 255, 200, 180, 160, 255, 255, 255, 255, 255, 10, 10, 10, 255, 50, 60,
                70, 255, 90, 100, 110, 255, 120, 130, 140, 255, 200, 20, 20, 255, 1, 2, 3, 255, 80,
                90, 100, 255, 150, 160, 170, 255, 240, 240, 240, 255,
            ],
        )
        .expect("sample image dimensions are valid")
        .encode(ImageFileFormat::Png)
        .expect("sample image encodes to PNG")
    }

    /// Toggle Before/After. Deliberately does not touch the recipe.
    pub fn toggle_before_after(&mut self) {
        self.before_after = !self.before_after;
        trace!(
            "GUI interaction: toggle_before_after -> {}",
            self.before_after
        );
    }

    /// Derive a deterministic WB (temperature, tint) from a picked sRGB point so
    /// the channel means become neutral. `None` for non-positive channels.
    pub fn white_balance_from_point(r: f64, g: f64, b: f64) -> Option<(f64, f64)> {
        if r <= 0.0 || g <= 0.0 || b <= 0.0 {
            return None;
        }
        let l = (r + g + b) / 3.0;
        let gr = l / r;
        let gg = l / g;
        let gb = l / b;
        // Pipeline gains: R = 1 - warmth*0.35, G = 1 - tint*0.20, B = 1 + warmth*0.35.
        let warmth = (((1.0 - gr) + (gb - 1.0)) / 2.0) / 0.35;
        let tint = (1.0 - gg) / 0.20;
        let temperature = (6500.0 + warmth * 5500.0).clamp(1500.0, 12000.0);
        let tint = tint.clamp(-1.0, 1.0);
        Some((temperature, tint))
    }

    /// Set the `wb_temperature`/`wb_tint` recipe fields from a picked point
    /// (Core F-036-N1 path: `render_frame` applies them via the sRGB model).
    pub fn set_white_balance_from_point(&mut self, r: f64, g: f64, b: f64) -> Result<(), GuiError> {
        let Some((temp, tint)) = Self::white_balance_from_point(r, g, b) else {
            return Err(GuiError::Io(
                "Cannot derive white balance from this point".into(),
            ));
        };
        self.recipe
            .adjustments
            .insert("wb_temperature".into(), temp);
        self.recipe.adjustments.insert("wb_tint".into(), tint);
        self.wb_pick_mode = false;
        self.mark_dirty();
        self.status = "White balance set from picked point".into();
        if self.original.is_some() {
            self.render()?;
        }
        Ok(())
    }

    /// REVIEW-GUI-WASM-FOLLOWUP: the eyedropper reads source pixels from the
    /// loaded frame — a native capability (no file IO on wasm).
    #[cfg(not(target_arch = "wasm32"))]
    fn pick_white_balance_at(&mut self, nx: f64, ny: f64) {
        trace!(
            "GUI interaction: pick_white_balance_at nx={:.4} ny={:.4}",
            nx,
            ny
        );
        let Some(frame) = &self.original else {
            return;
        };
        let x = ((nx * frame.width as f64) as u32).min(frame.width.saturating_sub(1));
        let y = ((ny * frame.height as f64) as u32).min(frame.height.saturating_sub(1));
        let idx = ((y * frame.width + x) * 4) as usize;
        let px = &frame.pixels[idx..idx + 4];
        let r = px[0] as f64 / 255.0;
        let g = px[1] as f64 / 255.0;
        let b = px[2] as f64 / 255.0;
        if let Err(e) = self.set_white_balance_from_point(r, g, b) {
            error!("white balance pick failed at ({nx:.3},{ny:.3}): {e}");
            self.show_error(e);
        }
    }

    /// One horizontal Lightroom-style adjustment row bound to a flat recipe key.
    fn adjustment_slider(&mut self, ui: &mut egui::Ui, key: &str, label: &str, spec: SliderSpec) {
        let mut v = self
            .recipe
            .adjustments
            .get(key)
            .copied()
            .unwrap_or(Self::default_for_adjustment(key));
        match lr_slider(ui, label, &mut v, spec) {
            SliderAction::Changed => self.set_adjustment(key, v),
            SliderAction::ResetRequested => self.reset_single_adjustment(key),
            SliderAction::Nothing => {}
        }
    }

    // ---- Develop panel sections (fixed F-100 order) ----

    fn draw_basic(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Basic.t(), |ui| {
            ui.label(Str::WhiteBalance.t());
            self.adjustment_slider(
                ui,
                "wb_temperature",
                Str::Temperature.t(),
                identity_spec(1500.0..=12000.0, 6500.0, 50.0).unit(" K"),
            );
            self.adjustment_slider(ui, "wb_tint", Str::Tint.t(), percent_spec(-1.0..=1.0, 0.0));
            if self.wb_pick_mode {
                ui.horizontal(|ui| {
                    if ui.button(Str::WbEyedropperActive.t()).clicked() {
                        self.wb_pick_mode = false;
                    }
                    if ui.button(Str::Cancel.t()).clicked() {
                        self.wb_pick_mode = false;
                    }
                });
                ui.label(Str::PickWhiteBalanceHint.t());
            } else if ui.button(Str::WbEyedropper.t()).clicked() {
                // REVIEW-GUI-MASKGEO-1: with active Crop/Rotation/Mirror/
                // Perspective the clicked preview position no longer maps
                // 1:1 onto source pixels — refuse visibly instead of picking
                // transformed-wrong values.
                #[cfg(not(target_arch = "wasm32"))]
                let geometry_blocked = self.geometry_blocks_source_mapping();
                #[cfg(target_arch = "wasm32")]
                let geometry_blocked = false;
                if geometry_blocked {
                    warn!("WB eyedropper refused while recipe geometry is active");
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.status = Self::GEOMETRY_TOOL_BLOCKED.into();
                    }
                } else {
                    self.wb_pick_mode = true;
                }
            }
            ui.separator();
            self.adjustment_slider(
                ui,
                "exposure",
                Str::Exposure.t(),
                identity_spec(-10.0..=10.0, 0.0, 0.1),
            );
            self.adjustment_slider(
                ui,
                "contrast",
                Str::Contrast.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "highlights",
                Str::Highlights.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "shadows",
                Str::Shadows.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(ui, "whites", Str::Whites.t(), percent_spec(-1.0..=1.0, 0.0));
            self.adjustment_slider(ui, "blacks", Str::Blacks.t(), percent_spec(-1.0..=1.0, 0.0));
            if ui.button(Str::Auto.t()).clicked() {
                if let Err(e) = self.auto_tone() {
                    self.show_error(e);
                }
            }
        });
    }

    fn draw_tone_curve(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::ToneCurve.t(), |ui| {
            ui.label(Str::CurveRegions.t());
            let (mut s, mut d, mut l, mut h) = tone_curve_regions(&self.recipe);
            let spec = percent_spec(-1.0..=1.0, 0.0);
            let mut changed = false;
            for (val, label) in [
                (&mut s, Str::ToneCurveShadows),
                (&mut d, Str::ToneCurveDarks),
                (&mut l, Str::ToneCurveLights),
                (&mut h, Str::ToneCurveHighlights),
            ] {
                match lr_slider(ui, label.t(), val, spec) {
                    SliderAction::Changed | SliderAction::ResetRequested => changed = true,
                    SliderAction::Nothing => {}
                }
            }
            if changed {
                self.recipe.curves = Some(build_tone_curve(s, d, l, h));
                self.mark_dirty();
                // REVIEW-GUI-CURVE-1: a clamped output absorbs part of a
                // delta (Shadows cannot go below its 0.0 base point), so the
                // affected slider visibly snaps back. Surface that MVP limit
                // explicitly instead of leaving the user with a silently
                // moving slider.
                if tone_curve_roundtrip_is_lossy(s, d, l, h) {
                    self.status = "Tone curve: extreme region values are clamped to the 0..=1 output range (MVP limit) — negative Shadows beyond the base point are not representable.".into();
                }
            }
        });
    }

    fn draw_color(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Color.t(), |ui| {
            ui.label(Str::HslMixer.t());
            let mut hsl = self.recipe.hsl.clone().unwrap_or_default();
            let spec = percent_spec(-1.0..=1.0, 0.0);
            let channels = [
                (Str::HslRed, "red"),
                (Str::HslOrange, "orange"),
                (Str::HslYellow, "yellow"),
                (Str::HslGreen, "green"),
                (Str::HslCyan, "cyan"),
                (Str::HslBlue, "blue"),
                (Str::HslViolet, "violet"),
                (Str::HslMagenta, "magenta"),
            ];
            let mut changed = false;
            for (label, key) in channels {
                ui.label(label.t());
                let slot = hsl_channel_mut(&mut hsl, key);
                for (field, label) in [
                    (&mut slot.hue, Str::Hue),
                    (&mut slot.saturation, Str::Saturation),
                    (&mut slot.luminance, Str::Luminance),
                ] {
                    match lr_slider(ui, label.t(), field, spec) {
                        SliderAction::Changed | SliderAction::ResetRequested => changed = true,
                        SliderAction::Nothing => {}
                    }
                }
            }
            ui.separator();
            ui.label(Str::ColorGrading.t());
            let mut cg = self.recipe.color_grading.clone().unwrap_or(ColorGrading {
                version: 1,
                shadows: ColorGradingRange {
                    hue_degrees: 0.0,
                    saturation: 0.0,
                },
                midtones: ColorGradingRange {
                    hue_degrees: 0.0,
                    saturation: 0.0,
                },
                highlights: ColorGradingRange {
                    hue_degrees: 0.0,
                    saturation: 0.0,
                },
                balance: 0.0,
            });
            let mut changed_cg = false;
            for (range, label) in [
                (&mut cg.shadows, Str::GradingShadows),
                (&mut cg.midtones, Str::GradingMidtones),
                (&mut cg.highlights, Str::GradingHighlights),
            ] {
                self.color_grading_range_slider(ui, range, label, &mut changed_cg);
            }
            let mut balance = cg.balance;
            match lr_slider(
                ui,
                Str::GradingBalance.t(),
                &mut balance,
                percent_spec(-1.0..=1.0, 0.0),
            ) {
                SliderAction::Changed | SliderAction::ResetRequested => {
                    cg.balance = balance;
                    changed_cg = true;
                }
                SliderAction::Nothing => {}
            }
            if changed {
                self.recipe.hsl = Some(hsl);
                self.mark_dirty();
            }
            if changed_cg {
                self.recipe.color_grading = Some(cg);
                self.mark_dirty();
            }

            ui.separator();
            // F-100 (F-094): Presence (Texture, Clarity, Dehaze) belongs to the
            // Color section and is ordered *before* Vibrance/Saturation (F-092)
            // and before Sharpening / Noise Reduction / Vignette. We render it as
            // its own labeled group here — between Color Grading and Vibrance/
            // Saturation — so the normative F-100 control order is visible.
            // (F-103-N7 allowed either `draw_effects` above Vignette/Grain or an
            // own group; the F-100 ordering requires it here, ahead of
            // Vibrance/Saturation, hence this dedicated group.)
            ui.label(Str::Presence.t());
            let mut presence = self.recipe.presence.unwrap_or(Presence {
                version: 1,
                texture: 0.0,
                clarity: 0.0,
                dehaze: 0.0,
            });
            let mut changed_presence = false;
            let spec = percent_spec(-1.0..=1.0, 0.0);
            for (label, field) in [
                (Str::Texture, &mut presence.texture),
                (Str::Clarity, &mut presence.clarity),
                (Str::Dehaze, &mut presence.dehaze),
            ] {
                if matches!(
                    lr_slider(ui, label.t(), field, spec),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    changed_presence = true;
                }
            }
            if changed_presence {
                self.recipe.presence = Some(presence);
                self.mark_dirty();
            }

            ui.separator();
            // F-100 (F-092): Dynamics/Saturation — Vibrance then Saturation, both
            // flat adjustments on the `-1..=1` domain shown as `-100..+100`.
            self.adjustment_slider(
                ui,
                "vibrance",
                Str::Vibrance.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "saturation",
                Str::Saturation.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
        });
    }

    fn color_grading_range_slider(
        &mut self,
        ui: &mut egui::Ui,
        range: &mut ColorGradingRange,
        label: Str,
        changed: &mut bool,
    ) {
        let mut hue = range.hue_degrees;
        if matches!(
            lr_slider(
                ui,
                &Str::HuePattern.format_arg(label.t()),
                &mut hue,
                identity_spec(0.0..=360.0, 0.0, 1.0)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            range.hue_degrees = hue;
            *changed = true;
        }
        let mut sat = range.saturation;
        if matches!(
            lr_slider(
                ui,
                &Str::SatPattern.format_arg(label.t()),
                &mut sat,
                identity_spec(0.0..=1.0, 0.0, 0.01)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            range.saturation = sat;
            *changed = true;
        }
    }

    fn draw_effects(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Effects.t(), |ui| {
            ui.label(Str::Vignette.t());
            let mut effects = self.recipe.effects.clone().unwrap_or(Effects {
                vignette: Some(Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.5,
                    roundness: 0.0,
                    feather: 0.0,
                }),
                grain: Some(Grain {
                    version: 1,
                    amount: 0.0,
                    size: 0.0,
                    roughness: 0.0,
                    seed: 0,
                }),
            });
            let mut changed = false;
            if let Some(v) = &mut effects.vignette {
                let mut amount = v.amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Amount.t(),
                        &mut amount,
                        percent_spec(-1.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.amount = amount;
                    changed = true;
                }
                let mut midpoint = v.midpoint;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Midpoint.t(),
                        &mut midpoint,
                        identity_spec(0.0..=1.0, 0.5, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.midpoint = midpoint;
                    changed = true;
                }
                let mut roundness = v.roundness;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Roundness.t(),
                        &mut roundness,
                        percent_spec(-1.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.roundness = roundness;
                    changed = true;
                }
                let mut feather = v.feather;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Feather.t(),
                        &mut feather,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    v.feather = feather;
                    changed = true;
                }
            }
            ui.label(Str::Grain.t());
            if let Some(g) = &mut effects.grain {
                let mut amount = g.amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Amount.t(),
                        &mut amount,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.amount = amount;
                    changed = true;
                }
                let mut size = g.size;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Size.t(),
                        &mut size,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.size = size;
                    changed = true;
                }
                let mut roughness = g.roughness;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Roughness.t(),
                        &mut roughness,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.roughness = roughness;
                    changed = true;
                }
                let mut seed = g.seed as f64;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Seed.t(),
                        &mut seed,
                        identity_spec(0.0..=1_000_000.0, 0.0, 1.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    g.seed = seed as u64;
                    changed = true;
                }
            }
            if changed {
                self.recipe.effects = Some(effects);
                self.mark_dirty();
            }
        });
    }

    fn draw_detail(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Detail.t(), |ui| {
            ui.label(Str::Sharpening.t());
            let mut sh = self.recipe.sharpening.unwrap_or(Sharpening {
                version: 1,
                amount: 0.0,
                radius: 0.5,
                detail: 0.0,
                masking: 0.0,
            });
            let mut changed = false;
            let mut amount = sh.amount;
            if matches!(
                lr_slider(
                    ui,
                    Str::Amount.t(),
                    &mut amount,
                    identity_spec(0.0..=3.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.amount = amount;
                changed = true;
            }
            let mut radius = sh.radius;
            if matches!(
                lr_slider(
                    ui,
                    Str::Radius.t(),
                    &mut radius,
                    identity_spec(0.1..=10.0, 0.5, 0.1)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.radius = radius;
                changed = true;
            }
            let mut detail = sh.detail;
            if matches!(
                lr_slider(
                    ui,
                    Str::Detail.t(),
                    &mut detail,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.detail = detail;
                changed = true;
            }
            let mut masking = sh.masking;
            if matches!(
                lr_slider(
                    ui,
                    Str::Masking.t(),
                    &mut masking,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                sh.masking = masking;
                changed = true;
            }
            if changed {
                self.recipe.sharpening = Some(sh);
                self.mark_dirty();
            }
            ui.label(Str::NoiseReduction.t());
            let mut nr = self.recipe.noise_reduction.unwrap_or(NoiseReduction {
                version: 1,
                luminance: 0.0,
                color: 0.0,
            });
            let mut changed_nr = false;
            let mut lum = nr.luminance;
            if matches!(
                lr_slider(
                    ui,
                    Str::Luminance.t(),
                    &mut lum,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                nr.luminance = lum;
                changed_nr = true;
            }
            let mut col = nr.color;
            if matches!(
                lr_slider(
                    ui,
                    Str::Color.t(),
                    &mut col,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                nr.color = col;
                changed_nr = true;
            }
            if changed_nr {
                self.recipe.noise_reduction = Some(nr);
                self.mark_dirty();
            }
        });
    }

    fn draw_optics(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Optics.t(), |ui| {
            if cfg!(feature = "lensfun") {
                ui.label(Str::LensCorrection.t());
                let mut lc = self
                    .recipe
                    .lens_correction
                    .clone()
                    .unwrap_or(LensCorrection {
                        version: 1,
                        profile: None,
                        distortion_k1: None,
                        distortion_k2: None,
                        distortion_k3: None,
                        vignette_c0: None,
                        vignette_c1: None,
                        vignette_c2: None,
                        ca_red: None,
                        ca_blue: None,
                    });
                let mut changed = false;
                for (field, label, spec) in [
                    (
                        &mut lc.distortion_k1,
                        Str::DistortionK1,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.distortion_k2,
                        Str::DistortionK2,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.distortion_k3,
                        Str::DistortionK3,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c0,
                        Str::VignetteC0,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c1,
                        Str::VignetteC1,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c2,
                        Str::VignetteC2,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.ca_red,
                        Str::ChromaticRed,
                        identity_spec(-0.05..=0.05, 0.0, 0.001),
                    ),
                    (
                        &mut lc.ca_blue,
                        Str::ChromaticBlue,
                        identity_spec(-0.05..=0.05, 0.0, 0.001),
                    ),
                ] {
                    let current = field.as_ref().copied();
                    if let Some(c) = current {
                        let mut v = c;
                        match lr_slider(ui, label.t(), &mut v, spec) {
                            SliderAction::Changed | SliderAction::ResetRequested => {
                                *field = Some(v);
                                changed = true;
                            }
                            SliderAction::Nothing => {}
                        }
                    } else {
                        ui.label(Str::UnsetPattern.format_arg(label.t()));
                    }
                }
                if changed {
                    self.recipe.lens_correction = Some(lc);
                    self.mark_dirty();
                }
            } else {
                ui.label(Str::OpticsRequiresLensfun.t());
                ui.label(Str::NotAvailable.t());
            }
        });
    }

    fn draw_geometry(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Geometry.t(), |ui| {
            if cfg!(feature = "lensfun") {
                ui.label(Str::Crop.t());
                let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                });
                let mut changed = false;
                let mut rotation = geo.rotation_degrees;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Rotation.t(),
                        &mut rotation,
                        identity_spec(-180.0..=180.0, 0.0, 1.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    geo.rotation_degrees = rotation;
                    changed = true;
                }
                let mut mh = geo.mirror_horizontal;
                if ui.checkbox(&mut mh, Str::MirrorHorizontal.t()).changed() {
                    geo.mirror_horizontal = mh;
                    changed = true;
                }
                let mut mv = geo.mirror_vertical;
                if ui.checkbox(&mut mv, Str::MirrorVertical.t()).changed() {
                    geo.mirror_vertical = mv;
                    changed = true;
                }
                ui.label(Str::Perspective.t());
                let mut persp = self.recipe.perspective.unwrap_or(Perspective {
                    version: 1,
                    vertical: 0.0,
                    horizontal: 0.0,
                    rotation: 0.0,
                    scale: 1.0,
                    aspect_ratio: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0,
                });
                let mut changed_p = false;
                for (field, label, spec) in [
                    (
                        &mut persp.vertical,
                        Str::Vertical,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.horizontal,
                        Str::Horizontal,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.rotation,
                        Str::Rotation,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.scale,
                        Str::Scale,
                        identity_spec(0.1..=10.0, 1.0, 0.01),
                    ),
                    (
                        &mut persp.aspect_ratio,
                        Str::AspectRatio,
                        identity_spec(0.1..=10.0, 1.0, 0.01),
                    ),
                    (
                        &mut persp.shift_x,
                        Str::ShiftX,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut persp.shift_y,
                        Str::ShiftY,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                ] {
                    let mut v = *field;
                    match lr_slider(ui, label.t(), &mut v, spec) {
                        SliderAction::Changed | SliderAction::ResetRequested => {
                            *field = v;
                            changed_p = true;
                        }
                        SliderAction::Nothing => {}
                    }
                }
                if changed {
                    self.recipe.geometry = Some(geo);
                    self.mark_dirty();
                }
                if changed_p {
                    self.recipe.perspective = Some(persp);
                    self.mark_dirty();
                }
            } else {
                ui.label(Str::GeometryRequiresLensfun.t());
                ui.label(Str::NotAvailable.t());
            }
        });
    }

    fn draw_masking(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        ui.collapsing(Str::Masking.t(), |ui| {
            let Some(document) = self.document.clone() else {
                return;
            };
            let mask_options: Vec<(String, String)> = document
                .virtual_copies
                .iter()
                .find(|c| c.id == self.virtual_copy_id)
                .map(|c| {
                    c.mask_library
                        .iter()
                        .map(|m| (m.id.clone(), m.name.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let mut selected_mask = self.selected_mask_id.clone().unwrap_or_default();
            egui::ComboBox::from_label(Str::SelectMask.t())
                .selected_text(
                    mask_options
                        .iter()
                        .find(|(id, _)| id == &selected_mask)
                        .map(|(_, name)| name.as_str())
                        .unwrap_or("None"),
                )
                .show_ui(ui, |ui| {
                    for (id, name) in &mask_options {
                        ui.selectable_value(&mut selected_mask, id.clone(), name);
                    }
                });
            if selected_mask != self.selected_mask_id.clone().unwrap_or_default()
                && !selected_mask.is_empty()
            {
                if let Err(e) = self.select_mask(&selected_mask) {
                    self.show_error(e);
                }
            }
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.mask_name_input);
                if ui.button(Str::NewMask.t()).clicked() {
                    if let Err(e) = self.create_mask(self.mask_name_input.clone()) {
                        self.show_error(e);
                    } else {
                        self.mask_name_input.clear();
                    }
                }
            });
            // F-103-N4: interactive mask tools. The tool only picks how a drag on
            // the preview is interpreted; persistence goes through the sidecar.
            ui.separator();
            ui.label(Str::MaskTool.t());
            // REVIEW-GUI-MASKGEO-1: while recipe geometry is active the drawn
            // coordinates would land transformed-wrong, so the tool row is
            // disabled and an explicit hint explains why (no silent fallback).
            let geometry_blocked = self.geometry_blocks_source_mapping();
            ui.add_enabled_ui(!geometry_blocked, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tool, label) in [
                        (MaskTool::Brush, Str::MaskToolBrush),
                        (MaskTool::LinearGradient, Str::MaskToolGradient),
                        (MaskTool::Radial, Str::MaskToolRadial),
                    ] {
                        if ui
                            .selectable_label(self.mask_tool == tool, label.t())
                            .clicked()
                        {
                            self.set_mask_tool(tool);
                        }
                    }
                    if ui
                        .selectable_label(self.mask_tool == MaskTool::None, Str::MaskToolNone.t())
                        .clicked()
                    {
                        self.set_mask_tool(MaskTool::None);
                    }
                });
            });
            if geometry_blocked {
                ui.colored_label(egui::Color32::YELLOW, Self::GEOMETRY_TOOL_BLOCKED);
            }
            if self.mask_tool == MaskTool::Brush {
                let mut radius = self.brush_radius;
                if ui
                    .add(egui::Slider::new(&mut radius, 0.005..=1.0).text(Str::BrushSize.t()))
                    .changed()
                {
                    if let Err(e) = self.set_brush_radius(radius) {
                        self.show_error(e);
                    }
                }
                ui.checkbox(&mut self.brush_eraser, Str::BrushEraser.t());
            }
            ui.label(Str::DrawMaskHint.t());
            if self.selected_mask_id.is_some() {
                let mut inverted = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .is_some_and(|layer| layer.inverted);
                if ui.checkbox(&mut inverted, Str::Invert.t()).changed() {
                    if let Err(e) = self.set_mask_inverted(inverted) {
                        self.show_error(e);
                    }
                }
                let mut feather = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(0.0, |layer| layer.feather);
                if ui
                    .add(egui::Slider::new(&mut feather, 0.0..=1.0).text(Str::Feather.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_feather(feather) {
                        self.show_error(e);
                    }
                }
                let mut blur = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(0.0, |layer| layer.blur);
                if ui
                    .add(egui::Slider::new(&mut blur, 0.0..=1.0).text(Str::Blur.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_blur(blur) {
                        self.show_error(e);
                    }
                }
                let mut density = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(1.0, |layer| layer.density);
                if ui
                    .add(egui::Slider::new(&mut density, 0.0..=1.0).text(Str::Density.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_density(density) {
                        self.show_error(e);
                    }
                }
                if ui.button(Str::OfferRecalculation.t()).clicked() {
                    if let Err(e) = self.offer_mask_recalculation().and_then(|offered| {
                        if offered {
                            self.mark_mask_for_recalculation()
                        } else {
                            Ok(())
                        }
                    }) {
                        self.show_error(e);
                    }
                }
                ui.label(Str::LocalAdjustments.t());
                for (key, label) in [("exposure", Str::Exposure), ("contrast", Str::Contrast)] {
                    let stored = document
                        .virtual_copies
                        .iter()
                        .find(|c| c.id == self.virtual_copy_id)
                        .and_then(|c| c.mask_layers.first())
                        .and_then(|layer| layer.extras.get(&format!("adjustment_{key}")))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let range = if key == "exposure" {
                        -10.0..=10.0
                    } else {
                        -1.0..=1.0
                    };
                    let mut value = stored;
                    if ui
                        .add(egui::Slider::new(&mut value, range).text(label.t()))
                        .changed()
                    {
                        if let Err(e) = self.set_mask_local_adjustment(key, value) {
                            self.show_error(e);
                        }
                    }
                }
            }
        });
        #[cfg(target_arch = "wasm32")]
        ui.collapsing(Str::Masking.t(), |ui| {
            ui.label(Str::NotAvailable.t());
        });
    }

    /// Lightroom-like Library folder tree (left panel): directory hierarchy
    /// rooted at `$HOME` (or two ancestors above the current directory when it
    /// lives outside the home tree), lazily expanded via `read_dir`, showing a
    /// depth-limited RAW count per node. Clicking a node selects the directory.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_folder_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Folders.t());
        // Direct path entry stays available (replaces the old text browser's
        // address row) plus a manual rescan.
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.directory);
            if ui.button(Str::Open.t()).clicked() {
                let target = self.directory.clone();
                self.set_directory(target);
            }
        });
        if ui.button(Str::Refresh.t()).clicked() {
            self.list_directory();
        }
        ui.separator();
        let root = library_root(&self.directory);
        // The root itself is always visible/expanded.
        self.open_folders.insert(root.display().to_string());
        let mut select_target: Option<String> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.draw_folder_node(ui, &root, &root, 0, &mut select_target);
        });
        if let Some(path) = select_target {
            trace!("GUI interaction: folder select {}", path);
            self.set_directory(path);
        }
    }

    /// One folder-tree node: disclosure arrow + label with RAW count, then the
    /// lazily cached children when expanded.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_folder_node(
        &mut self,
        ui: &mut egui::Ui,
        root: &Path,
        path: &Path,
        depth: usize,
        select_target: &mut Option<String>,
    ) {
        let path_str = path.display().to_string();
        // Depth-limited RAW count, computed once per folder and cached.
        if !self.folder_raw_counts.contains_key(&path_str) {
            let count = count_raw_files(path, FOLDER_SCAN_DEPTH);
            self.folder_raw_counts.insert(path_str.clone(), count);
        }
        let raw_count = self.folder_raw_counts[&path_str];
        let open = self.open_folders.contains(&path_str);
        ui.horizontal(|ui| {
            ui.add_space((depth * 14) as f32);
            // Disclosure toggle.
            let (arrow_rect, arrow_resp) =
                ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::click());
            ui.painter().text(
                arrow_rect.center(),
                egui::Align2::CENTER_CENTER,
                if open { "▾" } else { "▸" },
                egui::FontId::default(),
                ui.visuals().text_color(),
            );
            if arrow_resp.clicked() {
                if open {
                    self.open_folders.remove(&path_str);
                } else {
                    self.open_folders.insert(path_str.clone());
                }
            }
            let label = format!("{} ({})", folder_label(root, path), raw_count);
            if ui
                .selectable_label(self.directory == path_str, label)
                .clicked()
            {
                *select_target = Some(path_str.clone());
            }
        });
        if !open {
            return;
        }
        // Lazy children cache: fill via read_dir on first expansion.
        let children = match self.folder_children.get(&path_str) {
            Some(children) => children.clone(),
            None => {
                let children: Vec<String> = subdirectories(path)
                    .iter()
                    .map(|child| child.display().to_string())
                    .collect();
                self.folder_children
                    .insert(path_str.clone(), children.clone());
                children
            }
        };
        for child in children {
            self.draw_folder_node(ui, root, Path::new(&child), depth + 1, select_target);
        }
    }

    /// Lightroom-like Library grid view (center): RAW files of the current
    /// directory rendered through the shared ThumbnailManager pipeline (no
    /// duplicate generation). Double-click opens a file and switches to
    /// Develop (Loupe). The thumbnail cell size is user-adjustable via a
    /// toolbar slider (Lightroom "Grid" thumbnails).
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_library_grid(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // Toolbar: a thumbnail-size slider (Lightroom-like). Small/simple stub
        // for now — it drives the cell size of the grid below.
        ui.horizontal(|ui| {
            ui.label(Str::LibraryThumbSize.t());
            let mut size = self.library_thumb_size;
            if ui
                .add(
                    egui::Slider::new(&mut size, 72.0..=240.0)
                        .show_value(true)
                        .fixed_decimals(0),
                )
                .changed()
            {
                self.library_thumb_size = size.round();
            }
        });
        ui.separator();
        // GUI-SCROLL-200-1: index-based view over the RAW entries. Only the
        // visible rows are laid out (show_rows) and only the buffered window's
        // thumbnails are ensured per frame — never an O(n) loop over all
        // entries.
        let raw_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| is_raw_name(&entry.name))
            .map(|(i, _)| i)
            .collect();
        if raw_indices.is_empty() {
            ui.heading(Str::Library.t());
            ui.label(Str::ReadyForImage.t());
            return;
        }
        let thumb = self.library_thumb_size;
        const CELL_INNER_PAD: f32 = 8.0;
        let cell_inner = (thumb - CELL_INNER_PAD).max(32.0);
        let cols = ((ui.available_width() / thumb).floor() as usize).max(1);
        let count = raw_indices.len();
        let total_rows = count.div_ceil(cols);
        // The closure returns the laid-out row window so scheduling below runs
        // with the exact visible range.
        let visible_rows = {
            egui::ScrollArea::vertical()
                .show_rows(
                    ui,
                    cell_inner,
                    total_rows,
                    |ui, rows: std::ops::Range<usize>| {
                        for row in rows.clone() {
                            let row_start = row * cols;
                            let row_end = (row_start + cols).min(count);
                            ui.horizontal(|ui| {
                                for &entry_idx in &raw_indices[row_start..row_end] {
                                    let entry = self.entries[entry_idx].clone();
                                    let selected = self.path == entry.path.display().to_string();
                                    let tex = self.thumbnails.get(&entry.thumb_key).cloned();
                                    let placeholder_label =
                                        self.thumbnail_placeholder_label(&entry);
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(cell_inner, cell_inner),
                                        egui::Sense::click(),
                                    );
                                    if selected {
                                        ui.painter().rect_stroke(
                                            rect.expand(2.0),
                                            3.0,
                                            egui::Stroke::new(
                                                2.0_f32,
                                                ui.visuals().selection.bg_fill,
                                            ),
                                            egui::StrokeKind::Outside,
                                        );
                                    }
                                    if let Some(texture) = tex {
                                        ui.put(
                                            rect,
                                            egui::Image::from_texture(&texture)
                                                .max_size(rect.size()),
                                        );
                                    } else {
                                        ui.painter().rect_filled(
                                            rect,
                                            2.0,
                                            egui::Color32::from_gray(40),
                                        );
                                        ui.put(rect, egui::Label::new(placeholder_label));
                                    }
                                    if resp.double_clicked() {
                                        trace!(
                                            "GUI interaction: library grid open {}",
                                            entry.path.display()
                                        );
                                        self.open_file(entry.path.display().to_string());
                                        self.active_module = Module::Develop;
                                    }
                                    // Sidecar/copy status on hover (kept from the former
                                    // text file-browser).
                                    resp.on_hover_text(format!(
                                        "{}\n[{}] {}:{} {}:{}",
                                        entry.name,
                                        entry.status_label(),
                                        Str::Copies.t(),
                                        entry.virtual_copies,
                                        Str::Masking.t(),
                                        entry.missing_models
                                    ));
                                }
                            });
                        }
                        rows
                    },
                )
                .inner
        };
        // GUI-SCROLL-200-1: schedule thumbnail work only for the visible
        // window (+ buffer), then a bounded nearest-first off-screen prefetch.
        let window = visible_rows.start * cols..(visible_rows.end * cols).min(count);
        self.frame_thumb_enqueued += self.ensure_thumbnail_priority(ctx, &raw_indices, window);
    }

    /// Display-only crop/histogram thumbnail at the top of the right Develop
    /// panel (Lightroom-like): the current render texture with the recipe's
    /// free-crop rectangle overlaid and the outside dimmed. Purely visual —
    /// no interaction, no recipe mutation.
    fn draw_crop_thumb(&mut self, ui: &mut egui::Ui) {
        let Some(texture) = self.texture.clone() else {
            return;
        };
        let size = texture.size();
        let (tex_w, tex_h) = (size[0] as f32, size[1] as f32);
        let width = ui.available_width();
        let height = 120.0_f32.min(ui.available_height().min(160.0));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 2.0, egui::Color32::from_gray(24));
        if tex_w <= 0.0 || tex_h <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let scale = (rect.width() / tex_w).min(rect.height() / tex_h);
        let draw_size = egui::vec2(tex_w * scale, tex_h * scale);
        let img_rect = egui::Rect::from_center_size(rect.center(), draw_size);
        ui.put(
            img_rect,
            egui::Image::from_texture(&texture).max_size(draw_size),
        );
        let crop = self
            .recipe
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref());
        let Some(crop_rect) = crop_overlay_rect(crop, img_rect) else {
            return;
        };
        let painter = ui.painter();
        let dim = egui::Color32::from_black_alpha(140);
        // Four border bands around the crop window keep the outside dimmed
        // without a second texture pass.
        painter.rect_filled(
            egui::Rect::from_min_max(img_rect.min, egui::pos2(img_rect.max.x, crop_rect.top())),
            0.0,
            dim,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(img_rect.min.x, crop_rect.bottom()), img_rect.max),
            0.0,
            dim,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(img_rect.min.x, crop_rect.top()),
                egui::pos2(crop_rect.left(), crop_rect.bottom()),
            ),
            0.0,
            dim,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(crop_rect.right(), crop_rect.top()),
                egui::pos2(img_rect.max.x, crop_rect.bottom()),
            ),
            0.0,
            dim,
        );
        painter.rect_stroke(
            crop_rect,
            0.0,
            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 214, 0)),
            egui::StrokeKind::Inside,
        );
    }

    /// Lightroom-style Presets section (F-009): the file-backed preset list
    /// from the user-global presets directory (`<name>.lumina-preset.json`,
    /// click to apply, failing files stay visible with their error text), the
    /// save-to-file action, and the in-memory create/apply flow for the
    /// current field selection.
    fn draw_presets_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::PresetsSection.t(), |ui| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.draw_preset_file_list(ui);
                ui.separator();
            }
            ui.text_edit_singleline(&mut self.preset_name);
            for field in ["exposure", "contrast", "highlights", "shadows"] {
                let selected = self.preset_fields.entry(field.into()).or_insert(false);
                ui.checkbox(selected, field);
            }
            ui.checkbox(
                &mut self.preset_relative_exposure,
                Str::ExposureRelative.t(),
            );
            if ui.button(Str::ApplyPreset.t()).clicked() {
                match self
                    .create_preset(self.preset_name.clone())
                    .and_then(|preset| self.apply_preset(&preset))
                {
                    Ok(()) => self.status = "Preset applied, new history step".into(),
                    Err(error) => self.show_error(error),
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui.button(Str::SavePresetFile.t()).clicked() {
                match self.save_current_selection_as_preset_file() {
                    Ok(path) => {
                        trace!("GUI interaction: saved preset file {}", path.display());
                        self.status = Str::PresetSaved.format_arg(&path.display().to_string());
                    }
                    Err(error) => self.show_error(error),
                }
            }
        });
    }

    /// F-009: renders the file-backed preset list of `self.preset_entries`.
    /// The folder is shown so the storage location stays visible; every entry
    /// that failed validation is rendered with its error instead of being
    /// skipped silently. Entries are cloned first so clicking can borrow
    /// `self` mutably for [`Self::apply_preset`].
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_preset_file_list(&mut self, ui: &mut egui::Ui) {
        let Some(directory) = self.presets_dir.clone() else {
            ui.label(Str::PresetsUnavailable.t());
            return;
        };
        ui.horizontal(|ui| {
            ui.label(Str::PresetsFolder.t());
            ui.monospace(directory.display().to_string());
        });
        if ui.button(Str::Refresh.t()).clicked() {
            self.reload_preset_entries();
        }
        if self.preset_entries.is_empty() {
            ui.label(Str::NoPresets.t());
            return;
        }
        let entries = self.preset_entries.clone();
        for entry in &entries {
            match entry {
                presets::PresetEntry::Available { preset, .. } => {
                    if ui.selectable_label(false, &preset.name).clicked() {
                        trace!("GUI interaction: apply file preset {}", preset.name);
                        match self.apply_preset(preset) {
                            Ok(()) => self.status = Str::PresetApplied.format_arg(&preset.name),
                            Err(error) => self.show_error(error),
                        }
                    }
                }
                presets::PresetEntry::Failed { path, error } => {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("{name}: {error}"));
                }
            }
        }
    }

    /// F-009: rescans the user presets directory. Scan problems surface as
    /// failed entries inside the list, never as silent drops.
    #[cfg(not(target_arch = "wasm32"))]
    fn reload_preset_entries(&mut self) {
        if let Some(directory) = self.presets_dir.as_deref() {
            self.preset_entries = presets::scan_presets_dir(directory);
        }
    }

    /// F-009: persists the currently selected preset fields as
    /// `<name>.lumina-preset.json` in the user presets directory and refreshes
    /// the list. Overwriting an existing name is the documented update
    /// semantics (the display name is the identity and the list above shows
    /// the names before replacement); validation failures are loud errors.
    #[cfg(not(target_arch = "wasm32"))]
    fn save_current_selection_as_preset_file(&mut self) -> Result<std::path::PathBuf, GuiError> {
        let directory = self
            .presets_dir
            .clone()
            .ok_or_else(|| GuiError::Io(Str::PresetsUnavailable.t().to_string()))?;
        let preset = self.create_preset(self.preset_name.clone())?;
        let path = presets::save_preset_file(&directory, &preset, true)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        self.reload_preset_entries();
        Ok(path)
    }

    /// Lightroom-style History section: reverse-chronological entries of the
    /// active virtual copy; clicking an entry restores its stored recipe into
    /// the session recipe (non-destructive until Save Recipe / Sidecar).
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_history_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::History.t(), |ui| {
            let Some(document) = self.document.clone() else {
                ui.label(Str::NoSidecarLoaded.t());
                return;
            };
            let Some(copy) = document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
            else {
                ui.label(Str::VirtualCopyNotFound.t());
                return;
            };
            if copy.history.is_empty() {
                ui.label(Str::NoHistory.t());
                return;
            }
            let mut restore_target: Option<String> = None;
            for (index, entry) in copy.history.iter().enumerate().rev() {
                let mut label = format!("{}. {}", index + 1, entry.id);
                if let Some(recorded_at) = &entry.recorded_at {
                    label.push_str(&format!(" ({})", recorded_at));
                }
                let selected = self.history_selected.as_deref() == Some(entry.id.as_str());
                if ui.selectable_label(selected, label).clicked() {
                    restore_target = Some(entry.id.clone());
                }
            }
            if let Some(id) = restore_target {
                if let Err(error) = self.restore_history(&id) {
                    self.show_error(error);
                }
            }
        });
    }

    /// Normative draw order of the eight F-100 Develop sections, rendered by
    /// [`LuminaApp::draw_develop_panel`] in exactly this sequence.
    /// Lightroom Classic panel order: Basic → Tone Curve → HSL/Color →
    /// Color Grading → **Detail → Effects** → Optics → Geometry → Masking.
    ///
    /// F-103-N10 (user decision 2026-08-25): **Detail BEFORE Effects** —
    /// Sharpening/Noise Reduction are shown above Vignette/Grain, matching
    /// Lightroom Classic (previously Effects was drawn first; SOLL and GUI
    /// were aligned in `feature/platform/cli-gui-wasm.md` § UI-Konventionen).
    ///
    /// Collapse state is keyed by the section label (egui auto-IDs), not by
    /// position, so reordering here neither changes nor resets user collapse
    /// state. The order is pinned by the
    /// `develop_section_order_is_lightroom_conform` test below.
    const DEVELOP_SECTIONS: &[(Str, DevelopSectionDraw)] = &[
        (Str::Basic, LuminaApp::draw_basic),
        (Str::ToneCurve, LuminaApp::draw_tone_curve),
        (Str::Color, LuminaApp::draw_color),
        (Str::Detail, LuminaApp::draw_detail),
        (Str::Effects, LuminaApp::draw_effects),
        (Str::Optics, LuminaApp::draw_optics),
        (Str::Geometry, LuminaApp::draw_geometry),
        (Str::Masking, LuminaApp::draw_masking),
    ];

    /// The full Develop control stack: the eight F-100 sections in fixed order,
    /// then the preset manager and the global render/save actions.  Every
    /// adjustment uses [`lr_slider`] so the F-100 reset/scroll/scale rules apply.
    fn draw_develop_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Lightroom-style panel head: display-only crop/histogram
                // thumbnail with the active crop overlay, then the Presets and
                // History collapsible sections.
                self.draw_crop_thumb(ui);
                self.draw_presets_section(ui);
                #[cfg(not(target_arch = "wasm32"))]
                self.draw_history_section(ui);
                ui.separator();
                // The eight adjustment sections are grayed and non-interactive until an
                // image is loaded (F-100 disabled-while-empty behaviour).
                ui.add_enabled_ui(self.original.is_some(), |ui| {
                    // F-100 section order (incl. F-103-N10: Detail BEFORE
                    // Effects) has its single source of truth in
                    // `DEVELOP_SECTIONS`; see there.
                    for (_, draw_section) in Self::DEVELOP_SECTIONS {
                        draw_section(self, ui);
                    }
                });
                ui.separator();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.path);
                        if ui.button(Str::Load.t()).clicked() {
                            self.begin_load_path(self.path.clone());
                        }
                    });
                    if ui.button(Str::ChooseFile.t()).clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            // REVIEW-GUI-PATHDESYNC-1: no immediate
                            // `self.path` commit; `finish_decode` adopts the
                            // path after a successful decode.
                            self.begin_load_path(path.display().to_string());
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label(Str::NotAvailable.t());
                }
                if ui.button(Str::MatchExposure.t()).clicked() {
                    if let Err(error) = self.match_total_exposure(0.5) {
                        self.show_error(error);
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button(Str::Reset.t()).clicked() {
                        self.reset();
                    }
                    if ui.button(Str::RenderApply.t()).clicked() {
                        if let Err(error) = self.render() {
                            self.show_error(error);
                        }
                    }
                });
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button(Str::SaveRecipe.t()).clicked() {
                    self.save_sidecar();
                }
            });
    }

    /// Library-module sidecar / virtual-copy manager (native only).  Mask editing
    /// lives in the Develop panel's Masking section; here the user picks which
    /// source copy to work on and can duplicate it.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_filmstrip(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.heading(Str::Filmstrip.t());
        ui.label(Str::FilmstripHint.t());
        // RAW-only: the Develop/Lightroom preview pipeline is RAW-first, so the
        // filmstrip never shows jpg/png/webp/raster entries (those remain
        // browseable in the Library file-browser via `is_supported_image`).
        let raw_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| is_raw_name(&e.name))
            .map(|(i, _)| i)
            .collect();
        let count = raw_indices.len();
        // GUI-SCROLL-200-1: fixed-size cells let us lay out only the visible
        // window (+ a small buffer). Off-screen cells are never allocated,
        // painted or probed for thumbnails on this frame.
        // Lightroom-like filmstrip cell: the larger 140x110 cell (was 110x84)
        // keeps the strip readable on high-DPI displays ("switching too small").
        const CELL_W: f32 = 140.0;
        const CELL_H: f32 = 110.0;
        let step = CELL_W + ui.spacing().item_spacing.x;
        // The closure returns the visible window it laid out so thumbnail
        // scheduling below runs *after* drawing with no extra state.
        let visible = {
            egui::ScrollArea::horizontal()
                .show_viewport(ui, |ui, viewport| {
                    ui.set_height(CELL_H);
                    let visible = viewport::visible_cell_range(
                        viewport.left(),
                        viewport.width(),
                        step,
                        count,
                    );
                    // Leading spacer positions the first *drawn* cell at its
                    // absolute content position; `set_width` keeps the
                    // scrollbar proportional to the full strip even though
                    // only the window is laid out.
                    let buffered = viewport::buffered_range(
                        visible.clone(),
                        count,
                        viewport::VISIBLE_BUFFER_CELLS,
                    );
                    // R2-GUI-FILMSTRIP-ROW: the filmstrip must lay out as one
                    // horizontal row inside the horizontally scrolled area.
                    // `ScrollArea::horizontal()` only enables the horizontal
                    // scrollbar — it does NOT change the child UI's layout
                    // direction, which would otherwise stay top-down (vertical)
                    // and stack the cells into a column. The horizontal wrapper
                    // restores the single-row filmstrip (this was lost in the
                    // GUI-SCROLL-200-1 virtualization refactor).
                    ui.horizontal(|ui| {
                        // Leading spacer positions the first *drawn* cell at its
                        // absolute content position; `set_width` keeps the
                        // scrollbar proportional to the full strip even though
                        // only the window is laid out.
                        if buffered.start > 0 {
                            ui.add_space(buffered.start as f32 * step);
                        }
                        for i in buffered.clone() {
                            let entry = self.entries[raw_indices[i]].clone();
                            let tex = self.thumbnails.get(&entry.thumb_key).cloned();
                            let placeholder_label = self.thumbnail_placeholder_label(&entry);
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(CELL_W, CELL_H),
                                egui::Sense::click(),
                            );
                            if let Some(texture) = tex {
                                ui.put(
                                    rect,
                                    egui::Image::from_texture(&texture).max_size(rect.size()),
                                );
                            } else {
                                ui.painter()
                                    .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                                ui.put(rect, egui::Label::new(placeholder_label));
                            }
                            if resp.clicked() {
                                trace!("GUI interaction: filmstrip click {}", entry.path.display());
                                self.open_file(entry.path.display().to_string());
                            }
                        }
                        let total_width =
                            (count as f32 * step - ui.spacing().item_spacing.x).max(0.0);
                        ui.set_width(total_width);
                    });
                    visible
                })
                .inner
        };
        // GUI-SCROLL-200-1: visible-first thumbnail scheduling (see
        // `ensure_thumbnail_priority`). No O(n) per-frame loop anymore.
        self.frame_thumb_enqueued += self.ensure_thumbnail_priority(ctx, &raw_indices, visible);
    }

    /// GUI-SCROLL-200-1: visible-first thumbnail scheduling.
    ///
    /// Enqueues thumbnail work for the entries in `visible_window` (widened by
    /// [`viewport::VISIBLE_BUFFER_CELLS`]) first and unconditionally; then
    /// touches at most [`viewport::PREFETCH_BUDGET_PER_FRAME`] off-screen
    /// entries, nearest to the window first ([`viewport::prefetch_order`]).
    /// Entries that already have a texture or an in-flight job are skipped for
    /// free (`ThumbnailManager::needs_job`) and never consume budget.
    ///
    /// This ordering *is* the job priority mechanism: the worker pool drains
    /// the unbounded FIFO channel in order, so a visible cell's job is always
    /// enqueued — and therefore started — before any prefetched off-screen
    /// job of the same frame. Off-screen work is additionally rate-limited to
    /// keep the worst-case per-frame disk-cache probes bounded.
    ///
    /// Returns how many worker jobs were enqueued / cached previews loaded
    /// this call (fed into the `LUMINA_PERF_LOG` counters).
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_thumbnail_priority(
        &mut self,
        ctx: &egui::Context,
        raw_indices: &[usize],
        visible_window: std::ops::Range<usize>,
    ) -> usize {
        let count = raw_indices.len();
        let buffered =
            viewport::buffered_range(visible_window, count, viewport::VISIBLE_BUFFER_CELLS);
        let mut enqueued = 0;
        // Pass 1: the buffered visible window — always fully ensured, no cap.
        for i in buffered.clone() {
            let entry = self.entries[raw_indices[i]].clone();
            if self.ensure_thumbnail(ctx, &entry) {
                enqueued += 1;
            }
        }
        // Pass 2: bounded nearest-first off-screen prefetch.
        let mut budget = viewport::PREFETCH_BUDGET_PER_FRAME;
        for i in viewport::prefetch_order(count, buffered) {
            if budget == 0 {
                break;
            }
            let key = &self.entries[raw_indices[i]].thumb_key;
            // Free check: skips cells with a texture / in-flight job without
            // any disk IO. Only real candidates consume the per-frame budget.
            if !self.thumbnails.needs_job(key) {
                continue;
            }
            budget -= 1;
            let entry = self.entries[raw_indices[i]].clone();
            if self.ensure_thumbnail(ctx, &entry) {
                enqueued += 1;
            }
        }
        enqueued
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Ensure a thumbnail exists for `entry`.
    ///
    /// Returns `true` when this call did potentially expensive work: enqueued a
    /// background worker job or synchronously loaded/inserted a cached preview
    /// from the disk cache. Callers feed this into the `LUMINA_PERF_LOG`
    /// frame counters (GUI-SCROLL-200-1). `false` means the call was cheap
    /// (texture already present, job in flight, retry budget exhausted).
    fn ensure_thumbnail(&mut self, ctx: &egui::Context, entry: &FileBrowserEntry) -> bool {
        // Key is the canonicalized absolute path, never the bare filename
        // (REVIEW-GUI-THUMB-1).
        let key = entry.thumb_key.clone();
        if self.thumbnails.get(&key).is_some() {
            return false;
        }
        if !self.thumbnails.needs_job(&key) {
            return false;
        }
        if let Ok(cache) = DiskFolderCache::for_image(entry.path.as_path()) {
            // Use the headless-testable cache probe; on a hit, load and display
            // the stored preview.  A miss enqueues a background thumbnail job
            // (no silent fallback to a wrong/sized-up image).
            if filmstrip::filmstrip_preview_cached(&cache, &entry.name, "vc-original") {
                if let Ok(Some(bytes)) =
                    cache.load_preview(&entry.name, "vc-original", PreviewKind::Standard)
                {
                    match ImageFrame::decode(&bytes) {
                        Ok(frame) => {
                            let tex = self.make_thumbnail_texture(ctx, &frame, &key);
                            // insert marks the key probed *after* success only
                            // (REVIEW-GUI-THUMB-2).
                            self.thumbnails.insert(&key, tex);
                            return true;
                        }
                        Err(error) => {
                            // A cached-but-corrupt preview is a visible error,
                            // not a silent miss.
                            self.thumbnails
                                .mark_failed(&key, format!("cached preview unreadable: {error}"));
                            return true;
                        }
                    }
                }
            }
        }
        // Cache miss: enqueue a background thumbnail job on the dedicated thread
        // pool rather than the bounded `IdleQueue`. The channel is unbounded, so
        // it never drops jobs under load. The key is marked in-flight (NOT
        // probed) so a worker failure can retry in a bounded way and surface a
        // visible error instead of a permanent gray cell (REVIEW-GUI-THUMB-2).
        self.thumbnails.begin_job(&key);
        match self.thumbnail_tx.send(ThumbnailJob {
            source: entry.path.clone(),
            name: entry.name.clone(),
            key,
        }) {
            Ok(()) => {
                debug!("enqueued thumbnail job for {}", entry.name);
                true
            }
            Err(_) => {
                // Channel closed: release the in-flight slot so a later frame
                // retries once the pool is back.
                self.thumbnails.job_dispatch_failed(&entry.thumb_key);
                debug!(
                    "thumbnail channel closed; will retry {} on a later frame",
                    entry.name
                );
                false
            }
        }
    }

    /// Placeholder caption for a thumbnail cell: the filename, plus the visible
    /// failure message once the retry budget is exhausted
    /// (REVIEW-GUI-THUMB-2 — never a silent gray cell).
    #[cfg(not(target_arch = "wasm32"))]
    fn thumbnail_placeholder_label(&self, entry: &FileBrowserEntry) -> String {
        match self.thumbnails.failure(&entry.thumb_key) {
            Some(message) => format!("{} ⚠ {}", entry.name, message),
            None => entry.name.clone(),
        }
    }

    /// REVIEW-GUI-WASM-FOLLOWUP: thumbnail textures are produced by the native
    /// worker pool only.
    #[cfg(not(target_arch = "wasm32"))]
    fn make_thumbnail_texture(
        &self,
        ctx: &egui::Context,
        frame: &ImageFrame,
        key: &str,
    ) -> egui::TextureHandle {
        let size = [frame.width as usize, frame.height as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
        ctx.load_texture(format!("thumb-{key}"), image, egui::TextureOptions::LINEAR)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Central working area: a zoom toolbar (Lightroom-like Fit / 1:1 / 200% /
    /// Fit Width + a live zoom readout and a collapsed-navigator reopen button),
    /// then the rendered preview and the render-state label. Shared by the
    /// Develop and Export modules.
    fn draw_preview_area(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                if !self.navigator_open {
                    if ui.button(Str::Navigator.t()).clicked() {
                        self.navigator_open = true;
                        trace!("GUI interaction: navigator open");
                    }
                    ui.separator();
                }
            }
            ui.label(Str::Preview.t());
            ui.separator();
            self.zoom_toolbar(ui);
        });
        self.update_texture(ctx);
        self.draw_preview(ui);
        if let Some(key) = &self.render_key {
            ui.label(format!(
                "{}: {}",
                Str::RenderStateCurrent.t(),
                &key.digest()[..12]
            ));
        } else {
            ui.colored_label(egui::Color32::YELLOW, Str::RenderStateStale.t());
        }
        // R2-GUIMOD-06: surface the otherwise-silent GPU→CPU routing fallback
        // as a visible status badge (with tooltip) instead of only a stderr
        // `log::warn!`. No-op while `gpu_route_fallback` is `None` (GPU present
        // path usable, or no GPU context bound at all).
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
        if let Some(reason) = &self.gpu_route_fallback {
            ui.colored_label(egui::Color32::YELLOW, reason)
                .on_hover_text(Str::CpuFallbackTooltip.t().to_string());
        }
    }

    /// Lightroom-like zoom toolbar: absolute zoom modes (re-derived each frame
    /// from the pane) plus a live zoom percentage readout. The active mode is
    /// highlighted. Native-only (REVIEW-GUI-WASM-FOLLOWUP): rendered by the
    /// native preview-area header.
    #[cfg(not(target_arch = "wasm32"))]
    fn zoom_toolbar(&mut self, ui: &mut egui::Ui) {
        let pct = (self.preview_effective_scale * 100.0) as i32;
        ui.label(format!("{}: {}%", Str::Zoom.t(), pct));
        if ui
            .selectable_label(self.zoom_mode == ZoomMode::Fit, Str::ZoomFit.t())
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::Fit);
        }
        if ui
            .selectable_label(self.zoom_mode == ZoomMode::OneToOne, Str::ZoomOneToOne.t())
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::OneToOne);
        }
        if ui
            .selectable_label(
                self.zoom_mode == ZoomMode::TwoHundred,
                Str::ZoomTwoHundred.t(),
            )
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::TwoHundred);
        }
        if ui
            .selectable_label(self.zoom_mode == ZoomMode::FitWidth, Str::ZoomFitWidth.t())
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::FitWidth);
        }
    }

    /// Left thumbnail navigator rail (Lightroom-like). Reuses the filmstrip
    /// [`Self::ensure_thumbnail`] / [`ThumbnailManager`] pipeline — no duplicate
    /// thumbnail generation — shows a vertical scroll of directory entries,
    /// highlights the active image and opens an entry on click.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_navigator(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(Str::Navigator.t());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("‹").clicked() {
                    self.navigator_open = false;
                    trace!("GUI interaction: navigator collapse");
                }
            });
        });
        ui.separator();
        ui.label(Str::FilmstripHint.t());
        // RAW-only: mirror the filmstrip filter so the left navigator rail shows
        // only RAW entries (jpg/png/webp are excluded from the Develop preview).
        // GUI-SCROLL-200-1: index view + `show_rows` — one fixed-height row per
        // entry, only the visible window is laid out and scheduled.
        let raw_indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| is_raw_name(&e.name))
            .map(|(i, _)| i)
            .collect();
        let count = raw_indices.len();
        const CELL_W: f32 = 120.0;
        const CELL_H: f32 = 90.0;
        let active_path = self.path.clone();
        let visible_rows = egui::ScrollArea::vertical()
            .show_rows(ui, CELL_H, count, |ui, rows: std::ops::Range<usize>| {
                for i in rows.clone() {
                    // Reuse the filmstrip thumbnail pipeline (no duplicate
                    // generation): ensure_thumbnail populates the shared
                    // ThumbnailManager entry.
                    let entry = self.entries[raw_indices[i]].clone();
                    self.ensure_thumbnail(ctx, &entry);
                    let tex = self.thumbnails.get(&entry.thumb_key).cloned();
                    let placeholder_label = self.thumbnail_placeholder_label(&entry);
                    let active = active_path == entry.path.display().to_string();
                    let (cell, resp) =
                        ui.allocate_exact_size(egui::vec2(CELL_W, CELL_H), egui::Sense::click());
                    if let Some(texture) = tex {
                        ui.put(
                            cell,
                            egui::Image::from_texture(&texture).max_size(cell.size()),
                        );
                    } else {
                        ui.painter()
                            .rect_filled(cell, 2.0, egui::Color32::from_gray(40));
                        ui.put(cell, egui::Label::new(placeholder_label));
                    }
                    if active {
                        ui.painter().rect_stroke(
                            cell,
                            2.0_f32,
                            egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
                            egui::StrokeKind::Middle,
                        );
                    }
                    // PREVIEW-CACHE-FEATURE (A2): visible per-cell neighbor-preview
                    // state („wird vorbereitet / Veraltet / Fehler"), never only in
                    // logs. The thumb_key is the canonical path used as the probe id.
                    if let Some((text, color)) = self.neighbor_preview_badge(&entry.thumb_key) {
                        let corner_max = CELL_W.min(CELL_H) * 0.5;
                        let badge_w = corner_max + text.len() as f32 * 5.5 + 8.0;
                        let badge_h = corner_max + 8.0;
                        let badge_rect = egui::Rect::from_min_size(
                            egui::pos2(cell.min.x + 2.0, cell.min.y + 2.0),
                            egui::vec2(badge_w, badge_h),
                        );
                        ui.painter().rect_filled(badge_rect, 3.0, color);
                        ui.painter().text(
                            badge_rect.min + egui::vec2(5.0, 5.0),
                            egui::Align2::LEFT_TOP,
                            text,
                            egui::FontId::proportional(10.0),
                            egui::Color32::WHITE,
                        );
                    }
                    if resp.clicked() {
                        trace!("GUI interaction: navigator open {}", entry.path.display());
                        self.open_file(entry.path.display().to_string());
                    }
                }
                rows
            })
            .inner;
        // GUI-SCROLL-200-1: visible-first scheduling + bounded prefetch for the
        // rail as well; show_rows covers the drawing side.
        let window = visible_rows.start..visible_rows.end.min(count);
        self.frame_thumb_enqueued += self.ensure_thumbnail_priority(ctx, &raw_indices, window);
    }
}

/// The four Lightroom parametric tone-curve regions (Shadows, Darks, Lights,
/// Highlights) as the GUI's source of truth.  They are persisted as a master
/// [`Curves`] point list via [`build_tone_curve`]; the read-back keeps the
/// slider values stable for typical (unclamped) adjustments.
fn tone_curve_regions(recipe: &EditRecipe) -> (f64, f64, f64, f64) {
    let points = recipe
        .curves
        .as_ref()
        .map(|c| c.master.clone())
        .unwrap_or_default();
    tone_curve_regions_from_points(&points)
}

/// Read-back of the four region deltas from stored curve points
/// (REVIEW-GUI-CURVE-1): kept separate so the roundtrip-loss detection can
/// evaluate the exact same math without constructing an [`EditRecipe`].
fn tone_curve_regions_from_points(points: &[CurvePoint]) -> (f64, f64, f64, f64) {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let mut out = (0.0f64, 0.0, 0.0, 0.0);
    let vals: [f64; 4] = std::array::from_fn(|i| {
        let bx = base[i];
        let out_v = points.get(i).map(|p| p.output as f64).unwrap_or(bx);
        (out_v - bx).clamp(-1.0, 1.0)
    });
    out.0 = vals[0];
    out.1 = vals[1];
    out.2 = vals[2];
    out.3 = vals[3];
    out
}

/// Persist the four region values as a master [`Curves`] point list.  Outputs
/// stay in `[0,1]` so the render pipeline never sees an out-of-range control
/// point; extreme region values are clamped (a documented MVP simplification).
fn build_tone_curve(shadows: f64, darks: f64, lights: f64, highlights: f64) -> Curves {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let deltas = [shadows, darks, lights, highlights];
    let master: Vec<CurvePoint> = base
        .iter()
        .zip(deltas.iter())
        .map(|(bx, d)| CurvePoint {
            input: *bx as f32,
            output: ((bx + d).clamp(0.0, 1.0)) as f32,
        })
        .collect();
    Curves {
        version: 1,
        master,
        channels: CurveChannels::default(),
    }
}

/// REVIEW-GUI-CURVE-1: true when building the master curve from the four
/// region deltas loses part of a delta because the clamped `[0,1]` output
/// absorbs it — most visibly Shadows, whose base point is `0.0`, so any
/// negative delta clamps to "no change" and the slider would snap back to 0.
/// Storing raw deltas would be a recipe-schema change (`Curves` outputs are
/// normatively `[0,1]`), so the GUI instead surfaces this limit explicitly in
/// the UI instead of letting the slider move silently.
///
/// The comparison carries an epsilon because outputs are stored as `f32`:
/// read-back noise (~1e-7) must not be reported as clamp loss; real losses
/// are multiples of the slider step (≥1e-2).
fn tone_curve_roundtrip_is_lossy(shadows: f64, darks: f64, lights: f64, highlights: f64) -> bool {
    const EPSILON: f64 = 1e-3;
    let curve = build_tone_curve(shadows, darks, lights, highlights);
    let (rs, rd, rl, rh) = tone_curve_regions_from_points(&curve.master);
    (rs - shadows).abs() > EPSILON
        || (rd - darks).abs() > EPSILON
        || (rl - lights).abs() > EPSILON
        || (rh - highlights).abs() > EPSILON
}

/// Idle-debounce window (seconds) before the pending full-quality render is
/// committed after the last edit (PERF-GUI-3/4).
const FULL_RENDER_DEBOUNCE_SECONDS: f64 = 0.150;

/// REVIEW-GUI-DEBOUNCE-1: pure decision helper for the debounced full render.
///
/// Returns `Some(remaining_seconds)` while the wait window is still open (the
/// caller must schedule a repaint for exactly that long), and `None` when the
/// debounce has elapsed or no drag time was recorded (`last_edit_time == 0.0`
/// → immediate render). Kept pure so the stranding fix is unit-testable
/// without an event loop.
fn full_render_debounce_remaining(last_edit_time: f64, now: f64) -> Option<f64> {
    if last_edit_time <= 0.0 {
        return None;
    }
    let remaining = FULL_RENDER_DEBOUNCE_SECONDS - (now - last_edit_time);
    (remaining > 0.0).then_some(remaining)
}

/// Mutable reference to one HSL mixer channel, creating the `Option` slot on
/// first use so the GUI never has to special-case `None`.
fn hsl_channel_mut<'a>(hsl: &'a mut HslAdjustments, ch: &str) -> &'a mut HslChannel {
    let slot = match ch {
        "red" => &mut hsl.red,
        "orange" => &mut hsl.orange,
        "yellow" => &mut hsl.yellow,
        "green" => &mut hsl.green,
        "cyan" => &mut hsl.cyan,
        "blue" => &mut hsl.blue,
        "violet" => &mut hsl.violet,
        "magenta" => &mut hsl.magenta,
        _ => &mut hsl.red,
    };
    slot.get_or_insert_with(HslChannel::default)
}

#[cfg(not(target_arch = "wasm32"))]
fn is_supported_image(path: &Path) -> bool {
    // The file browser lists all editable formats; the filmstrip display applies
    // its own RAW-only filter (see `draw_filmstrip`). v1: PNG/JPEG/WebP plus the
    // RAW extensions.
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp") => true,
        _ => is_raw_name(&path.display().to_string()),
    }
}

/// Human-readable label for an [`ImageFileFormat`] used by the Export panel.
/// Native-only (REVIEW-GUI-WASM-FOLLOWUP): the wasm Export module is a
/// capability hint without a format picker.
#[cfg(not(target_arch = "wasm32"))]
fn format_label(format: ImageFileFormat) -> &'static str {
    match format {
        ImageFileFormat::Png => "PNG",
        ImageFileFormat::Jpeg => "JPEG",
        ImageFileFormat::WebP => "WebP",
    }
}

fn is_raw_name(name: &str) -> bool {
    // Single source of truth: delegate to `lumina_raw::is_raw_extension`
    // (RAW_EXTENSIONS lives there). R2-CLI-01 already consolidated the same
    // list for the CLI path; keeping one canonical extension list prevents the
    // two crates from drifting and re-introducing the silent 9-of-18 skip the
    // review flagged. Matching stays ASCII-case-insensitive.
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(lumina_raw::is_raw_extension)
}

/// Root of the Library folder tree — the **workdir** itself (the current
/// `directory` field), per Lightroom-parity: the Folders panel shows the
/// working directory as the root, not the whole `$HOME` tree. Pure path logic
/// so headless tests can exercise it without mutating process environment
/// state. Previously this rooted at `$HOME` (or a grandparent); the user asked
/// for root = workdir so the Library only ever browses the opened folder.
#[cfg(not(target_arch = "wasm32"))]
fn library_root(directory: &str) -> PathBuf {
    let dir = Path::new(directory).to_path_buf();
    if dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        dir
    }
}

/// Short display label of a folder node: path relative to the tree root, or
/// the final component for the root itself.
#[cfg(not(target_arch = "wasm32"))]
fn folder_label(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        _ => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
    }
}

/// How many directory levels the RAW-count scan descends at most. Keeps the
/// per-folder count cheap even under large trees.
#[cfg(not(target_arch = "wasm32"))]
const FOLDER_SCAN_DEPTH: usize = 3;

/// How much larger than the strictly visible window the zoom ROI is rendered
/// (REVIEW-GUI-PANROI-1): the extra border is panning headroom so the hand
/// tool always has off-screen content to drag into view without waiting for a
/// re-render. 1.0 would render exactly the visible window (no pan slack);
/// larger values trade render cost for smoother panning.
const PREVIEW_ROI_MARGIN: f64 = 1.3;

/// PERF-GUI-1: byte budget of the in-RAM base-stage cache
/// ([`lumina_core::StageFrameCache`]). Holds prepared, pre-adjustment frames
/// (post decode/source-actions/ROI-crop) so an exposure/color slider change
/// re-renders only the adjustment stage instead of re-running the crop +
/// source-action head and re-hashing the whole source file per tick. Native
/// desktop gets a generous budget; wasm32 a small one (browser heap).
#[cfg(not(target_arch = "wasm32"))]
const BASE_STAGE_CACHE_MAX_BYTES: usize = 512 * 1024 * 1024;
#[cfg(target_arch = "wasm32")]
const BASE_STAGE_CACHE_MAX_BYTES: usize = 48 * 1024 * 1024;

/// Number of RAW files under `dir`, descending at most `remaining_depth`
/// directory levels (depth 0 scans nothing). Pure read-only helper used by the
/// Library folder tree.
#[cfg(not(target_arch = "wasm32"))]
fn count_raw_files(dir: &Path, remaining_depth: usize) -> usize {
    if remaining_depth == 0 {
        return 0;
    }
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_raw_files(&path, remaining_depth - 1);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if is_raw_name(name) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Immediate subdirectories of `dir`, sorted; empty when unreadable so a
/// permission error degrades to "no children" instead of a broken node.
#[cfg(not(target_arch = "wasm32"))]
fn subdirectories(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// Maps the recipe's normalized free-crop rectangle (`Crop::Free` coordinates
/// are `0..=1`) into an on-screen image rect for the display-only crop overlay
/// thumbnail. Returns `None` for no crop / aspect presets (whose normalized
/// rect depends on the decoded aspect ratio and is not tracked here).
fn crop_overlay_rect(crop: Option<&Crop>, img_rect: egui::Rect) -> Option<egui::Rect> {
    let Some(Crop::Free {
        x,
        y,
        width,
        height,
    }) = crop
    else {
        return None;
    };
    if *width <= 0.0 || *height <= 0.0 {
        return None;
    }
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    let min = img_rect.min
        + egui::vec2(
            clamp01(*x) * img_rect.width(),
            clamp01(*y) * img_rect.height(),
        );
    let max = img_rect.min
        + egui::vec2(
            clamp01(x + width) * img_rect.width(),
            clamp01(y + height) * img_rect.height(),
        );
    Some(egui::Rect::from_min_max(min, max))
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_stale_auto_tone(recipe: &mut EditRecipe) {
    recipe.auto_features.auto_exposure = None;
    recipe.auto_features.auto_contrast = None;
    recipe.adjustments.remove("exposure");
    recipe.adjustments.remove("contrast");
}

#[cfg(not(target_arch = "wasm32"))]
fn is_current_tone_analysis(stored: &AnalysisFingerprint, input_fingerprint: &str) -> bool {
    stored.input_fingerprint == input_fingerprint
}

#[cfg(not(target_arch = "wasm32"))]
fn decoder_identity(source_is_raw: bool) -> &'static str {
    if source_is_raw {
        "libraw"
    } else {
        "image"
    }
}

impl eframe::App for LuminaApp {
    // eframe 0.36: `update(&mut self, ctx, frame)` was replaced by
    // `ui(&mut self, ui, frame)`; the context is cloned off the root `Ui`.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let perf_t0 = if std::env::var("LUMINA_PERF_LOG").as_deref() == Ok("1") {
            Some(std::time::Instant::now())
        } else {
            None
        };
        // Apply the Lumina dark theme once per frame. `egui` only re-applies the
        // fields that changed, so this is cheap and keeps the Lightroom feeling
        // consistent across modules.
        apply_lightroom_dark(&ctx);

        // Keyboard: `Y` toggles Before/After (which never mutates the recipe);
        // `Esc` cancels an armed white-balance eyedropper.
        if ctx.input(|i| i.key_pressed(egui::Key::Y)) {
            self.toggle_before_after();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.wb_pick_mode = false;
        }

        // Module-switch shortcuts (`G` Library, `D` Develop, `E` Library alias).
        // They are ignored while a widget wants keyboard input — e.g. a focused
        // text field for mask/preset names — so they cannot hijack typing.
        // Switching modules never mutates the recipe or sidecar.
        if !ctx.egui_wants_keyboard_input() {
            if let Some(module) = ctx.input(|i| {
                for key in [egui::Key::G, egui::Key::D, egui::Key::E] {
                    if i.key_pressed(key) {
                        if let Some(target) = module_for_key(key) {
                            return Some(target);
                        }
                    }
                }
                None
            }) {
                self.active_module = module;
            }
        }

        // Consume idle tasks only while there is no interactive pointer input.
        // Only mask inference remains here; filmstrip thumbnails are produced by
        // the dedicated background thread pool (handled just below, without a
        // pointer gate, so switching the filmstrip never freezes).
        #[cfg(not(target_arch = "wasm32"))]
        if !ctx.input(|input| input.pointer.any_down()) {
            if let Some((_id, task)) = self.idle_queue.pop_next() {
                match task {
                    IdleTask::MaskInference { mask_id } => {
                        self.status = Str::InferenceWaiting.format_arg(&mask_id);
                    }
                    IdleTask::Thumbnail { .. } => {
                        // Thumbnails are no longer enqueued on the idle queue
                        // (the thread pool owns their generation); kept only for
                        // an exhaustive match.
                    }
                }
            }
        }

        // Zoom shortcuts (Lightroom-like). Ignored while a widget wants keyboard
        // input so they never hijack typing. These set the zoom mode / a custom
        // multiplier; the actual `preview_zoom` is derived per-frame in
        // `sync_zoom()` so the ROI crop matches the on-screen view.
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
                self.zoom_step(1.2);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
                self.zoom_step(1.0 / 1.2);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F)) {
                self.set_zoom_mode(ZoomMode::Fit);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                self.set_zoom_mode(ZoomMode::Fit);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
                self.set_zoom_mode(ZoomMode::OneToOne);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num2)) {
                self.set_zoom_mode(ZoomMode::TwoHundred);
            }
        }

        // PERF-FILMSTRIP: drain completed thumbnails from the background pool and
        // build their textures on the main thread. This runs every frame
        // *regardless of pointer state* — thumbnails stream in while the user
        // scrolls/clicks the filmstrip, so switching directories no longer blocks
        // on a synchronous decode+render on the UI thread.
        #[cfg(not(target_arch = "wasm32"))]
        {
            // GUI-SCROLL-200-1: reset the per-frame diagnostic counters before
            // any thumbnail work of this frame runs.
            self.frame_thumb_enqueued = 0;
            self.frame_thumbs_ready = 0;
            while let Ok(result) = self.thumbnail_rx.try_recv() {
                match result.outcome {
                    ThumbnailOutcome::Ready(frame) => {
                        let tex = self.make_thumbnail_texture(&ctx, &frame, &result.key);
                        self.thumbnails.insert(&result.key, tex);
                        trace!("thumbnail ready: {}", result.name);
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

        // PREVIEW-CACHE-FEATURE: per-frame LUMINA_PERF_LOG counters reset before any
        // neighbor work of this frame (a schedule inside `poll_decode` counts).
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.frame_previews_enqueued = 0;
            self.frame_previews_ready = 0;
        }

        // PERF-GUI-7: drain any completed background RAW/raster decode without
        // blocking the UI (non-blocking `try_recv`). The decoded frame is applied
        // on the main thread here, so a slow decode never freezes interaction.
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_decode();

        // PREVIEW-CACHE-FEATURE: drain neighbor-preview worker results (RAM LRU
        // insert + visible failure states) on the main thread; the prefetch
        // itself runs on dedicated background workers, never the IdleQueue.
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_neighbor_previews(&ctx);

        // Derive `preview_zoom` from the active mode using the geometry cached by
        // the previous frame's `draw_preview`, so the render's ROI crop matches
        // the on-screen zoom (even on the frame a mode button/shortcut fires).
        self.sync_zoom();

        // PERF-GUI-3/4: draft render while a pointer drag is in progress
        // (coalesced: latest params overwrite, intermediate frames are dropped,
        // a repaint is requested); a debounced full-quality render fires on
        // mouse-up / idle (150 ms) so the final frame is computed once.
        // GUI-60FPS-1: slider/mask hot path prefers the VRAM-resident GPU tone
        // stage (`render_to_vram`, no `map_async` CPU readback). The CPU fallback
        // remains fully functional when no adapter is bound or the `gpu` feature
        // is off.
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        let now = ctx.input(|i| i.time);
        if pointer_down
            && self.pending_full_render
            && self.original.is_some()
            && self.render_key.is_none()
        {
            trace!("GUI render: draft render during pointer drag");
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
            {
                if let Some(gpu) = self.gpu.as_ref() {
                    if gpu.is_available() {
                        // R2-GUIMOD-03: borrow instead of clone. The fallback
                        // branch (`draft_original` absent on the first tick
                        // after a full render) used to memcpy the entire
                        // full-resolution original (~180 MB worst case) into
                        // a temporary that was dropped immediately after the
                        // call — `render_to_vram` only needs `&ImageFrame`.
                        let source = self.draft_original.as_ref().or(self.original.as_ref());
                        if let Some(src) = source {
                            match gpu.render_to_vram(src, &self.recipe) {
                                Ok(()) => {
                                    // GUI-WGPU-PRESENT-1: the VRAM output now
                                    // matches the current recipe/source — the
                                    // present path may use it this frame.
                                    self.vram_fresh = true;
                                }
                                Err(err) => {
                                    warn!("gpu render_to_vram failed: {err}");
                                    self.vram_fresh = false;
                                }
                            }
                        }
                    }
                }
            }
            let screen = ctx.input(|i| i.viewport_rect());
            let viewport = [screen.width() as u32, screen.height() as u32];
            if let Err(e) = self.render_draft(viewport, None) {
                self.show_error(e);
            }
            self.last_edit_time = now;
            ctx.request_repaint();
        } else if !pointer_down && self.pending_full_render {
            // 150 ms debounce after the last edit before committing the full
            // render. `last_edit_time == 0` (no drag recorded) routes to an
            // immediate full render so non-drag edits are never stranded.
            //
            // REVIEW-GUI-DEBOUNCE-1: while still inside the wait window
            // (<150 ms) neither a render happens nor did anything schedule a
            // repaint — egui would sleep indefinitely and the draft preview
            // stayed until the next unrelated input. The waiting branch now
            // requests a timed repaint exactly when the debounce elapses.
            match full_render_debounce_remaining(self.last_edit_time, now) {
                Some(remaining_seconds) => {
                    trace!(
                        "GUI render: debounce wait, repaint in {:.1} ms",
                        remaining_seconds * 1000.0
                    );
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        remaining_seconds,
                    ));
                }
                None => {
                    trace!("GUI render: debounced full render after interaction");
                    let screen = ctx.input(|i| i.viewport_rect());
                    let viewport = [screen.width() as u32, screen.height() as u32];
                    if let Err(e) = self.render_full(viewport, None) {
                        self.show_error(e);
                    }
                    self.last_edit_time = 0.0;
                }
            }
        }

        // Dropped files (path or bytes) load a new source.
        // egui 0.36: dropped files are trait objects (`DroppedFileHandle`);
        // on native contents are read synchronously via `bytes() -> Result`,
        // on wasm via `bytes_async() -> Future` (async) and a relative `path()`.
        // See `feature/platform/cli-gui-wasm.md` Capability-Matrix § WASM.
        #[cfg(not(target_arch = "wasm32"))]
        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            match file.bytes() {
                Ok(bytes) => {
                    let name = file
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if let Err(error) = self.load_bytes(bytes, name) {
                        self.show_error(error);
                    }
                }
                Err(read_error) => {
                    // No silent fallback: a dropped file that cannot be read is
                    // surfaced as a visible error.
                    log::warn!("dropped file could not be read: {read_error}");
                    self.show_error(format!("dropped file unreadable: {read_error}"));
                }
            }
            if !file.path().as_os_str().is_empty() {
                // REVIEW-GUI-PATHDESYNC-1: no immediate `self.path` commit;
                // `finish_decode` adopts the path after a successful decode.
                self.begin_load_path(file.path().display().to_string());
            }
        }
        #[cfg(target_arch = "wasm32")]
        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            // Capability decision (MVP): WASM drag-and-drop is intentionally not
            // supported. egui 0.36 exposes `DroppedFile::bytes_async()` (async)
            // on wasm32 with only a relative `path()` (no absolute fs path); the
            // synchronous `bytes()` used on native does not exist on wasm. Bridging
            // the async read via `wasm_bindgen_futures::spawn_local` and wiring the
            // result back into the synchronous `update()` loop is not yet
            // implemented. The drop is therefore surfaced visibly instead of
            // silently ignored (Agents.md: no silent fallbacks). File loading on
            // WASM remains via the file picker; native drag-and-drop stays fully
            // functional. See `feature/platform/cli-gui-wasm.md` § WASM.
            let name = file.path().display().to_string();
            let display = if name.is_empty() { "file" } else { &name };
            log::warn!("WASM drag-and-drop not supported yet (requires bytes_async): {display}");
            self.status = format!("Drop not supported on WASM yet: {display}");
            self.error = Some(
                "Drag-and-drop on WASM requires async file reading (DroppedFile::bytes_async) — not yet implemented; use the file picker"
                    .into(),
            );
        }

        // Top: brand + status/error.
        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lumina");
                ui.separator();
                ui.label(&self.status);
            });
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });

        // Top: module bar (Library / Develop / Export) + the Before/After toggle,
        // then the histogram of the currently displayed render state. The module
        // labels advertise their Lightroom keyboard shortcuts (`G`, `D`).
        egui::Panel::top("modules").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (module, label) in [
                    (Module::Library, Str::LibraryShortcut.format_arg("G")),
                    (Module::Develop, Str::DevelopShortcut.format_arg("D")),
                    (Module::Export, Str::Export.t().to_string()),
                ] {
                    if ui
                        .selectable_label(self.active_module == module, label)
                        .clicked()
                    {
                        self.active_module = module;
                    }
                }
                ui.separator();
                if ui.button(Str::BeforeAfter.t()).clicked() {
                    self.toggle_before_after();
                }
            });
            self.draw_histogram(ui);
        });

        // Left: Lightroom-like Library folder tree. Develop/Export leave the
        // left edge to the navigator/preview working area.
        #[cfg(not(target_arch = "wasm32"))]
        if self.active_module == Module::Library {
            egui::Panel::left("folders")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| self.draw_folder_tree(ui));
        }

        // Left: Lightroom-like thumbnail navigator rail (Develop / Export). The
        // Library module keeps its text file-browser on the left instead, so the
        // two never collide on the same side. It reuses the filmstrip
        // ThumbnailManager (no duplicate generation) and highlights the active
        // image.
        #[cfg(not(target_arch = "wasm32"))]
        if self.navigator_open && !matches!(self.active_module, Module::Library) {
            egui::Panel::left("navigator")
                .resizable(true)
                .default_size(150.0)
                .show(ui, |ui| self.draw_navigator(&ctx, ui));
        }

        // Right: Develop controls (eight sections), or nothing extra for
        // Export (placeholder shown centrally). The Library module is a
        // two-pane layout (folder tree left, thumbnail grid center) with no
        // right-hand Source panel — that source/sidecar/copy info belongs to
        // the Develop/Export context (Lightroom-parity: the Source panel was
        // removed from Library).
        egui::Panel::right("controls")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| match self.active_module {
                Module::Develop => self.draw_develop_panel(ui),
                Module::Library => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        // No right Source panel in Library — intentional.
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        ui.label(Str::NotAvailable.t());
                    }
                }
                Module::Export => {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.draw_export_panel(ui);
                    #[cfg(target_arch = "wasm32")]
                    ui.label(Str::NotAvailable.t());
                }
            });

        // Bottom: filmstrip in Library/Develop. Native builds show generated
        // thumbnails (miss -> background job); the wasm build shows placeholders
        // only, since in-browser RAW/file IO is a documented native capability.
        let show_filmstrip = matches!(self.active_module, Module::Library | Module::Develop);
        if show_filmstrip {
            #[cfg(not(target_arch = "wasm32"))]
            egui::Panel::bottom("filmstrip").show(ui, |ui| self.draw_filmstrip(&ctx, ui));
            #[cfg(target_arch = "wasm32")]
            egui::Panel::bottom("filmstrip").show(ui, |ui| {
                ui.heading(Str::Filmstrip.t());
                ui.label(Str::NotAvailable.t());
            });
        }

        // Central: the large preview/navigator. The Export module shows the
        // current render (what will be exported); the controls live in the
        // right-side Export panel. Under wasm32 (no file-system export) the
        // module is a clear capability hint.
        egui::CentralPanel::default().show(ui, |ui| match self.active_module {
            Module::Export => {
                #[cfg(target_arch = "wasm32")]
                {
                    ui.centered_and_justified(|ui| ui.label(Str::NotAvailable.t()));
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.draw_preview_area(&ctx, ui);
                }
            }
            // Library: Lightroom-like grid view (folders tree left, RAW
            // thumbnail grid center); Develop/Export keep the large preview.
            // Under wasm32 the grid import path is not available; fall back to
            // the plain preview like the other modules.
            Module::Library => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.draw_library_grid(&ctx, ui);
                }
                #[cfg(target_arch = "wasm32")]
                {
                    self.draw_preview(ui);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            _ => self.draw_preview_area(&ctx, ui),
            #[cfg(target_arch = "wasm32")]
            _ => self.draw_preview(ui),
        });
        if let Some(t0) = perf_t0 {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            // GUI-SCROLL-200-1: `slow_frame` flags every frame over the 16.7 ms
            // (60 Hz) budget so scrolling spikes are greppable while thumbnail
            // jobs run; `thumb_jobs_enqueued`/`thumbs_ready` correlate a spike
            // with same-frame thumbnail work.
            let slow_frame = ms > 16.7;
            #[cfg(not(target_arch = "wasm32"))]
            let counters = (
                self.frame_thumb_enqueued,
                self.frame_thumbs_ready,
                self.frame_previews_enqueued,
                self.frame_previews_ready,
            );
            #[cfg(target_arch = "wasm32")]
            let counters = (0usize, 0usize, 0usize, 0usize);
            log::info!(
                "LUMINA_PERF frame={:.2}ms pointer_down={} thumb_jobs_enqueued={} thumbs_ready={} neighbor_previews_enqueued={} neighbor_previews_ready={} slow_frame={}",
                ms,
                ctx.input(|i| i.pointer.any_down()),
                counters.0,
                counters.1,
                counters.2,
                counters.3,
                slow_frame
            );
            eprintln!(
                "LUMINA_PERF frame={:.2}ms thumb_jobs_enqueued={} thumbs_ready={} neighbor_previews_enqueued={} neighbor_previews_ready={} slow_frame={}",
                ms, counters.0, counters.1, counters.2, counters.3, slow_frame
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let canvas = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("lumina_canvas"))
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("Lumina canvas was not found"))?;
    wasm_bindgen_futures::spawn_local(async {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(LuminaApp::new(cc.egui_ctx.clone())))),
            )
            .await;
        if let Err(error) = result {
            web_sys::console::error_1(&error);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::ImageFileFormat;
    use lumina_sidecar::{
        BrushMark, BrushMarkSign, CoordinateSystem, DecodeFingerprint, GeometryFingerprint,
        MaskDefinition, MaskOperation, MaskPrompt, MaskStatus, ModelIdentity, Point2,
        Preprocessing, PromptTransform, Resolution, SourceFingerprint, SourceStatus,
    };
    fn new_app() -> LuminaApp {
        LuminaApp::new(egui::Context::default())
    }

    /// Open a file and synchronously drain the background decode (PERF-GUI-7)
    /// channel. The headless test harness has no `update()` event loop, so the
    /// async `decode_rx` must be pumped here before asserting on the result.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_and_decode(app: &mut LuminaApp, path: impl Into<String>) {
        app.open_file(path);
        // Pump the background decode channel; yield so the worker thread is
        // scheduled. Bounded so a genuine failure can't hang the suite.
        for _ in 0..2000 {
            app.poll_decode();
            if app.original.is_some() || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn png() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }
    /// F-100 / F-103-N10 (user decision 2026-08-25): the Develop sections are
    /// drawn in Lightroom Classic panel order — **Detail BEFORE Effects**.
    /// `draw_develop_panel` renders exactly this table, so this pins the real
    /// render order without a GPU harness.
    #[test]
    fn develop_section_order_is_lightroom_conform() {
        let order: Vec<Str> = LuminaApp::DEVELOP_SECTIONS
            .iter()
            .map(|(label, _)| *label)
            .collect();
        assert_eq!(
            order,
            vec![
                Str::Basic,
                Str::ToneCurve,
                Str::Color,
                Str::Detail,
                Str::Effects,
                Str::Optics,
                Str::Geometry,
                Str::Masking,
            ]
        );
        let detail = order.iter().position(|s| *s == Str::Detail).unwrap();
        let effects = order.iter().position(|s| *s == Str::Effects).unwrap();
        assert!(detail < effects, "Detail must precede Effects");
    }
    #[test]
    fn recipe_change_and_render() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 1.0);
        app.render().unwrap();
        assert_eq!(app.recipe().adjustments["exposure"], 1.0);
        assert_eq!(app.preview().unwrap().pixels[0], 20);
    }
    #[test]
    fn auto_and_matching_use_core_and_persist_recipe_state() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.auto_tone().unwrap();
        assert!(app.recipe().auto_features.enable_auto_tone);
        app.match_total_exposure(0.5).unwrap();
        assert!(app.recipe().auto_features.match_total_exposure);
        assert!(app.recipe().auto_features.matched_exposure.is_some());
    }
    #[test]
    fn reset_restores_original_preview() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("contrast", 1.0);
        app.render().unwrap();
        app.reset();
        assert!(app.recipe().adjustments.is_empty());
        assert_eq!(app.preview().unwrap().pixels[0], 10);
    }

    #[test]
    fn module_for_key_maps_lightroom_module_shortcuts() {
        // `G` -> Library, `D` -> Develop, `E` -> Library (Loupe alias).
        assert_eq!(module_for_key(egui::Key::G), Some(Module::Library));
        assert_eq!(module_for_key(egui::Key::D), Some(Module::Develop));
        assert_eq!(module_for_key(egui::Key::E), Some(Module::Library));
        // Existing non-module shortcuts must not collide with the mapping.
        assert_eq!(module_for_key(egui::Key::Y), None);
        assert_eq!(module_for_key(egui::Key::Escape), None);
        // Arbitrary other keys resolve to no module change.
        assert_eq!(module_for_key(egui::Key::A), None);
    }

    #[test]
    fn module_shortcut_switch_changes_module_without_mutating_recipe() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.active_module = Module::Library;
        app.set_adjustment("exposure", 0.75);
        let adjustments_before = app.recipe().adjustments.clone();

        // Simulate the `D` shortcut resolving to its target module.
        app.active_module = module_for_key(egui::Key::D).unwrap();

        // Module changed...
        assert_eq!(app.active_module, Module::Develop);
        // ...but the recipe (and therefore any sidecar state) is untouched.
        assert_eq!(app.recipe().adjustments, adjustments_before);
        assert_eq!(app.recipe().adjustments["exposure"], 0.75);
    }

    #[test]
    fn render_key_is_invalidated_until_render() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        assert!(app.render_key().is_some());
        app.set_adjustment("exposure", 1.0);
        assert!(app.render_key().is_none());
        app.render().unwrap();
        assert!(app.render_key().is_some());
        assert_eq!(app.tone_analysis().unwrap().sample_count, 2);
    }

    // ---- PERF-GUI-1: staged base cache (hit/miss, stepwise invalidation,
    // pixel identity) ----

    /// Deterministic RGBA gradient PNG so the render stages exercise real
    /// per-pixel math over more than a handful of samples.
    fn gradient_png(width: u32, height: u32) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width) as u8;
                let g = ((y * 255) / height) as u8;
                let b = ((x + y) % 256) as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        ImageFrame::new(width, height, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    #[test]
    fn exposure_change_hits_base_cache_and_stays_pixel_identical_to_cold_render() {
        let mut app = new_app();
        app.load_bytes(gradient_png(16, 12), "grad.png").unwrap();

        // `load_bytes` renders once internally, so the base stage is warm:
        // every recipe-only change below must be a pure downstream re-render.
        assert_eq!(app.base_stage_cache_len(), 1);
        app.set_adjustment("exposure", 1.0);
        app.render().unwrap();
        let first = app.last_stage_work().expect("stage work recorded");
        assert!(
            first.base_cache_hit,
            "a pure exposure change must reuse the base built at load time"
        );
        assert_eq!(first.adjustments_passes, 1);

        // A further drag tick keeps hitting without growing the cache.
        app.set_adjustment("exposure", -0.5);
        app.render().unwrap();
        let warm = app.last_stage_work().expect("stage work recorded");
        assert!(warm.base_cache_hit);
        assert_eq!(warm.adjustments_passes, 1);
        assert_eq!(
            app.base_stage_cache_len(),
            1,
            "recipe-only changes must not create new base entries"
        );
        let warm_pixels = app.preview().unwrap().pixels.clone();

        // Pixel identity proof: forcing the cold path (cache cleared) for the
        // same recipe reproduces the warm output byte-for-byte.
        app.clear_preview_stage_cache();
        app.render().unwrap();
        let forced_cold = app.last_stage_work().unwrap();
        assert!(!forced_cold.base_cache_hit);
        assert_eq!(
            warm_pixels,
            app.preview().unwrap().pixels,
            "the base-stage shortcut must not change a single pixel"
        );
    }

    #[test]
    fn color_change_reuses_base_while_geometry_change_keeps_it_too() {
        let mut app = new_app();
        app.load_bytes(gradient_png(16, 12), "grad.png").unwrap();
        app.set_adjustment("wb_temperature", 7000.0);
        app.set_adjustment("saturation", 0.3);
        app.render().unwrap();
        assert!(app.last_stage_work().unwrap().base_cache_hit);

        // A further WB/color tick is an adjustment like exposure: base stays.
        app.set_adjustment("wb_tint", -0.2);
        app.render().unwrap();
        let color_tick = app.last_stage_work().unwrap();
        assert!(color_tick.base_cache_hit);
        assert_eq!(color_tick.adjustments_passes, 1);

        // Geometry runs downstream of the base in the documented order, so it
        // also reuses the cached base while invalidating the final render.
        app.recipe.geometry = Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 90.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        app.mark_dirty();
        app.render().unwrap();
        let geometry_tick = app.last_stage_work().unwrap();
        assert!(geometry_tick.base_cache_hit);
        assert_eq!(app.base_stage_cache_len(), 1);
    }

    #[test]
    fn roi_changes_get_separate_base_entries() {
        let mut app = new_app();
        app.load_bytes(gradient_png(16, 12), "grad.png").unwrap();
        // The load-time render already populated the full-frame entry.
        assert_eq!(app.base_stage_cache_len(), 1);

        // A zoom ROI is part of the base identity → its own entry.
        app.render_full([800, 600], Some([2, 2, 8, 6])).unwrap();
        assert_eq!(app.base_stage_cache_len(), 2);
        assert!(!app.last_stage_work().unwrap().base_cache_hit);

        // The same window hits its entry without growing the cache.
        app.render_full([800, 600], Some([2, 2, 8, 6])).unwrap();
        assert_eq!(app.base_stage_cache_len(), 2);
        assert!(app.last_stage_work().unwrap().base_cache_hit);

        // A different offset with equal size is a different base window.
        app.render_full([800, 600], Some([3, 2, 8, 6])).unwrap();
        assert_eq!(app.base_stage_cache_len(), 3);
    }

    #[test]
    fn draft_drag_ticks_share_the_base_stage_with_full_renders() {
        let mut app = new_app();
        app.load_bytes(gradient_png(64, 48), "grad.png").unwrap();
        assert_eq!(app.base_stage_cache_len(), 1);

        // Slider drag: draft renders use the same (sub-draft-cap) source, so
        // the prepared base is identical to the full one and must be shared —
        // the first tick already hits the entry built by the load render.
        app.set_adjustment("exposure", 0.25);
        app.render_draft([800, 600], None).unwrap();
        let first_tick = app.last_stage_work().unwrap();
        assert!(first_tick.base_cache_hit);
        assert_eq!(app.base_stage_cache_len(), 1);

        app.set_adjustment("exposure", 0.75);
        app.render_draft([800, 600], None).unwrap();
        assert!(app.last_stage_work().unwrap().base_cache_hit);

        // Committing the drag (full-quality render) keeps hitting the same
        // base instead of rebuilding it.
        app.render().unwrap();
        assert!(app.last_stage_work().unwrap().base_cache_hit);
        assert_eq!(app.base_stage_cache_len(), 1);
    }

    #[test]
    fn loading_a_new_source_clears_the_base_stage_cache() {
        let mut app = new_app();
        app.load_bytes(gradient_png(16, 12), "a.png").unwrap();
        app.render_full([800, 600], Some([2, 2, 8, 6])).unwrap();
        assert_eq!(app.base_stage_cache_len(), 2);

        // A new source identity invalidates every cached stage at once.
        app.load_bytes(gradient_png(12, 16), "b.png").unwrap();
        assert_eq!(
            app.base_stage_cache_len(),
            1,
            "the old source entries are gone; only b's own base remains"
        );
    }

    /// REVIEW-CORE-DIGEST-WIRING: pins the contract of the single RenderKey
    /// construction site (the preview render). Identical inputs must produce
    /// an identical digest (cache-hit contract), and the digest must separate
    /// the deliberately plain in-memory preview from every attached export-
    /// option set or source-action artifact hash list.
    #[test]
    fn render_key_digest_separates_export_options_and_source_action_hashes() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.render().unwrap();
        let key = app.render_key().expect("render key after render").clone();

        // Same inputs -> same digest: an unchanged state keeps cache identity.
        app.render().unwrap();
        assert_eq!(
            key.digest(),
            app.render_key().expect("re-rendered").digest()
        );

        // The preview is a plain in-memory frame render: no encoder options,
        // no applied source-action artifacts.
        assert!(key.export_options.is_none());
        assert!(key.source_action_artifact_hashes.is_empty());

        // Varying export options change the digest ...
        let quality_90 = key.clone().with_export_options(ExportOptions {
            quality: 90,
            ..Default::default()
        });
        let quality_60 = key.clone().with_export_options(ExportOptions {
            quality: 60,
            ..Default::default()
        });
        assert_ne!(key.digest(), quality_90.digest());
        assert_ne!(quality_90.digest(), quality_60.digest());
        // ... and equal options reproduce it exactly.
        let quality_90_again = key.clone().with_export_options(ExportOptions {
            quality: 90,
            ..Default::default()
        });
        assert_eq!(quality_90.digest(), quality_90_again.digest());

        // Varying source-action artifact hashes change the digest ...
        let repaired = key
            .clone()
            .with_source_action_hashes(["blake3:repair-artifact".to_owned()]);
        assert_ne!(key.digest(), repaired.digest());
        assert_ne!(
            repaired.digest(),
            key.clone()
                .with_source_action_hashes(["blake3:other-artifact".to_owned()])
                .digest()
        );
        // ... and equal hashes reproduce it exactly.
        assert_eq!(
            repaired.digest(),
            key.with_source_action_hashes(["blake3:repair-artifact".to_owned()])
                .digest()
        );
    }

    #[test]
    fn preset_requires_name_and_validates_relative_exposure() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        assert!(app.create_preset("").is_err());
        app.preset_relative_exposure = true;
        assert!(app.create_preset("relative").is_err());
        app.recipe.auto_features.enable_auto_tone = true;
        let preset = app.create_preset("relative").unwrap();
        assert_eq!(preset.recipe.options["exposure_semantics"], "relative");
    }
    #[test]
    fn decode_error_is_visible() {
        let mut app = new_app();
        let result = app.load_bytes(vec![1, 2, 3], "bad.png");
        assert!(result.is_err());
        app.show_error(result.unwrap_err());
        assert_eq!(app.status(), Str::Error.t());
        assert!(app.error().is_some());
    }

    #[test]
    fn stale_auto_tone_clears_active_adjustments_but_keeps_status_state() {
        let mut recipe = EditRecipe::default();
        recipe.auto_features.enable_auto_tone = true;
        recipe.auto_features.auto_exposure = Some(1.25);
        recipe.auto_features.auto_contrast = Some(-0.2);
        recipe.adjustments.insert("exposure".into(), 1.25);
        recipe.adjustments.insert("contrast".into(), -0.2);
        recipe.adjustments.insert("highlights".into(), -0.5);

        clear_stale_auto_tone(&mut recipe);

        assert!(recipe.auto_features.enable_auto_tone);
        assert!(recipe.auto_features.auto_exposure.is_none());
        assert!(recipe.auto_features.auto_contrast.is_none());
        assert!(!recipe.adjustments.contains_key("exposure"));
        assert!(!recipe.adjustments.contains_key("contrast"));
        assert_eq!(recipe.adjustments["highlights"], -0.5);
    }

    #[test]
    fn stale_auto_tone_validation_checks_input_fingerprint() {
        let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        let input = tone_fingerprint(&frame, AutoToneConfig::default());
        let valid = AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint: input.clone(),
            extras: BTreeMap::new(),
        };
        assert!(is_current_tone_analysis(&valid, &input));
        for stored_input in [input.as_str(), "wrong"] {
            let stored = AnalysisFingerprint {
                algorithm: "arbitrary-pre-mvp-label".into(),
                version: "arbitrary-pre-mvp-value".into(),
                input_fingerprint: stored_input.into(),
                extras: BTreeMap::new(),
            };
            assert_eq!(
                is_current_tone_analysis(&stored, &input),
                stored_input == input
            );
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn sidecar_decoder_identity_distinguishes_raw_from_raster() {
        assert_eq!(decoder_identity(true), "libraw");
        assert_eq!(decoder_identity(false), "image");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn to_normalized_is_finite_for_zero_size_rect() {
        // Regression guard for the division-by-zero / NaN protection in
        // `to_normalized`: a momentarily empty preview rect (zero width/height)
        // must not yield non-finite normalized coordinates, which would
        // otherwise propagate into the recipe through the WB eyedropper / mask
        // tool mapping.
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(0.0, 0.0));
        let (nx, ny) = LuminaApp::to_normalized(egui::pos2(10.0, 20.0), rect, None, (100, 100));
        assert!(nx.is_finite(), "nx must be finite, got {nx}");
        assert!(ny.is_finite(), "ny must be finite, got {ny}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_png(path: &Path) {
        let png = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_writes_sidecar_and_restores_recipe_on_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("exposure", 1.5);
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        assert!(sidecar.is_file(), "Sidecar must be written");
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["exposure"],
            1.5
        );

        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_persists_virtual_copies_across_save_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("contrast", 0.3);
        app.save_sidecar();
        app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(document.virtual_copies.len(), 2);
        assert!(document
            .virtual_copies
            .iter()
            .any(|copy| copy.id == "vc-2" && copy.name == "Copy 2"));
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["contrast"],
            0.3
        );

        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        assert_eq!(reopened.entries().len(), 1);
        let reloaded = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(reloaded.virtual_copies.len(), 2);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_browser_index_reports_sidecar_and_copy_count() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.save_sidecar();
        app.duplicate_virtual_copy("vc-2", "Two").unwrap();
        app.duplicate_virtual_copy("vc-3", "Three").unwrap();
        app.save_sidecar();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert!(entry.has_sidecar);
        assert_eq!(entry.virtual_copies, 3);
        assert!(!entry.conflict);
        assert!(!entry.is_offline());
        assert_eq!(entry.status_label(), Str::Sidecar.t());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_browser_detects_offline_source_and_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.save_sidecar();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert!(!entry.is_offline());
        assert!(!entry.conflict);

        std::fs::remove_file(&source).unwrap();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert!(entry.is_offline());
        assert!(entry.conflict);
        assert_eq!(entry.source_status, SourceStatus::Missing);
        assert_eq!(entry.status_label(), "Conflict");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_browser_reports_missing_mask_models() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.save_sidecar();
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let mut document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        document.virtual_copies[0]
            .mask_library
            .push(MaskDefinition {
                id: "m1".into(),
                name: "subject".into(),
                source_fingerprint: SourceFingerprint {
                    content_hash: "blake3:x".into(),
                    byte_length: 1,
                    extras: BTreeMap::new(),
                },
                decode_context: DecodeFingerprint {
                    decoder: "libraw".into(),
                    version: "1".into(),
                    parameters: BTreeMap::new(),
                    extras: BTreeMap::new(),
                },
                geometry_context: GeometryFingerprint {
                    width: 2,
                    height: 1,
                    orientation: 1,
                    pixel_aspect_ratio: 1.0,
                    extras: BTreeMap::new(),
                },
                model: ModelIdentity {
                    name: "birefnet".into(),
                    version: "1".into(),
                    hash: "h".into(),
                    extras: BTreeMap::new(),
                },
                inference_resolution: Resolution {
                    width: 2,
                    height: 1,
                    extras: BTreeMap::new(),
                },
                preprocessing: Preprocessing {
                    name: "std".into(),
                    version: "1".into(),
                    parameters: BTreeMap::new(),
                    extras: BTreeMap::new(),
                },
                rescaling_method: "bilinear".into(),
                rescaling_parameters: BTreeMap::new(),
                coordinate_system: CoordinateSystem::SourceOriented,
                status: MaskStatus::Missing,
                created_at: "2026-01-01T00:00:00Z".into(),
                generator_version: "g".into(),
                error_text: None,
                artifact: None,
                operation: MaskOperation::Source,
                references: vec![],
                prompt: None,
                extras: BTreeMap::new(),
            });
        lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
        app.set_directory(directory.path().display().to_string());
        let entry = app
            .entries()
            .iter()
            .find(|e| e.name == "photo.png")
            .unwrap();
        assert_eq!(entry.missing_models, 1);
        assert!(entry.has_sidecar);

        let reloaded = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        assert_eq!(reloaded.virtual_copies[0].mask_library.len(), 1);
        assert_eq!(
            reloaded.virtual_copies[0].mask_library[0].status,
            MaskStatus::Missing
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mask_selection_and_name_roundtrip_through_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        let id = app.create_mask("Subject").unwrap();
        app.rename_mask(&id, "Main subject").unwrap();
        app.save_sidecar();

        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
        assert_eq!(
            document.virtual_copies[0].mask_library[0].name,
            "Main subject"
        );
        assert_eq!(document.virtual_copies[0].mask_layers[0].mask.mask_id, id);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mask_layer_parameters_are_non_destructive_and_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.create_mask("Subject").unwrap();
        app.set_mask_inverted(true).unwrap();
        app.set_mask_feather(0.25).unwrap();
        app.save_sidecar();

        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
        let layer = &document.virtual_copies[0].mask_layers[0];
        assert!(layer.inverted);
        assert_eq!(layer.feather, 0.25);
        assert_eq!(
            document.virtual_copies[0].mask_library[0].status,
            MaskStatus::Pending
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn stale_mask_offers_recalculation_without_running_inference() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.create_mask("Subject").unwrap();
        assert!(app.offer_mask_recalculation().unwrap());
        assert!(app.status().contains("recalculation"));
        app.mark_mask_for_recalculation().unwrap();
        assert!(app.status().contains("requested"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn local_mask_adjustments_roundtrip_through_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.create_mask("Subject").unwrap();
        app.set_mask_local_adjustment("exposure", 1.25).unwrap();
        app.set_mask_local_adjustment("contrast", -0.35).unwrap();
        app.save_sidecar();

        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        let layer = &reopened.document.as_ref().unwrap().virtual_copies[0].mask_layers[0];
        assert_eq!(layer.extras["adjustment_exposure"].as_f64(), Some(1.25));
        assert_eq!(layer.extras["adjustment_contrast"].as_f64(), Some(-0.35));
    }

    #[test]
    fn idle_queue_is_bounded_prioritized_and_cancellable() {
        let mut queue = IdleQueue::new(2);
        let low = queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "low".into(),
                },
                1,
            )
            .unwrap();
        queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "high".into(),
                },
                9,
            )
            .unwrap();
        assert!(queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "full".into()
                },
                9
            )
            .is_none());
        assert!(queue.cancel(low));
        assert_eq!(
            queue.pop_next().unwrap().1,
            IdleTask::MaskInference {
                mask_id: "high".into()
            }
        );
        assert!(queue.is_empty());
    }

    // ---- F-103-N2: single-control reset semantics + display scaling ----

    #[test]
    fn slider_reset_only_this_control_keeps_other_adjustments() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 2.0);
        app.set_adjustment("contrast", 0.5);
        app.set_adjustment("wb_temperature", 7000.0);
        // Resetting one control must not touch the others or the whole recipe.
        app.reset_single_adjustment("exposure");
        assert_eq!(app.recipe().adjustments["exposure"], 0.0);
        assert_eq!(app.recipe().adjustments["contrast"], 0.5);
        assert_eq!(app.recipe().adjustments["wb_temperature"], 7000.0);
        assert!(!app.recipe().adjustments.is_empty());
    }

    #[test]
    fn display_scale_percent_maps_internal_domain() {
        // `-1..=1` is shown as `-100..+100`; Exposure/Kelvin stay identity.
        let percent = crate::slider::percent_spec(-1.0..=1.0, 0.0);
        assert_eq!(percent.scale, crate::slider::DisplayScale::Percent);
        assert_eq!(
            crate::slider::to_display(-1.0, crate::slider::DisplayScale::Percent),
            -100.0
        );
        assert_eq!(
            crate::slider::to_display(0.5, crate::slider::DisplayScale::Percent),
            50.0
        );
        assert_eq!(
            crate::slider::from_display(-100.0, crate::slider::DisplayScale::Percent),
            -1.0
        );
        let identity = crate::slider::identity_spec(-10.0..=10.0, 0.0, 0.1);
        assert_eq!(identity.scale, crate::slider::DisplayScale::Identity);
        assert_eq!(
            crate::slider::to_display(2.5, crate::slider::DisplayScale::Identity),
            2.5
        );
    }

    // ---- F-103-N7: Presence + Vibrance/Saturation controls ----

    #[test]
    fn vibrance_and_saturation_write_correct_adjustment_keys_and_domain() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        // F-092 Dynamics/Saturation: flat adjustments on the `-1..=1` domain.
        app.set_adjustment("vibrance", 0.5);
        app.set_adjustment("saturation", -0.25);
        assert_eq!(app.recipe().adjustments["vibrance"], 0.5);
        assert_eq!(app.recipe().adjustments["saturation"], -0.25);
        // Stored in the normative domain (pipeline/sidecar validate `-1..=1`).
        assert!(((-1.0)..=1.0).contains(&app.recipe().adjustments["vibrance"]));
        assert!(((-1.0)..=1.0).contains(&app.recipe().adjustments["saturation"]));
        // The flat `saturation` adjustment is distinct from the HSL mixer's
        // per-channel saturation storage.
        assert!(app.recipe().hsl.is_none());
    }

    #[test]
    fn presence_set_writes_recipe_fields_with_neutral_default_and_domain() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        // F-094 Presence: `texture` / `clarity` / `dehaze` on the `-1..=1`
        // domain; the GUI initializes to the neutral 0.0 each.
        app.set_presence("texture", 0.0);
        app.set_presence("clarity", 0.0);
        app.set_presence("dehaze", 0.0);
        let p = app.recipe().presence.as_ref().unwrap();
        assert_eq!((p.texture, p.clarity, p.dehaze), (0.0, 0.0, 0.0));

        // A non-zero setting lands in the correct recipe field, in-domain.
        app.set_presence("texture", 0.8);
        app.set_presence("clarity", -0.5);
        app.set_presence("dehaze", 0.3);
        let p = app.recipe().presence.as_ref().unwrap();
        assert_eq!(p.texture, 0.8);
        assert_eq!(p.clarity, -0.5);
        assert_eq!(p.dehaze, 0.3);
        for v in [p.texture, p.clarity, p.dehaze] {
            assert!((-1.0..=1.0).contains(&(v as f64)));
        }

        // Unknown field names are ignored, leaving the struct untouched.
        let before = *app.recipe().presence.as_ref().unwrap();
        app.set_presence("echo", 0.9);
        assert_eq!(*app.recipe().presence.as_ref().unwrap(), before);
    }

    #[test]
    fn presence_display_scaling_is_percent_for_internal_domain() {
        // F-094 Presence shares the `-1..=1` -> `-100..+100` Lightroom scale.
        let spec = crate::slider::percent_spec(-1.0..=1.0, 0.0);
        assert_eq!(spec.scale, crate::slider::DisplayScale::Percent);
        assert_eq!(
            crate::slider::to_display(0.8, crate::slider::DisplayScale::Percent),
            80.0
        );
        assert_eq!(
            crate::slider::from_display(-50.0, crate::slider::DisplayScale::Percent),
            -0.5
        );
    }

    #[test]
    fn single_control_reset_keeps_other_dynamics_and_presence() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        // Seed Dynamics and a Presence field, then reset only one of each.
        app.set_adjustment("vibrance", 0.7);
        app.set_adjustment("saturation", 0.4);
        app.set_presence("clarity", 0.6);
        // Resetting Vibrance must not touch Saturation or Presence.
        app.reset_single_adjustment("vibrance");
        assert_eq!(app.recipe().adjustments["vibrance"], 0.0);
        assert_eq!(app.recipe().adjustments["saturation"], 0.4);
        // Default for these flat keys is the documented neutral 0.0.
        assert_eq!(LuminaApp::default_for_adjustment("vibrance"), 0.0);
        assert_eq!(LuminaApp::default_for_adjustment("saturation"), 0.0);
        // Presence neutral default is 0.0 per the GUI initializer.
        assert_eq!(app.recipe().presence.as_ref().unwrap().clarity, 0.6);
    }

    // ---- REVIEW-GUI-THUMB-1 / THUMB-2 / PATHDESYNC-1 (headless) ----

    /// REVIEW-GUI-THUMB-1: identical filenames in two folders must produce
    /// distinct thumbnail keys so neither cell can show the other's image.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn thumbnail_keys_distinguish_same_filename_across_folders() {
        let root = tempfile::tempdir().unwrap();
        let dir_a = root.path().join("album-a");
        let dir_b = root.path().join("album-b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        let path_a = dir_a.join("IMG_0001.png");
        let path_b = dir_b.join("IMG_0001.png");
        save_png(&path_a);
        save_png(&path_b);

        let entry_a = LuminaApp::scan_entry(&path_a).unwrap();
        let entry_b = LuminaApp::scan_entry(&path_b).unwrap();
        assert_eq!(entry_a.name, entry_b.name, "fixture must share a filename");
        assert_ne!(
            entry_a.thumb_key, entry_b.thumb_key,
            "same filename in two folders must not share a thumbnail key"
        );
        // Keys are stable across scans of the same file.
        assert_eq!(
            entry_a.thumb_key,
            LuminaApp::scan_entry(&path_a).unwrap().thumb_key
        );

        // Manager-level: inserting under key A never satisfies lookups for B.
        let ctx = egui::Context::default();
        let mut manager = crate::filmstrip::ThumbnailManager::new();
        let tex = ctx.load_texture(
            "test",
            egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]),
            egui::TextureOptions::LINEAR,
        );
        manager.insert(&entry_a.thumb_key, tex);
        assert!(manager.get(&entry_a.thumb_key).is_some());
        assert!(manager.get(&entry_b.thumb_key).is_none());
    }

    /// REVIEW-GUI-PATHDESYNC-1: `open_file` must not adopt the new path while
    /// the asynchronous decode is running; `finish_decode` commits it on
    /// success only.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn open_file_commits_path_only_after_successful_decode() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);

        let mut app = new_app();
        assert!(app.path.is_empty());
        app.open_file(source.display().to_string());
        // Decode is in flight: the path is NOT yet committed, so any Save /
        // Export would operate on the previous (still consistent) state.
        assert!(app.decode_rx.is_some(), "decode must run asynchronously");
        assert_eq!(
            app.path, "",
            "path must not be adopted before decode success"
        );

        open_and_decode(&mut app, source.display().to_string());
        assert_eq!(app.path, source.display().to_string());
        assert!(app.error().is_none());
    }

    /// REVIEW-GUI-PATHDESYNC-1: a failed decode keeps the previously loaded
    /// image/path pair intact and reports the failure visibly — no phantom
    /// sidecar target, no silent fallback.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn failed_decode_keeps_previous_path_and_reports_error() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("good.png");
        save_png(&source);

        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        let loaded_path = app.path.clone();
        assert_eq!(loaded_path, source.display().to_string());

        let missing = directory.path().join("missing.png");
        app.open_file(missing.display().to_string());
        let mut decoded_or_failed = false;
        for _ in 0..2000 {
            app.poll_decode();
            if app.error().is_some() || app.decode_rx.is_none() {
                decoded_or_failed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(decoded_or_failed, "failed decode must surface promptly");
        assert!(
            app.error().is_some(),
            "decode failure must be reported visibly"
        );
        assert_eq!(
            app.path, loaded_path,
            "a failed decode must not adopt the new path"
        );
    }

    // ---- F-103-N3: Before/After + white-balance eyedropper ----

    #[test]
    fn before_after_toggle_does_not_mutate_recipe() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 1.5);
        let snapshot = app.recipe().adjustments.clone();
        app.toggle_before_after();
        assert!(app.before_after);
        // The toggle only swaps the displayed frame; the recipe is untouched.
        assert_eq!(app.recipe().adjustments, snapshot);
        assert_eq!(app.recipe().adjustments["exposure"], 1.5);
        app.toggle_before_after();
        assert!(!app.before_after);
        assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    }

    #[test]
    fn white_balance_eyedropper_sets_recipe_fields() {
        let mut app = new_app();
        app.load_bytes(png(), "test.png").unwrap();
        // A colored picked point sets both WB fields and disarms the picker.
        app.set_white_balance_from_point(1.0, 0.5, 0.25).unwrap();
        assert!(app.recipe().adjustments.contains_key("wb_temperature"));
        assert!(app.recipe().adjustments.contains_key("wb_tint"));
        let temp = app.recipe().adjustments["wb_temperature"];
        assert!((1500.0..=12000.0).contains(&temp));
        assert!(!app.wb_pick_mode);
        // A neutral grey point is the documented default (6500 K, tint 0).
        app.set_white_balance_from_point(0.5, 0.5, 0.5).unwrap();
        assert_eq!(app.recipe().adjustments["wb_temperature"], 6500.0);
        assert_eq!(app.recipe().adjustments["wb_tint"], 0.0);
        // Non-positive channels cannot derive a white balance.
        assert!(app.set_white_balance_from_point(0.0, 0.5, 0.5).is_err());
    }

    // ---- Filmstrip helpers (headless) ----

    #[test]
    fn filmstrip_downscale_keeps_small_images_and_shrinks_large() {
        let small = vec![1u8, 2, 3, 255, 4, 5, 6, 255];
        let (out, w, h) = crate::filmstrip::downscale_rgba(&small, 2, 1, 160);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, small);

        let big = vec![0u8; (4 * 320 * 200) as usize];
        let (out2, w2, h2) = crate::filmstrip::downscale_rgba(&big, 320, 200, 160);
        assert_eq!((w2, h2), (160, 100));
        assert_eq!(out2.len(), 4 * 160 * 100);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn filmstrip_cache_miss_then_hit_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let name = source.file_name().unwrap().to_string_lossy().to_string();
        let cache = lumina_core::cache::disk::DiskFolderCache::for_image(&source).unwrap();
        // No preview on disk yet -> miss (no silent fallback to a wrong image).
        assert!(!crate::filmstrip::filmstrip_preview_cached(
            &cache,
            &name,
            "vc-original"
        ));
        let thumbnail = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        cache
            .store_preview(
                &name,
                "vc-original",
                lumina_core::cache::PreviewKind::Standard,
                &thumbnail,
            )
            .unwrap();
        // After storing, the same probe is a cache hit.
        assert!(crate::filmstrip::filmstrip_preview_cached(
            &cache,
            &name,
            "vc-original"
        ));
    }

    // ---- F-103-N4: interactive mask tools (Brush / Linear / Radial) ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn brush_marks_roundtrip_through_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        let id = app.create_mask("Subject").unwrap();
        let marks = vec![
            BrushMark {
                x: 0.2,
                y: 0.3,
                radius: 0.05,
                sign: BrushMarkSign::Positive,
            },
            BrushMark {
                x: 0.5,
                y: 0.6,
                radius: 0.05,
                sign: BrushMarkSign::Positive,
            },
        ];
        app.commit_brush_stroke(marks).unwrap();

        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        let mask = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert_eq!(
            mask.prompt,
            Some(MaskPrompt::Brush {
                marks: vec![
                    BrushMark {
                        x: 0.2,
                        y: 0.3,
                        radius: 0.05,
                        sign: BrushMarkSign::Positive,
                    },
                    BrushMark {
                        x: 0.5,
                        y: 0.6,
                        radius: 0.05,
                        sign: BrushMarkSign::Positive,
                    },
                ],
                resolution: (2, 1),
                transformation: PromptTransform::default(),
            })
        );
        // The active layer references the same mask.
        assert_eq!(document.virtual_copies[0].mask_layers[0].mask.mask_id, id);

        // The prompt survives a reopen.
        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        let reloaded = reopened.document.as_ref().unwrap();
        let reloaded_mask = reloaded.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert!(reloaded_mask.prompt.is_some());
        assert_eq!(reloaded_mask.status, MaskStatus::Valid);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn empty_brush_stroke_is_visible_error_and_writes_no_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.create_mask("Subject").unwrap();
        // An empty stroke is rejected by the commit path (returns Err, no write).
        let result = app.commit_brush_stroke(vec![]);
        assert!(result.is_err());

        // The interactive UI path surfaces it as a visible GuiError via
        // `finish_drawing` (which the preview drag-stop calls) and writes
        // nothing to disk.
        app.set_mask_tool(MaskTool::Brush);
        app.pending_brush_marks.clear();
        app.drag_start = Some(Point2 { x: 0.3, y: 0.3 });
        app.drag_current = Some(Point2 { x: 0.3, y: 0.3 });
        app.finish_drawing();
        assert_eq!(app.status(), Str::Error.t());
        assert!(app.error().is_some());

        // No sidecar was written (the empty stroke never persisted).
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        assert!(!sidecar.is_file());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gradient_coordinate_calculation_from_drag() {
        // Helper exposes the prompt-building math directly.
        let p = LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.1, y: 0.5 },
            Point2 { x: 0.9, y: 0.5 },
        );
        // Horizontal drag -> 0 degrees; start/end are the documented matte values.
        match p {
            MaskPrompt::Gradient {
                angle_deg,
                start,
                end,
                ..
            } => {
                assert!((angle_deg - 0.0).abs() < 1e-3);
                assert_eq!(start, 1.0);
                assert_eq!(end, 0.0);
            }
            _ => panic!("expected gradient prompt"),
        }

        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.5, y: 0.1 },
            Point2 { x: 0.5, y: 0.9 },
        ) {
            MaskPrompt::Gradient { angle_deg, .. } => {
                assert!((angle_deg - 90.0).abs() < 1e-3, "vertical drag -> 90°")
            }
            _ => panic!("expected gradient prompt"),
        }

        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.9, y: 0.5 },
            Point2 { x: 0.1, y: 0.5 },
        ) {
            MaskPrompt::Gradient { angle_deg, .. } => {
                assert!((angle_deg - 180.0).abs() < 1e-3, "right-to-left -> 180°")
            }
            _ => panic!("expected gradient prompt"),
        }

        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 0.9, y: 0.9 },
            Point2 { x: 0.1, y: 0.1 },
        ) {
            MaskPrompt::Gradient { angle_deg, .. } => assert!(
                (angle_deg - 225.0).abs() < 1e-3,
                "up-left -> 225° (negative direction normalized to [0,360))"
            ),
            _ => panic!("expected gradient prompt"),
        }

        // Points outside 0..=1 are clamped before the angle is computed.
        match LuminaApp::gradient_prompt_from_drag(
            Point2 { x: 2.0, y: 0.5 },
            Point2 { x: -1.0, y: 0.5 },
        ) {
            // After clamping: (1.0,0.5)->(0.0,0.5) -> dx=-1.0 -> 180°.
            MaskPrompt::Gradient { angle_deg, .. } => {
                assert!((angle_deg - 180.0).abs() < 1e-3)
            }
            _ => panic!("expected gradient prompt"),
        }

        // A zero-length drag is rejected by the commit path.
        let mut app = new_app();
        assert!(app
            .commit_gradient(Point2 { x: 0.5, y: 0.5 }, Point2 { x: 0.5001, y: 0.5 })
            .is_err());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn ellipse_generated_from_center_and_radii() {
        match LuminaApp::ellipse_prompt_from_drag(
            Point2 { x: 0.2, y: 0.2 },
            Point2 { x: 0.8, y: 0.6 },
        ) {
            MaskPrompt::Ellipse { center, radii, .. } => {
                assert!((center.x - 0.5).abs() < 1e-6);
                assert!((center.y - 0.4).abs() < 1e-6);
                assert!((radii.x - 0.3).abs() < 1e-6);
                assert!((radii.y - 0.2).abs() < 1e-6);
            }
            _ => panic!("expected ellipse prompt"),
        }

        // A gradient/radial prompt also persists through the sidecar.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        let id = app.create_mask("Sky").unwrap();
        app.commit_gradient(Point2 { x: 0.1, y: 0.5 }, Point2 { x: 0.9, y: 0.5 })
            .unwrap();
        app.create_mask("Sun").unwrap();
        app.commit_radial(Point2 { x: 0.2, y: 0.2 }, Point2 { x: 0.8, y: 0.6 })
            .unwrap();

        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
        let sky = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert!(matches!(sky.prompt, Some(MaskPrompt::Gradient { .. })));

        let radial = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.name == "Sun")
            .unwrap();
        assert!(matches!(radial.prompt, Some(MaskPrompt::Ellipse { .. })));
    }

    // ---- F-103-N5: shared export path is byte-identical to the CLI ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_export_is_byte_identical_to_shared_export_path() {
        // The GUI export module must produce the exact same bytes as the CLI's
        // shared `lumina_core::export_image` (render + encode) for the same
        // source frame, recipe and export options. PNG is used for the byte
        // comparison because its encoder is deterministic (see
        // feature/platform/cli-gui-wasm.md, "Desktop-GUI / Export-Determinismus").
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let bytes = std::fs::read(&source).unwrap();
        let recipe = EditRecipe {
            adjustments: std::collections::BTreeMap::from([("exposure".into(), 0.7)]),
            ..Default::default()
        };

        // CLI-style shared path: decode + render + encode via the single shared
        // function, with the same neutral context the GUI uses (no masks, no
        // source actions, no white balance for a raster PNG).
        let frame = ImageFrame::decode(&bytes).unwrap();
        let context = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
        };
        let options = ExportOptions {
            format: ImageFileFormat::Png,
            quality: 90,
            dither: false,
            ..Default::default()
        };
        let cli_bytes = export_image(&frame, &context, options).unwrap();

        // GUI path: load the same bytes, set the same recipe, then export via the
        // module's own `export_to` (which internally calls `export_image`).
        let mut app = new_app();
        app.load_bytes(bytes.clone(), "photo.png").unwrap();
        app.set_adjustment("exposure", 0.7);
        app.render().unwrap();
        app.export_format = ImageFileFormat::Png;
        app.export_quality = 90;
        let out = directory.path().join("photo_export.png");
        app.export_to(out.clone()).unwrap();
        let gui_bytes = std::fs::read(&out).unwrap();

        assert_eq!(
            cli_bytes, gui_bytes,
            "GUI and CLI/shared export paths must be byte-identical"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn gui_jpeg_export_is_functional_and_byte_identical_to_shared_path() {
        // JPEG is functionally validated (deterministic within one encoder
        // version, see feature/platform/cli-gui-wasm.md,
        // "Desktop-GUI / Export-Determinismus"): the GUI JPEG export equals the
        // shared `export_image` JPEG export and decodes to the same dimensions
        // as the source.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let bytes = std::fs::read(&source).unwrap();

        let frame = ImageFrame::decode(&bytes).unwrap();
        let context = RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
        };
        let options = ExportOptions {
            format: ImageFileFormat::Jpeg,
            quality: 85,
            dither: false,
            ..Default::default()
        };
        let cli_bytes = export_image(&frame, &context, options).unwrap();

        let mut app = new_app();
        app.load_bytes(bytes.clone(), "photo.png").unwrap();
        app.export_format = ImageFileFormat::Jpeg;
        app.export_quality = 85;
        let out = directory.path().join("photo_export.jpg");
        app.export_to(out.clone()).unwrap();
        let gui_bytes = std::fs::read(&out).unwrap();

        // Both paths use the identical image encoder call, so the bytes match.
        assert_eq!(
            cli_bytes, gui_bytes,
            "JPEG GUI and shared export must match"
        );
        // And the decoded JPEG has the same pixel dimensions as the source.
        let decoded = ImageFrame::decode(&gui_bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (frame.width, frame.height));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn export_rejects_same_path_as_gui_error() {
        // Exporting onto the source file is rejected (non-destructive contract);
        // nothing is written.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("exposure", 0.3);
        let result = app.export_to(source.clone());
        assert!(result.is_err(), "exporting onto the source must fail");
        // No export artifact was created with the source's name.
        assert!(!source.with_extension("jpg").exists());
        assert!(source.is_file());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn export_rejects_extensionless_target_resolving_onto_source() {
        // REVIEW-GUI-EXPORT-1 regression: the extension must be applied BEFORE
        // the same-path check. Target `/d/photo` with format PNG resolves to
        // `/d/photo.png`, which IS the loaded source — the old pre-extension
        // guard compared `/d/photo` against `/d/photo.png` and let the export
        // overwrite the original in full.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let before = blake3::hash(&std::fs::read(&source).unwrap());
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.export_format = ImageFileFormat::Png;

        // Extensionless target that derives onto the source.
        let result = app.export_to(directory.path().join("photo"));
        assert!(
            result.is_err(),
            "extensionless target deriving onto the source must be refused"
        );
        // And onto the source's sidecar / zdata artefacts as well (pure-logic
        // level, see `resolve_export_target_applies_extension_before_guard`;
        // through `export_to` the format extension always lands on png/jpg/webp,
        // so only the source collision itself is reachable end-to-end).

        // The original is untouched and no stray artifacts appeared.
        let after = blake3::hash(&std::fs::read(&source).unwrap());
        assert_eq!(before, after, "original must remain byte-identical");
        let entries: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "only the source file may exist: {entries:?}"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_export_target_applies_extension_before_guard() {
        // Pure-logic coverage of the REVIEW-GUI-EXPORT-1 guard.
        let directory = tempfile::tempdir().unwrap();
        let dir = directory.path();
        let source = dir.join("photo.png");
        std::fs::write(&source, b"original").unwrap();

        // Extensionless target + PNG derives exactly onto the source: refuse.
        let err = LuminaApp::resolve_export_target(
            &source.display().to_string(),
            dir.join("photo"),
            "png",
        )
        .expect_err("must refuse target that resolves onto the source");
        assert!(err.to_string().contains("refusing"), "{err}");

        // Same target with a different format is fine (different file).
        let resolved = LuminaApp::resolve_export_target(
            &source.display().to_string(),
            dir.join("photo"),
            "jpg",
        )
        .unwrap();
        assert_eq!(resolved, dir.join("photo.jpg"));

        // Sidecar and binary mask bundle targets are protected too.
        for blocked_ext in ["lumina.json", "lumina.zdata"] {
            // A typed-in full artefact name keeps its stem; simulate a target
            // whose post-extension form equals the artefact by requesting the
            // matching extension-less stem plus the artefact's own extension
            // via an explicit path.
            let artefact = dir.join(format!("photo.png.{blocked_ext}"));
            std::fs::write(&artefact, b"artefact").unwrap();
            let err = LuminaApp::resolve_export_target(
                &source.display().to_string(),
                artefact.clone(),
                if blocked_ext.ends_with("json") {
                    "json"
                } else {
                    "zdata"
                },
            )
            .expect_err("must refuse target that resolves onto a persisted artefact");
            assert!(err.to_string().contains("refusing"), "{err}");
            assert_eq!(
                std::fs::read(&artefact).unwrap(),
                b"artefact",
                "protected artefact must stay untouched"
            );
        }

        // An unrelated target passes through with the extension applied.
        let ok = LuminaApp::resolve_export_target(
            &source.display().to_string(),
            dir.join("export"),
            "png",
        )
        .unwrap();
        assert_eq!(ok, dir.join("export.png"));

        // Empty source (nothing loaded) never blocks.
        let ok = LuminaApp::resolve_export_target("  ", dir.join("out"), "png").unwrap();
        assert_eq!(ok, dir.join("out.png"));
    }

    #[test]
    fn roi_from_zoom_follows_pan_and_reaches_borders() {
        use super::{LuminaApp, PREVIEW_ROI_MARGIN};
        // Landscape 4000×3000 source in an 800×600 pane: fit = 0.2.
        let (w, h) = (4000_u32, 3000_u32);
        let (pw, ph) = (800.0_f32, 600.0_f32);

        // Fit/zoom-out renders the whole frame.
        assert_eq!(
            LuminaApp::roi_from_zoom(w, h, 1.0, egui::Vec2::ZERO, pw, ph),
            None
        );
        assert_eq!(
            LuminaApp::roi_from_zoom(w, h, 0.5, egui::Vec2::ZERO, pw, ph),
            None
        );

        // Centered pan at 4×: ROI is centred and covers the visible window
        // (pane / (fit·zoom)) plus panning margin on every side.
        let roi = LuminaApp::roi_from_zoom(w, h, 4.0, egui::Vec2::ZERO, pw, ph).unwrap();
        let scale = 0.2_f64 * 4.0;
        let window_w = (800.0_f64 / scale) * PREVIEW_ROI_MARGIN;
        let window_h = (600.0_f64 / scale) * PREVIEW_ROI_MARGIN;
        assert_eq!(roi[2] as f64, window_w.floor());
        assert_eq!(roi[3] as f64, window_h.floor());
        assert!((roi[0] as f64 - (4000.0 - window_w) / 2.0).abs() <= 1.0);
        assert!((roi[1] as f64 - (3000.0 - window_h) / 2.0).abs() <= 1.0);

        // Dragging the image right/up (negative pan delta) moves the window
        // towards the bottom-right; far enough it clamps against that border
        // so the corner becomes reachable (REVIEW-GUI-PANROI-1).
        let br = LuminaApp::roi_from_zoom(w, h, 4.0, egui::vec2(-1200.0, -1200.0), pw, ph)
            .expect("panned ROI");
        assert_eq!(br[0] + br[2], w, "right border reachable");
        assert_eq!(br[1] + br[3], h, "bottom border reachable");

        // Dragging left/down clamps against the top-left border.
        let tl = LuminaApp::roi_from_zoom(w, h, 4.0, egui::vec2(1200.0, 1200.0), pw, ph).unwrap();
        assert_eq!(tl[0], 0, "left border reachable");
        assert_eq!(tl[1], 0, "top border reachable");

        // Extreme zoom stays inside bounds and never returns an empty rect.
        let roi =
            LuminaApp::roi_from_zoom(w, h, 32.0, egui::vec2(12345.0, -9999.0), pw, ph).unwrap();
        assert!(roi[2] >= 1 && roi[3] >= 1);
        assert!(roi[0] + roi[2] <= w && roi[1] + roi[3] <= h);

        // At fit-like zoom the window would cover the whole frame: whole-frame
        // render instead of a degenerate crop.
        assert_eq!(
            LuminaApp::roi_from_zoom(w, h, 1.01, egui::Vec2::ZERO, pw, ph),
            None
        );
    }

    /// REVIEW-GUI-PANROI-1 follow-up: the hand tool must drive the ROI
    /// re-render pipeline. A pan drag invalidates the render key and sets
    /// `pending_full_render`, so the PERF-GUI-3/4 hot path renders a cheap
    /// draft from the new offset while the pointer stays down, and the
    /// debounced full render honours the FINAL pan (borders reachable).
    #[test]
    fn pan_drag_schedules_draft_and_final_roi_render() {
        let ctx = egui::Context::default();
        let mut app = LuminaApp::new(ctx.clone());
        // 200×150 source in an ~800×600 pane → base fit 4.0; at zoom 2 the
        // drawn image overflows the pane, so panning is eligible.
        app.load_bytes(
            ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
                .unwrap()
                .encode(ImageFileFormat::Png)
                .unwrap(),
            "pan.png",
        )
        .unwrap();
        app.render().unwrap();
        // Viewport state as cached by previous frames of `draw_preview`.
        app.preview_zoom = 2.0;
        app.zoom_mode = ZoomMode::Custom;
        app.preview_base_fit_scale = 4.0;
        app.preview_src_w = 200.0;
        app.preview_src_h = 150.0;
        // `draw_preview` needs an existing preview texture to draw at all; it
        // derives the on-screen draw size from the texture dimensions, so it
        // must match a rendered full preview (200×150), not a tiny placeholder.
        app.texture = Some(ctx.load_texture(
            "preview",
            egui::ColorImage::filled([200, 150], egui::Color32::BLACK),
            egui::TextureOptions::LINEAR,
        ));

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let pass = |app: &mut LuminaApp, events: Vec<egui::Event>, time: f64| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
                },
            );
            // No GPU renderer consumes the per-frame texture deltas in these
            // headless tests; dropping them would trip epaint's
            // "unapplied deltas" debug assertion.
            output.textures_delta.clear();
        };

        // Pass 1: pointer-down inside the pane pans nothing yet.
        let start = screen.center();
        // Warm-up pass so the preview widget exists before the press is
        // hit-tested (egui registers interactions one frame after layout).
        pass(&mut app, vec![egui::Event::PointerMoved(start)], 0.9);
        pass(
            &mut app,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            1.0,
        );
        assert!(!app.pending_full_render, "press alone must not re-render");

        // Pass 2: drag right — the pan changes, so a re-render must be armed.
        pass(
            &mut app,
            vec![egui::Event::PointerMoved(start + egui::vec2(60.0, 0.0))],
            1.1,
        );
        assert!(app.preview_pan.x > 0.0, "drag must move the pan");
        assert_eq!(app.zoom_mode, ZoomMode::Custom);
        assert!(
            app.pending_full_render,
            "pan change must schedule the full re-render"
        );
        assert!(
            app.render_key.is_none(),
            "pan change must invalidate the render key so the draft hot path fires"
        );

        // Pass 3: pointer release — the pending full render survives until the
        // debounce commits it with the FINAL pan offset.
        pass(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: start + egui::vec2(60.0, 0.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            1.2,
        );
        assert!(
            app.pending_full_render,
            "released pan must still await its full render"
        );

        // The debounced full render consumes the pending flag and derives the
        // ROI from the post-drag pan.
        let (pw, ph) = (app.preview_pane_w, app.preview_pane_h);
        let pan = app.preview_pan;
        app.render_full([800, 600], None).unwrap();
        assert!(!app.pending_full_render);
        assert_eq!(
            app.preview_roi,
            LuminaApp::roi_from_zoom(
                app.original.as_ref().map(|o| o.width).unwrap_or_default(),
                app.original.as_ref().map(|o| o.height).unwrap_or_default(),
                2.0,
                pan,
                pw,
                ph,
            ),
            "full render must crop exactly the panned visible window"
        );
    }

    /// GUI-SCROLL-200-1 (single view): scroll-wheel zoom over the preview must
    /// stay fluid — it only *arms* the debounced re-render pipeline
    /// (`mark_dirty`); no synchronous full decode/full render may run inside
    /// the frame. A synchronous render would have consumed
    /// `pending_full_render` and produced a fresh `render_key` in the same
    /// pass.
    #[test]
    fn scroll_wheel_zoom_arms_debounce_without_synchronous_render() {
        let ctx = egui::Context::default();
        let mut app = LuminaApp::new(ctx.clone());
        app.load_bytes(
            ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
                .unwrap()
                .encode(ImageFileFormat::Png)
                .unwrap(),
            "zoom.png",
        )
        .unwrap();
        app.render().unwrap();
        app.texture = Some(ctx.load_texture(
            "preview",
            egui::ColorImage::filled([200, 150], egui::Color32::BLACK),
            egui::TextureOptions::LINEAR,
        ));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let pointer = screen.center();
        // Warm-up so the preview widget exists and hit-testing sees the cursor.
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(0.9),
                events: vec![egui::Event::PointerMoved(pointer)],
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
            },
        );
        output.textures_delta.clear();
        // Scroll-wheel zoom with the pointer hovering the preview.
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(1.0),
                events: vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 120.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
            },
        );
        output.textures_delta.clear();
        assert!(app.preview_zoom > 1.0, "wheel must zoom in");
        assert_eq!(app.zoom_mode, ZoomMode::Custom);
        assert!(
            app.pending_full_render,
            "zoom must arm the debounced full render"
        );
        assert!(
            app.render_key.is_none(),
            "no synchronous full render may run during the wheel frame"
        );
    }

    #[test]
    fn sync_zoom_derives_absolute_modes_from_uncropped_source_fit() {
        // REVIEW-GUI-ZOOMLOOP-1 regression: absolute zoom modes must derive
        // from the fit of the pane against the UN-CROPPED source dimensions,
        // never from the currently displayed (ROI-cropped) texture — otherwise
        // 100%/200%/Fit-Width oscillate frame-by-frame once zoom > 1.
        let mut app = new_app();
        // 4000×3000 source in an 800×600 pane → base fit 0.2.
        app.preview_base_fit_scale = 0.2;
        app.preview_src_w = 4000.0;
        app.preview_src_h = 3000.0;
        app.preview_pane_w = 800.0;
        app.preview_pane_h = 600.0;

        // One-to-one: one source pixel per screen point → 1/fit.
        app.zoom_mode = ZoomMode::OneToOne;
        app.preview_zoom = 42.0; // stale value from a previous frame must not matter
        app.sync_zoom();
        assert!((app.preview_zoom - 5.0).abs() < 1e-5);

        // 200% likewise: 2/fit.
        app.zoom_mode = ZoomMode::TwoHundred;
        app.sync_zoom();
        assert!((app.preview_zoom - 10.0).abs() < 1e-5);

        // Fit-width: pane/source ratio relative to base fit (width-limited
        // here, so identical to fit).
        app.zoom_mode = ZoomMode::FitWidth;
        app.sync_zoom();
        assert!((app.preview_zoom - 1.0).abs() < 1e-5);

        // Stability across frames: re-deriving after a simulated render (the
        // texture changed, the cached base geometry did not) yields the exact
        // same value — no oscillation.
        app.zoom_mode = ZoomMode::OneToOne;
        app.sync_zoom();
        let first = app.preview_zoom;
        app.preview_base_fit_scale = 0.2; // unchanged by draw_preview by design
        app.sync_zoom();
        assert_eq!(app.preview_zoom, first);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn export_preserves_original_bytes_unchanged() {
        // The original source file is byte-for-byte untouched by an export.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let before = blake3::hash(&std::fs::read(&source).unwrap());
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("contrast", 0.4);
        app.render().unwrap();
        let out = directory.path().join("photo_out.png");
        app.export_to(out.clone()).unwrap();
        let after = blake3::hash(&std::fs::read(&source).unwrap());
        assert_eq!(
            before, after,
            "original must be byte-identical after export"
        );
        assert!(out.is_file(), "export artifact was written");
    }

    #[test]
    fn export_options_validate_quality_range() {
        // Quality must be in 1..=100; 0 and 101 are rejected, 1 and 100 ok.
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 101,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 1,
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(ExportOptions {
            format: ImageFileFormat::Png,
            quality: 100,
            ..Default::default()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn image_format_from_extension_parses_known_and_rejects_unknown() {
        assert_eq!(
            ImageFileFormat::from_extension("png"),
            Some(ImageFileFormat::Png)
        );
        assert_eq!(
            ImageFileFormat::from_extension("JPG"),
            Some(ImageFileFormat::Jpeg)
        );
        assert_eq!(
            ImageFileFormat::from_extension("jpeg"),
            Some(ImageFileFormat::Jpeg)
        );
        assert_eq!(
            ImageFileFormat::from_extension("webp"),
            Some(ImageFileFormat::WebP)
        );
        assert_eq!(ImageFileFormat::from_extension("tiff"), None);
        assert_eq!(ImageFileFormat::from_extension(""), None);
        assert_eq!(
            ImageFileFormat::default_extension(ImageFileFormat::Jpeg),
            "jpg"
        );
        assert_eq!(
            ImageFileFormat::default_extension(ImageFileFormat::Png),
            "png"
        );
    }

    // ---- Lightroom-like Library folder tree (pure helpers) ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn library_root_is_the_workdir() {
        // Lightroom-parity: the Folders tree roots at the current workdir
        // (`directory` field), never at `$HOME` or an ancestor. Deterministic
        // regardless of the environment `$HOME`.
        assert_eq!(
            library_root("/var/folders/xy/ab/cd"),
            PathBuf::from("/var/folders/xy/ab/cd")
        );
        assert_eq!(library_root("/etc"), PathBuf::from("/etc"));
        assert_eq!(library_root("/"), PathBuf::from("/"));
        assert_eq!(library_root("relative/dir"), PathBuf::from("relative/dir"));
        // Empty workdir (unset) falls back to "." so the tree still has a root.
        assert_eq!(library_root(""), PathBuf::from("."));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn folder_scan_helpers_count_raw_files_with_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        let deeper = sub.join("deeper");
        std::fs::create_dir_all(&deeper).unwrap();
        std::fs::write(dir.path().join("a.ARW"), b"x").unwrap();
        std::fs::write(dir.path().join("b.jpg"), b"x").unwrap();
        std::fs::write(sub.join("c.nef"), b"x").unwrap();
        std::fs::write(deeper.join("d.orf"), b"x").unwrap();

        assert_eq!(count_raw_files(dir.path(), 3), 3);
        // The depth limit stops the scan below `sub`.
        assert_eq!(count_raw_files(dir.path(), 2), 2);
        assert_eq!(count_raw_files(dir.path(), 1), 1);
        assert_eq!(count_raw_files(dir.path(), 0), 0);

        let subs = subdirectories(dir.path());
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0], sub);

        // Labels are root-relative; the root itself shows its final component.
        assert_eq!(folder_label(dir.path(), &sub), "sub");
        let root_name = dir.path().file_name().unwrap().to_string_lossy();
        assert_eq!(folder_label(dir.path(), dir.path()), root_name);
    }

    // ---- Display-only crop overlay thumbnail (pure helper) ----

    #[test]
    fn crop_overlay_rect_maps_normalized_free_crop_into_image_rect() {
        let img = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
        assert!(crop_overlay_rect(None, img).is_none());
        let geometry_none = Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        };
        assert!(crop_overlay_rect(geometry_none.crop.as_ref(), img).is_none());

        // Aspect presets have no normalized rect without the source aspect.
        let aspect = Crop::Aspect {
            preset: lumina_sidecar::AspectPreset::OneToOne,
        };
        assert!(crop_overlay_rect(Some(&aspect), img).is_none());

        let free = Crop::Free {
            x: 0.1,
            y: 0.2,
            width: 0.5,
            height: 0.4,
        };
        let rect = crop_overlay_rect(Some(&free), img).unwrap();
        assert!((rect.min.x - 20.0).abs() < 1e-4);
        assert!((rect.min.y - 30.0).abs() < 1e-4);
        assert!((rect.max.x - 70.0).abs() < 1e-4);
        assert!((rect.max.y - 50.0).abs() < 1e-4);

        // Degenerate crop sizes resolve to no overlay.
        let degenerate = Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.5,
        };
        assert!(crop_overlay_rect(Some(&degenerate), img).is_none());
    }

    // ---- History restore (non-destructive session state change) ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn history_restore_swaps_session_recipe_without_touching_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("exposure", -2.0);
        app.save_sidecar();

        // Seed one explicit history step holding a different recipe state.
        let mut stored = EditRecipe::default();
        stored.adjustments.insert("exposure".into(), 1.5);
        {
            let document = app.document.as_mut().unwrap();
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == app.virtual_copy_id)
                .unwrap();
            copy.history.push(HistoryEntry {
                id: "history-test-1".into(),
                recipe: stored,
                recorded_at: Some("2026-08-23T10:00:00Z".into()),
                extras: BTreeMap::new(),
            });
        }

        // Restore swaps the session recipe and marks the selection.
        app.restore_history("history-test-1").unwrap();
        assert_eq!(app.recipe().adjustments["exposure"], 1.5);
        assert_eq!(app.history_selected.as_deref(), Some("history-test-1"));

        // Non-destructive: the sidecar still holds the pre-restore recipe.
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == "vc-original")
            .unwrap();
        assert_eq!(copy.recipe.adjustments["exposure"], -2.0);

        // Unknown entry ids fail visibly instead of silently resetting.
        assert!(app.restore_history("nope").is_err());
    }

    // ---- GUI-SCROLL-200-1: visible-first thumbnail scheduling (headless) ----

    /// Build an app browsing a folder with `count` supported images and return
    /// the app plus all entry indices (the scheduling helpers are format-
    /// agnostic; the RAW filter only applies to which views show entries).
    #[cfg(not(target_arch = "wasm32"))]
    fn app_with_entries(count: usize) -> (LuminaApp, tempfile::TempDir, Vec<usize>) {
        let directory = tempfile::tempdir().unwrap();
        for i in 0..count {
            save_png(&directory.path().join(format!("img{i:03}.png")));
        }
        let mut app = new_app();
        app.set_directory(directory.path().display().to_string());
        let indices: Vec<usize> = (0..app.entries().len()).collect();
        assert_eq!(indices.len(), count, "every image must become an entry");
        (app, directory, indices)
    }

    /// Visible cells (+ buffer ring) are enqueued first and in full; off-screen
    /// cells receive at most `PREFETCH_BUDGET_PER_FRAME` nearest-first jobs.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn thumbnail_scheduling_prioritizes_visible_over_off_screen() {
        let (mut app, _dir, indices) = app_with_entries(40);
        let keys: Vec<String> = indices
            .iter()
            .map(|&i| app.entries()[i].thumb_key().to_owned())
            .collect();
        let ctx = egui::Context::default();

        // Viewport shows cells 0..10 → buffered 0..18 immediate + 4 prefetch.
        let window = 0..10;
        let buffered_len = viewport::buffered_range(
            window.clone(),
            indices.len(),
            viewport::VISIBLE_BUFFER_CELLS,
        )
        .len();
        let enqueued = app.ensure_thumbnail_priority(&ctx, &indices, window);
        assert_eq!(
            enqueued,
            buffered_len + viewport::PREFETCH_BUDGET_PER_FRAME,
            "visible work must be unbounded, off-screen work capped per frame"
        );
        for (i, key) in keys.iter().enumerate().take(buffered_len) {
            assert!(
                !app.thumbnails.needs_job(key),
                "cell {i} (visible/buffered) must be scheduled"
            );
        }
        for key in keys
            .iter()
            .skip(buffered_len + viewport::PREFETCH_BUDGET_PER_FRAME)
        {
            assert!(
                app.thumbnails.needs_job(key),
                "distant cell must NOT be scheduled yet"
            );
        }

        // Re-scheduling the same window must never redo visible work: every
        // visible cell is already in flight/done, so only the bounded
        // off-screen prefetch progresses (≤ budget per frame).
        let again = app.ensure_thumbnail_priority(&ctx, &indices, 0..10);
        assert!(
            again <= viewport::PREFETCH_BUDGET_PER_FRAME,
            "identical schedule call must stay within the prefetch budget, got {again}"
        );
        let visible_again =
            viewport::buffered_range(0..10, indices.len(), viewport::VISIBLE_BUFFER_CELLS);
        for key in keys.iter().take(visible_again.len()) {
            assert!(
                !app.thumbnails.needs_job(key),
                "visible cell must not be rescheduled"
            );
        }
    }

    /// Scrolling to the end schedules the end first and prefetches backwards
    /// from there (nearest-first), never the far-away start.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn thumbnail_scheduling_follows_the_viewport_nearest_first() {
        let (mut app, _dir, indices) = app_with_entries(40);
        let keys: Vec<String> = indices
            .iter()
            .map(|&i| app.entries()[i].thumb_key().to_owned())
            .collect();
        let ctx = egui::Context::default();
        // First frame: user is at the top.
        app.ensure_thumbnail_priority(&ctx, &indices, 0..10);
        // Then scrolls to the very end without idle time to prefetch.
        let count = indices.len();
        let enqueued = app.ensure_thumbnail_priority(&ctx, &indices, count - 2..count);
        assert_eq!(enqueued, 10 + viewport::PREFETCH_BUDGET_PER_FRAME);
        for key in keys.iter().skip(count - 12) {
            assert!(
                !app.thumbnails.needs_job(key),
                "end-of-list cell must be scheduled"
            );
        }
        // Prefetch walked backwards from index 30: 29, 28, 27, 26.
        for key in &keys[26..30] {
            assert!(
                !app.thumbnails.needs_job(key),
                "nearest prefetch cell must be scheduled"
            );
        }
        // The middle of the list was never touched by this short session.
        for key in &keys[23..26] {
            assert!(
                app.thumbnails.needs_job(key),
                "middle cell must remain untouched"
            );
        }
        for key in keys.iter().take(18.min(count)) {
            assert!(
                !app.thumbnails.needs_job(key),
                "first-frame cells stay scheduled"
            );
        }
    }

    /// A failed worker frees its in-flight slot and stays inside the bounded
    /// retry budget — scheduling never re-enqueues beyond
    /// [`filmstrip::THUMBNAIL_MAX_ATTEMPTS`] (REVIEW-GUI-THUMB-2 regression).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn thumbnail_scheduling_respects_retry_budget() {
        let (mut app, _dir, indices) = app_with_entries(1);
        let key = app.entries()[indices[0]].thumb_key().to_owned();
        let ctx = egui::Context::default();
        // First schedule enqueues exactly one job for the single visible cell.
        assert_eq!(app.ensure_thumbnail_priority(&ctx, &indices, 0..1), 1);
        // While that job is in flight (or has already finished) nothing is
        // rescheduled — no duplicate jobs regardless of worker speed.
        assert_eq!(
            app.ensure_thumbnail_priority(&ctx, &indices, 0..1),
            0,
            "in-flight/done cells must not be rescheduled"
        );
        // Each reported worker failure consumes one bounded retry unit
        // (REVIEW-GUI-THUMB-2); the schedule path must honour that budget.
        for _ in 0..filmstrip::THUMBNAIL_MAX_ATTEMPTS {
            app.thumbnails.mark_failed(&key, "boom");
        }
        assert_eq!(
            app.ensure_thumbnail_priority(&ctx, &indices, 0..1),
            0,
            "exhausted retry budget must not be rescheduled"
        );
        assert_eq!(
            app.thumbnails.failure(&key),
            Some("boom"),
            "exhausted retries must stay a visible error, never a silent fallback"
        );
    }

    // ---- PREVIEW-CACHE-FEATURE: neighbor prefetch scheduling (native) ----

    /// Scheduling the neighbor window around the active image must lazily spawn
    /// the controller and enqueue exactly the available +4/−2 neighbors in the
    /// mandated priority order (no wrap at the edges).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn neighbor_prefetch_schedules_asymmetric_window_around_active() {
        let (mut app, _dir, indices) = app_with_entries(8);
        let active_path = app.entries()[indices[3]].path.display().to_string();
        app.schedule_neighbor_previews(&active_path);
        let ctrl = app.preview_ctrl.as_ref().expect("lazily spawned");
        let mut probes = ctrl.in_flight_probes();
        probes.sort();
        // Active pic3 of 8: window +1..+4, −1..−2 → indices 4,5,2,6,1,7.
        let expected: Vec<String> = [1usize, 2, 4, 5, 6, 7]
            .into_iter()
            .map(|i| app.entries()[indices[i]].thumb_key().to_owned())
            .collect();
        assert_eq!(
            probes.len(),
            6,
            "+4/−2 window on a mid folder = 6 neighbors"
        );
        for want in &expected {
            assert!(probes.contains(want), "scheduled {want}");
        }
        // The active image itself is never a prefetch target (GPU texture only).
        assert!(!probes
            .iter()
            .any(|p| *p == app.entries()[indices[3]].thumb_key()));

        // A second schedule for the same active must not enqueue duplicates
        // (one job per key: in-flight/done probes are skipped).
        let again_enqueued = app.schedule_neighbor_previews(&active_path);
        assert_eq!(again_enqueued, 0, "identical window is fully deduplicated");
    }

    /// At the folder start the window shrinks (no wrap-around).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn neighbor_prefetch_window_shrinks_at_folder_edge() {
        let (mut app, _dir, indices) = app_with_entries(8);
        let start_path = app.entries()[indices[0]].path.display().to_string();
        let enqueued = app.schedule_neighbor_previews(&start_path);
        assert_eq!(enqueued, 4, "+1..+4 only, no backward wrap");
        let ctrl = app.preview_ctrl.as_ref().unwrap();
        let probes = ctrl.in_flight_probes();
        assert_eq!(probes.len(), 4);
        let expected: Vec<String> = [1usize, 2, 3, 4]
            .into_iter()
            .map(|i| app.entries()[indices[i]].thumb_key().to_owned())
            .collect();
        for want in &expected {
            assert!(probes.contains(want), "scheduled {want}");
        }
    }

    /// A directory change discards the neighbor-cache state for the previous
    /// folder (RAM LRU, jobs, failures) so stale entries never resurface.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn directory_change_resets_neighbor_cache() {
        let (mut app, _dir, indices) = app_with_entries(4);
        let active_path = app.entries()[indices[0]].path.display().to_string();
        app.schedule_neighbor_previews(&active_path);
        let ctrl = app.preview_ctrl.as_ref().unwrap();
        assert!(!ctrl.in_flight_probes().is_empty(), "jobs enqueued");
        let second = tempfile::tempdir().unwrap();
        app.set_directory(second.path().display().to_string());
        let ctrl = app.preview_ctrl.as_ref().unwrap();
        assert!(ctrl.lru().is_empty(), "RAM LRU cleared on directory change");
        assert!(
            ctrl.in_flight_probes().is_empty(),
            "in-flight jobs cleared on directory change"
        );
    }

    // ---- REVIEW-GUI-N4: IdleQueue FIFO tie-break ----

    #[test]
    fn idle_queue_pops_fifo_within_same_priority() {
        let mut queue = IdleQueue::new(4);
        queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "first".into(),
                },
                5,
            )
            .unwrap();
        queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "second".into(),
                },
                5,
            )
            .unwrap();
        queue
            .enqueue(
                IdleTask::MaskInference {
                    mask_id: "high".into(),
                },
                9,
            )
            .unwrap();
        // Higher priority first…
        assert_eq!(
            queue.pop_next().unwrap().1,
            IdleTask::MaskInference {
                mask_id: "high".into()
            }
        );
        // …then the *earliest*-enqueued task of the equal-priority class
        // (the old `max_by_key` implementation returned the last maximum →
        // LIFO and popped "second" first).
        assert_eq!(
            queue.pop_next().unwrap().1,
            IdleTask::MaskInference {
                mask_id: "first".into()
            }
        );
        assert_eq!(
            queue.pop_next().unwrap().1,
            IdleTask::MaskInference {
                mask_id: "second".into()
            }
        );
        assert!(queue.pop_next().is_none());
    }

    // ---- REVIEW-GUI-MASKGEO-1: geometry blocks source-coordinate tools ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn geometry_blocks_source_mapping_flags_each_dimension() {
        let mut app = new_app();
        assert!(!app.geometry_blocks_source_mapping(), "default is neutral");
        app.recipe.geometry = Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 90.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        assert!(app.geometry_blocks_source_mapping(), "rotation blocks");
        app.recipe.geometry = Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        });
        assert!(app.geometry_blocks_source_mapping(), "mirror blocks");
        app.recipe.geometry = Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        assert!(app.geometry_blocks_source_mapping(), "crop blocks");
        app.recipe.geometry = None;
        app.recipe.perspective = Some(Perspective {
            version: 1,
            vertical: 0.4,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        assert!(app.geometry_blocks_source_mapping(), "perspective blocks");
        app.recipe.perspective = Some(Perspective {
            version: 1,
            vertical: 0.0,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        assert!(
            !app.geometry_blocks_source_mapping(),
            "a neutral perspective is not blocking"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn set_mask_tool_refused_visibly_while_geometry_active() {
        let mut app = new_app();
        app.load_bytes(png(), "geo.png").unwrap();
        app.recipe.geometry = Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.0,
                y: 0.0,
                width: 0.6,
                height: 0.6,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        app.set_mask_tool(MaskTool::Brush);
        assert_eq!(app.mask_tool, MaskTool::None, "arming must be refused");
        assert!(
            app.status().contains("unavailable"),
            "refusal must be visible, got {:?}",
            app.status()
        );
        // Without geometry arming works again.
        app.recipe.geometry = None;
        app.set_mask_tool(MaskTool::Brush);
        assert_eq!(app.mask_tool, MaskTool::Brush);
        // Disarming stays possible in every state.
        app.set_mask_tool(MaskTool::None);
        assert_eq!(app.mask_tool, MaskTool::None);
    }

    // ---- REVIEW-GUI-SAVEMSG-1 / REVIEW-GUI-N1: save status + CAS ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn failed_save_reports_error_and_never_claims_sidecar_saved() {
        use lumina_sidecar::{load_sidecar, save_sidecar as raw_save, sidecar_path_for};
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("exposure", 1.0);
        app.save_sidecar();
        assert!(app.error().is_none());
        assert_eq!(app.status(), Str::SidecarSaved.t());

        // External modification behind the GUI's back.
        let sidecar = sidecar_path_for(&source);
        let mut external = load_sidecar(&sidecar).unwrap();
        external.virtual_copies[0]
            .recipe
            .adjustments
            .insert("contrast".into(), 0.9);
        raw_save(&sidecar, &external).unwrap();

        // Local unsaved edit + save → the CAS must refuse with a visible
        // conflict instead of silently overwriting the external change.
        app.set_adjustment("exposure", 2.0);
        app.save_sidecar();
        assert_eq!(
            app.status(),
            Str::Error.t(),
            "conflicting save must surface as an error status"
        );
        assert!(app.error().is_some(), "conflict must be visible");
        assert_ne!(
            app.status(),
            Str::SidecarSaved.t(),
            "REVIEW-GUI-SAVEMSG-1: a failed save must never report success"
        );

        // The on-disk document is untouched by the refused write.
        let after = load_sidecar(&sidecar).unwrap();
        assert_eq!(
            after.virtual_copies[0].recipe.adjustments.get("exposure"),
            Some(&1.0)
        );
        assert_eq!(
            after.virtual_copies[0].recipe.adjustments.get("contrast"),
            Some(&0.9)
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn successful_save_keeps_loaded_source_identity_instead_of_recomputing_it() {
        use lumina_sidecar::{load_sidecar, sidecar_path_for};
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        // First session writes an initial sidecar.
        let mut first = new_app();
        open_and_decode(&mut first, source.display().to_string());
        first.save_sidecar();
        assert!(first.error().is_none());

        // Second session LOADS the sidecar; its identity must survive a
        // subsequent save untouched (REVIEW-GUI-N1: no silent recompute /
        // conflict laundering).
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        let loaded_hash = app.document.as_ref().unwrap().source.content_hash.clone();
        assert!(
            loaded_hash.starts_with("blake3:"),
            "precondition: a real loaded identity, got {loaded_hash}"
        );
        app.set_adjustment("exposure", 0.5);
        app.save_sidecar();
        assert!(app.error().is_none());
        assert_eq!(app.status(), Str::SidecarSaved.t());
        let stored = load_sidecar(&sidecar_path_for(&source)).unwrap();
        assert_eq!(
            stored.source.content_hash, loaded_hash,
            "saving must not rewrite the loaded source identity"
        );
    }

    // ---- REVIEW-GUI-VCSWITCH-1: copy switch resets state, surfaces errors ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn select_virtual_copy_resets_session_state_and_notes_discarded_edits() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("contrast", 0.3);
        app.save_sidecar();
        app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
        app.save_sidecar();

        // Previous-copy session state that must not leak across the switch.
        app.history_selected = Some("history-stale".into());
        app.drag_start = Some(Point2 { x: 0.2, y: 0.2 });
        app.drag_current = Some(Point2 { x: 0.4, y: 0.4 });
        app.drawing = true;
        // Unsaved edit relative to vc-original.
        app.set_adjustment("exposure", 3.0);

        app.select_virtual_copy("vc-2").unwrap();
        assert_eq!(app.virtual_copy_id, "vc-2");
        assert_eq!(app.history_selected, None, "history selection must reset");
        assert_eq!(app.drag_start, None, "drag gesture state must reset");
        assert!(!app.drawing, "in-progress drag flag must reset");
        assert!(
            app.status().contains("discarded"),
            "discarding unsaved edits must be stated, got {:?}",
            app.status()
        );

        // A clean switch (no unsaved edits) reports without the warning.
        app.select_virtual_copy("vc-original").unwrap();
        assert!(app.status().starts_with("Switched to copy"));
        assert!(!app.status().contains("discarded"));

        // Unknown ids fail visibly instead of being swallowed.
        assert!(app.select_virtual_copy("nope").is_err());
    }

    // ---- REVIEW-GUI-CURVE-1: tone-curve clamp loss detection ----

    #[test]
    fn tone_curve_roundtrip_loss_is_detected() {
        // Shadows base point is 0.0 — any negative delta is clamped away.
        assert!(tone_curve_roundtrip_is_lossy(-0.5, 0.0, 0.0, 0.0));
        // Darks base 1/3: -1.0 would overshoot below 0 → clamped → lossy.
        assert!(tone_curve_roundtrip_is_lossy(0.0, -1.0, 0.0, 0.0));
        // Lights base 2/3: +1.0 would exceed 1 → clamped → lossy.
        assert!(tone_curve_roundtrip_is_lossy(0.0, 0.0, 1.0, 0.0));
        // Typical representable adjustments are not lossy.
        assert!(!tone_curve_roundtrip_is_lossy(0.25, -0.1, 0.1, -0.25));
        assert!(!tone_curve_roundtrip_is_lossy(0.0, 0.0, 0.0, 0.0));
    }

    // ---- REVIEW-GUI-DEBOUNCE-1: debounce wait schedules its own repaint ----

    #[test]
    fn full_render_debounce_remaining_math() {
        // No drag recorded → immediate render (None).
        assert_eq!(full_render_debounce_remaining(0.0, 500.0), None);
        // Debounce elapsed → due now.
        assert_eq!(full_render_debounce_remaining(10.0, 10.2), None);
        // Still inside the window → the remaining wait, so the caller can
        // request a timed repaint instead of stranding the draft preview.
        let remaining = full_render_debounce_remaining(10.0, 10.05).unwrap();
        assert!((remaining - 0.100).abs() < 1e-9, "got {remaining}");
        let boundary = full_render_debounce_remaining(10.0, 10.0 + 0.150).unwrap_or(0.0);
        assert_eq!(boundary, 0.0, "at the boundary the render is due");
    }

    // ---- REVIEW-GUI-MASKRENDER-1: layer edits schedule a render ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mask_layer_edits_route_through_mark_dirty() {
        let mut app = new_app();
        app.load_bytes(png(), "layer.png").unwrap();
        app.create_mask("Subject").unwrap();
        app.render().unwrap();
        assert!(app.render_key().is_some());

        for edit in [
            |app: &mut LuminaApp| app.set_mask_inverted(true),
            |app: &mut LuminaApp| app.set_mask_feather(0.3),
            |app: &mut LuminaApp| app.set_mask_blur(0.2),
            |app: &mut LuminaApp| app.set_mask_density(0.8),
        ] {
            app.render().unwrap();
            assert!(!app.pending_full_render);
            edit(&mut app).unwrap();
            assert!(
                app.pending_full_render,
                "layer edit must schedule the debounced render"
            );
            assert!(
                app.render_key().is_none(),
                "layer edit must invalidate the stale render key"
            );
        }
    }

    // ---- REVIEW-GUI-N2: recipe restore resolves copies by identity ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn finish_decode_restores_recipe_by_copy_identity_not_position() {
        use lumina_sidecar::{load_sidecar, save_sidecar as raw_save, sidecar_path_for};
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.set_adjustment("exposure", 1.5);
        app.save_sidecar();

        // Add a second copy with a clearly different recipe, then REORDER the
        // JSON array so the default copy is no longer at position 0 — the old
        // positional restore picked up the wrong recipe here.
        let sidecar = sidecar_path_for(&source);
        let mut document = load_sidecar(&sidecar).unwrap();
        document
            .duplicate_virtual_copy("vc-original", "vc-2", "Copy 2")
            .unwrap();
        for copy in &mut document.virtual_copies {
            if copy.id == "vc-2" {
                copy.is_default = false;
                copy.recipe.adjustments.insert("exposure".into(), -4.0);
            } else if copy.id == "vc-original" {
                copy.is_default = true;
            }
        }
        document.virtual_copies.reverse();
        raw_save(&sidecar, &document).unwrap();

        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        assert_eq!(
            reopened.virtual_copy_id, "vc-original",
            "the copy matching the session id must win over array position"
        );
        assert_eq!(
            reopened.recipe().adjustments.get("exposure"),
            Some(&1.5),
            "the restored recipe must come from the identity-matched copy"
        );
    }

    // ---- REVIEW-GUI-N3: file switch resets viewport/session state ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn loading_a_new_image_resets_viewport_and_session_state() {
        let mut app = new_app();
        app.load_bytes(png(), "a.png").unwrap();
        app.preview_zoom = 8.0;
        app.zoom_mode = ZoomMode::Custom;
        app.preview_pan = egui::vec2(42.0, -17.0);
        app.preview_roi = Some([0, 0, 1, 1]);
        app.before_after = true;
        app.wb_pick_mode = true;
        app.history_selected = Some("history-stale".into());
        app.drag_start = Some(Point2 { x: 0.1, y: 0.1 });
        app.drawing = true;

        app.load_bytes(png(), "b.png").unwrap();
        assert_eq!(app.preview_zoom, 1.0, "zoom resets on file switch");
        assert_eq!(app.zoom_mode, ZoomMode::Fit);
        assert_eq!(app.preview_pan, egui::Vec2::ZERO);
        assert_eq!(app.preview_roi, None);
        assert!(!app.before_after, "Before/After must reset");
        assert!(!app.wb_pick_mode, "WB eyedropper must disarm");
        assert_eq!(app.history_selected, None);
        assert_eq!(app.drag_start, None);
        assert!(!app.drawing);
    }

    // ---- REVIEW-GUI-N5: draft preview is never silently measured ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn match_total_exposure_commits_draft_before_measuring() {
        let mut app = new_app();
        app.load_bytes(png(), "draft.png").unwrap();
        app.render().unwrap();
        app.set_adjustment("exposure", 0.5);
        // Simulate the drag-draft state the hot path produces.
        app.render_draft([800, 600], None).unwrap();
        assert!(app.preview_is_draft(), "precondition: preview is a draft");

        app.match_total_exposure(0.5).unwrap();
        assert!(
            !app.preview_is_draft(),
            "matching must measure the committed full render, not the draft"
        );
        assert!(app.recipe().auto_features.matched_exposure.is_some());
    }

    // ---- REVIEW-GUI-N6: failed ROI crop clears preview_roi ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn failed_roi_crop_falls_back_to_full_frame_and_clears_preview_roi() {
        let mut app = new_app();
        app.load_bytes(png(), "roi.png").unwrap(); // 2×1 image

        // A zero-sized crop request genuinely fails; the full frame is
        // rendered, and the rejected ROI must NOT be recorded (it feeds the
        // pointer→source mapping).
        app.render_full([800, 600], Some([0, 0, 0, 9999])).unwrap();
        assert_eq!(app.preview_roi, None);

        // An oversized request is clamped by `crop_region`; the *effective*
        // rect is recorded so the mapping stays truthful.
        app.render_full([800, 600], Some([0, 0, 9999, 9999]))
            .unwrap();
        assert_eq!(app.preview_roi, Some([0, 0, 2, 1]));

        // A valid sub-rect is recorded unchanged.
        app.render_full([800, 600], Some([1, 0, 1, 1])).unwrap();
        assert_eq!(app.preview_roi, Some([1, 0, 1, 1]));
    }

    // ---- KONSISTENZ (REVIEW-CLI-N1): composite zdata tile key ----

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn load_mask_planes_reads_composite_tile_key_and_legacy_fallback() {
        use lumina_sidecar::{save_zdata, zdata_path_for, MaskTile, ZDataContainer};

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        let id = app.create_mask("Subject").unwrap();
        // The plane loader only picks Valid masks; a hand-drawn prompt mask is
        // complete without a model, so mark it valid directly.
        {
            let document = app.document.as_mut().unwrap();
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|c| c.id == app.virtual_copy_id)
                .unwrap();
            let mask = copy.mask_library.iter_mut().find(|m| m.id == id).unwrap();
            mask.status = MaskStatus::Valid;
        }

        let zdata_path = zdata_path_for(&source);
        let tile = |mask_id: String| MaskTile {
            mask_id,
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 1,
            values: vec![u16::MAX, 0],
        };

        // 1) Composite key `"{copy_id}/{mask_id}"` (shared with the CLI).
        save_zdata(
            &zdata_path,
            &ZDataContainer::new(vec![tile(LuminaApp::zdata_tile_record_id(
                "vc-original",
                &id,
            ))])
            .unwrap(),
        )
        .unwrap();
        let planes = app.load_mask_planes();
        assert!(
            planes.contains_key(&("vc-original".to_string(), id.clone())),
            "composite-keyed tile must load under (copy_id, mask_id)"
        );

        // 2) Legacy containers carry the bare mask id; they stay readable via
        // the documented, logged fallback.
        save_zdata(
            &zdata_path,
            &ZDataContainer::new(vec![tile(id.clone())]).unwrap(),
        )
        .unwrap();
        let planes = app.load_mask_planes();
        assert!(
            planes.contains_key(&("vc-original".to_string(), id)),
            "legacy bare-mask-id tiles must remain readable"
        );
    }

    // ---- R2-GUIMOD-01: the debounced full render must retire stale VRAM ----

    /// Simulates the drag→release sequence at the state level: during the drag
    /// `render_to_vram` marks VRAM fresh while the preview shows a draft; the
    /// debounced full render that follows must invalidate that freshness so
    /// the gate stops presenting the soft draft and the sharp CPU pixels get
    /// uploaded instead (the reported "preview stays blurry forever" bug).
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn full_render_invalidates_stale_vram_but_draft_render_keeps_it() {
        let mut app = new_app();
        app.load_bytes(png(), "vram.png").unwrap();

        // Drag state: VRAM carries a draft-source tone result.
        app.preview_is_draft = true;
        app.vram_fresh = true;
        app.render_draft([800, 600], None).unwrap();
        assert!(
            app.vram_fresh,
            "a draft render must keep the VRAM result presentable — it is what \
             the interactive path just rendered into VRAM"
        );

        // Release: the debounced full-quality render supersedes VRAM.
        app.render_full([800, 600], None).unwrap();
        assert!(
            !app.vram_fresh,
            "the full render must invalidate vram_fresh or the present gate \
             keeps showing the superseded draft"
        );
        assert!(!app.preview_is_draft());
    }

    /// R2-GUIMOD-01 belt-and-braces: even if a stale freshness flag ever
    /// slipped through, the geometry cross-check must refuse to present VRAM
    /// content whose dimensions do not describe the current full-quality
    /// preview. Draft previews are exempt by design: the interactive path
    /// presents exactly the draft-source VRAM render.
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn vram_geometry_gate_rejects_dimension_mismatch_for_full_previews() {
        let mut app = new_app();
        app.load_bytes(png(), "geom.png").unwrap(); // 2×1 source
        app.render().unwrap(); // full-quality preview, not a draft

        assert!(
            !app.preview_is_draft() && app.vram_content_matches_displayed_preview((2, 1)),
            "matching dimensions describe the same pixels"
        );
        assert!(
            !app.vram_content_matches_displayed_preview((1280, 720)),
            "draft-sized VRAM behind a full-quality preview must be rejected"
        );

        // Draft exemption: geometry mismatches are allowed while a draft is
        // displayed (the VRAM tone output *is* the draft render).
        app.preview_is_draft = true;
        assert!(app.vram_content_matches_displayed_preview((1280, 720)));

        // No preview at all → nothing may be presented from VRAM.
        app.preview_is_draft = false;
        app.preview = None;
        assert!(!app.vram_content_matches_displayed_preview((2, 1)));
    }

    // ---- R2-GUIMOD-02: CPU present uploads only on content changes ----

    /// The texture handle must survive repaints without new content (same egui
    /// texture id) and be updated **in place** when the preview changes —
    /// never re-created per frame (`load_texture` would mint a fresh id every
    /// time and pay a full-frame upload even for pure mousemoves).
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn cpu_present_reuses_texture_handle_until_content_changes() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        app.load_bytes(png(), "tex.png").unwrap();
        app.render().unwrap();

        app.update_texture(&ctx);
        let first_id = app
            .texture
            .as_ref()
            .expect("texture after first upload")
            .id();
        assert_eq!(
            app.texture_identity.map(|(gen, _, _)| gen),
            Some(app.preview_generation),
            "identity records the generation it was uploaded from"
        );

        // Repaint without any render change (e.g. mousemove over panels):
        // neither the handle nor its pixels may be touched.
        app.update_texture(&ctx);
        assert_eq!(app.texture.as_ref().unwrap().id(), first_id);

        // New preview content: same handle, updated in place, identity bumped.
        app.set_adjustment("exposure", 0.5);
        app.render().unwrap();
        let generation_after_edit = app.preview_generation;
        assert!(
            generation_after_edit > 1,
            "each completed render bumps the preview generation"
        );
        app.update_texture(&ctx);
        assert_eq!(
            app.texture.as_ref().unwrap().id(),
            first_id,
            "the handle must be reused (set), not replaced by load_texture"
        );
        assert_eq!(
            app.texture_identity.map(|(gen, _, _)| gen),
            Some(generation_after_edit)
        );

        // A follow-up repaint with unchanged content stays a no-op again.
        app.update_texture(&ctx);
        assert_eq!(app.texture.as_ref().unwrap().id(), first_id);
    }

    /// Before/After swaps which frame is displayed without touching the
    /// preview generation — the identity must catch the flag change so the
    /// toggle still swaps the visible pixels exactly once per flip.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn cpu_present_uploads_again_when_before_after_flips() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        app.load_bytes(png(), "ba.png").unwrap();
        app.render().unwrap();
        app.update_texture(&ctx);
        assert!(!app.texture_identity.unwrap().1, "preview shown initially");

        app.before_after = true;
        app.update_texture(&ctx);
        assert!(app.texture_identity.unwrap().1, "original shown after flip");

        // Flipping back re-uploads the preview once more.
        app.before_after = false;
        app.update_texture(&ctx);
        assert!(!app.texture_identity.unwrap().1);
    }

    // ---- R2-GUIMOD-05: unsupported-stage verdict memoized per render key ----

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn unsupported_gpu_stage_verdict_is_memoized_per_render_key() {
        let mut app = new_app();
        app.load_bytes(png(), "stages.png").unwrap();
        app.render().unwrap();
        assert!(app.render_key.is_some());

        let verdict = app.recipe_has_unsupported_gpu_stages();
        assert!(
            !verdict,
            "plain exposure-only recipe is fully GPU-supported"
        );
        assert!(
            app.gpu_stage_gate.is_some(),
            "a keyed verdict must be stored once a render key exists"
        );
        // Repeat queries hit the memo and stay consistent.
        assert_eq!(app.recipe_has_unsupported_gpu_stages(), verdict);

        // Editing nulls the render key: the next query must NOT trust (nor
        // store) a memo entry keyed by nothing — the recipe can drift across
        // edits before the next render produces a new key.
        app.set_adjustment("exposure", 1.0);
        assert!(app.render_key.is_none());
        let _ = app.recipe_has_unsupported_gpu_stages();
        assert!(
            app.gpu_stage_gate.is_none(),
            "no memo entry may be cached without a render key"
        );
    }

    // ---- R2-GUIMOD-09: GPU context construction is deferred to attach ----

    #[test]
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu"))]
    fn gpu_context_is_not_created_eagerly_in_new() {
        let app = LuminaApp::new(egui::Context::default());
        assert!(
            app.gpu.is_none(),
            "LuminaApp::new must not perform a blocking adapter/device request; \
             attach_wgpu_render_state owns the single GPU init (R2-GUIMOD-09)"
        );
        assert!(app.wgpu_render_state.is_none());
        assert!(!app.vram_fresh);
    }
}
