//! HSL-BAND-KLICK-43: the two **unclicked** rails of the mask-local HSL
//! editor — `Hue` (loop index 0) and `Luminance` (loop index 2) — each get a
//! real click test, in the style of `mask_local_color_controls.rs`.
//!
//! # Why this was a gap and not a documentation nicety
//!
//! `draw_mask_local_color` (`src/mask_local_color.rs`) paints the three HSL
//! shifts from **two** independent sources: the *label* comes from the loop
//! index (`match index { 0 => Hue, 1 => Saturation, _ => Luminance }`), the
//! *field name* from `HSL_FIELDS[index]`. Only index 1 was ever clicked, so
//! only index 1 was pinned. Measured, not assumed:
//!
//! | mutation | before this file | after this file |
//! |---|---|---|
//! | `HSL_FIELDS` 0↔1 | **rot** (`mask_local_color_controls.rs:225`) | **rot** (`:165`, `:255`) |
//! | `HSL_FIELDS` 0↔2 | **grün, 931/931** | **rot** (`:195`, `:255`) |
//! | label-`match` 0↔2 (Felder unberührt) | — | **rot** (`:129`) |
//!
//! A transposition of `Hue` against `Luminance` was therefore invisible while
//! the panel would have mislabelled both rails, and the third row is the mirror
//! defect: correct fields, swapped captions. `ai-masks.md` §6.2 named the first
//! as rows 6 and 8; this file is the anchor that closes them.
//!
//! Under 0↔2 the `Luminance` test trips on its *witness* pin (`:255`) before it
//! reaches its own claim, because the witness is loaded through the `Hue` rail.
//! That its own field pin is independently sensitive was measured separately,
//! with the witness temporarily loaded through the mutation-neutral
//! `Saturation` rail: the drag then fails at "dragging the HSL luminance rail
//! left must move the stored luminance, got 0". So the detection does not rest
//! on the witness alone.
//!
//! # What each test pins, and why it can fail
//!
//! The claim of a rail test is **not** "the value changed" — that is the
//! already-covered mechanic. It is *"this label drives **this** field of
//! **this** band"*, which is a three-way mapping. Each test therefore asserts:
//!
//! 1. the field pin: the dragged rail moves the expected member of the
//!    selected band's triple and leaves **both** siblings at exactly `0.0`;
//! 2. the label pin: the three rails are the canonical
//!    `Hue` → `Saturation` → `Luminance` stack directly under the band row,
//!    and the HSL block ends in exactly one `Reset` below the trio — the
//!    per-band one, identified from both ends of the block (asserted once, in
//!    the `Hue` test; asserting it in both would be a duplicate with no added
//!    claim);
//! 3. the scope pin, through a **second, non-neutral witness band**: a sibling
//!    band is made non-neutral *in a different field* first, and its whole
//!    triple is re-read from disk afterwards and must be unchanged.
//!
//! Point 3 is the anti-gaming half. A write that ignored the selected band, or
//! that reached for the whole block, would still satisfy 1. and 2. With a
//! *neutral* witness band it would also satisfy 3., because "unchanged" and
//! "still zero" are the same observable — so the witness is deliberately
//! pre-loaded with a value that a block-wide write would destroy. The two tests
//! use opposite witness fields (Saturation witnesses `Hue`, `Hue` witnesses
//! `Luminance`), so neither test can pass on a defect that swaps hue and
//! luminance *and* the witness along with it.
//!
//! # F-4: settle, then read the file, then the next gesture
//!
//! The debounced save is last-write-wins and its clock restarts on every edit,
//! so a disk assertion placed at the end of a test would be satisfied by
//! whichever gesture flushed last. Every gesture below is therefore followed by
//! `settle_persisted` **and** its own disk read before the next one. The
//! rationale and the measured trace are in `mask_local_color_controls.rs`.
//!
//! # Drag directions
//!
//! `Hue` is dragged to `0.8` of the rail and `Luminance` to `0.2`, i.e. to
//! opposite signs on the shared `-1..=1` range. Beyond pinning the value, the
//! sign makes a mix-up between the two rails visible as a *sign flip* rather
//! than as a plausible magnitude.
//!
//! No drag helper was touched: `drag_slider` keeps its zero-intermediate-position
//! form, because a helper that walks extra positions to dodge a bug makes the
//! coverage vacuous (see its doc comment and `SIDECAR-SAVE-STRAND-39`).

mod mask_local_editors_support;
mod mask_local_label_support;
mod mask_local_slider_support;
use mask_local_editors_support::*;
use mask_local_label_support::*;
use mask_local_slider_support::*;

/// The persisted `(hue, saturation, luminance)` triple of one band, or `None`
/// when the band is not on disk at all.
///
/// Read through the *whole* triple, never through one field: a witness band
/// only survives if **all three** of its members are unchanged.
fn band_on_disk(dir: &tempfile::TempDir, band: &str) -> Option<(f64, f64, f64)> {
    let block = persisted_local_recipe(dir).hsl?;
    let channel = match band {
        "cyan" => block.cyan,
        "yellow" => block.yellow,
        _ => None,
    }?;
    Some((
        f64::from(channel.hue),
        f64::from(channel.saturation),
        f64::from(channel.luminance),
    ))
}

/// Field-wise comparison of two band triples at the shared slider tolerance.
fn same_band(left: (f64, f64, f64), right: (f64, f64, f64)) -> bool {
    close(left.0, right.0) && close(left.1, right.1) && close(left.2, right.2)
}

/// The HSL `Hue` rail writes the `hue` field of the selected band and nothing
/// else: not its two sibling fields, not another band, not the global recipe.
#[test]
fn the_hsl_hue_rail_writes_only_the_hue_field_of_the_selected_band() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let global_before = harness.state().recipe().clone();
    assert!(
        !harness.state().has_mask_local_color().unwrap(),
        "the layer must start without a local colour block"
    );

    // 1) Address the HSL trio and pin the **label -> row** mapping. The field
    //    side of the same loop is pinned by the drags below; without this half
    //    a mutation that only permuted the label `match` would still pass.
    let cyan = only_rect(&harness, "cyan");
    click(&mut harness, cyan);
    let hue_rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Hue");
    let saturation_rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Saturation");
    let luminance_rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Luminance");
    assert!(
        hue_rail.min.y > cyan.max.y
            && hue_rail.min.y < saturation_rail.min.y
            && saturation_rail.min.y < luminance_rail.min.y,
        "the HSL rails must be the canonical hue -> saturation -> luminance stack \
         under the band row, got {hue_rail:?} / {saturation_rail:?} / {luminance_rail:?}"
    );
    // The per-band `Reset` is the only `Reset` inside the HSL block. It is
    // identified from **both** ends of that block — the first `Reset` below the
    // block caption, the nearest `Reset` above the Point-Colour `Add color`
    // button — and the two lookups must resolve to the same widget. That is the
    // block-membership proof for the trio: a rail that had left the HSL block
    // (or a second `Reset` painted inside it) moves one of the two anchors.
    let reset_from_caption = below(&harness, "HSL / Color Mixer", "Reset");
    let reset_from_point_color = above(&harness, "Add color", "Reset");
    assert!(
        reset_from_caption == reset_from_point_color,
        "the HSL block must end in exactly one Reset, got {reset_from_caption:?} (below the \
         caption) vs {reset_from_point_color:?} (above Add color)"
    );
    assert!(
        luminance_rail.min.y < reset_from_caption.min.y,
        "all three HSL rails must sit above the per-band reset, got {luminance_rail:?} \
         vs {reset_from_caption:?}"
    );

    // 2) The **witness band**, loaded through a *different* field
    //    (Saturation) so that a Hue drag has something of its own to clobber.
    let yellow = only_rect(&harness, "yellow");
    click(&mut harness, yellow);
    let rail = slider_rail(&harness, ("yellow", 0), Row::Below, "Saturation");
    drag_slider(&mut harness, rail, 0.8);
    let witness = harness
        .state()
        .selected_mask_local_hsl_band("yellow")
        .unwrap();
    assert!(
        witness.1 > 0.4,
        "the yellow witness band must be non-neutral before the Hue drag, got {witness:?}"
    );
    assert!(
        same_band(witness, (0.0, witness.1, 0.0)),
        "the witness drag must land in saturation only, got {witness:?}"
    );
    // F-4: settle, read the file, and only then start the next gesture.
    settle_persisted(&mut harness);
    let witness_on_disk = band_on_disk(&dir, "yellow")
        .expect("the yellow witness must reach the sidecar before the cyan drag");
    assert!(
        same_band(witness_on_disk, (0.0, witness.1, 0.0)),
        "the witness must be persisted through saturation only, got {witness_on_disk:?}"
    );
    assert!(
        band_on_disk(&dir, "cyan").is_none(),
        "the band that is edited next must not be on disk yet"
    );

    // 3) The claim. The rail is re-looked-up after the band switch, so the
    //    gesture is addressed the same way a user reaches it.
    click(&mut harness, cyan);
    let hue_rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Hue");
    drag_slider(&mut harness, hue_rail, 0.8);
    let (hue, saturation, luminance) = harness
        .state()
        .selected_mask_local_hsl_band("cyan")
        .unwrap();
    assert!(
        hue > 0.4,
        "dragging the HSL hue rail must move the stored hue, got {hue}"
    );
    assert!(
        close(saturation, 0.0) && close(luminance, 0.0),
        "the HSL hue rail must write **only** the hue field, got saturation {saturation} / \
         luminance {luminance}"
    );
    assert!(harness.state().has_mask_local_color().unwrap());
    // The global recipe is not a second writer for this block.
    assert_eq!(harness.state().recipe(), &global_before);
    assert!(harness.state().recipe().hsl.is_none());
    assert!(!harness.state().recipe().adjustments.contains_key("hue"));

    // 4) The disk read, again before any later gesture.
    settle_persisted(&mut harness);
    let cyan_on_disk =
        band_on_disk(&dir, "cyan").expect("the cyan hue drag must reach the sidecar on its own");
    assert!(
        same_band(cyan_on_disk, (hue, 0.0, 0.0)),
        "the sidecar must carry the hue in the hue field and nothing else, got {cyan_on_disk:?} \
         (in-memory hue {hue})"
    );
    assert!(
        same_band(
            band_on_disk(&dir, "yellow").expect("the witness band must stay on disk"),
            witness_on_disk
        ),
        "the HSL hue rail is band-scoped: the non-neutral sibling band must survive unchanged, \
         expected {witness_on_disk:?}"
    );
}

/// The HSL `Luminance` rail writes the `luminance` field of the selected band
/// and nothing else — the mirror image of the `Hue` test, which is the only
/// reason a `Hue`↔`Luminance` transposition of `HSL_FIELDS` cannot pass.
#[test]
fn the_hsl_luminance_rail_writes_only_the_luminance_field_of_the_selected_band() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let global_before = harness.state().recipe().clone();

    // 1) The witness band, loaded through its **hue** rail. `Hue` witnesses
    //    `Luminance` here, the other way round from the sibling test, so a
    //    transposition of the two fields moves the witness instead of hiding
    //    behind it.
    //
    //    A hue-*only* HSL channel is safe as a witness, unlike a hue-only
    //    *grading* range: `hsl_is_neutral` requires all three shifts to be zero,
    //    so the band persists (the sibling test's per-range witness has to use
    //    Saturation for the opposite reason — see `mask_local_color_controls.rs`).
    let yellow = only_rect(&harness, "yellow");
    click(&mut harness, yellow);
    let rail = slider_rail(&harness, ("yellow", 0), Row::Below, "Hue");
    drag_slider(&mut harness, rail, 0.8);
    let witness = harness
        .state()
        .selected_mask_local_hsl_band("yellow")
        .unwrap();
    assert!(
        witness.0 > 0.4,
        "the yellow witness band must be non-neutral before the luminance drag, got {witness:?}"
    );
    assert!(
        same_band(witness, (witness.0, 0.0, 0.0)),
        "the witness drag must land in hue only, got {witness:?}"
    );
    settle_persisted(&mut harness);
    let witness_on_disk = band_on_disk(&dir, "yellow")
        .expect("the yellow witness must reach the sidecar before the cyan drag");
    assert!(
        same_band(witness_on_disk, (witness.0, 0.0, 0.0)),
        "the witness must be persisted through hue only, got {witness_on_disk:?}"
    );
    assert!(
        band_on_disk(&dir, "cyan").is_none(),
        "the band that is edited next must not be on disk yet"
    );

    // 2) The claim, dragged **left** so the stored value is negative. "Luminance"
    //    is also the grading and the noise-reduction slider, so a wrong-rail
    //    drag is possible in principle. It is deliberately **not** re-asserted
    //    here as a separate structural claim — the sibling test already pins the
    //    trio's position inside the HSL block, and repeating it would be the
    //    duplicate the test-coverage policy forbids. A wrong rail surfaces
    //    through the field pin below instead, where it belongs.
    let cyan = only_rect(&harness, "cyan");
    click(&mut harness, cyan);
    let luminance_rail = slider_rail(&harness, ("cyan", 0), Row::Below, "Luminance");
    drag_slider(&mut harness, luminance_rail, 0.2);
    let (hue, saturation, luminance) = harness
        .state()
        .selected_mask_local_hsl_band("cyan")
        .unwrap();
    assert!(
        luminance < -0.4,
        "dragging the HSL luminance rail left must move the stored luminance, got {luminance}"
    );
    assert!(
        close(hue, 0.0) && close(saturation, 0.0),
        "the HSL luminance rail must write **only** the luminance field, got hue {hue} / \
         saturation {saturation}"
    );
    assert!(harness.state().has_mask_local_color().unwrap());
    assert_eq!(harness.state().recipe(), &global_before);
    assert!(harness.state().recipe().hsl.is_none());
    assert!(!harness
        .state()
        .recipe()
        .adjustments
        .contains_key("luminance"));
    assert!(!harness.state().recipe().adjustments.contains_key("hue"));

    // 3) The disk read, again before any later gesture.
    settle_persisted(&mut harness);
    let cyan_on_disk = band_on_disk(&dir, "cyan")
        .expect("the cyan luminance drag must reach the sidecar on its own");
    assert!(
        same_band(cyan_on_disk, (0.0, 0.0, luminance)),
        "the sidecar must carry the luminance in the luminance field and nothing else, got \
         {cyan_on_disk:?} (in-memory luminance {luminance})"
    );
    assert!(
        same_band(
            band_on_disk(&dir, "yellow").expect("the witness band must stay on disk"),
            witness_on_disk
        ),
        "the HSL luminance rail is band-scoped: the non-neutral sibling band must survive \
         unchanged, expected {witness_on_disk:?}"
    );
}
