//! render/full-render/dirty and mask-plane bookkeeping tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- REVIEW-GUI-CURVE-1: tone-curve clamp loss detection ----

#[test]
fn tone_curve_roundtrip_loss_is_detected() {
    // Shadows base point is 0.0 — any negative delta is clamped away.
    assert!(tone_curve_roundtrip_is_lossy(-0.5, 0.0, 0.0, 0.0));
    // Darks base 1/3: -1.0 would overshoot below 0 → clamped → lossy.
    assert!(tone_curve_roundtrip_is_lossy(0.0, -1.0, 0.0, 0.0));
    // Lights base 2/3: +1.0 would exceed 1 → clamped → lossy.
    assert!(tone_curve_roundtrip_is_lossy(0.0, 0.0, 1.0, 0.0));
    // Typical representable adjustments are not lossy.
    assert!(!tone_curve_roundtrip_is_lossy(0.25, -0.1, 0.1, -0.25));
    assert!(!tone_curve_roundtrip_is_lossy(0.0, 0.0, 0.0, 0.0));
}

// ---- REVIEW-GUI-DEBOUNCE-1: debounce wait schedules its own repaint ----

#[test]
fn full_render_debounce_remaining_math() {
    // No drag recorded → immediate render (None).
    assert_eq!(full_render_debounce_remaining(0.0, 500.0), None);
    // Debounce elapsed → due now.
    assert_eq!(full_render_debounce_remaining(10.0, 10.2), None);
    // Still inside the window → the remaining wait, so the caller can
    // request a timed repaint instead of stranding the draft preview.
    let remaining = full_render_debounce_remaining(10.0, 10.05).unwrap();
    assert!((remaining - 0.100).abs() < 1e-9, "got {remaining}");
    let boundary = full_render_debounce_remaining(10.0, 10.0 + 0.150).unwrap_or(0.0);
    assert_eq!(boundary, 0.0, "at the boundary the render is due");
}

// ---- REVIEW-GUI-MASKRENDER-1: layer edits schedule a render ----

#[test]
fn mask_layer_edits_route_through_mark_dirty() {
    let mut app = new_app();
    app.load_bytes(png(), "layer.png").unwrap();
    app.create_mask("Subject").unwrap();
    app.render().unwrap();
    assert!(app.render_key().is_some());

    for edit in [
        |app: &mut LuminaApp| app.set_mask_inverted(true),
        |app: &mut LuminaApp| app.set_mask_feather(0.3),
        |app: &mut LuminaApp| app.set_mask_blur(0.2),
        |app: &mut LuminaApp| app.set_mask_density(0.8),
    ] {
        app.render().unwrap();
        assert!(!app.pending_full_render);
        edit(&mut app).unwrap();
        assert!(
            app.pending_full_render,
            "layer edit must schedule the debounced render"
        );
        assert!(
            app.render_key().is_none(),
            "layer edit must invalidate the stale render key"
        );
    }
}

// ---- REVIEW-GUI-N2: recipe restore resolves copies by identity ----

#[test]
fn finish_decode_restores_recipe_by_copy_identity_not_position() {
    use lumina_sidecar::{load_sidecar, save_sidecar as raw_save, sidecar_path_for};
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.5);
    app.save_sidecar();

    // Add a second copy with a clearly different recipe, then REORDER the
    // JSON array so the default copy is no longer at position 0 — the old
    // positional restore picked up the wrong recipe here.
    let sidecar = sidecar_path_for(&source);
    let mut document = load_sidecar(&sidecar).unwrap();
    document
        .duplicate_virtual_copy("vc-original", "vc-2", "Copy 2")
        .unwrap();
    for copy in &mut document.virtual_copies {
        if copy.id == "vc-2" {
            copy.is_default = false;
            copy.recipe.adjustments.insert("exposure".into(), -4.0);
        } else if copy.id == "vc-original" {
            copy.is_default = true;
        }
    }
    document.virtual_copies.reverse();
    raw_save(&sidecar, &document).unwrap();

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(
        reopened.virtual_copy_id, "vc-original",
        "the copy matching the session id must win over array position"
    );
    assert_eq!(
        reopened.recipe().adjustments.get("exposure"),
        Some(&1.5),
        "the restored recipe must come from the identity-matched copy"
    );
}

// ---- REVIEW-GUI-N3: file switch resets viewport/session state ----

#[test]
fn loading_a_new_image_resets_viewport_and_session_state() {
    let mut app = new_app();
    app.load_bytes(png(), "a.png").unwrap();
    app.preview_zoom = 8.0;
    app.zoom_mode = ZoomMode::Custom;
    app.preview_pan = egui::vec2(42.0, -17.0);
    app.preview_roi = Some([0, 0, 1, 1]);
    app.before_after = true;
    app.wb_pick_mode = true;
    app.red_eye_pick_mode = true;
    app.history_selected = Some("history-stale".into());
    app.drag_start = Some(Point2 { x: 0.1, y: 0.1 });
    app.drawing = true;

    app.load_bytes(png(), "b.png").unwrap();
    assert_eq!(app.preview_zoom, 1.0, "zoom resets on file switch");
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert_eq!(app.preview_pan, egui::Vec2::ZERO);
    assert_eq!(app.preview_roi, None);
    assert!(!app.before_after, "Before/After must reset");
    assert!(!app.wb_pick_mode, "WB eyedropper must disarm");
    assert!(!app.red_eye_pick_mode, "red-eye picker must disarm");
    assert_eq!(app.history_selected, None);
    assert_eq!(app.drag_start, None);
    assert!(!app.drawing);
}

/// G-14 H2: `Esc` cancels an armed red-eye region picker (and the WB
/// eyedropper / spot tool it shares the cancel path with). The recipe is
/// untouched. The real `Esc` key event is driven headlessly.
#[test]
fn escape_cancels_armed_preview_pickers() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(png(), "test.png").unwrap();
    let recipe = app.recipe().clone();

    let escape = |app: &mut LuminaApp| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1024.0, 720.0),
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
            |ui| app.handle_escape_shortcut(ui.ctx()),
        );
        output.textures_delta.clear();
    };

    app.arm_wb_picker();
    assert!(app.wb_pick_mode);
    escape(&mut app);
    assert!(!app.wb_pick_mode, "Esc must disarm the WB eyedropper");

    app.set_red_eye_pick_mode(true);
    app.set_spot_tool(SpotTool::Heal);
    assert!(!app.red_eye_pick_mode && app.spot_tool == SpotTool::Heal);
    escape(&mut app);
    assert!(!app.red_eye_pick_mode, "Esc must disarm the red-eye picker");
    assert_eq!(app.spot_tool, SpotTool::None);
    assert_eq!(app.recipe().adjustments, recipe.adjustments);
    assert_eq!(app.recipe().red_eye, recipe.red_eye);
}

// ---- REVIEW-GUI-N5: draft preview is never silently measured ----

#[test]
fn match_total_exposure_commits_draft_before_measuring() {
    let mut app = new_app();
    app.load_bytes(png(), "draft.png").unwrap();
    app.render().unwrap();
    app.set_adjustment("exposure", 0.5);
    // Simulate the drag-draft state the hot path produces.
    app.render_draft([800, 600], None).unwrap();
    assert!(app.preview_is_draft(), "precondition: preview is a draft");

    app.match_total_exposure(0.5).unwrap();
    assert!(
        !app.preview_is_draft(),
        "matching must measure the committed full render, not the draft"
    );
    assert!(app.recipe().auto_features.matched_exposure.is_some());
}

// ---- REVIEW-GUI-N6: failed ROI crop clears preview_roi ----

#[test]
fn failed_roi_crop_falls_back_to_full_frame_and_clears_preview_roi() {
    let mut app = new_app();
    app.load_bytes(png(), "roi.png").unwrap(); // 2×1 image

    // A zero-sized crop request genuinely fails; the full frame is
    // rendered, and the rejected ROI must NOT be recorded (it feeds the
    // pointer→source mapping).
    app.render_full([800, 600], Some([0, 0, 0, 9999])).unwrap();
    assert_eq!(app.preview_roi, None);

    // An oversized request is clamped by `crop_region`; the *effective* rect is
    // recorded so the mapping stays truthful. R3-RENDER-SIZE-1 caps the 2×1
    // source at the viewport cap (floored to 256 device px) — the source is
    // already far below the cap, so no downscale happens and the recorded rect
    // stays full-source geometry ([0, 0, 2, 1]).
    app.render_full([800, 600], Some([0, 0, 9999, 9999]))
        .unwrap();
    assert_eq!(app.preview_roi, Some([0, 0, 2, 1]));

    // A valid sub-rect is recorded unchanged.
    app.render_full([800, 600], Some([1, 0, 1, 1])).unwrap();
    assert_eq!(app.preview_roi, Some([1, 0, 1, 1]));
}

// ---- KONSISTENZ (REVIEW-CLI-N1): composite zdata tile key ----

#[test]
fn load_mask_planes_reads_composite_tile_key_and_legacy_fallback() {
    use lumina_sidecar::{save_zdata, zdata_path_for, MaskTile, ZDataContainer};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Subject").unwrap();
    // The plane loader only picks Valid masks; a hand-drawn prompt mask is
    // complete without a model, so mark it valid directly.
    {
        let document = app.document.as_mut().unwrap();
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|c| c.id == app.virtual_copy_id)
            .unwrap();
        let mask = copy.mask_library.iter_mut().find(|m| m.id == id).unwrap();
        mask.status = MaskStatus::Valid;
    }

    let zdata_path = zdata_path_for(&source);
    let tile = |mask_id: String| MaskTile {
        mask_id,
        tile_x: 0,
        tile_y: 0,
        width: 2,
        height: 1,
        values: vec![u16::MAX, 0],
    };

    // 1) Composite key `"{copy_id}/{mask_id}"` (shared with the CLI).
    save_zdata(
        &zdata_path,
        &ZDataContainer::new(vec![tile(LuminaApp::zdata_tile_record_id(
            "vc-original",
            &id,
        ))])
        .unwrap(),
    )
    .unwrap();
    let planes = app.load_mask_planes();
    assert!(
        planes.contains_key(&("vc-original".to_string(), id.clone())),
        "composite-keyed tile must load under (copy_id, mask_id)"
    );

    // 2) Legacy containers carry the bare mask id; they stay readable via
    // the documented, logged fallback.
    save_zdata(
        &zdata_path,
        &ZDataContainer::new(vec![tile(id.clone())]).unwrap(),
    )
    .unwrap();
    let planes = app.load_mask_planes();
    assert!(
        planes.contains_key(&("vc-original".to_string(), id)),
        "legacy bare-mask-id tiles must remain readable"
    );
}

// ---- R2-JANK-1 F1: draft tick frame budget ----

/// F1: two draft ticks inside one 16 ms budget execute at most one CPU draft
/// render; once the budget elapsed the next tick renders again.
#[test]
fn draft_tick_throttles_to_the_frame_budget() {
    let mut app = new_app();
    app.load_bytes(png(), "throttle.png").unwrap();
    app.set_adjustment("exposure", 0.25);
    assert!(app.render_key.is_none(), "edit arms the draft path");

    app.render_draft_tick_at([800, 600], 1.000);
    let after_first = app.preview_generation();
    assert!(after_first > 0, "the first tick must render");

    app.render_draft_tick_at([800, 600], 1.005);
    assert_eq!(
        app.preview_generation(),
        after_first,
        "a second tick inside the 16 ms budget must not re-render"
    );
    assert_eq!(
        app.draft_throttle.last_render_time(),
        1.000,
        "the render anchor is unchanged by a throttled tick"
    );

    app.render_draft_tick_at([800, 600], 1.020);
    assert!(
        app.preview_generation() > after_first,
        "past the budget the draft renders again"
    );
}

/// F1: a throttled tick keeps the pending render key invalid, so the visible
/// "Stale" state never silently disappears while the draft lags the recipe.
#[test]
fn throttled_draft_keeps_the_stale_render_key() {
    let mut app = new_app();
    app.load_bytes(png(), "stale.png").unwrap();
    app.set_adjustment("exposure", 0.1);
    app.render_draft_tick_at([800, 600], 2.000);
    assert!(app.render_key.is_some(), "the first tick renders");

    app.set_adjustment("exposure", 0.2);
    assert!(app.render_key.is_none(), "the new edit invalidates the key");
    app.render_draft_tick_at([800, 600], 2.005); // inside the budget
    assert!(
        app.render_key.is_none(),
        "a throttled draft must leave the key invalid → 'Stale' stays visible"
    );
}

// ---- R2-JANK-1 F4: draft analysis cadence ----

/// F4: inside the analysis period a rendered draft keeps the previous analysis
/// and marks it pending; the released full render clears the marker.
#[test]
fn draft_analysis_cadence_marks_pending_until_the_full_render() {
    let mut app = new_app();
    app.load_bytes(png(), "analysis.png").unwrap();
    app.set_adjustment("exposure", 0.1);
    app.render_draft_tick_at([800, 600], 1.000);
    assert!(
        !app.draft_throttle.analysis_pending(),
        "the first draft analysis runs"
    );
    let histogram = app.preview_histogram.clone();

    app.set_adjustment("exposure", 0.2);
    app.render_draft_tick_at([800, 600], 1.050); // render due, analysis not
    assert!(
        app.draft_throttle.analysis_pending(),
        "a throttled analysis must be visibly pending"
    );
    assert_eq!(
        app.preview_histogram, histogram,
        "the previous analysis stays displayed (never an empty histogram)"
    );

    app.render_full([800, 600], None).unwrap();
    assert!(
        !app.draft_throttle.analysis_pending(),
        "the released full render runs the analysis and clears the marker"
    );
}

/// F4: the pending analysis is a visible preview-state label — never a silent
/// stale histogram.
#[test]
fn pending_analysis_is_painted_as_a_preview_label() {
    let mut app = new_app();
    app.load_bytes(png(), "label.png").unwrap();
    app.render().unwrap();
    app.preview_is_draft = true;

    app.draft_throttle.note_analysis_pending();
    let shapes = preview_area_badge_shapes(&mut app);
    assert!(
        text_contains(&shapes, crate::draft_throttle::DRAFT_ANALYSIS_PENDING_LABEL),
        "the pending-analysis marker must be painted in the preview area"
    );

    app.draft_throttle.note_analysis();
    let shapes = preview_area_badge_shapes(&mut app);
    assert!(
        text_contains(&shapes, "Draft"),
        "without a pending analysis the ordinary Draft badge shows"
    );
    assert!(
        !text_contains(&shapes, crate::draft_throttle::DRAFT_ANALYSIS_PENDING_LABEL),
        "the pending marker disappears once the analysis is current"
    );
}

/// F4: the analysis runs at its cadence, not once per draft tick. Three draft
/// renders inside the period all update the pixels while keeping the previous
/// analysis (pending); the first render past the period recomputes it and
/// clears the marker.
#[test]
fn draft_analysis_runs_at_the_cadence_not_per_tick() {
    let mut app = new_app();
    app.load_bytes(png(), "cadence.png").unwrap();
    app.set_adjustment("exposure", 0.1);
    app.render_draft_tick_at([800, 600], 1.000);
    assert!(
        !app.draft_throttle.analysis_pending(),
        "the first draft analysis always runs"
    );

    for (i, (t, value)) in [(1.050, 0.2), (1.100, 0.3), (1.140, 0.4)]
        .into_iter()
        .enumerate()
    {
        app.set_adjustment("exposure", value);
        app.render_draft_tick_at([800, 600], t);
        assert!(
            app.render_key.is_some(),
            "draft render {i} inside the budget must land"
        );
        assert!(
            app.draft_throttle.analysis_pending(),
            "tick {i} inside the analysis period skips the analysis and marks it pending"
        );
    }

    // Past the analysis period the cadence elapses: recompute. (200 ms, not
    // the exact 150 ms boundary — the literal `1.150 - 1.000` is not exactly
    // `DRAFT_ANALYSIS_PERIOD_SECONDS` in f64; the pure unit test above pins the
    // boundary itself.)
    app.set_adjustment("exposure", 0.9);
    app.render_draft_tick_at([800, 600], 1.200);
    assert!(
        !app.draft_throttle.analysis_pending(),
        "past the cadence the analysis runs again and the marker clears"
    );
}
