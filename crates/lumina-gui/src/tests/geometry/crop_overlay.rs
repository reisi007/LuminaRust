//! UX-LOOK-CROP-18 (UXG-01) headless tests: interactive crop overlay.
//!
//! Covers the gesture set (corner resize / move), the session-draft contract
//! (`Enter` commits through the existing free-crop setter + debounced sidecar;
//! `Esc` discards and leaves the recipe untouched), the paint chrome
//! (darkening bands, thirds grid, corner handles) and the clamp behaviour.

use super::*;
use crate::develop_geometry::crop_overlay::{crop_draft, crop_overlay_id};
use crop_overlay_support::{crop_app, drag_corner, CropHarness};

/// Corner resize updates the live session draft, and the recipe/sidecar stay
/// untouched until `Enter` (the SOLL's draft/commit separation).
#[test]
fn crop_corner_drag_updates_draft_without_touching_recipe() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.25, 0.25));
    let draft = crop_draft(&harness.ctx).expect("corner drag must set the session draft");
    assert!(
        (draft.x - 0.25).abs() < 0.02 && (draft.y - 0.25).abs() < 0.02,
        "dragged top-left corner must land at the pointer: {draft:?}"
    );
    assert!(
        (draft.width - 0.75).abs() < 0.02 && (draft.height - 0.75).abs() < 0.02,
        "opposite corner stays anchored: {draft:?}"
    );
    assert!(
        app.recipe()
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .is_none(),
        "the recipe must not carry the draft before commit"
    );
    assert!(
        app.pending_slider_commit.is_none(),
        "a drag without Enter must not arm a sidecar write"
    );
}

/// Dragging inside the frame moves it; the draft stays inside the frame and
/// keeps a positive extent when dragged far past the edge.
#[test]
fn crop_move_and_clamp_keep_draft_inside_frame() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    // Seed a committed crop so there is an interior to grab.
    app.set_crop_free(0.25, 0.25, 0.5, 0.5).unwrap();
    drag_corner(&mut harness, &mut app, (0.5, 0.5), (0.9, 0.9));
    let moved = crop_draft(&harness.ctx).expect("move drag must set the session draft");
    assert!(
        (moved.width - 0.5).abs() < 0.02 && (moved.height - 0.5).abs() < 0.02,
        "a move keeps the extent: {moved:?}"
    );
    assert!(
        moved.x <= (1.0 - moved.width) + 1e-4 && moved.y <= (1.0 - moved.height) + 1e-4,
        "a move never leaves the frame: {moved:?}"
    );
    // Drag the top-left corner far outside the image: the draft clamps to a
    // valid, non-degenerate rect instead of inverting or exceeding the frame.
    drag_corner(&mut harness, &mut app, (0.5, 0.5), (-1.0, -1.0));
    let clamped = crop_draft(&harness.ctx).expect("clamped drag must still set a draft");
    assert!(
        clamped.x >= 0.0 && clamped.y >= 0.0,
        "corner drag clamps into the frame: {clamped:?}"
    );
    assert!(
        clamped.width >= 0.02 - 1e-4 && clamped.height >= 0.02 - 1e-4,
        "corner drag keeps the minimum extent: {clamped:?}"
    );
}

/// `Enter` commits the draft through the existing setter and the debounced save
/// writes it to the sidecar; a reload restores the free crop (DoD §1 chain).
#[test]
fn crop_enter_commits_and_persists() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.25, 0.25));
    assert!(crop_draft(&harness.ctx).is_some(), "draft before Enter");
    harness.key_pass(&mut app, egui::Key::Enter);
    assert!(
        crop_draft(&harness.ctx).is_none(),
        "Enter consumes the session draft"
    );
    let committed = app
        .recipe()
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.clone())
        .expect("Enter must write the crop into the recipe");
    assert!(
        matches!(committed, Crop::Free { .. }),
        "interactive commit writes a free rect: {committed:?}"
    );
    assert!(
        app.pending_slider_commit.is_some(),
        "Enter arms the debounced sidecar write"
    );
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(
        document.virtual_copies[0].history.len(),
        1,
        "Enter must arm exactly one geometry history step"
    );
    assert_eq!(
        document.virtual_copies[0].history[0]
            .extras
            .get("action")
            .and_then(|value| value.as_str()),
        Some("geometry.crop_free")
    );
    let persisted = document.virtual_copies[0]
        .recipe
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.as_ref())
        .expect("crop persisted to the sidecar");
    match persisted {
        Crop::Free {
            x,
            y,
            width,
            height,
        } => {
            assert!(
                (x - 0.25).abs() < 0.02 && (y - 0.25).abs() < 0.02,
                "{persisted:?}"
            );
            assert!(
                (width - 0.75).abs() < 0.02 && (height - 0.75).abs() < 0.02,
                "{persisted:?}"
            );
        }
        other => panic!("expected a free crop, got {other:?}"),
    }
    let reopened = reopen_app(&source);
    assert!(
        matches!(
            reopened
                .recipe()
                .geometry
                .as_ref()
                .and_then(|geometry| geometry.crop.as_ref()),
            Some(Crop::Free { .. })
        ),
        "the committed crop must survive a reload"
    );
}

/// `Esc` discards the draft, leaves the recipe unchanged and writes no sidecar.
#[test]
fn crop_escape_discards_draft_and_leaves_recipe() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.3, 0.3));
    assert!(crop_draft(&harness.ctx).is_some(), "draft before Esc");
    harness.key_pass(&mut app, egui::Key::Escape);
    assert!(
        crop_draft(&harness.ctx).is_none(),
        "Esc must discard the session draft"
    );
    assert!(
        app.recipe()
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .is_none(),
        "Esc must leave the recipe exactly as before the gesture"
    );
    assert!(
        app.pending_slider_commit.is_none(),
        "Esc must not arm a sidecar write"
    );
    assert!(
        !sidecar.is_file(),
        "Esc must not write a sidecar (nothing committed)"
    );
}

/// The armed overlay paints the darkening bands, the thirds grid and the four
/// corner handles (headless paint guard; the pixel golden is a separate test).
#[test]
fn crop_mode_paints_handles_grid_and_darkening() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    app.set_crop_free(0.2, 0.2, 0.6, 0.5).unwrap();
    let shapes = harness.pass(
        &mut app,
        vec![egui::Event::PointerMoved(harness.screen.center())],
    );
    let dark = egui::Color32::from_black_alpha(150);
    let (mut bands, mut grid, mut handles) = (0usize, 0usize, 0usize);
    for clipped in &shapes {
        match &clipped.shape {
            egui::Shape::Rect(rect) => {
                if rect.fill == dark {
                    bands += 1;
                }
                if rect.fill == egui::Color32::WHITE
                    && rect.rect.width() <= 9.0
                    && rect.rect.height() <= 9.0
                {
                    handles += 1;
                }
            }
            egui::Shape::LineSegment { .. } => grid += 1,
            _ => {}
        }
    }
    assert!(bands >= 4, "four darkening bands must paint, got {bands}");
    assert!(grid >= 4, "the thirds grid must paint 4 lines, got {grid}");
    assert!(
        handles >= 4,
        "four corner handles must paint, got {handles}"
    );
    // The interactive region is registered for real hit-testing.
    assert!(
        harness.ctx.read_response(crop_overlay_id()).is_some(),
        "the crop overlay interaction region must be registered"
    );
}

/// R4-RECT-1: without crop mode a committed recipe crop paints NOTHING — the
/// preview pixels already carry the crop, and the former full-source white
/// stroke reached past the cropped image (the reported rectangle over the
/// preview). No darkening, no white crop frame.
#[test]
fn crop_overlay_unarmed_paints_no_crop_frame() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    app.toggle_crop_mode(); // off again
    app.set_crop_free(0.2, 0.2, 0.6, 0.5).unwrap();
    let shapes = harness.pass(
        &mut app,
        vec![egui::Event::PointerMoved(harness.screen.center())],
    );
    let dark = egui::Color32::from_black_alpha(150);
    let mut bands = 0usize;
    let mut strokes = 0usize;
    for clipped in &shapes {
        if let egui::Shape::Rect(rect) = &clipped.shape {
            if rect.fill == dark {
                bands += 1;
            } else if rect.stroke.color == egui::Color32::WHITE {
                strokes += 1;
            }
        }
    }
    assert_eq!(bands, 0, "unarmed overlay must not darken");
    assert_eq!(
        strokes, 0,
        "an unarmed committed crop must not paint a crop frame"
    );
}

/// Every corner is a member of the resize class: the dragged corner follows
/// the pointer and the opposite corner stays anchored.
#[test]
fn crop_each_corner_resizes_with_opposite_corner_anchored() {
    // (press fraction, drag fraction, expected x/y/width/height)
    let cases = [
        ((0.0, 0.0), (0.3, 0.2), (0.3, 0.2, 0.7, 0.8)),
        ((1.0, 0.0), (0.7, 0.2), (0.0, 0.2, 0.7, 0.8)),
        ((0.0, 1.0), (0.3, 0.8), (0.3, 0.0, 0.7, 0.8)),
        ((1.0, 1.0), (0.7, 0.8), (0.0, 0.0, 0.7, 0.8)),
    ];
    for (from, to, expected) in cases {
        let mut harness = CropHarness::new();
        let (_directory, mut app) = crop_app(&harness.ctx);
        app.set_crop_free(0.0, 0.0, 1.0, 1.0).unwrap();
        drag_corner(&mut harness, &mut app, from, to);
        let draft = crop_draft(&harness.ctx).expect("corner drag must set a draft");
        let (x, y, w, h) = (draft.x, draft.y, draft.width, draft.height);
        assert!(
            (x - expected.0).abs() < 0.02
                && (y - expected.1).abs() < 0.02
                && (w - expected.2).abs() < 0.02
                && (h - expected.3).abs() < 0.02,
            "corner {from:?} -> {to:?}: expected {expected:?}, got ({x:.3},{y:.3},{w:.3},{h:.3})"
        );
    }
}

/// An interactive drag resizes a locked aspect crop into a free rectangle on
/// `Enter` (documented commit: `Crop` has no size-preserving aspect field, so
/// the schema-representable result is `Free`; the aspect selector remains the
/// way to request a locked ratio).
#[test]
fn crop_interactive_commit_converts_aspect_preset_to_free() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    app.set_crop_aspect("1:1").unwrap();
    // Flush the seed commit so only the interactive `Enter` commit is observed.
    let seeded = commit_and_load_doc(&mut app, &source);
    assert!(matches!(
        seeded.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref()),
        Some(Crop::Aspect { .. })
    ));
    // 1:1 on a 2:1 source centers at x=0.25,w=0.5, y=0,h=1: drag its top-left.
    drag_corner(&mut harness, &mut app, (0.25, 0.0), (0.4, 0.1));
    harness.key_pass(&mut app, egui::Key::Enter);
    let document = commit_and_load_doc(&mut app, &source);
    let committed = document.virtual_copies[0]
        .recipe
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.clone())
        .expect("interactive commit persisted");
    assert!(
        matches!(committed, Crop::Free { .. }),
        "interactive resize writes a free rect: {committed:?}"
    );
}

/// Leaving crop mode without `Enter` (e.g. `R` toggled off) discards the
/// session draft loudly — it is never carried into a later crop session.
#[test]
fn leaving_crop_mode_discards_the_draft() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.3, 0.3));
    assert!(crop_draft(&harness.ctx).is_some(), "draft after the drag");
    app.toggle_crop_mode(); // R off
                            // The next frame's key handler observes crop mode off and discards.
    harness.key_pass(&mut app, egui::Key::Z);
    assert!(
        crop_draft(&harness.ctx).is_none(),
        "leaving crop mode must discard the session draft"
    );
    assert!(
        app.recipe()
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .is_none(),
        "discarding must not touch the recipe"
    );
}

/// With crop mode disarmed the new key handler must not swallow `Esc`: the
/// armed-preview-tool cancel path stays reachable through the replaced call
/// site.
#[test]
fn escape_still_cancels_armed_tools_without_crop_mode() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(png(), "test.png").unwrap();
    app.arm_wb_picker();
    assert!(app.wb_pick_mode);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        },
        |ui| app.handle_crop_shortcuts(ui.ctx()),
    );
    output.textures_delta.clear();
    assert!(
        !app.wb_pick_mode,
        "Esc must still disarm the WB eyedropper when crop mode is off"
    );
}
