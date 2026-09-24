//! R5-MASKVIS-25 overlay tint contract: the GPU present path must consume the
//! exact session tint, with no private default or hardcoded accent color.

use crate::gpu_util::{overlay_uniforms, readback_texture};
use crate::shaders;
use crate::GpuContext;
use lumina_core::ImageFrame;
use lumina_sidecar::EditRecipe;

#[test]
fn present_uses_non_default_overlay_color_and_matches_cpu_mix() {
    // Deliberately unlike both the historical GUI default (red) and the former
    // GPU-only blue. This conversion assertion runs even when no adapter exists.
    let requested = [17, 129, 231];
    let uniforms = overlay_uniforms(requested);
    assert_eq!(uniforms.color[0], f32::from(requested[0]) / 255.0);
    assert_eq!(uniforms.color[1], f32::from(requested[1]) / 255.0);
    assert_eq!(uniforms.color[2], f32::from(requested[2]) / 255.0);
    assert_eq!(uniforms.color[3], 0.45);

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(error) => {
            eprintln!("overlay-color uniform passed; adapter-backed pixel check skipped: {error}");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("overlay-color uniform passed; adapter-backed pixel check skipped: no adapter");
        return;
    }

    const WIDTH: u32 = 4;
    const HEIGHT: u32 = 1;
    const BASE: [u8; 4] = [40, 80, 120, 255];
    let frame = ImageFrame::new(WIDTH, HEIGHT, BASE.repeat((WIDTH * HEIGHT) as usize))
        .expect("constant test frame");
    ctx.render_to_vram(&frame, &EditRecipe::default())
        .expect("neutral VRAM render");
    ctx.upload_mask_plane(WIDTH, HEIGHT, &vec![u16::MAX; (WIDTH * HEIGHT) as usize])
        .expect("full-coverage mask");

    let resources = ctx
        .resources
        .as_ref()
        .expect("available context has wgpu resources");
    let destination = shaders::create_output_texture(
        &resources.device,
        WIDTH,
        HEIGHT,
        "overlay-color-test-destination",
    );
    ctx.copy_vram_to_texture(&destination, requested)
        .expect("overlay present");
    let actual = readback_texture(resources, &destination, WIDTH, HEIGHT)
        .expect("overlay destination readback");

    // The CPU painter's normalized mix is base*(1-strength) + tint*strength.
    // GPU unorm conversion can differ by at most one byte; a hardcoded tint
    // misses this oracle by tens of channels.
    for actual in actual.pixels.as_chunks::<4>().0 {
        for channel in 0..3 {
            let expected = (f32::from(BASE[channel]) * 0.55 + f32::from(requested[channel]) * 0.45)
                .round() as u8;
            assert!(
                actual[channel].abs_diff(expected) <= 1,
                "GPU channel {channel} must match the CPU mix: actual {}, expected {expected}",
                actual[channel]
            );
        }
        assert_eq!(actual[3], 255);
    }
}
