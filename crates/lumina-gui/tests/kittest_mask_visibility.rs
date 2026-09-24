//! R5-MASKVIS-25 representative native golden.
//!
//! This intentionally pins the open Masking view with the selected-mask full
//! matte, brush controls, and the reversible panel/focus affordances. It is a
//! separate integration target so the existing unrelated goldens are not
//! rebaselined.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{
    LuminaApp, MaskOverlayMode, MaskTool, PinVisibility, SECTION_COUNT, SECTION_MASKING,
};
use lumina_sidecar::{BrushMark, BrushMarkSign};

fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

#[test]
#[ignore = "native wgpu adapter required; run: cargo test -p lumina-gui --test kittest_mask_visibility -- --ignored"]
fn mask_view_visibility() {
    let mut harness: Harness<'static, LuminaApp> = build_harness();
    harness.state_mut().set_module(lumina_gui::Module::Develop);
    harness
        .state_mut()
        .load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .expect("sample image loads");
    for index in 0..SECTION_COUNT {
        harness
            .state_mut()
            .set_section_open(index, index == SECTION_MASKING);
    }
    harness
        .state_mut()
        .create_mask("Golden subject")
        .expect("seed mask");
    harness
        .state_mut()
        .commit_brush_stroke(vec![BrushMark {
            x: 0.5,
            y: 0.5,
            radius: 0.42,
            sign: BrushMarkSign::Positive,
            softness: 0.2,
            flow: 0.9,
        }])
        .expect("seed brush matte");
    harness
        .state_mut()
        .set_mask_overlay_mode(MaskOverlayMode::SelectedFull);
    harness
        .state_mut()
        .set_pin_visibility(PinVisibility::Always);
    harness.state_mut().set_mask_tool(MaskTool::Brush);
    harness.state_mut().render().expect("seed render");

    for _ in 0..4 {
        harness.run();
    }
    assert!(harness.state_mut().mask_view_open());
    assert!(harness.state_mut().mask_overlay_allowed());
    assert!(!harness.state_mut().visible_edit_pins().is_empty());
    // Keep the new display toggle and the reversible panel-hide control in
    // the representative viewport instead of snapshotting only the top of the
    // long masking editor.
    harness
        .query_all_by_label("Tool overlay")
        .next()
        .expect("mask overlay toggle must be in the accesskit tree")
        .scroll_to_me();
    harness.run_steps(2);
    harness
        .query_all_by_label("Panels (Tab)")
        .next()
        .expect("panel-hide control must be in the accesskit tree")
        .scroll_to_me();
    harness.run_steps(2);
    harness.snapshot("mask_view_visibility");
}
