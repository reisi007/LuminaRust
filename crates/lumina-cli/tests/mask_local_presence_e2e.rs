//! MASK-LOCAL-P1.2c CLI end-to-end coverage for the mask-local presence block.
//!
//! Everything here goes through the real binary and the real sidecar file, so it
//! is evidence about the *file format* and the CLI contract: a presence edit
//! written by the CLI must be readable, resettable and refusable from the file,
//! it must never leak into the global recipe or across images, and it must
//! reach the pixels through the CPU compositor.
//!
//! The CLI gained **no** second flag family: the local presence rides the
//! existing generic `--set-local-adjustment KEY=VALUE` /
//! `--reset-local-adjustment KEY` channel in the `presence.` namespace.

use std::fs;

use lumina_sidecar::{load_sidecar, sidecar_path_for};

#[path = "mask_local_color_common/mod.rs"]
mod common;

use common::{
    add_range_mask, assert_success, cli, import_image, imported_local_image, mask_list_json,
    render_png, reset_local, set_local, stored_local, write_png,
};

#[test]
fn local_presence_round_trips_through_the_sidecar_file_and_resets() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "presence-roundtrip.png");

    for spec in [
        "presence.texture=0.5",
        "presence.clarity=-0.25",
        "presence.dehaze=0.4",
    ] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
    }

    // The typed object lands in the file with the current schema version and
    // the *global* `Presence` block, version 1.
    let local = stored_local(&input);
    assert_eq!(local.version, lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(local.version, 5);
    let presence = local.presence.as_ref().expect("presence block");
    assert_eq!(presence.version, 1);
    assert_eq!(presence.texture, 0.5);
    assert_eq!(presence.clarity, -0.25);
    assert_eq!(presence.dehaze, 0.4);
    assert!(local.has_local_presence());
    assert!(!local.is_neutral());
    assert_eq!(local.presence_summary(), "texture+clarity+dehaze");
    assert!(local
        .to_string()
        .contains("presence=texture+clarity+dehaze"));

    // The JSON listing is structured (and not a Debug dump).
    let listed = mask_list_json(&input);
    let json_presence = &listed["copies"][0]["layers"][0]["local_adjustments"]["presence"];
    assert_eq!(json_presence["version"], 1);
    assert_eq!(json_presence["texture"].as_f64().unwrap() as f32, 0.5_f32);
    assert_eq!(json_presence["clarity"].as_f64().unwrap() as f32, -0.25_f32);
    assert_eq!(json_presence["dehaze"].as_f64().unwrap() as f32, 0.4_f32);

    // A local presence edit never touches the global recipe.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].recipe.presence.is_none());
    assert!(!document.virtual_copies[0]
        .recipe
        .adjustments
        .contains_key("presence"));

    // Typing one amount back to zero keeps the rest of the block.
    assert_success(
        &set_local(&input, &layer_id, "presence.texture=0"),
        "texture back to zero",
    );
    let reduced = stored_local(&input);
    assert_eq!(reduced.presence.as_ref().unwrap().texture, 0.0);
    assert_eq!(reduced.presence.as_ref().unwrap().clarity, -0.25);
    assert!(reduced.has_local_presence());
    assert_eq!(reduced.presence_summary(), "clarity+dehaze");

    // `presence` resets the whole block.
    assert_success(
        &reset_local(&input, &layer_id, "presence"),
        "reset presence",
    );
    let after_reset = stored_local(&input);
    assert!(after_reset.presence.is_none());
    assert!(after_reset.is_neutral());
    assert_eq!(after_reset.presence_summary(), "none");
    // The other local stages are untouched by a presence reset.
    assert_success(
        &set_local(&input, &layer_id, "exposure=0.75"),
        "scalar after presence reset",
    );
    let combined = stored_local(&input);
    assert_eq!(combined.exposure, 0.75);
    assert!(combined.presence.is_none());
    assert!(!combined.has_local_presence());
}

#[test]
fn local_presence_edit_reaches_pixels_through_the_cpu_compositor() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "presence-pixels.png");
    import_image(&input);
    add_range_mask(&input);
    let layer_id = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .id
        .clone();
    let before = render_png(&input, &directory.path().join("presence-before.png"));

    // Texture, clarity and dehaze each reach the rendered pixels on their own.
    for spec in [
        "presence.texture=0.8",
        "presence.clarity=0.5",
        "presence.dehaze=0.6",
    ] {
        assert_success(&set_local(&input, &layer_id, spec), spec);
        let after = render_png(
            &input,
            &directory
                .path()
                .join(format!("presence-{}.png", spec.replace('=', "-"))),
        );
        assert_ne!(before, after, "`{spec}` must reach pixels");
        assert_success(
            &reset_local(&input, &layer_id, "presence"),
            "reset presence",
        );
        let restored = render_png(
            &input,
            &directory
                .path()
                .join(format!("presence-restored-{}.png", spec.replace('=', "-"))),
        );
        assert_eq!(
            before, restored,
            "`{spec}`: a presence reset must be byte-identical"
        );
    }

    // A persisted all-zero presence block is byte-identical to no block at all:
    // the kernel-path choice is content-based.
    assert_success(
        &set_local(&input, &layer_id, "presence.texture=0.5"),
        "set texture",
    );
    let with_presence = render_png(&input, &directory.path().join("presence-on.png"));
    assert_ne!(before, with_presence);
    assert_success(
        &set_local(&input, &layer_id, "presence.texture=0"),
        "texture back to zero",
    );
    let zero_block = stored_local(&input);
    assert!(
        zero_block.presence.is_none(),
        "an all-zero block must not linger in storage"
    );
    assert_eq!(
        render_png(&input, &directory.path().join("presence-zero.png")),
        before,
        "an all-zero presence block must keep the pre-P1.2c bytes"
    );
}

#[test]
fn invalid_local_presence_specs_are_loud_and_change_no_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "presence-invalid.png");
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();

    for spec in [
        // out of range
        "presence.texture=1.5",
        "presence.texture=-1.000001",
        "presence.clarity=2",
        "presence.dehaze=-1.5",
        // unknown field
        "presence.grain=0.5",
        "presence.detail=0.5",
        "presence.sharpening=0.5",
        "presence.noise_reduction=0.5",
        "presence.optics=0.5",
        // `presence` needs a field
        "presence=0.5",
        // not a number
        "presence.texture=abc",
        "presence.texture=",
        // a key that only looks like the presence namespace
        "presencex.texture=0.5",
        // and the still-disabled local stages as bare scalars
        "detail=0.5",
        "sharpening=0.5",
        "noise_reduction=0.5",
        "denoise_ai=0.5",
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
    assert_success(
        &set_local(&input, &layer_id, "presence.texture=0.5"),
        "set texture",
    );
    let with_presence = fs::read(&sidecar).unwrap();
    for key in ["presence.texture", "presence.grain", "presencex"] {
        let output = reset_local(&input, &layer_id, key);
        assert_eq!(output.status.code(), Some(1), "{key} must be loud");
        assert_eq!(
            fs::read(&sidecar).unwrap(),
            with_presence,
            "{key} changed bytes"
        );
    }
    // The block is still there after all those refusals.
    assert_eq!(stored_local(&input).presence.as_ref().unwrap().texture, 0.5);
}

#[test]
fn local_presence_survives_history_and_reload_verbatim() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "presence-history.png");
    assert_success(
        &set_local(&input, &layer_id, "presence.texture=0.5"),
        "set texture",
    );
    assert_success(
        &set_local(&input, &layer_id, "presence.dehaze=0.4"),
        "set dehaze",
    );

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.history.len(), 2);
    // The first snapshot is the pre-edit state.
    assert!(copy.history[0].mask_state().unwrap().unwrap().layers[0]
        .local_adjustments
        .is_none());
    // The second snapshot is the state *before* the dehaze edit, so it carries
    // the texture amount and not the dehaze amount.
    let second = copy.history[1].mask_state().unwrap().unwrap();
    let restored = second.layers[0].local_adjustments.as_ref().unwrap();
    assert_eq!(restored.version, 5);
    assert_eq!(restored.presence.as_ref().unwrap().texture, 0.5);
    assert_eq!(restored.presence.as_ref().unwrap().dehaze, 0.0);
    assert!(copy.history[1].changes().unwrap()[0]
        .to
        .contains("presence=texture"));
    // The live layer carries both amounts.
    let live = copy.mask_layers[0].local_adjustments.as_ref().unwrap();
    assert_eq!(live.presence.as_ref().unwrap().texture, 0.5);
    assert_eq!(live.presence.as_ref().unwrap().dehaze, 0.4);

    // A full sidecar reload restores the same block.
    let reloaded = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        reloaded.virtual_copies[0].mask_layers[0].local_adjustments,
        copy.mask_layers[0].local_adjustments
    );
}

/// A local presence block **alone** is a cross-image refusal reason:
/// `previous` stays recipe-only and must refuse loudly on both the source and
/// the target copy, with unchanged target bytes.
#[test]
fn previous_refuses_a_presence_only_local_state_on_source_and_target() {
    let directory = tempfile::tempdir().unwrap();

    let (source, source_layer) = imported_local_image(&directory, "presence-source.png");
    assert_success(
        &set_local(&source, &source_layer, "presence.texture=0.5"),
        "set source texture",
    );
    // Nothing else is set: a presence block ALONE is already a refusal reason.
    assert!(stored_local(&source).exposure == 0.0);
    let clean_target = write_png(&directory, "presence-clean-target.png");
    import_image(&clean_target);
    let clean_target_before = fs::read(sidecar_path_for(&clean_target)).unwrap();

    let source_refusal = cli()
        .args([
            "previous",
            "--from",
            source.to_str().unwrap(),
            "--to",
            clean_target.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(source_refusal.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&source_refusal.stderr);
    assert!(stderr.contains("refused recipe-only transfer"), "{stderr}");
    assert!(
        stderr.contains(source_layer.as_str()) || stderr.contains("layer-"),
        "the refusal must name the offending layer: {stderr}"
    );
    assert_eq!(
        fs::read(sidecar_path_for(&clean_target)).unwrap(),
        clean_target_before,
        "the source abort must not touch the target"
    );

    let clean_source = write_png(&directory, "presence-clean-source.png");
    import_image(&clean_source);
    assert_success(
        &cli()
            .args([
                "develop",
                "--input",
                clean_source.to_str().unwrap(),
                "--exposure",
                "1.5",
            ])
            .output()
            .unwrap(),
        "develop clean source",
    );
    let (target, target_layer) = imported_local_image(&directory, "presence-target.png");
    assert_success(
        &set_local(&target, &target_layer, "presence.clarity=0.5"),
        "set target clarity",
    );
    let target_before = fs::read(sidecar_path_for(&target)).unwrap();

    let target_refusal = cli()
        .args([
            "previous",
            "--from",
            clean_source.to_str().unwrap(),
            "--to",
            target.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(target_refusal.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&target_refusal.stderr);
    assert!(stderr.contains("refused recipe-only transfer"), "{stderr}");
    assert!(stderr.contains("no sidecar was changed"), "{stderr}");
    assert_eq!(
        fs::read(sidecar_path_for(&target)).unwrap(),
        target_before,
        "a refused target must keep byte-identical bytes"
    );
    let summary: serde_json::Value = serde_json::from_slice(&target_refusal.stdout).unwrap();
    assert_eq!(summary["status"], "partial");
    assert_eq!(summary["failed"], 1);
    // The target still owns exactly its own local presence.
    assert_eq!(
        stored_local(&target).presence.as_ref().unwrap().clarity,
        0.5
    );
}
