/// `apply_noise_reduction` (F-096): 5x5 bilateral luminance filter plus a
/// chroma filter, strengths blending the source with the filtered value.
/// Both-shared uniform block (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NoiseParams {
    /// Luminance filter strength (`0..=1`; `0` is identity).
    pub luminance: f32,
    /// Chroma filter strength (`0..=1`; `0` is identity).
    pub color: f32,
    pub _pad: [u32; 2],
}

impl NoiseParams {
    /// Build from a recipe's `noise_reduction`. The caller must only enqueue
    /// the pass when `luminance != 0 || color != 0` (the oracle's early return).
    pub fn from_recipe(n: &lumina_sidecar::NoiseReduction) -> Self {
        Self {
            luminance: n.luminance,
            color: n.color,
            _pad: [0; 2],
        }
    }

    /// Whether the oracle would run the filter (neither strength is identity).
    pub fn needs_stage(n: &lumina_sidecar::NoiseReduction) -> bool {
        n.luminance != 0.0 || n.color != 0.0
    }
}

/// WGSL for the Noise Reduction pass. Operates in the byte domain
/// (`round(texel * 255)`), accumulates the 5x5 window in the oracle's
/// row-major order and rounds the result back to `u8`.
pub const NOISE_STAGE_SRC: &str = concat!(
    r#"
struct NoiseParams {
  luminance : f32,
  color : f32,
  pad0 : u32,
  pad1 : u32,
};
@group(0) @binding(0) var<uniform> params : NoiseParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
fn lum_byte(p : vec4<f32>) -> f32 {
  return 0.2126 * roundi(p.r * 255.0)
    + 0.7152 * roundi(p.g * 255.0)
    + 0.0722 * roundi(p.b * 255.0);
}

fn byte255(x : f32) -> f32 {
  return clamp(roundi(x), 0.0, 255.0) / 255.0;
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let dims = textureDimensions(input_tex);
  let w = i32(dims.x);
  let h = i32(dims.y);
  let coord = vec2<i32>(i32(frag_coord.x), i32(frag_coord.y));
  let src = textureLoad(input_tex, vec2<u32>(coord), 0);
  let r_b = roundi(src.r * 255.0);
  let g_b = roundi(src.g * 255.0);
  let b_b = roundi(src.b * 255.0);
  let base_y = lum_byte(src);
  var ly : f32 = 0.0;
  var cy_r : f32 = 0.0;
  var cy_b : f32 = 0.0;
  var sum : f32 = 0.0;
  var csum : f32 = 0.0;
  for (var dy = -2; dy <= 2; dy = dy + 1) {
    for (var dx = -2; dx <= 2; dx = dx + 1) {
      let xx = clamp(coord.x + dx, 0, w - 1);
      let yy = clamp(coord.y + dy, 0, h - 1);
      let p = textureLoad(input_tex, vec2<u32>(u32(xx), u32(yy)), 0);
      let yj = lum_byte(p);
      let d2 = f32(dx * dx + dy * dy);
      let spatial = exp(-d2 / (2.0 * 1.5 * 1.5));
      let diff = base_y - yj;
      let lum = exp(-(diff * diff) / (2.0 * 0.12 * 255.0 * 0.12 * 255.0));
      let weight = spatial * lum;
      ly = ly + weight * yj;
      sum = sum + weight;
      let cw = exp(-d2 / (2.0 * 2.0 * 2.0));
      csum = csum + cw;
      cy_r = cy_r + cw * (roundi(p.r * 255.0) - yj);
      cy_b = cy_b + cw * (roundi(p.b * 255.0) - yj);
    }
  }
  let filtered_y = ly / sum;
  let yv = base_y * (1.0 - params.luminance) + filtered_y * params.luminance;
  let cr = (r_b - base_y) * (1.0 - params.color) + (cy_r / csum) * params.color;
  let cb = (b_b - base_y) * (1.0 - params.color) + (cy_b / csum) * params.color;
  let cg = g_b - base_y;
  return vec4<f32>(
    byte255(yv + cr),
    byte255(yv + cg),
    byte255(yv + cb),
    src.a
  );
}
"#
);

/// Bind group layout for [`NOISE_STAGE_SRC`]: uniform (0) + input (1).
pub fn create_noise_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-noise-bgl"),
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

/// Build the Noise Reduction render pipeline for `target_format`.
pub fn create_noise_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "noise",
        NOISE_STAGE_SRC,
        &create_noise_bind_group_layout(device),
        target_format,
    )
}

pub fn create_noise_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-noise-params"),
        size: std::mem::size_of::<NoiseParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_noise_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &NoiseParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

pub fn create_noise_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-noise-bindgroup"),
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
