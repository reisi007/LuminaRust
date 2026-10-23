// ---------------------------------------------------------------------------
// Presence: Dehaze dark channel + apply.
// ---------------------------------------------------------------------------

/// WGSL for the dark-channel pass. Writes `min(R,G,B)` over a radius-2 window
/// (the CPU `apply_presence` dark channel) into the red channel of an RGBA8
/// target; the value is a `u8`, so the readback histogram is exact.
pub const DARK_STAGE_SRC: &str = concat!(
    r#"
@group(0) @binding(0) var input_tex : texture_2d<f32>;
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
  let x0 = max(coord.x - 2, 0);
  let x1 = min(coord.x + 2, dims.x - 1);
  let y0 = max(coord.y - 2, 0);
  let y1 = min(coord.y + 2, dims.y - 1);
  var m : u32 = 255u;
  for (var yy = y0; yy <= y1; yy = yy + 1) {
    for (var xx = x0; xx <= x1; xx = xx + 1) {
      let p = textureLoad(input_tex, texel_coord(vec2<i32>(xx, yy)), 0);
      let r = u32(roundi(p.r * 255.0));
      let g = u32(roundi(p.g * 255.0));
      let b = u32(roundi(p.b * 255.0));
      m = min(m, min(r, min(g, b)));
    }
  }
  return vec4<f32>(f32(m) / 255.0, 0.0, 0.0, 1.0);
}
"#
);

pub fn create_dark_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-dark-bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    })
}

pub fn create_dark_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "dark",
        DARK_STAGE_SRC,
        &create_dark_bind_group_layout(device),
        target_format,
    )
}

pub fn create_dark_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-dark-bindgroup"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(input_view),
        }],
    })
}

/// Uniform block for the Dehaze apply pass (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DehazeParams {
    /// The deterministic airlight `a` (95th percentile of the dark channel).
    pub airlight: f32,
    /// The (signed) recipe dehaze strength.
    pub strength: f32,
    pub _pad: [u32; 2],
}

/// WGSL for the Dehaze apply pass (`apply_presence`'s dehaze block).
pub const DEHAZE_STAGE_SRC: &str = concat!(
    r#"
struct DehazeParams {
  airlight : f32,
  strength : f32,
  pad0 : u32,
  pad1 : u32,
};
@group(0) @binding(0) var<uniform> params : DehazeParams;
@group(0) @binding(1) var color_tex : texture_2d<f32>;
@group(0) @binding(2) var dark_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coords = vec2<u32>(frag_coord.xy);
  let px = textureLoad(color_tex, coords, 0);
  let dark = textureLoad(dark_tex, coords, 0).r;
  let a = params.airlight;
  let base_t = clamp(1.0 - 0.95 * dark / a, 0.05, 1.0);
  var t : f32;
  if (params.strength > 0.0) {
    t = 1.0 - params.strength * (1.0 - base_t);
  } else {
    t = 1.0 + (-params.strength) * 0.5 * (1.0 - base_t);
  }
  var out : vec3<f32>;
  for (var c = 0u; c < 3u; c = c + 1u) {
    let x = px[c];
    out[c] = byte_from_norm((x - a) / t + a);
  }
  return vec4<f32>(norm_from_byte(out.x), norm_from_byte(out.y), norm_from_byte(out.z), px.a);
}
"#
);

pub fn create_dehaze_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-dehaze-bgl"),
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
            wgpu::BindGroupLayoutEntry {
                binding: 2,
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

pub fn create_dehaze_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "dehaze",
        DEHAZE_STAGE_SRC,
        &create_dehaze_bind_group_layout(device),
        target_format,
    )
}

pub fn create_dehaze_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-dehaze-params"),
        size: std::mem::size_of::<DehazeParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_dehaze_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &DehazeParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

pub fn create_dehaze_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    color_view: &wgpu::TextureView,
    dark_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-dehaze-bindgroup"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(color_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(dark_view),
            },
        ],
    })
}

/// Compute the deterministic Dehaze airlight from the GPU dark-channel bytes.
///
/// Mirrors `apply_presence` exactly: sort the dark channel ascending and take
/// index `min((n * 0.95) as usize, n - 1)`, floored at `0.05`. Because every
/// dark sample is `min(R,G,B)` over `u8` channels, the value set is exactly
/// `k/255` for `k in 0..=255`, so a 256-bin counting sort over the bytes is an
/// exact — not approximate — reproduction of the oracle's full sort.
pub fn dehaze_airlight(dark_bytes: &[u8]) -> f32 {
    let n = dark_bytes.len();
    if n == 0 {
        return 0.05;
    }
    let mut histogram = [0usize; 256];
    for &value in dark_bytes {
        histogram[value as usize] += 1;
    }
    let index = ((n as f32 * 0.95) as usize).min(n - 1);
    let mut cumulative = 0usize;
    let mut sorted_value = 255usize;
    for (value, &count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative > index {
            sorted_value = value;
            break;
        }
    }
    (sorted_value as f32 / 255.0).max(0.05)
}
