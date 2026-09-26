use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for};

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

fn mask_list(path: &Path, json: bool) -> Output {
    let mut command = cli();
    command.args(["mask", "--input", path.to_str().unwrap(), "--list"]);
    if json {
        command.arg("--json");
    }
    command.output().unwrap()
}

fn imported_local_image(directory: &tempfile::TempDir, name: &str, pixel: u8) -> (PathBuf, String) {
    let path = write_png(directory, name, pixel);
    import_image(&path);
    let layer_id = add_and_attach_layer(&path);
    (path, layer_id)
}

#[test]
fn local_history_snapshot_restores_complete_state_and_reset_preserves_prior_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "history.png", 40);
    let add_second = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--add-luminance-range",
            "--name",
            "Bright",
            "--range-min",
            "0",
            "--range-max",
            "1",
        ])
        .output()
        .unwrap();
    assert_success(&add_second, "add second mask");
    let second_mask_id = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_library
        .iter()
        .find(|mask| mask.name == "Bright")
        .unwrap()
        .id
        .clone();
    let attach_second = cli()
        .args([
            "mask",
            "--input",
            input.to_str().unwrap(),
            "--attach-layer",
            &second_mask_id,
        ])
        .output()
        .unwrap();
    assert_success(&attach_second, "attach second mask layer");
    let original_layers = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers
        .clone();
    assert_eq!(original_layers.len(), 2);

    let initial = set_local(&input, &layer_id, "exposure=0.5");
    assert_success(&initial, "initial local edit");
    let contrast = set_local(&input, &layer_id, "contrast=0.25");
    assert_success(&contrast, "second local edit");
    let before_restore_layers = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers
        .clone();

    let final_edit = set_local(&input, &layer_id, "exposure=1.25");
    assert_success(&final_edit, "third local edit");
    let before_highlight_layers = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers
        .clone();
    let highlight = set_local(&input, &layer_id, "highlights=-0.2");
    assert_success(&highlight, "fourth local edit");

    let mut document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &mut document.virtual_copies[0];
    assert_eq!(copy.history.len(), 4);
    assert_eq!(
        copy.history[0].mask_state().unwrap().unwrap().layers,
        original_layers
    );
    let entry = copy.history.last().unwrap();
    assert!(entry.id.starts_with("mask-local-"));
    assert_eq!(
        entry.mask_state().unwrap().unwrap().layers,
        before_highlight_layers
    );
    assert_eq!(entry.changes().unwrap()[0].parameter, "mask.local");
    assert!(original_layers[0].local_adjustments.is_none());

    // Consume the typed history snapshot exactly as a non-destructive history
    // restore does, then prove the real CLI status exposes the restored value.
    let entry = copy.history[2].clone();
    let restored = entry.mask_state().unwrap().unwrap();
    copy.recipe = entry.recipe.clone();
    copy.mask_layers = restored.layers;
    document.validate().unwrap();
    save_sidecar(&sidecar_path_for(&input), &document).unwrap();

    let listed = mask_list(&input, false);
    assert_success(&listed, "list restored state");
    assert!(String::from_utf8_lossy(&listed.stdout)
        .contains("local=v6 exposure=0.5 contrast=0.25 highlights=0 shadows=0 temperature_delta_k=0 tint_delta=0 curves=none hsl=none point_color=none color_grading=none vibrance=0 saturation=0 presence=none detail=none"));

    let reset = reset_local(&input, &layer_id, "contrast");
    assert_success(&reset, "reset local adjustment");
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &document.virtual_copies[0];
    let local = copy.mask_layers[0].local_adjustments.as_ref().unwrap();
    assert_eq!(local.exposure, 0.5);
    assert_eq!(local.contrast, 0.0);
    let reset_entry = copy.history.last().unwrap();
    assert_eq!(
        reset_entry.mask_state().unwrap().unwrap().layers,
        before_restore_layers
    );
}

#[test]
fn failed_local_save_is_visible_and_the_same_edit_can_be_retried() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "failed-save.png", 50);
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();
    let sidecar_name = sidecar.file_name().unwrap().to_string_lossy();
    let lock = sidecar.with_file_name(format!(".{sidecar_name}.lock"));
    fs::write(&lock, b"held by test").unwrap();

    let failed = set_local(&input, &layer_id, "exposure=1.5");
    fs::remove_file(&lock).unwrap();

    assert_eq!(failed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        stderr.contains("sidecar is locked"),
        "missing visible save error: {stderr}"
    );
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("\"status\":\"ok\""));
    assert_eq!(fs::read(&sidecar).unwrap(), before);

    let retried = set_local(&input, &layer_id, "exposure=1.5");
    assert_success(&retried, "retried local edit");
    let document = load_sidecar(&sidecar).unwrap();
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.history.len(), 1);
    assert!(copy.history[0].mask_state().unwrap().is_some());
    assert_eq!(
        copy.mask_layers[0]
            .local_adjustments
            .as_ref()
            .unwrap()
            .exposure,
        1.5
    );
}

#[test]
fn local_status_text_is_stable_while_json_stays_structured() {
    let directory = tempfile::tempdir().unwrap();
    let (input, layer_id) = imported_local_image(&directory, "stable-output.png", 60);
    let set = set_local(&input, &layer_id, "exposure=1.25");
    assert_success(&set, "set local adjustment");
    let set = set_local(&input, &layer_id, "highlights=-0.2");
    assert_success(&set, "set local highlight adjustment");

    let text_output = mask_list(&input, false);
    assert_success(&text_output, "list local adjustments as text");
    let stdout = String::from_utf8_lossy(&text_output.stdout);
    assert!(stdout.contains("local=v6 exposure=1.25 contrast=0 highlights=-0.2 shadows=0 temperature_delta_k=0 tint_delta=0 curves=none hsl=none point_color=none color_grading=none vibrance=0 saturation=0 presence=none detail=none"));
    assert!(!stdout.contains("LocalAdjustments {"));
    assert!(!stdout.contains("version:"));

    let json_output = mask_list(&input, true);
    assert_success(&json_output, "list local adjustments as JSON");
    let document: serde_json::Value = serde_json::from_slice(&json_output.stdout).unwrap();
    let local = &document["copies"][0]["layers"][0]["local_adjustments"];
    assert!(local.is_object());
    assert_eq!(
        local["version"].as_u64(),
        Some(u64::from(lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION))
    );
    assert_eq!(local["temperature_delta_k"], 0.0);
    assert_eq!(local["tint_delta"], 0.0);
    assert_eq!(local["exposure"], 1.25);
    assert_eq!(local["contrast"], 0.0);
    assert_eq!(local["highlights"], -0.2);
    assert_eq!(local["shadows"], 0.0);
    // MASK-LOCAL-P1.2a/P1.2b: an unedited local recipe stores no curve and no
    // colour block at all — the scalars are still there and neutral.
    assert!(local.get("curves").is_none(), "{local}");
    assert!(local.get("hsl").is_none(), "{local}");
    assert!(local.get("point_color").is_none(), "{local}");
    assert!(local.get("color_grading").is_none(), "{local}");
    assert_eq!(local["vibrance"], 0.0);
    assert_eq!(local["saturation"], 0.0);
}

#[test]
fn previous_never_transfers_local_mask_state_between_images() {
    // MASK-LOCAL-P0/P1.1: cross-image `previous` is recipe-only. It must not
    // copy the reference's local mask layers — with or without P1.1 relative
    // WB deltas — and the refusal stays loud with unchanged target bytes.
    let directory = tempfile::tempdir().unwrap();
    let (source, source_layer) = imported_local_image(&directory, "source-wb.png", 70);
    assert_success(
        &set_local(&source, &source_layer, "temperature_delta_k=1250"),
        "set source temperature delta",
    );
    assert_success(
        &set_local(&source, &source_layer, "tint_delta=0.25"),
        "set source tint delta",
    );
    let (target, target_layer) = imported_local_image(&directory, "target-wb.png", 80);
    assert_success(
        &set_local(&target, &target_layer, "temperature_delta_k=-500"),
        "set target local state",
    );
    let target_before = fs::read(sidecar_path_for(&target)).unwrap();

    let output = cli()
        .args([
            "previous",
            "--from",
            source.to_str().unwrap(),
            "--to",
            target.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refused recipe-only transfer"), "{stderr}");
    assert!(
        stderr.contains("cross-image mask state transfer is unsafe"),
        "{stderr}"
    );
    assert_eq!(fs::read(sidecar_path_for(&target)).unwrap(), target_before);
    let document = load_sidecar(&sidecar_path_for(&target)).unwrap();
    let local = document.virtual_copies[0].mask_layers[0]
        .local_adjustments
        .as_ref()
        .unwrap();
    assert_eq!(local.temperature_delta_k, -500.0);
    assert_eq!(local.tint_delta, 0.0);
}

#[test]
fn previous_refuses_recipe_only_local_mask_transfer_on_source_or_target() {
    let directory = tempfile::tempdir().unwrap();

    let (local_source, source_layer) = imported_local_image(&directory, "local-source.png", 70);
    let set = set_local(&local_source, &source_layer, "exposure=1.0");
    assert_success(&set, "set source local adjustment");
    // A P1.1 relative-WB delta alone is already a refusal reason: a
    // tint-only local state must not cross images either.
    assert_success(
        &set_local(&local_source, &source_layer, "tint_delta=-0.4"),
        "set source tint delta",
    );
    let clean_target = write_png(&directory, "clean-target.png", 80);
    import_image(&clean_target);
    let target_before = fs::read(sidecar_path_for(&clean_target)).unwrap();

    let source_refusal = cli()
        .args([
            "previous",
            "--from",
            local_source.to_str().unwrap(),
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
        target_before
    );

    let clean_source = write_png(&directory, "clean-source.png", 90);
    import_image(&clean_source);
    let develop = cli()
        .args([
            "develop",
            "--input",
            clean_source.to_str().unwrap(),
            "--exposure",
            "1.5",
        ])
        .output()
        .unwrap();
    assert_success(&develop, "develop clean source");
    let (local_target, target_layer) = imported_local_image(&directory, "local-target.png", 100);
    let set = set_local(&local_target, &target_layer, "contrast=-0.25");
    assert_success(&set, "set target local adjustment");
    let local_target_before = fs::read(sidecar_path_for(&local_target)).unwrap();

    let target_refusal = cli()
        .args([
            "previous",
            "--from",
            clean_source.to_str().unwrap(),
            "--to",
            local_target.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(target_refusal.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&target_refusal.stderr);
    assert!(stderr.contains("refused recipe-only transfer"), "{stderr}");
    assert!(stderr.contains("no sidecar was changed"), "{stderr}");
    assert_eq!(
        fs::read(sidecar_path_for(&local_target)).unwrap(),
        local_target_before
    );
    let summary: serde_json::Value = serde_json::from_slice(&target_refusal.stdout).unwrap();
    assert_eq!(summary["status"], "partial");
    assert_eq!(summary["failed"], 1);
}
