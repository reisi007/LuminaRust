//! GPU-RENDER-MASK-19-ADAPTER: real GUI production-path proof for an applied
//! mask-local adjustment at the wgpu present.
//!
//! Unlike `kittest_mask_local` (layout-only S2 goldens), this test uses a
//! deterministic full image, a valid brush-prompt matte and a non-neutral local
//! exposure edit. A real pointer-down drag reaches the production scheduler and
//! `render_draft_tick`; the same `LuminaApp::ui` frame then runs
//! `update_texture` and paints the registered VRAM output. It is ignored because
//! it requires a local Metal/Vulkan adapter, but opting in without one is a hard
//! failure.

#![cfg(feature = "gpu")]

use eframe::egui::{Pos2, Rect};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{attach_wgpu_render_state, LuminaApp, Module, SECTION_COUNT, SECTION_MASKING};
use lumina_sidecar::{BrushMark, BrushMarkSign, MaskPrompt, PromptTransform};
use std::path::{Path, PathBuf};

#[path = "kittest_decode_support/mod.rs"]
mod decode_support;

use decode_support::{pump_until_ready, DecodeSettled, SETTLE_DEADLINE};

const SOURCE_WIDTH: u32 = 128;
const SOURCE_HEIGHT: u32 = 96;
// Keep the actual local Exposure slider inside the interactable UI viewport.
// The Masking panel is one long column; its local basic sliders sit below the
// mask/tool controls (around y=1,630 with this layout), not in the first 720 px.
const VIEWPORT: [f32; 2] = [1024.0, 1800.0];
const FRAME_DT: f32 = 1.0 / 60.0;
const SETTLE_FRAMES: usize = 20;
const MASK_INSIDE_SOURCE_PIXEL: (u32, u32) = (46, 48);
const MASK_OUTSIDE_SOURCE_PIXEL: (u32, u32) = (110, 48);
const SAMPLE_RADIUS: u32 = 1;

struct PhotoPixels {
    dimensions: [u32; 2],
    rgba: Vec<u8>,
}

fn deterministic_source_png() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((SOURCE_WIDTH * SOURCE_HEIGHT * 4) as usize);
    for y in 0..SOURCE_HEIGHT {
        for x in 0..SOURCE_WIDTH {
            let checker = ((x / 8 + y / 8) % 2) as u8 * 23;
            let red = ((x * 211 / (SOURCE_WIDTH - 1)) as u8).saturating_add(checker);
            let green = ((y * 197 / (SOURCE_HEIGHT - 1)) as u8).saturating_add(checker);
            let blue = (((x + y) * 173 / (SOURCE_WIDTH + SOURCE_HEIGHT - 2)) as u8)
                .saturating_add(checker);
            pixels.extend_from_slice(&[red, green, blue, 255]);
        }
    }
    ImageFrame::new(SOURCE_WIDTH, SOURCE_HEIGHT, pixels)
        .expect("deterministic source frame")
        .encode(ImageFileFormat::Png)
        .expect("deterministic source PNG")
}

fn write_source(dir: &tempfile::TempDir, png: &[u8]) -> PathBuf {
    let path = dir.path().join("gpu-mask-present-source.png");
    std::fs::write(&path, png).expect("write deterministic source");
    path
}

fn build_harness(path: &Path, attach_gpu: bool) -> Harness<'static, LuminaApp> {
    let mut harness = Harness::builder()
        .with_size(VIEWPORT)
        .with_step_dt(FRAME_DT)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = LuminaApp::new(cc.egui_ctx.clone());
            if attach_gpu {
                // Match the native `run_native` construction: lumina-gpu and
                // egui's renderer share the exact Device/Queue being presented.
                attach_wgpu_render_state(&mut app, cc.wgpu_render_state.clone());
            }
            app
        });
    if attach_gpu {
        assert!(
            harness.state().gpu_adapter_available(),
            "GPU-RENDER-MASK-19-ADAPTER requires a real wgpu adapter; opt-in may not skip"
        );
    }
    harness.state_mut().set_module(Module::Develop);
    pump_until_ready(
        &mut harness,
        path,
        DecodeSettled::new("the deterministic source has decoded", |app| {
            !app.decode_pending() && app.preview_generation() > 0
        }),
        SETTLE_DEADLINE,
    );
    assert!(
        harness.state().error().is_none(),
        "the source must decode without an error: {:?}",
        harness.state().error()
    );
    for index in 0..SECTION_COUNT {
        harness
            .state_mut()
            .set_section_open(index, index == SECTION_MASKING);
    }
    harness
}

fn brush_prompt_marks() -> Vec<BrushMark> {
    vec![BrushMark {
        x: 0.36,
        y: 0.5,
        radius: 0.28,
        sign: BrushMarkSign::Positive,
        softness: 0.2,
        flow: 0.9,
    }]
}

fn brush_prompt_matte() -> lumina_core::MaskPlane {
    let prompt = MaskPrompt::Brush {
        marks: brush_prompt_marks(),
        resolution: (SOURCE_WIDTH, SOURCE_HEIGHT),
        transformation: PromptTransform::default(),
    };
    lumina_core::masks::rasterize_prompt(&prompt, SOURCE_WIDTH, SOURCE_HEIGHT)
        .expect("rasterize the committed brush prompt for sample coordinates")
}

fn add_applied_prompt_mask(harness: &mut Harness<'_, LuminaApp>) -> String {
    let app = harness.state_mut();
    let id = app
        .create_mask("GPU present local subject")
        .expect("create the mask layer");
    app.commit_brush_stroke(brush_prompt_marks())
        .expect("commit a valid, deterministic brush-prompt matte");
    assert_eq!(app.selected_mask_id(), Some(id.as_str()));
    id
}

fn photo_pixels(harness: &mut Harness<'_, LuminaApp>) -> PhotoPixels {
    let rect = harness
        .state()
        .preview_screen_rect()
        .expect("the real Develop UI painted its preview");
    let image = harness
        .render()
        .expect("wgpu harness renders the presented frame");
    let (width, height) = image.dimensions();
    let min_x = (rect.min.x.max(0.0).floor() as u32).min(width);
    let max_x = (rect.max.x.ceil() as u32).min(width);
    let min_y = (rect.min.y.max(0.0).floor() as u32).min(height);
    let max_y = (rect.max.y.ceil() as u32).min(height);
    assert!(
        max_x > min_x && max_y > min_y,
        "preview photo rect {rect:?} must contain pixels in the {width}x{height} frame"
    );
    let mut rgba = Vec::with_capacity(((max_x - min_x) * (max_y - min_y) * 4) as usize);
    for y in min_y..max_y {
        for x in min_x..max_x {
            rgba.extend_from_slice(&image.get_pixel(x, y).0);
        }
    }
    PhotoPixels {
        dimensions: [max_x - min_x, max_y - min_y],
        rgba,
    }
}

fn photo_region_at_source_pixel(photo: &PhotoPixels, source_pixel: (u32, u32)) -> Vec<[u8; 4]> {
    let center_x = (((source_pixel.0 as f32 + 0.5) / SOURCE_WIDTH as f32)
        * photo.dimensions[0] as f32)
        .floor() as u32;
    let center_y = (((source_pixel.1 as f32 + 0.5) / SOURCE_HEIGHT as f32)
        * photo.dimensions[1] as f32)
        .floor() as u32;
    let min_x = center_x.saturating_sub(SAMPLE_RADIUS);
    let min_y = center_y.saturating_sub(SAMPLE_RADIUS);
    let max_x = (center_x + SAMPLE_RADIUS + 1).min(photo.dimensions[0]);
    let max_y = (center_y + SAMPLE_RADIUS + 1).min(photo.dimensions[1]);
    let mut region = Vec::with_capacity(((max_x - min_x) * (max_y - min_y)) as usize);
    for y in min_y..max_y {
        for x in min_x..max_x {
            let offset = ((y * photo.dimensions[0] + x) * 4) as usize;
            region.push(
                photo.rgba[offset..offset + 4]
                    .try_into()
                    .expect("complete RGBA pixel"),
            );
        }
    }
    region
}

fn assert_masked_exposure_is_spatial(
    masked: &PhotoPixels,
    mask_control: &PhotoPixels,
    mask_hidden: &PhotoPixels,
    matte: &lumina_core::MaskPlane,
) {
    assert_eq!(masked.dimensions, mask_hidden.dimensions);
    assert_eq!(masked.dimensions, mask_control.dimensions);
    let alpha_at = |(x, y): (u32, u32)| matte.values[(y * matte.width + x) as usize];
    let inside_alpha = alpha_at(MASK_INSIDE_SOURCE_PIXEL);
    let outside_alpha = alpha_at(MASK_OUTSIDE_SOURCE_PIXEL);
    assert!(
        inside_alpha > 0,
        "inner sample {:?} must be covered by the committed brush matte, alpha={inside_alpha}",
        MASK_INSIDE_SOURCE_PIXEL
    );
    assert_eq!(
        outside_alpha, 0,
        "outer sample {:?} must be outside the committed brush matte",
        MASK_OUTSIDE_SOURCE_PIXEL
    );

    let inside_masked = photo_region_at_source_pixel(masked, MASK_INSIDE_SOURCE_PIXEL);
    let inside_control = photo_region_at_source_pixel(mask_control, MASK_INSIDE_SOURCE_PIXEL);
    let inside_hidden = photo_region_at_source_pixel(mask_hidden, MASK_INSIDE_SOURCE_PIXEL);
    assert_eq!(inside_masked.len(), inside_control.len());
    assert_eq!(inside_masked.len(), inside_hidden.len());
    let changed_from_control = inside_masked
        .iter()
        .zip(&inside_control)
        .filter(|(left, right)| left[..3] != right[..3])
        .count();
    let changed_inside = inside_masked
        .iter()
        .zip(&inside_hidden)
        .filter(|(left, right)| left[..3] != right[..3])
        .count();
    eprintln!(
        "local-mask-present[spatial]: inside source={:?} alpha={inside_alpha}, local-vs-control={changed_from_control}/{}, local-vs-hidden={changed_inside}/{}",
        MASK_INSIDE_SOURCE_PIXEL,
        inside_masked.len(),
        inside_masked.len()
    );
    assert!(
        changed_from_control > 0,
        "local Exposure must change inner pixels beyond the identical visible-mask overlay"
    );
    assert!(
        changed_inside > 0,
        "the non-neutral local Exposure must change pixels inside the applied matte"
    );

    let outside_masked = photo_region_at_source_pixel(masked, MASK_OUTSIDE_SOURCE_PIXEL);
    let outside_hidden = photo_region_at_source_pixel(mask_hidden, MASK_OUTSIDE_SOURCE_PIXEL);
    assert_eq!(outside_masked.len(), outside_hidden.len());
    let changed_outside = outside_masked
        .iter()
        .zip(&outside_hidden)
        .filter(|(left, right)| left[..3] != right[..3])
        .count();
    eprintln!(
        "local-mask-present[spatial]: outside source={:?} alpha={outside_alpha}, differing RGB pixels={changed_outside}/{}",
        MASK_OUTSIDE_SOURCE_PIXEL,
        outside_masked.len()
    );
    assert_eq!(
        changed_outside, 0,
        "local Exposure must leave pixels outside the applied matte byte-identical"
    );
}

fn local_exposure_rail(harness: &Harness<'_, LuminaApp>) -> Rect {
    let mut rows: Vec<Rect> = harness
        .query_all_by_label("Exposure")
        .map(|node| node.rect())
        .collect();
    rows.sort_by(|a, b| a.center().y.total_cmp(&b.center().y));
    let first_y = rows
        .first()
        .unwrap_or_else(|| panic!("the Masking panel must paint local Exposure"))
        .center()
        .y;
    rows.retain(|rect| (rect.center().y - first_y).abs() < 2.0);
    rows.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    assert_eq!(
        rows.len(),
        2,
        "local Exposure must expose one caption and one numeric readout"
    );
    let caption = rows[0];
    let readout = rows[1];
    assert!(
        caption.max.x < readout.min.x,
        "Exposure caption precedes readout"
    );
    assert!(
        caption.min.y >= 0.0 && caption.max.y <= VIEWPORT[1],
        "the real local Exposure slider must be inside the interactable viewport, got {caption:?}"
    );
    Rect::from_min_max(
        Pos2::new(caption.min.x + 3.0, caption.center().y),
        Pos2::new(
            (caption.max.x - 3.0).max(caption.min.x + 3.0),
            caption.center().y,
        ),
    )
}

fn drag_local_exposure_and_render_production_tick(
    harness: &mut Harness<'_, LuminaApp>,
    rail: Rect,
) {
    let global_recipe_before = harness.state().recipe().clone();
    let at = |fraction: f32| Pos2::new(rail.min.x + rail.width() * fraction, rail.center().y);
    // Separate press, value-changing move, and held-pointer scheduler frame.
    // The third step is essential: UI edits are painted after schedule_render,
    // so only the next pointer-down frame reaches render_draft_tick.
    harness.hover_at(at(0.5));
    harness.drag_at(at(0.5));
    harness.step();
    harness.hover_at(at(0.7));
    harness.step();
    // egui_kittest dispatches the held move through a frame boundary; retain
    // the pointer for one more frame so the widget's changed response is
    // applied before the scheduler's subsequent render tick.
    harness.step();
    let value = harness
        .state()
        .selected_mask_local_adjustment("exposure")
        .expect("selected local exposure")
        .expect("the real slider gesture must store a local exposure");
    assert!(
        value > 2.0,
        "the UI drag must set non-neutral exposure, got {value}"
    );
    assert_eq!(
        harness.state().recipe(),
        &global_recipe_before,
        "the mask-local slider must not mutate the global recipe"
    );

    // Pointer remains down. This frame is the actual production `ui` call into
    // schedule_render -> render_draft_tick -> VRAM mask composition ->
    // draw_preview_area/update_texture -> egui-wgpu present.
    harness.step();
    assert!(
        harness.state().last_drag_tick().is_some(),
        "the real scheduler must execute render_draft_tick while the slider is held"
    );
    assert!(
        !harness.state().preview_is_draft(),
        "visible local masks must upgrade the scheduler's draft tick to the full mask-aware render"
    );
}

fn move_held_local_exposure_and_render_production_tick(
    harness: &mut Harness<'_, LuminaApp>,
    rail: Rect,
    fraction: f32,
) -> f64 {
    let at = Pos2::new(rail.min.x + rail.width() * fraction, rail.center().y);
    // Keep the real pointer down and let the changed UI value reach the next
    // scheduler frame, just as in the initial non-neutral drag.
    harness.hover_at(at);
    harness.step();
    harness.step();
    harness.step();
    assert!(
        harness.state().last_drag_tick().is_some(),
        "the real scheduler must continue rendering while the slider is held"
    );
    harness
        .state()
        .selected_mask_local_adjustment("exposure")
        .expect("selected local Exposure")
        .expect("the held slider must retain local Exposure")
}

#[test]
#[ignore = "requires a real local Metal/Vulkan adapter; opt-in must fail without one"]
fn applied_local_mask_pixels_reach_the_real_wgpu_present() {
    let png = deterministic_source_png();
    let dir = tempfile::tempdir().expect("masked fixture directory");
    let source = write_source(&dir, &png);
    let mut harness = build_harness(&source, true);
    let mask_id = add_applied_prompt_mask(&mut harness);
    let matte = brush_prompt_matte();
    // Paint the newly-created mask controls and warm the initial present before
    // querying the real slider; this frame is not a comparison baseline.
    harness.run_steps(SETTLE_FRAMES);
    let _ = photo_pixels(&mut harness);

    let rail = local_exposure_rail(&harness);
    drag_local_exposure_and_render_production_tick(&mut harness, rail);
    let global_recipe_after_drag = harness.state().recipe().clone();
    let local_exposure = harness
        .state()
        .selected_mask_local_adjustment("exposure")
        .expect("selected local Exposure")
        .expect("the UI gesture must store local Exposure");
    assert!(harness.state().mask_visible(&mask_id));
    assert!(
        harness.state().show_mask_overlay(),
        "the selected matte overlay must remain enabled for the real VRAM-present gate"
    );
    let mask_present = photo_pixels(&mut harness);
    let app = harness.state();
    eprintln!(
        "local-mask-present[vram]: frame_size={:?}, badge={:?}, error={:?}",
        app.gpu_present_frame_size(),
        app.gpu_routing_fallback_badge(),
        app.error()
    );
    assert!(
        app.gpu_present_frame_size().is_some(),
        "the captured frame after the production drag tick must be presented from fresh VRAM"
    );
    assert!(
        app.gpu_routing_fallback_badge().is_none(),
        "the local mask must not fall back to CPU or carry a VRAM refusal: {:?}",
        app.gpu_routing_fallback_badge()
    );
    assert!(
        app.error().is_none(),
        "the applied-mask render must not report an error: {:?}",
        app.error()
    );

    // The selected mask's visual tint is part of the real VRAM present. Capture
    // a lower, still non-neutral Exposure control with the same visible mask so
    // the production-pass proof cannot be satisfied by tint alone. A zero
    // adjustment would omit the evaluated layer and close the VRAM mask gate.
    let control_exposure =
        move_held_local_exposure_and_render_production_tick(&mut harness, rail, 0.55);
    assert!(
        control_exposure > 0.0 && control_exposure < local_exposure,
        "the real Exposure slider control must remain non-neutral and below {local_exposure}, got {control_exposure}"
    );
    assert_eq!(harness.state().recipe(), &global_recipe_after_drag);
    let mask_control = photo_pixels(&mut harness);
    assert!(
        harness.state().gpu_present_frame_size().is_some(),
        "the same-mask Exposure control must also be presented from VRAM"
    );
    assert!(
        harness.state().gpu_routing_fallback_badge().is_none(),
        "the same-mask Exposure control must not route away from VRAM"
    );

    let restored_exposure =
        move_held_local_exposure_and_render_production_tick(&mut harness, rail, 0.7);
    assert_eq!(
        restored_exposure, local_exposure,
        "the repeated pointer position must restore the exact same Exposure"
    );

    // Hide the same persisted matte only after the real UI gesture. Its
    // non-neutral local Exposure and the global recipe stay identical, giving
    // the counterfactual that detects a globally applied local adjustment.
    harness
        .state_mut()
        .set_mask_visible(&mask_id, false)
        .expect("hide the same matte without changing its local recipe");
    harness.run_steps(SETTLE_FRAMES);
    assert_eq!(harness.state().recipe(), &global_recipe_after_drag);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_adjustment("exposure")
            .expect("selected local Exposure remains readable"),
        Some(local_exposure),
        "hiding the matte must retain the same non-neutral local Exposure"
    );
    let mask_hidden = photo_pixels(&mut harness);
    let app = harness.state();
    assert!(
        app.gpu_present_frame_size().is_some(),
        "the mask-hidden counterfactual must also present the same global recipe from VRAM"
    );
    assert!(
        app.gpu_routing_fallback_badge().is_none(),
        "the mask-hidden counterfactual must not route away from VRAM: {:?}",
        app.gpu_routing_fallback_badge()
    );
    assert!(
        app.error().is_none(),
        "mask-hidden render error: {:?}",
        app.error()
    );

    assert_masked_exposure_is_spatial(&mask_present, &mask_control, &mask_hidden, &matte);
}
