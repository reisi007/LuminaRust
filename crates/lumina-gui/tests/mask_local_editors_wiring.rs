//! GUI-INT-MASKLOCAL-38: the **paint/provenance guard** for the four
//! mask-local editors (MASK-LOCAL-P1.2a Tone Curves, P1.2b Color, P1.2c
//! Presence, P1.2d Detail).
//!
//! # What this file claims — and what it does not
//!
//! `Agents.md` is explicit that "a module with tests that is never drawn
//! anywhere is **not integrated**", and that a painted-but-not-clickable
//! control proves nothing. Those are two different claims, so they live in two
//! files:
//!
//! * **here** — each of the four editors is painted by its **own** production
//!   `draw_*` path, reached through the real `LuminaApp::draw_masking` chain,
//!   and the labels used to address them belong to the mask-local editors
//!   rather than to the global Develop sections;
//! * `mask_local_editors.rs` — each editor is **clickable** and writes through
//!   to the persisted sidecar.
//!
//! The split is a real module boundary (paint provenance vs. input contract),
//! not a size hack: it also keeps the two failure modes apart, because a
//! mis-wired `draw_` call breaks this file while a non-clickable widget breaks
//! its sibling.
//!
//! The harness, the frame clock and the label lookups are shared with
//! `mask_local_editors_support`; the graph location, the curve gestures and the
//! curve block's label lookups live in `mask_local_curve_graph_support`.

mod mask_local_curve_graph_support;
mod mask_local_editors_support;
use mask_local_curve_graph_support::*;
use mask_local_editors_support::*;

/// `SECTION_MASKING` — the section that hosts all four mask-local editors.
const SECTION_MASKING: usize = lumina_gui::SECTION_MASKING;

/// The Masking panel really contains the four mask-local editors, and the
/// labels used to address them belong to the **local** editors, not to the
/// global sections.
///
/// This is the wiring guard: with every other Develop section closed, a label
/// like "Point curve" or "HSL / Color Mixer" can only come from
/// `draw_mask_local_tone_curve` / `draw_mask_local_color`. Opening the global
/// Tone Curve section afterwards must add a *second* occurrence, which proves
/// the two editors are distinct widgets rather than one shared approximation.
#[test]
fn the_masking_panel_paints_each_mask_local_editor_from_its_own_draw_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);

    // One occurrence each: only the local editor is open.
    for label in [
        "Point curve",
        "HSL / Color Mixer",
        "Presence",
        "Noise Reduction",
    ] {
        assert_eq!(
            nodes(&harness, label).len(),
            1,
            "{label:?} must be painted exactly once by its mask-local editor"
        );
    }
    // The interactive graph is really painted, not just its caption.
    let graph = curve_graph_rect(&harness);
    assert!(
        graph.min.x > 0.0 && graph.width() > 0.0 && graph.height() > 0.0,
        "the curve graph must occupy a positive rect, got {graph:?}"
    );
    assert!(
        graph.max.y < PANEL_VIEWPORT[1] + 1.0,
        "the curve graph must lie inside the viewport, got {graph:?}"
    );

    // **Set + drag** on the drawn local curve: a click on the drawn identity
    // diagonal inserts a control point, and the drag moves it off the diagonal
    // so the channel is no longer the pixel-neutral identity.
    let global_before = harness.state().recipe().clone();
    edit_curve_point(&mut harness, graph, 0.5, 0.5, 0.5, 0.75);
    let master = harness.state().selected_mask_local_curve("master").unwrap();
    assert_eq!(
        master.len(),
        3,
        "a click on the drawn local curve must add a point: {master:?}"
    );
    assert!(
        close(f64::from(master[1].input), 0.5) && close(f64::from(master[1].output), 0.75),
        "the inserted point must end up where it was dragged: {master:?}"
    );
    assert!(
        harness.state().has_mask_local_curves().unwrap(),
        "a non-identity local curve must be reported as pixel-changing"
    );
    // The global curve block is untouched — the local editor is not a second
    // writer for `EditRecipe::curves`.
    assert_eq!(harness.state().recipe(), &global_before);
    assert!(harness.state().recipe().curves.is_none());
    assert_eq!(
        harness
            .state()
            .selected_mask_local_curve("red")
            .unwrap()
            .len(),
        2,
        "an untouched channel stays the identity"
    );

    // The channel selector is operable: click "Red" under the "Channel"
    // caption (the *other* "Red" in the panel is the HSL band, further down).
    let target = below(&harness, "Channel", "Red");
    click(&mut harness, target);
    // Switching channel is a view change, not a recipe write.
    assert_eq!(harness.state().recipe(), &global_before);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_curve("red")
            .unwrap()
            .len(),
        2
    );
    // …and the graph now edits the red channel: the same set+drag gesture adds
    // a point there and leaves the master curve alone.
    let graph = curve_graph_rect(&harness);
    edit_curve_point(&mut harness, graph, 0.25, 0.25, 0.25, 0.4);
    let red = harness.state().selected_mask_local_curve("red").unwrap();
    assert_eq!(red.len(), 3, "the red channel must take the point: {red:?}");
    assert!(close(f64::from(red[1].input), 0.25) && close(f64::from(red[1].output), 0.4));
    assert_eq!(
        harness.state().selected_mask_local_curve("master").unwrap(),
        master,
        "the master curve must be untouched by a red-channel edit"
    );
    assert_eq!(harness.state().recipe(), &global_before);

    // **Delete** by double-click on the interior point, the third documented
    // gesture. The click chain of the insert+drag above has to expire first,
    // or egui counts the pair as a triple click (see `break_click_chain`).
    break_click_chain(&mut harness);
    let interior = graph_pos_of(&harness, 0.25, 0.4);
    double_click(&mut harness, interior);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_curve("red")
            .unwrap()
            .len(),
        2,
        "a double click on an interior point must remove it"
    );
    assert_eq!(
        harness.state().selected_mask_local_curve("master").unwrap(),
        master
    );

    // **Re-create** the red edit, so the per-channel reset has something to
    // clear and the difference from the block reset stays observable.
    let graph = curve_graph_rect(&harness);
    edit_curve_point(&mut harness, graph, 0.25, 0.25, 0.25, 0.4);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_curve("red")
            .unwrap()
            .len(),
        3
    );

    // The per-channel reset is a real editor transaction, and it targets the
    // *selected* channel: the nearest "Reset" above the "all local curves
    // reset" button is this block's channel reset.
    let target = above(&harness, "all local curves reset", "Reset");
    click(&mut harness, target);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_curve("red")
            .unwrap()
            .len(),
        2,
        "the per-channel reset must clear the selected red channel"
    );
    assert_eq!(
        harness.state().selected_mask_local_curve("master").unwrap(),
        master,
        "the per-channel reset must not clear the master channel"
    );
    assert_eq!(harness.state().recipe(), &global_before);

    // The block reset clears everything the local curve editor wrote. The
    // click chain of the per-channel reset above has to expire first — two
    // clicks on adjacent buttons inside the double-click window are a double
    // click, and egui's button does not report that as a second `clicked()`.
    break_click_chain(&mut harness);
    let target = only_rect(&harness, "all local curves reset");
    click(&mut harness, target);
    assert!(!harness.state().has_mask_local_curves().unwrap());
    assert_eq!(harness.state().recipe(), &global_before);

    // …and the reset reaches the sidecar too, so a reopened project does not
    // resurrect the curve.
    settle_persisted(&mut harness);
    assert!(
        persisted_local_recipe(&dir).curves.is_none(),
        "the block reset must also reach the sidecar"
    );

    // With the global Tone Curve section opened as well, the caption appears
    // twice — the local and the global graph are two separate widgets.
    for index in 0..lumina_gui::SECTION_COUNT {
        let open = matches!(index, SECTION_MASKING | lumina_gui::SECTION_TONE_CURVE);
        harness.state_mut().set_section_open(index, open);
    }
    settle_persisted(&mut harness);
    assert_eq!(
        nodes(&harness, "Point curve").len(),
        2,
        "the local and the global curve graph must be independent widgets"
    );
}
