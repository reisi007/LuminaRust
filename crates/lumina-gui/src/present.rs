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

    pub(crate) fn source_identity(&self, frame: &ImageFrame) -> SourceIdentity {
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
