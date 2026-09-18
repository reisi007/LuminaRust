//! Small GPU encoding/readback helpers extracted from `lib.rs` (file-size
//! ratchet, DoD §8): the fullscreen-pass encoder, the geometry sub-stage encoder
//! and the two texture/buffer readback helpers.

use crate::{Frame, GpuError, GpuResources};

/// Map a 4-byte `MAP_READ` staging buffer and reinterpret its `u32` payload as
/// the sharpening gradient maximum (`f32::from_bits`). Used by
/// [`crate::GpuContext::encode_sharpening`] after its `atomicMax` reduction.
pub(crate) fn readback_gradient_max(
    resources: &GpuResources,
    staging: &wgpu::Buffer,
) -> Result<f32, GpuError> {
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
    let bits = u32::from_le_bytes([mapped[0], mapped[1], mapped[2], mapped[3]]);
    drop(mapped);
    staging.unmap();
    Ok(f32::from_bits(bits))
}

/// Read an arbitrary RGBA8 texture back into a CPU [`Frame`].
///
/// Used by the dimension-changing geometry path (`render_with_gpu`), whose
/// final texture carries the oracle's output dimensions rather than the pooled
/// source-sized readback buffer.
pub(crate) fn readback_texture(
    resources: &GpuResources,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Frame, GpuError> {
    let bytes_per_row = crate::shaders::aligned_bytes_per_row(width);
    let staging = resources.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-geometry-readback"),
        size: (bytes_per_row * height.max(1)) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = resources
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lumina-gpu-geometry-readback-enc"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
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
    resources.queue.submit(Some(encoder.finish()));
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
    let row_bytes = (width * 4) as usize;
    let mut pixels = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        let start = y * bytes_per_row as usize;
        pixels.extend_from_slice(&mapped[start..start + row_bytes]);
    }
    drop(mapped);
    staging.unmap();
    Ok(Frame {
        width,
        height,
        pixels,
    })
}

/// Encode one fullscreen-triangle draw into `dst` with `pipeline`/`bind_group`.
pub(crate) fn encode_fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    dst: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("lumina-gpu-post-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: dst,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Encode one geometry sub-stage pass: allocate a transient uniform buffer for
/// `params`, bind the (uniform + input texture) pair against `input_view` and
/// draw into `dst`.
pub(crate) fn encode_geometry_pass<T: bytemuck::Pod>(
    resources: &GpuResources,
    layout: &wgpu::BindGroupLayout,
    pipeline: &wgpu::RenderPipeline,
    params: &T,
    input_view: &wgpu::TextureView,
    dst: &wgpu::TextureView,
    encoder: &mut wgpu::CommandEncoder,
) {
    let buffer = crate::geometry::create_geometry_uniform_buffer(
        &resources.device,
        std::mem::size_of::<T>() as u64,
        "lumina-gpu-geometry-params",
    );
    crate::geometry::write_geometry_params(&resources.queue, &buffer, params);
    let bind =
        crate::geometry::create_geometry_bind_group(&resources.device, layout, &buffer, input_view);
    encode_fullscreen_pass(encoder, pipeline, &bind, dst);
}
