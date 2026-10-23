// ---------------------------------------------------------------------------
// Effects: grain (F-097).
// ---------------------------------------------------------------------------

/// `apply_grain` uniform block. `seed32` and `cell` are precomputed on the host
/// with the oracle's exact integer arithmetic (`grain_hash` over the u64 seed
/// folded with the frame dimensions), so the shader only runs the per-cell hash.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GrainParams {
    /// `grain_hash(seed_state as u32)` after folding `seed`/`width`/`height`.
    pub seed32: u32,
    /// Spatial cell size in pixels (`1 + round(size * 7)`, min 1).
    pub cell: u32,
    pub amount: f32,
    pub roughness: f32,
}

impl GrainParams {
    /// Build from a recipe's `effects.grain` for a `width`×`height` frame. The
    /// caller must only enqueue the pass when `amount != 0` (early return).
    pub fn from_grain(g: &lumina_sidecar::Grain, width: u32, height: u32) -> Self {
        Self {
            seed32: grain_effective_seed(g.seed, width, height),
            cell: (1 + (g.size * 7.0).round() as usize).max(1) as u32,
            amount: g.amount,
            roughness: g.roughness,
        }
    }
}

/// `grain_hash` (`lumina-core`) ported to the host: the dimension-aware seed
/// folding. Kept identical so the GPU grain is deterministic and seed-exact.
pub(super) fn grain_effective_seed(seed: u64, width: u32, height: u32) -> u32 {
    fn grain_hash(mut z: u32) -> u32 {
        z = z.wrapping_add(0x9e37_79b9);
        z = (z ^ (z >> 16)).wrapping_mul(0x85eb_ca6b);
        z = (z ^ (z >> 13)).wrapping_mul(0xc2b2_ae35);
        z ^= z >> 16;
        z
    }
    let mut seed_state = seed;
    seed_state = seed_state.wrapping_add((width as u64) << 32);
    seed_state = seed_state.wrapping_add(height as u64);
    seed_state ^= seed_state >> 32;
    seed_state = seed_state.wrapping_mul(0x9e37_79b9);
    grain_hash(seed_state as u32)
}

/// WGSL for the grain pass (`apply_grain`). The `grain_hash` integer sequence is
/// ported operation-for-operation onto wrapping `u32` arithmetic, so the delta
/// is exact and the SAME value is added to R/G/B (channel-coupled).
pub const GRAIN_STAGE_SRC: &str = concat!(
    r#"
struct GrainParams {
  seed32 : u32,
  cell : u32,
  amount : f32,
  roughness : f32,
};
@group(0) @binding(0) var<uniform> params : GrainParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
fn grain_hash(z_in : u32) -> u32 {
  var z = z_in;
  z = z + 0x9e3779b9u;
  z = (z ^ (z >> 16u)) * 0x85ebca6bu;
  z = (z ^ (z >> 13u)) * 0xc2b2ae35u;
  z = z ^ (z >> 16u);
  return z;
}

fn grain_noise(cx : u32, cy : u32, seed : u32) -> f32 {
  let n = grain_hash(cx + seed) ^ grain_hash(cy * 0x85ebca6bu);
  return (f32(grain_hash(n)) / f32(0xFFFFFFFFu)) * 2.0 - 1.0;
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag_coord.xy);
  let cx = coord.x / params.cell;
  let cy = coord.y / params.cell;
  let raw = grain_noise(cx, cy, params.seed32);
  var sum : f32 = 0.0;
  for (var dy = -1; dy <= 1; dy = dy + 1) {
    for (var dx = -1; dx <= 1; dx = dx + 1) {
      let ncx = u32(max(i32(cx) + dx, 0));
      let ncy = u32(max(i32(cy) + dy, 0));
      sum = sum + grain_noise(ncx, ncy, params.seed32);
    }
  }
  let low = sum / 9.0;
  // Separate the multiply/add so a contracted FMA cannot shift `value` by one
  // ulp and flip a `round()` tie in the delta below (the oracle does not FMA).
  let low_weight = 1.0 - params.roughness;
  let low_term = low * low_weight;
  let raw_term = raw * params.roughness;
  let value = low_term + raw_term;
  let delta = i32(roundi(value * params.amount * 40.0));
  let src = textureLoad(input_tex, coord, 0);
  return vec4<f32>(
    f32(clamp(i32(roundi(src.r * 255.0)) + delta, 0i, 255i)) / 255.0,
    f32(clamp(i32(roundi(src.g * 255.0)) + delta, 0i, 255i)) / 255.0,
    f32(clamp(i32(roundi(src.b * 255.0)) + delta, 0i, 255i)) / 255.0,
    src.a
  );
}
"#
);

pub fn create_grain_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-grain-bgl"),
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

pub fn create_grain_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "grain",
        GRAIN_STAGE_SRC,
        &create_grain_bind_group_layout(device),
        target_format,
    )
}

pub fn create_grain_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-grain-params"),
        size: std::mem::size_of::<GrainParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_grain_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &GrainParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

pub fn create_grain_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-grain-bindgroup"),
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
