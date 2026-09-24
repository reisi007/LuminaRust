//! R5-BRUSH-24 live mask-plane lifecycle for the GPU preview.
//!
//! The retained R16 plane is a cache of exactly one `(source, copy, mask)`
//! prompt. Selection/copy/source changes and committed prompts drop the cache;
//! the next dab rebuilds that prompt before applying the live mark. This keeps a
//! sibling mask's pixels from leaking into the active brush or present texture.

use super::*;

impl LuminaApp {
    /// Drop the session-only live-plane cache. The next selected-mask dab
    /// rebuilds and re-uploads the full prompt; no stale plane is presented in
    /// the meantime because prompt/selection commits call `mark_dirty`.
    pub(crate) fn reset_brush_mask_plane(&mut self) {
        #[cfg(feature = "gpu")]
        {
            self.brush_mask_plane = None;
            self.brush_mask_plane_dims = None;
            self.brush_mask_plane_scope = None;
        }
    }

    #[cfg(feature = "gpu")]
    fn current_brush_plane_scope(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(String, String, String, u32, u32), GuiError> {
        let document = self
            .document
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
        if !document
            .virtual_copies
            .iter()
            .any(|copy| copy.id == self.virtual_copy_id)
        {
            return Err(GuiError::Io(Str::VirtualCopyNotFound.t().to_string()));
        }
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        if !document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .is_some_and(|copy| copy.mask_library.iter().any(|mask| mask.id == mask_id))
        {
            return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
        }
        Ok((
            document.source.content_hash.clone(),
            self.virtual_copy_id.clone(),
            mask_id,
            width,
            height,
        ))
    }

    /// Ensure the retained plane is the selected mask's current prompt.
    /// Returns true when a full rebuild happened and the complete VRAM texture
    /// must be uploaded before dirty tiles.
    #[cfg(feature = "gpu")]
    fn ensure_brush_mask_plane(&mut self, width: u32, height: u32) -> Result<bool, GuiError> {
        let scope = self.current_brush_plane_scope(width, height)?;
        if self.brush_mask_plane_scope.as_ref() == Some(&scope)
            && self.brush_mask_plane_dims == Some((width, height))
            && self.brush_mask_plane.is_some()
        {
            return Ok(false);
        }
        let prompt = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .and_then(|copy| {
                copy.mask_library
                    .iter()
                    .find(|mask| Some(mask.id.as_str()) == self.selected_mask_id.as_deref())
            })
            .and_then(|mask| mask.prompt.clone());
        let values = match prompt {
            // A completed brush gesture replaces a non-brush prompt. Until the
            // release is committed, therefore, its live base is an empty
            // plane; this also keeps range prompts (which need source pixels
            // and cannot be geometrically rasterized here) from blocking the
            // first brush dab.
            Some(prompt @ MaskPrompt::Brush { .. }) => {
                rasterize_prompt(&prompt, width, height)
                    .map_err(|error| {
                        let message = Str::MaskLivePlaneRebuildPattern
                            .t()
                            .replacen("{}", &format!("{scope:?}"), 1)
                            .replacen("{}", &error.to_string(), 1);
                        GuiError::Io(message)
                    })?
                    .values
            }
            Some(_) | None => vec![0u16; width as usize * height as usize],
        };
        self.brush_mask_plane = Some(values);
        self.brush_mask_plane_dims = Some((width, height));
        self.brush_mask_plane_scope = Some(scope);
        Ok(true)
    }

    /// Rebuild (when needed) and stamp one live mark into the selected mask's
    /// plane. Returns the tiles that changed and whether a full prompt upload is
    /// required. Kept separate from wgpu so headless tests exercise identity and
    /// separation without requiring a GPU adapter.
    #[cfg(feature = "gpu")]
    pub(crate) fn stamp_live_brush_mark(
        &mut self,
        mark: BrushMark,
    ) -> Result<(Vec<lumina_gpu::tiling::TileKey>, bool), GuiError> {
        let (width, height) = self.image_dims()?;
        let rebuilt = self.ensure_brush_mask_plane(width, height)?;
        let plane = self
            .brush_mask_plane
            .as_mut()
            .ok_or_else(|| GuiError::Io(Str::MaskLivePlaneMissing.t().to_string()))?;
        lumina_core::mask_tiles::stamp_brush_mark_with_options(plane, width, height, mark);
        Ok((
            lumina_gpu::tiling::dirty_tiles_for_brush_mark(
                mark.x,
                mark.y,
                mark.radius,
                width,
                height,
            ),
            rebuilt,
        ))
    }

    /// Stamp and upload the live mark. A selection/prompt rebuild uploads the
    /// entire prompt first; subsequent dabs upload only intersecting 512² tiles.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_upload_brush_tile(&mut self, nx: f32, ny: f32) {
        if !self
            .gpu
            .as_ref()
            .is_some_and(lumina_gpu::GpuContext::is_available)
        {
            return;
        }
        let Ok((width, height)) = self.image_dims() else {
            return;
        };
        if let Some(gpu) = self.gpu.as_ref() {
            if let Err(error) = gpu.ensure_vram(width, height) {
                log::warn!("gpu ensure_vram({width}x{height}) failed: {error}");
                return;
            }
        }
        let mark = self.brush_mark_at(nx, ny);
        let (tiles, rebuilt) = match self.stamp_live_brush_mark(mark) {
            Ok(value) => value,
            Err(error) => {
                self.show_error(error);
                return;
            }
        };
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if rebuilt {
            let plane = self.brush_mask_plane.as_deref().unwrap_or_default();
            if let Err(error) = gpu.upload_mask_plane(width, height, plane) {
                log::warn!(
                    "full live brush plane upload failed for {}x{}: {error}",
                    width,
                    height
                );
            }
            return;
        }
        for tile in tiles {
            let x0 = tile.tx * lumina_gpu::tiling::TILE_SIZE;
            let y0 = tile.ty * lumina_gpu::tiling::TILE_SIZE;
            let tile_width = (lumina_gpu::tiling::TILE_SIZE)
                .min(width.saturating_sub(x0))
                .max(1);
            let tile_height = (lumina_gpu::tiling::TILE_SIZE)
                .min(height.saturating_sub(y0))
                .max(1);
            let mut tile_u16 = Vec::with_capacity((tile_width * tile_height) as usize);
            let plane = self
                .brush_mask_plane
                .as_deref()
                .expect("live plane exists after stamp");
            for row in 0..tile_height {
                let start = ((y0 + row) * width + x0) as usize;
                let end = start + tile_width as usize;
                tile_u16.extend_from_slice(&plane[start..end]);
            }
            let tile_bytes: &[u8] = bytemuck::cast_slice(&tile_u16);
            if let Err(error) = gpu.upload_mask_tile(x0, y0, tile_width, tile_height, tile_bytes) {
                log::warn!(
                    "brush tile upload failed at tile ({x0},{y0}) ({tile_width}x{tile_height}): {error}"
                );
            } else {
                log::trace!(
                    "gpu_upload_brush_tile stamped ({nx:.3},{ny:.3}) r={:.3} -> tile ({x0},{y0}) {tile_width}x{tile_height}",
                    mark.radius
                );
            }
        }
    }
}
