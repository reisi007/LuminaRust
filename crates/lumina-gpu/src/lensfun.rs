//! GPU Lensfun corrector pass (GPU-LENSFUN-PARITY-1).
//!
//! Lensfun supplies arbitrary distortion / TCA / vignetting models as a
//! per-pixel destination→source coordinate function plus a per-pixel,
//! per-channel vignetting gain. Rather than reimplement those models in WGSL,
//! the CPU precomputes a [`lumina_core::LensfunMap`] once per source and
//! dimensions (`LensfunMap::from_corrector`, using the exact row-batch wrappers
//! the CPU oracle uses) and this pass performs a plain inverse-bilinear resample
//! over the source, mirroring `lumina_core::apply_lens`'s corrector branch:
//!
//! ```text
//! r = sample(src, red_coord)   * gain.r
//! g = sample(src, green_coord) * gain.g
//! b = sample(src, blue_coord)  * gain.b
//! a = sample(src, green_coord)          (no gain on alpha)
//! ```
//!
//! The pass is dimension-preserving (like the corrector itself) and slots into
//! the geometry chain exactly where the manual lens step sits
//! (`Lensfun → [auto-fill] → Perspective → CA → [expand] → Crop`). The map is a
//! caller-bound render-context input (like the depth plane / As-Shot gains),
//! validated loudly on bind and required to match the frame dimensions.
//!
//! The resample uses the same byte-domain inverse-bilinear `bilinear255` helper
//! as the manual geometry passes, so the CPU map-vs-GPU parity is asserted in
//! `tests/parity.rs` (§F-043 maxAbsDiff/PSNR).

use bytemuck::{Pod, Zeroable};
use lumina_core::LensfunMap;

use crate::shaders;
use crate::GpuError;

/// WGSL for the Lensfun resample pass.
pub(crate) const LENSFUN_STAGE_SRC: &str = r#"
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

// Oracle `sample` on one channel: bounds check + bilinear in the 0..=255 byte
// domain, far neighbour clamped to the frame edge.
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

struct LensfunParams {
  has_tca : u32,
  pad0 : u32,
  pad1 : u32,
  pad2 : u32,
};

@group(0) @binding(0) var<uniform> params : LensfunParams;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
@group(0) @binding(2) var coord_tex : texture_2d<f32>;
@group(0) @binding(3) var blue_tex : texture_2d<f32>;
@group(0) @binding(4) var gain_tex : texture_2d<f32>;

@fragment
fn fs_main(@builtin(position) frag : vec4<f32>) -> @location(0) vec4<f32> {
  let coord = vec2<i32>(i32(frag.x), i32(frag.y));
  let c = textureLoad(coord_tex, coord, 0);
  let g = textureLoad(gain_tex, coord, 0);
  var bx = c.z;
  var by = c.w;
  if (params.has_tca != 0u) {
    let b = textureLoad(blue_tex, coord, 0);
    bx = b.x;
    by = b.y;
  }
  let sr = bilinear255(input_tex, vec2<f32>(c.x, c.y));
  let sg = bilinear255(input_tex, vec2<f32>(c.z, c.w));
  let sb = bilinear255(input_tex, vec2<f32>(bx, by));
  return vec4<f32>(byte1(sr.r * g.x), byte1(sg.g * g.y), byte1(sb.b * g.z), byte1(sg.a));
}
"#;

/// Tightly-packed coordinate format (RGBA: red.x, red.y, green.x, green.y).
const COORD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
/// Per-channel TCA blue coordinate format (RG: blue.x, blue.y).
const BLUE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;
/// Per-channel vignetting gain format (RGBA: gain.r, gain.g, gain.b, 0).
const GAIN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// Uniform block carrying the TCA flag.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct LensfunParams {
    pub has_tca: u32,
    pub _pad: [u32; 3],
}

impl LensfunParams {
    fn new(has_tca: bool) -> Self {
        Self {
            has_tca: u32::from(has_tca),
            _pad: [0; 3],
        }
    }
}

/// Compiled Lensfun pass: pipeline, layout and the 1×1 blue placeholder bound
/// when the map has no TCA plane (the shader's `has_tca == 0` branch never
/// samples it, but the binding slot must stay filled).
pub(crate) struct LensfunPipelineState {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: wgpu::BindGroupLayout,
    pub params: wgpu::Buffer,
    #[allow(dead_code)]
    pub dummy_blue: wgpu::Texture,
    pub dummy_blue_view: wgpu::TextureView,
}

/// Per-pixel GPU mirror of a [`LensfunMap`]: precomputed coordinate/gain
/// textures (uploaded once on bind) plus the owned map for the CPU-side
/// routing guards. `textures` is `None` only when no adapter is bound.
pub(crate) struct LensfunMapGpu {
    pub map: LensfunMap,
    textures: Option<LensfunTextures>,
}

struct LensfunTextures {
    #[allow(dead_code)]
    coord: wgpu::Texture,
    coord_view: wgpu::TextureView,
    #[allow(dead_code)]
    blue: Option<(wgpu::Texture, wgpu::TextureView)>,
    #[allow(dead_code)]
    gain: wgpu::Texture,
    gain_view: wgpu::TextureView,
}

pub(crate) fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-lensfun-bgl"),
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
            // The coordinate/gain textures are `Rgba32Float`/`Rg32Float`, which
            // are not filterable without the optional `FLOAT32_FILTERABLE`
            // feature; `textureLoad` needs no sampler anyway.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
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
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

fn create_map_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
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
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    })
}

pub(crate) fn build_lensfun_pipeline(
    device: &wgpu::Device,
) -> Result<LensfunPipelineState, GpuError> {
    let layout = create_bind_group_layout(device);
    let pipeline = crate::stages::build_simple_pipeline(
        device,
        "geometry-lensfun",
        LENSFUN_STAGE_SRC,
        &layout,
        shaders::RGBA8_FORMAT,
    )?;
    let params = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-lensfun-params"),
        size: std::mem::size_of::<LensfunParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let dummy_blue = create_map_texture(device, 1, 1, BLUE_FORMAT, "lumina-gpu-lensfun-dummy-blue");
    let dummy_blue_view = dummy_blue.create_view(&wgpu::TextureViewDescriptor::default());
    Ok(LensfunPipelineState {
        pipeline,
        layout,
        params,
        dummy_blue,
        dummy_blue_view,
    })
}

/// Validate and upload a caller-bound [`LensfunMap`]. `None` clears the
/// binding. An invalid map (dimension/length/TCA-pairing/non-finite values) is
/// rejected with [`GpuError::Core`] and **no** state is stored — never a silent
/// clamp. Without an adapter the map is still stored (no textures) so the
/// routing guards can see it; the render entries then report the missing
/// adapter.
pub(crate) fn bind_map(
    resources: Option<&crate::GpuResources>,
    map: Option<&LensfunMap>,
) -> Result<Option<LensfunMapGpu>, GpuError> {
    let Some(map) = map else {
        return Ok(None);
    };
    map.validate().map_err(GpuError::Core)?;
    let textures = match resources {
        Some(resources) => Some(upload_map(resources, map)?),
        None => None,
    };
    Ok(Some(LensfunMapGpu {
        map: map.clone(),
        textures,
    }))
}

fn upload_map(
    resources: &crate::GpuResources,
    map: &LensfunMap,
) -> Result<LensfunTextures, GpuError> {
    let (width, height) = (map.width, map.height);
    let has_tca = map.has_tca();
    // Pack the CPU planes into the GPU's interleaved formats.
    let mut coord: Vec<[f32; 4]> = Vec::with_capacity(map.green.len());
    let mut gain: Vec<[f32; 4]> = Vec::with_capacity(map.gain.len());
    for (index, green) in map.green.iter().enumerate() {
        let red = map.red.as_ref().map_or(*green, |plane| plane[index]);
        coord.push([red[0], red[1], green[0], green[1]]);
        let g = map.gain[index];
        gain.push([g[0], g[1], g[2], 0.0]);
    }
    let coord_texture = create_map_texture(
        &resources.device,
        width,
        height,
        COORD_FORMAT,
        "lumina-gpu-lensfun-coord",
    );
    resources.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &coord_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&coord),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 16),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let coord_view = coord_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let blue = if has_tca {
        let plane = map.blue.as_ref().expect("validated TCA pairing");
        let blue_texture = create_map_texture(
            &resources.device,
            width,
            height,
            BLUE_FORMAT,
            "lumina-gpu-lensfun-blue",
        );
        resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &blue_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(plane),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = blue_texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some((blue_texture, view))
    } else {
        None
    };
    let gain_texture = create_map_texture(
        &resources.device,
        width,
        height,
        GAIN_FORMAT,
        "lumina-gpu-lensfun-gain",
    );
    resources.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &gain_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&gain),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 16),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let gain_view = gain_texture.create_view(&wgpu::TextureViewDescriptor::default());
    Ok(LensfunTextures {
        coord: coord_texture,
        coord_view,
        blue,
        gain: gain_texture,
        gain_view,
    })
}

/// Encode the Lensfun resample pass from `input_view` into `dst`.
pub(crate) fn encode_lensfun_pass(
    resources: &crate::GpuResources,
    state: &LensfunPipelineState,
    map: &LensfunMapGpu,
    input_view: &wgpu::TextureView,
    dst: &wgpu::TextureView,
    encoder: &mut wgpu::CommandEncoder,
) -> Result<(), GpuError> {
    let Some(textures) = map.textures.as_ref() else {
        return Err(GpuError::RenderFailed(
            "Lensfun map has no GPU textures (no adapter bound)".into(),
        ));
    };
    resources.queue.write_buffer(
        &state.params,
        0,
        bytemuck::bytes_of(&LensfunParams::new(map.map.has_tca())),
    );
    let blue_view = textures
        .blue
        .as_ref()
        .map(|(_, view)| view)
        .unwrap_or(&state.dummy_blue_view);
    let bind = resources
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lumina-gpu-lensfun-bindgroup"),
            layout: &state.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&textures.coord_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(blue_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&textures.gain_view),
                },
            ],
        });
    crate::gpu_util::encode_fullscreen_pass(encoder, &state.pipeline, &bind, dst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lensfun_pipeline_validates() {
        let ctx = match crate::GpuContext::new() {
            Ok(ctx) => ctx,
            Err(err) => {
                eprintln!("lensfun pipeline validation skipped (no GPU context: {err})");
                return;
            }
        };
        if !ctx.is_available() {
            eprintln!("lensfun pipeline validation skipped (no GPU adapter)");
            return;
        }
        let device = ctx.device().expect("device");
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _pipeline = build_lensfun_pipeline(device);
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "lensfun shader must validate: {err:?}");
    }

    #[test]
    fn identity_map_is_accepted_and_has_no_tca() {
        let map = LensfunMap::new(
            2,
            2,
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            None,
            None,
            vec![[1.0; 3]; 4],
            false,
        )
        .expect("valid map");
        assert!(!map.has_tca());
    }
}
