//! Neighbor-preview job planning and the background worker render.
//!
//! Extracted from `preview_ctrl.rs` and `lib.rs` for the file-size ratchet
//! (R3-ROUTING-1 / R3-DENOISE-1 wave): the controller keeps the UI-side
//! bookkeeping (LRU, in-flight/retry, priority dispatch), while this module
//! owns the +4/−2 window planning ([`LuminaApp::schedule_neighbor_previews`])
//! and the off-thread decode/render/encode ([`worker_preview`]). Cohesive
//! boundary: everything that reads a neighbor file or allocates a job lives
//! here; nothing here touches the active preview.

use crate::preview_ctrl::{
    file_stamp, plan_window_jobs, PreviewController, PreviewJob, PreviewOutcome, PreviewResult,
    PreviewStamp,
};
use crate::LuminaApp;
use lumina_core::preview_cache::{encode_webp_lossless, PreviewDiskCache, PreviewKey, PreviewKind};
use lumina_core::{
    render_frame_with_denoise, DenoiseStageInput, DenoiseStageStatus, ImageFrame, RenderContext,
};
use lumina_sidecar::EditRecipe;
use std::path::{Path, PathBuf};

impl LuminaApp {
    /// Plan and enqueue the asymmetric +4/−2 neighbor-preview window around the
    /// currently active image `active_path`. The worker pool is spawned lazily
    /// on first navigation so headless tests stay thread-free. The authoritative
    /// state of each neighbor (content hash, sidecar recipe) is resolved inside
    /// the workers, never on the UI thread.
    pub(crate) fn schedule_neighbor_previews(&mut self, active_path: &str) -> usize {
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
        // R3-DENOISE-1: read the session policy before the `preview_ctrl` borrow
        // so the worker jobs carry the same fallback semantics as the active
        // render.
        let denoise_policy = self.denoise_policy();
        let preview_ctrl = self.preview_ctrl.get_or_insert_with(|| {
            // Pool clamped to a small dedicated size (the SOLL mandates a fixed
            // small pool; thumbnails keep their own pool). The disk tier is
            // rooted per-job at the source's own `.lumina/previews` folder.
            let pool_size = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(2, 4);
            let (ctrl, _queue) = PreviewController::spawn(pool_size);
            ctrl
        });
        // A6: inherit the folder / display option — a 1:1 zoom plans neighbors
        // at 1:1 resolution, otherwise the (default) Screen preview is used.
        // The worker keeps the decoded frame at full resolution for `OneToOne`
        // (no downscaling), so the target here only drives the Screen path.
        let (kind, target) = if self.zoom_mode == crate::ZoomMode::OneToOne {
            (PreviewKind::OneToOne, (0, 0))
        } else {
            (
                PreviewKind::Screen,
                (self.draft_max_dim, self.draft_max_dim),
            )
        };
        // A6: when the kind/resolution changes (e.g. zoom → 1:1) the previously
        // prepared neighbors are stale for the new key and are lazily re-rendered.
        preview_ctrl.plan_kind(kind);
        let jobs = plan_window_jobs(
            &probe_ids,
            &sources,
            &names,
            active,
            target,
            kind,
            denoise_policy,
        );
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
}

/// Decode + render + downscale + WebP-encode a neighbor on the background
/// worker. Returns the decoded frame so the UI thread can use it immediately,
/// and stores the encoded WebP to the source's own `.lumina/previews` tier.
pub(crate) fn worker_preview(job: PreviewJob) -> Result<PreviewResult, String> {
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
        depth: None,
    };
    // LRPAR-G14-DENOISE-IMPL-20 / R3-DENOISE-1: the neighbor must follow the
    // *same* denoise fallback policy as the active preview. The stand-in frame
    // is downscaled, so the full-frame `denoise_rgb` artifact can never blend
    // here; the stage is therefore resolved as non-ready for this render and,
    // under the default `Warn` policy, falls back visibly (manual F-096 /
    // identity) — instead of the former core `Strict` default that hard-failed
    // every neighbor with an active `denoise_ai` while the active image fell
    // back (the R3-DENOISE-1 inconsistency). `Strict` still aborts loudly here
    // exactly like the active render.
    let denoise = DenoiseStageInput::non_ready(
        DenoiseStageStatus::Unavailable,
        "neighbor preview stand-in: the full-frame denoise artifact is applied \
         only to the active full-resolution render",
    )
    .with_policy(job.denoise_policy);
    let rendered = render_frame_with_denoise(&frame, &context, &denoise)
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
