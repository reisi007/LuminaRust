//! GPU shader stage: uniform buffers, 32³ 3D LUT bake, FP16 framebuffer helper.
//!
//! This module owns the data the color/tone fragment shader consumes. The actual
//! WGSL shader body is filled in by a later subagent; the structures here (and
//! the identity-LUT stub in [`bake_3d_lut`]) define the contract that the shader
//! and the [`super::GpuContext`] pipeline must honour.

use lumina_sidecar::EditRecipe;

/// Uniform-buffer layout for the color/tone shader stage.
///
/// Mirrors the slider parameters from [`EditRecipe::adjustments`]. `#[repr(C)]`
/// + `bytemuck::{Pod, Zeroable}` so it can be uploaded directly into a
/// `wgpu::Buffer` bound as a uniform. Padded to 64 bytes (16 × f32) to satisfy
/// uniform-buffer alignment.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub wb_temperature: f32,
    pub wb_tint: f32,
    pub vibrance: f32,
    pub saturation: f32,
    /// Padding to 64 bytes (multiple of 16) for uniform-buffer alignment.
    pub _pad: [f32; 6],
}

impl Uniforms {
    /// Build the uniform block from a recipe.
    ///
    /// Missing keys default to `0.0` (identity for every Lumina adjustment),
    /// with one deliberate exception: `wb_temperature` defaults to the neutral
    /// colour temperature **6500 K** (not `0.0`). This mirrors the CPU oracle in
    /// `lumina-core::apply_channel_lut_adjustments`, which uses
    /// `temperature.unwrap_or(6500.0)` and `tint.unwrap_or(0.0)` — so an absent
    /// WB key yields `warmth = 0` and therefore the identity gain triple
    /// `[1, 1, 1]`, while a present key (with or without its sibling) is graded
    /// exactly as the CPU does. Keeping the GPU path *always applying* WB with
    /// the same neutral defaults is therefore byte-equivalent to the CPU's
    /// conditional `Option<[f64; 3]>` while avoiding a separate presence flag in
    /// the 64-byte uniform block.
    pub fn from_recipe(recipe: &EditRecipe) -> Self {
        let get = |key: &str| recipe.adjustments.get(key).copied().unwrap_or(0.0) as f32;
        Self {
            exposure: get("exposure"),
            contrast: get("contrast"),
            highlights: get("highlights"),
            shadows: get("shadows"),
            whites: get("whites"),
            blacks: get("blacks"),
            wb_temperature: recipe
                .adjustments
                .get("wb_temperature")
                .copied()
                .unwrap_or(6500.0) as f32,
            wb_tint: get("wb_tint"),
            vibrance: get("vibrance"),
            saturation: get("saturation"),
            _pad: [0.0; 6],
        }
    }
}

/// Edge length of the baked 3D LUT (32³ nodes, per the DAG plan).
pub const LUT_DIM: usize = 32;
/// Total node count of the 32³ LUT.
pub const LUT_SIZE: usize = LUT_DIM * LUT_DIM * LUT_DIM;

/// A 32×32×32 RGBA 3D lookup table.
///
/// Indexed `data[r + g*32 + b*32*32]`; each entry is `(r, g, b, a)` in `0..=1`.
/// [`bake_3d_lut`] produces the graded table; the shader samples it trilinearly.
#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct Lut32x32x32 {
    pub data: [[f32; 4]; LUT_SIZE],
}

impl Lut32x32x32 {
    /// Identity LUT: node `(r, g, b)` maps to `(r/31, g/31, b/31, 1)`.
    pub fn identity() -> Self {
        let mut data = [[0.0f32; 4]; LUT_SIZE];
        let mut i = 0usize;
        for b in 0..LUT_DIM {
            for g in 0..LUT_DIM {
                for r in 0..LUT_DIM {
                    data[i] = [r as f32 / 31.0, g as f32 / 31.0, b as f32 / 31.0, 1.0];
                    i += 1;
                }
            }
        }
        debug_assert_eq!(i, LUT_SIZE);
        Self { data }
    }
}

/// Bake the recipe into a 32³ 3D LUT.
///
/// **Stub:** returns the identity LUT and logs. The real implementation maps the
/// tone/color-grade portion of `recipe` through the CPU reference kernel
/// (mirroring `lumina-core`) once the shader stage is wired, so the GPU and CPU
/// paths stay byte-consistent.
pub fn bake_3d_lut(recipe: &EditRecipe) -> Lut32x32x32 {
    log::debug!(
        "bake_3d_lut: building identity 32³ LUT stub (recipe '{}' adjustments ignored for now)",
        recipe.recipe_version
    );
    Lut32x32x32::identity()
}

/// Output/working color format for the byte-consistent color/tone pass:
/// 8-bit unorm RGBA. The shader runs the same integer-rounded math as the
/// `lumina-core` CPU oracle directly in the sRGB-encoded byte domain, so the
/// render target is plain `Rgba8Unorm` (no automatic sRGB↔linear conversion).
/// The FP16 helper below stays available for a later linear working space.
pub const RGBA8_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Working color format for the linear pipeline: 16-bit float RGBA (kept for a
/// later linear working-space / HDR intermediate).
pub const FP16_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Round a byte-row size up to the 256-byte alignment `wgpu` requires for
/// `copy_texture_to_buffer` (and `copy_buffer_to_texture`) source/target
/// strides. A 64px-wide RGBA8 row is exactly 256 bytes, but other widths need
/// padding that the readback copy must account for.
pub fn aligned_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
    // Ceil to multiple of 256.
    ((unpadded + 255) / 256) * 256
}

/// Create the upload texture that carries the decoded source frame into the
/// color/tone shader. Format is `RGBA8_FORMAT` (raw sRGB-encoded bytes, no
/// sRGB→linear decode on sample), with `TEXTURE_BINDING` (sampled) and
/// `COPY_DST` (uploaded via `queue.write_texture`).
pub fn create_input_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        view_formats: &[],
        format: RGBA8_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    })
}

/// Create the nearest-neighbour, clamp-to-edge sampler used to read the input
/// texture. Nearest filtering (plus exact texel-centre UVs from
/// `@builtin(position)`) makes the sample bit-for-bit the stored byte, which is
/// what the integer-rounded tone math expects.
pub fn create_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

/// Create the RGBA8 render target the color/tone pass draws into. It is both a
/// `RENDER_ATTACHMENT` (drawn to) and `COPY_SRC` (read back to the CPU `Frame`).
pub fn create_output_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        view_formats: &[],
        format: RGBA8_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    })
}

/// Create the staging buffer the rendered RGBA8 target is copied into before
/// `map_async` readback. Sized to the 256-byte-aligned stride × height, with
/// `COPY_DST | MAP_READ`.
pub fn create_readback_buffer(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> wgpu::Buffer {
    let bytes_per_row = aligned_bytes_per_row(width);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (bytes_per_row * height.max(1)) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    })
}

/// Create the bind group that binds the uniform block (binding 0), the input
/// texture (binding 1) and the sampler (binding 2) for the color/tone pass.
/// Built per render because the input texture/sampler change with each frame.
pub fn create_color_tone_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-color-tone-bindgroup"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(input_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Create an FP16 (`RGBA16Float`) texture usable as a render attachment, sampled
/// binding and copy source — the intermediate framebuffer for the color/tone pass.
pub fn create_fp16_framebuffer(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        view_formats: &[],
        format: FP16_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    })
}

/// Allocate a uniform buffer sized for [`Uniforms`].
pub fn create_uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload `uniforms` into `buffer` via the queue.
pub fn write_uniforms(queue: &wgpu::Queue, buffer: &wgpu::Buffer, uniforms: &Uniforms) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(uniforms));
}
