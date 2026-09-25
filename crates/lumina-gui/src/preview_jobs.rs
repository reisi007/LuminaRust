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
    plan_window_jobs, PreviewController, PreviewJob, PreviewOutcome, PreviewResult,
};
use crate::LuminaApp;
use lumina_core::preview_cache::{encode_webp_lossless, PreviewDiskCache, PreviewKey, PreviewKind};
use lumina_core::{
    prepare_source_base, render_frame_with_denoise, DenoiseStageInput, DenoiseStageStatus,
    ImageFrame, RenderContext, StageWork,
};
use lumina_sidecar::EditRecipe;
use std::path::{Path, PathBuf};

impl LuminaApp {
    /// Plan and enqueue the asymmetric +4/−2 neighbor-preview window around the
    /// currently active image `active_path`. The worker pool is spawned lazily
    /// on first navigation so headless tests stay thread-free. Enqueue captures
    /// deterministic input identities; workers resolve the exact recipes and
    /// artifact bundles they actually render.
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
                (
                    self.preview_cap_state.draft_max_dim,
                    self.preview_cap_state.draft_max_dim,
                ),
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
#[cfg(test)]
pub(crate) fn worker_preview(job: PreviewJob) -> Result<PreviewResult, String> {
    let identity =
        crate::source_actions::NeighborInputIdentity::capture(&job.source, &job.virtual_copy);
    worker_preview_with_identity(job, identity)
}

pub(crate) fn worker_preview_with_identity(
    job: PreviewJob,
    enqueued_identity: crate::source_actions::NeighborInputIdentity,
) -> Result<PreviewResult, String> {
    let bytes = std::fs::read(&job.source).map_err(|e| format!("{}: {e}", job.source.display()))?;

    // Render with the neighbor's own recipe (its sidecar, if any): the worker
    // — not the UI thread — reads the sidecar, keeping the main thread free of
    // per-neighbor file I/O on navigation. Validate `SidecarDocument.source`
    // against these exact bytes before even decoding the stand-in.
    //
    // GUI-SRCACC-1: source actions are full-frame artifacts. Resolve and apply
    // them before Screen downscaling; a missing/stale/corrupt/invalid bundle is
    // returned as a visible worker failure. B7 likewise forbids replacing a
    // later render failure with the un-rendered base frame.
    let sidecar = crate::source_actions::read_sidecar_recipe_snapshot_for_bytes(
        &job.source,
        &job.virtual_copy,
        &bytes,
    )?;
    let recipe = sidecar.recipe;
    let decoded = if crate::is_raw_name(&job.name) {
        lumina_raw::decode_bytes(&bytes, &job.name)
            .map_err(|e| e.to_string())?
            .frame
    } else {
        ImageFrame::decode(&bytes).map_err(|e| e.to_string())?
    };
    let zdata_path = lumina_sidecar::zdata_path_for(&job.source);
    let source_actions =
        crate::source_actions::resolve_source_actions(&recipe, &zdata_path, &decoded)
            .map_err(|error| error.to_string())?;
    let prepared_identity = crate::source_actions::NeighborInputIdentity::from_worker(
        &bytes,
        sidecar.document_identity,
        &source_actions,
    );
    let source_base = if source_actions.is_empty() {
        decoded
    } else {
        let mut source_work = StageWork::default();
        prepare_source_base(&decoded, source_actions.artifacts(), &mut source_work)
            .map_err(|error| format!("source actions for {}: {error}", job.name))?
    };

    // Build the render input only after SourceActions: 1:1 keeps the prepared
    // full frame; Screen reduces that post-action frame to the target long edge.
    let frame = if job.kind == PreviewKind::OneToOne {
        source_base
    } else {
        downscale_to_target(&source_base, job.target)
    };
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
        render_key: render_key_of(&recipe, (rendered.width, rendered.height), &source_actions),
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

    Ok(PreviewResult {
        digest,
        probe_id: job.probe_id.clone(),
        source: job.source,
        virtual_copy: job.virtual_copy,
        name: job.name,
        enqueued_identity,
        prepared_identity: Some(prepared_identity),
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
fn render_key_of(
    recipe: &EditRecipe,
    target: (u32, u32),
    source_actions: &crate::source_actions::ResolvedSourceActions,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"preview-render");
    hasher.update(&target.0.to_le_bytes());
    hasher.update(&target.1.to_le_bytes());
    if let Ok(bytes) = serde_json::to_vec(recipe) {
        hasher.update(&bytes);
    }
    // GUI-SRCACC-1: recipe contains only references. Add the identities of the
    // runtime artifacts that were actually composited before downscaling.
    for identity in source_actions.identities() {
        hasher.update(&[0]);
        hasher.update(identity.id.as_bytes());
        hasher.update(&[0]);
        hasher.update(identity.checksum.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
