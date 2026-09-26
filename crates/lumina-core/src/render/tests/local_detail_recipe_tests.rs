//! MASK-LOCAL-P1.2d: the local detail block never mutates the global recipe.
//!
//! Split out of `local_detail_tests.rs` (file-size ratchet): the global-recipe
//! isolation and the JSON/identity round-trip are schema contracts, not pixel
//! goldens.

use super::local_detail::{render_row, step_row};
use lumina_sidecar::LocalAdjustments;

/// A local detail edit never touches the global recipe.
#[test]
fn a_local_detail_edit_never_touches_the_global_recipe() {
    let row = step_row();
    let mut recipe = LocalAdjustments::default();
    recipe.set_local_sharpening_field("amount", 0.75).unwrap();
    recipe.set_local_sharpening_field("radius", 2.0).unwrap();
    recipe.set_local_sharpening_field("detail", 0.5).unwrap();
    recipe
        .set_local_noise_reduction_field("luminance", 0.3)
        .unwrap();
    let as_recipe = recipe.as_recipe();
    assert!(
        as_recipe.sharpening.is_none(),
        "global sharpening must stay None"
    );
    assert!(
        as_recipe.noise_reduction.is_none(),
        "global noise reduction must stay None"
    );
    for forbidden in [
        "sharpening",
        "noise_reduction",
        "luminance",
        "color",
        "amount",
        "radius",
        "detail",
        "masking",
    ] {
        assert!(
            !as_recipe.adjustments.contains_key(forbidden),
            "global adjustments must not carry `{forbidden}`"
        );
    }
    // And the pixels really did change, so the assertions above are not vacuous.
    assert_ne!(render_row(&recipe, row, u16::MAX), step_row());
}
