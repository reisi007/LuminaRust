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
) -> Result<wgpu::RenderPipeline, crate::GpuError> {
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
