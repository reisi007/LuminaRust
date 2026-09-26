//! MASK-LOCAL-P1.2b CLI end-to-end coverage, part 2: the local colour block
//! inside history snapshots, reloads and the cross-image `previous` refusal.
//!
//! Split from `mask_local_color_e2e.rs` (file-size ratchet): that half owns the
//! round trip, the pixel proof and the loud refusals, this half owns history,
//! reload and the cross-image contract.

use std::fs;

use lumina_sidecar::{load_sidecar, sidecar_path_for};

#[path = "mask_local_color_common/mod.rs"]
mod common;

use common::{
    assert_success, cli, import_image, imported_local_image, set_local, stored_local, write_png,
    GRADING_SHADOWS, HSL_RED_HUE,
};

#[test]
fn local_color_survives_history_and_reload_verbatim() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "color-history.png");
    assert_success(&set_local(&input, &layer_id, HSL_RED_HUE), "set hsl");
    assert_success(
        &set_local(&input, &layer_id, GRADING_SHADOWS),
        "set grading",
    );

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.history.len(), 2);
    // The first snapshot is the pre-edit state, the second carries the colour
    // block verbatim.
    assert!(copy.history[0].mask_state().unwrap().unwrap().layers[0]
        .local_adjustments
        .is_none());
    // The second snapshot is the state *before* the grading edit, so it carries
    // the HSL block and not the grading block yet.
    let second = copy.history[1].mask_state().unwrap().unwrap();
    let restored = second.layers[0].local_adjustments.as_ref().unwrap();
    assert_eq!(restored.local_hsl_band("red").unwrap().hue, -0.25);
    assert!(restored.color_grading.is_none());
    assert!(copy.history[1].changes().unwrap()[0]
        .to
        .contains("hsl=1bands"));
    // The live layer carries both areas.
    assert_eq!(
        copy.mask_layers[0]
            .local_adjustments
            .as_ref()
            .unwrap()
            .color_grading
            .as_ref()
            .unwrap()
            .shadows
            .saturation,
        0.4
    );

    // A full sidecar reload restores the same block.
    let reloaded = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        reloaded.virtual_copies[0].mask_layers[0].local_adjustments,
        copy.mask_layers[0].local_adjustments
    );
}

/// A local colour block alone is a cross-image refusal reason: `previous`
/// stays recipe-only and must refuse loudly on both the source and the target
/// copy, with unchanged target bytes.
#[test]
fn previous_refuses_a_color_only_local_state_on_source_and_target() {
    let directory = tempfile::tempdir().unwrap();

    let (source, source_layer) = imported_local_image(&directory, "color-source.png");
    assert_success(&set_local(&source, &source_layer, HSL_RED_HUE), "set hsl");
    let clean_target = write_png(&directory, "color-clean-target.png");
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
    assert_eq!(
        fs::read(sidecar_path_for(&clean_target)).unwrap(),
        clean_target_before
    );

    let clean_source = write_png(&directory, "color-clean-source.png");
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
    let (target, target_layer) = imported_local_image(&directory, "color-target.png");
    assert_success(
        &set_local(&target, &target_layer, "vibrance=0.5"),
        "set target vibrance",
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
    assert_eq!(fs::read(sidecar_path_for(&target)).unwrap(), target_before);
    let summary: serde_json::Value = serde_json::from_slice(&target_refusal.stdout).unwrap();
    assert_eq!(summary["status"], "partial");
    assert_eq!(summary["failed"], 1);
    // The target still owns exactly its own local vibrance.
    assert_eq!(stored_local(&target).vibrance, 0.5);
}
