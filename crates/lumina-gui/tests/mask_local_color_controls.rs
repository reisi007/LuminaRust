//! GUI-INT-MASKLOCAL-38: the **remaining** clickable controls of the
//! mask-local colour editor (MASK-LOCAL-P1.2b), plus the two *scoped* resets.
//!
//! # Why a separate target
//!
//! DoD §3 forbids a sample: "one slider green ⇒ all good" is no proof. The
//! first interaction target (`mask_local_editors.rs`) drove one representative
//! per block, which left five controls of `draw_mask_local_color` without any
//! click at all. This target closes exactly those five, and nothing else:
//!
//! 1. the **Vibrance** slider,
//! 2. the **Saturation** slider of the vibrance/saturation pair,
//! 3. the Point-Colour **`Remove`** button,
//! 4. the **per-band HSL reset** (band-scoped, *not* the block reset),
//! 5. the **per-range grading reset** (range-scoped, *not* the block reset).
//!
//! The remaining controls of that draw function and their coverage class are
//! enumerated in `feature/product/ai-masks.md` §6 — this file is the anchor for
//! the "click-covered" column, not a claim that nothing is left.
//!
//! # Why every gesture is followed by a settle **and** a disk assertion
//!
//! This is the F-4 discipline, and it is not cosmetic. Measured with a
//! `log::Log` tap at trace level (DoD §2 names the path, the test drives it):
//! a mask-local drag upgrades its draft tick to a **full** render
//! (`render_tick.rs`: "absolute-frame or local-mask stage active"), and the
//! debounced save only runs in the `!pointer_down && pending_full_render`
//! branch of `schedule_render`. The debounce clock is **restarted by every
//! edit**, and `pending_slider_commit` is last-write-wins, so a second gesture
//! inside the 150 ms window pushes the first one's commit out. A disk
//! assertion placed at the *end* of the test would therefore be satisfied by
//! whichever gesture flushed last — which is precisely the false pass F-4
//! forbids. The first version of this test had exactly that defect (no settle
//! between the cyan and the yellow drag) and the trace showed the cyan commit
//! never getting its debounce time. Hence: settle, then read the file, then
//! the next gesture.
//!
//! No value is ever lost this way — the save carries the whole current layer
//! state — but "the file contains my edit *now*" is only true once the
//! debounce has run, and that is what each assertion checks.
//!
//! # The one deliberate exception
//!
//! The rule above is stated for **value gestures**: drags and field clicks that
//! a later gesture could overwrite before its debounce expires. The two
//! consecutive `Add color` clicks below are *not* value gestures — each appends
//! a distinct entry to the list, neither can clobber the other, and the
//! assertion that follows needs both to exist. The file is read once, after
//! that pair, and `entries.len() == 2` on disk is then unambiguous. No value
//! assertion in this file relies on a gesture that skipped its own settle.
//!
//! # What makes the two resets worth their own test
//!
//! A reset that clears too much is the classic silent defect, so each reset is
//! driven while the **other** blocks — and a second band/range — are
//! deliberately non-neutral, and are then asserted to have survived. A reset
//! that reached for the whole block fails both. (With only one non-neutral
//! band/range this test is *vacuous*: a mutation that made the per-band reset
//! clear the entire HSL block passed the first version of it, measured.)

mod mask_local_editors_support;
mod mask_local_label_support;
mod mask_local_slider_support;
use mask_local_editors_support::*;
use mask_local_label_support::*;
use mask_local_slider_support::*;

/// The five remaining colour controls are clickable, each writes through to the
/// mask layer and the sidecar, and the two scoped resets clear only their own
/// scope.
#[test]
fn the_remaining_mask_local_color_controls_are_clickable_and_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let global_before = harness.state().recipe().clone();
    assert_eq!(
        harness.state().selected_mask_local_vibrance().unwrap(),
        (0.0, 0.0)
    );

    // 1) Vibrance. Anchored on the block caption: "Vibrance" is the first
    //    slider row below "HSL / Color Mixer", and the HSL band's own
    //    "Saturation" sits *above* it, so the two are never confused.
    let vibrance_rail = slider_rail(&harness, ("HSL / Color Mixer", 0), Row::Below, "Vibrance");
    drag_slider(&mut harness, vibrance_rail, 0.8);
    let (vibrance, saturation) = harness.state().selected_mask_local_vibrance().unwrap();
    assert!(
        vibrance > 0.5,
        "dragging the vibrance rail must change the stored value, got {vibrance}"
    );
    assert!(
        close(saturation, 0.0),
        "the sibling slider must stay neutral"
    );
    assert!(harness.state().has_mask_local_color().unwrap());
    assert_eq!(harness.state().recipe(), &global_before);
    // The global colour block is not a second writer for the flat map.
    assert!(harness.state().recipe().hsl.is_none());
    assert!(!harness
        .state()
        .recipe()
        .adjustments
        .contains_key("vibrance"));
    settle_persisted(&mut harness);
    assert!(
        close(persisted_local_recipe(&dir).vibrance, vibrance),
        "the vibrance drag must reach the sidecar on its own, before any later click"
    );

    // 2) Saturation of the vibrance pair, dragged *left* so it lands negative
    //    and cannot be confused with the HSL band's saturation (which
    //    `mask_local_editors.rs` already drives positively).
    let saturation_rail = slider_rail(&harness, ("Vibrance", 0), Row::Below, "Saturation");
    assert!(
        saturation_rail.min.y > vibrance_rail.min.y,
        "the vibrance-pair saturation must be the row below vibrance, got \
         {saturation_rail:?} vs {vibrance_rail:?}"
    );
    drag_slider(&mut harness, saturation_rail, 0.2);
    let (vibrance, saturation) = harness.state().selected_mask_local_vibrance().unwrap();
    assert!(
        saturation < -0.5,
        "dragging the vibrance-pair saturation rail left must change it, got {saturation}"
    );
    assert!(vibrance > 0.5, "the sibling slider must be untouched");
    assert!(!harness
        .state()
        .recipe()
        .adjustments
        .contains_key("saturation"));
    assert_eq!(harness.state().recipe(), &global_before);
    settle_persisted(&mut harness);
    let persisted = persisted_local_recipe(&dir);
    assert!(close(persisted.saturation, saturation));
    assert!(close(persisted.vibrance, vibrance));

    // 3) Point Color: add **two** entries, then remove exactly one of them.
    //    Two, not one: with a single entry "remove this entry" and "remove the
    //    whole block" are the same observable (measured — a mutation that made
    //    the button clear the block passed the one-entry version). The *second*
    //    button is the one clicked, so a hard-coded "always remove the first"
    //    defect cannot hide either. `Remove` only exists while the list is
    //    non-empty, so it cannot be clicked before the adds.
    let target = only_rect(&harness, "Add color");
    click(&mut harness, target);
    let target = only_rect(&harness, "Add color");
    click(&mut harness, target);
    let entries = harness.state().selected_mask_local_point_color().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "Add color twice must create two entries: {entries:?}"
    );
    let (first, second) = (entries[0].id.clone(), entries[1].id.clone());
    assert_ne!(
        first, second,
        "the two entries must have distinct stable ids"
    );
    assert!(harness.state().recipe().point_color.is_none());
    settle_persisted(&mut harness);
    assert_eq!(
        persisted_local_recipe(&dir)
            .point_color
            .as_ref()
            .map(|block| block.entries.len()),
        Some(2),
        "both entries must reach the sidecar before one can be removed"
    );
    let target = occurrence(&harness, "Remove", 1);
    click(&mut harness, target);
    let entries = harness.state().selected_mask_local_point_color().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "Remove must delete exactly one entry: {entries:?}"
    );
    assert_eq!(
        entries[0].id, first,
        "the second button must remove the second entry, not the first"
    );
    settle_persisted(&mut harness);
    let persisted_ids: Vec<String> = persisted_local_recipe(&dir)
        .point_color
        .as_ref()
        .expect("one entry must remain on disk")
        .entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    assert_eq!(
        persisted_ids,
        vec![first],
        "the removal must reach the sidecar on its own, before any later click"
    );
    // Clear the rest, so the later reset lookup is not confused by the
    // point-colour block reset — it is only painted for a non-empty list.
    let target = occurrence(&harness, "Remove", 0);
    click(&mut harness, target);
    assert!(harness
        .state()
        .selected_mask_local_point_color()
        .unwrap()
        .is_empty());
    settle_persisted(&mut harness);
    assert!(
        persisted_local_recipe(&dir).point_color.is_none(),
        "removing the last entry must drop the block on disk too"
    );
    assert_eq!(harness.state().recipe(), &global_before);

    // 4) The **per-band** HSL reset, with the vibrance pair still non-neutral
    //    as its witness. Two bands are made non-neutral on purpose: with only
    //    one, a reset that wrongly cleared the *whole* HSL block would be
    //    indistinguishable. "yellow" is the second band because it is
    //    panel-unique, unlike "red"/"green"/"blue", which are also curve
    //    channels.
    let cyan = only_rect(&harness, "cyan");
    click(&mut harness, cyan);
    let rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Saturation");
    drag_slider(&mut harness, rail, 0.8);
    let (_, cyan_saturation, _) = harness
        .state()
        .selected_mask_local_hsl_band("cyan")
        .unwrap();
    assert!(
        cyan_saturation > 0.5,
        "the cyan band must be non-neutral, got {cyan_saturation}"
    );
    settle_persisted(&mut harness);
    // Read through `and_then`: once *every* band is neutral the whole `hsl`
    // block is legitimately dropped, so requiring the block to exist would
    // encode a serialisation detail instead of the claim.
    let band_on_disk = |band: &str| {
        persisted_local_recipe(&dir)
            .hsl
            .as_ref()
            .and_then(|block| match band {
                "cyan" => block.cyan,
                "yellow" => block.yellow,
                _ => None,
            })
            .map(|channel| f64::from(channel.saturation))
    };
    assert!(
        band_on_disk("cyan").is_some(),
        "the cyan drag must reach the sidecar on its own, before the yellow drag"
    );

    let yellow = only_rect(&harness, "yellow");
    click(&mut harness, yellow);
    let rail = slider_rail(&harness, ("yellow", 0), Row::Below, "Saturation");
    drag_slider(&mut harness, rail, 0.8);
    let (_, yellow_saturation, _) = harness
        .state()
        .selected_mask_local_hsl_band("yellow")
        .unwrap();
    assert!(
        yellow_saturation > 0.5,
        "the yellow witness band must be non-neutral, got {yellow_saturation}"
    );
    settle_persisted(&mut harness);
    assert!(
        band_on_disk("cyan").is_some() && band_on_disk("yellow").is_some(),
        "both bands must be on disk: {:?} / {:?}",
        band_on_disk("cyan"),
        band_on_disk("yellow")
    );
    // Re-select cyan: the per-band reset acts on the *selected* band.
    click(&mut harness, cyan);
    settle_persisted(&mut harness);

    // The first "Reset" below the block caption is the per-band one (only the
    // band row and the three HSL sliders sit between them).
    let target = below(&harness, "HSL / Color Mixer", "Reset");
    click(&mut harness, target);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_hsl_band("cyan")
            .unwrap(),
        (0.0, 0.0, 0.0),
        "the per-band reset must clear the selected band"
    );
    assert!(
        harness
            .state()
            .selected_mask_local_hsl_band("yellow")
            .unwrap()
            .1
            > 0.5,
        "the per-band reset must NOT clear the other bands: it is band-scoped"
    );
    let (vibrance, saturation) = harness.state().selected_mask_local_vibrance().unwrap();
    assert!(
        vibrance > 0.5 && saturation < -0.5,
        "the per-band reset must NOT clear the vibrance/saturation pair, got \
         {vibrance}/{saturation}"
    );
    assert_eq!(harness.state().recipe(), &global_before);
    settle_persisted(&mut harness);
    let persisted = persisted_local_recipe(&dir);
    assert!(
        close(persisted.vibrance, vibrance) && close(persisted.saturation, saturation),
        "the per-band reset must not disturb the persisted vibrance pair"
    );
    assert!(
        band_on_disk("cyan").is_none() && band_on_disk("yellow").is_some(),
        "only the selected band may disappear on disk, got {:?} / {:?}",
        band_on_disk("cyan"),
        band_on_disk("yellow")
    );

    // 5) The **per-range** grading reset, with the vibrance pair and the yellow
    //    HSL band non-neutral as its witnesses — plus a second, non-selected
    //    grading range, for the same reason as above.
    //
    //    The witness range is made non-neutral through **Saturation**, not
    //    through Hue, and that is a product fact, not a convenience:
    //    `color_grading_is_neutral` is the *pixel* predicate and deliberately
    //    ignores `hue_degrees` (a hue only selects a tint; the kernel skips
    //    `saturation == 0`). A hue-only block is therefore pixel-neutral and is
    //    dropped instead of persisted — shared with the global editor, not a
    //    mask-local quirk. With a hue-only witness the block would vanish on
    //    the reset and "the other range survived" would be unobservable.
    let midtones = only_rect(&harness, "Midtones");
    click(&mut harness, midtones);
    let rail = slider_rail(&harness, ("Color Grading", 0), Row::Below, "Hue");
    drag_slider(&mut harness, rail, 0.7);
    let (midtones_hue, _, _) = harness
        .state()
        .selected_mask_local_grading_range("midtones")
        .unwrap();
    assert!(
        midtones_hue > 200.0,
        "the midtones range must carry the dragged hue, got {midtones_hue}"
    );
    settle_persisted(&mut harness);
    let grading = |range: &str| {
        persisted_local_recipe(&dir)
            .color_grading
            .as_ref()
            .map(|block| match range {
                "midtones" => block.midtones,
                "highlights" => block.highlights,
                _ => unreachable!("unknown grading range {range}"),
            })
    };
    assert!(
        grading("midtones").is_some_and(|range| range.hue_degrees > 200.0),
        "the midtones drag must reach the sidecar on its own, before the \
         highlights drag"
    );

    // "Highlights" is also a global local-adjustment slider, so the mask-local
    // range is addressed through the grading caption.
    let highlights = below(&harness, "Color Grading", "Highlights");
    click(&mut harness, highlights);
    let rail = slider_rail(&harness, ("Color Grading", 0), Row::Below, "Saturation");
    drag_slider(&mut harness, rail, 0.8);
    let (_, witness_saturation, _) = harness
        .state()
        .selected_mask_local_grading_range("highlights")
        .unwrap();
    assert!(
        witness_saturation > 0.5,
        "the highlights witness range must be pixel-relevant, got {witness_saturation}"
    );
    settle_persisted(&mut harness);
    assert!(
        grading("midtones").is_some_and(|range| range.hue_degrees > 200.0)
            && grading("highlights").is_some_and(|range| range.saturation > 0.5),
        "both grading ranges must be on disk: {:?} / {:?}",
        grading("midtones"),
        grading("highlights")
    );
    // Re-select midtones: the per-range reset acts on the *selected* range.
    click(&mut harness, midtones);
    settle_persisted(&mut harness);

    // The nearest "Reset" above the block-wide reset is the per-range one: the
    // point-colour block reset is not painted at all while the list is empty,
    // and the per-band reset is further up.
    let target = above(&harness, "all local color reset", "Reset");
    click(&mut harness, target);
    assert_eq!(
        harness
            .state()
            .selected_mask_local_grading_range("midtones")
            .unwrap(),
        (0.0, 0.0, 0.0),
        "the per-range reset must clear the selected grading range"
    );
    assert!(
        harness
            .state()
            .selected_mask_local_grading_range("highlights")
            .unwrap()
            .1
            > 0.5,
        "the per-range reset must NOT clear the other ranges: it is range-scoped"
    );
    let (vibrance, saturation) = harness.state().selected_mask_local_vibrance().unwrap();
    assert!(
        vibrance > 0.5 && saturation < -0.5,
        "the per-range reset must NOT clear the vibrance pair, got \
         {vibrance}/{saturation}"
    );
    assert!(
        harness
            .state()
            .selected_mask_local_hsl_band("yellow")
            .unwrap()
            .1
            > 0.5,
        "the per-range reset must NOT clear the HSL block"
    );
    assert_eq!(harness.state().recipe(), &global_before);
    settle_persisted(&mut harness);
    let midtones_on_disk = grading("midtones")
        .expect("the block must survive: the highlights range is still pixel-relevant");
    assert!(
        close(f64::from(midtones_on_disk.hue_degrees), 0.0),
        "the cleared range must be neutral on disk, got {midtones_on_disk:?}"
    );
    assert!(
        close(
            f64::from(grading("highlights").expect("witness range").saturation),
            witness_saturation
        ),
        "the witness range must be unchanged on disk"
    );
    assert!(close(persisted_local_recipe(&dir).vibrance, vibrance));
}
