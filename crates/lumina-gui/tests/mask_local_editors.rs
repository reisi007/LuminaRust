//! GUI-INT-MASKLOCAL-38: the **clickable** headless proof for the mask-local
//! editors (MASK-LOCAL-P1.2b Color, P1.2c Presence, P1.2d Detail).
//!
//! # What this target is for
//!
//! The mask-local editors shipped **headless** (schema, `lumina-core`,
//! `lumina-cli`, `lumina-sidecar`). Their lib tests prove the setters, and they
//! *paint* each editor — but painting is not interaction, and a module that is
//! never called from a panel is not integrated. This target therefore drives
//! the **real** `LuminaApp` (through `eframe::App::ui`, so the production
//! `LuminaApp::draw_masking` chain runs) inside an `egui_kittest::Harness` and
//! sends **real pointer events at the real widgets**: a multi-frame drag on the
//! real slider rail, a click on the real channel selector, a click on the real
//! reset button. After each gesture the assertion is on the *persisted* state:
//! the public `selected_mask_local_*` getters, the untouched global
//! `recipe()`, and — because the fixture is a real file in a `tempfile::tempdir`
//! — the debounced save's actual sidecar bytes on disk.
//!
//! # Scope split (why three tests here and not four)
//!
//! * **This target** — the three surfaces whose value proof *is* a value: a
//!   slider/selector gesture must move a number that survives to disk. Presence
//!   (2 sliders + block reset), Color (HSL band row, grading range row, point
//!   color), Detail (slider + reset).
//! * [`mask_local_editors_wiring`](../mask_local_editors_wiring.rs) — the
//!   **tone-curve** surface, whose value is a whole point set rather than one
//!   scalar, plus the paint-provenance guard that all four editors are reached
//!   from the Masking section. The curve gestures have their own timing and
//!   geometry rules; they are documented in `mask_local_curve_graph_support`
//!   and keep the harness file small.
//! * [`kittest_mask_local`](../kittest_mask_local.rs) — the four `#[ignore]`d
//!   visual goldens.
//!
//! Both targets share `mask_local_editors_support` (harness, frame clock,
//! label lookups, persisted readback); the slider gestures live in
//! `mask_local_slider_support`, the curve-graph gestures and the curve block's
//! label lookups in `mask_local_curve_graph_support`. A helper only one target
//! calls would be dead code, and `Agents.md` forbids closing a `-D warnings`
//! gate with an `allow` attribute.
//!
//! No GPU and no wgpu adapter are needed: the harness only has to lay out and
//! dispatch input, not rasterize. That keeps this target inside the normal
//! `cargo test -p lumina-gui` run (the kittest **goldens** for all four
//! surfaces live in the separate `kittest_mask_local` target).
//!
//! # Explicit boundary (Agents.md, User-Regel 2026-09-26)
//!
//! This is the only kind of GUI verification the project has: headless
//! `egui` + `LuminaApp` + a tempdir, plus `egui_kittest` goldens over the wgpu
//! adapter as a *local* macOS gate. There is **no** real-window test, and none
//! is claimed: nothing here proves DPI scaling, multi-monitor behaviour, real
//! mouse hardware or a human-visible frame. Those stay a named gap.
//!
//! Normative contract: `feature/product/ai-masks.md`, section
//! „GUI- und kittest-Vertrag der mask-local Editoren (P1.2a–d)".

mod mask_local_editors_support;
mod mask_local_slider_support;
use mask_local_editors_support::*;
use mask_local_slider_support::*;

/// The presence sliders and the block reset are clickable and write through to
/// the mask layer, the sidecar and nothing else.
#[test]
fn the_mask_local_presence_editor_is_clickable_and_writes_through() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let global_before = harness.state().recipe().clone();
    assert_eq!(
        harness.state().selected_mask_local_presence().unwrap(),
        (0.0, 0.0, 0.0)
    );
    assert!(!harness.state().has_mask_local_presence().unwrap());

    // Drag the real dehaze rail.
    let dehaze_rail = slider_rail(&harness, ("Presence", 0), Row::Below, "Dehaze");
    drag_slider(&mut harness, dehaze_rail, 0.8);

    let (texture, clarity, dehaze) = harness.state().selected_mask_local_presence().unwrap();
    assert!(
        dehaze > 0.5,
        "dragging the dehaze rail must change the stored value, got {dehaze}"
    );
    assert!(close(texture, 0.0) && close(clarity, 0.0));
    assert!(
        harness.state().has_mask_local_presence().unwrap(),
        "a non-zero presence block must be reported as pixel-changing"
    );
    // The global presence block stays empty: the local editor never writes
    // `EditRecipe::presence`.
    assert_eq!(harness.state().recipe(), &global_before);
    assert!(harness.state().recipe().presence.is_none());

    // …and the value is in the persisted bytes, not only in memory.
    settle_persisted(&mut harness);
    let persisted = persisted_local_recipe(&dir);
    let stored = persisted
        .presence
        .as_ref()
        .expect("the persisted block must carry presence");
    assert!(close(f64::from(stored.dehaze), dehaze));
    assert!(close(f64::from(stored.texture), 0.0) && close(f64::from(stored.clarity), 0.0));

    // The block reset is a real control and really clears the block.
    let target = only_rect(&harness, "all local presence reset");
    click(&mut harness, target);
    assert!(!harness.state().has_mask_local_presence().unwrap());
    assert_eq!(
        harness.state().selected_mask_local_presence().unwrap(),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(harness.state().recipe(), &global_before);
    settle_persisted(&mut harness);
    assert!(
        persisted_local_recipe(&dir).presence.is_none(),
        "the reset must also reach the sidecar"
    );
}

/// The colour editor's four sub-surfaces — HSL band + slider, point-colour
/// entries, grading range + slider, and the block reset — are clickable and
/// each writes through to the mask layer and the sidecar only.
#[test]
fn the_mask_local_color_editor_is_clickable_and_writes_through() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let global_before = harness.state().recipe().clone();
    assert!(!harness.state().has_mask_local_color().unwrap());
    assert_eq!(
        harness
            .state()
            .selected_mask_local_hsl_band("cyan")
            .unwrap(),
        (0.0, 0.0, 0.0)
    );

    // 1) HSL: pick a band, then drag that band's saturation slider. The band
    //    row sits between the block caption and the first HSL slider.
    let band = only_rect(&harness, "cyan");
    assert!(
        band.min.y >= only_rect(&harness, "HSL / Color Mixer").max.y,
        "the band row must follow the colour block caption"
    );
    click(&mut harness, band);
    // Choosing a band is a view change; the band it selects is the one whose
    // sliders are then editable.
    assert_eq!(harness.state().recipe(), &global_before);
    let saturation_rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Saturation");
    assert!(
        saturation_rail.min.y > band.max.y,
        "the HSL saturation slider must follow the band row"
    );
    drag_slider(&mut harness, saturation_rail, 0.8);
    let (hue, saturation, luminance) = harness
        .state()
        .selected_mask_local_hsl_band("cyan")
        .unwrap();
    assert!(
        saturation > 0.2,
        "dragging the cyan saturation slider must change the stored band, got {saturation}"
    );
    assert!(close(hue, 0.0) && close(luminance, 0.0));
    // A different band is still neutral: the slider edited the *selected* one.
    assert_eq!(
        harness
            .state()
            .selected_mask_local_hsl_band("blue")
            .unwrap(),
        (0.0, 0.0, 0.0)
    );
    assert!(harness.state().has_mask_local_color().unwrap());
    // The global HSL block is untouched.
    assert_eq!(harness.state().recipe(), &global_before);
    assert!(harness.state().recipe().hsl.is_none());
    // The legacy flat adjustment map is a *second* place a global HSL write
    // would show up, so both are asserted to stay empty.
    assert!(!harness
        .state()
        .recipe()
        .adjustments
        .contains_key("saturation"));
    assert!(!harness.state().recipe().adjustments.contains_key("hue"));

    // 2) Point Color: the "Add color" button creates a real entry.
    assert!(harness
        .state()
        .selected_mask_local_point_color()
        .unwrap()
        .is_empty());
    let target = only_rect(&harness, "Add color");
    click(&mut harness, target);
    let entries = harness.state().selected_mask_local_point_color().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "Add color must create one entry: {entries:?}"
    );
    assert!(harness.state().recipe().point_color.is_none());

    // 3) Color Grading: pick a range, then drag its hue slider. "Midtones" is
    //    unique in the panel (the local-adjustment block uses "Highlights" and
    //    "Shadows", not "Midtones").
    let midtones = only_rect(&harness, "Midtones");
    assert!(
        midtones.min.y > only_rect(&harness, "Color Grading").max.y,
        "the grading range row must follow the grading caption"
    );
    click(&mut harness, midtones);
    let grading_rail = slider_rail(&harness, ("Color Grading", 0), Row::Below, "Hue");
    assert!(
        grading_rail.min.y > midtones.max.y,
        "the grading hue slider must follow the range row"
    );
    drag_slider(&mut harness, grading_rail, 0.7);
    let (hue_degrees, grading_saturation, grading_luminance) = harness
        .state()
        .selected_mask_local_grading_range("midtones")
        .unwrap();
    assert!(
        hue_degrees > 200.0,
        "dragging the midtones hue slider (0..=360) must change the stored range, got {hue_degrees}"
    );
    assert!(close(grading_saturation, 0.0) && close(grading_luminance, 0.0));
    assert!(harness.state().recipe().color_grading.is_none());
    assert_eq!(harness.state().recipe(), &global_before);

    // …and all three sub-surfaces are in the persisted bytes.
    settle_persisted(&mut harness);
    let persisted = persisted_local_recipe(&dir);
    let hsl = persisted.hsl.as_ref().expect("persisted HSL block");
    assert!(close(
        f64::from(hsl.cyan.expect("persisted cyan channel").saturation),
        saturation
    ));
    assert_eq!(
        persisted
            .point_color
            .as_ref()
            .map(|block| block.entries.len()),
        Some(1)
    );
    let grading = &persisted
        .color_grading
        .as_ref()
        .expect("persisted grading block")
        .midtones;
    assert!(close(f64::from(grading.hue_degrees), hue_degrees));

    // 4) The block reset clears all of it.
    let target = only_rect(&harness, "all local color reset");
    click(&mut harness, target);
    assert!(!harness.state().has_mask_local_color().unwrap());
    assert_eq!(
        harness
            .state()
            .selected_mask_local_hsl_band("cyan")
            .unwrap(),
        (0.0, 0.0, 0.0)
    );
    assert!(harness
        .state()
        .selected_mask_local_point_color()
        .unwrap()
        .is_empty());
    assert_eq!(
        harness
            .state()
            .selected_mask_local_grading_range("midtones")
            .unwrap(),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(harness.state().recipe(), &global_before);
    settle_persisted(&mut harness);
    let persisted = persisted_local_recipe(&dir);
    assert!(persisted.hsl.is_none() && persisted.point_color.is_none());
    assert!(persisted.color_grading.is_none());
}

/// The detail editor's sharpening and noise-reduction sub-blocks plus both
/// reset levels are clickable and write through.
#[test]
fn the_mask_local_detail_editor_is_clickable_and_writes_through() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let global_before = harness.state().recipe().clone();
    assert!(!harness.state().has_mask_local_sharpening().unwrap());
    assert!(!harness.state().has_mask_local_noise_reduction().unwrap());

    // 1) Sharpening amount. "Amount" is unique in the panel: the global Detail
    //    section is closed, and the local HSL/grading blocks have no amount.
    let amount_rail = slider_rail(&harness, ("Detail", 1), Row::Below, "Amount");
    assert!(
        amount_rail.max.y <= only_rect(&harness, "Noise Reduction").min.y,
        "the sharpening sliders must precede the noise-reduction sub-block"
    );
    drag_slider(&mut harness, amount_rail, 0.7);
    let sharpening = harness.state().selected_mask_local_sharpening().unwrap();
    assert!(
        sharpening.amount > 0.5,
        "dragging the amount slider must change the stored sharpening, got {sharpening:?}"
    );
    assert!(close(f64::from(sharpening.radius), 1.0) && close(f64::from(sharpening.detail), 0.5));
    assert!(harness.state().has_mask_local_sharpening().unwrap());
    assert!(harness.state().recipe().sharpening.is_none());

    // 2) The sub-block reset returns only sharpening to neutral and keeps the
    //    noise-reduction block untouched (it is not neutral yet, so the check
    //    below is meaningful).
    let luminance_rail = slider_rail(&harness, ("Noise Reduction", 0), Row::Below, "Luminance");
    drag_slider(&mut harness, luminance_rail, 0.7);
    let noise = harness
        .state()
        .selected_mask_local_noise_reduction()
        .unwrap();
    assert!(
        noise.luminance > 0.5,
        "dragging the luminance slider must change the stored noise reduction, got {noise:?}"
    );
    assert!(close(f64::from(noise.color), 0.0));
    assert!(harness.state().recipe().noise_reduction.is_none());

    let sharpening_reset = only_rect(&harness, "local Sharpening reset");
    click(&mut harness, sharpening_reset);
    assert!(!harness.state().has_mask_local_sharpening().unwrap());
    let neutral = harness.state().selected_mask_local_sharpening().unwrap();
    assert!(close(f64::from(neutral.amount), 0.0) && close(f64::from(neutral.radius), 1.0));
    assert!(
        harness.state().has_mask_local_noise_reduction().unwrap(),
        "the sharpening reset must not clear the noise-reduction sub-block"
    );
    assert_eq!(harness.state().recipe(), &global_before);

    // …and both sub-blocks are in the persisted bytes.
    settle_persisted(&mut harness);
    let persisted = persisted_local_recipe(&dir);
    let detail = persisted.detail.as_ref().expect("persisted detail block");
    assert!(
        detail.sharpening.is_none(),
        "the sub-block reset must persist"
    );
    let stored_noise = detail
        .noise_reduction
        .as_ref()
        .expect("persisted noise-reduction sub-block");
    assert!(close(
        f64::from(stored_noise.luminance),
        f64::from(noise.luminance)
    ));

    // 3) The block reset clears the rest.
    let detail_reset = only_rect(&harness, "all local detail reset");
    click(&mut harness, detail_reset);
    assert!(!harness.state().has_mask_local_noise_reduction().unwrap());
    assert_eq!(
        harness
            .state()
            .selected_mask_local_noise_reduction()
            .unwrap(),
        lumina_sidecar::NoiseReduction {
            version: lumina_sidecar::DETAIL_BLOCK_VERSION,
            luminance: 0.0,
            color: 0.0,
        }
    );
    assert_eq!(harness.state().recipe(), &global_before);
    settle_persisted(&mut harness);
    assert!(persisted_local_recipe(&dir).detail.is_none());
}
