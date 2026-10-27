use super::*;

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_basic() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only the Basic section is expanded; everything else is collapsed.
    collapse_except(&mut harness, &["Basic"]);
    harness.snapshot("develop_basic");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_sections_expanded() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Basic + Color + Masking expanded; the rest collapsed.
    collapse_except(&mut harness, &["Basic", "Color", "Masking"]);
    harness.snapshot("develop_sections_expanded");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_color() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Color open: the HSL mixer renders near the panel top.
    expand_and_scroll_to(&mut harness, SECTION_COLOR, "HSL / Color Mixer");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen,
    // otherwise the golden below could pass on clipped (invisible) pixels.
    assert_label_on_screen(&mut harness, "HSL / Color Mixer");
    harness.snapshot("develop_section_color");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_masking() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // `load_bytes` leaves `document` empty (lazy `ensure_document_loaded`),
    // and `draw_masking` returns early without one — so seed an in-memory
    // mask entry first. `create_mask` is pure session state (no sidecar
    // write, no disk), keeping this snapshot side-effect-free like the rest.
    harness
        .state_mut()
        .create_mask("Snapshot Mask")
        .expect("seed mask entry");
    // Only Masking open: the mask controls render near the panel top.
    expand_and_scroll_to(&mut harness, SECTION_MASKING, "New Mask");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "New Mask");
    harness.snapshot("develop_section_masking");
}

/// KITTEST-COVERAGE-SECTIONS-1: one golden per remaining Develop section.
/// Each test opens exactly one section (the other seven stay closed) and
/// scrolls a section-unique static group label into view, following the
/// KITTEST-EXPANDED-VIEWPORT-1 pattern (`expand_and_scroll_to` +
/// `assert_label_on_screen` + non-vacuous guard). No production code is
/// touched; the five `section_open` sections below need no document seed
/// (they render on `original.is_some()` from `load_sample`).

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_tone_curve() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Tone Curve open: the parametric regions + the interactive
    // point-curve graph. UX-LOOK-TONECURVE-18 replaced the P0/P1 slider rows
    // with the graph, so the scroll target is the graph's group label.
    expand_and_scroll_to(&mut harness, SECTION_TONE_CURVE, "Point curve");
    // Non-vacuous guard: the scrolled-to widget must actually be on-screen,
    // otherwise the golden below could pass on clipped (invisible) pixels.
    assert_label_on_screen(&mut harness, "Point curve");
    harness.snapshot("develop_section_tone_curve");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_detail() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Detail open: Sharpening + Noise Reduction sliders.
    expand_and_scroll_to(&mut harness, SECTION_DETAIL, "Sharpening");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Sharpening");
    harness.snapshot("develop_section_detail");
}

/// LRPAR-G14-DENOISE-IMPL-20 (GUI): the Detail section with the AI-denoise
/// controls enabled and the honest `unavailable` status badge (no weights
/// integrated, F-078 gate). Golden; `#[ignore]`d like every GPU snapshot.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_denoise() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    // A real on-disk source (not `load_bytes`): enabling the stage arms the
    // debounced save/render path, and an in-memory source would raise the
    // "must be loaded via a local path" dialog into the golden.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("sample.png");
    std::fs::write(&source, LuminaApp::sample_image_png()).unwrap();
    pump_until_ready(
        &mut harness,
        &source,
        DecodeSettled::new("preview().is_some()", |app: &LuminaApp| {
            app.preview().is_some()
        }),
        SETTLE_DEADLINE,
    );
    harness
        .state_mut()
        .set_denoise_enabled(true)
        .expect("enabling the denoise stage is a recipe edit");
    expand_and_scroll_to(&mut harness, SECTION_DETAIL, "AI Denoise");
    assert_label_on_screen(&mut harness, "AI Denoise");
    harness.snapshot("develop_section_denoise");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_effects() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Effects open: Vignette + Grain sliders ("Grain" is unique to
    // this section; Optics uses "Vignette (light falloff)").
    expand_and_scroll_to(&mut harness, SECTION_EFFECTS, "Grain");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Grain");
    harness.snapshot("develop_section_effects");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_optics() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Optics open: lens profile picker + manual correction groups.
    expand_and_scroll_to(&mut harness, SECTION_OPTICS, "Lens Correction");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Lens Correction");
    harness.snapshot("develop_section_optics");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_optics2() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Optics open, scrolled to its lower half: the CA group + Lens
    // Blur subgroup sit below the Vignette fold (`develop_section_optics`
    // ends at Vignette C1). Lens Blur is a nested collapsing header
    // (default closed) — scroll it into view first (a below-fold click
    // would be discarded), then click it open so its controls render.
    expand_and_scroll_to(&mut harness, SECTION_OPTICS, "Lens Blur");
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
    // Re-scroll after expanding (the new content moved the layout), same
    // 2-frame settle as `expand_and_scroll_to`.
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
    // Non-vacuous guard: the subgroup header must actually be on-screen,
    // otherwise the golden below could pass on clipped (invisible) pixels.
    assert_label_on_screen(&mut harness, "Lens Blur");
    harness.snapshot("develop_section_optics2");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_geometry() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Geometry open: Crop / Straighten / Perspective controls (no
    // Auto-Upright control exists yet — LRPAR-G06-UPRIGHT-15, Release 1.5).
    expand_and_scroll_to(&mut harness, SECTION_GEOMETRY, "Crop");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Crop");
    harness.snapshot("develop_section_geometry");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_presets() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Presets open: file list + Create & Apply / Save-as-file buttons.
    // No seed needed (the section renders without a document); the preset
    // list itself mirrors the machine-global presets dir, so the folder row
    // carries a machine-specific path (known risk, see report).
    open_collapsing_and_scroll_to(&mut harness, "Presets", "Create & Apply Preset");
    // Non-vacuous guard: the expanded section must expose its action row,
    // otherwise the golden below could pass on a collapsed header.
    assert_label_on_screen(&mut harness, "Create & Apply Preset");
    harness.snapshot("develop_section_presets");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_history() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // `load_bytes` leaves `document` empty, and the History section shows
    // "No sidecar loaded" without one — so seed an in-memory document plus
    // one history step first. `create_mask` (pure session state, Masking
    // precedent above) ensures the document; `create_preset`/`apply_preset`
    // records `history-1`. UX-LOOK-HISTORY-18: the row now renders the
    // structured change plus a timestamp, so the session-only clock override
    // pins it deterministically (the label text is read back from the same
    // formatter the panel uses — layout is pinned, not a literal word choice).
    harness
        .state_mut()
        .set_history_timestamp_override(Some("2026-09-19T12:00:00Z".into()));
    harness
        .state_mut()
        .create_mask("Snapshot Seed")
        .expect("seed mask entry");
    harness.state_mut().set_adjustment("exposure", 0.4);
    harness.state_mut().set_adjustment("contrast", -0.2);
    let preset = harness
        .state_mut()
        .create_preset("Snapshot Seed")
        .expect("seed preset");
    // Move the session recipe away from the preset so applying it is a real,
    // readable change (name + old→new) in the history row.
    harness.state_mut().set_adjustment("exposure", -1.0);
    harness.state_mut().set_adjustment("contrast", 0.5);
    harness
        .state_mut()
        .apply_preset(&preset)
        .expect("seed history entry");
    let label = harness
        .state_mut()
        .history_labels()
        .into_iter()
        .next()
        .expect("one readable history entry");
    // Only History open: the readable entry row.
    open_collapsing_and_scroll_to(&mut harness, "History", &label);
    // Non-vacuous guard: the entry row must actually be on-screen.
    assert_label_on_screen(&mut harness, &label);
    harness.snapshot("develop_section_history");
}
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_section_rating() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Same in-memory document seed as History (`draw_rating_section`
    // shows "No sidecar loaded" without one). Deliberately no
    // `set_rating`: it calls `save_sidecar` (disk write) — rating 0 already
    // renders the star / flag / color-label rows deterministically.
    harness
        .state_mut()
        .create_mask("Snapshot Seed")
        .expect("seed mask entry");
    // Only Rating open: star buttons + flag + color-label rows.
    open_collapsing_and_scroll_to(&mut harness, "Rating", "Color Label");
    settle_render(&mut harness);
    // Non-vacuous guard: the color-label row must actually be on-screen.
    assert_label_on_screen(&mut harness, "Color Label");
    harness.snapshot("develop_section_rating");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn export_module() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Export);
    load_sample(&mut harness);
    harness.run();
    harness.snapshot("export_module");
}

/// Changing an adjustment (`set_adjustment`) must invalidate the preview and
/// produce a *new* render, i.e. bump `preview_generation` — even outside a
/// pointer drag (the debounced full render path).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn slider_changes_preview_generation() {
    let mut harness = build_harness();
    load_sample(&mut harness);
    harness.run();
    let before = harness.state_mut().preview_generation();
    assert!(before >= 1, "loaded sample must render at least once");

    harness.state_mut().set_adjustment("exposure", 1.0);
    harness.run();
    let after = harness.state_mut().preview_generation();
    assert!(
        after > before,
        "set_adjustment must re-render (preview_generation {before} -> {after})"
    );
}

/// GUI-HISTOGRAM-1: the histogram renders as a real graphic (filled
/// 256-bin Painter curve with P01/P99 markers) in its own collapsible
/// Develop-panel section instead of the old clip bar in the module bar.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn histogram_graphic() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // The histogram section defaults to open; Basic stays collapsed so the
    // golden focuses on the graphic. Two frames: the first renders and
    // uploads the preview texture, the second paints it.
    harness.run();
    harness.run();
    harness.snapshot("histogram_graphic");
}

/// GUI-PREVIEW-NAV-1: the navigator shows the full image with the viewport
/// rectangle of the zoomed working area (Custom zoom so the rectangle is
/// strictly smaller than the overview).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn navigator_viewport() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    harness.state_mut().set_navigator_open(true);
    // Custom zoom above fit: the viewport rectangle must be smaller than the
    // navigator overview. `zoom_step` pins `Custom` like modifier-wheel zoom.
    // Two frames: the first settles the zoomed render and uploads the preview
    // texture, the second paints the navigator overview from it.
    harness.state_mut().zoom_step(4.0);
    harness.run();
    harness.run();
    harness.snapshot("navigator_viewport");
}

/// Navigator closed: the rail is gone, the preview header offers the
/// "Navigator" reopen button. The "‹" collapse button exists exactly once
/// and only while the rail is open, so its absence is the non-vacuous
/// closed proof (the "Navigator" label alone is ambiguous — heading and
/// button share it).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn navigator_closed() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.state_mut().set_navigator_open(false);
    // Two frames: the first settles the closed layout and uploads the
    // preview texture, the second paints without the rail.
    harness.run();
    harness.run();
    // Non-vacuous guards: the source is loaded (no placeholder) and the
    // rail-only collapse button is gone (rail really closed, not just
    // scrolled away).
    assert_preview_loaded(&mut harness);
    assert!(
        harness.query_all_by_label("‹").next().is_none(),
        "rail collapse button must be gone when the navigator is closed"
    );
    // On-screen assert: the preview-header reopen button (the closed-state
    // navigator affordance).
    assert_label_on_screen(&mut harness, "Navigator");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("navigator_closed");
}

/// Navigator state matrix, headless (no golden): open → closed → open via
/// the public setter. The open golden is `navigator_viewport`, the closed
/// golden is `navigator_closed`; this test proves both states are
/// reachably distinct through the rail-only "‹" collapse button plus the
/// shared "Navigator" label, with real pixels throughout.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn navigator_open_closed_matrix() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    // Open: rail heading + collapse button pixel-visible, source loaded.
    harness.state_mut().set_navigator_open(true);
    harness.run();
    harness.run();
    assert_preview_loaded(&mut harness);
    assert_label_on_screen(&mut harness, "Navigator");
    assert_label_on_screen(&mut harness, "‹");
    // Closed: collapse button gone, reopen button in its place.
    harness.state_mut().set_navigator_open(false);
    harness.run();
    harness.run();
    assert!(
        harness.query_all_by_label("‹").next().is_none(),
        "rail collapse button must be gone when closed"
    );
    assert_label_on_screen(&mut harness, "Navigator");
    // Re-open: the rail comes back (no stuck-closed state).
    harness.state_mut().set_navigator_open(true);
    harness.run();
    harness.run();
    assert_label_on_screen(&mut harness, "‹");
    assert_label_on_screen(&mut harness, "Navigator");
}

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-STATES-1: UI states pixel-visible per kittest golden —
// toast (info), error popup dialog, empty states, missing-sidecar hint, the
// metadata panel's own copy/paste system and the 20-dummy filmstrip.
//
// Pattern per test (KITTEST-COVERAGE-*): deterministic seeds through the
// public API, a non-vacuous model/label guard + on-screen assert, then
// `snapshot`. No existing golden is rebaselined.
// ---------------------------------------------------------------------------

/// Info toast (`show_toast`): the transient overlay owns the preview-ready
/// signal. `show_toast` is shown at an egui-time far above the headless clock
/// so it stays visible when the frame is read back (same trick as the
/// agent-harness `toast_overlap` probe).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn toast_info() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness
        .state_mut()
        .show_toast("Preview ready".to_string(), 1000.0);
    harness.run();
    harness.run();
    // Non-vacuous model guard: the toast state machine is armed.
    assert!(
        harness.state_mut().toast_visible(1000.0),
        "info toast must be visible after show_toast"
    );
    // The overlay Area is exposed to AccessKit; when it is, guard the pixels.
    if harness.query_all_by_label("Preview ready").next().is_some() {
        assert_label_on_screen(&mut harness, "Preview ready");
        assert_label_on_screen(&mut harness, "Dismiss");
    }
    harness.snapshot("toast_info");
}

/// Error popup dialog: `show_error` surfaces a failure as a floating dialog
/// (and logs it at `error!`). Triggered through the Export panel's Export
/// button with the default empty destination → the fixed code literal
/// "Choose an export target first" (machine-independent golden text).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn error_dialog() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Export);
    load_sample(&mut harness);
    harness.run();
    // Two Button-role "Export" nodes exist (module bar + run button); the run
    // button is the one in the right panel.
    let clicked = harness
        .query_all_by_label("Export")
        .find(|node| {
            node.accesskit_node().role() == eframe::egui::accesskit::Role::Button
                && node.rect().min.x > 600.0
        })
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "export run button not found in headed harness");
    harness.run();
    harness.run();
    // Non-vacuous model guard: the failure reached the loud path.
    assert!(
        harness.state_mut().error().is_some(),
        "empty destination must raise the loud export error"
    );
    assert_eq!(harness.state_mut().status(), "Error");
    // Dialog content is on-screen, not just in the tree.
    assert_label_on_screen(&mut harness, "Close");
    assert_label_on_screen(&mut harness, "Choose an export target first");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("error_dialog");
}

/// Empty Develop state (no image loaded): the centered preview placeholder and
/// the honest empty filmstrip text — the counterpart to `library_empty`.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_empty() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    harness.run();
    // Non-vacuous guards: both empty-state texts must actually be on-screen.
    assert_label_on_screen(&mut harness, "Drop an image here or load a path");
    assert_label_on_screen(&mut harness, "No images in this folder");
    harness.snapshot("develop_empty");
}

/// Missing-sidecar hint: the Develop History section without a loaded document
/// shows "No sidecar loaded". `load_bytes` leaves `document` empty, so no seed
/// is needed (the existing `develop_section_history` golden seeds one).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn develop_history_no_sidecar() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only History open (a plain collapsing header outside the eight F-100
    // sections); scroll its content into view.
    open_collapsing_and_scroll_to(&mut harness, "History", "No sidecar loaded");
    // Non-vacuous guard: the hint must actually be on-screen.
    assert_label_on_screen(&mut harness, "No sidecar loaded");
    harness.snapshot("develop_history_no_sidecar");
}

/// Export-panel variant: JPEG selected via the format setter (PNG is the
/// default) — the PNG-only "Quality applies to JPEG / WebP only" hint goes
/// away and the quality slider is live. No metadata flag exists in the GUI
/// (metadata is a Library concern); the Library metadata copy/paste state is
/// pinned separately.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn export_module_jpeg() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Export);
    harness.state_mut().set_export_format(ImageFileFormat::Jpeg);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    // Non-vacuous guard: the PNG-only quality hint is gone (JPEG selected),
    // while the quality control itself stays visible.
    assert!(
        harness
            .query_all_by_label("Quality applies to JPEG / WebP only")
            .next()
            .is_none(),
        "JPEG must drop the PNG-only quality hint"
    );
    assert_label_on_screen(&mut harness, "Quality");
    harness.snapshot("export_module_jpeg");
}

// ---------------------------------------------------------------------------
// GEN-ONNX-1 Welle 2b: Generative Expand / Auto-Fill panel goldens.
//
// The visible GUI change of the wave is the `Generative Expand` section: the
// `Generate` action and the per-role status label (`valid`/`stale`/`missing`/
// `corrupt`), plus the auto-fill toggle. Pattern per test
// (KITTEST-COVERAGE-*): deterministic seeds through the public API, a
// non-vacuous model/label guard + on-screen assert, cursor parked outside the
// frame, then `snapshot`.
//
// The panel is a plain `ui.collapsing("Generative Expand", …)` rendered after
// the eight F-100 sections (not a `section_open` index), so the existing
// `open_collapsing_and_scroll_to` helper opens it. The generate action needs a
// real path to write its `.lumina.zdata` bundle, so those tests load a
// **committed relative fixture** (`tests/fixtures/generative/…`): the Develop
// panel renders the loaded path, and a `tempfile::tempdir` prefix would leak
// nondeterministic pixels into the golden (exactly the `LIBRARY_FIXTURE_DIR`
// rationale — the path field is on-screen at this scroll position, unlike the
// existing overlay goldens). The generated sidecar/`.zdata` are gitignored and
// cleared on every run.
//
// Crop-decision (`keep_generative_content`) goldens are intentionally absent:
// no GUI control exists yet for that field (the wave exposes expand + auto-fill
// only). The auto-fill wedge case is covered by `generative_auto_fill_panel`.
// ---------------------------------------------------------------------------

/// Committed relative fixture directory for the generative panel goldens (see
/// the block comment above for why a tempdir is wrong here).
const GENERATIVE_FIXTURE_DIR: &str = "tests/fixtures/generative";

/// (Re-)write one deterministic generative-panel fixture and clear its stale
/// sidecar/`.zdata` from a previous run. Returns the *relative* source path.
/// Idempotent.
fn ensure_generative_fixture(name: &str, png: Vec<u8>) -> PathBuf {
    let root = Path::new(GENERATIVE_FIXTURE_DIR);
    std::fs::create_dir_all(root).expect("create generative fixture dir");
    let source = root.join(name);
    std::fs::write(&source, png).expect("write generative fixture");
    let _ = std::fs::remove_file(lumina_sidecar::sidecar_path_for(&source));
    let _ = std::fs::remove_file(lumina_sidecar::zdata_path_for(&source));
    source
}

/// The Generative Expand panel in its default (off) state: both toggles off,
/// the hint visible and no generate/status row (no active role). Non-vacuous
/// guard: both toggles are on-screen, so the golden cannot pass on a collapsed
/// header.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn generative_expand_panel_off() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    open_collapsing_and_scroll_to(&mut harness, "Generative Expand", "Expand beyond image");
    assert_label_on_screen(&mut harness, "Expand beyond image");
    assert_label_on_screen(&mut harness, "Auto-fill transparent pixels (after lens)");
    // No active role → no generate/status row (the state this golden pins).
    assert!(
        harness.query_all_by_label("Generate").next().is_none(),
        "an inactive generative edit must not show the Generate action"
    );
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("generative_expand_panel_off");
}

/// The Generative Expand panel after confirming/generating the expand canvas:
/// the canvas row, the `Generate` action and the `valid` status label are
/// pixel-visible. Uses a committed relative PNG so the generate action can
/// persist its sidecar bundle without leaking a tempdir path into the golden;
/// the non-vacuous guard proves the recipe really generated (`Expand: valid`).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn generative_expand_panel_ready() {
    let source = ensure_generative_fixture("photo.png", LuminaApp::sample_image_png());
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &source, ready_rendered());
    // Arm the expand role (loud until a canvas exists) and generate the
    // deterministic fixture canvas + persisted bundle.
    let _ = harness.state_mut().set_expand_beyond_image(true);
    harness
        .state_mut()
        .generate_generative_canvas()
        .expect("generate the expand fixture canvas");
    // Non-vacuous model guard: the recipe is really armed and the canvas
    // resolved valid — otherwise the golden could pass on an empty panel.
    assert!(
        harness
            .state_mut()
            .recipe()
            .generative_edit
            .as_ref()
            .is_some_and(|edit| edit.effective_expand()),
        "the expand role must be armed"
    );
    open_collapsing_and_scroll_to(&mut harness, "Generative Expand", "Generate");
    assert_label_on_screen(&mut harness, "Generate");
    assert_label_on_screen(&mut harness, "Generative canvas — Expand: valid");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("generative_expand_panel_ready");
}

/// The auto-fill role after generation: a deterministic source with a
/// transparent border (the SOLL "Auto-Fill-Keile" case) is healed, and the
/// panel shows `AutoFillTransparent: valid`. Proves the auto-fill toggle +
/// generate path is pixel-visible, not just the expand role.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn generative_auto_fill_panel() {
    // 64x64 with an 8px fully transparent frame: deterministic, no lens model.
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            let alpha = if x < 8 || y < 8 { 0 } else { 255 };
            pixels.extend_from_slice(&[120, 140, 160, alpha]);
        }
    }
    let frame = ImageFrame::new(64, 64, pixels).expect("fixture frame");
    let source = ensure_generative_fixture(
        "auto_fill.png",
        frame.encode(ImageFileFormat::Png).expect("png encodes"),
    );
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &source, ready_rendered());
    let _ = harness.state_mut().set_auto_fill_transparent(true);
    harness
        .state_mut()
        .generate_generative_canvas()
        .expect("generate the auto-fill fixture canvas");
    assert!(
        harness
            .state_mut()
            .recipe()
            .generative_edit
            .as_ref()
            .is_some_and(|edit| edit.auto_fill_transparent.unwrap_or(false)),
        "the auto-fill role must be armed"
    );
    open_collapsing_and_scroll_to(&mut harness, "Generative Expand", "Generate");
    assert_label_on_screen(&mut harness, "Generate");
    assert_label_on_screen(
        &mut harness,
        "Generative canvas — AutoFillTransparent: valid",
    );
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("generative_auto_fill_panel");
}
