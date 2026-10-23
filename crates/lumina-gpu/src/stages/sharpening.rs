use bytemuck::Zeroable;

// ---------------------------------------------------------------------------
// Sharpening (F-095): separable Gaussian luminance unsharp mask.
//
// `apply_sharpening` computes, per pixel, `lum - fine` and `lum - coarse` where
// `fine`/`coarse` are separable three-sigma Gaussian blurs of the Rec.709
// luminance. The GPU chain mirrors that in three passes:
//
// 1. a horizontal blur pass per radius, writing the intermediate `tmp` rows
//    into an `R32Float` texture (exact `f32` storage);
// 2. a compute pass reducing the luminance-gradient maximum (`maxg`) into a
//    single `atomicMax` cell, read back once (like Dehaze's airlight — the one
//    host round-trip this stage needs);
// 3. an apply pass doing the vertical blur on the fly and the oracle's ratio.
//
// The Gaussian kernels are precomputed on the host with the oracle's exact
// `exp`/normalization, so only FMA-vs-separate-rounding remains as residual.
// ---------------------------------------------------------------------------

/// Maximum taps per Gaussian kernel: radius 10 (schema max) at `effective_scale
/// = 1.0` (the only scale `render_frame`/`render_with_gpu` use) gives
/// `sigma = 10 * 1.5 = 15`, `r = ceil(3 * sigma) = 45`, i.e. 91 taps.
pub const MAX_SHARPEN_TAPS: usize = 91;

/// Effective output scale the GPU adjustment chain renders at.
///
/// `render_with_gpu`/`render_to_vram` always evaluate the full-frame
/// `render_frame` semantics (no draft/preview scale), which call
/// `apply_recipe_with_scale_and_white_balance(recipe, 1.0, …)`. The
/// radius-sensitive Sharpening stage derives its pixel radii from this scale;
/// it is a named constant so the assumption is explicit and cannot be changed
/// silently (GPU-RENDER-PARITY-1 stage-2 follow-up). A future scaled GPU path
/// must thread the real scale through instead of reusing this constant.
pub const GPU_EFFECTIVE_SCALE: f32 = 1.0;

/// Storage-buffer layout for the sharpening passes. `kernels[0..91]` is the
/// fine kernel, `kernels[91..182]` the coarse one.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SharpenParams {
    pub amount: f32,
    pub detail: f32,
    pub masking: f32,
    pub _pad0: u32,
    pub fine_radius: u32,
    pub coarse_radius: u32,
    pub _pad1: u32,
    pub _pad2: u32,
    /// Fine kernel in `0..MAX_SHARPEN_TAPS`, coarse in `MAX_SHARPEN_TAPS..`.
    pub kernels: [f32; 2 * MAX_SHARPEN_TAPS],
}

impl SharpenParams {
    /// Build from a recipe's `sharpening` at [`GPU_EFFECTIVE_SCALE`] (matching
    /// `render_frame`, which uses scale `1.0`). The caller must only enqueue the
    /// passes when `amount != 0` (the oracle's early return) and must reject a
    /// radius outside the schema range before calling this
    /// ([`super::validate_gpu_recipe`]).
    pub fn from_sharpening(s: &lumina_sidecar::Sharpening) -> Self {
        let mut params = Self::zeroed();
        params.amount = s.amount;
        params.detail = s.detail;
        params.masking = s.masking;
        let scale = GPU_EFFECTIVE_SCALE;
        let (fine_radius, fine) = sharpen_kernel((s.radius * 0.5 * scale).max(0.5));
        let (coarse_radius, coarse) = sharpen_kernel((s.radius * 1.5 * scale).max(0.5));
        params.fine_radius = fine_radius;
        params.coarse_radius = coarse_radius;
        params.kernels[..fine.len()].copy_from_slice(&fine);
        params.kernels[MAX_SHARPEN_TAPS..MAX_SHARPEN_TAPS + coarse.len()].copy_from_slice(&coarse);
        params
    }

    /// Whether the oracle would run the sharpener (`amount != 0`).
    pub fn needs_stage(s: &lumina_sidecar::Sharpening) -> bool {
        s.amount != 0.0
    }
}

/// Normalized Gaussian kernel for `sigma` (`apply_sharpening`'s `blur`):
/// `r = ceil(3*sigma)`, `exp(-k²/(2σ²))` for `k in -r..=r`, normalized by the
/// `f32` sum — operation-for-operation identical to the oracle.
pub(super) fn sharpen_kernel(sigma: f32) -> (u32, Vec<f32>) {
    let r = (sigma * 3.0).ceil() as i32;
    let r = r.clamp(0, (MAX_SHARPEN_TAPS as i32 - 1) / 2);
    let mut kernel = Vec::with_capacity((2 * r + 1) as usize);
    for k in -r..=r {
        kernel.push(((-(k * k) as f32) / (2.0 * sigma * sigma)).exp());
    }
    let z: f32 = kernel.iter().sum();
    for value in &mut kernel {
        *value /= z;
    }
    (r as u32, kernel)
}

/// Tiny uniform selecting the kernel half + radius for one horizontal pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SharpenBlurSelect {
    /// 0 for the fine kernel, [`MAX_SHARPEN_TAPS`] for the coarse one.
    pub base: u32,
    pub radius: u32,
    pub _pad: [u32; 2],
}

/// Uniform carrying the reduced gradient maximum for the apply pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SharpenMaxg {
    pub maxg: f32,
    pub _pad: [u32; 3],
}

/// WGSL for one horizontal Gaussian blur pass of the luminance.
pub const SHARPEN_BLUR_SRC: &str = concat!(
    r#"
struct SharpenParams {
  amount : f32,
  detail : f32,
  masking : f32,
  pad0 : u32,
  fine_radius : u32,
  coarse_radius : u32,
  pad1 : u32,
  pad2 : u32,
  kernels : array<f32, 182>,
};
struct BlurSelect {
  base : u32,
  radius : u32,
  pad0 : u32,
  pad1 : u32,
};
@group(0) @binding(0) var<storage, read> params : SharpenParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
@group(0) @binding(2) var<uniform> blur_sel : BlurSelect;
"#,
    common_src!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) f32 {
  let dims = textureDimensions(input_tex);
  let coord = vec2<i32>(i32(frag_coord.x), i32(frag_coord.y));
  let r = i32(blur_sel.radius);
  var sum : f32 = 0.0;
  for (var k = -r; k <= r; k = k + 1) {
    let xx = clamp(coord.x + k, 0, i32(dims.x) - 1);
    let c = textureLoad(input_tex, vec2<u32>(u32(xx), u32(coord.y)), 0);
    let lum = 0.2126 * roundi(c.r * 255.0)
      + 0.7152 * roundi(c.g * 255.0)
      + 0.0722 * roundi(c.b * 255.0);
    sum = sum + params.kernels[blur_sel.base + u32(k + r)] * lum;
  }
  return sum;
}
"#
);

/// WGSL compute pass reducing the Rec.709 luminance-gradient maximum
/// (`apply_sharpening`'s `maxg`) into an `atomicMax` cell. `grad >= 0`, so the
/// `u32` bit pattern orders identically to the `f32` value.
pub const SHARPEN_GRADIENT_SRC: &str = concat!(
    r#"
@group(0) @binding(0) var input_tex : texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> max_grad : atomic<u32>;
"#,
    common_src!(),
    r#"
fn lum_at(c : vec2<u32>) -> f32 {
  let p = textureLoad(input_tex, c, 0);
  return 0.2126 * roundi(p.r * 255.0)
    + 0.7152 * roundi(p.g * 255.0)
    + 0.0722 * roundi(p.b * 255.0);
}

@compute @workgroup_size(16, 16)
fn cs_main(@builtin(global_invocation_id) gid : vec3<u32>) {
  let dims = textureDimensions(input_tex);
  if (gid.x >= dims.x || gid.y >= dims.y) {
    return;
  }
  let x = i32(gid.x);
  let y = i32(gid.y);
  let w = i32(dims.x);
  let h = i32(dims.y);
  let gx = lum_at(vec2<u32>(u32(min(x + 1, w - 1)), gid.y))
    - lum_at(vec2<u32>(u32(max(x - 1, 0)), gid.y));
  let gy = lum_at(vec2<u32>(gid.x, u32(min(y + 1, h - 1))))
    - lum_at(vec2<u32>(gid.x, u32(max(y - 1, 0))));
  let grad = abs(gx) + abs(gy);
  atomicMax(&max_grad, bitcast<u32>(grad));
}
"#
);

/// WGSL for the sharpening apply pass: vertical blur on the fly, gradient from
/// the source luminance and the oracle's `ratio` application.
pub const SHARPEN_APPLY_SRC: &str = concat!(
    r#"
struct SharpenParams {
  amount : f32,
  detail : f32,
  masking : f32,
  pad0 : u32,
  fine_radius : u32,
  coarse_radius : u32,
  pad1 : u32,
  pad2 : u32,
  kernels : array<f32, 182>,
};
struct SharpenMaxg {
  maxg : f32,
  pad0 : u32,
  pad1 : u32,
  pad2 : u32,
};
const MAX_TAPS : u32 = 91u;
@group(0) @binding(0) var<storage, read> params : SharpenParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
@group(0) @binding(2) var fine_tex : texture_2d<f32>;
@group(0) @binding(3) var coarse_tex : texture_2d<f32>;
@group(0) @binding(4) var<uniform> maxg_params : SharpenMaxg;
"#,
    common_src!(),
    r#"
fn lum_at(c : vec2<u32>) -> f32 {
  let p = textureLoad(input_tex, c, 0);
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
  let coord_u = vec2<u32>(frag_coord.xy);
  let c0 = textureLoad(input_tex, coord_u, 0);
  let lum0 = lum_at(coord_u);
  var fine_v : f32 = 0.0;
  let rf = i32(params.fine_radius);
  for (var k = -rf; k <= rf; k = k + 1) {
    let yy = clamp(coord.y + k, 0, h - 1);
    fine_v = fine_v
      + params.kernels[u32(k + rf)]
        * textureLoad(fine_tex, vec2<u32>(coord_u.x, u32(yy)), 0).r;
  }
  var coarse_v : f32 = 0.0;
  let rc = i32(params.coarse_radius);
  for (var k = -rc; k <= rc; k = k + 1) {
    let yy = clamp(coord.y + k, 0, h - 1);
    coarse_v = coarse_v
      + params.kernels[MAX_TAPS + u32(k + rc)]
        * textureLoad(coarse_tex, vec2<u32>(coord_u.x, u32(yy)), 0).r;
  }
  let gx = lum_at(vec2<u32>(u32(min(coord.x + 1, w - 1)), coord_u.y))
    - lum_at(vec2<u32>(u32(max(coord.x - 1, 0)), coord_u.y));
  let gy = lum_at(vec2<u32>(coord_u.x, u32(min(coord.y + 1, h - 1))))
    - lum_at(vec2<u32>(coord_u.x, u32(max(coord.y - 1, 0))));
  let gradient = abs(gx) + abs(gy);
  var edge : f32 = 0.0;
  if (maxg_params.maxg > 0.0) {
    edge = clamp(gradient / maxg_params.maxg, 0.0, 1.0);
  }
  let amount = params.amount
    * ((1.0 - params.masking) + params.masking * edge);
  let d = params.detail * (lum0 - fine_v)
    + (1.0 - params.detail) * (lum0 - coarse_v);
  let ny = clamp(lum0 + amount * d, 0.0, 255.0);
  var ratio : f32 = 0.0;
  if (lum0 > 1e-6) {
    ratio = ny / lum0;
  }
  return vec4<f32>(
    byte255(roundi(c0.r * 255.0) * ratio),
    byte255(roundi(c0.g * 255.0) * ratio),
    byte255(roundi(c0.b * 255.0) * ratio),
    c0.a
  );
}
"#
);

/// Scalar (`R32Float`) intermediate format for the separable blur passes.
pub const SHARPEN_SCALAR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// Create an `R32Float` render/read texture for a blur intermediate.
pub fn create_sharpen_scalar_texture(
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
        format: SHARPEN_SCALAR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    })
}

/// Bind group layout for [`SHARPEN_BLUR_SRC`]: storage (0) + input (1) + uniform (2).
pub fn create_sharpen_blur_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-sharpen-blur-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
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
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

/// Bind group layout for [`SHARPEN_APPLY_SRC`].
pub fn create_sharpen_apply_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-sharpen-apply-bgl"),
        entries: &[
            storage_entry(0, true),
            texture_entry(1, true),
            texture_entry(2, false),
            texture_entry(3, false),
            uniform_entry(4),
        ],
    })
}

/// Bind group layout for [`SHARPEN_GRADIENT_SRC`] (compute): input (0) +
/// read-write atomic storage (1).
pub fn create_sharpen_gradient_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-sharpen-gradient-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub fn create_sharpen_blur_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "sharpen-blur",
        SHARPEN_BLUR_SRC,
        &create_sharpen_blur_bind_group_layout(device),
        target_format,
    )
}

pub fn create_sharpen_apply_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
    super::build_simple_pipeline(
        device,
        "sharpen-apply",
        SHARPEN_APPLY_SRC,
        &create_sharpen_apply_bind_group_layout(device),
        target_format,
    )
}

/// Build the sharpening gradient-reduction compute pipeline.
pub fn create_sharpen_gradient_pipeline(
    device: &wgpu::Device,
) -> Result<wgpu::ComputePipeline, crate::GpuError> {
    let layout = create_sharpen_gradient_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-sharpen-gradient-pl"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-sharpen-gradient-shader"),
        source: wgpu::ShaderSource::Wgsl(SHARPEN_GRADIENT_SRC.into()),
    });
    Ok(
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("lumina-gpu-sharpen-gradient-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("cs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }),
    )
}

pub fn create_sharpen_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-sharpen-params"),
        size: std::mem::size_of::<SharpenParams>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_sharpen_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &SharpenParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

pub fn create_sharpen_blur_select_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-sharpen-blur-select"),
        size: std::mem::size_of::<SharpenBlurSelect>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_sharpen_blur_select(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    select: &SharpenBlurSelect,
) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(select));
}

pub fn create_sharpen_maxg_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-sharpen-maxg"),
        size: std::mem::size_of::<SharpenMaxg>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn write_sharpen_maxg(queue: &wgpu::Queue, buffer: &wgpu::Buffer, maxg: f32) {
    let params = SharpenMaxg { maxg, _pad: [0; 3] };
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(&params));
}

/// Create the 4-byte read-write storage cell the gradient reduction
/// `atomicMax`es into. `COPY_SRC` lets the caller copy it into a map buffer.
pub fn create_sharpen_gradient_cell(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-sharpen-max-cell"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

pub fn create_sharpen_blur_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
    select_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-sharpen-blur-bindgroup"),
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
            wgpu::BindGroupEntry {
                binding: 2,
                resource: select_buffer.as_entire_binding(),
            },
        ],
    })
}

pub fn create_sharpen_apply_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
    fine_view: &wgpu::TextureView,
    coarse_view: &wgpu::TextureView,
    maxg_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-sharpen-apply-bindgroup"),
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
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(fine_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(coarse_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: maxg_buffer.as_entire_binding(),
            },
        ],
    })
}

pub fn create_sharpen_gradient_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    input_view: &wgpu::TextureView,
    max_cell: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-sharpen-gradient-bindgroup"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(input_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: max_cell.as_entire_binding(),
            },
        ],
    })
}
