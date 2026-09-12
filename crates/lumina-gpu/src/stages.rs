//! GPU post-tone adjustment stages: per-pixel color grading (curves, HSL,
//! Point Color, vibrance/saturation, Color Grading) and the neighborhood
//! Presence stages (Texture/Clarity Difference-of-Gaussians + Dehaze).
//!
//! Every shader in this module mirrors the exact per-stage byte math of the
//! platform-neutral CPU oracle in `lumina-core` (`ImageFrame::
//! apply_recipe_with_scale_and_white_balance`) so the GPU path stays
//! pixel-equivalent to the CPU reference:
//!
//! * the CPU rounds to `u8` after **each** sub-stage and the next sub-stage
//!   reads those bytes; the shaders therefore work in the `0..=255` byte domain
//!   and quantize with `roundi` (round half away from zero) between stages;
//! * `rgb_to_hsl`/`hsl_to_rgb` and the Piecewise-cubic-Hermite `monotone_curve`
//!   are ported operation-for-operation;
//! * Presence is a full 2-D clamped-window box average (identical accumulation
//!   order to `apply_dog`, **not** a separable approximation) so the
//!   Difference-of-Gaussians detail term matches the oracle;
//! * Dehaze's deterministic 95th-percentile airlight is computed on the CPU
//!   from a GPU-produced dark-channel texture ([`dehaze_airlight`]), which is
//!   exact because every dark-channel sample is a `u8`-quantized value.
//!
//! Where IEEE-754 rounding of a different operation order is unavoidable
//! (f32 HSL/curve evaluation vs. the oracle's mixed f32/f64), the parity tests
//! document and gate the residual per-channel difference instead of hiding it.

use bytemuck::Zeroable;
use lumina_sidecar::EditRecipe;

// ---------------------------------------------------------------------------
// Shared WGSL helpers (inlined into each shader below)
// ---------------------------------------------------------------------------

/// WGSL fragment preamble: fullscreen-triangle vertex stage plus the shared
/// numeric helpers every post-tone stage uses.
///
/// A macro (not a `const`) because `concat!` only expands literal/macro
/// arguments, not named constants.
macro_rules! common_src {
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
//
// `floor(x + 0.5)` is NOT equivalent in f32: for `x` an ulp below `k + 0.5`
// the sum `x + 0.5` can round up to `k + 1`, adding a spurious LSB. Splitting
// off the exact fractional part (`x - trunc(x)` is always exact) and comparing
// against `0.5` reproduces the oracle bit-for-bit.
fn roundi(x : f32) -> f32 {
  let t = trunc(x);
  let f = x - t;
  if (abs(f) >= 0.5) {
    return t + sign(f);
  }
  return t;
}

fn clamp01(x : f32) -> f32 {
  return clamp(x, 0.0, 1.0);
}

// Mirrors Rust's `f32::rem_euclid` (truncated remainder then adjust for a
// negative result); the divisor is always positive here.
fn rem_euclid_f(x : f32, y : f32) -> f32 {
  var r = x % y;
  if (r < 0.0) {
    r = r + abs(y);
  }
  return r;
}

// `lumina-core::rgb_to_hsl` ported operation-for-operation.
fn rgb_to_hsl(r : f32, g : f32, b : f32) -> vec3<f32> {
  let mx = max(max(r, g), b);
  let mn = min(min(r, g), b);
  let l = (mx + mn) / 2.0;
  if (mx == mn) {
    return vec3<f32>(0.0, 0.0, l);
  }
  let d = mx - mn;
  let s = d / (1.0 - abs(2.0 * l - 1.0));
  var h : f32;
  if (mx == r) {
    h = 60.0 * rem_euclid_f((g - b) / d, 6.0);
  } else if (mx == g) {
    h = 60.0 * ((b - r) / d + 2.0);
  } else {
    h = 60.0 * ((r - g) / d + 4.0);
  }
  if (h < 0.0) {
    h = h + 360.0;
  }
  return vec3<f32>(h, s, l);
}

// `lumina-core::hsl_to_rgb` ported operation-for-operation.
fn hsl_to_rgb(h : f32, s : f32, l : f32) -> vec3<f32> {
  let c = (1.0 - abs(2.0 * l - 1.0)) * s;
  let x = c * (1.0 - abs(rem_euclid_f(h / 60.0, 2.0) - 1.0));
  let m = l - c / 2.0;
  var q : vec3<f32>;
  if (h < 60.0) {
    q = vec3<f32>(c, x, 0.0);
  } else if (h < 120.0) {
    q = vec3<f32>(x, c, 0.0);
  } else if (h < 180.0) {
    q = vec3<f32>(0.0, c, x);
  } else if (h < 240.0) {
    q = vec3<f32>(0.0, x, c);
  } else if (h < 300.0) {
    q = vec3<f32>(x, 0.0, c);
  } else {
    q = vec3<f32>(c, 0.0, x);
  }
  return q + vec3<f32>(m);
}

fn byte_from_norm(x : f32) -> f32 {
  return roundi(clamp(x, 0.0, 1.0) * 255.0);
}

fn norm_from_byte(x : f32) -> f32 {
  return x / 255.0;
}
"#
    };
}

// ---------------------------------------------------------------------------
// Per-pixel color stage (curves → HSL → Point Color → vibrance/saturation →
// Color Grading), matching `apply_recipe`'s order and per-stage quantization.
// ---------------------------------------------------------------------------

/// Maximum curve control points per channel (sidecar `validate_curve`).
pub const MAX_CURVE_POINTS: usize = 32;
/// Maximum persisted Point Color entries (sidecar validation: max 8).
pub const MAX_POINT_COLOR_ENTRIES: usize = 8;

/// Storage-buffer layout for the per-pixel color stage.
///
/// The WGSL struct (`ColorParams` in [`COLOR_STAGE_SRC`]) matches this
/// `#[repr(C)]` layout byte-for-byte: 20 scalar words then the fixed-capacity
/// arrays. Curve points are packed four per `vec4` slot (`vec4` here /
/// `array<vec4<f32>,128>` in WGSL) so the storage alignment is trivial to keep
/// in lockstep; see the `color_params_layout` unit test.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColorParams {
    /// Vibrance slider value (`0` when absent).
    pub vibrance: f32,
    /// Saturation slider value (`0` when absent).
    pub saturation: f32,
    /// 1 when either `vibrance` or `saturation` key is present (the CPU runs the
    /// HSL roundtrip even for a present-but-zero value).
    pub has_vibrance_or_saturation: u32,
    pub has_curves: u32,
    pub has_hsl: u32,
    pub has_point_color: u32,
    pub has_grading: u32,
    /// 1 when any Color Grading range has a non-zero luminance shift.
    pub grading_apply_luminance: u32,
    pub pc_count: u32,
    pub curve_master_count: u32,
    pub curve_r_count: u32,
    pub curve_g_count: u32,
    pub curve_b_count: u32,
    pub has_curve_r: u32,
    pub has_curve_g: u32,
    pub has_curve_b: u32,
    pub grading_balance: f32,
    pub grading_blending: f32,
    pub _pad0: u32,
    pub _pad1: u32,
    /// 4 curves (`master`, `r`, `g`, `b`) × 32 points; `.xy` = (input, output).
    pub curve_pts: [[f32; 4]; 4 * MAX_CURVE_POINTS],
    /// 8 HSL channels; `.xyz` = (hue, saturation, luminance), `.w` = present.
    pub hsl: [[f32; 4]; 8],
    /// shadows/midtones/highlights; `.xyz` = (hue°, saturation, luminance).
    pub grading: [[f32; 4]; 3],
    /// Point Color entries; `.xyzw` = (hue_center, hue_range, hue_shift, sat).
    pub pc: [[f32; 4]; MAX_POINT_COLOR_ENTRIES],
    /// Point Color luminance shifts, parallel to [`ColorParams::pc`].
    pub pc_lum: [f32; MAX_POINT_COLOR_ENTRIES],
}

impl ColorParams {
    /// Fill the fixed-capacity arrays from a recipe, clamping every count to the
    /// sidecar limits. Malformed (shorter-than-2) curves are treated as absent
    /// per channel; the CPU oracle would reject such a recipe during validation,
    /// so this never changes a valid render.
    pub fn from_recipe(recipe: &EditRecipe) -> Self {
        let mut params = Self::zeroed();

        params.vibrance = recipe.adjustments.get("vibrance").copied().unwrap_or(0.0) as f32;
        params.saturation = recipe.adjustments.get("saturation").copied().unwrap_or(0.0) as f32;
        params.has_vibrance_or_saturation = u32::from(
            adjustment_key_present(recipe, "vibrance")
                || adjustment_key_present(recipe, "saturation"),
        );

        if let Some(curves) = &recipe.curves {
            params.has_curves = 1;
            params.curve_master_count = copy_curve(&mut params.curve_pts, 0, &curves.master);
            params.has_curve_r = u32::from(copy_channel_curve(
                &mut params.curve_pts,
                &mut params.curve_r_count,
                1,
                &curves.channels.red,
            ));
            params.has_curve_g = u32::from(copy_channel_curve(
                &mut params.curve_pts,
                &mut params.curve_g_count,
                2,
                &curves.channels.green,
            ));
            params.has_curve_b = u32::from(copy_channel_curve(
                &mut params.curve_pts,
                &mut params.curve_b_count,
                3,
                &curves.channels.blue,
            ));
            if params.curve_master_count < 2 {
                params.has_curves = 0;
            }
        }

        if let Some(hsl) = &recipe.hsl {
            // Presence is per-sub-stage: the CPU runs `apply_hsl` whenever the
            // object is present, even if every channel is `None`/zero.
            params.has_hsl = 1;
            let channels = [
                hsl.red,
                hsl.orange,
                hsl.yellow,
                hsl.green,
                hsl.cyan,
                hsl.blue,
                hsl.violet,
                hsl.magenta,
            ];
            for (slot, channel) in params.hsl.iter_mut().zip(channels.iter()) {
                if let Some(channel) = channel {
                    *slot = [channel.hue, channel.saturation, channel.luminance, 1.0];
                }
            }
        }

        if let Some(point_color) = &recipe.point_color {
            if !point_color.entries.is_empty() {
                params.has_point_color = 1;
                let count = point_color.entries.len().min(MAX_POINT_COLOR_ENTRIES);
                params.pc_count = count as u32;
                for (i, entry) in point_color.entries.iter().take(count).enumerate() {
                    params.pc[i] = [
                        entry.hue_center,
                        entry.hue_range,
                        entry.hue_shift,
                        entry.saturation_shift,
                    ];
                    params.pc_lum[i] = entry.luminance_shift;
                }
            }
        }

        if let Some(grading) = &recipe.color_grading {
            params.has_grading = 1;
            params.grading[0] = [
                grading.shadows.hue_degrees,
                grading.shadows.saturation,
                grading.shadows.luminance,
                0.0,
            ];
            params.grading[1] = [
                grading.midtones.hue_degrees,
                grading.midtones.saturation,
                grading.midtones.luminance,
                0.0,
            ];
            params.grading[2] = [
                grading.highlights.hue_degrees,
                grading.highlights.saturation,
                grading.highlights.luminance,
                0.0,
            ];
            params.grading_apply_luminance = u32::from(
                grading.shadows.luminance != 0.0
                    || grading.midtones.luminance != 0.0
                    || grading.highlights.luminance != 0.0,
            );
            params.grading_balance = grading.balance;
            params.grading_blending = grading.blending;
        }

        params
    }

    /// Whether any sub-stage of the color pass is active.
    pub fn needs_stage(recipe: &EditRecipe) -> bool {
        recipe.curves.is_some()
            || recipe.hsl.is_some()
            || recipe.color_grading.is_some()
            || recipe
                .point_color
                .as_ref()
                .is_some_and(|pc| !pc.entries.is_empty())
            || adjustment_key_present(recipe, "vibrance")
            || adjustment_key_present(recipe, "saturation")
    }
}

fn adjustment_key_present(recipe: &EditRecipe, key: &str) -> bool {
    recipe.adjustments.contains_key(key)
}

/// Copy a curve into slot `curve_index` of the packed point array. Returns the
/// number of points written (`0` for a malformed curve, treated as absent).
fn copy_curve(
    points: &mut [[f32; 4]; 4 * MAX_CURVE_POINTS],
    curve_index: usize,
    curve: &[lumina_sidecar::CurvePoint],
) -> u32 {
    let count = curve.len().min(MAX_CURVE_POINTS);
    for (i, point) in curve.iter().take(count).enumerate() {
        points[curve_index * MAX_CURVE_POINTS + i] = [point.input, point.output, 0.0, 0.0];
    }
    count as u32
}

/// Copy an optional per-channel curve. Returns `true` only when it has the
/// minimum of two control points the oracle's `monotone_curve` requires.
fn copy_channel_curve(
    points: &mut [[f32; 4]; 4 * MAX_CURVE_POINTS],
    count_out: &mut u32,
    curve_index: usize,
    curve: &Option<Vec<lumina_sidecar::CurvePoint>>,
) -> bool {
    match curve {
        Some(curve) if curve.len() >= 2 => {
            *count_out = copy_curve(points, curve_index, curve);
            true
        }
        _ => false,
    }
}

/// The per-pixel color stage WGSL. `@group(0) @binding(0)` is the
/// [`ColorParams`] storage buffer, `@binding(1)` the input RGBA8 texture
/// (read with exact `textureLoad`).
pub const COLOR_STAGE_SRC: &str = concat!(
    r#"
struct ColorParams {
  vibrance : f32,
  saturation : f32,
  has_vibrance_or_saturation : u32,
  has_curves : u32,
  has_hsl : u32,
  has_point_color : u32,
  has_grading : u32,
  grading_apply_luminance : u32,
  pc_count : u32,
  curve_master_count : u32,
  curve_r_count : u32,
  curve_g_count : u32,
  curve_b_count : u32,
  has_curve_r : u32,
  has_curve_g : u32,
  has_curve_b : u32,
  grading_balance : f32,
  grading_blending : f32,
  pad0 : u32,
  pad1 : u32,
  curve_pts : array<vec4<f32>, 128>,
  hsl : array<vec4<f32>, 8>,
  grading : array<vec4<f32>, 3>,
  pc : array<vec4<f32>, 8>,
  pc_lum : array<f32, 8>,
};
@group(0) @binding(0) var<storage, read> params : ColorParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;

const F32_EPS : f32 = 1.1920929e-7; // f32::EPSILON
"#,
    common_src!(),
    r#"
// ---------------------------------------------------------------------------
// Monotone cubic Hermite curve (lumina-core::monotone_curve).
// ---------------------------------------------------------------------------
fn curve_point(base : u32, i : u32) -> vec2<f32> {
  let p = params.curve_pts[base + i];
  return vec2<f32>(p.x, p.y);
}

fn curve_slope(base : u32, count : u32, j : u32) -> f32 {
  if (j == 0u) {
    let a = curve_point(base, 0u);
    let b = curve_point(base, 1u);
    return (b.y - a.y) / (b.x - a.x);
  } else if (j + 1u == count) {
    let a = curve_point(base, j - 1u);
    let b = curve_point(base, j);
    return (b.y - a.y) / (b.x - a.x);
  }
  let a = curve_point(base, j - 1u);
  let b = curve_point(base, j + 1u);
  return (b.y - a.y) / (b.x - a.x);
}

fn monotone_curve(base : u32, count : u32, x_in : f32) -> f32 {
  let x = clamp(x_in, 0.0, 1.0);
  if (count < 2u) {
    return x;
  }
  var idx : u32 = count - 2u;
  for (var i : u32 = 0u; i + 1u < count; i = i + 1u) {
    if (x <= curve_point(base, i + 1u).x) {
      idx = i;
      break;
    }
  }
  let a = curve_point(base, idx);
  let b = curve_point(base, idx + 1u);
  let h = b.x - a.x;
  let t = clamp((x - a.x) / h, 0.0, 1.0);
  let d = (b.y - a.y) / h;
  var m0 : f32;
  var m1 : f32;
  if (d == 0.0) {
    m0 = 0.0;
    m1 = 0.0;
  } else {
    let lo = min(0.0, 3.0 * d);
    let hi = max(0.0, 3.0 * d);
    m0 = clamp(curve_slope(base, count, idx), lo, hi);
    m1 = clamp(curve_slope(base, count, idx + 1u), lo, hi);
  }
  let t2 = t * t;
  let t3 = t2 * t;
  return clamp(
    (2.0 * t3 - 3.0 * t2 + 1.0) * a.y
      + (t3 - 2.0 * t2 + t) * h * m0
      + (-2.0 * t3 + 3.0 * t2) * b.y
      + (t3 - t2) * h * m1,
    0.0,
    1.0
  );
}

// ---------------------------------------------------------------------------
// Stage: curves (apply_recipe's curves block).
// ---------------------------------------------------------------------------
fn apply_curves_stage(px : vec3<f32>) -> vec3<f32> {
  let r0 = norm_from_byte(px.x);
  let g0 = norm_from_byte(px.y);
  let b0 = norm_from_byte(px.z);
  let lum = 0.2126 * r0 + 0.7152 * g0 + 0.0722 * b0;
  let master = monotone_curve(0u, params.curve_master_count, lum);

  var out : vec3<f32>;
  let originals = array<f32, 3>(r0, g0, b0);
  let bases = array<u32, 3>(32u, 64u, 96u);
  let counts = array<u32, 3>(params.curve_r_count, params.curve_g_count, params.curve_b_count);
  let present = array<u32, 3>(params.has_curve_r, params.has_curve_g, params.has_curve_b);
  for (var i = 0u; i < 3u; i = i + 1u) {
    var value = originals[i];
    if (present[i] != 0u) {
      value = monotone_curve(bases[i], counts[i], originals[i]);
    }
    if (lum > 1e-9) {
      value = value * master / lum;
    } else {
      value = master;
    }
    out[i] = byte_from_norm(value);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Stage: HSL (apply_hsl).
// ---------------------------------------------------------------------------
fn hsl_weight(idx : u32, hue : f32) -> f32 {
  let centers = array<f32, 8>(0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0);
  let center = centers[idx];
  var previous : f32;
  if (idx == 0u) {
    previous = 360.0 - centers[7];
  } else {
    previous = center - centers[idx - 1u];
  }
  var next : f32;
  if (idx + 1u == 8u) {
    next = 360.0 - center + centers[0];
  } else {
    next = centers[idx + 1u] - center;
  }
  let cw = rem_euclid_f(hue - center, 360.0);
  let ccw = rem_euclid_f(center - hue, 360.0);
  if (cw <= next) {
    return 1.0 - cw / next;
  } else if (ccw <= previous) {
    return 1.0 - ccw / previous;
  }
  return 0.0;
}

fn apply_hsl_stage(px : vec3<f32>) -> vec3<f32> {
  let hsl = rgb_to_hsl(norm_from_byte(px.x), norm_from_byte(px.y), norm_from_byte(px.z));
  var hue = hsl.x;
  var sat = hsl.y;
  var light = hsl.z;
  var weights : array<f32, 8>;
  var weight_sum : f32 = 0.0;
  for (var i = 0u; i < 8u; i = i + 1u) {
    let w = hsl_weight(i, hue);
    weights[i] = w;
    weight_sum = weight_sum + w;
  }
  if (weight_sum > F32_EPS) {
    var dh : f32 = 0.0;
    var ds : f32 = 0.0;
    var dl : f32 = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
      let channel = params.hsl[i];
      let w = weights[i] / weight_sum;
      if (channel.w != 0.0) {
        dh = dh + channel.x * 30.0 * w;
        ds = ds + channel.y * w;
        dl = dl + channel.z * w;
      }
    }
    hue = rem_euclid_f(hue + dh, 360.0);
    sat = clamp(sat + ds, 0.0, 1.0);
    light = clamp(light + dl, 0.0, 1.0);
    let rgb = hsl_to_rgb(hue, sat, light);
    return vec3<f32>(
      byte_from_norm(rgb.x),
      byte_from_norm(rgb.y),
      byte_from_norm(rgb.z)
    );
  }
  return px;
}

// ---------------------------------------------------------------------------
// Stage: Point Color (apply_point_color).
// ---------------------------------------------------------------------------
fn apply_point_color_stage(px : vec3<f32>) -> vec3<f32> {
  let hsl = rgb_to_hsl(norm_from_byte(px.x), norm_from_byte(px.y), norm_from_byte(px.z));
  var hue = hsl.x;
  var sat = hsl.y;
  var light = hsl.z;
  var touched = false;
  for (var i = 0u; i < params.pc_count; i = i + 1u) {
    let entry = params.pc[i];
    let center = entry.x;
    let range = entry.y;
    let distance = min(
      rem_euclid_f(hue - center, 360.0),
      rem_euclid_f(center - hue, 360.0)
    );
    var weight : f32;
    if (range <= F32_EPS) {
      if (distance <= F32_EPS) {
        weight = 1.0;
      } else {
        weight = 0.0;
      }
    } else if (distance >= range) {
      weight = 0.0;
    } else {
      weight = 1.0 - distance / range;
    }
    if (weight <= F32_EPS) {
      continue;
    }
    touched = true;
    hue = rem_euclid_f(hue + entry.z * 30.0 * weight, 360.0);
    sat = clamp(sat + entry.w * weight, 0.0, 1.0);
    light = clamp(light + params.pc_lum[i] * weight, 0.0, 1.0);
  }
  if (touched) {
    let rgb = hsl_to_rgb(hue, sat, light);
    return vec3<f32>(
      byte_from_norm(rgb.x),
      byte_from_norm(rgb.y),
      byte_from_norm(rgb.z)
    );
  }
  return px;
}

// ---------------------------------------------------------------------------
// Stage: vibrance + saturation (apply_vibrance_and_saturation).
// ---------------------------------------------------------------------------
fn apply_vibrance_stage(px : vec3<f32>) -> vec3<f32> {
  let hsl = rgb_to_hsl(norm_from_byte(px.x), norm_from_byte(px.y), norm_from_byte(px.z));
  let hue = hsl.x;
  var sat = hsl.y;
  let light = hsl.z;
  if (params.vibrance != 0.0) {
    var skin : f32;
    if (!(hue >= 5.0 && hue <= 65.0)) {
      skin = 1.0;
    } else if (hue < 15.0) {
      skin = (15.0 - hue) / 10.0;
    } else if (hue <= 55.0) {
      skin = 0.0;
    } else {
      skin = (hue - 55.0) / 10.0;
    }
    let protection = (1.0 - sat) * skin;
    let direction_weight = select(sat, 1.0 - sat, params.vibrance >= 0.0);
    sat = clamp(sat + params.vibrance * protection * direction_weight, 0.0, 1.0);
  }
  sat = clamp(sat * (1.0 + params.saturation), 0.0, 1.0);
  let rgb = hsl_to_rgb(hue, sat, light);
  return vec3<f32>(
    byte_from_norm(rgb.x),
    byte_from_norm(rgb.y),
    byte_from_norm(rgb.z)
  );
}

// ---------------------------------------------------------------------------
// Stage: Color Grading (apply_color_grading).
// ---------------------------------------------------------------------------
fn apply_grading_stage(px : vec3<f32>) -> vec3<f32> {
  let shadow_edge = 0.65 - params.grading_balance * 0.15
    + (params.grading_blending - 0.5) * 0.2;
  let highlight_edge = 0.35 - params.grading_balance * 0.15
    - (params.grading_blending - 0.5) * 0.2;
  var output = vec3<f32>(
    norm_from_byte(px.x),
    norm_from_byte(px.y),
    norm_from_byte(px.z)
  );
  let luminance = 0.2126 * output.x + 0.7152 * output.y + 0.0722 * output.z;
  let t_shadow = clamp(luminance / shadow_edge, 0.0, 1.0);
  let shadow = 1.0 - t_shadow * t_shadow * (3.0 - 2.0 * t_shadow);
  let t_high = clamp((luminance - highlight_edge) / (1.0 - highlight_edge), 0.0, 1.0);
  let highlight = t_high * t_high * (3.0 - 2.0 * t_high);
  let midtone = max(1.0 - shadow - highlight, 0.0);
  let sum = shadow + midtone + highlight;
  let w_shadow = shadow / sum;
  let w_mid = midtone / sum;
  let w_high = highlight / sum;
  let weights = array<f32, 3>(w_shadow, w_mid, w_high);
  for (var i = 0u; i < 3u; i = i + 1u) {
    let range = params.grading[i];
    let weight = weights[i];
    if (range.y == 0.0 || weight == 0.0) {
      continue;
    }
    let tint = hsl_to_rgb(rem_euclid_f(range.x, 360.0), 1.0, 0.5);
    let amount = weight * range.y;
    output = output + (tint - output) * amount;
  }
  if (params.grading_apply_luminance != 0u) {
    let lum_shift = weights[0] * params.grading[0].z
      + weights[1] * params.grading[1].z
      + weights[2] * params.grading[2].z;
    let hsl = rgb_to_hsl(output.x, output.y, output.z);
    output = hsl_to_rgb(hsl.x, hsl.y, clamp(hsl.z + lum_shift, 0.0, 1.0));
  }
  return vec3<f32>(
    byte_from_norm(output.x),
    byte_from_norm(output.y),
    byte_from_norm(output.z)
  );
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coords = vec2<u32>(frag_coord.xy);
  let src = textureLoad(input_tex, coords, 0);
  var px = vec3<f32>(roundi(src.r * 255.0), roundi(src.g * 255.0), roundi(src.b * 255.0));
  if (params.has_curves != 0u) {
    px = apply_curves_stage(px);
  }
  if (params.has_hsl != 0u) {
    px = apply_hsl_stage(px);
  }
  if (params.has_point_color != 0u) {
    px = apply_point_color_stage(px);
  }
  if (params.has_vibrance_or_saturation != 0u) {
    px = apply_vibrance_stage(px);
  }
  if (params.has_grading != 0u) {
    px = apply_grading_stage(px);
  }
  return vec4<f32>(norm_from_byte(px.x), norm_from_byte(px.y), norm_from_byte(px.z), src.a);
}
"#
);

/// Bind group layout for [`COLOR_STAGE_SRC`]: storage params (0) + input (1).
pub fn create_color_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-color-bgl"),
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
        ],
    })
}

/// Build the per-pixel color render pipeline for `target_format`.
pub fn create_color_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    let layout = create_color_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-color-pl"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-color-shader"),
        source: wgpu::ShaderSource::Wgsl(COLOR_STAGE_SRC.into()),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumina-gpu-color-pipeline"),
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
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        }),
    )
}

/// Allocate the [`ColorParams`] storage buffer (sized exactly for the struct).
pub fn create_color_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-color-params"),
        size: std::mem::size_of::<ColorParams>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload [`ColorParams`] into its storage buffer.
pub fn write_color_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &ColorParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

/// Bind group for one color pass over `input_view`.
pub fn create_color_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-color-bindgroup"),
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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

/// Shared pipeline builder for the simple fullscreen passes.
fn build_simple_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader_src: &str,
    bind_group_layout: &wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("lumina-gpu-{label}-pl")),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&format!("lumina-gpu-{label}-shader")),
        source: wgpu::ShaderSource::Wgsl(shader_src.into()),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("lumina-gpu-{label}-pipeline")),
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
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{ColorGrading, ColorGradingRange, PointColor, PointColorEntry};

    /// The WGSL storage struct must match the `#[repr(C)]` Rust struct exactly;
    /// a mismatch would silently read garbage shader-side. Pins the total size
    /// and the offsets of the array members the shader indexes.
    #[test]
    fn color_params_layout_matches_wgsl() {
        assert_eq!(std::mem::size_of::<ColorParams>(), 2464);
        // Leading scalars are 20 words.
        assert_eq!(std::mem::offset_of!(ColorParams, curve_pts), 80);
        assert_eq!(std::mem::offset_of!(ColorParams, hsl), 80 + 2048);
        assert_eq!(std::mem::offset_of!(ColorParams, grading), 80 + 2048 + 128);
        assert_eq!(std::mem::offset_of!(ColorParams, pc), 80 + 2048 + 128 + 48);
        assert_eq!(
            std::mem::offset_of!(ColorParams, pc_lum),
            80 + 2048 + 128 + 48 + 128
        );
    }

    #[test]
    fn color_params_presence_flags_match_oracle_stage_gates() {
        // Absent stages: no flags.
        let params = ColorParams::from_recipe(&EditRecipe::default());
        assert_eq!(params.has_curves, 0);
        assert_eq!(params.has_hsl, 0);
        assert_eq!(params.has_point_color, 0);
        assert_eq!(params.has_grading, 0);
        assert_eq!(params.has_vibrance_or_saturation, 0);

        // Present-but-neutral stages still run on the CPU, so the GPU must
        // enable them too (the roundtrip is the oracle's byte behavior).
        let recipe = EditRecipe {
            adjustments: std::collections::BTreeMap::from([("vibrance".into(), 0.0)]),
            hsl: Some(Default::default()),
            point_color: Some(PointColor {
                version: 1,
                entries: vec![PointColorEntry {
                    id: "pc-1".into(),
                    hue_center: 30.0,
                    hue_range: 20.0,
                    hue_shift: 0.0,
                    saturation_shift: 0.0,
                    luminance_shift: 0.0,
                }],
            }),
            color_grading: Some(ColorGrading {
                version: 1,
                shadows: ColorGradingRange {
                    hue_degrees: 10.0,
                    saturation: 0.2,
                    luminance: 0.0,
                },
                midtones: ColorGradingRange::neutral(),
                highlights: ColorGradingRange::neutral(),
                balance: 0.1,
                blending: 0.5,
            }),
            ..Default::default()
        };
        let params = ColorParams::from_recipe(&recipe);
        assert_eq!(params.has_vibrance_or_saturation, 1);
        assert_eq!(params.has_hsl, 1);
        assert_eq!(params.has_point_color, 1);
        assert_eq!(params.pc_count, 1);
        assert_eq!(params.has_grading, 1);
        assert_eq!(params.grading_apply_luminance, 0);
        assert!((params.grading_balance - 0.1).abs() < 1e-6);
    }

    /// Dehaze airlight reproduces the oracle's `sorted[p].max(0.05)` exactly.
    #[test]
    fn dehaze_airlight_matches_counting_sort() {
        // 101 values 0..=100; n=101, index = (101*0.95) as usize = 95 →
        // sorted value 95, well above the 0.05 floor.
        let dark: Vec<u8> = (0..=100).collect();
        let a = dehaze_airlight(&dark);
        assert!((a - 95.0 / 255.0).abs() < 1e-7, "got {a}");

        // A small dark channel stays below the floor and clamps at 0.05.
        let dark: Vec<u8> = (0..10).collect();
        assert_eq!(dehaze_airlight(&dark), 0.05);

        // A dark channel capped well below the floor clamps at 0.05.
        let dark = vec![0u8; 100];
        assert_eq!(dehaze_airlight(&dark), 0.05);

        // Empty slice: explicit floor, no panic.
        assert_eq!(dehaze_airlight(&[]), 0.05);
    }

    #[test]
    fn dog_params_radii_match_oracle() {
        assert_eq!(DogParams::texture(0.0).radius, 1);
        assert_eq!(DogParams::texture(0.5).radius, 1 + 1);
        assert_eq!(DogParams::texture(-1.0).radius, 3);
        assert_eq!(DogParams::clarity(0.0).radius, 8);
        assert_eq!(DogParams::clarity(1.0).radius, 8 + 24);
        assert_eq!(DogParams::clarity(-0.5).radius, 8 + 12);
    }
}
