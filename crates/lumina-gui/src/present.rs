//! GUI-REFACTOR-W1-20 S1.4a: preview texture upload and the readback-free VRAM
//! present path, extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::update_texture`] owns the CPU `ColorImage` upload (identity
//! cached per `preview_generation`/Before-After/size) and delegates to
//! [`LuminaApp::gpu_present_if_ready`] for the GUI-WGPU-PRESENT-1 present
//! composite; [`LuminaApp::ensure_present_target`] creates/resizes the
//! registered offscreen target. The GPU→CPU fallback behaviour is identical:
//! every condition that cannot present from VRAM keeps the historical CPU
//! upload (no silent divergence). [`LuminaApp::source_identity`] stays with the
//! present path because it shares the same content-hash/decoder identity.
//!
//! `update_texture` and `source_identity` are `pub(crate)` because the app root
//! calls them; the GPU-only helpers keep their original private visibility.

use super::*;
#[cfg(feature = "gpu")]
use log::warn;

impl LuminaApp {
    /// The texture upload is driven by the preview-area path.
    ///
    /// R2-GUIMOD-02: the CPU upload used to run on **every** repaint —
    /// `ColorImage::from_rgba_unmultiplied` (full-frame memcpy) plus
    /// `ctx.load_texture` (full texture re-upload) even when neither the
    /// preview nor the Before/After toggle had changed (e.g. mousemoves over
    /// panels). The upload now happens only when the displayed content
    /// identity changes; the [`egui::TextureHandle`] itself is retained and
    /// updated in place (`handle.set`) so the egui texture id stays stable.
    /// Pixel output is unchanged: identical RGBA bytes, identical options.
    pub(crate) fn update_texture(&mut self, ctx: &egui::Context) {
        // GUI-WGPU-PRESENT-1: when the wgpu renderer shares its device with
        // `lumina-gpu` and the VRAM content is fresh, present straight from
        // VRAM (overlay composite → registered user texture). No CPU readback,
        // no `ColorImage` upload. Every fallback condition below drops to the
        // historical CPU upload, which remains fully functional.
        #[cfg(feature = "gpu")]
        let gpu_present_active = {
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
                    true
                }
                None => {
                    self.gpu_route_fallback = self.routing_fallback_reason();
                    false
                }
            }
        };
        #[cfg(not(feature = "gpu"))]
        let gpu_present_active = false;
        self.update_cpu_texture(ctx, gpu_present_active);
    }

    /// R2-JANK-1 F3: CPU preview-texture upload.
    ///
    /// `gpu_present_active` is the present decision of this frame
    /// ([`Self::gpu_present_if_ready`] succeeded). When it is set, the painted
    /// preview is the registered VRAM user texture, so re-uploading the CPU
    /// `ColorImage` every draft tick is redundant — and during a slider drag
    /// that memcpy + texture upload ran once per tick even though the CPU
    /// pixels were never painted. The upload is therefore skipped while the
    /// GPU present path is active **except** while no CPU handle exists yet:
    /// the handle is created once so the Navigator overview
    /// (`navigator_viewport` clones `self.texture`) and the CPU fallback keep a
    /// valid texture. The identity is left stale while skipping, so the first
    /// frame after the GPU path ends re-uploads the current pixels once.
    /// Before/After always takes the CPU path (`gpu_present_if_ready` refuses
    /// it), so the swap stays exact.
    pub(crate) fn update_cpu_texture(&mut self, ctx: &egui::Context, gpu_present_active: bool) {
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
            let keep_for_navigator = self.texture.is_none();
            if self.texture_identity != Some(identity)
                && (!gpu_present_active || keep_for_navigator)
            {
                let target = if keep_for_navigator {
                    "navigator-handle"
                } else {
                    "preview"
                };
                let bytes = frame.pixels.len();
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
                // R3-LOG-1: name the upload target and the uploaded byte count.
                timing::emit(|| timing::texture_upload_line(target, bytes));
            } else if self.texture_identity != Some(identity) && gpu_present_active {
                // R3-LOG-1 / R2-JANK-1 F3: the GPU present path is active and a
                // CPU handle already exists — the upload is skipped; name the
                // saved bytes instead (measurement only, pixels unchanged).
                timing::emit(|| timing::texture_upload_skip_line("preview", frame.pixels.len()));
            }
        }
    }

    /// GUI-WGPU-PRESENT-1 / GPU-STAGE-1: push the pipeline-evaluated combined
    /// mask planes into the VRAM present composite. Extracted verbatim from
    /// `render_pipeline.rs` (ratchet: the pipeline hub stays <= 500 lines);
    /// failures are loud but never break the CPU preview path.
    #[cfg(feature = "gpu")]
    pub(crate) fn upload_evaluated_mask_to_vram(&mut self) {
        self.vram_mask_is_evaluated = false;
        if self.render_mask_layers.is_empty() {
            return;
        }
        let planes: Vec<lumina_core::MaskPlane> = self
            .render_mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        match lumina_gpu::combine_mask_planes(&planes) {
            Ok(Some(combined))
                if combined.width == self.preview.as_ref().map(|p| p.width).unwrap_or(0)
                    && combined.height == self.preview.as_ref().map(|p| p.height).unwrap_or(0) =>
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

    /// GPU-present eligibility + composite + registration (GUI-WGPU-PRESENT-1).
    ///
    /// Returns the registered texture id and pixel size when the preview can
    /// be presented readback-free this frame. Deliberately conservative: any
    /// condition that would change visible pixels beyond the documented F-043
    /// tolerance keeps the CPU present path (no silent divergence):
    /// - Before/After displays the original — never in VRAM;
    /// - zoomed ROI previews crop on the CPU — geometry must not jump;
    /// - recipes with GPU-unsupported stages would render tone-only in VRAM
    ///   (the documented GPU-STAGE-1 Restrisiko) — CPU pixels are exact
    ///   ([`Self::gpu_unsupported_reasons`]);
    /// - a non-identity Lensfun corrector is applied by the CPU reference even
    ///   without a manual `lens_correction`; since GPU-LENSFUN-PARITY-1 the
    ///   present path binds its `LensfunMap` on the VRAM context
    ///   (`lensfun_gpu::bind`), so only an unbindable map or a `lumina-gpu` map
    ///   guard refusal (dimension mismatch, distortion without an explicit crop)
    ///   keeps the exact CPU route — captured in `vram_render_refusal`;
    /// - stale VRAM (`vram_fresh == false`) after any edit **or** after any
    ///   completed full-quality CPU render (R2-GUIMOD-01);
    /// - R2-GUIMOD-01 belt-and-braces: for a non-draft preview the VRAM
    ///   dimensions must match the preview dimensions — a full-resolution CPU
    ///   result whose geometry differs from the (draft-sized) VRAM content
    ///   must win even if a stale freshness flag ever slipped through.
    #[cfg(feature = "gpu")]
    fn gpu_present_if_ready(&mut self) -> Option<(egui::TextureId, [usize; 2])> {
        if !self.vram_fresh || self.before_after || self.preview_roi.is_some() {
            return None;
        }
        // R5-MASKVIS-25: the VRAM overlay pass combines all evaluated layers.
        // Present from the CPU texture when any editorial mask-overlay gate is
        // closed, when a live gradient/radial prompt needs the CPU painter, or
        // when the evaluated plane set is not exactly the selected mask;
        // otherwise the CPU painter can show the required selected matte.
        if !self.gpu_mask_overlay_is_selected() {
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
        if let Err(err) = self
            .gpu
            .as_ref()?
            .copy_vram_to_texture(&texture, self.overlay_color)
        {
            log::warn!("gpu overlay present failed: {err}");
            return None;
        }
        Some((id, [dims.0 as usize, dims.1 as usize]))
    }

    /// Create or resize the offscreen present target and keep it registered as
    /// an egui user texture with the eframe wgpu renderer.
    #[cfg(feature = "gpu")]
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

    /// THUMB-HASH-PERF-35: the loaded source's persisted identity, built by the
    /// one shared constructor so it cannot drift from the selection-sidecar
    /// path (which is the stateless twin of this method).
    pub(crate) fn source_identity(&self, frame: &ImageFrame) -> SourceIdentity {
        crate::source_identity::build_source_identity(
            if self.source_name.is_empty() {
                "dropped-image".into()
            } else {
                self.source_name.clone()
            },
            self.source_bytes.as_deref(),
            frame,
            self.raw_orientation,
            self.source_is_raw,
        )
    }
}

/// GUI-JANKLOG-19 (`janklog`, debug only): the present route and the visible
/// badge reason of the last painted frame, sourced from the values this module
/// produces (`update_texture` sets `gpu_route_fallback`). Borrowed so the jank
/// record clones only when a slow scope is actually emitted. Diagnostic only —
/// never consulted for routing.
#[cfg(all(feature = "janklog", debug_assertions))]
impl LuminaApp {
    pub(crate) fn jank_route_and_badge(&self) -> (&'static str, Option<&str>) {
        #[cfg(feature = "gpu")]
        {
            (self.gpu_route_label(), self.gpu_route_fallback.as_deref())
        }
        #[cfg(not(feature = "gpu"))]
        {
            (GPU_ROUTE_NA, None)
        }
    }
}
