//! MASK-LOCAL-P1.2b CLI end-to-end coverage for the mask-local colour block.
//!
//! Everything here goes through the real binary and the real sidecar file, so
//! it is evidence about the *file format* and the CLI contract: a colour edit
//! written by the CLI must be readable, resettable and refusable from the file,
//! it must never leak into a global recipe or across images, and it must reach
//! the pixels through the CPU compositor.

use std::fs;

use lumina_sidecar::{load_sidecar, sidecar_path_for};

#[path = "mask_local_color_common/mod.rs"]
mod common;

use common::{
    add_range_mask, assert_success, import_image, imported_local_image, mask_list_json, render_png,
    reset_local, set_local, stored_local, write_png, GRADING_SHADOWS, HSL_RED_HUE, POINT_COLOR_ADD,
};

#[test]
fn local_color_round_trips_through_the_sidecar_file_and_resets() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "color-roundtrip.png");

    assert_success(&set_local(&input, &layer_id, HSL_RED_HUE), "set hsl");
    assert_success(
        &set_local(&input, &layer_id, "vibrance=0.4"),
        "set vibrance",
    );
    assert_success(
        &set_local(&input, &layer_id, "saturation=-0.2"),
        "set saturation",
    );
    assert_success(
        &set_local(&input, &layer_id, POINT_COLOR_ADD),
        "add point color",
    );
    assert_success(
        &set_local(&input, &layer_id, GRADING_SHADOWS),
        "set grading shadows",
    );
    assert_success(
        &set_local(&input, &layer_id, "color_grading.shadows.hue=200"),
        "set grading hue",
    );
    assert_success(
        &set_local(&input, &layer_id, "color_grading.balance=-0.3"),
        "set grading balance",
    );

    // The typed object lands in the file with the current schema version and
    // every colour area.
    let local = stored_local(&input);
    assert_eq!(local.version, lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION);
    let hsl = local.hsl.as_ref().expect("hsl block");
    assert_eq!(hsl.version, 1);
    assert_eq!(hsl.red.as_ref().expect("red band").hue, -0.25);
    assert!(hsl.blue.is_none());
    assert_eq!(local.vibrance, 0.4);
    assert_eq!(local.saturation, -0.2);
    let point_color = local.point_color.as_ref().expect("point color block");
    assert_eq!(point_color.version, 1);
    assert_eq!(point_color.entries.len(), 1);
    assert_eq!(point_color.entries[0].id, "pc-1");
    assert_eq!(point_color.entries[0].hue_range, 45.0);
    let grading = local.color_grading.as_ref().expect("grading block");
    assert_eq!(grading.version, 1);
    assert_eq!(grading.shadows.hue_degrees, 200.0);
    assert_eq!(grading.shadows.saturation, 0.4);
    assert_eq!(grading.balance, -0.3);
    assert!(local.has_local_color());
    assert!(!local.is_neutral());

    // The JSON listing is structured (and not a Debug dump).
    let listed = mask_list_json(&input);
    let json_local = &listed["copies"][0]["layers"][0]["local_adjustments"];
    assert_eq!(
        json_local["hsl"]["red"]["hue"].as_f64().unwrap() as f32,
        -0.25_f32
    );
    assert_eq!(json_local["point_color"]["entries"][0]["id"], "pc-1");
    assert_eq!(
        json_local["color_grading"]["shadows"]["saturation"]
            .as_f64()
            .unwrap() as f32,
        0.4_f32
    );
    assert_eq!(json_local["vibrance"].as_f64().unwrap(), 0.4);

    // A local colour edit never touches the global recipe.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].recipe.hsl.is_none());
    assert!(document.virtual_copies[0].recipe.point_color.is_none());
    assert!(document.virtual_copies[0].recipe.color_grading.is_none());
    assert!(!document.virtual_copies[0]
        .recipe
        .adjustments
        .contains_key("vibrance"));
    assert!(!document.virtual_copies[0]
        .recipe
        .adjustments
        .contains_key("saturation"));

    // Per-area resets keep the rest of the block.
    assert_success(&reset_local(&input, &layer_id, "hsl.red"), "reset band");
    assert!(stored_local(&input).hsl.is_none());
    assert!(stored_local(&input).has_local_color());

    assert_success(
        &reset_local(&input, &layer_id, "point_color"),
        "reset point color",
    );
    assert!(stored_local(&input).point_color.is_none());

    assert_success(
        &reset_local(&input, &layer_id, "color_grading.shadows"),
        "reset grading range",
    );
    let after_range_reset = stored_local(&input);
    assert_eq!(
        after_range_reset
            .color_grading
            .as_ref()
            .unwrap()
            .shadows
            .saturation,
        0.0
    );
    assert_eq!(
        after_range_reset.color_grading.as_ref().unwrap().balance,
        -0.3
    );

    assert_success(
        &reset_local(&input, &layer_id, "color_grading.balance"),
        "reset grading balance",
    );
    // The remaining grading block is a stored hue selection with no tint, so
    // the explicit range reset is what makes the block go away.
    assert_success(
        &reset_local(&input, &layer_id, "color_grading"),
        "reset grading block",
    );
    assert!(stored_local(&input).color_grading.is_none());

    // `color` resets the whole block, including the two scalars.
    assert_success(
        &set_local(&input, &layer_id, "vibrance=0.4"),
        "set vibrance again",
    );
    assert_success(&reset_local(&input, &layer_id, "color"), "reset color");
    let after_reset = stored_local(&input);
    assert!(after_reset.is_neutral());
    assert_eq!(after_reset.vibrance, 0.0);
    assert_eq!(after_reset.saturation, 0.0);
    assert_eq!(after_reset.hsl_summary(), "none");
    assert_eq!(after_reset.point_color_summary(), "none");
    assert_eq!(after_reset.color_grading_summary(), "none");
    // The other local stages are untouched by a colour reset.
    assert_success(
        &set_local(&input, &layer_id, "exposure=0.75"),
        "scalar after color reset",
    );
    let combined = stored_local(&input);
    assert_eq!(combined.exposure, 0.75);
    assert!(!combined.has_local_color());
}

#[test]
fn local_color_edit_reaches_pixels_through_the_cpu_compositor() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "color-pixels.png");
    import_image(&input);
    add_range_mask(&input);
    let layer_id = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .id
        .clone();
    let before = render_png(&input, &directory.path().join("color-before.png"));

    assert_success(
        &set_local(&input, &layer_id, "saturation=-0.5"),
        "set local saturation",
    );
    let after = render_png(&input, &directory.path().join("color-after.png"));
    assert_ne!(before, after, "the local colour block must reach pixels");

    // Resetting it restores the exact input bytes: the render identity and the
    // local state went back to "never edited".
    assert_success(&reset_local(&input, &layer_id, "color"), "reset color");
    let restored = render_png(&input, &directory.path().join("color-restored.png"));
    assert_eq!(before, restored, "a colour reset must be byte-identical");
}

#[test]
fn invalid_local_color_specs_are_loud_and_change_no_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "color-invalid.png");
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();

    for spec in [
        // HSL out of range
        "hsl.red.hue=1.5",
        "hsl.red.hue=-1.5",
        // unknown band / field
        "hsl.luma.hue=0.2",
        "hsl.red.brightness=0.2",
        // `hsl` needs a band *and* a field
        "hsl=0.5",
        "hsl.red=0.5",
        // point colour arity and ranges
        "point_color.add=30,45,0.2",
        "point_color.add=30,45,0.2,0.1,-0.1,7",
        "point_color.add=400,45,0.2,0.1,-0.1",
        "point_color.add=30,200,0.2,0.1,-0.1",
        "point_color.add=30,45,2,0.1,-0.1",
        // unknown entry / field
        "point_color.pc-9.hue_shift=0.2",
        "point_color.pc-1.brightness=0.2",
        // grading range / field / value
        "color_grading.whites.saturation=0.4",
        "color_grading.shadows.hue=400",
        "color_grading.shadows.saturation=1.4",
        "color_grading.shadows.brightness=0.2",
        // scalars
        "vibrance=2.0",
        "saturation=-1.5",
        // a key that only looks like a colour namespace
        "hsx.red.hue=0.2",
        // and the still-disabled local stages
        "presence=0.5",
        "detail=0.5",
        "optics=0.5",
    ] {
        let output = set_local(&input, &layer_id, spec);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{spec} must be a loud error, stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("\"status\":\"ok\""),
            "{spec} reported success"
        );
        assert_eq!(fs::read(&sidecar).unwrap(), before, "{spec} changed bytes");
    }

    // An unknown reset key is loud too, and never resets the whole block.
    assert_success(&set_local(&input, &layer_id, HSL_RED_HUE), "set hsl");
    let with_color = fs::read(&sidecar).unwrap();
    for key in ["hsl.luma", "color_grading.whites", "point_color.pc-9"] {
        let output = reset_local(&input, &layer_id, key);
        assert_eq!(output.status.code(), Some(1), "{key} must be loud");
        assert_eq!(
            fs::read(&sidecar).unwrap(),
            with_color,
            "{key} changed bytes"
        );
    }
    // The block is still there after all those refusals.
    assert_eq!(
        stored_local(&input).local_hsl_band("red").unwrap().hue,
        -0.25
    );
}
