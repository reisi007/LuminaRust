use super::*;

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-OVERLAYS-1: Develop preview overlays + navigator states.
//
// Four overlay goldens (mask matte, edit pins, crop rect, lens-blur focus
// rect) plus the navigator closed golden; the open state is already pinned
// by `navigator_viewport`, and `navigator_open_closed_matrix` below drives
// both rail states headless (open → closed → open).
//
// Pattern per test (68215fd/7fd528b/dd73806/182dfc1): deterministic seeds
// through the public API, non-vacuous model guard + on-screen assert, cursor
// parked outside the frame, then `snapshot`. The mask-matte follow-up
// required a production fix (retained overlay texture) plus its historical
// rebaseline. R5-MASKVIS-25 intentionally rebaselines only `develop_overlay_pins`
// below because opening Masking exposes the new Tool overlay/Panels controls.
//
// The overlays themselves are Painter content (invisible to AccessKit, like
// the G-11 pins documented on `visible_edit_pins`), so the on-screen assert
// is an overlay-widget label (panel control scrolled into view) while the
// model guard proves the overlay state is armed. Every preview loads a real
// source and renders it (`load_sample` synchronously, `open_file` pumped to
// `preview_generation() >= 1`, placeholder label asserted absent) — no
// empty-preview goldens. Snapshot-visible are both the painter primitives
// (pin circle, crop/lens strokes, observed) AND the texture blits: the
// `library_loupe` golden proves texture pixels are captured, and the mask
// matte is asserted directly by `assert_mask_overlay_visible` below. The mask
// golden therefore pins the armed state, the panel layout and the matte
// pixels; the DoD §6 Vision check confirms each layout.
//
// Disk discipline: `commit_spot_heal` + `create_mask` + `set_pin_visibility`
// are pure session state (`mark_dirty` only, the debounced commit renders
// without saving), so the pins test stays path-less. Prompt commits
// (`commit_gradient`), crop and lens-blur edits route through the save path
// (`save_sidecar` / debounced `mark_recipe_dirty`), which shows an error on
// a path-less harness — those three tests therefore open a real `photo.png`
// in a tempdir (fixed file name, so `Loaded: photo.png` stays
// deterministic) and point the browser back at `LIBRARY_FIXTURE_DIR`
// afterwards (`open_file_and_restore_fixture`); `assert_no_tmp_leak` proves
// the random tempdir prefix never reaches pixels.
// ---------------------------------------------------------------------------

/// Assert two preview-space points coincide within a subpixel epsilon.
/// Overlay rects are `f32` math (`x * width` on normalized seeds like 0.1),
/// so exact `assert_eq!` fails on 0.5-ulp rounding (e.g. `(0.1 + 0.8) * 400`
/// is not exactly `360.0`); 1e-3 is far below a pixel and keeps the mapping
/// guard exact in intent.
fn assert_pos_near(actual: eframe::egui::Pos2, expected: eframe::egui::Pos2) {
    assert!(
        (actual.x - expected.x).abs() < 1e-3 && (actual.y - expected.y).abs() < 1e-3,
        "expected {expected:?}, got {actual:?}"
    );
}

/// Non-vacuous pixel guard for the mask matte (`KITTEST-COVERAGE-OVERLAYS-1`):
/// renders the current frame and asserts that red-dominant matte pixels are
/// present in quantity and extent. Before the `draw_mask_overlay` lifetime fix
/// the matte texture was freed in the same frame it was uploaded, so the tint
/// never reached the framebuffer and the preview area contained only the
/// sample image's single native-size red texel (roughly a handful of pixels).
///
/// Deliberately NOT a fixed pane coordinate / exact count: the matte spans the
/// fitted full-frame rect (hundreds of points in each dimension), so we assert
/// a lower-bound on the *order of magnitude* (`>= 5_000` pixels) plus a large
/// bounding box. Panel changes shift the constant ~600-pixel "Overlay color"
/// red swatch but can never produce a thousands-scale, several-hundred-point
/// wide red region — only the matte can. `r > g + 20 && r > b + 20` isolates
/// the `[255, 0, 0]` tint from the near-neutral UI while tolerating the
/// translucent composite over the preview pixels.
fn assert_mask_overlay_visible(harness: &mut Harness<'_, LuminaApp>) {
    let rendered = harness.render().expect("kittest renders the frame");
    let (width, height) = rendered.dimensions();
    let mut red_dominant = 0usize;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (width, height, 0u32, 0u32);
    for (x, y, pixel) in rendered.enumerate_pixels() {
        let [r, g, b, _a] = pixel.0;
        if r > g.saturating_add(20) && r > b.saturating_add(20) {
            red_dominant += 1;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    assert!(
        red_dominant >= 5_000,
        "mask matte must paint a thousands-scale red-dominant area, got {red_dominant} pixels"
    );
    let bbox_w = max_x - min_x;
    let bbox_h = max_y - min_y;
    assert!(
        bbox_w >= 200 && bbox_h >= 200,
        "mask matte must span the fitted preview rect, got a {bbox_w}x{bbox_h} red bounding box"
    );
}

/// Mask matte overlay (`draw_mask_overlay`): a saved gradient prompt paints
/// the translucent tint over the preview (Show on + `OverlayMode::Always`
/// default, selected mask visible). Seed goes through the public
/// `commit_gradient` API (`ensure_selected_mask` auto-creates "Mask 1"; the
/// sidecar write lands in the tempdir only).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_mask() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo, ready_rendered());
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness.state_mut().set_section_open(SECTION_MASKING, true);
    harness
        .state_mut()
        .commit_gradient(
            lumina_sidecar::Point2 { x: 0.1, y: 0.2 },
            lumina_sidecar::Point2 { x: 0.9, y: 0.8 },
        )
        .expect("seed gradient prompt");
    // `apply_mask_prompt` behind `commit_gradient` invalidates the render
    // identity but does not arm a re-render, leaving a stale frame (and a
    // stale status). `set_zoom_mode(Fit)` is display-neutral (already Fit)
    // and re-arms the render through public API, so the guards below prove
    // the post-seed state instead of the pre-seed frame. (The matte is a
    // retained texture blit that IS captured — see `assert_mask_overlay_visible`
    // — so this golden pins red matte pixels, the armed overlay state and the
    // Masking panel.)
    let ready_gen = harness.state_mut().preview_generation();
    harness.state_mut().set_zoom_mode(ZoomMode::Fit);
    harness.run();
    harness.run();
    harness.run();
    // Non-vacuous guards: the seeded recipe really re-rendered (no stale
    // frame), the source is loaded (no placeholder), the prompt persisted
    // (Valid) on the selected mask, and the Show+mode gate allows the
    // overlay — otherwise the golden below could pass on an unpainted
    // preview.
    assert!(
        harness.state_mut().preview_generation() > ready_gen,
        "seeded prompt must re-render (stale frames hide the overlay)"
    );
    assert_preview_loaded(&mut harness);
    assert!(
        harness.state_mut().selected_mask_id().is_some(),
        "gradient seed must leave a selected mask"
    );
    assert_eq!(
        harness
            .state_mut()
            .selected_mask_status()
            .map(|(status, _)| status),
        Some(lumina_sidecar::MaskStatus::Valid),
        "saved prompt must persist as Valid"
    );
    assert!(
        harness.state_mut().mask_overlay_allowed(),
        "Show switch + Always mode + visible selection must allow the overlay"
    );
    // On-screen assert: the Masking panel scrolled to its entry row, proving
    // the scrolled layout.
    expand_and_scroll_to(&mut harness, SECTION_MASKING, "New Mask");
    assert_label_on_screen(&mut harness, "New Mask");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    // Non-vacuous pixel assert on exactly the frame that is snapshotted below:
    // the matte is a texture blit (captured in this environment,
    // KITTEST-COVERAGE-OVERLAYS-1) and must paint a thousands-scale red-dominant
    // area over the fitted preview rect — the earlier lifetime bug left the
    // preview blank (only the sample's single native-size red texel).
    assert_mask_overlay_visible(&mut harness);
    harness.snapshot("develop_overlay_mask");
}

/// Edit pins (`draw_edit_pins`): one spot-heal pin painted as a numbered
/// circle on the preview. `commit_spot_heal` is pure session state
/// (`mark_dirty` only — the debounced commit renders without saving), so
/// this test stays path-less on `load_sample`; `create_mask` (same
/// in-memory precedent as `develop_section_masking`) provides the document
/// the Masking section needs to render the "Edit pins" control. No prompt
/// is seeded, hence no mask pin — the single spot pin is the `>= 1` pin.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_pins() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    // R5-MASKVIS-25: this is the sole intentional existing-golden layout
    // rebaseline. Opening the view exposes the new Tool overlay and Panels
    // controls; the preview, navigator and numbered spot pin remain unchanged.
    harness.state_mut().set_section_open(SECTION_MASKING, true);
    harness
        .state_mut()
        .create_mask("Snapshot Pins")
        .expect("seed mask entry");
    harness
        .state_mut()
        .commit_spot_heal(
            lumina_sidecar::Point2 { x: 0.25, y: 0.5 },
            2.0,
            0.5,
            lumina_sidecar::Point2 { x: 0.5, y: 0.0 },
            1.0,
        )
        .expect("seed spot heal");
    harness
        .state_mut()
        .set_pin_visibility(PinVisibility::Always);
    // Non-vacuous guards: pins visible without an armed tool, exactly the
    // seeded spot pin at its normalized anchor with label "1" — otherwise
    // the golden below could pass on a pin-less preview.
    assert!(
        harness.state_mut().pins_visible(),
        "Always must show pins without an armed tool"
    );
    let pins = harness.state_mut().visible_edit_pins();
    assert_eq!(pins.len(), 1, "expected exactly the seeded spot pin");
    assert_eq!(pins[0].label, "1");
    assert!(
        (pins[0].pos.0 - 0.25).abs() < 1e-6 && (pins[0].pos.1 - 0.5).abs() < 1e-6,
        "spot pin must sit at its seeded anchor, got {:?}",
        pins[0].pos
    );
    harness.run();
    harness.run();
    // Non-vacuous guards: the seeded spot really re-rendered (no stale
    // frame), the source is loaded (no placeholder) — plus the pin model
    // above. The pin circle is a painter primitive (snapshot-visible,
    // observed in this golden); the preview texture blit is captured as well
    // (see `assert_preview_loaded`).
    assert!(
        harness.state_mut().preview_generation() > 1,
        "seeded spot must re-render past the load frame"
    );
    assert_preview_loaded(&mut harness);
    // On-screen assert: the pin-visibility control scrolled into view, and
    // the pin circle paints over the preview (primitive content).
    expand_and_scroll_to(&mut harness, SECTION_MASKING, "Edit pins");
    assert_label_on_screen(&mut harness, "Edit pins");
    assert_label_on_screen(&mut harness, "Tool overlay");
    assert_label_on_screen(&mut harness, "Panels (Tab)");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_pins");
}

/// Crop-rectangle overlay (`draw_crop_overlay`): the active free-crop rect
/// paints as a white stroke over the preview (`OverlayMode::Always` default
/// shows it without arming crop mode). `set_crop_free` routes through the
/// debounced save path, hence the tempdir-backed harness (the save lands in
/// the tempdir only).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_crop() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo, ready_rendered());
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness
        .state_mut()
        .set_crop_free(0.1, 0.1, 0.8, 0.6)
        .expect("seed free crop");
    // Non-vacuous guards: the recipe carries the rect, the seeded edit
    // really re-rendered (no stale frame), the source is loaded (no
    // placeholder), and the pure mapping helper resolves the rect onto a
    // 400x300 rect at (40,30)-(360,210) — otherwise the golden could pass
    // on a rect-less preview. Sample dims come from a real decode (4x3),
    // not a literal. The rect stroke is a painter primitive
    // (snapshot-visible, observed in this golden).
    let ready_gen = harness.state_mut().preview_generation();
    assert_preview_loaded(&mut harness);
    {
        let frame = ImageFrame::decode(&LuminaApp::sample_image_png()).expect("sample decodes");
        assert_eq!((frame.width(), frame.height()), (4, 3));
        let app = harness.state_mut();
        let crop = app
            .recipe()
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .expect("crop rect seeded");
        assert!(
            matches!(crop, lumina_sidecar::Crop::Free { .. }),
            "expected the seeded free rect"
        );
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(400.0, 300.0),
        );
        let overlay = LuminaApp::crop_overlay_rect(rect, Some(crop), frame.width(), frame.height())
            .expect("free crop must map");
        assert_pos_near(overlay.min, eframe::egui::pos2(40.0, 30.0));
        assert_pos_near(overlay.max, eframe::egui::pos2(360.0, 210.0));
    }
    // On-screen assert: the Geometry Crop controls scrolled into view; the
    // rect stroke is a painter primitive (snapshot-visible, observed here).
    // The scroll frames also settle the debounced post-seed render.
    expand_and_scroll_to(&mut harness, SECTION_GEOMETRY, "Crop");
    assert_label_on_screen(&mut harness, "Crop");
    assert!(
        harness.state_mut().preview_generation() > ready_gen,
        "seeded crop must re-render past the load frame"
    );
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_crop");
}

/// Scroll the Optics "Lens Blur" nested subgroup into view and click it open
/// (same below-fold lesson as `develop_section_optics2`: scroll first, then
/// click, then re-scroll with the 2-frame settle).
fn open_lens_blur_subgroup(harness: &mut Harness<'_, LuminaApp>) {
    expand_and_scroll_to(harness, SECTION_OPTICS, "Lens Blur");
    let clicked = harness
        .query_all_by_label("Lens Blur")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "Lens Blur subgroup not found in headed harness");
    harness.run();
    let found = harness
        .query_all_by_label("Lens Blur")
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(found, "scroll target \"Lens Blur\" lost after expanding");
    harness.run();
    harness.run();
}

/// Lens-blur focus-rect overlay (`draw_lens_blur_overlay`): the enabled
/// stage's focus rect paints as an accent stroke over the preview. Both
/// setters route through the debounced save path, hence the tempdir-backed
/// harness (saves land in the tempdir only).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_lens_blur() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo, ready_rendered());
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness.state_mut().set_lens_blur_enabled(true);
    harness
        .state_mut()
        .set_lens_blur_focus_rect(0.2, 0.2, 0.6, 0.5)
        .expect("seed focus rect");
    // Non-vacuous guards: the stage enabled, the source loaded (no
    // placeholder), and the pure mapping helper resolves the seeded rect
    // onto a 400x300 rect at (80,60)-(320,210) — otherwise the golden could
    // pass on a rect-less preview. The focus-rect stroke is a painter
    // primitive (snapshot-visible, observed in this golden).
    let ready_gen = harness.state_mut().preview_generation();
    assert_preview_loaded(&mut harness);
    {
        let blur = harness
            .state_mut()
            .lens_blur()
            .expect("lens blur stage seeded");
        assert!(blur.enabled, "lens blur must be enabled");
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(400.0, 300.0),
        );
        let overlay =
            LuminaApp::lens_blur_focus_overlay(rect, Some(&blur)).expect("focus rect must map");
        assert_pos_near(overlay.min, eframe::egui::pos2(80.0, 60.0));
        assert_pos_near(overlay.max, eframe::egui::pos2(320.0, 210.0));
    }
    // On-screen assert: the Lens Blur subgroup header scrolled into view.
    // The scroll frames also settle the debounced post-seed render.
    open_lens_blur_subgroup(&mut harness);
    assert_label_on_screen(&mut harness, "Lens Blur");
    assert!(
        harness.state_mut().preview_generation() > ready_gen,
        "seeded lens blur must re-render past the load frame"
    );
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_lens_blur");
}
