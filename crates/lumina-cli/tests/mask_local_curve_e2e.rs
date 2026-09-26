//! MASK-LOCAL-P1.2a CLI end-to-end coverage for the mask-local tone curve.
//!
//! Everything here goes through the real binary and the real sidecar file, so
//! it is evidence about the *file format* and the CLI contract, not about an
//! in-memory model: a curve written by the CLI must be readable, renderable
//! and resettable from the file, and it must never leak into a global recipe
//! or across images.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for};

/// The CLI never gained a second, near-identical flag pair: the local curve
/// rides the existing generic `--set-local-adjustment KEY=VALUE` /
/// `--reset-local-adjustment KEY` channel in the `curves.` key namespace.
const CURVE_MASTER: &str = "curves.master=0,0;0.25,0.35;0.5,0.7;1,1";
const CURVE_RED: &str = "curves.red=0,0;0.5,0.4;1,1";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str, pixel: u8) -> PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(2, 2, vec![pixel; 16]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "{action} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn import_image(path: &Path) {
    let output = cli()
        .args(["import", "--input", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_success(&output, "import");
}

fn add_and_attach_layer(path: &Path) -> String {
    let add = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--add-ai-select",
            "subject",
            "--name",
            "Subject",
        ])
        .output()
        .unwrap();
    assert_success(&add, "add mask");
    let mask_id = load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_library[0]
        .id
        .clone();
    let attach = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--attach-layer",
            &mask_id,
        ])
        .output()
        .unwrap();
    assert_success(&attach, "attach mask layer");
    load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .id
        .clone()
}

fn set_local(path: &Path, layer_id: &str, specification: &str) -> Output {
    cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--local-layer",
            layer_id,
            "--set-local-adjustment",
            specification,
        ])
        .output()
        .unwrap()
}

fn reset_local(path: &Path, layer_id: &str, key: &str) -> Output {
    cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--local-layer",
            layer_id,
            "--reset-local-adjustment",
            key,
        ])
        .output()
        .unwrap()
}

fn mask_list_json(path: &Path) -> serde_json::Value {
    let output = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--list",
            "--json",
        ])
        .output()
        .unwrap();
    assert_success(&output, "list");
    serde_json::from_slice(&output.stdout).expect("json list output")
}

fn imported_local_image(directory: &tempfile::TempDir, name: &str, pixel: u8) -> (PathBuf, String) {
    let path = write_png(directory, name, pixel);
    import_image(&path);
    let layer_id = add_and_attach_layer(&path);
    (path, layer_id)
}

fn stored_local(path: &Path) -> lumina_sidecar::LocalAdjustments {
    load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .local_adjustments
        .clone()
        .expect("typed local recipe")
}

#[test]
fn local_curve_round_trips_through_the_sidecar_file_and_resets() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "curve-roundtrip.png", 90);

    assert_success(
        &set_local(&input, &layer_id, CURVE_MASTER),
        "set local master curve",
    );
    assert_success(
        &set_local(&input, &layer_id, CURVE_RED),
        "set local red curve",
    );

    // The typed object lands in the file with both channels and the current
    // schema version.
    let local = stored_local(&input);
    assert_eq!(local.version, lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION);
    let curves = local.curves.as_ref().expect("curve block");
    assert_eq!(curves.version, 1);
    assert_eq!(curves.master.len(), 4);
    assert_eq!(curves.master[1].input, 0.25);
    assert_eq!(curves.master[1].output, 0.35);
    let red = curves.channels.red.as_ref().expect("red channel");
    assert_eq!(red.len(), 3);
    assert_eq!(red[1].output, 0.4);
    assert!(curves.channels.green.is_none());
    assert!(curves.channels.blue.is_none());

    // The JSON listing is structured (and not a Debug dump). Curve points are
    // stored as `f32`, so the comparison goes through `f32` too.
    let listed = mask_list_json(&input);
    let json_curves = &listed["copies"][0]["layers"][0]["local_adjustments"]["curves"];
    assert_eq!(json_curves["version"], 1);
    assert_eq!(json_curves["master"].as_array().unwrap().len(), 4);
    assert_eq!(
        json_curves["channels"]["red"][1]["output"]
            .as_f64()
            .unwrap() as f32,
        0.4_f32
    );
    assert!(json_curves["channels"].get("green").is_none());

    // Per-channel reset keeps the other channel.
    assert_success(
        &reset_local(&input, &layer_id, "curves.red"),
        "reset red channel",
    );
    let after_channel_reset = stored_local(&input);
    assert!(after_channel_reset
        .curves
        .as_ref()
        .unwrap()
        .channels
        .red
        .is_none());
    assert!(after_channel_reset.has_local_curves());

    // Resetting the whole block is byte-identical to never having edited it.
    assert_success(
        &reset_local(&input, &layer_id, "curves"),
        "reset all local curves",
    );
    let after_reset = stored_local(&input);
    assert!(after_reset.curves.is_none());
    assert!(after_reset.is_neutral());
    assert_eq!(after_reset.curve_summary(), "none");
    assert_eq!(after_reset.exposure, 0.0);
    assert_eq!(after_reset.temperature_delta_k, 0.0);

    // The scalar keys still work in the same transaction, and a local curve
    // never touches the global recipe.
    assert_success(
        &set_local(&input, &layer_id, "exposure=0.75"),
        "scalar after curve reset",
    );
    let combined = stored_local(&input);
    assert_eq!(combined.exposure, 0.75);
    assert!(combined.curves.is_none());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].recipe.curves.is_none());
    assert!(document.virtual_copies[0].recipe.adjustments.is_empty());
}

#[test]
fn invalid_local_curve_specs_are_loud_and_change_no_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "curve-invalid.png", 70);
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();

    for spec in [
        // one point only
        "curves.master=0,0",
        // non-ascending inputs
        "curves.master=0,0;0.7,0.7;0.4,0.4;1,1",
        // out of range output
        "curves.master=0,0;0.5,1.5;1,1",
        // missing (0,0)
        "curves.master=0.1,0.1;1,1",
        // missing (1,1)
        "curves.master=0,0;0.9,0.9",
        // unknown channel
        "curves.luma=0,0;1,1",
        // `curves` without a channel
        "curves=0,0;1,1",
        // a key that only looks like the curve namespace
        "curves_master=0,0;1,1",
        // a malformed pair
        "curves.master=0,0;x,1",
        // the P1.2b colour block needs its full key, not a bare scalar
        "hsl=0.5",
        // and a still-disabled local stage stays a loud unknown key
        "presence=0.5",
        "detail=0.5",
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
        &set_local(&input, &layer_id, CURVE_MASTER),
        "set local master curve",
    );
    let with_curve = fs::read(&sidecar).unwrap();
    let bad_reset = reset_local(&input, &layer_id, "curves.luma");
    assert_eq!(bad_reset.status.code(), Some(1));
    assert_eq!(fs::read(&sidecar).unwrap(), with_curve);
}

#[test]
fn local_curve_survives_history_reload_and_is_restored_verbatim() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "curve-history.png", 110);
    assert_success(
        &set_local(&input, &layer_id, CURVE_MASTER),
        "set local master curve",
    );
    assert_success(
        &set_local(&input, &layer_id, "exposure=0.5"),
        "second local edit",
    );

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.history.len(), 2);
    // The first snapshot is the pre-edit state (no curve), the second carries
    // the curve verbatim.
    let first = copy.history[0].mask_state().unwrap().unwrap();
    assert!(first.layers[0].local_adjustments.is_none());
    let second = copy.history[1].mask_state().unwrap().unwrap();
    let restored = second.layers[0].local_adjustments.as_ref().unwrap();
    assert_eq!(restored.curves.as_ref().unwrap().master.len(), 4);
    assert!(copy.history[1].changes().unwrap()[0]
        .to
        .contains("curves=master:4"));

    // Consume the snapshot exactly as a history restore does and prove the
    // real CLI status exposes the restored curve again.
    let entry = copy.history[0].clone();
    let snapshot = entry.mask_state().unwrap().unwrap();
    let mut copy_mut = document.virtual_copies[0].clone();
    copy_mut.recipe = entry.recipe.clone();
    copy_mut.mask_layers = snapshot.layers;
    let mut restored_document = document.clone();
    restored_document.virtual_copies[0] = copy_mut;
    restored_document.validate().unwrap();
    assert!(restored_document.virtual_copies[0].mask_layers[0]
        .local_adjustments
        .is_none());
}

/// A local curve alone is a cross-image refusal reason: `previous` stays
/// recipe-only and must refuse loudly on both the source and the target copy,
/// with unchanged target bytes.
#[test]
fn previous_refuses_a_curve_only_local_state_on_source_and_target() {
    let directory = tempfile::tempdir().unwrap();

    let (source, source_layer) = imported_local_image(&directory, "curve-source.png", 60);
    assert_success(
        &set_local(&source, &source_layer, CURVE_MASTER),
        "set source local curve",
    );
    let clean_target = write_png(&directory, "curve-clean-target.png", 80);
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
        stderr.contains("cross-image mask state transfer is unsafe"),
        "{stderr}"
    );
    assert_eq!(
        fs::read(sidecar_path_for(&clean_target)).unwrap(),
        clean_target_before
    );

    let clean_source = write_png(&directory, "curve-clean-source.png", 90);
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
    let (target, target_layer) = imported_local_image(&directory, "curve-target.png", 100);
    assert_success(
        &set_local(&target, &target_layer, CURVE_RED),
        "set target local curve",
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
    // The target still owns exactly its own curve.
    assert!(stored_local(&target).has_local_curves());
}
