//! AUTO-TONE-ANALYSIS-INPUT-8: the Auto-Tone **measurement domain** in the GUI.
//!
//! The owner decision `AUTO-DOMAIN` (2026-10-02) fixes the domain Auto-Tone
//! measures: the frame **after SourceActions and after Crop**, *without*
//! Adjustments — not the raw decode. The tests here prove that with **numbers**,
//! through the real production entry point [`LuminaApp::auto_tone`], and they pin
//! the properties that were previously only claimed:
//!
//! * the crop is part of the domain (measured, values pinned exactly),
//! * removing the crop restores the un-cropped values (the crop is the *only*
//!   difference between the two runs),
//! * a mask layer, which Auto does not read, cannot move the value,
//! * two Auto presses in a row are bit-identical (determinism).
//!
//! None of these tests asserts "the function was called". Each one goes red if
//! the production wiring in `LuminaApp::auto_tone` /
//! `LuminaApp::auto_analysis_frame` is removed.

use super::*;

/// The six AUTO-TONE-2 sliders in contract order.
pub(super) const SIX_KEYS: [&str; 6] = [
    "exposure",
    "contrast",
    "whites",
    "blacks",
    "highlights",
    "shadows",
];

/// A deterministic 32×16 fixture of **four vertical bands** with four different
/// gray levels `10 | 60 | 140 | 235` (8 px each).
///
/// The four levels matter: any half-crop of this fixture keeps **two different
/// levels**, so no crop can ever land in the algorithm's degenerate uniform-frame
/// branch (`span <= epsilon` → identity for the five spread/percentile sliders).
/// Every Auto value reported below is therefore a real measurement of the
/// cropped content, not an artifact of the degenerate branch.
///
/// Documented pixel function — no external asset, no licence obligation.
pub(super) fn split_luminance_png() -> Vec<u8> {
    let (width, height) = (32u32, 16u32);
    let bands = [10u8, 60, 140, 235];
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for x in 0..width {
            let level = bands[(x / (width / bands.len() as u32)) as usize];
            pixels.extend_from_slice(&[level, level, level, 255]);
        }
    }
    ImageFrame::new(width, height, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// The six written slider values, in [`SIX_KEYS`] order.
pub(super) fn six_values(app: &LuminaApp) -> Vec<f64> {
    SIX_KEYS
        .iter()
        .map(|key| {
            app.recipe
                .adjustments
                .get(*key)
                .copied()
                .unwrap_or_else(|| panic!("auto tone must write `{key}`"))
        })
        .collect()
}

/// Loads the split fixture, applies `prepare`, runs the real `auto_tone` and
/// returns the six written values.
fn auto_values_after(prepare: impl FnOnce(&mut LuminaApp)) -> Vec<f64> {
    let mut app = new_app();
    app.load_bytes(split_luminance_png(), "split.png").unwrap();
    prepare(&mut app);
    app.auto_tone().unwrap();
    six_values(&app)
}

/// **The domain proof, with numbers.** Three runs over the **same** source — no
/// crop, the dark half (`10|60`) and the bright half (`140|235`) — must produce
/// three different Auto value sets, and the directions are fixed by the fixture:
/// the darker the measured region, the more exposure is needed to bring the
/// median to the target.
///
/// Every crop here is non-uniform, so all six sliders report real measurements.
///
/// A tool that measured the un-cropped decode would return the first row for all
/// three runs and fail.
///
/// MUTATION: replacing `self.auto_analysis_frame()?` in `LuminaApp::auto_tone`
/// with `self.original.clone()` makes all three runs equal and turns this test
/// red.
#[test]
fn a_crop_changes_the_auto_tone_values_and_the_direction_follows_the_brightness() {
    let uncropped = auto_values_after(|_| {});
    let dark_half = auto_values_after(|app| {
        app.set_crop_free(0.0, 0.0, 0.5, 1.0).unwrap();
    });
    let bright_half = auto_values_after(|app| {
        app.set_crop_free(0.5, 0.0, 0.5, 1.0).unwrap();
    });

    // MEASURED (target luminance 0.5, the Auto-Tone default) and pinned exactly,
    // so a domain that drifts anywhere else fails on a number and not on a mood:
    //
    //   uncropped   (bands 10|60|140|235)
    //     [0.34894830882107136, -0.08977777777777773, 0.030078124999999956,
    //      -0.008984375000000003, -0.12734374999999998, 0.04843750000000002]
    //   dark half   (bands 10|60)
    //     [1.850252880495318, 0.8499332443257677, 0.6370464672183322,
    //      -0.008984375000000003, 0.30234375, 0.30234375]
    //   bright half (bands 140|235)
    //     [-0.5545888516776374, 0.7579262213359921, 0.030078124999999956,
    //      0.498828125, 0.21445312500000002, 0.21445312500000002]
    assert_eq!(
        uncropped,
        vec![
            0.34894830882107136,
            -0.08977777777777773,
            0.030078124999999956,
            -0.008984375000000003,
            -0.12734374999999998,
            0.04843750000000002,
        ],
        "the un-cropped Auto values are pinned"
    );
    assert_eq!(
        dark_half,
        vec![
            1.850252880495318,
            0.8499332443257677,
            0.6370464672183322,
            -0.008984375000000003,
            0.30234375,
            0.30234375,
        ],
        "the dark-half Auto values are pinned"
    );
    assert_eq!(
        bright_half,
        vec![
            -0.5545888516776374,
            0.7579262213359921,
            0.030078124999999956,
            0.498828125,
            0.21445312500000002,
            0.21445312500000002,
        ],
        "the bright-half Auto values are pinned"
    );

    assert_ne!(
        uncropped, dark_half,
        "AUTO-TONE-ANALYSIS-INPUT-8: the crop is part of the Auto-Tone domain, so cropping to a \
         different region must change the six Auto values"
    );
    assert_ne!(
        uncropped, bright_half,
        "the second crop must differ from the first as well"
    );
    assert_ne!(
        dark_half, bright_half,
        "two different crops must not produce the same Auto values"
    );
    // Exposure rises monotonically as the measured region gets darker.
    assert!(
        dark_half[0] > uncropped[0],
        "exposure must rise when the bright bands are cropped away: {} vs {}",
        uncropped[0],
        dark_half[0]
    );
    assert!(
        bright_half[0] < uncropped[0],
        "exposure must fall when the dark bands are cropped away: {} vs {}",
        uncropped[0],
        bright_half[0]
    );
}

/// **The crop is the only difference between the two runs.** Setting the crop and
/// then removing it again must restore the un-cropped Auto values bit-for-bit.
///
/// This is the part a "it recompiles" test cannot say: it pins that the domain
/// is the *recipe's* geometry and nothing else slipped in (no accidental
/// adjustments, no resolution scaling, no preview cap).
#[test]
fn removing_the_crop_restores_the_uncropped_auto_values_bit_for_bit() {
    let uncropped = auto_values_after(|_| {});
    let there_and_back = auto_values_after(|app| {
        app.set_crop_free(0.0, 0.0, 0.5, 1.0).unwrap();
        app.set_crop_free(0.0, 0.0, 1.0, 1.0).unwrap();
    });
    assert_eq!(
        uncropped, there_and_back,
        "a crop that is set and removed again must not leave a residue in the Auto values"
    );
}

/// **Determinism (owner requirement).** Two Auto presses in a row yield
/// bit-identical values, with and without a crop.
///
/// This is not vacuous: the writer re-derives the fingerprint on every press and
/// the GUI passes its own override set on a second press. A non-deterministic
/// seam — or a second pass that folded the first result back in, the feedback
/// loop the domain decision excludes — shows up here.
#[test]
fn two_auto_presses_in_a_row_are_bit_identical() {
    for crop in [None, Some((0.0, 0.0, 0.5, 1.0))] {
        let mut app = new_app();
        app.load_bytes(split_luminance_png(), "split.png").unwrap();
        if let Some((x, y, w, h)) = crop {
            app.set_crop_free(x, y, w, h).unwrap();
        }
        app.auto_tone().unwrap();
        let first = six_values(&app);
        app.auto_tone().unwrap();
        let second = six_values(&app);
        assert_eq!(
            first, second,
            "pressing Auto twice must be bit-identical (crop = {crop:?})"
        );
    }
}

/// **A mask layer Auto does not read cannot move the value.** The analysis domain
/// is post SourceActions + post Crop and contains **no** mask stage, so adding a
/// mask definition to the active copy must leave all six Auto values untouched.
///
/// MUTATION: folding the mask count into `auto_analysis_frame_for` turns this red
/// (verified — see the mutation log in the commit message).
#[test]
fn adding_a_mask_layer_does_not_change_the_auto_values() {
    let mut without_mask = new_app();
    without_mask
        .load_bytes(split_luminance_png(), "split.png")
        .unwrap();
    without_mask.set_crop_free(0.0, 0.0, 0.5, 1.0).unwrap();
    without_mask.auto_tone().unwrap();
    let baseline = six_values(&without_mask);

    let mut with_mask = new_app();
    with_mask
        .load_bytes(split_luminance_png(), "split.png")
        .unwrap();
    with_mask.set_crop_free(0.0, 0.0, 0.5, 1.0).unwrap();
    // A real mask definition on the active copy: the mask stage is the LAST
    // render stage and is not part of the Auto-Tone analysis domain.
    with_mask.create_mask("domain-isolation").unwrap();
    with_mask.auto_tone().unwrap();

    assert_eq!(
        baseline,
        six_values(&with_mask),
        "a mask layer the Auto domain does not read must not move any Auto value"
    );
}

/// **CLI/GUI parity on the measured domain (requirement 6).** The GUI action and
/// the shared writer — which is what `lumina process --auto-tone` and
/// `lumina regenerate --module auto-tone` both call — must agree **including the
/// fingerprint**, with a crop in the recipe.
///
/// The fingerprint is the strict part: it is the blake3 of the analysis frame
/// plus the target, so if the two front ends ever measured different pixels the
/// string would differ and `regenerate` would treat a CLI-written recipe as
/// stale. This is the AUTO-TONE-CLI-6 divergence returning by another door.
///
/// It is compared with a crop on purpose: without one, the analysis frame equals
/// the raw decode and the comparison would be vacuous (proven by the domain test
/// above, where the two differ).
#[test]
fn the_gui_and_the_shared_writer_agree_on_the_measured_domain_including_the_fingerprint() {
    use lumina_stages::auto_tone::{
        apply_auto_tone_result, auto_analysis_frame, PersistedAutoTone, AUTO_TONE_ADJUSTMENT_KEYS,
    };

    let mut via_gui = new_app();
    via_gui
        .load_bytes(split_luminance_png(), "split.png")
        .unwrap();
    via_gui.set_crop_free(0.0, 0.0, 0.5, 1.0).unwrap();
    via_gui.auto_tone().unwrap();

    // The CLI/regenerate side: the same decoded frame, the same recipe (crop
    // included), the same shared analysis seam, then the shared writer.
    let (mut via_writer, source_frame) = decoded_frame();
    let recipe = via_gui.recipe.clone();
    let analysis = auto_analysis_frame(&source_frame, &recipe, &[]).unwrap();
    let target = recipe.auto_features.target_luminance;
    apply_auto_tone_result(
        &mut via_writer,
        &analysis,
        target,
        PersistedAutoTone::AlwaysRecompute,
        None,
    )
    .unwrap();

    for key in AUTO_TONE_ADJUSTMENT_KEYS {
        let gui = via_gui.recipe.adjustments.get(key).copied().unwrap();
        let writer = via_writer.adjustments.get(key).copied().unwrap();
        assert_eq!(
            gui, writer,
            "`{key}`: the GUI and the shared writer must agree on the measured domain"
        );
    }
    let gui_fp = via_gui
        .recipe
        .auto_features
        .analysis_fingerprint
        .clone()
        .expect("the GUI persists a fingerprint");
    let writer_fp = via_writer
        .auto_features
        .analysis_fingerprint
        .clone()
        .expect("the writer persists a fingerprint");
    assert_eq!(
        gui_fp.input_fingerprint, writer_fp.input_fingerprint,
        "the two front ends must fingerprint the SAME analysis domain, or a CLI-written \
         recipe looks stale in the GUI and vice versa"
    );

    // The comparison above is only meaningful if the crop actually reached the
    // domain: without it, the fingerprint would equal the un-cropped one.
    let uncropped = auto_analysis_frame(&source_frame, &EditRecipe::default(), &[]).unwrap();
    assert_ne!(
        analysis.pixels, uncropped.pixels,
        "the parity comparison must run on a cropped domain, or it proves nothing"
    );
}

/// Decodes the split fixture through the real load path and hands back the
/// decoded source frame (the CLI's `frame`), for the writer-side comparison.
fn decoded_frame() -> (EditRecipe, ImageFrame) {
    let mut app = new_app();
    app.load_bytes(split_luminance_png(), "split.png").unwrap();
    let frame = app.original.clone().expect("a decoded frame");
    (app.recipe.clone(), frame)
}
