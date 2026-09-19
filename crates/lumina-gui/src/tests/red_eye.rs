//! upright and red-eye detection/picker tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// LRPAR-G06-UPRIGHT-15: analyze → apply → disable → reload. The analysis
/// persists with a source fingerprint and supplies the effective
/// perspective while enabled (DoD §1 chain: action → commit → file →
/// reload).
#[test]
fn upright_analyze_apply_disable_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("tilted.png");
    save_tilted_png(&source, 6.0);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.recipe().upright.is_none());
    assert_eq!(app.upright_status(), "none");

    // Enabling without an analysis is refused loudly, recipe untouched.
    let error = app.set_upright_enabled(true).unwrap_err().to_string();
    assert!(error.contains("no persisted upright analysis"), "{error}");
    assert!(app.recipe().upright.is_none());

    // Analyze: applied by default, effective perspective from the analysis.
    app.analyze_upright_now().unwrap();
    let stage = app.recipe().upright.as_ref().expect("stage persisted");
    assert!(stage.enabled);
    let analysis = stage.analysis.as_ref().expect("analysis persisted");
    assert_eq!(
        analysis.fingerprint.algorithm,
        lumina_core::UPRIGHT_ALGORITHM
    );
    assert!(analysis.line_count > 0);
    assert!(analysis.rotation.abs() > 0.0);
    let effective = app
        .recipe()
        .effective_perspective()
        .expect("analysis supplies the perspective");
    assert!(effective.rotation.abs() > 0.0);
    assert_eq!(app.upright_status(), "fresh");

    let document = commit_and_load_doc(&mut app, &source);
    let stage = document.virtual_copies[0]
        .recipe
        .upright
        .as_ref()
        .expect("upright persisted");
    assert!(stage.enabled);
    assert_eq!(
        stage.analysis.as_ref().unwrap().fingerprint.algorithm,
        lumina_core::UPRIGHT_ALGORITHM
    );
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    assert_eq!(
        document.virtual_copies[0].history[0]
            .extras
            .get("action")
            .and_then(|value| value.as_str()),
        Some("upright.analyze")
    );

    // Reload → the analysis is fresh for the same source.
    let mut reopened = reopen_app(&source);
    assert!(reopened.recipe().upright.as_ref().unwrap().enabled);
    assert_eq!(reopened.upright_status(), "fresh");

    // Disable → the manual perspective (none here) is authoritative again.
    reopened.set_upright_enabled(false).unwrap();
    assert!(reopened.recipe().effective_perspective().is_none());
    assert!(!reopened.recipe().upright.as_ref().unwrap().enabled);
    // The analysis itself stays persisted and still reports its freshness.
    assert_eq!(reopened.upright_status(), "fresh");

    // A changed fingerprint is reported `stale`, never silently recomputed.
    reopened
        .recipe
        .upright
        .as_mut()
        .unwrap()
        .analysis
        .as_mut()
        .unwrap()
        .fingerprint
        .input_fingerprint = "blake3:other".into();
    assert_eq!(reopened.upright_status(), "stale");

    // Clear removes the whole stage.
    reopened.clear_upright();
    assert!(reopened.recipe().upright.is_none());
    assert_eq!(reopened.upright_status(), "none");
}

/// G-14: the preview picker marks regions with stable `re-N` ids, the
/// per-region strengths commit and survive a reload, and removal/clear are
/// visible recipe edits (DoD §1).
#[test]
fn red_eye_picker_marks_regions_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.recipe().red_eye.is_none());

    app.add_red_eye_region(0.25, 0.35).unwrap();
    app.add_red_eye_region(0.60, 0.40).unwrap();
    // Stable ids in marking order.
    let ids: Vec<String> = app
        .recipe()
        .red_eye
        .as_ref()
        .unwrap()
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    assert_eq!(ids, vec!["re-1", "re-2"]);
    app.set_red_eye_region_value("re-1", "radius", 0.08);
    app.set_red_eye_region_value("re-1", "desaturate", 1.0);
    app.set_red_eye_region_value("re-1", "darken", 0.6);
    // Unknown id/field is ignored loudly (recipe unchanged).
    app.set_red_eye_region_value("re-missing", "radius", 0.5);
    app.set_red_eye_region_value("re-2", "bogus", 0.5);
    assert_eq!(
        app.recipe().red_eye.as_ref().unwrap().regions[1].radius,
        0.05
    );

    let document = commit_and_load_doc(&mut app, &source);
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .expect("red-eye persisted")
        .regions;
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].id, "re-1");
    assert_eq!(regions[0].x, 0.25);
    assert_eq!(regions[0].y, 0.35);
    assert_eq!(regions[0].radius, 0.08);
    assert_eq!(regions[0].desaturate, 1.0);
    assert_eq!(regions[0].darken, 0.6);
    assert_eq!(document.virtual_copies[0].history.len(), 1);

    let mut reopened = reopen_app(&source);
    assert_eq!(reopened.recipe().red_eye.as_ref().unwrap().regions.len(), 2);
    // Remove one region, then clear the rest: each is a visible edit.
    reopened.remove_red_eye_region("re-1");
    assert_eq!(
        reopened.recipe().red_eye.as_ref().unwrap().regions[0].id,
        "re-2"
    );
    reopened.clear_red_eye();
    assert!(reopened.recipe().red_eye.is_none());
}

/// G-14: the picker refuses to add beyond the 32-region cap (loud).
#[test]
fn red_eye_picker_enforces_region_cap() {
    let mut app = new_app();
    for index in 0..RED_EYE_MAX_REGIONS {
        app.add_red_eye_region(0.5, 0.5)
            .unwrap_or_else(|e| panic!("region {index}: {e}"));
    }
    let error = app.add_red_eye_region(0.5, 0.5).unwrap_err().to_string();
    assert!(error.contains("region limit"), "{error}");
    assert_eq!(
        app.recipe().red_eye.as_ref().unwrap().regions.len(),
        RED_EYE_MAX_REGIONS
    );
}

/// LRPAR-G14-REDEYE-AUTO-15: loading/rendering never prefills regions;
/// detection lists without persisting; the explicit apply persists through
/// the shared save path and survives a reload.
#[test]
fn g14_red_eye_detect_lists_then_applies_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("pupil.png");
    save_red_pupil_png(&source, 64, 64, &[(30, 30, 36, 36)]);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // No automatic prefill just from loading.
    assert!(app.recipe().red_eye.is_none());
    assert!(app.red_eye_detect_status.is_empty());

    let candidates = app.detect_red_eye_candidates().unwrap();
    assert_eq!(candidates.len(), 1, "{candidates:?}");
    assert!(!app.red_eye_detect_status.is_empty());
    // Listing persisted nothing.
    assert!(app.recipe().red_eye.is_none());

    let applied = app.apply_detected_red_eyes(&candidates).unwrap();
    assert_eq!(applied, 1);
    let regions = &app.recipe().red_eye.as_ref().unwrap().regions;
    assert_eq!(regions.len(), 1);
    assert!(regions[0].id.starts_with(RED_EYE_DETECT_ID_PREFIX));
    assert_eq!(regions[0].desaturate, 0.8);
    assert_eq!(regions[0].darken, 0.4);

    // Manual region is preserved by a re-detection; auto region is replaced.
    app.add_red_eye_region(0.1, 0.1).unwrap();
    let candidates = app.detect_red_eye_candidates().unwrap();
    assert_eq!(app.apply_detected_red_eyes(&candidates).unwrap(), 1);
    let regions = &app.recipe().red_eye.as_ref().unwrap().regions;
    assert_eq!(regions.len(), 2);
    assert!(regions.iter().any(|region| region.id == "re-1"));
    assert!(regions
        .iter()
        .any(|region| region.id.starts_with(RED_EYE_DETECT_ID_PREFIX)));

    // Explicit apply commits through the shared save path; reload restores.
    let document = commit_and_load_doc(&mut app, &source);
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .unwrap()
        .regions;
    assert_eq!(regions.len(), 2);
    let reopened = reopen_app(&source);
    assert_eq!(reopened.recipe().red_eye.as_ref().unwrap().regions.len(), 2);
}

/// LRPAR-G14-REDEYE-AUTO-15: no red pupils → no candidates, never a stage;
/// applying nothing is a loud no-op that writes no sidecar/history.
#[test]
fn g14_red_eye_detect_without_pupils_never_prefills() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("grey.png");
    save_red_pupil_png(&source, 64, 64, &[]);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let candidates = app.detect_red_eye_candidates().unwrap();
    assert!(candidates.is_empty());
    assert_eq!(app.apply_detected_red_eyes(&candidates).unwrap(), 0);
    assert!(app.recipe().red_eye.is_none());
    // The no-op does not arm a save, so no sidecar is written.
    app.commit_pending_slider_save([0, 0]);
    assert!(
        !lumina_sidecar::sidecar_path_for(&source).is_file(),
        "a no-op detection must not write a sidecar"
    );
}

/// LRPAR-G14-REDEYE-AUTO-15: the 32-region cap is enforced loudly on the
/// detection apply path (no silent truncation).
#[test]
fn g14_red_eye_detect_apply_enforces_region_cap() {
    let mut app = new_app();
    for _ in 0..RED_EYE_MAX_REGIONS {
        app.add_red_eye_region(0.5, 0.5).unwrap();
    }
    let candidate = DetectedRedEye {
        id: format!("{RED_EYE_DETECT_ID_PREFIX}deadbeef"),
        x: 0.5,
        y: 0.5,
        radius: 0.05,
        confidence: 0.9,
    };
    let error = app
        .apply_detected_red_eyes(&[candidate])
        .unwrap_err()
        .to_string();
    assert!(error.contains("exceed the"), "{error}");
    assert_eq!(
        app.recipe().red_eye.as_ref().unwrap().regions.len(),
        RED_EYE_MAX_REGIONS
    );
}

/// G-14 L1: `add_red_eye_region` validates normalized coordinates loudly
/// instead of silently clipping them; rejected marks persist nothing, and
/// the `0..=1` borders are accepted verbatim.
#[test]
fn add_red_eye_region_rejects_out_of_range_coordinates() {
    let mut app = new_app();
    for (x, y) in [
        (1.5_f32, 0.5_f32),
        (-0.1, 0.5),
        (0.5, 2.0),
        (f32::NAN, 0.5),
        (0.5, f32::INFINITY),
        (f32::NEG_INFINITY, 0.5),
    ] {
        let error = app.add_red_eye_region(x, y).unwrap_err().to_string();
        assert!(error.contains("out of 0..=1"), "({x}, {y}): {error}");
    }
    assert!(
        app.recipe().red_eye.is_none(),
        "rejected marks must not be persisted"
    );
    app.add_red_eye_region(0.0, 1.0).unwrap();
    let region = &app.recipe().red_eye.as_ref().unwrap().regions[0];
    assert_eq!((region.x, region.y), (0.0, 1.0));
}

/// G-14 H1 regression: the WB eyedropper and the red-eye region picker are
/// mutually exclusive on the shared preview click. Arming one disarms the
/// other, so one click can never sample a white balance *and* mark a
/// pupil. The click is driven headlessly through `draw_preview`.
#[test]
fn wb_and_red_eye_pickers_are_mutually_exclusive_on_click() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(
        ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap(),
        "pickers.png",
    )
    .unwrap();
    app.render().unwrap();
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([200, 150], egui::Color32::BLACK),
        egui::TextureOptions::LINEAR,
    ));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let pos = screen.center();
    let pass = |app: &mut LuminaApp, events: Vec<egui::Event>, time: f64| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
            },
        );
        // No GPU renderer consumes the per-frame texture deltas headlessly.
        output.textures_delta.clear();
    };
    // Warm-up pass so the preview widget exists and is hit-tested.
    pass(&mut app, vec![egui::Event::PointerMoved(pos)], 0.5);
    let click = |app: &mut LuminaApp, time: f64| {
        pass(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            time,
        );
        pass(
            app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            time + 0.05,
        );
    };

    // Arm A (red-eye), then arm B (WB): B wins and A is disarmed.
    app.set_red_eye_pick_mode(true);
    assert!(app.red_eye_pick_mode && !app.wb_pick_mode);
    app.arm_wb_picker();
    assert!(
        app.wb_pick_mode && !app.red_eye_pick_mode,
        "arming the WB eyedropper must disarm the red-eye picker"
    );
    // The grey frame samples 6500 K; the disarmed red-eye picker must not
    // add a region for the very same click.
    click(&mut app, 1.0);
    assert_eq!(
        app.recipe().adjustments.get("wb_temperature"),
        Some(&6500.0)
    );
    assert!(
        app.recipe().red_eye.is_none(),
        "a disarmed red-eye picker must not mark a region"
    );
    assert!(!app.wb_pick_mode, "a WB pick disarms the eyedropper");

    // Arm A (WB), then arm B (red-eye): B wins and A is disarmed.
    app.arm_wb_picker();
    app.set_red_eye_pick_mode(true);
    assert!(
        app.red_eye_pick_mode && !app.wb_pick_mode,
        "arming the red-eye picker must disarm the WB eyedropper"
    );
    let picked = app.recipe().adjustments.get("wb_temperature").copied();
    click(&mut app, 2.0);
    assert_eq!(
        app.recipe().red_eye.as_ref().map(|c| c.regions.len()),
        Some(1),
        "the armed red-eye picker must mark exactly one region"
    );
    assert_eq!(
        app.recipe().adjustments.get("wb_temperature").copied(),
        picked,
        "the disarmed WB eyedropper must not resample"
    );
    assert!(
        app.red_eye_pick_mode,
        "the red-eye picker stays armed for multiple marks"
    );
}

/// The Develop panel paints the Upright controls (Geometry section) and the
/// Red Eye controls (Detail section) headlessly — every visible label.
#[test]
fn upright_and_red_eye_panels_paint() {
    let mut app = new_app();
    app.set_section_open(SECTION_GEOMETRY, true);
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_geometry(ui));
    let texts = painted_texts(&shapes);
    assert!(
        texts.iter().any(|t| t == Str::UprightAnalyze.t()),
        "Upright analyze button must paint, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == Str::UprightEnable.t()),
        "Upright apply checkbox must paint, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains(Str::UprightNone.t())),
        "Upright status must paint, got {texts:?}"
    );

    let mut app = new_app();
    app.set_section_open(SECTION_DETAIL, true);
    // Give the section one region so every per-region control paints.
    app.add_red_eye_region(0.3, 0.3).unwrap();
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_detail(ui));
    let texts = painted_texts(&shapes);
    for needle in [
        Str::RedEye.t(),
        Str::RedEyePickMode.t(),
        Str::RedEyeRadius.t(),
        Str::RedEyeDesaturate.t(),
        Str::RedEyeDarken.t(),
        Str::RedEyeRemove.t(),
        Str::RedEyeClear.t(),
    ] {
        assert!(
            texts.iter().any(|t| t == needle),
            "red-eye control {needle:?} must paint, got {texts:?}"
        );
    }
    assert!(
        texts.iter().any(|t| t.contains("re-1")),
        "the marked region id must paint, got {texts:?}"
    );
}
