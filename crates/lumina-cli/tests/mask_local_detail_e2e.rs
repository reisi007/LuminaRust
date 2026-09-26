//! MASK-LOCAL-P1.2d CLI end-to-end coverage for the mask-local detail block.
//!
//! Everything here goes through the real binary and the real sidecar file, so it
//! is evidence about the *file format* and the CLI contract: a detail edit
//! written by the CLI must be readable, resettable and refusable from the file,
//! it must never leak into the global recipe or across images, and it must reach
//! the pixels through the CPU compositor.
//!
//! The CLI gained **no** second flag family: the local detail rides the existing
//! generic `--set-local-adjustment KEY=VALUE` / `--reset-local-adjustment KEY`
//! channel in the `sharpening.` and `noise_reduction.` namespaces.

use std::fs;

use lumina_sidecar::{load_sidecar, sidecar_path_for};

#[path = "mask_local_color_common/mod.rs"]
mod common;

use common::{
    add_range_mask, assert_success, import_image, imported_local_image, mask_list_json, render_png,
    reset_local, set_local, stored_local, write_png,
};

#[test]
fn local_detail_round_trips_through_the_sidecar_file_and_resets() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "detail-roundtrip.png");

    for spec in [
        "sharpening.amount=1.25",
        "sharpening.radius=3.5",
        "sharpening.detail=0.25",
        "sharpening.masking=0.75",
        "noise_reduction.luminance=0.5",
        "noise_reduction.color=0.25",
    ] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
    }

    // The typed object lands in the file with the current schema version and
    // the *global* `Sharpening` / `NoiseReduction` blocks, version 1.
    let local = stored_local(&input);
    assert_eq!(local.version, lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(
        local.version,
        lumina_sidecar::DETAIL_LOCAL_ADJUSTMENTS_VERSION
    );
    let detail = local.detail.as_ref().expect("detail block");
    let sharpening = detail.sharpening.as_ref().expect("sharpening");
    assert_eq!(sharpening.version, 1);
    assert_eq!(sharpening.amount, 1.25);
    assert_eq!(sharpening.radius, 3.5);
    assert_eq!(sharpening.detail, 0.25);
    assert_eq!(sharpening.masking, 0.75);
    let noise = detail.noise_reduction.as_ref().expect("noise reduction");
    assert_eq!(noise.version, 1);
    assert_eq!(noise.luminance, 0.5);
    assert_eq!(noise.color, 0.25);
    assert!(local.has_local_detail());
    assert!(local.has_local_sharpening());
    assert!(local.has_local_noise_reduction());
    assert!(!local.is_neutral());
    assert_eq!(local.detail_summary(), "sharpening+noise_reduction");
    assert!(local
        .to_string()
        .contains("detail=sharpening+noise_reduction"));

    // The JSON listing is structured (and not a Debug dump).
    let listed = mask_list_json(&input);
    let json_detail = &listed["copies"][0]["layers"][0]["local_adjustments"]["detail"];
    assert_eq!(json_detail["sharpening"]["version"], 1);
    assert_eq!(
        json_detail["sharpening"]["amount"].as_f64().unwrap() as f32,
        1.25_f32
    );
    assert_eq!(json_detail["noise_reduction"]["version"], 1);
    assert_eq!(
        json_detail["noise_reduction"]["luminance"]
            .as_f64()
            .unwrap() as f32,
        0.5_f32
    );
    // The global recipe is untouched: no global sharpening or noise reduction.
    let recipe = &listed["copies"][0]["recipe"];
    assert!(
        recipe["sharpening"].is_null() && recipe["noise_reduction"].is_null(),
        "the global recipe must not carry a local detail: {recipe}"
    );

    // The whole-block reset removes everything and is byte-identical to never
    // having been edited.
    assert_success(&reset_local(&input, &layer_id, "detail"), "reset detail");
    let cleared = stored_local(&input);
    assert!(cleared.detail.is_none());
    assert!(!cleared.has_local_detail());
    assert!(cleared.is_neutral());
    assert_eq!(cleared.detail_summary(), "none");
    assert!(cleared.to_string().contains("detail=none"));
}

#[test]
fn per_area_detail_resets_remove_exactly_one_sub_block() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "detail-reset.png");
    for spec in ["sharpening.amount=1.0", "noise_reduction.luminance=0.4"] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
    }
    assert_eq!(
        stored_local(&input).detail_summary(),
        "sharpening+noise_reduction"
    );

    // Reset one area: only that sub-block disappears.
    assert_success(
        &reset_local(&input, &layer_id, "noise_reduction"),
        "reset noise reduction",
    );
    let after_noise_reset = stored_local(&input);
    assert_eq!(after_noise_reset.detail_summary(), "sharpening");
    assert!(after_noise_reset.has_local_sharpening());
    assert!(!after_noise_reset.has_local_noise_reduction());
    assert!(after_noise_reset
        .detail
        .as_ref()
        .expect("detail")
        .noise_reduction
        .is_none());

    // Reset the other area: the whole container is gone.
    assert_success(
        &reset_local(&input, &layer_id, "sharpening"),
        "reset sharpening",
    );
    let after_all = stored_local(&input);
    assert!(after_all.detail.is_none());
    assert!(after_all.is_neutral());

    // Writing an amount back to zero drops only the sharpening sub-block.
    assert_success(
        &set_local(&input, &layer_id, "sharpening.amount=1.0"),
        "set amount",
    );
    assert_success(
        &set_local(&input, &layer_id, "noise_reduction.color=0.5"),
        "set color",
    );
    assert_success(
        &set_local(&input, &layer_id, "sharpening.amount=0"),
        "amount back to zero",
    );
    let after_zero = stored_local(&input);
    assert_eq!(after_zero.detail_summary(), "noise_reduction");
    assert!(after_zero
        .detail
        .as_ref()
        .expect("detail")
        .sharpening
        .is_none());
}

#[test]
fn local_detail_edit_reaches_pixels_through_the_cpu_compositor() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "detail-pixels.png");
    import_image(&input);
    add_range_mask(&input);
    let layer_id = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .id
        .clone();
    let before = render_png(&input, &directory.path().join("detail-before.png"));

    // The `amount` gate: with the amount at zero the whole sharpening sub-block
    // is the F-095 identity, so radius/detail/masking alone change no byte. This
    // is the global kernel's own early return, and it is what makes the
    // kernel-path choice a content decision.
    for spec in [
        "sharpening.radius=4.0",
        "sharpening.detail=0.25",
        "sharpening.masking=0.9",
    ] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
        let block = stored_local(&input);
        assert!(
            block.detail.is_none(),
            "`{spec}` alone must not persist a sharpening block"
        );
        assert_eq!(
            render_png(
                &input,
                &directory
                    .path()
                    .join(format!("detail-neutral-{}.png", spec.replace('=', "-")))
            ),
            before,
            "`{spec}` without an amount must be byte-identical"
        );
    }

    // With a non-zero amount, every sharpening field reaches the pixels on its
    // own and its per-area reset restores the pre-P1.2d bytes exactly.
    assert_success(
        &set_local(&input, &layer_id, "sharpening.amount=0.8"),
        "set amount",
    );
    let with_amount = render_png(&input, &directory.path().join("detail-amount.png"));
    assert_ne!(before, with_amount, "the amount must reach pixels");
    for (spec, previous) in [
        ("sharpening.radius=4.0", &with_amount),
        ("sharpening.detail=0.25", &with_amount),
        ("sharpening.masking=0.9", &with_amount),
    ] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
        let after = render_png(
            &input,
            &directory
                .path()
                .join(format!("detail-{}.png", spec.replace('=', "-"))),
        );
        assert_ne!(previous, &after, "`{spec}` must reach pixels");
    }
    // `masking = 0` is the strongest setting, not a no-op.
    assert_success(
        &set_local(&input, &layer_id, "sharpening.masking=0"),
        "masking back to zero",
    );
    let unmasked = render_png(&input, &directory.path().join("detail-masking-0.png"));
    assert_ne!(
        unmasked, with_amount,
        "masking 0 and masking 0.9 must differ"
    );
    assert_success(&reset_local(&input, &layer_id, "detail"), "reset detail");
    assert_eq!(
        render_png(&input, &directory.path().join("detail-restored-all.png")),
        before
    );

    // The noise-reduction fields reach the pixels on their own.
    for spec in ["noise_reduction.luminance=0.6", "noise_reduction.color=0.5"] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
        let after = render_png(
            &input,
            &directory
                .path()
                .join(format!("detail-{}.png", spec.replace('=', "-"))),
        );
        assert_ne!(before, after, "`{spec}` must reach pixels");
        assert_success(
            &reset_local(&input, &layer_id, "noise_reduction"),
            "per-area reset",
        );
        let restored = render_png(
            &input,
            &directory
                .path()
                .join(format!("detail-restored-{}.png", spec.replace('=', "-"))),
        );
        assert_eq!(
            before, restored,
            "`{spec}`: a per-area detail reset must be byte-identical"
        );
    }

    // A persisted block that cannot change a pixel keeps the pre-P1.2d bytes.
    assert_success(
        &set_local(&input, &layer_id, "noise_reduction.luminance=0.5"),
        "set luminance",
    );
    assert_success(
        &set_local(&input, &layer_id, "noise_reduction.luminance=0"),
        "luminance back to zero",
    );
    let zero_block = stored_local(&input);
    assert!(
        zero_block.detail.is_none(),
        "a neutral noise-reduction block must not linger in storage"
    );
    assert_eq!(
        render_png(&input, &directory.path().join("detail-zero.png")),
        before,
        "a neutral detail block must keep the pre-P1.2d bytes"
    );
}

#[test]
fn invalid_local_detail_specs_are_loud_and_change_no_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "detail-invalid.png");
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();

    for spec in [
        // out of range, including just outside the two radius boundaries
        "sharpening.amount=3.5",
        "sharpening.amount=-0.5",
        "sharpening.radius=0.05",
        "sharpening.radius=10.5",
        "sharpening.detail=1.5",
        "sharpening.masking=-0.5",
        "noise_reduction.luminance=1.5",
        "noise_reduction.color=-0.5",
        // not a number
        "sharpening.amount=abc",
        "sharpening.amount=",
        "noise_reduction.luminance=nan-ish",
        // unknown field inside a known sub-block
        "sharpening.grain=0.5",
        "sharpening.texture=0.5",
        "noise_reduction.amount=0.5",
        "noise_reduction.colorfulness=0.5",
        // the sub-block names need a field
        "sharpening=0.5",
        "noise_reduction=0.5",
        // a key that only looks like the namespace
        "sharpeningx.amount=0.5",
        "noise_reductionx.luminance=0.5",
        // the whole block needs no value, and never had a set form
        "detail.sharpening.amount=0.5",
        // and the still-disabled local stages stay refused
        "denoise_ai=0.5",
        "optics=0.5",
        "lens_correction=0.5",
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

    // An unknown reset key is loud too, and never resets a real sub-block.
    assert_success(
        &set_local(&input, &layer_id, "sharpening.amount=1.0"),
        "set amount",
    );
    assert_success(
        &set_local(&input, &layer_id, "noise_reduction.luminance=0.4"),
        "set luminance",
    );
    let with_detail = fs::read(&sidecar).unwrap();
    for key in [
        "sharpening.grain",
        "noise_reduction.amount",
        "detail.sharpening",
        "sharpeningx",
        "denoise_ai",
        "optics",
    ] {
        let output = reset_local(&input, &layer_id, key);
        assert_eq!(output.status.code(), Some(1), "{key} must be loud");
        assert_eq!(
            fs::read(&sidecar).unwrap(),
            with_detail,
            "{key} changed bytes"
        );
    }
    // The whole block is still there after all those refusals.
    let after = stored_local(&input);
    assert_eq!(after.detail_summary(), "sharpening+noise_reduction");
    assert_eq!(
        after
            .detail
            .as_ref()
            .unwrap()
            .sharpening
            .as_ref()
            .unwrap()
            .amount,
        1.0
    );
    assert_eq!(
        after
            .detail
            .as_ref()
            .unwrap()
            .noise_reduction
            .as_ref()
            .unwrap()
            .luminance,
        0.4
    );
}

#[test]
fn local_detail_survives_history_and_reload_verbatim() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "detail-history.png");
    assert_success(
        &set_local(&input, &layer_id, "sharpening.amount=1.0"),
        "set amount",
    );
    assert_success(
        &set_local(&input, &layer_id, "noise_reduction.luminance=0.4"),
        "set luminance",
    );
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.history.len(), 2);
    // The first snapshot is the pre-edit state.
    assert!(copy.history[0].mask_state().unwrap().unwrap().layers[0]
        .local_adjustments
        .is_none());
    // The second snapshot is the state before the noise-reduction edit, so it
    // carries the sharpening sub-block and not the noise-reduction one.
    let second = copy.history[1].mask_state().unwrap().unwrap();
    let restored = second.layers[0].local_adjustments.as_ref().unwrap();
    assert!(restored.has_local_sharpening());
    assert!(!restored.has_local_noise_reduction());
    assert_eq!(
        restored
            .detail
            .as_ref()
            .unwrap()
            .sharpening
            .as_ref()
            .unwrap()
            .amount,
        1.0
    );
    assert!(copy.history[1].changes().unwrap()[0]
        .to
        .contains("detail=sharpening"));
    // The live layer carries both sub-blocks.
    let live = copy.mask_layers[0].local_adjustments.as_ref().unwrap();
    assert_eq!(live.detail_summary(), "sharpening+noise_reduction");

    // A real save/reload round-trip is byte-stable.
    let reloaded = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let after = stored_local(&input);
    assert_eq!(after.detail, live.detail);
    let json = reloaded.to_json().unwrap();
    let again = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(again.to_json().unwrap(), json);
    assert_eq!(stored_local(&input).detail, after.detail);
}
