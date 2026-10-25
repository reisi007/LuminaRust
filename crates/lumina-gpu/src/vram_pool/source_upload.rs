//! The single CPU→GPU upload of a source frame into a cached input texture.
//!
//! Extracted from the two render entry points (GPU-UPLOAD-IDENTITY-92,
//! file-size ratchet §8) so the decision ([`super::upload_identity::should_upload`])
//! and the transfer cannot drift apart: both sites call the same write with the
//! frame's own geometry and pixel buffer.

use lumina_core::ImageFrame;

/// Uploads `frame` into `texture` through `queue`.
///
/// The caller has already decided that the upload is needed (see
/// `should_upload`); this function only performs the transfer, byte-for-byte the
/// same `write_texture` call both entry points used inline before.
pub(crate) fn upload_source_texture(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    frame: &ImageFrame,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        frame.pixels(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(frame.width() * 4),
            rows_per_image: Some(frame.height()),
        },
        wgpu::Extent3d {
            width: frame.width(),
            height: frame.height(),
            depth_or_array_layers: 1,
        },
    );
}
