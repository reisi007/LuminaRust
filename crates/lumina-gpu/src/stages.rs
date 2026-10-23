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

#[cfg(test)]
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

pub mod color;
pub mod dehaze;
pub mod dog;
pub mod grain;
pub mod noise;
pub mod red_eye;
pub mod sharpening;
pub mod spot_heal;
pub mod vignette;

pub use color::*;
pub use dehaze::*;
pub use dog::*;
pub use grain::*;
pub use noise::*;
pub use red_eye::*;
pub use sharpening::*;
pub use spot_heal::*;
pub use vignette::*;

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
