//! GPU geometry stages (GPU-RENDER-PARITY-1, geometry wave): lens correction
//! (manual distortion/vignette + CA), perspective (homography) and the
//! crop/rotation/mirror stage, ported from the `lumina-core` CPU oracle.
//!
//! The CPU reference applies the geometry sub-stages in this exact order
//! (`ImageFrame::apply_geometry` / `render_frame_from_base`):
//!
//! ```text
//! lens distortion+vignette → perspective → CA → crop → rotation → mirror
//! ```
//!
//! Each sub-stage is a self-contained fullscreen fragment pass that mirrors the
//! oracle's per-pixel byte math and quantizes with the shared `roundi` helper
//! before the next pass reads the RGBA8 result. The pass chain is planned on the
//! host by [`GeometryPlan::from_recipe`], which ports the oracle's dimension
//! math (`perspective_dimensions`, `crop_rect`, `rotate_dimensions`) so the GPU
//! output dimensions are **exactly** the CPU oracle's output dimensions.
//!
//! Sampling replicates the oracle's `sample` helper: manual bilinear with the
//! `x1/y1` neighbour clamped to the frame edge and out-of-bounds coordinates
//! yielding transparent black, evaluated in the `0..=255` byte domain. Quarter
//! turns, crop and mirroring are exact integer copies (no resampling), matching
//! the oracle's special cases.
//!
//! **Dimension-changing output.** [`GeometryPlan`] carries the planned output
//! dimensions; `render_with_gpu` reads the final texture back at those
//! dimensions. The readback-free VRAM present path only carries geometry whose
//! output dimensions equal the source dimensions (lens, identity crop/rotation)
//! and refuses dimension-changing geometry loudly — the caller (GUI) then uses
//! the exact CPU present path. There is no silent path.

use lumina_core::{CoreError, MemoryBudget};
use lumina_sidecar::{AspectPreset, Crop, LensCorrection, Perspective};

use crate::shaders;
use crate::stages::build_simple_pipeline;
use crate::GpuError;

fn invalid(name: &str, value: f64, minimum: f64, maximum: f64) -> GpuError {
    GpuError::Core(CoreError::InvalidAdjustment {
        name: name.into(),
        value,
        minimum,
        maximum,
    })
}

// ---------------------------------------------------------------------------
// Shared WGSL helpers (inlined into every geometry shader below)
// ---------------------------------------------------------------------------

macro_rules! geom_common {
    () => {
        r#"
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

// Round half away from zero, matching Rust's `f32::round` exactly.
fn roundi(x : f32) -> f32 {
  let t = trunc(x);
  let f = x - t;
  if (abs(f) >= 0.5) {
    return t + sign(f);
  }
  return t;
}

// Oracle `sample` (on one channel): bounds check + bilinear in the 0..=255
// byte domain, with the far neighbour clamped to the frame edge.
fn bilinear255(tex : texture_2d<f32>, p : vec2<f32>) -> vec4<f32> {
  let dims = textureDimensions(tex);
  let dw = f32(dims.x);
  let dh = f32(dims.y);
  if (p.x < 0.0 || p.y < 0.0 || p.x >= dw || p.y >= dh) {
    return vec4<f32>(0.0);
  }
  let x0 = i32(floor(p.x));
  let y0 = i32(floor(p.y));
  let x1 = min(x0 + 1, i32(dims.x) - 1);
  let y1 = min(y0 + 1, i32(dims.y) - 1);
  let fx = p.x - f32(x0);
  let fy = p.y - f32(y0);
  let a = textureLoad(tex, vec2<i32>(x0, y0), 0) * 255.0;
  let b = textureLoad(tex, vec2<i32>(x1, y0), 0) * 255.0;
  let c = textureLoad(tex, vec2<i32>(x0, y1), 0) * 255.0;
  let d = textureLoad(tex, vec2<i32>(x1, y1), 0) * 255.0;
  return (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy;
}

fn byte1(v : f32) -> f32 {
  return roundi(clamp(v, 0.0, 255.0)) / 255.0;
}

fn out_bytes(v : vec4<f32>) -> vec4<f32> {
  return vec4<f32>(byte1(v.r), byte1(v.g), byte1(v.b), byte1(v.a));
}
"#
    };
}

// ---------------------------------------------------------------------------
// Lens correction (manual model): distortion + vignette (F-098)
// ---------------------------------------------------------------------------

/// Uniform block for the manual lens pass. `c45.zw` carry the distortion
/// centre; `setup.x` carries `diag = sqrt(w² + h²) / 2`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LensParams {
    pub c0123: [f32; 4],
    pub c45: [f32; 4],
    pub setup: [f32; 4],
}

impl LensParams {
    fn new(c: &[f32; 8], width: u32, height: u32) -> Self {
        let (w, h) = (width as f32, height as f32);
        let diag = (w * w + h * h).sqrt() / 2.0;
        Self {
            c0123: [c[0], c[1], c[2], c[3]],
            c45: [c[4], c[5], (w - 1.0) / 2.0, (h - 1.0) / 2.0],
            setup: [diag, 0.0, 0.0, 0.0],
        }
    }
}

/// WGSL for the manual lens pass — a direct port of `lumina-core::apply_lens`.
pub(crate) const LENS_STAGE_SRC: &str = concat!(
    r#"
struct LensParams {
  c0123 : vec4<f32>,
  c45 : vec4<f32>,
  setup : vec4<f32>,
};
@group(0) @binding(0) var<uniform> params : LensParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    geom_common!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let c0 = params.c0123.x;
  let c1 = params.c0123.y;
  let c2 = params.c0123.z;
  let c3 = params.c0123.w;
  let c4 = params.c45.x;
  let c5 = params.c45.y;
  let cx = params.c45.z;
  let cy = params.c45.w;
  let diag = params.setup.x;
  let nx = (f32(coord.x) - cx) / diag;
  let ny = (f32(coord.y) - cy) / diag;
  let tgt = sqrt(nx * nx + ny * ny);
  var r = tgt;
  for (var i = 0; i < 8; i = i + 1) {
    let r2 = r * r;
    let r4 = r2 * r2;
    let r6 = r4 * r2;
    let f = r * (1.0 + c0 * r2 + c1 * r4 + c2 * r6) - tgt;
    let d = 1.0 + 3.0 * c0 * r2 + 5.0 * c1 * r4 + 7.0 * c2 * r6;
    r = max(r - f / d, 0.0);
  }
  var q = 1.0;
  if (tgt > 1e-6) {
    q = r / tgt;
  }
  let sx = cx + nx * q * diag;
  let sy = cy + ny * q * diag;
  let t2 = tgt * tgt;
  let t4 = t2 * t2;
  let vig = max(c3 + c4 * t2 + c5 * t4, 0.01);
  let s = bilinear255(input_tex, vec2<f32>(sx, sy));
  let rgb = s.rgb * vig;
  return out_bytes(vec4<f32>(rgb.r, rgb.g, rgb.b, s.a));
}
"#
);

// ---------------------------------------------------------------------------
// Perspective (F-099): inverse homography resampling
// ---------------------------------------------------------------------------

/// Uniform block for the perspective pass. The shader re-derives the oracle's
/// formal inverse from the forward matrix `m` (rows in `row0..row2`), the
/// projected-corner bounds (`ranges`) and the source/output dimensions.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PerspectiveParams {
    pub row0: [f32; 4],
    pub row1: [f32; 4],
    pub row2: [f32; 4],
    /// `[min.x, min.y, range.x, range.y]`.
    pub ranges: [f32; 4],
    /// `[src_w, src_h, out_w, out_h]`.
    pub dims: [f32; 4],
}

impl PerspectiveParams {
    fn new(
        m: [[f32; 3]; 3],
        min: [f32; 2],
        max: [f32; 2],
        src_width: u32,
        src_height: u32,
        out_width: u32,
        out_height: u32,
    ) -> Self {
        Self {
            row0: [m[0][0], m[0][1], m[0][2], 0.0],
            row1: [m[1][0], m[1][1], m[1][2], 0.0],
            row2: [m[2][0], m[2][1], m[2][2], 0.0],
            ranges: [min[0], min[1], max[0] - min[0], max[1] - min[1]],
            dims: [
                src_width as f32,
                src_height as f32,
                out_width as f32,
                out_height as f32,
            ],
        }
    }
}

/// WGSL for the perspective pass — a direct port of `lumina-core::apply_perspective`.
pub(crate) const PERSPECTIVE_STAGE_SRC: &str = concat!(
    r#"
struct PerspectiveParams {
  row0 : vec4<f32>,
  row1 : vec4<f32>,
  row2 : vec4<f32>,
  ranges : vec4<f32>,
  dims : vec4<f32>,
};
@group(0) @binding(0) var<uniform> params : PerspectiveParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    geom_common!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let m00 = params.row0.x;
  let m01 = params.row0.y;
  let m02 = params.row0.z;
  let m10 = params.row1.x;
  let m11 = params.row1.y;
  let m12 = params.row1.z;
  let m20 = params.row2.x;
  let m21 = params.row2.y;
  let m22 = params.row2.z;
  let range_x = params.ranges.z;
  let range_y = params.ranges.w;
  let ow = u32(params.dims.z);
  let oh = u32(params.dims.w);
  let denom_x = f32(max(ow, 2u) - 1u);
  let denom_y = f32(max(oh, 2u) - 1u);
  let nx = -range_x / 2.0 + (f32(coord.x) / denom_x) * range_x;
  let ny = -range_y / 2.0 + (f32(coord.y) / denom_y) * range_y;
  let det = m00 * (m11 * m22 - m12 * m21)
          - m01 * (m10 * m22 - m12 * m20)
          + m02 * (m10 * m21 - m11 * m20);
  if (!(abs(det) >= 1e-6)) {
    return vec4<f32>(0.0);
  }
  let a = (m11 * m22 - m12 * m21) * nx
        + (m02 * m21 - m01 * m22) * ny
        + (m01 * m12 - m02 * m11);
  let b = (m12 * m20 - m10 * m22) * nx
        + (m00 * m22 - m02 * m20) * ny
        + (m02 * m10 - m00 * m12);
  let d = (m10 * m21 - m11 * m20) * nx
        + (m01 * m20 - m00 * m21) * ny
        + (m00 * m11 - m01 * m10);
  let ad = a / det;
  let bd = b / det;
  let dd = d / det;
  if (!(abs(dd) >= 1e-6)) {
    return vec4<f32>(0.0);
  }
  let sx = ad / dd;
  let sy = bd / dd;
  let src_w = params.dims.x;
  let src_h = params.dims.y;
  let px = (sx / 2.0 + 0.5) * (src_w - 1.0);
  let py = (sy / 2.0 + 0.5) * (src_h - 1.0);
  let s = bilinear255(input_tex, vec2<f32>(px, py));
  return out_bytes(s);
}
"#
);

// ---------------------------------------------------------------------------
// Chromatic aberration (manual model): R/B channel rescale (F-098)
// ---------------------------------------------------------------------------

/// Uniform block for the CA pass: `[ca_red, ca_blue, cx, cy]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CaParams {
    pub ca: [f32; 4],
}

impl CaParams {
    fn new(c: &[f32; 8], width: u32, height: u32) -> Self {
        Self {
            ca: [
                c[6],
                c[7],
                (width as f32 - 1.0) / 2.0,
                (height as f32 - 1.0) / 2.0,
            ],
        }
    }
}

/// WGSL for the CA pass — a direct port of `lumina-core::apply_ca`.
pub(crate) const CA_STAGE_SRC: &str = concat!(
    r#"
struct CaParams {
  ca : vec4<f32>,
};
@group(0) @binding(0) var<uniform> params : CaParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    geom_common!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let base = textureLoad(input_tex, vec2<i32>(i32(coord.x), i32(coord.y)), 0);
  let cx = params.ca.z;
  let cy = params.ca.w;
  let x = f32(coord.x);
  let y = f32(coord.y);
  let kr = 1.0 + params.ca.x;
  let kb = 1.0 + params.ca.y;
  let sr = bilinear255(input_tex, vec2<f32>(cx + (x - cx) * kr, cy + (y - cy) * kr));
  let sb = bilinear255(input_tex, vec2<f32>(cx + (x - cx) * kb, cy + (y - cy) * kb));
  return vec4<f32>(byte1(sr.r), base.g, byte1(sb.b), base.a);
}
"#
);

// ---------------------------------------------------------------------------
// Crop: exact sub-rectangle copy
// ---------------------------------------------------------------------------

/// Uniform block for the crop pass: `[offset_x, offset_y, 0, 0]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CropParams {
    pub offset: [u32; 4],
}

/// WGSL for the crop pass — a direct port of `lumina-core::crop_frame`.
pub(crate) const CROP_STAGE_SRC: &str = concat!(
    r#"
struct CropParams {
  offset : vec4<u32>,
};
@group(0) @binding(0) var<uniform> params : CropParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    geom_common!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let sx = i32(coord.x) + i32(params.offset.x);
  let sy = i32(coord.y) + i32(params.offset.y);
  return textureLoad(input_tex, vec2<i32>(sx, sy), 0);
}
"#
);

// ---------------------------------------------------------------------------
// Rotation: exact quarter turns + general bilinear
// ---------------------------------------------------------------------------

/// Uniform block for the rotation pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RotateParams {
    /// `[mode (0 = quarter turn, 1 = general), turn, src_w, src_h]`.
    pub mode_turn: [u32; 4],
    /// `[out_w, out_h, 0, 0]`.
    pub dims: [u32; 4],
    /// `[cos, sin, src_cx, src_cy]` (general mode).
    pub trig: [f32; 4],
    /// `[out_cx, out_cy, 0, 0]` (general mode).
    pub centers: [f32; 4],
}

impl RotateParams {
    fn quarter(
        turn: u32,
        src_width: u32,
        src_height: u32,
        out_width: u32,
        out_height: u32,
    ) -> Self {
        Self {
            mode_turn: [0, turn, src_width, src_height],
            dims: [out_width, out_height, 0, 0],
            trig: [0.0; 4],
            centers: [0.0; 4],
        }
    }

    fn general(
        cos: f32,
        sin: f32,
        src_width: u32,
        src_height: u32,
        out_width: u32,
        out_height: u32,
    ) -> Self {
        Self {
            mode_turn: [1, 0, src_width, src_height],
            dims: [out_width, out_height, 0, 0],
            trig: [
                cos,
                sin,
                (src_width as f32 - 1.0) / 2.0,
                (src_height as f32 - 1.0) / 2.0,
            ],
            centers: [
                (out_width as f32 - 1.0) / 2.0,
                (out_height as f32 - 1.0) / 2.0,
                0.0,
                0.0,
            ],
        }
    }
}

/// WGSL for the rotation pass — a direct port of `lumina-core::rotate_frame`.
pub(crate) const ROTATE_STAGE_SRC: &str = concat!(
    r#"
struct RotateParams {
  mode_turn : vec4<u32>,
  dims : vec4<u32>,
  trig : vec4<f32>,
  centers : vec4<f32>,
};
@group(0) @binding(0) var<uniform> params : RotateParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    geom_common!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let x = i32(coord.x);
  let y = i32(coord.y);
  if (params.mode_turn.x == 0u) {
    let turn = params.mode_turn.y;
    let sw = i32(params.mode_turn.z);
    let sh = i32(params.mode_turn.w);
    var sp = vec2<i32>(x, y);
    if (turn == 1u) {
      sp = vec2<i32>(y, sh - 1 - x);
    } else if (turn == 2u) {
      sp = vec2<i32>(sw - 1 - x, sh - 1 - y);
    } else if (turn == 3u) {
      sp = vec2<i32>(sw - 1 - y, x);
    }
    return textureLoad(input_tex, sp, 0);
  }
  let c = params.trig.x;
  let s = params.trig.y;
  let src_cx = params.trig.z;
  let src_cy = params.trig.w;
  let out_cx = params.centers.x;
  let out_cy = params.centers.y;
  let dx = f32(x) - out_cx;
  let dy = f32(y) - out_cy;
  let sx = c * dx + s * dy + src_cx;
  let sy = -s * dx + c * dy + src_cy;
  let src_w = f32(params.mode_turn.z);
  let src_h = f32(params.mode_turn.w);
  if (!(sx >= 0.0 && sy >= 0.0 && sx < src_w && sy < src_h)) {
    return vec4<f32>(0.0);
  }
  return out_bytes(bilinear255(input_tex, vec2<f32>(sx, sy)));
}
"#
);

// ---------------------------------------------------------------------------
// Mirroring: exact horizontal / vertical flips
// ---------------------------------------------------------------------------

/// Uniform block for the mirror pass: `[flags (bit0 h, bit1 v), 0, 0, 0]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MirrorParams {
    pub flags: [u32; 4],
}

impl MirrorParams {
    fn new(horizontal: bool, vertical: bool) -> Self {
        let mut flags = 0u32;
        if horizontal {
            flags |= 1;
        }
        if vertical {
            flags |= 2;
        }
        Self {
            flags: [flags, 0, 0, 0],
        }
    }
}

/// WGSL for the mirror pass — a direct port of `flip_horizontal`/`flip_vertical`.
pub(crate) const MIRROR_STAGE_SRC: &str = concat!(
    r#"
struct MirrorParams {
  flags : vec4<u32>,
};
@group(0) @binding(0) var<uniform> params : MirrorParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    geom_common!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let dims = textureDimensions(input_tex);
  var x = i32(coord.x);
  var y = i32(coord.y);
  if ((params.flags.x & 1u) != 0u) {
    x = i32(dims.x) - 1 - x;
  }
  if ((params.flags.x & 2u) != 0u) {
    y = i32(dims.y) - 1 - y;
  }
  return textureLoad(input_tex, vec2<i32>(x, y), 0);
}
"#
);

// ---------------------------------------------------------------------------
// Pipelines and per-pass plumbing
// ---------------------------------------------------------------------------

/// Compiled geometry pipelines. All six passes share one bind group layout
/// (uniform params at binding 0, sampled input at binding 1).
pub(crate) struct GeometryPipelineState {
    pub layout: wgpu::BindGroupLayout,
    pub lens: wgpu::RenderPipeline,
    pub perspective: wgpu::RenderPipeline,
    pub ca: wgpu::RenderPipeline,
    pub crop: wgpu::RenderPipeline,
    pub rotate: wgpu::RenderPipeline,
    pub mirror: wgpu::RenderPipeline,
}

pub(crate) fn create_geometry_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-geometry-bgl"),
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

pub(crate) fn build_geometry_pipelines(
    device: &wgpu::Device,
) -> Result<GeometryPipelineState, GpuError> {
    let layout = create_geometry_bind_group_layout(device);
    Ok(GeometryPipelineState {
        lens: build_simple_pipeline(
            device,
            "geometry-lens",
            LENS_STAGE_SRC,
            &layout,
            shaders::RGBA8_FORMAT,
        )?,
        perspective: build_simple_pipeline(
            device,
            "geometry-perspective",
            PERSPECTIVE_STAGE_SRC,
            &layout,
            shaders::RGBA8_FORMAT,
        )?,
        ca: build_simple_pipeline(
            device,
            "geometry-ca",
            CA_STAGE_SRC,
            &layout,
            shaders::RGBA8_FORMAT,
        )?,
        crop: build_simple_pipeline(
            device,
            "geometry-crop",
            CROP_STAGE_SRC,
            &layout,
            shaders::RGBA8_FORMAT,
        )?,
        rotate: build_simple_pipeline(
            device,
            "geometry-rotate",
            ROTATE_STAGE_SRC,
            &layout,
            shaders::RGBA8_FORMAT,
        )?,
        mirror: build_simple_pipeline(
            device,
            "geometry-mirror",
            MIRROR_STAGE_SRC,
            &layout,
            shaders::RGBA8_FORMAT,
        )?,
        layout,
    })
}

/// Allocate a transient geometry uniform buffer of `size` bytes.
pub(crate) fn create_geometry_uniform_buffer(
    device: &wgpu::Device,
    size: u64,
    label: &str,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload any `Pod` geometry parameter block.
pub(crate) fn write_geometry_params<T: bytemuck::Pod>(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    params: &T,
) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

/// Bind group for one geometry pass over `input_view`.
pub(crate) fn create_geometry_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-geometry-bindgroup"),
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

// ---------------------------------------------------------------------------
// Host-side plan (dimension + parameter math ported from lumina-core)
// ---------------------------------------------------------------------------

/// One planned geometry sub-stage with its output dimensions.
pub(crate) enum GeometryStep {
    Lens {
        params: LensParams,
        out_width: u32,
        out_height: u32,
    },
    Perspective {
        params: PerspectiveParams,
        out_width: u32,
        out_height: u32,
    },
    Ca {
        params: CaParams,
        out_width: u32,
        out_height: u32,
    },
    Crop {
        params: CropParams,
        out_width: u32,
        out_height: u32,
    },
    Rotate {
        params: RotateParams,
        out_width: u32,
        out_height: u32,
    },
    Mirror {
        params: MirrorParams,
        out_width: u32,
        out_height: u32,
    },
}

impl GeometryStep {
    pub(crate) fn out_dims(&self) -> (u32, u32) {
        match self {
            GeometryStep::Lens {
                out_width,
                out_height,
                ..
            }
            | GeometryStep::Perspective {
                out_width,
                out_height,
                ..
            }
            | GeometryStep::Ca {
                out_width,
                out_height,
                ..
            }
            | GeometryStep::Crop {
                out_width,
                out_height,
                ..
            }
            | GeometryStep::Rotate {
                out_width,
                out_height,
                ..
            }
            | GeometryStep::Mirror {
                out_width,
                out_height,
                ..
            } => (*out_width, *out_height),
        }
    }
}

/// The ordered geometry pass chain plus its final output dimensions.
pub(crate) struct GeometryPlan {
    pub steps: Vec<GeometryStep>,
    pub output_width: u32,
    pub output_height: u32,
}

impl GeometryPlan {
    /// Plan the geometry chain for `recipe` at `width`×`height`, mirroring the
    /// CPU oracle's `apply_lens` → `apply_perspective` → `apply_ca` →
    /// `apply_crop_stage` order and its dimension math. Returns `Ok(None)` when
    /// no geometry sub-stage is active (byte-identical identity).
    pub(crate) fn from_recipe(
        recipe: &lumina_sidecar::EditRecipe,
        width: u32,
        height: u32,
    ) -> Result<Option<Self>, GpuError> {
        let lens = recipe.lens_correction.as_ref();
        let perspective = recipe.perspective.as_ref().filter(|p| !is_neutral(p));
        let geometry = recipe.geometry.as_ref();

        let mut steps: Vec<GeometryStep> = Vec::new();
        let mut cw = width;
        let mut ch = height;

        if let Some(l) = lens {
            let c = lens_coefficients(l)?;
            steps.push(GeometryStep::Lens {
                params: LensParams::new(&c, cw, ch),
                out_width: cw,
                out_height: ch,
            });
        }

        if let Some(p) = perspective {
            let (matrix, min, max, ow, oh) = perspective_setup(cw, ch, p)?;
            det_guard(matrix)?;
            steps.push(GeometryStep::Perspective {
                params: PerspectiveParams::new(matrix, min, max, cw, ch, ow, oh),
                out_width: ow,
                out_height: oh,
            });
            cw = ow;
            ch = oh;
        }

        if let Some(l) = lens {
            let c = lens_coefficients(l)?;
            steps.push(GeometryStep::Ca {
                params: CaParams::new(&c, cw, ch),
                out_width: cw,
                out_height: ch,
            });
        }

        if let Some(g) = geometry {
            if let Some(crop) = g.crop.as_ref() {
                let (px, py, pw, ph) = crop_rect(cw, ch, Some(crop))?;
                if (px, py, pw, ph) != (0, 0, cw, ch) {
                    steps.push(GeometryStep::Crop {
                        params: CropParams {
                            offset: [px, py, 0, 0],
                        },
                        out_width: pw,
                        out_height: ph,
                    });
                    cw = pw;
                    ch = ph;
                }
            }
            if let Some((params, ow, oh)) = rotate_params(g.rotation_degrees, cw, ch) {
                steps.push(GeometryStep::Rotate {
                    params,
                    out_width: ow,
                    out_height: oh,
                });
                cw = ow;
                ch = oh;
            }
            if g.mirror_horizontal || g.mirror_vertical {
                steps.push(GeometryStep::Mirror {
                    params: MirrorParams::new(g.mirror_horizontal, g.mirror_vertical),
                    out_width: cw,
                    out_height: ch,
                });
            }
        }

        if steps.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            steps,
            output_width: cw,
            output_height: ch,
        }))
    }
}

/// Whether a perspective stage is the oracle's identity early-return.
fn is_neutral(p: &Perspective) -> bool {
    p.vertical == 0.0
        && p.horizontal == 0.0
        && p.rotation == 0.0
        && p.scale == 1.0
        && p.aspect_ratio == 1.0
        && p.shift_x == 0.0
        && p.shift_y == 0.0
}

/// Verbatim port of `lumina-core::perspective_matrix`.
fn perspective_matrix(p: &Perspective) -> [[f32; 3]; 3] {
    let sh = (p.horizontal * std::f32::consts::FRAC_PI_4).tan();
    let sv = (p.vertical * std::f32::consts::FRAC_PI_4).tan();
    let a = p.rotation * std::f32::consts::FRAC_PI_4;
    let (s, c) = (a.sin(), a.cos());
    let sx = p.scale;
    let sy = p.scale * p.aspect_ratio;
    let t = [[1., 0., p.shift_x], [0., 1., p.shift_y], [0., 0., 1.]];
    let r = [[c, -s, 0.], [s, c, 0.], [0., 0., 1.]];
    let scale = [[sx, 0., 0.], [0., sy, 0.], [0., 0., 1.]];
    let hy = [[1., 0., 0.], [0., 1., 0.], [0., sv, 1.]];
    let hx = [[1., 0., 0.], [0., 1., 0.], [sh, 0., 1.]];
    fn mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut o = [[0.; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    o[i][j] += a[i][k] * b[k][j];
                }
            }
        }
        o
    }
    mul(mul(mul(mul(t, r), scale), hy), hx)
}

/// Verbatim port of the projection block shared by
/// `lumina-core::perspective_dimensions` and `apply_perspective`.
#[allow(clippy::type_complexity)]
fn perspective_setup(
    width: u32,
    height: u32,
    p: &Perspective,
) -> Result<([[f32; 3]; 3], [f32; 2], [f32; 2], u32, u32), GpuError> {
    let m = perspective_matrix(p);
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for x in [-1.0f32, 1.0] {
        for y in [-1.0f32, 1.0] {
            let d = m[2][0] * x + m[2][1] * y + m[2][2];
            if !d.is_finite() || d.abs() < 1e-6 {
                return Err(invalid("perspective", d as f64, -1.0, 1.0));
            }
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            if !q[0].is_finite() || !q[1].is_finite() {
                return Err(invalid("perspective", q[0] as f64, -1.0, 1.0));
            }
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    let ow_f = ((max[0] - min[0]) * width as f32 / 2.0).ceil().max(1.0);
    let oh_f = ((max[1] - min[1]) * height as f32 / 2.0).ceil().max(1.0);
    if !ow_f.is_finite() || !oh_f.is_finite() {
        return Err(invalid("perspective", ow_f as f64, 1.0, 1_000_000.0));
    }
    let ow = ow_f as u32;
    let oh = oh_f as u32;
    MemoryBudget::default()
        .check_decode(ow as u64, oh as u64, 4, 1)
        .map_err(|e| {
            invalid(
                "perspective canvas too large",
                ow as f64 * oh as f64,
                0.0,
                e.limit() as f64,
            )
        })?;
    Ok((m, min, max, ow, oh))
}

/// The oracle's post-allocation determinant guard from `apply_perspective`.
fn det_guard(m: [[f32; 3]; 3]) -> Result<(), GpuError> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if !det.is_finite() || det.abs() < 1e-6 {
        return Err(invalid("perspective", det as f64, -1.0, 1.0));
    }
    Ok(())
}

/// Verbatim port of `lumina-core::crop_rect`.
fn crop_rect(
    width: u32,
    height: u32,
    crop: Option<&Crop>,
) -> Result<(u32, u32, u32, u32), GpuError> {
    let (x, y, w, h) = match crop {
        None => (0.0, 0.0, 1.0, 1.0),
        Some(Crop::Free {
            x,
            y,
            width,
            height,
        }) => (*x as f64, *y as f64, *width as f64, *height as f64),
        Some(Crop::Aspect { preset }) => {
            let ratio = match preset {
                AspectPreset::Original => width as f64 / height as f64,
                AspectPreset::OneToOne => 1.0,
                AspectPreset::FourToFive => 4.0 / 5.0,
                AspectPreset::FiveToFour => 5.0 / 4.0,
                AspectPreset::ThreeToTwo => 3.0 / 2.0,
                AspectPreset::TwoToThree => 2.0 / 3.0,
                AspectPreset::FourToThree => 4.0 / 3.0,
                AspectPreset::ThreeToFour => 3.0 / 4.0,
                AspectPreset::SixteenToNine => 16.0 / 9.0,
                AspectPreset::NineToSixteen => 9.0 / 16.0,
            };
            let source_ratio = width as f64 / height as f64;
            if source_ratio > ratio {
                (
                    (1.0 - ratio / source_ratio) / 2.0,
                    0.0,
                    ratio / source_ratio,
                    1.0,
                )
            } else {
                (
                    0.0,
                    (1.0 - source_ratio / ratio) / 2.0,
                    1.0,
                    source_ratio / ratio,
                )
            }
        }
    };
    if ![x, y, w, h].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || x < 0.0
        || y < 0.0
        || x > 1.0
        || y > 1.0
        || x + w > 1.0 + 1e-6
        || y + h > 1.0 + 1e-6
    {
        return Err(invalid("geometry.crop", -1.0, 0.0, 1.0));
    }
    if width == 0 || height == 0 {
        return Err(invalid("geometry.crop (empty frame)", -1.0, 0.0, 1.0));
    }
    let px = (((x * width as f64).round() as i64).clamp(0, i64::from(width) - 1)) as u32;
    let py = (((y * height as f64).round() as i64).clamp(0, i64::from(height) - 1)) as u32;
    let pw = ((w * width as f64).round() as u32).max(1).min(width - px);
    let ph = ((h * height as f64).round() as u32).max(1).min(height - py);
    if pw == 0 || ph == 0 {
        return Err(invalid(
            "geometry.crop (empty crop rectangle)",
            -1.0,
            0.0,
            1.0,
        ));
    }
    Ok((px, py, pw, ph))
}

/// Verbatim port of `lumina-core::rotate_dimensions`.
fn rotate_dimensions(w: u32, h: u32, degrees: f32) -> (u32, u32) {
    let quarter_turn = degrees.rem_euclid(180.0).abs() < 1e-4;
    if quarter_turn {
        return (w.max(1), h.max(1));
    }
    let right_angle = (degrees - 90.0).rem_euclid(180.0).abs() < 1e-4;
    if right_angle {
        return (h.max(1), w.max(1));
    }
    let r = degrees.to_radians();
    (
        (w as f32 * r.cos().abs() + h as f32 * r.sin().abs())
            .ceil()
            .max(1.0) as u32,
        (w as f32 * r.sin().abs() + h as f32 * r.cos().abs())
            .ceil()
            .max(1.0) as u32,
    )
}

/// Plan the rotation pass. `None` means the oracle's identity clone (turn 0);
/// otherwise the returned params/out dims mirror `rotate_frame`/
/// `rotate_dimensions` exactly.
fn rotate_params(degrees: f32, w: u32, h: u32) -> Option<(RotateParams, u32, u32)> {
    let turns = (degrees / 90.0).round();
    if (degrees - turns * 90.0).abs() < 1e-4 {
        let turn = (turns as i32).rem_euclid(4);
        if turn == 0 {
            return None;
        }
        let (ow, oh) = if turn % 2 == 0 { (w, h) } else { (h, w) };
        return Some((RotateParams::quarter(turn as u32, w, h, ow, oh), ow, oh));
    }
    let (ow, oh) = rotate_dimensions(w, h, degrees);
    let r = degrees.to_radians();
    Some((
        RotateParams::general(r.cos(), r.sin(), w, h, ow, oh),
        ow,
        oh,
    ))
}

/// Verbatim port of `lumina-core::lens_coefficients` (profile defaults overlaid
/// by explicit fields). An unknown profile is rejected loudly — the same
/// `UnsupportedAdjustment` the CPU oracle's panic guard stands in for and that
/// `validate_gpu_recipe` already emits.
fn lens_coefficients(l: &LensCorrection) -> Result<[f32; 8], GpuError> {
    let mut c = match l.profile.as_deref() {
        Some("wide-light") => [0.12, -0.04, 0.01, 1., 0., 0., 0.006, -0.006],
        Some("tele-light") => [-0.08, 0.02, 0., 1., 0., 0., -0.004, 0.004],
        Some("standard-neutral") => [0., 0., 0., 1., 0., 0., 0., 0.],
        None => [0., 0., 0., 1., 0., 0., 0., 0.],
        Some(other) => {
            return Err(GpuError::Core(CoreError::UnsupportedAdjustment {
                key: format!("lens profile `{other}`"),
            }))
        }
    };
    let explicit = [
        l.distortion_k1.unwrap_or(c[0]),
        l.distortion_k2.unwrap_or(c[1]),
        l.distortion_k3.unwrap_or(c[2]),
        l.vignette_c0.unwrap_or(c[3]),
        l.vignette_c1.unwrap_or(c[4]),
        l.vignette_c2.unwrap_or(c[5]),
        l.ca_red.unwrap_or(c[6]),
        l.ca_blue.unwrap_or(c[7]),
    ];
    c.copy_from_slice(&explicit);
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_pipelines_validate() {
        // Guards the WGSL sources against keyword/syntax regressions (e.g. a
        // reserved word): an invalid shader would produce a pipeline that
        // silently writes nothing, which the pixel parity test would catch as
        // all-zero output — this test makes the root cause explicit.
        let ctx = match crate::GpuContext::new() {
            Ok(ctx) => ctx,
            Err(err) => {
                eprintln!("geometry pipeline validation skipped (no GPU context: {err})");
                return;
            }
        };
        if !ctx.is_available() {
            eprintln!("geometry pipeline validation skipped (no GPU adapter)");
            return;
        }
        let device = ctx.device().expect("device");
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _pipelines = build_geometry_pipelines(device);
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "geometry shaders must validate: {err:?}");
    }

    fn perspective(vertical: f32) -> Perspective {
        Perspective {
            version: 1,
            vertical,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }
    }

    fn lens() -> LensCorrection {
        LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.1),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: Some(0.006),
            ca_blue: Some(-0.006),
        }
    }

    fn recipe_with(
        perspective: Option<Perspective>,
        lens: Option<LensCorrection>,
        geometry: Option<lumina_sidecar::Geometry>,
    ) -> lumina_sidecar::EditRecipe {
        lumina_sidecar::EditRecipe {
            perspective,
            lens_correction: lens,
            geometry,
            ..Default::default()
        }
    }

    #[test]
    fn no_geometry_is_none() {
        let recipe = lumina_sidecar::EditRecipe::default();
        assert!(GeometryPlan::from_recipe(&recipe, 32, 24)
            .unwrap()
            .is_none());
    }

    #[test]
    fn neutral_perspective_is_skipped() {
        let recipe = recipe_with(Some(perspective(0.0)), None, None);
        assert!(GeometryPlan::from_recipe(&recipe, 32, 24)
            .unwrap()
            .is_none());
    }

    #[test]
    fn perspective_changes_dims_like_oracle() {
        let frame = lumina_core::ImageFrame::new(40, 30, vec![0u8; 40 * 30 * 4]).unwrap();
        for vertical in [0.2f32, 0.8] {
            let p = perspective(vertical);
            let domain = frame
                .measurement_domain_with_perspective(None, None, Some(&p))
                .expect("oracle dimensions");
            let recipe = recipe_with(Some(p), None, None);
            let plan = GeometryPlan::from_recipe(&recipe, 40, 30)
                .unwrap()
                .expect("active");
            assert_eq!(plan.steps.len(), 1);
            assert_eq!(
                (plan.output_width, plan.output_height),
                (domain.output_width, domain.output_height),
                "plan dimensions must match the CPU oracle's measurement domain"
            );
        }
    }

    /// Cross-check the planned output dimensions for a full crop + rotation +
    /// perspective chain against the public oracle
    /// (`ImageFrame::measurement_domain_with_perspective`), which is exactly
    /// what `render_frame` produces.
    #[test]
    fn geometry_plan_dims_match_measurement_domain() {
        let frame = lumina_core::ImageFrame::new(64, 48, vec![0u8; 64 * 48 * 4]).unwrap();
        let geometry = lumina_sidecar::Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.1,
                y: 0.15,
                width: 0.6,
                height: 0.5,
            }),
            rotation_degrees: 23.0,
            mirror_horizontal: true,
            mirror_vertical: true,
        };
        let p = perspective(0.2);
        let domain = frame
            .measurement_domain_with_perspective(Some(&geometry), None, Some(&p))
            .expect("oracle dimensions");
        let recipe = recipe_with(Some(p), None, Some(geometry));
        let plan = GeometryPlan::from_recipe(&recipe, 64, 48)
            .unwrap()
            .expect("active");
        assert_eq!(
            (plan.output_width, plan.output_height),
            (domain.output_width, domain.output_height)
        );
    }

    #[test]
    fn crop_rect_ports_oracle_clamping() {
        // Full-frame free rect is the identity crop.
        let crop = Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        assert_eq!(crop_rect(8, 6, Some(&crop)).unwrap(), (0, 0, 8, 6));
        // Half-width centred rect.
        let crop = Crop::Free {
            x: 0.25,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        assert_eq!(crop_rect(8, 6, Some(&crop)).unwrap(), (2, 0, 4, 6));
    }

    #[test]
    fn rotate_plan_matches_oracle_cases() {
        assert!(rotate_params(0.0, 8, 6).is_none());
        let (params, ow, oh) = rotate_params(90.0, 8, 6).unwrap();
        assert_eq!((ow, oh), (6, 8));
        assert_eq!(params.mode_turn[0], 0);
        assert_eq!(params.mode_turn[1], 1);
        let (params, ow, oh) = rotate_params(33.0, 8, 6).unwrap();
        assert_eq!(params.mode_turn[0], 1);
        assert_eq!((ow, oh), rotate_dimensions(8, 6, 33.0));
    }

    #[test]
    fn lens_unknown_profile_is_loud() {
        let l = LensCorrection {
            profile: Some("mystery".into()),
            ..lens()
        };
        assert!(lens_coefficients(&l).is_err());
    }

    #[test]
    fn lens_profile_defaults_and_overrides() {
        let l = LensCorrection {
            profile: Some("standard-neutral".into()),
            distortion_k1: None,
            ..lens()
        };
        let c = lens_coefficients(&l).unwrap();
        assert_eq!(c[0], 0.0);
        assert_eq!(c[3], 1.0);
        // Explicit fields still override profile defaults.
        assert_eq!(c[6], 0.006);
    }

    #[test]
    fn lens_only_plan_keeps_dims_and_adds_ca() {
        let recipe = recipe_with(None, Some(lens()), None);
        let plan = GeometryPlan::from_recipe(&recipe, 20, 10)
            .unwrap()
            .expect("active");
        // lens + CA.
        assert_eq!(plan.steps.len(), 2);
        assert_eq!((plan.output_width, plan.output_height), (20, 10));
    }

    #[test]
    fn full_chain_orders_lens_perspective_ca_crop_rotate_mirror() {
        let recipe = recipe_with(
            Some(perspective(0.2)),
            Some(lens()),
            Some(lumina_sidecar::Geometry {
                version: 1,
                crop: Some(Crop::Free {
                    x: 0.1,
                    y: 0.1,
                    width: 0.5,
                    height: 0.5,
                }),
                rotation_degrees: 12.0,
                mirror_horizontal: true,
                mirror_vertical: false,
            }),
        );
        let plan = GeometryPlan::from_recipe(&recipe, 64, 48)
            .unwrap()
            .expect("active");
        let kinds: Vec<&str> = plan
            .steps
            .iter()
            .map(|s| match s {
                GeometryStep::Lens { .. } => "lens",
                GeometryStep::Perspective { .. } => "perspective",
                GeometryStep::Ca { .. } => "ca",
                GeometryStep::Crop { .. } => "crop",
                GeometryStep::Rotate { .. } => "rotate",
                GeometryStep::Mirror { .. } => "mirror",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["lens", "perspective", "ca", "crop", "rotate", "mirror"]
        );
    }
}
