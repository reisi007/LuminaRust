//! UX-LOOK-CROP-18b (UXG-01 Runde 2) headless tests: full-frame crop re-edit
//! and the crop tool's straighten rotation + Auto-Level.
//!
//! The Runde-2 finding: after `Enter` the preview showed the *cropped* texture,
//! so the crop tool could only shrink an already committed crop and never grow
//! back to the full frame. The crop-mode preview now renders the full frame
//! (`crop_display.rs`); these tests pin that plus the session rotation draft,
//! the Auto-Level branches and the bit-identical `Esc`.

use super::*;
use crate::develop_geometry::crop_overlay::crop_draft;
use crate::develop_geometry::crop_rotation::{
    crop_auto_level_stash, crop_rotation_draft, set_crop_rotation_draft, AUTO_LEVEL_MIN_CONFIDENCE,
};
use crop_overlay_support::{crop_app, drag_corner, CropHarness};
use crop_session_support::{click_auto_button, drag_straighten_slider};

// R5-FIX-WELLE-20: nested test module so this 500-line-ratcheted file does not
// grow (straighten commit + tool-flow switch).
mod r5_tools;

/// Deterministic `w`×`h` checker PNG (real decode, non-degenerate content).
fn save_sized_png(path: &Path, width: u32, height: u32) {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = if (x / 8 + y / 8) % 2 == 0 {
                200u8
            } else {
                40u8
            };
            let index = ((y * width + x) as usize) * 4;
            pixels[index..index + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    std::fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
}

/// The committed crop shrinks the normal preview, but arming the crop tool
/// renders the **full frame** again — so a committed crop stays re-editable
/// (shrink AND grow). The recipe itself is untouched by the display override.
#[test]
fn crop_mode_renders_full_frame_over_a_committed_crop() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_sized_png(&source, 128, 96);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_crop_free(0.25, 0.25, 0.5, 0.5).unwrap();
    app.render().unwrap();
    let cropped = app.preview().expect("committed render");
    assert_eq!(
        (cropped.width, cropped.height),
        (64, 48),
        "a committed crop must render the cropped frame"
    );
    // Arm the crop tool: the preview texture becomes the full frame again.
    app.toggle_crop_mode();
    app.render().unwrap();
    let full = app.preview().expect("crop-mode render");
    assert_eq!(
        (full.width, full.height),
        (128, 96),
        "the crop tool must render the full frame (Runde-2 fixed)"
    );
    // The recipe keeps the committed rect — the override is display-only.
    let crop = app
        .recipe()
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.clone())
        .expect("committed crop stays in the recipe");
    assert!(matches!(crop, Crop::Free { .. }), "got {crop:?}");
}

/// The display override neutralizes only the geometry stage (crop/rot/ mirror)
/// and leaves adjustments and the real recipe untouched.
#[test]
fn crop_mode_display_recipe_neutralizes_geometry_only() {
    let mut app = new_app();
    app.set_crop_free(0.1, 0.1, 0.5, 0.5).unwrap();
    app.set_straighten(5.0);
    app.set_geometry_mirror(true, false);
    app.recipe.adjustments.insert("exposure".to_owned(), 1.5);
    let display = app.crop_mode_display_recipe();
    assert!(display.geometry.is_none(), "geometry must be neutralized");
    assert_eq!(
        display.adjustments.get("exposure"),
        Some(&1.5),
        "non-geometry edits stay in the display recipe"
    );
    assert!(
        app.recipe().geometry.is_some(),
        "the real recipe must keep its geometry"
    );
}

/// Commit → re-edit → grow back to the full frame → commit: the roundtrip
/// persists `(0,0,1,1)` through the existing free-crop setter (one step).
#[test]
fn crop_reedit_grows_back_to_full_frame_and_persists() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    app.set_crop_free(0.25, 0.25, 0.5, 0.5).unwrap();
    let seeded = commit_and_load_doc(&mut app, &source);
    assert!(matches!(
        seeded.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref()),
        Some(Crop::Free { .. })
    ));
    // Re-edit the committed crop and grow both corners back out to full frame.
    drag_corner(&mut harness, &mut app, (0.25, 0.25), (0.0, 0.0));
    drag_corner(&mut harness, &mut app, (0.75, 0.75), (1.0, 1.0));
    harness.key_pass(&mut app, egui::Key::Enter);
    let document = commit_and_load_doc(&mut app, &source);
    let committed = document.virtual_copies[0]
        .recipe
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.clone())
        .expect("re-edit persisted");
    match committed {
        Crop::Free {
            x,
            y,
            width,
            height,
        } => {
            assert!(
                x.abs() < 1e-4
                    && y.abs() < 1e-4
                    && (width - 1.0).abs() < 1e-4
                    && (height - 1.0).abs() < 1e-4,
                "growing back must persist the full frame, got ({x},{y},{width},{height})"
            );
        }
        other => panic!("expected a free rect, got {other:?}"),
    }
}

/// A crop-bar rotation edit commits through the existing straighten path on
/// `Enter` (one `geometry.straighten` history step) and survives a reload.
#[test]
fn crop_rotation_draft_commits_on_enter_and_persists() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    set_crop_rotation_draft(&harness.ctx, Some(-4.5));
    assert!(crop_draft(&harness.ctx).is_none(), "no crop draft set");
    harness.key_pass(&mut app, egui::Key::Enter);
    assert!(
        crop_rotation_draft(&harness.ctx).is_none(),
        "Enter consumes the rotation draft"
    );
    assert_eq!(
        app.recipe()
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(-4.5)
    );
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    assert_eq!(
        document.virtual_copies[0].history[0]
            .extras
            .get("action")
            .and_then(|value| value.as_str()),
        Some("geometry.straighten")
    );
    let reopened = reopen_app(&source);
    assert_eq!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(-4.5),
        "the committed rotation must survive a reload"
    );
}

/// Auto-Level on a real line fixture: non-trivial rotation draft, the
/// fingerprinted analysis is stashed and persisted (disabled) on `Enter`, and
/// the committed rotation actually reduces the tilt residual.
#[test]
fn auto_level_applies_rotation_and_persists_fingerprint() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("tilted.png");
    save_tilted_png(&source, 6.0);
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    open_and_decode(&mut app, source.display().to_string());
    app.toggle_crop_mode();
    app.apply_auto_level(&ctx);
    let degrees = crop_rotation_draft(&ctx).expect("auto level must set the rotation draft");
    assert!(
        degrees.abs() > 0.5,
        "a 6° tilt must yield a non-trivial rotation, got {degrees}"
    );
    let stash = crop_auto_level_stash(&ctx).expect("analysis must be stashed");
    assert!(
        stash.analysis.confidence >= AUTO_LEVEL_MIN_CONFIDENCE,
        "fixture must clear the confidence gate, got {}",
        stash.analysis.confidence
    );
    app.commit_crop_edit(&ctx);
    assert!(
        app.recipe()
            .upright
            .as_ref()
            .is_some_and(|stage| !stage.enabled),
        "the stashed analysis is persisted disabled (evidence, never double-applied)"
    );
    let analysis_rotation = app
        .recipe()
        .upright
        .as_ref()
        .and_then(|stage| stage.analysis.as_ref())
        .map(|analysis| analysis.rotation)
        .expect("fingerprinted analysis persisted");
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(
        document.virtual_copies[0].history.len(),
        1,
        "one shared step"
    );
    // The fingerprinted analysis reached the sidecar (identity persistence),
    // still disabled so it cannot double-apply next to the geometry rotation.
    let persisted_stage = document.virtual_copies[0]
        .recipe
        .upright
        .as_ref()
        .expect("upright analysis persisted to the sidecar");
    assert!(!persisted_stage.enabled);
    assert_eq!(
        persisted_stage
            .analysis
            .as_ref()
            .map(|analysis| analysis.fingerprint.algorithm.as_str()),
        Some(lumina_core::UPRIGHT_ALGORITHM)
    );
    // The rotation is committed through the existing geometry field.
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(degrees)
    );
    // Leave crop mode so the geometry rotation lands in the preview, then
    // re-run the detector: the residual tilt must shrink.
    app.toggle_crop_mode();
    app.render().unwrap();
    let residual = lumina_core::analyze_upright(app.preview().expect("re-render"));
    assert!(
        residual.rotation.abs() < analysis_rotation.abs(),
        "auto level must reduce the tilt (suggested {}, residual {})",
        analysis_rotation,
        residual.rotation
    );
}

/// Auto-Level at low confidence refuses loudly: visible status, no draft, no
/// stash and no recipe/sidecar write.
#[test]
fn auto_level_low_confidence_reports_and_saves_nothing() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let before = app.recipe().clone();
    app.apply_auto_level(&harness.ctx);
    assert!(
        crop_rotation_draft(&harness.ctx).is_none(),
        "low confidence must not set a rotation draft"
    );
    assert!(crop_auto_level_stash(&harness.ctx).is_none());
    assert_eq!(
        app.status(),
        Str::UprightStatusPattern.format_arg(Str::UprightNone.t()),
        "the refusal must be visible"
    );
    assert_eq!(app.recipe(), &before, "the recipe must stay untouched");
    assert!(app.pending_slider_commit.is_none(), "nothing may be armed");
    assert!(!sidecar.is_file(), "low confidence must not save a sidecar");
}

/// `Esc` after a crop draft *and* a rotation draft (and Auto-Level) discards
/// everything bit-for-bit: recipe equal, no sidecar, no armed write.
#[test]
fn escape_discards_crop_and_rotation_bit_identically() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let before = app.recipe().clone();
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.3, 0.3));
    set_crop_rotation_draft(&harness.ctx, Some(7.5));
    harness.key_pass(&mut app, egui::Key::Escape);
    assert!(crop_draft(&harness.ctx).is_none());
    assert!(crop_rotation_draft(&harness.ctx).is_none());
    assert_eq!(app.recipe(), &before, "Esc must be bit-identical");
    assert!(app.pending_slider_commit.is_none());
    assert!(!sidecar.is_file(), "Esc must not write a sidecar");
}

/// Crop + rotation committed together on a single `Enter` coalesce into **one**
/// geometry history step (documented choice: one shared step), and both values
/// reach the sidecar.
#[test]
fn crop_and_rotation_commit_in_one_history_step() {
    let mut harness = CropHarness::new();
    let (directory, mut app) = crop_app(&harness.ctx);
    let source = directory.path().join("photo.png");
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.2, 0.2));
    set_crop_rotation_draft(&harness.ctx, Some(3.0));
    harness.key_pass(&mut app, egui::Key::Enter);
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(
        document.virtual_copies[0].history.len(),
        1,
        "crop + rotation must share one history step"
    );
    assert_eq!(
        document.virtual_copies[0].history[0]
            .extras
            .get("action")
            .and_then(|value| value.as_str()),
        Some("geometry.crop_free"),
        "the crop commit is the final setter (single shared step label)"
    );
    let recipe = &document.virtual_copies[0].recipe;
    assert!(recipe
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.as_ref())
        .is_some());
    assert_eq!(
        recipe
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(3.0)
    );
}

/// The armed crop bar paints the straighten control and the Auto-Level button
/// (headless paint guard; the pixel golden is a separate kittest test).
#[test]
fn crop_bar_paints_straighten_and_auto_level() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    let shapes = harness.pass(
        &mut app,
        vec![egui::Event::PointerMoved(harness.screen.center())],
    );
    let texts = painted_texts(&shapes);
    for want in [Str::Straighten.t(), Str::Auto.t()] {
        assert!(
            texts.iter().any(|text| text == want),
            "crop bar control {want:?} must paint, got {texts:?}"
        );
    }
}

/// F1(a) / R5-STRAIGHTEN-1: the Straighten slider is really interactive — a
/// pointer drag on the painted track reaches the recipe through the real
/// `set_straighten` path (logged at `info!`, save armed by the setter) and the
/// session rotation draft is kept in sync.
#[test]
fn crop_bar_slider_drag_sets_the_session_rotation_draft() {
    let mut app = new_app();
    let ctx = drag_straighten_slider(&mut app, 0.85);
    let draft = crop_rotation_draft(&ctx).expect("drag must set the session rotation draft");
    assert!(
        draft > 10.0 && draft <= 180.0,
        "dragging right of centre must raise the angle, got {draft}"
    );
    assert_eq!(
        app.recipe()
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(draft),
        "R5-STRAIGHTEN-1: the drag must write the rotation into the recipe"
    );
    assert!(
        app.pending_slider_commit.is_some(),
        "the drag must arm the debounced save (the value must persist)"
    );
}

/// F1(b) high branch: a real click on the `Auto` button runs the upright
/// analysis and sets the rotation draft + fingerprinted stash (still no save).
#[test]
fn crop_bar_auto_button_click_applies_high_confidence_level() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("tilted.png");
    save_tilted_png(&source, 6.0);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let ctx = click_auto_button(&mut app);
    let draft = crop_rotation_draft(&ctx).expect("Auto click must set the rotation draft");
    assert!(draft.abs() > 0.5, "got {draft}");
    assert!(
        crop_auto_level_stash(&ctx).is_some(),
        "the analysis must be stashed"
    );
    assert!(
        app.recipe().geometry.is_none(),
        "the click must not write the recipe"
    );
    assert!(
        app.pending_slider_commit.is_none(),
        "the click must not arm a save"
    );
}

/// F1(b) low branch: a real click on `Auto` on a fixture without line signal
/// refuses loudly (visible status), without draft, stash or save.
#[test]
fn crop_bar_auto_button_click_low_confidence_refuses_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("flat.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let ctx = click_auto_button(&mut app);
    assert!(crop_rotation_draft(&ctx).is_none());
    assert!(crop_auto_level_stash(&ctx).is_none());
    assert_eq!(
        app.status(),
        Str::UprightStatusPattern.format_arg(Str::UprightNone.t()),
        "the refusal must be visible"
    );
    assert!(app.pending_slider_commit.is_none(), "nothing may be armed");
}

/// F3: Auto-Level never overwrites an already persisted upright stage — the
/// high branch still stashes, but `Enter` keeps the existing analysis and arms
/// no upright save (only the geometry rotation).
#[test]
fn auto_level_never_overwrites_an_existing_upright_stage() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("tilted.png");
    save_tilted_png(&source, 6.0);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let sentinel = lumina_core::upright_analysis(
        lumina_core::UprightSuggestion {
            vertical: 0.0,
            horizontal: 0.0,
            rotation: 0.0,
            line_count: 1,
            confidence: 0.0,
        },
        "blake3:existing-sentinel",
    );
    app.recipe.upright = Some(Upright {
        version: 1,
        enabled: true,
        analysis: Some(sentinel),
    });
    let ctx = click_auto_button(&mut app);
    assert!(
        crop_auto_level_stash(&ctx).is_some(),
        "high confidence must stash"
    );
    app.commit_crop_edit(&ctx);
    let stage = app.recipe().upright.as_ref().expect("existing stage kept");
    assert!(stage.enabled, "the existing stage must stay enabled");
    assert_eq!(
        stage
            .analysis
            .as_ref()
            .map(|analysis| analysis.fingerprint.input_fingerprint.as_str()),
        Some("blake3:existing-sentinel"),
        "the persisted analysis must not be overwritten"
    );
}
