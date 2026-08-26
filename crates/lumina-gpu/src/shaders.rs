//! GPU shader stage: uniform buffers, 32³ 3D LUT bake, FP16 framebuffer helper.
//!
//! This module owns the data the color/tone fragment shader consumes. The actual
//! WGSL shader body is filled in by a later subagent; the structures here (and
//! the identity-LUT stub in [`bake_3d_lut`]) define the contract that the shader
//! and the [`super::GpuContext`] pipeline must honour.

use lumina_sidecar::EditRecipe;

/// Uniform-buffer layout for the color/tone shader stage.
///
/// Mirrors the slider parameters from [`EditRecipe::adjustments`]. `#[repr(C)]`
/// + `bytemuck::{Pod, Zeroable}` so it can be uploaded directly into a
/// `wgpu::Buffer` bound as a uniform. Padded to 64 bytes (16 × f32) to satisfy
/// uniform-buffer alignment.
#[allow(clippy::doc_lazy_continuation)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub wb_temperature: f32,
    pub wb_tint: f32,
    pub vibrance: f32,
    pub saturation: f32,
    /// Padding to 64 bytes (multiple of 16) for uniform-buffer alignment.
    pub _pad: [f32; 6],
}

impl Uniforms {
    /// Build the uniform block from a recipe.
    ///
    /// Missing keys default to `0.0` (identity for every Lumina adjustment),
    /// with one deliberate exception: `wb_temperature` defaults to the neutral
    /// colour temperature **6500 K** (not `0.0`). This mirrors the CPU oracle in
    /// `lumina-core::apply_channel_lut_adjustments`, which uses
    /// `temperature.unwrap_or(6500.0)` and `tint.unwrap_or(0.0)` — so an absent
    /// WB key yields `warmth = 0` and therefore the identity gain triple
    /// `[1, 1, 1]`, while a present key (with or without its sibling) is graded
    /// exactly as the CPU does. Keeping the GPU path *always applying* WB with
    /// the same neutral defaults is therefore byte-equivalent to the CPU's
    /// conditional `Option<[f64; 3]>` while avoiding a separate presence flag in
    /// the 64-byte uniform block.
    pub fn from_recipe(recipe: &EditRecipe) -> Self {
        let get = |key: &str| recipe.adjustments.get(key).copied().unwrap_or(0.0) as f32;
        Self {
            exposure: get("exposure"),
            contrast: get("contrast"),
            highlights: get("highlights"),
            shadows: get("shadows"),
            whites: get("whites"),
            blacks: get("blacks"),
            wb_temperature: recipe
                .adjustments
                .get("wb_temperature")
                .copied()
                .unwrap_or(6500.0) as f32,
            wb_tint: get("wb_tint"),
            vibrance: get("vibrance"),
            saturation: get("saturation"),
            _pad: [0.0; 6],
        }
    }
}

/// Edge length of the baked 3D LUT (32³ nodes, per the DAG plan).
pub const LUT_DIM: usize = 32;
/// Total node count of the 32³ LUT.
pub const LUT_SIZE: usize = LUT_DIM * LUT_DIM * LUT_DIM;

/// A 32×32×32 RGBA 3D lookup table.
///
/// Indexed `data[r + g*32 + b*32*32]`; each entry is `(r, g, b, a)` in `0..=1`.
/// [`bake_3d_lut`] produces the graded table; the shader samples it trilinearly.
#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct Lut32x32x32 {
    pub data: [[f32; 4]; LUT_SIZE],
}

impl Lut32x32x32 {
    /// Identity LUT: node `(r, g, b)` maps to `(r/31, g/31, b/31, 1)`.
    pub fn identity() -> Self {
        let mut data = [[0.0f32; 4]; LUT_SIZE];
        let mut i = 0usize;
        for b in 0..LUT_DIM {
            for g in 0..LUT_DIM {
                for r in 0..LUT_DIM {
                    data[i] = [r as f32 / 31.0, g as f32 / 31.0, b as f32 / 31.0, 1.0];
                    i += 1;
                }
            }
        }
        debug_assert_eq!(i, LUT_SIZE);
        Self { data }
    }
}

/// Bake the recipe into a 32³ 3D LUT.
///
/// **Stub:** returns the identity LUT and logs. The real implementation maps the
/// tone/color-grade portion of `recipe` through the CPU reference kernel
/// (mirroring `lumina-core`) once the shader stage is wired, so the GPU and CPU
/// paths stay byte-consistent.
pub fn bake_3d_lut(recipe: &EditRecipe) -> Lut32x32x32 {
    log::debug!(
        "bake_3d_lut: building identity 32³ LUT stub (recipe '{}' adjustments ignored for now)",
        recipe.recipe_version
    );
    Lut32x32x32::identity()
}

/// Output/working color format for the byte-consistent color/tone pass:
/// 8-bit unorm RGBA. The shader runs the same integer-rounded math as the
/// `lumina-core` CPU oracle directly in the sRGB-encoded byte domain, so the
/// render target is plain `Rgba8Unorm` (no automatic sRGB↔linear conversion).
/// The FP16 helper below stays available for a later linear working space.
pub const RGBA8_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Working color format for the linear pipeline: 16-bit float RGBA (kept for a
/// later linear working-space / HDR intermediate).
pub const FP16_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Round a byte-row size up to the 256-byte alignment `wgpu` requires for
/// `copy_texture_to_buffer` (and `copy_buffer_to_texture`) source/target
/// strides. A 64px-wide RGBA8 row is exactly 256 bytes, but other widths need
/// padding that the readback copy must account for.
pub fn aligned_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
    // Ceil to multiple of 256.
    unpadded.div_ceil(256) * 256
}

/// Create the upload texture that carries the decoded source frame into the
/// color/tone shader. Format is `RGBA8_FORMAT` (raw sRGB-encoded bytes, no
/// sRGB→linear decode on sample), with `TEXTURE_BINDING` (sampled) and
/// `COPY_DST` (uploaded via `queue.write_texture`).
pub fn create_input_texture(
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
        format: RGBA8_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    })
}

/// Create the nearest-neighbour, clamp-to-edge sampler used to read the input
/// texture. Nearest filtering (plus exact texel-centre UVs from
/// `@builtin(position)`) makes the sample bit-for-bit the stored byte, which is
/// what the integer-rounded tone math expects.
pub fn create_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// Create the RGBA8 render target the color/tone pass draws into. It is a
/// `RENDER_ATTACHMENT` (drawn to), `COPY_SRC` (read back to the CPU `Frame` for
/// export/CLI paths) **and** `TEXTURE_BINDING` (sampled by the mask-overlay
/// present pass, GUI-60FPS-1 — the tone result never has to leave VRAM).
pub fn create_output_texture(
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
        format: RGBA8_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
    })
}

/// Create the VRAM brush-mask texture (GUI-60FPS-1): one `u16` coverage value
/// per source pixel (`R16Uint`, matching the sidecar's uint16 mask tiles).
/// Updated per dirty 512² tile via [`super::GpuContext::upload_mask_tile`]
/// (`queue.write_texture` subregion) — never re-uploaded wholesale in the
/// brush hot path.
pub fn create_mask_texture(
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
        format: MASK_FORMAT,
        // COPY_SRC lets tests (and future diagnostics) read the plane back.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
    })
}

/// Mask coverage texture format: one `uint16` channel per pixel (identical
/// value domain to `.lumina.zdata` mask tiles / `MaskPlane`).
///
/// **Why `R16Uint` instead of `R16Unorm`** (GPU-STAGE-1): the unorm-16 family
/// requires the optional `TEXTURE_FORMAT_16BIT_NORM` feature, which neither
/// this crate's nor eframe's shared devices enable by default — creating the
/// VRAM mask texture would fail validation on such devices (found by the
/// GPU-STAGE-1 equivalence test). Integer formats need no extra features,
/// sample as the exact stored `u16` value (`0..=65535`) so thresholds stay
/// exact, and only require a *non-filtering* sampler
/// ([`create_non_filtering_sampler`]).
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

/// Nearest-neighbour, clamp-to-edge **non-filtering** sampler for integer
/// mask/region textures ([`MASK_FORMAT`]). Integer textures reject filtering
/// samplers, so every pass that samples them binds its own non-filtering
/// sampler alongside the filtering one used for colour textures.
pub fn create_non_filtering_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// Upload a single 512² (or edge-clipped) mask tile into the VRAM mask texture
/// without touching any other tile. `tile_data` is little-endian `u16` coverage
/// bytes, row-major, length `tile_w * tile_h * 2`. Only the dirty tile is
/// written (GUI-60FPS-1 hot path).
pub fn write_mask_tile(
    queue: &wgpu::Queue,
    mask_texture: &wgpu::Texture,
    tile_x: u32,
    tile_y: u32,
    tile_w: u32,
    tile_h: u32,
    tile_data: &[u8],
) {
    debug_assert_eq!(tile_data.len(), (tile_w * tile_h * 2) as usize);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: mask_texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: tile_x,
                y: tile_y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        tile_data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(tile_w * 2),
            rows_per_image: Some(tile_h),
        },
        wgpu::Extent3d {
            width: tile_w,
            height: tile_h,
            depth_or_array_layers: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Source-action stage (GPU-STAGE-1)
// ---------------------------------------------------------------------------

use super::MAX_SOURCE_ACTIONS;

/// Uniform block for the source-action stage: how many of the unrolled
/// `MAX_SOURCE_ACTIONS` slot pairs are actually bound. Padded to 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SourceActionUniforms {
    /// Number of bound `(region, replacement)` pairs (`0..=MAX_SOURCE_ACTIONS`).
    pub count: u32,
    pub _pad: [u32; 3],
}

/// WGSL for the source-action compositing stage (GPU-STAGE-1).
///
/// Regions are `R16Uint` integer textures read with `textureLoad` (integer
/// textures have no sampling functions), using the fragment position as the
/// exact texel coordinate — the pass renders 1:1 into a same-size target, so
/// the 50% threshold is an **exact** integer comparison against `32768`,
/// identical to the CPU oracle's u16 comparison.
pub const SOURCE_ACTION_STAGE_SRC: &str = r#"
struct SourceActionParams {
  count : u32,
  pad0 : u32,
  pad1 : u32,
  pad2 : u32,
};
@group(0) @binding(0) var<uniform> params : SourceActionParams;
@group(0) @binding(1) var base_tex : texture_2d<f32>;
@group(0) @binding(2) var region_0 : texture_2d<u32>;
@group(0) @binding(3) var replacement_0 : texture_2d<f32>;
@group(0) @binding(4) var region_1 : texture_2d<u32>;
@group(0) @binding(5) var replacement_1 : texture_2d<f32>;
@group(0) @binding(6) var region_2 : texture_2d<u32>;
@group(0) @binding(7) var replacement_2 : texture_2d<f32>;
@group(0) @binding(8) var region_3 : texture_2d<u32>;
@group(0) @binding(9) var replacement_3 : texture_2d<f32>;
@group(0) @binding(10) var region_4 : texture_2d<u32>;
@group(0) @binding(11) var replacement_4 : texture_2d<f32>;
@group(0) @binding(12) var region_5 : texture_2d<u32>;
@group(0) @binding(13) var replacement_5 : texture_2d<f32>;
@group(0) @binding(14) var region_6 : texture_2d<u32>;
@group(0) @binding(15) var replacement_6 : texture_2d<f32>;

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

// Composite one artifact into `dst`: where the region coverage reaches the
// exact 50% threshold (`>= 32768` in the u16 domain), the replacement pixel
// (RGBA incl. alpha) replaces dst; otherwise dst stays. All reads are
// `textureLoad`s at the fragment's own texel, so every value is a pure byte
// copy and the threshold an integer compare.
fn composite_slot(
  dst : vec4<f32>,
  frag_coord : vec4<f32>,
  region : texture_2d<u32>,
  replacement : texture_2d<f32>,
) -> vec4<f32> {
  let coords = vec2<u32>(frag_coord.xy);
  let m = textureLoad(region, coords, 0).r;
  if (m >= 32768u) {
    return textureLoad(replacement, coords, 0);
  }
  return dst;
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let coords = vec2<u32>(frag_coord.xy);
  var out = textureLoad(base_tex, coords, 0);

  if (params.count > 0u) {
    out = composite_slot(out, frag_coord, region_0, replacement_0);
  }
  if (params.count > 1u) {
    out = composite_slot(out, frag_coord, region_1, replacement_1);
  }
  if (params.count > 2u) {
    out = composite_slot(out, frag_coord, region_2, replacement_2);
  }
  if (params.count > 3u) {
    out = composite_slot(out, frag_coord, region_3, replacement_3);
  }
  if (params.count > 4u) {
    out = composite_slot(out, frag_coord, region_4, replacement_4);
  }
  if (params.count > 5u) {
    out = composite_slot(out, frag_coord, region_5, replacement_5);
  }
  if (params.count > 6u) {
    out = composite_slot(out, frag_coord, region_6, replacement_6);
  }
  return out;
}
"#;

/// Bind group layout for the source-action stage: uniform (0), base texture
/// (1), then `MAX_SOURCE_ACTIONS` `(region, replacement)` pairs starting at
/// binding 2. No samplers: all reads are exact `textureLoad`s.
pub fn create_source_action_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let mut entries = Vec::with_capacity(2 + MAX_SOURCE_ACTIONS * 2);
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    // Binding 1: the float base texture; then per slot one integer region
    // texture and one float replacement texture. All reads are `textureLoad`s,
    // so no samplers exist in this layout at all.
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 1,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    });
    for i in 0..MAX_SOURCE_ACTIONS {
        let base = 2 + i * 2;
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: base as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: (base + 1) as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-sourceaction-bgl"),
        entries: &entries,
    })
}

/// Build the source-action render pipeline for the given target format
/// ([`RGBA8_FORMAT`] on the interactive path).
pub fn create_source_action_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    let layout = create_source_action_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-sourceaction-pl"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-sourceaction-shader"),
        source: wgpu::ShaderSource::Wgsl(SOURCE_ACTION_STAGE_SRC.into()),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumina-gpu-sourceaction-pipeline"),
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

/// Allocate the source-action uniform buffer ([`SourceActionUniforms`], 16 bytes).
pub fn create_source_action_uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-sourceaction-uniforms"),
        size: std::mem::size_of::<SourceActionUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload the source-action uniform block via the queue.
pub fn write_source_action_uniforms(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    uniforms: &SourceActionUniforms,
) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(uniforms));
}

/// Create the bind group for the source-action pass. `region_views`/
/// `replacement_views` must both carry exactly `count` views (the caller has
/// validated dimensions); unused slots up to [`MAX_SOURCE_ACTIONS`] are filled
/// with the *base* view so the bindings stay valid without being read
/// (`count` guards every slot access in the shader).
#[allow(clippy::too_many_arguments)]
pub fn create_source_action_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    base_view: &wgpu::TextureView,
    region_views: &[&wgpu::TextureView],
    replacement_views: &[&wgpu::TextureView],
    count: u32,
) -> wgpu::BindGroup {
    assert_eq!(
        region_views.len(),
        replacement_views.len(),
        "region/replacement view counts must match"
    );
    assert!(
        count as usize == region_views.len() && count as usize <= MAX_SOURCE_ACTIONS,
        "bind group needs one pair per counted action"
    );
    let mut entries = Vec::with_capacity(2 + MAX_SOURCE_ACTIONS * 2);
    entries.push(wgpu::BindGroupEntry {
        binding: 0,
        resource: uniform_buffer.as_entire_binding(),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: 1,
        resource: wgpu::BindingResource::TextureView(base_view),
    });
    // Inactive region slots must still satisfy the layout's *Uint* sample
    // type — the float `base_view` cannot fill them. A tiny retained filler
    // texture does; the bind group keeps its resources alive internally.
    let filler_region = create_region_texture(device, 1, 1, "lumina-gpu-sa-region-filler");
    let filler_view = filler_region.create_view(&wgpu::TextureViewDescriptor::default());
    for i in 0..MAX_SOURCE_ACTIONS {
        let base = 2 + i * 2;
        let active = i < count as usize;
        let region = if active {
            region_views[i]
        } else {
            &filler_view
        };
        let replacement = if active {
            replacement_views[i]
        } else {
            base_view
        };
        entries.push(wgpu::BindGroupEntry {
            binding: base as u32,
            resource: wgpu::BindingResource::TextureView(region),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: (base + 1) as u32,
            resource: wgpu::BindingResource::TextureView(replacement),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-sourceaction-bindgroup"),
        layout,
        entries: &entries,
    })
}

/// Create the `R16Uint` upload texture carrying a source-action *region* plane
/// (same value domain as `.lumina.zdata` mask tiles / `MaskPlane`; see
/// [`MASK_FORMAT`] for why this is uint, not unorm).
pub fn create_region_texture(
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
        format: MASK_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    })
}
/// Upload a full `u16` plane (row-major, little-endian) into an R16Uint
/// texture. Used for source-action regions and evaluated mask planes.
pub fn write_u16_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    data: &[u16],
) {
    debug_assert_eq!(data.len(), (width * height) as usize);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(data),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 2),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Uniform block for the mask-overlay present pass.
///
/// `color` is the overlay tint (RGB) with the blend strength in alpha
/// (`0.0..=0.45` mirrors the CPU overlay). `#[repr(C)]` + Pod so it uploads
/// directly; padded to 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OverlayUniforms {
    /// RGB tint + A = blend strength.
    pub color: [f32; 4],
}

/// WGSL for the mask-overlay present pass (GUI-60FPS-1).
///
/// Draws a fullscreen triangle into an *external* target (egui screen area or
/// swapchain view), sampling the tone-rendered image (VRAM-resident, no CPU
/// readback) and the VRAM brush-mask texture, and mixes the accent tint over
/// the image by `mask × strength`. This replaces any CPU composite of mask and
/// preview on the GPU path.
///
/// The mask texture is [`MASK_FORMAT`] (`R16Uint`): samples carry the raw
/// `u16` value (`0..=65535`) and are normalised here. Integer textures have
/// no sampling functions, so the mask is read with an exact `textureLoad` at
/// the fragment's own texel (the present pass renders 1:1 into a same-size
/// target — enforced by `copy_vram_to_texture`).
pub const MASK_OVERLAY_SRC: &str = r#"
struct OverlayParams {
  color_strength : vec4<f32>,
};
@group(0) @binding(0) var<uniform> params : OverlayParams;
@group(0) @binding(1) var color_tex : texture_2d<f32>;
@group(0) @binding(2) var mask_tex : texture_2d<u32>;
@group(0) @binding(3) var input_samp : sampler;

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

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let dims = vec2<f32>(textureDimensions(color_tex));
  let uv = frag_coord.xy / dims;
  let base = textureSampleLevel(color_tex, input_samp, uv, 0.0);
  let coords = vec2<u32>(frag_coord.xy);
  let mask_raw = textureLoad(mask_tex, coords, 0).r;
  let m = clamp(f32(mask_raw) / 65535.0 * params.color_strength.a, 0.0, 1.0);
  let rgb = mix(base.rgb, params.color_strength.rgb, m);
  return vec4<f32>(rgb, base.a);
}
"#;

/// Bind group layout for [`MASK_OVERLAY_SRC`]: uniform (0), color texture (1),
/// uint mask texture (2), filtering colour sampler (3).
pub fn create_overlay_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-overlay-bgl"),
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
            // R16Uint mask plane: integer sample type, not filterable.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Build the mask-overlay render pipeline for the given *external* target
/// format (the egui/swapchain surface format on the GUI side, or
/// [`RGBA8_FORMAT`] in tests).
pub fn create_overlay_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, super::GpuError> {
    let layout = create_overlay_bind_group_layout(device);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-overlay-pl"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-overlay-shader"),
        source: wgpu::ShaderSource::Wgsl(MASK_OVERLAY_SRC.into()),
    });
    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lumina-gpu-overlay-pipeline"),
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

/// Create the bind group for the overlay pass from its parts. The mask
/// texture is read via `textureLoad` in the shader, so only the filtering
/// colour sampler is bound.
pub fn create_overlay_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    color_view: &wgpu::TextureView,
    mask_view: &wgpu::TextureView,
    color_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-overlay-bindgroup"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(color_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(color_sampler),
            },
        ],
    })
}

/// Allocate the overlay uniform buffer ([`OverlayUniforms`], 16 bytes).
pub fn create_overlay_uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-overlay-uniforms"),
        size: std::mem::size_of::<OverlayUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload `uniforms` into the overlay uniform buffer via the queue.
pub fn write_overlay_uniforms(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    uniforms: &OverlayUniforms,
) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(uniforms));
}

/// Create the staging buffer the rendered RGBA8 target is copied into before
/// `map_async` readback. Sized to the 256-byte-aligned stride × height, with
/// `COPY_DST | MAP_READ`.
pub fn create_readback_buffer(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> wgpu::Buffer {
    let bytes_per_row = aligned_bytes_per_row(width);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (bytes_per_row * height.max(1)) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    })
}

/// Create the bind group that binds the uniform block (binding 0), the input
/// texture (binding 1) and the sampler (binding 2) for the color/tone pass.
/// Built per render because the input texture/sampler change with each frame.
pub fn create_color_tone_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    input_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lumina-gpu-color-tone-bindgroup"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(input_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Create an FP16 (`RGBA16Float`) texture usable as a render attachment, sampled
/// binding and copy source — the intermediate framebuffer for the color/tone pass.
pub fn create_fp16_framebuffer(
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
        format: FP16_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
    })
}

/// Allocate a uniform buffer sized for [`Uniforms`].
pub fn create_uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lumina-gpu-uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Upload `uniforms` into `buffer` via the queue.
pub fn write_uniforms(queue: &wgpu::Queue, buffer: &wgpu::Buffer, uniforms: &Uniforms) {
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(uniforms));
}
