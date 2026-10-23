//! The vignette effect stage.

// ---------------------------------------------------------------------------
// Effects: vignette (F-097).
// ---------------------------------------------------------------------------

/// `apply_vignette` uniform block. The (pixel-independent) radial constants are
/// derived on the host with the *same* `f32` arithmetic as the oracle's first
/// pass, so the shader's per-pixel normalized radius is bit-identical and no
/// global GPU reduction/readback is needed.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VignetteParams {
    pub amount: f32,
    pub midpoint: f32,
    /// `0.15 + feather * 0.7`.
    pub feather_width: f32,
    /// Minimum normalized radius over the frame (`apply_vignette`'s `r_min`).
    pub r_min: f32,
    /// `max(r_max - r_min, 1e-6)`.
    pub denom: f32,
    /// `max(1 - midpoint, 1e-6)`.
    pub t_denom: f32,
    pub cx: f32,
    pub cy: f32,
    pub half_w: f32,
    pub half_h: f32,
    pub ry_scale: f32,
    pub _pad: u32,
}

impl VignetteParams {
    /// Build from a recipe's `effects.vignette` for a `width`×`height` frame.
    /// The caller must only enqueue the pass when `amount != 0` (the oracle's
    /// early return).
    pub fn from_vignette(v: &lumina_sidecar::Vignette, width: u32, height: u32) -> Self {
        let (r_min, r_max) = vignette_radius_bounds(width, height, v.roundness);
        Self {
            amount: v.amount,
            midpoint: v.midpoint,
            feather_width: 0.15 + v.feather * 0.7,
            r_min,
            denom: (r_max - r_min).max(1e-6),
            t_denom: (1.0 - v.midpoint).max(1e-6),
            cx: (width - 1) as f32 / 2.0,
            cy: (height - 1) as f32 / 2.0,
            half_w: ((width - 1) as f32 / 2.0).max(1.0),
            half_h: ((height - 1) as f32 / 2.0).max(1.0),
            ry_scale: 1.0 + (1.0 - v.roundness) * 0.5,
            _pad: 0,
        }
    }
}

/// The oracle's `r_min`/`r_max` over the per-pixel normalized radius, computed
/// with the exact same `f32` operations as `apply_vignette`'s first pass.
pub fn vignette_radius_bounds(width: u32, height: u32, roundness: f32) -> (f32, f32) {
    let w = width as usize;
    let h = height as usize;
    let cx = (width - 1) as f32 / 2.0;
    let cy = (height - 1) as f32 / 2.0;
    let half_w = ((width - 1) as f32 / 2.0).max(1.0);
    let half_h = ((height - 1) as f32 / 2.0).max(1.0);
    let ry_scale = 1.0 + (1.0 - roundness) * 0.5;
    let mut r_min = f32::MAX;
    let mut r_max = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - cx) / half_w;
            let dy = (y as f32 - cy) / half_h * ry_scale;
            let r = (dx * dx + dy * dy).sqrt();
            r_min = r_min.min(r);
            r_max = r_max.max(r);
        }
    }
    (r_min, r_max)
}

/// WGSL for the vignette pass (`apply_vignette`). RGB only; alpha untouched.
pub const VIGNETTE_STAGE_SRC: &str = concat!(
    r#"
struct VignetteParams {
  amount : f32,
  midpoint : f32,
  feather_width : f32,
  r_min : f32,
  denom : f32,
  t_denom : f32,
  cx : f32,
  cy : f32,
  half_w : f32,
  half_h : f32,
  ry_scale : f32,
  pad0 : u32,
};
@group(0) @binding(0) var<uniform> params : VignetteParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag_coord.xy);
  let src = textureLoad(input_tex, coord, 0);
  let dx = (f32(coord.x) - params.cx) / params.half_w;
  let dy = (f32(coord.y) - params.cy) / params.half_h * params.ry_scale;
  let r = sqrt(dx * dx + dy * dy);
  let rn = (r - params.r_min) / params.denom;
  let t = clamp((rn - params.midpoint) / params.t_denom, 0.0, 1.0);
  let edge0 = 0.5 - params.feather_width / 2.0;
  let edge1 = 0.5 + params.feather_width / 2.0;
  let s = clamp((t - edge0) / (edge1 - edge0), 0.0, 1.0);
  let falloff = s * s * (3.0 - 2.0 * s);
  let factor = 1.0 - params.amount * falloff;
  return vec4<f32>(
    clamp(roundi(roundi(src.r * 255.0) * factor), 0.0, 255.0) / 255.0,
    clamp(roundi(roundi(src.g * 255.0) * factor), 0.0, 255.0) / 255.0,
    clamp(roundi(roundi(src.b * 255.0) * factor), 0.0, 255.0) / 255.0,
    src.a
  );
}
"#
);

pub fn create_vignette_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-vignette-bgl"),
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

pub fn create_vignette_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "vignette",
        VIGNETTE_STAGE_SRC,
        &create_vignette_bind_group_layout(device),
        target_format,
    )
}

pub fn create_vignette_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-vignette-params"),
        size: std::mem::size_of::<VignetteParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_vignette_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &VignetteParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

pub fn create_vignette_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-vignette-bindgroup"),
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
