//! CLI source-action reference parity regressions.
//!
//! The second half (AUTO-TONE-ANALYSIS-INPUT-8) drives the **real binary**
//! through `process --auto-tone --preset` in both directions of the
//! preset/source-action interaction, because that interaction is only observable
//! at the CLI level and no unit test reaches it: `apply_preset_layer` replaces
//! the **whole** recipe, so the source-action resolution must be taken *after*
//! that layer or the measurement and the render disagree about which retouche
//! is in force. See `AutoToneDomain` in `src/auto_tone_cli.rs`.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, EditRecipe, SidecarDocument, SourceActionSpec,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_repair_definition(directory: &Path, replacement_path: &Path) -> std::path::PathBuf {
    let definition = directory.join("repair.json");
    fs::write(
        &definition,
        serde_json::to_vec(&serde_json::json!({
            "id": "repair-1",
            "kind": "dustremoval",
            "region_width": 1,
            "region_height": 1,
            "region_values": [65535],
            "replacement_path": replacement_path,
        }))
        .unwrap(),
    )
    .unwrap();
    definition
}

#[test]
fn render_rejects_mismatched_source_action_bundle_without_touching_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let source = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&input, source.encode(ImageFileFormat::Png).unwrap()).unwrap();

    let import = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );

    let replacement_path = directory.path().join("replacement.png");
    let replacement = ImageFrame::new(1, 1, vec![201, 0, 0, 255]).unwrap();
    fs::write(
        &replacement_path,
        replacement.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    let definition = write_repair_definition(directory.path(), &replacement_path);
    let dust = cli()
        .args([
            "dust-removal",
            "--input",
            input.to_str().unwrap(),
            "--repair-region",
            definition.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        dust.status.success(),
        "{}",
        String::from_utf8_lossy(&dust.stderr)
    );

    let sidecar_path = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0].recipe.source_actions[0]
        .artifact
        .relative_path = "different.lumina.zdata".into();
    save_sidecar(&sidecar_path, &document).unwrap();
    let sidecar_before = fs::read(&sidecar_path).unwrap();

    let output = directory.path().join("render.png");
    let result = cli()
        .args([
            "render",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("bundle path mismatch"));
    assert!(!output.exists(), "a rejected render must not write output");
    assert_eq!(fs::read(&sidecar_path).unwrap(), sidecar_before);
}

// --- AUTO-TONE-ANALYSIS-INPUT-8: the preset layer and source-action resolution

/// Imports `input.png` and attaches one **real** persisted source action to the
/// sidecar (a written `.lumina.zdata` bundle plus the recipe spec), returning the
/// input path and the persisted spec.
fn import_with_source_action(directory: &Path) -> (PathBuf, Vec<SourceActionSpec>) {
    let input = directory.join("input.png");
    let source = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&input, source.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let import = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );

    let replacement_path = directory.join("replacement.png");
    let replacement = ImageFrame::new(1, 1, vec![201, 0, 0, 255]).unwrap();
    fs::write(
        &replacement_path,
        replacement.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    let definition = write_repair_definition(directory, &replacement_path);
    let dust = cli()
        .args([
            "dust-removal",
            "--input",
            input.to_str().unwrap(),
            "--repair-region",
            definition.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        dust.status.success(),
        "{}",
        String::from_utf8_lossy(&dust.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let specs = document.virtual_copies[0].recipe.source_actions.clone();
    assert_eq!(
        specs.len(),
        1,
        "premise: the sidecar carries one source action"
    );
    (input, specs)
}

/// Writes a `--preset` file whose recipe is `recipe` (the real `EditRecipe`
/// serializer, so nothing about the shape is hand-rolled here).
fn write_preset(directory: &Path, name: &str, recipe: &EditRecipe) -> PathBuf {
    let path = directory.join(name);
    let json = serde_json::json!({ "id": "preset-1", "name": name, "recipe": recipe });
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    path
}

/// `process --auto-tone --preset <preset> --output <out>`; returns
/// `(exit code, stderr)`.
fn process_auto_tone_with_preset(input: &Path, preset: &Path, out: &Path) -> (i32, String) {
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--auto-tone",
            "--preset",
            preset.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    (
        result.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&result.stderr).to_string(),
    )
}

/// **Direction 1 — the source action is in the SIDECAR and the preset has none.**
///
/// `--preset` replaces the whole recipe, so the *effective* recipe carries no
/// retouche: the run must succeed and render. The bundle is deleted first, and
/// that is the load-bearing part of the fixture — with the bundle present the
/// run would succeed no matter where the resolution is taken, so the test would
/// be vacuous. Resolving the source actions *before* the preset layer instead
/// aborts here with exit 1 on a retouche that is not in force at all.
///
/// MUTATION: moving `AutoToneDomain::resolve` back above `apply_preset_layer`
/// in `process_selected` turns this red (exit 1, no `out.png`).
#[test]
fn auto_tone_with_a_preset_without_source_actions_does_not_fail_on_the_sidecars_retouche() {
    let directory = tempfile::tempdir().unwrap();
    let (input, specs) = import_with_source_action(directory.path());
    // The effective recipe will have none; remove the bundle so that a
    // resolution against the *sidecar's* retouche could only fail loudly.
    fs::remove_file(lumina_sidecar::zdata_path_for(&input)).unwrap();

    let mut recipe = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe
        .clone();
    assert_eq!(
        recipe.source_actions, specs,
        "premise: the retouche really is in the sidecar before the preset replaces it"
    );
    recipe.source_actions.clear();
    recipe.adjustments.insert("exposure".into(), 0.25);
    let preset = write_preset(directory.path(), "without-retouche.json", &recipe);
    let out = directory.path().join("out.png");

    let (code, stderr) = process_auto_tone_with_preset(&input, &preset, &out);
    assert_eq!(
        code, 0,
        "a preset without source actions must not fail on the sidecar's retouche: {stderr}"
    );
    assert!(
        out.exists(),
        "the run must render the effective recipe, not abort: {stderr}"
    );
    // The preset's own (empty) source-action list is what got persisted — proof
    // that the render and the analysis agreed on one effective recipe.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(
        document.virtual_copies[0].recipe.source_actions.is_empty(),
        "the persisted recipe must be the preset's, not a mixture"
    );
}

/// **Direction 2 — the source action is in the PRESET and the sidecar has none.**
///
/// The effective recipe carries a retouche whose bundle is gone, and F-042-N1
/// requires that to be reported loudly — never rendered around and then written
/// into the sidecar as if it had been applied.
///
/// MUTATION: moving `AutoToneDomain::resolve` back above `apply_preset_layer`
/// turns this red (exit 0, `out.png` written, sidecar mutated).
#[test]
fn auto_tone_with_a_preset_carrying_source_actions_reports_the_missing_bundle_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let (input, specs) = import_with_source_action(directory.path());
    // Hand the retouche to the preset and take it away from the sidecar, then
    // delete the bundle: the effective recipe now references an artifact that
    // cannot be resolved.
    let sidecar_path = sidecar_path_for(&input);
    let mut document: SidecarDocument = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0].recipe.source_actions.clear();
    save_sidecar(&sidecar_path, &document).unwrap();
    fs::remove_file(lumina_sidecar::zdata_path_for(&input)).unwrap();

    let mut recipe = document.virtual_copies[0].recipe.clone();
    recipe.source_actions = specs;
    let preset = write_preset(directory.path(), "with-retouche.json", &recipe);
    let out = directory.path().join("out.png");
    let sidecar_before = fs::read(&sidecar_path).unwrap();

    let (code, stderr) = process_auto_tone_with_preset(&input, &preset, &out);
    assert_eq!(
        code, 1,
        "an unresolvable source action in the effective recipe must abort: {stderr}"
    );
    assert!(
        stderr.contains("could not read source-action bundle"),
        "the failure must name the missing bundle, not fail silently: {stderr}"
    );
    assert!(!out.exists(), "an aborted run must not write output");
    assert_eq!(
        fs::read(&sidecar_path).unwrap(),
        sidecar_before,
        "an aborted run must not persist the preset's unapplied retouche"
    );
}
