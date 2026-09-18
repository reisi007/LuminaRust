//! GUI-REFACTOR-W1-20 S1.2b (hub): `render_from`, the shared core render used by
//! both `render_full` (see `render_entry.rs`) and the interactive draft path,
//! extracted verbatim from `lib.rs`.
//!
//! This is the 472-line staged pipeline hub: base-stage cache lookup →
//! adjustments/geometry/masks → histogram analysis → present bookkeeping. It
//! is `pub(crate)` because the entry module and the draft tick in
//! `render_tick.rs` call it. No behaviour changes — the base-stage invalidation
//! contract (PERF-GUI-1) and every trace/warn are byte-identical.

use super::*;
use log::{trace, warn};

impl LuminaApp {
    /// Core render used by both [`Self::render_full`] and [`Self::render_draft`].
    /// `with_masks` enables the sidecar mask planes (skipped for the draft,
    /// whose source is downscaled and therefore misaligned with full-res masks).
    /// `roi` optionally crops the source to the visible region before the render
    /// (PERF-GUI-5). The crop applies to the DISPLAY texture only — the tone /
    /// histogram analysis always runs on the un-cropped full frame
    /// (GUI-HISTOGRAM-FULL-1).
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
    pub(crate) fn render_from(
        &mut self,
        source: &ImageFrame,
        with_masks: bool,
        roi: Option<[u32; 4]>,
        generative: GenerativeArtifacts,
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
        let copy_id = self.virtual_copy_id.clone();
        let mask_hashes = self
            .document
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
            .unwrap_or_default();
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

        // LRPAR-G14-DENOISE-IMPL-20: resolve the KI-Denoise stage state (recipe
        // request + persisted `denoise_rgb` record + producer provenance) once
        // per render, then feed the resolved input into the shared pipeline.
        // The state is computed here, before the shared `masks_context` borrow.
        let denoise_artifact = if self.denoise_stage_active() {
            self.load_denoise_artifact()
        } else {
            None
        };
        self.denoise_gui = self.resolve_denoise_state(denoise_artifact.as_ref());
        self.denoise_gui_dirty = false;
        // Mask artifact planes loaded from the optional `.lumina.zdata` sidecar
        // (native only).  Missing or unreadable zdata is not a hard error:
        // affected layers are reported through the `MaskPolicy::Warn` path.
        // G-06: Lensfun auto-corrector for the rendered base (cached by
        // identity + dimensions; the manual model applies when none
        // matches — same strict contract as the CLI render path). Runs
        // BEFORE the shared `masks_context` borrow below (`&mut` first,
        // shared borrows after).
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(base_frame.width, base_frame.height);
        #[cfg(feature = "lensfun")]
        let lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let lensfun = None;
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
        // ---- Downstream stages: Adjustments → geometry → Masks ----
        // GEN-ONNX-1 Welle 2b: the caller-supplied generative canvases enter the
        // shared pipeline at the mid-geometry positions
        // (`Lens → [auto-fill] → Perspective → [expand] → Crop`); the core owns
        // the compositing, the GUI never re-implements it.
        let denoise_input = self.denoise_render_input(denoise_artifact.as_ref());
        let output = render_frame_from_base_with_generative_and_denoise(
            base_frame,
            &RenderContext {
                recipe: &self.recipe,
                camera_white_balance: self.camera_white_balance,
                source_actions: &[],
                masks: masks_context,
                lensfun,
                depth: None,
            },
            &mut work,
            generative.input(),
            &denoise_input,
        )?;
        // GEN-ONNX-1 Welle 2b: `render_frame_from_base_with_generative` already
        // ran the generative stage internally
        // (`Lens → [auto-fill] → Perspective → [expand] → Crop`) by adopting the
        // caller-supplied canvas artifacts. The core frame is the preview — a
        // second post-render expand must not run here (the canvas no longer
        // matches the frame and `validate_with_source` would fail), and the GUI
        // keeps no generative pixel logic of its own (Agents.md: keine
        // GUI-spezifische Bildlogik außerhalb der Pipeline).
        let mask_warnings = output.mask_warnings;
        let mut preview = output.frame;
        // GUI-HISTOGRAM-FULL-1: identity of the un-cropped full-frame base for
        // the histogram analysis render below. Computed here while
        // `source_hash`/`decode_version`/`copy_id` are still owned — the
        // `render_key` construction beneath moves them. `None` when no ROI
        // was requested (the preview already is the full frame, so no second
        // analysis render is needed).
        let full_analysis_digest: Option<String> = if effective_roi.is_some() {
            Some(
                RenderKey::new(
                    source_hash.clone(),
                    decode_version.clone(),
                    "raster-mvp-1",
                    copy_id.clone(),
                    &EditRecipe::default(),
                    Vec::new(),
                    OutputSpec {
                        profile: "sRGB".into(),
                        width: source.width,
                        height: source.height,
                        format: "rgba8".into(),
                    },
                )
                .stage_digest(CacheStage::Base),
            )
        } else {
            None
        };
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
        // GUI-HISTOGRAM-FULL-1 (F-100): one shared pass yields both the tone
        // panel values and the 256-bin histogram feeding the Painter curve.
        // The analysis input is ALWAYS the un-cropped full frame — never the
        // ROI-cropped viewport texture: while zoomed the display preview is a
        // magnified crop, but the histogram must still describe the whole
        // image. Only when an ROI was actually rendered is a second,
        // un-cropped analysis render needed (at Fit the preview already is
        // the full frame, so it is analyzed directly with zero extra cost).
        // The draft/full distinction is untouched: a draft analysis render
        // uses the draft source, so `preview_is_draft` keeps describing the
        // histogram (REVIEW-GUI-N5).
        // R2-GUIMOD-04a: timed for the per-tick drag instrumentation
        // (measurement only — the result is used exactly as before).
        let ana_t0 = std::time::Instant::now();
        let (analysis, histogram) = match (
            effective_roi.is_some() && !crop_failed,
            full_analysis_digest,
        ) {
            (true, Some(full_digest)) => {
                // Resolve the full-frame base first (mutable cache borrow only —
                // no recipe borrow yet, so this never aliases the render below).
                // The digest matches a settled Fit render byte-for-byte, hence a
                // warm cache hit whenever the full frame was rendered before and
                // only the cheaper downstream stages re-execute here.
                let mut analysis_work = StageWork::default();
                let full_base = match self.base_stage_cache.get(&full_digest) {
                    Some(hit) => {
                        analysis_work.base_cache_hit = true;
                        trace!(
                            "GUI render: full-frame analysis base cache HIT ({}x{})",
                            source.width,
                            source.height
                        );
                        hit
                    }
                    None => {
                        let prepared = prepare_source_base(source, &[], &mut analysis_work)?;
                        self.base_stage_cache
                            .insert(full_digest.clone(), prepared.clone());
                        analysis_work.base_cache_hit = false;
                        trace!(
                            "GUI render: full-frame analysis base cache MISS — rebuilt ({}x{})",
                            source.width,
                            source.height
                        );
                        prepared
                    }
                };
                // G-06: Lensfun auto-corrector for the analysed full frame
                // (same cached lookup as the preview render above). Runs
                // BEFORE the shared `full_masks` borrow below.
                #[cfg(feature = "lensfun")]
                self.ensure_lensfun_cache(full_base.width, full_base.height);
                #[cfg(feature = "lensfun")]
                let full_lensfun = self.lensfun_render_ref();
                #[cfg(not(feature = "lensfun"))]
                let full_lensfun = None;
                let full_masks = if with_masks {
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
                let full_denoise_input = self.denoise_render_input(denoise_artifact.as_ref());
                let full_output = render_frame_from_base_with_generative_and_denoise(
                    full_base,
                    &RenderContext {
                        recipe: &self.recipe,
                        camera_white_balance: self.camera_white_balance,
                        source_actions: &[],
                        masks: full_masks,
                        lensfun: full_lensfun,
                        depth: None,
                    },
                    &mut analysis_work,
                    generative.input(),
                    &full_denoise_input,
                )?;
                analyze_tone_with_histogram(&full_output.frame)
            }
            _ => analyze_tone_with_histogram(&preview),
        };
        self.last_analysis_ms = ana_t0.elapsed().as_secs_f64() * 1000.0;
        self.tone_analysis = Some(analysis);
        self.preview_histogram = Some(histogram);
        // G04-FOLLOWUP-1 Visualize-Spots: the recipe threshold tints
        // candidate pixels red as a pure display post-process on the preview
        // frame. Render, export and CLI stay untinted (their paths never pass
        // here); the histogram above analyzed the clean frame. Gated by the
        // G-11 overlay mode via `spot_visualize_overlay_threshold`; an
        // invalid recipe value fails loudly instead of rendering untinted
        // silently. The `render_key` above still describes the pipeline
        // output — the tint is a deterministic view transform of it.
        if let Some(threshold) = self.spot_visualize_overlay_threshold() {
            let tinted = apply_visualize_overlay(&mut preview, threshold)?;
            trace!("GUI render: spot visualize overlay t={threshold} tinted {tinted}px");
        }
        self.preview = Some(preview);
        // R2-GUIMOD-02: new preview content — any CPU-present identity cached
        // in `texture_identity` is now stale and will re-upload once.
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
        #[cfg(feature = "gpu")]
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
        // GUI-DRAFT-JUMP-1: record which source space the texture (and
        // `preview_roi`) lives in so `draw_preview` and the pointer→source
        // mapping can scale back into full-source geometry. A failed crop
        // fell back to the full `source`, whose dims are recorded here just
        // the same — the texture always matches `source`, never the rejected
        // request.
        self.preview_render_src = Some((source.width, source.height));
        self.render_mask_layers = output.mask_layers;
        // GUI-WGPU-PRESENT-1 / GPU-STAGE-1: make the *pipeline-evaluated* mask
        // coverage visible in the GPU present composite by pushing the combined
        // effective planes into the VRAM mask texture. Failures are loud but
        // never break the CPU preview path.
        #[cfg(feature = "gpu")]
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
}
