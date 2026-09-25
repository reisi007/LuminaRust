//! MASK-LOCAL-P1.2b CPU compositor goldens for the mask-local colour block.
//!
//! Every expected byte here is derived from the documented kernel — the local
//! stage is `global result → local relative WB → local Basic → local tone curve
//! → local HSL → local Point Color → local Vibrance/Saturation → local Color
//! Grading → fractional mask blend`, evaluated in `f64` with exactly **one**
//! RGBA8 quantization at the end.
//!
//! The strongest statements are the byte-identity tests against the *global*
//! colour stage: a local-only colour block must be byte-identical to the
//! corresponding global stage on the same input, which is only possible because
//! both paths call the very same per-pixel stage functions.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{CurvePoint, CurvePoints, HslAdjustments, HslChannel, LocalAdjustments};

/// A master curve that lifts the midtones: (0,0) → (0.5,0.7) → (1,1).
pub(super) fn lifted_master() -> CurvePoints {
    vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 0.5,
            output: 0.7,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ]
}

/// Pin a mask definition's geometry context to the golden frame so the
/// evaluated planes are used 1:1 instead of being bilinearly resampled.
pub(super) fn geometry_3x1(mut definition: MaskDefinition) -> MaskDefinition {
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    definition
}

pub(super) fn color_layer(
    id: &str,
    mask: lumina_sidecar::MaskReference,
    recipe: LocalAdjustments,
) -> MaskLayer {
    let mut layer = layer(id, mask);
    layer.local_adjustments = Some(recipe);
    layer
}

/// A single-pixel layer with one mask, rendered through the full pipeline.
pub(super) fn render_single(
    recipe: &LocalAdjustments,
    global: &EditRecipe,
    pixel: [u8; 4],
) -> Vec<u8> {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![color_layer(
            "layer-1",
            reference("vc", "subject"),
            recipe.clone(),
        )],
    )];
    let frame = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    local_render(&frame, &copies, planes, global, None)
        .unwrap()
        .frame
        .pixels
}

/// The same input through the *global* recipe, as the byte-identity reference.
pub(super) fn render_global(global: &EditRecipe, pixel: [u8; 4]) -> Vec<u8> {
    let mut frame = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
    frame.apply_recipe(global).unwrap();
    frame.pixels
}

/// The pixels every byte-identity test runs over: black, a mid grey, a warm
/// orange, a saturated blue, an almost-white and a fully saturated red.
pub(super) fn probe_pixels() -> [[u8; 4]; 6] {
    [
        [0u8, 0, 0, 255],
        [64, 128, 192, 17],
        [128, 128, 128, 255],
        [200, 90, 40, 200],
        [250, 250, 245, 3],
        [255, 0, 0, 128],
    ]
}

// ---------------------------------------------------------------- HSL

/// A local HSL-only layer is byte-identical to the global HSL stage.
#[test]
fn local_hsl_only_is_byte_identical_to_the_global_hsl_stage() {
    let hsl = HslAdjustments {
        version: 1,
        red: Some(HslChannel {
            hue: 0.25,
            saturation: -0.5,
            luminance: 0.2,
        }),
        blue: Some(HslChannel {
            hue: -0.2,
            saturation: 0.4,
            luminance: -0.1,
        }),
        ..HslAdjustments::default()
    };
    let mut local = LocalAdjustments::default();
    local.hsl = Some(hsl.clone());
    for pixel in probe_pixels() {
        let mut global = EditRecipe::default();
        global.hsl = Some(hsl.clone());
        assert_eq!(
            render_single(&local, &EditRecipe::default(), pixel),
            render_global(&global, pixel),
            "local HSL must equal the global HSL stage for {pixel:?}"
        );
    }
}

/// A hand-derived exact golden for one HSL band.
///
/// Pixel (200, 90, 40): max 200, min 40, so
/// `l = 120/255 = 0.470588`, `d = 160/255 = 0.627451`,
/// `s = d / (1 - |2l-1|) = 0.627451 / 0.941176 = 0.666667` and
/// `h = 60 * (50/160) = 18.75°`. That hue sits in the overlap of the red
/// sector (`1 - 18.75/30 = 0.375`) and the orange sector
/// (`1 - 11.25/30 = 0.625`), so only the stored red shifts apply, weighted by
/// `0.375`: `h + 0.5*30*0.375 = 24.375°`, `s - 0.25*0.375 = 0.572917`,
/// `l + 0.1*0.375 = 0.508088`. `hsl_to_rgb(24.375, 0.572917, 0.508088)`
/// gives `(0.789915, 0.455246, 0.226261)`, i.e. `(201.43, 116.09, 57.70)`
/// rounded **once** at the end of the whole local chain.
#[test]
fn local_hsl_band_has_an_exact_golden_and_preserves_alpha() {
    let mut local = LocalAdjustments::default();
    local
        .set_local_hsl_band("red", "hue", 0.5)
        .expect("red hue");
    local
        .set_local_hsl_band("red", "saturation", -0.25)
        .expect("red saturation");
    local
        .set_local_hsl_band("red", "luminance", 0.1)
        .expect("red luminance");
    let out = render_single(&local, &EditRecipe::default(), [200, 90, 40, 42]);
    assert_eq!(out, vec![201, 116, 58, 42]);
}

/// A band that does not overlap the pixel's hue is a literal no-op: the global
/// stage skips its write, and so must the local chain.
#[test]
fn a_non_overlapping_local_hsl_band_is_byte_identical_to_the_input() {
    let mut local = LocalAdjustments::default();
    local
        .set_local_hsl_band("green", "hue", 0.9)
        .expect("green hue");
    // Pure red is 0°; the green band covers [90°, 150°], so nothing touches it.
    let out = render_single(&local, &EditRecipe::default(), [255, 0, 0, 9]);
    assert_eq!(out, vec![255, 0, 0, 9]);
}

// ------------------------------------------------- Vibrance / Saturation

/// A local vibrance/saturation pair is byte-identical to the global stage.
#[test]
fn local_vibrance_and_saturation_match_the_global_stage() {
    for (vibrance, saturation) in [(0.4, 0.0), (0.0, -0.5), (0.6, -0.25), (-0.5, 0.3)] {
        let mut local = LocalAdjustments::default();
        local.set_value("vibrance", vibrance).expect("vibrance");
        local
            .set_value("saturation", saturation)
            .expect("saturation");
        for pixel in probe_pixels() {
            let mut global = EditRecipe::default();
            global.adjustments.insert("vibrance".into(), vibrance);
            if saturation != 0.0 {
                global.adjustments.insert("saturation".into(), saturation);
            }
            assert_eq!(
                render_single(&local, &EditRecipe::default(), pixel),
                render_global(&global, pixel),
                "local vibrance {vibrance} / saturation {saturation} for {pixel:?}"
            );
        }
    }
}

/// A hand-derived exact golden: negative saturation on a fully saturated red
/// moves it towards grey, and only red's own value changes in a way that is
/// reproducible from the documented formula.
#[test]
fn local_saturation_has_an_exact_golden_and_preserves_alpha() {
    let mut local = LocalAdjustments::default();
    local.set_value("saturation", -0.5).expect("saturation");
    let out = render_single(&local, &EditRecipe::default(), [255, 0, 0, 3]);
    // s = 1 -> 1 * (1 - 0.5) = 0.5, l = 0.5, h = 0 => hsl_to_rgb = (0.75, 0.25,
    // 0.25) -> (191.25, 63.75, 63.75) rounded once at the end.
    assert_eq!(out, vec![191, 64, 64, 3]);
}
