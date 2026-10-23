use bytemuck::Zeroable;
use lumina_sidecar::EditRecipe;

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
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
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
