//! Mask texture uploads, GPU present composition, and mask-plane readback.

use super::gpu_util::overlay_uniforms;
use super::{shaders, GpuContext, GpuError};

impl GpuContext {
    /// Upload a single dirty 512² (or edge-clipped) mask tile into the active
    /// VRAM mask texture. `tile_data` is `u16` little-endian coverage,
    /// row-major. The tile must lie fully inside the active entry's dimensions
    /// — an out-of-bounds tile is a hard error, never silently clipped.
    pub fn upload_mask_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        tile_w: u32,
        tile_h: u32,
        tile_data: &[u8],
    ) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed(
                "vram not ready for mask upload".into(),
            ));
        };
        if tile_x.saturating_add(tile_w) > v.width || tile_y.saturating_add(tile_h) > v.height {
            return Err(GpuError::RenderFailed(format!(
                "mask tile ({tile_x},{tile_y} {tile_w}x{tile_h}) exceeds the active \
                 VRAM mask {}x{}",
                v.width, v.height
            )));
        }
        shaders::write_mask_tile(
            &resources.queue,
            &v.mask,
            tile_x,
            tile_y,
            tile_w,
            tile_h,
            tile_data,
        );
        Ok(())
    }

    /// Upload a complete evaluated mask plane into the active VRAM mask
    /// texture (GPU-STAGE-1).
    ///
    /// This is the data path that makes *pipeline-evaluated* masks visible in
    /// the GPU present composite: after a full render, the caller pushes the
    /// combined effective planes (`combine_mask_planes`) here and the overlay
    /// pass shows exactly the coverage the measurement semantics (F-041)
    /// attribute to the frame — instead of only live brush stamps.
    ///
    /// Errors when no VRAM state exists or the plane does not match its
    /// dimensions (no silent fallback).
    pub fn upload_mask_plane(
        &self,
        width: u32,
        height: u32,
        values: &[u16],
    ) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        if values.len() != (width as usize) * (height as usize) {
            return Err(GpuError::RenderFailed(format!(
                "mask plane has {} values, expected {width}*{height}",
                values.len()
            )));
        }
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed(
                "vram not ready for mask plane upload".into(),
            ));
        };
        if v.width != width || v.height != height {
            return Err(GpuError::RenderFailed(format!(
                "mask plane {width}x{height} does not match the active VRAM state {}x{}",
                v.width, v.height
            )));
        }
        shaders::write_u16_plane(&resources.queue, &v.mask, width, height, values);
        Ok(())
    }

    /// GPU-GPU copy/overlay of the active VRAM tone + mask textures into an
    /// egui-managed `dest` texture. No CPU readback — the copy is an overlay
    /// render pass directly on the queue. `dest` must be created with
    /// `TEXTURE_BINDING|COPY_DST|RENDER_ATTACHMENT`, `Rgba8Unorm`, and the same
    /// dimensions as the active VRAM cache entry (a mismatch is a hard error).
    /// `overlay_color` is the effective session/selected-mask tint and is
    /// uploaded to the existing overlay uniform; this entry has no default.
    ///
    /// The GUI shares the eframe device/queue via [`Self::from_parts`], registers
    /// `dest` as a native egui texture and paints it without a CPU readback.
    pub fn copy_vram_to_texture(
        &self,
        dest: &wgpu::Texture,
        overlay_color: [u8; 3],
    ) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        // R2-GPU-06: a lost device must not panic — skip the present so the GUI
        // keeps showing the last good CPU preview instead of crashing.
        if resources
            .device_lost
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(GpuError::AdapterUnavailable("GPU device lost".into()));
        }
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Ok(());
        };
        if dest.size().width != v.width || dest.size().height != v.height {
            return Err(GpuError::RenderFailed(format!(
                "present target {}x{} does not match the active VRAM state {}x{}",
                dest.size().width,
                dest.size().height,
                v.width,
                v.height
            )));
        }
        let uniforms = overlay_uniforms(overlay_color);
        shaders::write_overlay_uniforms(&resources.queue, &v.overlay_uniform, &uniforms);
        let dest_view = dest.create_view(&wgpu::TextureViewDescriptor::default());
        // R2-GPU-02: the existing pipeline is cached per target format; its bind
        // group remains stable because all referenced VRAM parts are pooled.
        let format = dest.format();
        if let std::collections::hash_map::Entry::Vacant(e) = v.overlay_pipelines.entry(format) {
            let pipe = shaders::create_overlay_pipeline(&resources.device, format)
                .map_err(|e| GpuError::RenderFailed(e.to_string()))?;
            e.insert(pipe);
        }
        let overlay_pipe = &v.overlay_pipelines[&format];
        let bind = &v.overlay_bind_group;
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-overlay-present"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dest_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(overlay_pipe);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        resources.queue.submit(Some(enc.finish()));
        Ok(())
    }

    /// Diagnostic/test helper: read the active VRAM mask plane back to the CPU
    /// (`map_async`) and return it as `(width, height, u16 values)` in the
    /// exact source domain. This is the counterpart of
    /// [`Self::upload_mask_plane`]/[`Self::upload_mask_tile`] and exists so
    /// the mask data path has a byte-exact regression net — the interactive
    /// present path itself never calls this.
    pub fn readback_mask_plane(&self) -> Result<(u32, u32, Vec<u16>), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Err(GpuError::AdapterUnavailable(
                "no adapter for mask readback".into(),
            ));
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        let (width, height) = (v.width, v.height);
        // The mask plane is `R16Uint` (2 bytes per texel), so a tight row is
        // `width * 2` bytes. `aligned_bytes_per_row` already multiplies by the
        // RGBA8 4 bytes/texel, so its argument is a *4-byte texel count*; the
        // equivalent for the mask row is `ceil(width * 2 / 4) = ceil(width / 2)`.
        // Passing `width * 2` (as the old code did) over-allocated 4× — the
        // same class as the fixed Dehaze dark-channel readback; at 24 MP that
        // was ≈186 MiB instead of ≈47 MiB of staging.
        let bytes_per_row = shaders::aligned_bytes_per_row(width.div_ceil(2));
        let staging = resources.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumina-gpu-mask-readback"),
            size: (bytes_per_row * height.max(1)) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-mask-readback-enc"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &v.mask,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        resources.queue.submit(Some(enc.finish()));
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        resources
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::RenderFailed(format!("device poll: {e}")))?;
        rx.recv()
            .map_err(|e| GpuError::RenderFailed(format!("map channel: {e}")))?
            .map_err(|e| GpuError::RenderFailed(format!("buffer map: {e}")))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|e| GpuError::RenderFailed(format!("mapped view: {e}")))?;
        let row_u16s = width as usize;
        let mut values = Vec::with_capacity(row_u16s * height as usize);
        for y in 0..height as usize {
            let start = y * bytes_per_row as usize;
            let row = &mapped[start..start + row_u16s * 2];
            values.extend(
                row.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b)),
            );
        }
        drop(mapped);
        drop(guard);
        staging.unmap();
        Ok((width, height, values))
    }
}
