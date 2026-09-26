//! MASK-LOCAL-P1.2d CLI end-to-end coverage for the cross-image `previous`
//! contract with a local detail block.
//!
//! Split from `mask_local_detail_e2e.rs` (file-size ratchet): `previous` stays a
//! recipe-only transfer, and a detail block **alone** is already a refusal
//! reason on both the source and the target.

use std::fs;

use lumina_sidecar::sidecar_path_for;

#[path = "mask_local_color_common/mod.rs"]
mod common;

use common::{
    assert_success, cli, import_image, imported_local_image, set_local, stored_local, write_png,
};

/// `previous` stays recipe-only and must refuse loudly on both the source and
/// the target copy, with unchanged target bytes.
#[test]
fn previous_refuses_a_detail_only_local_state_on_source_and_target() {
    let directory = tempfile::tempdir().unwrap();

    let (source, source_layer) = imported_local_image(&directory, "detail-source.png");
    assert_success(
        &set_local(&source, &source_layer, "sharpening.amount=1.0"),
        "set source amount",
    );
    // Nothing else is set: a detail block ALONE is already a refusal reason.
    assert_eq!(stored_local(&source).exposure, 0.0);
    let clean_target = write_png(&directory, "detail-clean-target.png");
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

    let clean_source = write_png(&directory, "detail-clean-source.png");
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
    let (target, target_layer) = imported_local_image(&directory, "detail-target.png");
    assert_success(
        &set_local(&target, &target_layer, "noise_reduction.color=0.5"),
        "set target colour",
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
    // The target still owns exactly its own local noise reduction.
    assert_eq!(
        stored_local(&target)
            .detail
            .as_ref()
            .unwrap()
            .noise_reduction
            .as_ref()
            .unwrap()
            .color,
        0.5
    );
}
