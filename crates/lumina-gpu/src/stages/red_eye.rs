use bytemuck::Zeroable;

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
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
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
