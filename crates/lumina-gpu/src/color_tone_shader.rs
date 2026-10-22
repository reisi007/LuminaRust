//! WGSL source of the base color/tone pass (file-size ratchet §8,
//! SPLICE-GPU-LIB-91). Extracted verbatim from `lib.rs`; the pass wiring
//! stays in `lib.rs`.

/// Real WGSL color/tone shader for the color/tone stage.
///
/// Mirrors the integer-rounded per-channel math of
/// `lumina-core::apply_channel_lut_adjustments` (white balance → exposure →
/// contrast → shadows → highlights → whites → blacks) in the sRGB-encoded
/// RGBA8 byte domain. The fullscreen-triangle vertex stage covers the target;
/// the fragment stage samples the uploaded source texture at its exact texel
/// centre (nearest sampler) and applies the tone mapping, writing the graded
/// RGBA8 result.
///
/// Rounding uses `floor(x + 0.5)` (registered as `roundi`) to match Rust's
/// `f64::round` (ties away from zero) for the non-negative values this kernel
/// produces, keeping the GPU path within the golden-image tolerance of the CPU
/// oracle.
#[cfg(feature = "gpu")]
pub(crate) const SHADER_SRC: &str = r#"
struct Params {
  exposure : f32,
  contrast : f32,
  highlights : f32,
  shadows : f32,
  whites : f32,
  blacks : f32,
  wb_temperature : f32,
  wb_tint : f32,
  vibrance : f32,
  saturation : f32,
  pad0 : f32,
  pad1 : f32,
  pad2 : f32,
  pad3 : f32,
  pad4 : f32,
  pad5 : f32,
};
@group(0) @binding(0) var<uniform> params : Params;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
@group(0) @binding(2) var input_samp : sampler;

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

// Round half away from zero, matching Rust's `f64::round` for non-negative x.
fn roundi(x : f32) -> f32 {
  return floor(x + 0.5);
}

fn clamp01(x : f32) -> f32 {
  return clamp(x, 0.0, 1.0);
}

// Per-channel tone mapping, identical in order and rounding to
// `lumina-core::apply_channel_lut_adjustments`. `in_norm` is the source channel
// normalised to [0,1]; `wb_gain` is this channel's white-balance multiplier.
fn tone_channel(in_norm : f32, wb_gain : f32) -> f32 {
  var v : f32 = in_norm * 255.0;
  // 1) White balance.
  v = roundi(v * wb_gain);
  v = clamp(v, 0.0, 255.0);
  // 2) Exposure (multiplier = 2^exposure).
  v = roundi(v * pow(2.0, params.exposure));
  v = clamp(v, 0.0, 255.0);
  // 3) Contrast (factor = 1 + c).
  v = roundi((v - 128.0) * (1.0 + params.contrast) + 128.0);
  v = clamp(v, 0.0, 255.0);
  // 4) Shadows.
  if (params.shadows != 0.0) {
    let x = v / 255.0;
    let w = pow(max(0.0, (0.5 - x) / 0.5), 2.0);
    v = roundi(clamp01(x + params.shadows * w * 0.25) * 255.0);
  }
  // 5) Highlights.
  if (params.highlights != 0.0) {
    let x = v / 255.0;
    let w = pow(max(0.0, (x - 0.5) / 0.5), 2.0);
    v = roundi(clamp01(x + params.highlights * w * 0.25) * 255.0);
  }
  // 6) Whites.
  if (params.whites != 0.0) {
    let x = v / 255.0;
    let w = max(0.0, (x - 0.5) / 0.5);
    v = roundi(clamp01(x + params.whites * w * 0.25) * 255.0);
  }
  // 7) Blacks.
  if (params.blacks != 0.0) {
    let x = v / 255.0;
    let w = max(0.0, (0.5 - x) / 0.5);
    v = roundi(clamp01(x - params.blacks * w * 0.25) * 255.0);
  }
  return v;
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let dims = vec2<f32>(textureDimensions(input_tex));
  let uv = frag_coord.xy / dims;
  let src = textureSampleLevel(input_tex, input_samp, uv, 0.0);

  let warmth = (params.wb_temperature - 6500.0) / 5500.0;
  let wb_r = 1.0 - warmth * 0.35;
  let wb_g = 1.0 - params.wb_tint * 0.20;
  let wb_b = 1.0 + warmth * 0.35;

  let r = tone_channel(src.r, wb_r);
  let g = tone_channel(src.g, wb_g);
  let b = tone_channel(src.b, wb_b);

  return vec4<f32>(r / 255.0, g / 255.0, b / 255.0, src.a);
}
"#;
