//! GPU-RENDER-MASK-19: WGSL and pipeline scaffolding for the GPU local-mask
//! pixel pass (split from [`super::local_mask_pass`] for the file-size ratchet,
//! DoD §8). Only compiled with the `gpu` feature.
//!
//! Two fullscreen passes, both byte-oriented: they read `Rgba8Unorm` texels back
//! to their exact `u8` code (`floor(v * 255 + 0.5)`), run integer/`f32` math, and
//! write the code back through `f32(code) / 255.0`.
//!
//! * [`LOCAL_MASK_BLEND_SRC`] is the literal WGSL transcription of the CPU
//!   oracle's `lumina-core::render::local_adjustments::blend_local_layer`
//!   fractional blend (the `u16` alpha intersection with the integer rounding
//!   constant `u16::MAX / 2 = 32767`). The `alpha == 0` and `alpha == u16::MAX`
//!   endpoints are short-circuited exactly like the oracle: `0` leaves the
//!   current pixel untouched, `u16::MAX` copies the adjusted RGB and keeps the
//!   current alpha.
//! * [`LOCAL_MASK_WB_SRC`] applies one mask-local recipe's **relative white
//!   balance delta plus the four P0 Basic controls**, mirroring
//!   `lumina-core::render::local_wb::scale_mask_local_wb_basic` with its single
//!   quantization boundary. It is only used when the layer carries a WB delta;
//!   the delta-free P0 path reuses the already parity-tested global tone
//!   pipeline with the layer's `as_recipe()` (see [`super::local_mask_pass`]).

/// The mask-blend pass: `current` × `adjusted` by the `R16Uint` alpha plane.
pub const LOCAL_MASK_BLEND_SRC: &str = r#"
struct VsOut {
  @builtin(position) pos : vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid : u32) -> VsOut {
  var p = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0)
  );
  var out : VsOut;
  out.pos = vec4<f32>(p[vid], 0.0, 1.0);
  return out;
}

@group(0) @binding(0) var current_tex : texture_2d<f32>;
@group(0) @binding(1) var adjusted_tex : texture_2d<f32>;
@group(0) @binding(2) var mask_tex : texture_2d<u32>;

// Exact byte code of an 8-bit unorm texel: floor(v * 255 + 0.5) recovers the
// stored integer (matching the Rust `f64::round` tie-away convention the CPU
// oracle uses, for the non-negative values this kernel produces).
fn unorm_code(v : f32) -> u32 {
  return u32(floor(v * 255.0 + 0.5));
}

// `blend_local_layer` fractional branch for one channel, in the u16 integer
// domain: (cur * (MAX - a) + adj * a + MAX/2) / MAX with MAX = 65535 and the
// rounding constant 32767.
fn blend_channel(cur : f32, adj : f32, alpha : u32, inv : u32) -> f32 {
  let c = unorm_code(cur);
  let a = unorm_code(adj);
  let v = (c * inv + a * alpha + 32767u) / 65535u;
  return f32(v) / 255.0;
}

@fragment
fn fs_main(@builtin(position) pos : vec4<f32>) -> @location(0) vec4<f32> {
  let coords = vec2<u32>(pos.xy);
  let cur = textureLoad(current_tex, coords, 0);
  let alpha = textureLoad(mask_tex, coords, 0).r;
  if (alpha == 0u) {
    return cur;
  }
  let adj = textureLoad(adjusted_tex, coords, 0);
  if (alpha == 65535u) {
    return vec4<f32>(adj.rgb, cur.a);
  }
  let inv = 65535u - alpha;
  return vec4<f32>(
    blend_channel(cur.r, adj.r, alpha, inv),
    blend_channel(cur.g, adj.g, alpha, inv),
    blend_channel(cur.b, adj.b, alpha, inv),
    cur.a,
  );
}
"#;

/// The local relative-WB + P0 Basic pass (one quantization boundary).
pub const LOCAL_MASK_WB_SRC: &str = r#"
struct WbParams {
  gains : vec4<f32>,
  exposure : f32,
  contrast : f32,
  shadows : f32,
  highlights : f32,
  _pad : vec4<f32>,
};

@group(0) @binding(0) var<uniform> params : WbParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;

struct VsOut {
  @builtin(position) pos : vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid : u32) -> VsOut {
  var p = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0)
  );
  var out : VsOut;
  out.pos = vec4<f32>(p[vid], 0.0, 1.0);
  return out;
}

// `lumina-core::render::local_wb::scale_mask_local_wb_basic` for one channel,
// in the same order (relative gain -> exposure -> contrast -> shadows ->
// highlights) with no intermediate quantization, then one round to `u8`.
fn wb_channel(in_norm : f32, gain : f32) -> f32 {
  var value = floor(in_norm * 255.0 + 0.5) * gain;
  value = clamp(value * exp2(params.exposure), 0.0, 255.0);
  value = clamp((value - 128.0) * (1.0 + params.contrast) + 128.0, 0.0, 255.0);
  var x = value / 255.0;
  let shadow_weight = pow(max((0.5 - x) / 0.5, 0.0), 2.0);
  value = clamp(x + params.shadows * shadow_weight * 0.25, 0.0, 1.0) * 255.0;
  x = value / 255.0;
  let highlight_weight = pow(max((x - 0.5) / 0.5, 0.0), 2.0);
  value = clamp(x + params.highlights * highlight_weight * 0.25, 0.0, 1.0) * 255.0;
  return floor(value + 0.5) / 255.0;
}

@fragment
fn fs_main(@builtin(position) pos : vec4<f32>) -> @location(0) vec4<f32> {
  let coords = vec2<u32>(pos.xy);
  let src = textureLoad(input_tex, coords, 0);
  return vec4<f32>(
    wb_channel(src.r, params.gains.x),
    wb_channel(src.g, params.gains.y),
    wb_channel(src.b, params.gains.z),
    src.a,
  );
}
"#;

use lumina_sidecar::LocalAdjustments;

use crate::shaders;

/// Uniform block for [`LOCAL_MASK_WB_SRC`] (48 bytes: `vec4` + 4×`f32` + pad).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct WbUniforms {
    gains: [f32; 4],
    exposure: f32,
    contrast: f32,
    shadows: f32,
    highlights: f32,
    _pad: [f32; 4],
}

impl WbUniforms {
    pub(crate) fn from_adjustments(adjustments: &LocalAdjustments) -> Self {
        let [r, g, b] = adjustments.relative_white_balance_gains();
        Self {
            gains: [r as f32, g as f32, b as f32, 0.0],
            exposure: adjustments.exposure as f32,
            contrast: adjustments.contrast as f32,
            shadows: adjustments.shadows as f32,
            highlights: adjustments.highlights as f32,
            _pad: [0.0; 4],
        }
    }
}

/// The two local-mask pipelines plus the WB uniform buffer.
pub(crate) struct LocalMaskPipeline {
    pub(crate) blend: wgpu::RenderPipeline,
    pub(crate) blend_layout: wgpu::BindGroupLayout,
    pub(crate) wb: wgpu::RenderPipeline,
    pub(crate) wb_layout: wgpu::BindGroupLayout,
    pub(crate) wb_uniforms: wgpu::Buffer,
}

/// Build the blend + WB pipelines and the WB uniform buffer for `device`.
pub(crate) fn build_local_mask_pipeline(device: &wgpu::Device) -> LocalMaskPipeline {
    let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-localmask-blend-bgl"),
        entries: &[texture_entry(0), texture_entry(1), uint_texture_entry(2)],
    });
    let wb_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-localmask-wb-bgl"),
        entries: &[uniform_entry(0), texture_entry(1)],
    });
    LocalMaskPipeline {
        blend: build_pipeline(
            device,
            "lumina-gpu-localmask-blend",
            LOCAL_MASK_BLEND_SRC,
            &blend_layout,
        ),
        blend_layout,
        wb: build_pipeline(
            device,
            "lumina-gpu-localmask-wb",
            LOCAL_MASK_WB_SRC,
            &wb_layout,
        ),
        wb_layout,
        wb_uniforms: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumina-gpu-localmask-wb-uniforms"),
            size: std::mem::size_of::<WbUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
    }
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

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn uint_texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    label: &str,
    src: &str,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: shaders::RGBA8_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
