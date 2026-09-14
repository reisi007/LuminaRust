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
pub(crate) fn build_simple_pipeline(
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

// ---------------------------------------------------------------------------
// GPU-RENDER-PARITY-1 stage 2: Noise Reduction, Sharpening and Effects
// (vignette + grain). Every shader mirrors the exact CPU-oracle math in
// `lumina-core::apply_noise_reduction` / `apply_sharpening` / `apply_vignette`
// / `apply_grain` (the same `u8`-quantized byte domain and the same operation
// order), so the GPU route stays pixel-equivalent to `render_frame`.
// ---------------------------------------------------------------------------

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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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
fn grain_effective_seed(seed: u64, width: u32, height: u32) -> u32 {
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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
fn sharpen_kernel(sigma: f32) -> (u32, Vec<f32>) {
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    build_simple_pipeline(
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
) -> Result<wgpu::ComputePipeline, super::GpuError> {
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

// ---------------------------------------------------------------------------
// Red-eye correction (G-14 / LRPAR-G14-REDEYE-15): per-pixel, region-local
// desaturation + darkening. Runs after Sharpening and before Effects — the
// same slot `apply_red_eye` occupies in `apply_recipe`.
//
// The oracle multiplies a per-pixel red-dominance (`redness`) with a spatial
// falloff (full strength inside 75 % of the radius, linear to the edge). Both
// operands are pure per-pixel/region functions, so the GPU port is a single
// fullscreen pass with the regions in a storage buffer and no neighborhood
// sampling; the only residual is `hypot` rounding at the disc edge.
// ---------------------------------------------------------------------------

/// Maximum persisted red-eye regions (sidecar [`lumina_sidecar::RED_EYE_MAX_REGIONS`]).
pub const MAX_RED_EYE_REGIONS: usize = lumina_sidecar::RED_EYE_MAX_REGIONS;

/// Storage-buffer layout for the red-eye pass.
///
/// The WGSL struct in [`RED_EYE_STAGE_SRC`] matches this `#[repr(C)]` layout
/// byte-for-byte: a 4-word header then the fixed-capacity region arrays.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RedEyeParams {
    /// Number of populated regions (`0..=MAX_RED_EYE_REGIONS`).
    pub count: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
    /// `.xy` = normalized pupil center, `.z` = normalized radius,
    /// `.w` = desaturate strength.
    pub regions: [[f32; 4]; MAX_RED_EYE_REGIONS],
    /// Darken strength, parallel to [`RedEyeParams::regions`].
    pub darken: [f32; MAX_RED_EYE_REGIONS],
}

impl RedEyeParams {
    /// Fill the fixed-capacity arrays from a recipe, clamping the region count to
    /// the sidecar limit (the CPU oracle rejects more than
    /// [`MAX_RED_EYE_REGIONS`] before rendering, so this never changes a valid
    /// render).
    pub fn from_red_eye(r: &lumina_sidecar::RedEyeCorrection) -> Self {
        let mut params = Self::zeroed();
        let count = r.regions.len().min(MAX_RED_EYE_REGIONS);
        params.count = count as u32;
        for (i, region) in r.regions.iter().take(count).enumerate() {
            params.regions[i] = [region.x, region.y, region.radius, region.desaturate];
            params.darken[i] = region.darken;
        }
        params
    }

    /// Whether the oracle would run the red-eye corrector: a non-empty region
    /// list with at least one non-zero strength (`apply_red_eye`'s early
    /// returns). A present-but-neutral correction is identity.
    pub fn needs_stage(r: &lumina_sidecar::RedEyeCorrection) -> bool {
        !r.regions.is_empty()
            && r.regions
                .iter()
                .any(|region| region.desaturate != 0.0 || region.darken != 0.0)
    }
}

/// WGSL for the red-eye pass — a direct port of `apply_red_eye`.
pub const RED_EYE_STAGE_SRC: &str = concat!(
    r#"
struct RedEyeParams {
  count : u32,
  pad0 : u32,
  pad1 : u32,
  pad2 : u32,
  regions : array<vec4<f32>, 32>,
  darken : array<f32, 32>,
};
@group(0) @binding(0) var<storage, read> params : RedEyeParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag_coord.xy);
  let src = textureLoad(input_tex, coord, 0);
  let dims = vec2<f32>(textureDimensions(input_tex));
  let min_dim = min(dims.x, dims.y);
  var r = src.r;
  var g = src.g;
  var b = src.b;
  let redness = clamp((r - max(g, b)) / max(r, 0.001), 0.0, 1.0);
  if (redness <= 0.0) {
    return vec4<f32>(src.r, src.g, src.b, src.a);
  }
  let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  let pixel = vec2<f32>(f32(coord.x), f32(coord.y));
  for (var i = 0u; i < params.count; i = i + 1u) {
    let region = params.regions[i];
    let center = vec2<f32>(region.x * dims.x, region.y * dims.y);
    let radius_px = region.z * min_dim;
    let dx = pixel.x - center.x;
    let dy = pixel.y - center.y;
    let dist = sqrt(dx * dx + dy * dy);
    if (dist > radius_px) {
      continue;
    }
    let feather = max(radius_px * 0.25, 1e-6);
    let falloff = clamp((radius_px - dist) / feather, 0.0, 1.0);
    let desat_k = region.w * falloff * redness;
    r = r + (luminance - r) * desat_k;
    let darken_k = params.darken[i] * falloff * redness;
    let factor = 1.0 - darken_k;
    r = r * factor;
    g = g * factor;
    b = b * factor;
  }
  return vec4<f32>(
    norm_from_byte(byte_from_norm(r)),
    norm_from_byte(byte_from_norm(g)),
    norm_from_byte(byte_from_norm(b)),
    src.a
  );
}
"#
);

/// Bind group layout for [`RED_EYE_STAGE_SRC`]: storage params (0) + input (1).
pub fn create_red_eye_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-red-eye-bgl"),
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

/// Build the red-eye render pipeline for `target_format`.
pub fn create_red_eye_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    let layout = create_red_eye_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-red-eye-pl"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-red-eye-shader"),
        source: wgpu::ShaderSource::Wgsl(RED_EYE_STAGE_SRC.into()),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumina-gpu-red-eye-pipeline"),
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

/// Allocate the [`RedEyeParams`] storage buffer (sized exactly for the struct).
pub fn create_red_eye_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-red-eye-params"),
        size: std::mem::size_of::<RedEyeParams>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload [`RedEyeParams`] into its storage buffer.
pub fn write_red_eye_params(queue: &wgpu::Queue, buffer: &wgpu::Buffer, params: &RedEyeParams) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(params));
}

/// Bind group for one red-eye pass over `input_view`.
pub fn create_red_eye_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-red-eye-bindgroup"),
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
// Spot heal (GPU-RENDER-PARITY-1 follow-up): the legacy
// `extras["spot_removals"]` heal geometry, ported operation-for-operation from
// `lumina_core::spot_heal::apply_spot_heals`.
// ---------------------------------------------------------------------------

/// One GPU spot-heal entry, byte-matching the WGSL `Spot` struct.
///
/// All values are the recipe's validated fields; the shader derives the pixel
/// geometry from the frame dimensions exactly like the oracle (`center_x *
/// width`, `offset_dx * width`, …).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpotGpu {
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
    pub feather: f32,
    pub offset_dx: f32,
    pub offset_dy: f32,
    pub opacity: f32,
    pub _pad: f32,
}

/// Storage-buffer header for the spot-heal pass (16-byte aligned count).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpotHealHeader {
    pub count: u32,
    pub _pad: [u32; 3],
}

/// Byte size of the spot-heal storage buffer for `count` spots.
pub fn spot_heal_buffer_size(count: usize) -> u64 {
    (std::mem::size_of::<SpotHealHeader>() + count * std::mem::size_of::<SpotGpu>()) as u64
}

/// Encode a validated spot list into the GPU buffer bytes (header + entries).
pub fn spot_heal_params_bytes(spots: &[lumina_core::SpotHeuristic]) -> Vec<u8> {
    let header = SpotHealHeader {
        count: spots.len() as u32,
        _pad: [0; 3],
    };
    let mut bytes = bytemuck::bytes_of(&header).to_vec();
    for spot in spots {
        let gpu = SpotGpu {
            center_x: spot.center_x,
            center_y: spot.center_y,
            radius: spot.radius,
            feather: spot.feather,
            offset_dx: spot.offset_dx,
            offset_dy: spot.offset_dy,
            opacity: spot.opacity,
            _pad: 0.0,
        };
        bytes.extend_from_slice(bytemuck::bytes_of(&gpu));
    }
    bytes
}

/// WGSL for the spot-heal pass — a direct port of `apply_spot_heals`.
///
/// The oracle samples every spot from the **pre-heal** frame (`src_pixels`) and
/// blends sequentially into the working frame, so an overlap is order-dependent;
/// this shader loops the spots in the persisted order and blends the same
/// pre-heal texel, which is per-pixel equivalent. RGB only, alpha unchanged.
pub const SPOT_HEAL_STAGE_SRC: &str = concat!(
    r#"
struct Spot {
  center_x : f32,
  center_y : f32,
  radius : f32,
  feather : f32,
  offset_dx : f32,
  offset_dy : f32,
  opacity : f32,
  pad : f32,
};

struct SpotHealParams {
  count : u32,
  pad0 : u32,
  pad1 : u32,
  pad2 : u32,
  spots : array<Spot>,
};

@group(0) @binding(0) var<storage, read> params : SpotHealParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
"#,
    common_src!(),
    r#"
@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag_coord.xy);
  let src = textureLoad(input_tex, coord, 0);
  var r = byte_from_norm(src.r);
  var g = byte_from_norm(src.g);
  var b = byte_from_norm(src.b);
  if (params.count == 0u) {
    return vec4<f32>(norm_from_byte(r), norm_from_byte(g), norm_from_byte(b), src.a);
  }
  let dims = vec2<f32>(textureDimensions(input_tex));
  let w = dims.x;
  let h = dims.y;
  let px = f32(coord.x);
  let py = f32(coord.y);
  let max_x = i32(dims.x) - 1;
  let max_y = i32(dims.y) - 1;
  for (var i = 0u; i < params.count; i = i + 1u) {
    let s = params.spots[i];
    let cx = s.center_x * w;
    let cy = s.center_y * h;
    let radius = s.radius;
    let ddx = px + 0.5 - cx;
    let ddy = py + 0.5 - cy;
    let dist = sqrt(ddx * ddx + ddy * ddy);
    var weight = 0.0;
    if (s.feather == 0.0) {
      if (dist <= radius) {
        weight = 1.0;
      }
    } else {
      let inner = radius * (1.0 - s.feather);
      if (dist <= inner) {
        weight = 1.0;
      } else if (dist <= radius) {
        weight = 1.0 - (dist - inner) / (radius - inner);
      }
    }
    if (weight == 0.0) {
      continue;
    }
    let alpha = weight * s.opacity;
    if (alpha == 0.0) {
      continue;
    }
    let sx = clamp(i32(roundi(px + s.offset_dx * w)), 0, max_x);
    let sy = clamp(i32(roundi(py + s.offset_dy * h)), 0, max_y);
    let sample = textureLoad(input_tex, vec2<u32>(u32(sx), u32(sy)), 0);
    r = roundi(r * (1.0 - alpha) + byte_from_norm(sample.r) * alpha);
    g = roundi(g * (1.0 - alpha) + byte_from_norm(sample.g) * alpha);
    b = roundi(b * (1.0 - alpha) + byte_from_norm(sample.b) * alpha);
  }
  return vec4<f32>(norm_from_byte(clamp(r, 0.0, 255.0)),
                   norm_from_byte(clamp(g, 0.0, 255.0)),
                   norm_from_byte(clamp(b, 0.0, 255.0)),
                   src.a);
}
"#
);

/// Bind group layout for [`SPOT_HEAL_STAGE_SRC`]: storage params (0) + input (1).
pub fn create_spot_heal_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-spot-heal-bgl"),
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

/// Build the spot-heal render pipeline for `target_format`.
pub fn create_spot_heal_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    let layout = create_spot_heal_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-spot-heal-pl"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-spot-heal-shader"),
        source: wgpu::ShaderSource::Wgsl(SPOT_HEAL_STAGE_SRC.into()),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumina-gpu-spot-heal-pipeline"),
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

/// Allocate the spot-heal storage buffer for `count` spots.
pub fn create_spot_heal_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-spot-heal-params"),
        size: spot_heal_buffer_size(count),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Bind group for one spot-heal pass over `input_view`.
pub fn create_spot_heal_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-spot-heal-bindgroup"),
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

    /// GPU-RENDER-PARITY-1 stage 2: the storage struct must match the WGSL
    /// `array<f32, 182>` layout exactly (32-byte scalar header, then the fine
    /// kernel at offset 0 and the coarse kernel at offset `MAX_SHARPEN_TAPS`).
    #[test]
    fn sharpen_params_layout_matches_wgsl() {
        assert_eq!(std::mem::size_of::<SharpenParams>(), 32 + 2 * 91 * 4);
        assert_eq!(std::mem::offset_of!(SharpenParams, kernels), 32);
    }

    /// The schema's `radius` maximum (10.0) at `effective_scale = 1.0` yields
    /// the largest supported kernel: `sigma = 15`, `r = 45`, 91 taps.
    #[test]
    fn sharpen_kernel_radius_matches_oracle_bounds() {
        // `(radius * 1.5).max(0.5)` at radius = 10.
        let (r, kernel) = sharpen_kernel((10.0f32 * 1.5).max(0.5));
        assert_eq!(r, 45);
        assert_eq!(kernel.len(), MAX_SHARPEN_TAPS);
        // The kernel is normalized.
        let sum: f32 = kernel.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "kernel sum {sum}");

        // Smallest radius still produces a usable kernel (sigma >= 0.5 → r=2).
        let (r, _) = sharpen_kernel((0.1f32 * 0.5).max(0.5));
        assert_eq!(r, 2);
    }

    /// Vignette radius bounds reproduce the oracle's `r_min`/`r_max` for a
    /// symmetric frame; the centre pixel always maps to radius 0.
    #[test]
    fn vignette_radius_bounds_are_symmetric_and_centre_zero() {
        let (r_min, r_max) = vignette_radius_bounds(5, 5, 1.0);
        assert_eq!(r_min, 0.0, "centre pixel must be radius 0");
        assert!(r_max > 0.0);
        // Circular roundness=1: corner radius ≈ sqrt(2).
        let expected = (2.0f32 * (2.0f32 / 2.0).powi(2)).sqrt();
        assert!((r_max - expected).abs() < 1e-6, "{r_max} vs {expected}");
    }

    /// Grain seed folding mirrors `apply_grain`: deterministic, dimension- and
    /// seed-sensitive.
    #[test]
    fn grain_effective_seed_is_deterministic_and_sensitive() {
        assert_eq!(
            grain_effective_seed(7, 64, 64),
            grain_effective_seed(7, 64, 64)
        );
        assert_ne!(
            grain_effective_seed(7, 64, 64),
            grain_effective_seed(8, 64, 64)
        );
        assert_ne!(
            grain_effective_seed(7, 64, 64),
            grain_effective_seed(7, 32, 64)
        );
    }

    /// Presence/identity gates match the oracle's early returns exactly, so a
    /// present-but-neutral stage is a no-op on the GPU (byte identity).
    #[test]
    fn detail_stage_identity_gates_match_oracle() {
        use lumina_sidecar::{Grain, NoiseReduction, Sharpening, Vignette};
        let zero_nr = NoiseReduction {
            version: 1,
            luminance: 0.0,
            color: 0.0,
        };
        assert!(!NoiseParams::needs_stage(&zero_nr));
        assert!(NoiseParams::needs_stage(&NoiseReduction {
            luminance: 0.2,
            ..zero_nr
        }));

        let zero_sharp = Sharpening {
            version: 1,
            amount: 0.0,
            radius: 1.0,
            detail: 0.5,
            masking: 0.0,
        };
        assert!(!SharpenParams::needs_stage(&zero_sharp));
        assert!(SharpenParams::needs_stage(&Sharpening {
            amount: 1.0,
            ..zero_sharp
        }));

        // Effects: the caller gates on the individual amounts (the oracle
        // early-returns per effect, not per container).
        assert_eq!(
            VignetteParams::from_vignette(
                &Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.5,
                    roundness: 1.0,
                    feather: 0.5,
                },
                8,
                8
            )
            .amount,
            0.0
        );
        assert_eq!(
            GrainParams::from_grain(
                &Grain {
                    version: 1,
                    amount: 0.0,
                    size: 0.0,
                    roughness: 0.5,
                    seed: 0,
                },
                8,
                8
            )
            .amount,
            0.0
        );
    }

    /// Red-eye storage struct must match the WGSL `array<vec4<f32>, 32>` +
    /// `array<f32, 32>` layout exactly (4-word header, then the region block).
    #[test]
    fn red_eye_params_layout_matches_wgsl() {
        assert_eq!(std::mem::size_of::<RedEyeParams>(), 16 + 32 * 16 + 32 * 4);
        assert_eq!(std::mem::offset_of!(RedEyeParams, regions), 16);
        assert_eq!(std::mem::offset_of!(RedEyeParams, darken), 16 + 32 * 16);
    }

    /// The red-eye identity gate matches `apply_red_eye`'s early returns: an
    /// empty region list or an all-zero-strength list is a no-op.
    #[test]
    fn red_eye_identity_gate_matches_oracle() {
        use lumina_sidecar::{RedEyeCorrection, RedEyeRegion};
        let neutral = RedEyeRegion {
            id: "re-1".into(),
            x: 0.5,
            y: 0.5,
            radius: 0.2,
            desaturate: 0.0,
            darken: 0.0,
        };
        assert!(!RedEyeParams::needs_stage(&RedEyeCorrection {
            version: 1,
            regions: vec![],
        }));
        assert!(!RedEyeParams::needs_stage(&RedEyeCorrection {
            version: 1,
            regions: vec![neutral.clone()],
        }));

        let mut active = neutral;
        active.desaturate = 0.5;
        assert!(RedEyeParams::needs_stage(&RedEyeCorrection {
            version: 1,
            regions: vec![active],
        }));

        let params = RedEyeParams::from_red_eye(&RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.25,
                y: 0.75,
                radius: 0.3,
                desaturate: 0.8,
                darken: 0.4,
            }],
        });
        assert_eq!(params.count, 1);
        assert_eq!(params.regions[0], [0.25, 0.75, 0.3, 0.8]);
        assert_eq!(params.darken[0], 0.4);
    }
}
