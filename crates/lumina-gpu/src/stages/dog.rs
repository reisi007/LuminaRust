//! The Presence Difference-of-Gaussians (Texture/Clarity) stage.
//!
//! `macro_rules!` macros are **textually scoped**: `common_src!()` is only
//! resolvable in this module because `pub mod dog;` in `../stages.rs` is
//! declared *after* the `common_src!` definition. Moving that declaration
//! breaks the build ("cannot find macro `common_src`").

use super::build_simple_pipeline;

// ---------------------------------------------------------------------------
// Presence: Texture/Clarity Difference-of-Gaussians (apply_dog).
// ---------------------------------------------------------------------------

/// Uniform block for one DoG pass (16 bytes, uniform-aligned).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DogParams {
    /// Box-window radius in pixels (`1..=32`).
    pub radius: u32,
    /// Detail amount (the CPU `texture`/`clarity` slider value).
    pub amount: f32,
    pub _pad: [u32; 2],
}

impl DogParams {
    /// Texture DoG parameters from a Presence recipe: radius
    /// `1 + round(|texture| * 2)`, amount `texture`.
    pub fn texture(texture: f32) -> Self {
        Self {
            radius: 1 + (texture.abs() * 2.0).round() as u32,
            amount: texture,
            _pad: [0; 2],
        }
    }

    /// Clarity DoG parameters: radius `8 + round(|clarity| * 24)`, amount
    /// `clarity`.
    pub fn clarity(clarity: f32) -> Self {
        Self {
            radius: 8 + (clarity.abs() * 24.0).round() as u32,
            amount: clarity,
            _pad: [0; 2],
        }
    }
}

/// WGSL for the box-DoG pass. The 2-D window is `(2r+1)²` **clamped to the
/// frame** (edge pixels excluded, exactly like `apply_dog`'s
/// `saturating_sub`/`min` bounds), accumulated in row-major order so the
/// floating-point sum matches the oracle.
pub const DOG_STAGE_SRC: &str = concat!(
    r#"
struct DogParams {
  radius : u32,
  amount : f32,
  pad0 : u32,
  pad1 : u32,
};
@group(0) @binding(0) var<uniform> params : DogParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
fn texel_coord(c : vec2<i32>) -> vec2<u32> {
  return vec2<u32>(u32(c.x), u32(c.y));
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let tex_dims = textureDimensions(input_tex);
  let dims = vec2<i32>(i32(tex_dims.x), i32(tex_dims.y));
  let coord = vec2<i32>(i32(frag_coord.x), i32(frag_coord.y));
  let radius = i32(params.radius);
  let x0 = max(coord.x - radius, 0);
  let x1 = min(coord.x + radius, dims.x - 1);
  let y0 = max(coord.y - radius, 0);
  let y1 = min(coord.y + radius, dims.y - 1);
  let source = textureLoad(input_tex, texel_coord(coord), 0);
  var out = source;
  for (var c = 0u; c < 3u; c = c + 1u) {
    let src_byte = roundi(source[c] * 255.0);
    var sum : f32 = 0.0;
    var count : f32 = 0.0;
    for (var yy = y0; yy <= y1; yy = yy + 1) {
      for (var xx = x0; xx <= x1; xx = xx + 1) {
        sum = sum + roundi(textureLoad(input_tex, texel_coord(vec2<i32>(xx, yy)), 0)[c] * 255.0);
        count = count + 1.0;
      }
    }
    let detail = src_byte - sum / count;
    out[c] = clamp(roundi(src_byte + params.amount * detail), 0.0, 255.0) / 255.0;
  }
  return out;
}
"#
);

/// Bind group layout for [`DOG_STAGE_SRC`]: uniform (0) + input (1).
pub fn create_dog_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-dog-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// Build the DoG render pipeline for `target_format`.
pub fn create_dog_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    build_simple_pipeline(
        device,
        "dog",
        DOG_STAGE_SRC,
        &create_dog_bind_group_layout(device),
        target_format,
    )
}

pub fn create_dog_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-dog-params"),
        size: std::mem::size_of::<DogParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_dog_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &DogParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

pub fn create_dog_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-dog-bindgroup"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(input_view),
            },
        ],
    })
}
