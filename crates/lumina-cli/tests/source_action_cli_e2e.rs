//! CLI source-action reference parity regressions.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for};
use std::fs;
use std::path::Path;
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
