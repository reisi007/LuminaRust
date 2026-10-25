//! GPU-UPLOAD-IDENTITY-92 regression: a cached input texture must follow the
//! **frame instance**, not the pixel content and not the allocation address.
//!
//! The adapter-free truth table for the decision itself lives next to the
//! function (`crates/lumina-gpu/src/vram_pool/upload_identity.rs` → tests). This
//! file pins the same statement on a real adapter: two frames of **different
//! content but equal length** are rendered one after the other through the same
//! `GpuContext`; the second render must show the second frame. A stale upload
//! (the former `(pixels_ptr, pixels_len)` key) would show the first frame's
//! pixels instead.
//!
//! Reference is a fresh context rendering the second frame (no prior upload), so
//! the assertion is exact byte equality between two GPU runs and carries no
//! tolerance. The default recipe is the identity render on both paths.

mod support;

use lumina_core::ImageFrame;
use lumina_gpu::{Frame, GpuContext};
use lumina_sidecar::EditRecipe;

fn frame(pixels: [u8; 16]) -> ImageFrame {
    ImageFrame::new(2, 2, pixels.to_vec()).expect("valid 2x2 RGBA8 frame")
}

fn first_frame() -> ImageFrame {
    frame([
        10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
    ])
}

fn second_frame() -> ImageFrame {
    frame([
        200, 10, 20, 255, 180, 30, 40, 255, 160, 50, 60, 255, 140, 70, 80, 255,
    ])
}

fn assert_second_render(rendered: &Frame, second: &ImageFrame, first_render: &Frame) {
    assert_eq!(
        (rendered.width, rendered.height),
        (second.width(), second.height())
    );
    assert_ne!(
        rendered.pixels, first_render.pixels,
        "the second render must not repeat the cached first frame"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn readback_path_renders_the_second_frame_with_equal_length() {
    let context = GpuContext::new().expect("GPU context initializes");
    assert!(support::require_adapter(&context));
    let recipe = EditRecipe::default();
    let second = second_frame();

    let first_render = context
        .render_with_gpu(&first_frame(), &recipe)
        .expect("first frame renders");
    let second_render = context
        .render_with_gpu(&second, &recipe)
        .expect("second frame renders");
    let reference = GpuContext::new()
        .expect("fresh GPU context initializes")
        .render_with_gpu(&second, &recipe)
        .expect("reference render succeeds");

    assert_second_render(&second_render, &second, &first_render);
    assert_eq!(
        second_render.pixels, reference.pixels,
        "a reused context must match a fresh context for the same frame"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn vram_path_renders_the_second_frame_with_equal_length() {
    let context = GpuContext::new().expect("GPU context initializes");
    assert!(support::require_adapter(&context));
    let recipe = EditRecipe::default();
    let second = second_frame();

    let first_render = context
        .render_to_vram(&first_frame(), &recipe)
        .and_then(|()| context.readback_output_frame())
        .expect("first frame renders into VRAM and reads back");
    context
        .render_to_vram(&second, &recipe)
        .expect("second frame renders into VRAM");
    let second_render = context
        .readback_output_frame()
        .expect("VRAM output reads back");

    let reference_context = GpuContext::new().expect("fresh GPU context initializes");
    reference_context
        .render_to_vram(&second, &recipe)
        .expect("reference render into VRAM succeeds");
    let reference = reference_context
        .readback_output_frame()
        .expect("reference VRAM output reads back");

    assert_second_render(&second_render, &second, &first_render);
    assert_eq!(
        second_render.pixels, reference.pixels,
        "a reused context must match a fresh context for the same frame"
    );
}
