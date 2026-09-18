//! WGSL sources and uniform parameter blocks for the GPU geometry passes
//! (GPU-RENDER-PARITY-1). Extracted from `geometry.rs` (file-size ratchet,
//! DoD §8); the pass planning/encoding stays in `geometry.rs`.

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
    pub(crate) fn new(c: &[f32; 8], width: u32, height: u32) -> Self {
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
    pub(crate) fn new(
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
    pub(crate) fn new(c: &[f32; 8], width: u32, height: u32) -> Self {
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
    pub(crate) fn quarter(
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

    pub(crate) fn general(
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
    pub(crate) fn new(horizontal: bool, vertical: bool) -> Self {
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
