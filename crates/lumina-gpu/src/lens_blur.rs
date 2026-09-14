//! G-05 Lens Blur GPU pass (GPU-RENDER-PARITY-1, lens-blur wave).
//!
//! A direct port of `lumina_core::lens_blur::apply_lens_blur`, run **after** the
//! geometry chain (`lens → perspective → CA → crop → rotation → mirror`) and
//! before masks/output — the exact slot the CPU oracle
//! (`render_frame_from_base`) uses:
//!
//! ```text
//! … → Crop(F-093) → LensBlur(G-05) → Masks → Output
//! ```
//!
//! Semantics mirrored operation-for-operation:
//!
//! * output dimensions are preserved (clamp-to-edge sampling, no resampling);
//! * `radius = round(blur_amount * 16)` (`> 0` else identity);
//! * the per-pixel sharpness weight is `focal_weight(depth, near, far)`
//!   (`0` inside the sharp band, linear ramps outside);
//! * `depth` comes from the deterministic focus-rect heuristic (depth `0`
//!   inside the focus rectangle, otherwise the diagonal-normalized distance to
//!   its edge) or from a caller-supplied external depth plane
//!   (`recipe.lens_blur.depth_artifact`);
//! * `bokeh_blur` is the integer-mask convolution (`round` disk, `2:1`
//!   ellipse, hexagon) with **clamp-to-edge** sampling and integer accumulation,
//!   normalized by the tap count and rounded half-away-from-zero;
//! * `output = round(src * (1 - w) + blurred * w)`, RGB only, alpha preserved.
//!
//! The kernel taps are computed on the host with the same integer loop as
//! `bokeh_kernel` and uploaded as a storage buffer, so the shader only ever
//! samples the oracle's exact tap set. The `f64`→`f32` residual of the final
//! lerp is the only difference and is gated by `tests/parity.rs`.

use lumina_sidecar::{BokehShape, LensBlur};

/// Maximum blur radius in pixels (`blur_amount == 1`). Mirrors
/// `lumina_core::lens_blur::LENS_BLUR_MAX_RADIUS`.
pub(crate) const LENS_BLUR_MAX_RADIUS: f32 = 16.0;

/// External depth-plane texture format: one finite `0..=1` value per pixel.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// One integer kernel tap; matches the WGSL `Tap` struct byte-for-byte.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Tap {
    pub dx: i32,
    pub dy: i32,
}

/// Storage-buffer header for the tap list (16-byte aligned count).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TapsHeader {
    pub count: u32,
    pub _pad: [u32; 3],
}

/// Uniform params for the pass. `_pad` keeps the block at a 16-byte multiple.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LensBlurParams {
    pub focus_x: f32,
    pub focus_y: f32,
    pub focus_w: f32,
    pub focus_h: f32,
    pub focal_near: f32,
    pub focal_far: f32,
    /// `1.0` when a caller-supplied external depth plane is bound, else `0.0`
    /// (focus-rect heuristic).
    pub use_external: f32,
    pub _pad: f32,
}

impl LensBlurParams {
    pub(crate) fn from_blur(blur: &LensBlur, use_external: bool) -> Self {
        Self {
            focus_x: blur.focus_rect.x,
            focus_y: blur.focus_rect.y,
            focus_w: blur.focus_rect.width,
            focus_h: blur.focus_rect.height,
            focal_near: blur.focal_near,
            focal_far: blur.focal_far,
            use_external: if use_external { 1.0 } else { 0.0 },
            _pad: 0.0,
        }
    }
}

/// Whether the oracle runs the blur at all: `apply_lens_blur` returns identity
/// for a disabled or zero-amount stage. This predicate intentionally does **not**
/// include the `radius > 0` / all-weights-zero short circuits — those are
/// checked separately so the external-depth requirement keeps the oracle's
/// ordering (a referenced-but-missing depth plane aborts even when the radius
/// would round to zero).
pub(crate) fn stage_active(blur: &LensBlur) -> bool {
    blur.enabled && blur.blur_amount != 0.0
}

/// `radius = round(blur_amount * 16)` px, exactly like the oracle.
pub(crate) fn radius_for(blur_amount: f32) -> i32 {
    (blur_amount * LENS_BLUR_MAX_RADIUS).round() as i32
}

/// Whether the oracle's `weights.iter().all(|w| *w == 0.0)` short circuit holds.
///
/// `focal_weight` is `0` everywhere in `0..=1` iff `focal_near <= 0` **and**
/// `focal_far >= 1` (otherwise depth `0` or `1` lands on a non-zero ramp), so
/// the GPU can skip the pass and keep the input bytes untouched.
pub(crate) fn has_no_effect(blur: &LensBlur) -> bool {
    blur.focal_near <= 0.0 && blur.focal_far >= 1.0
}

/// Integer kernel offsets for a bokeh shape at `radius >= 1`. Verbatim port of
/// `lumina_core::lens_blur::bokeh_kernel`.
pub(crate) fn bokeh_kernel(shape: BokehShape, radius: i32) -> Vec<(i32, i32)> {
    let mut taps = Vec::new();
    let r = radius as f32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let inside = match shape {
                BokehShape::Round => (dx * dx + dy * dy) as f32 <= r * r,
                BokehShape::Elliptical => (dx * dx + 4 * dy * dy) as f32 <= r * r,
                BokehShape::Hexagonal => (dx.abs() + dy.abs() + (dx + dy).abs()) as f32 <= 2.0 * r,
            };
            if inside {
                taps.push((dx, dy));
            }
        }
    }
    taps
}

/// Byte size of the tap storage buffer for `count` taps.
pub(crate) fn taps_buffer_size(count: usize) -> u64 {
    (std::mem::size_of::<TapsHeader>() + count * std::mem::size_of::<Tap>()) as u64
}

/// Encode a tap list into the GPU storage buffer bytes (header + entries).
pub(crate) fn taps_bytes(taps: &[(i32, i32)]) -> Vec<u8> {
    let header = TapsHeader {
        count: taps.len() as u32,
        _pad: [0; 3],
    };
    let mut bytes = bytemuck::bytes_of(&header).to_vec();
    for &(dx, dy) in taps {
        bytes.extend_from_slice(bytemuck::bytes_of(&Tap { dx, dy }));
    }
    bytes
}

/// WGSL for the lens-blur pass — a direct port of `apply_lens_blur`.
pub(crate) const LENS_BLUR_STAGE_SRC: &str = r#"
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

fn byte_from_norm(x : f32) -> f32 {
  return roundi(clamp(x, 0.0, 1.0) * 255.0);
}

struct LensBlurParams {
  focus_x : f32,
  focus_y : f32,
  focus_w : f32,
  focus_h : f32,
  focal_near : f32,
  focal_far : f32,
  use_external : f32,
  pad : f32,
};

struct Tap {
  dx : i32,
  dy : i32,
};

struct TapList {
  count : u32,
  pad0 : u32,
  pad1 : u32,
  pad2 : u32,
  taps : array<Tap>,
};

@group(0) @binding(0) var<uniform> params : LensBlurParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
@group(0) @binding(2) var depth_tex : texture_2d<f32>;
@group(0) @binding(3) var<storage, read> tap_list : TapList;

// Mirrors `lumina_core::lens_blur::focal_weight`.
fn focal_weight(depth : f32, near : f32, far : f32) -> f32 {
  if (depth < near) {
    if (near <= 0.0) {
      return 0.0;
    }
    return clamp((near - depth) / near, 0.0, 1.0);
  } else if (depth > far) {
    if (far >= 1.0) {
      return 0.0;
    }
    return clamp((depth - far) / (1.0 - far), 0.0, 1.0);
  }
  return 0.0;
}

@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<u32>(frag.xy);
  let src = textureLoad(input_tex, coord, 0);
  let dims = textureDimensions(input_tex);
  let w = f32(dims.x);
  let h = f32(dims.y);
  var depth : f32;
  if (params.use_external != 0.0) {
    depth = textureLoad(depth_tex, coord, 0).r;
  } else {
    // Deterministic heuristic: 0 inside the focus rect, else the
    // diagonal-normalized distance to its edge.
    let nx = frag.x / w;
    let ny = frag.y / h;
    let dx = max(max(params.focus_x - nx, 0.0), nx - (params.focus_x + params.focus_w));
    let dy = max(max(params.focus_y - ny, 0.0), ny - (params.focus_y + params.focus_h));
    let diag = max(sqrt(w * w + h * h), 1.0);
    depth = clamp(sqrt((dx * w) * (dx * w) + (dy * h) * (dy * h)) / diag, 0.0, 1.0);
  }
  let weight = focal_weight(depth, params.focal_near, params.focal_far);
  if (weight == 0.0) {
    return src;
  }
  let count = tap_list.count;
  var acc = vec3<u32>(0u, 0u, 0u);
  let max_x = i32(dims.x) - 1;
  let max_y = i32(dims.y) - 1;
  let cx = i32(coord.x);
  let cy = i32(coord.y);
  for (var i = 0u; i < count; i = i + 1u) {
    let tap = tap_list.taps[i];
    let sx = clamp(cx + tap.dx, 0, max_x);
    let sy = clamp(cy + tap.dy, 0, max_y);
    let sample = textureLoad(input_tex, vec2<i32>(sx, sy), 0);
    acc = acc + vec3<u32>(
      u32(byte_from_norm(sample.r)),
      u32(byte_from_norm(sample.g)),
      u32(byte_from_norm(sample.b))
    );
  }
  let n = f32(count);
  let blurred = vec3<f32>(
    roundi(f32(acc.r) / n),
    roundi(f32(acc.g) / n),
    roundi(f32(acc.b) / n)
  );
  let r = roundi(byte_from_norm(src.r) * (1.0 - weight) + blurred.r * weight);
  let g = roundi(byte_from_norm(src.g) * (1.0 - weight) + blurred.g * weight);
  let b = roundi(byte_from_norm(src.b) * (1.0 - weight) + blurred.b * weight);
  return vec4<f32>(
    clamp(r, 0.0, 255.0) / 255.0,
    clamp(g, 0.0, 255.0) / 255.0,
    clamp(b, 0.0, 255.0) / 255.0,
    src.a
  );
}
"#;

/// Compiled lens-blur pipeline plus the 1×1 dummy depth texture bound when the
/// recipe uses the heuristic (the shader's `use_external == 0` branch never
/// samples it, but the binding slot must still be filled).
pub(crate) struct LensBlurPipelineState {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
    #[allow(dead_code)]
    pub dummy_depth: wgpu::Texture,
    pub dummy_depth_view: wgpu::TextureView,
}

pub(crate) fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-lens-blur-bgl"),
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
                // `R32Float` is not filterable without the optional
                // `FLOAT32_FILTERABLE` feature; `textureLoad` needs no sampler.
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

/// Create an `R32Float` depth-plane texture. `COPY_DST` for the caller-supplied
/// plane upload, `TEXTURE_BINDING` for the pass.
pub(crate) fn create_depth_texture(
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
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    })
}

pub(crate) fn build_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<LensBlurPipelineState, crate::GpuError> {
    let layout = create_bind_group_layout(device);
    let pipeline = crate::stages::build_simple_pipeline(
        device,
        "lens-blur",
        LENS_BLUR_STAGE_SRC,
        &layout,
        crate::shaders::RGBA8_FORMAT,
    )?;
    let dummy_depth = create_depth_texture(device, 1, 1, "lumina-gpu-lens-blur-dummy-depth");
    // A 1×1 zeroed plane: never sampled (the heuristic branch is taken), but it
    // keeps the bind group layout identical for both depth sources.
    let zero = 0.0f32.to_le_bytes();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &dummy_depth,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &zero,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let dummy_depth_view = dummy_depth.create_view(&wgpu::TextureViewDescriptor::default());
    Ok(LensBlurPipelineState {
        pipeline,
        layout,
        dummy_depth,
        dummy_depth_view,
    })
}

pub(crate) fn create_params_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-lens-blur-params"),
        size: std::mem::size_of::<LensBlurParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn create_taps_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-lens-blur-taps"),
        size: taps_buffer_size(count),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
    taps_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-lens-blur-bindgroup"),
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
                resource: wgpu::BindingResource::TextureView(depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: taps_buffer.as_entire_binding(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_blur() -> LensBlur {
        LensBlur {
            version: 1,
            enabled: true,
            focus_rect: lumina_sidecar::FocusRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.0,
            focal_far: 0.05,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        }
    }

    /// The ported `bokeh_kernel` must produce the oracle's exact integer tap
    /// sets (shape area ordering: disk ⊃ ellipse ⊃ hexagon at the same radius).
    #[test]
    fn bokeh_kernel_ports_oracle_tap_sets() {
        let radius = radius_for(0.5);
        assert_eq!(radius, 8);
        let round = bokeh_kernel(BokehShape::Round, radius);
        let elliptical = bokeh_kernel(BokehShape::Elliptical, radius);
        let hexagonal = bokeh_kernel(BokehShape::Hexagonal, radius);
        // The disk is the full lattice square minus the corners: every tap with
        // dx²+dy² ≤ 64. Spot-check the extremes the shape test defines.
        assert!(round.contains(&(0, 0)));
        assert!(round.contains(&(8, 0)));
        assert!(!round.contains(&(8, 8)), "corner is outside the disk");
        // The ellipse is 2:1 (x radius `r`, y radius `r/2`).
        assert!(elliptical.contains(&(0, 4)));
        assert!(!elliptical.contains(&(0, 8)));
        assert!(!elliptical.contains(&(8, 8)));
        // Shapes are distinct, and the disk is the largest set.
        assert_ne!(round, elliptical);
        assert_ne!(elliptical, hexagonal);
        assert!(round.len() > elliptical.len());
    }

    /// Taps at the schema maximum (`blur_amount == 1`) stay within the
    /// documented radius and kernel size; the buffer is sized to match.
    #[test]
    fn max_radius_kernel_is_bounded() {
        let radius = radius_for(1.0);
        assert_eq!(radius, 16);
        let taps = bokeh_kernel(BokehShape::Round, radius);
        assert!(taps.len() > 700 && taps.len() < 1090, "{}", taps.len());
        assert_eq!(taps_buffer_size(taps.len()), 16 + (taps.len() * 8) as u64);
        let bytes = taps_bytes(&taps);
        assert_eq!(bytes.len() as u64, taps_buffer_size(taps.len()));
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            taps.len() as u32
        );
    }

    /// Guards the WGSL source against keyword/syntax regressions: an invalid
    /// shader would produce a pipeline that silently writes nothing, which the
    /// parity test would catch as divergent pixels — this makes the root cause
    /// explicit.
    #[test]
    fn lens_blur_pipeline_validates() {
        let ctx = match crate::GpuContext::new() {
            Ok(ctx) => ctx,
            Err(err) => {
                eprintln!("lens-blur pipeline validation skipped (no GPU context: {err})");
                return;
            }
        };
        if !ctx.is_available() {
            eprintln!("lens-blur pipeline validation skipped (no GPU adapter)");
            return;
        }
        let device = ctx.device().expect("device");
        let queue = ctx.queue().expect("queue");
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _pipeline = build_pipeline(device, queue);
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "lens-blur shader must validate: {err:?}");
    }

    /// `stage_active` / `has_no_effect` mirror `apply_lens_blur`'s early
    /// returns; a disabled or zero-amount stage is identity regardless of the
    /// focus band.
    #[test]
    fn activity_predicates_match_oracle_short_circuits() {
        assert!(!stage_active(&LensBlur {
            enabled: false,
            ..round_blur()
        }));
        assert!(!stage_active(&LensBlur {
            blur_amount: 0.0,
            ..round_blur()
        }));
        assert!(stage_active(&round_blur()));
        // focal_near == 0 && focal_far == 1 makes every weight zero.
        assert!(has_no_effect(&LensBlur {
            focal_near: 0.0,
            focal_far: 1.0,
            ..round_blur()
        }));
        assert!(!has_no_effect(&LensBlur {
            focal_near: 0.0,
            focal_far: 0.05,
            ..round_blur()
        }));
        assert!(!has_no_effect(&LensBlur {
            focal_near: 0.4,
            focal_far: 1.0,
            ..round_blur()
        }));
    }
}
