use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for, RepairRegionArtifact, SourceActionSpec};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

#[test]
fn help_is_available_from_built_binary() {
    let output = cli().args(["process", "--help"]).output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--highlights"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("--shadows"));
}

#[test]
fn process_writes_jpeg_output_and_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    let output = directory.path().join("output.jpg");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--exposure",
            "0.5",
            "--highlights=-0.25",
            "--shadows",
            "0.2",
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.is_file());
    assert!(sidecar_path_for(&input).is_file());
    let decoded = ImageFrame::decode(&fs::read(output).unwrap()).unwrap();
    assert_eq!((decoded.width, decoded.height), (1, 1));
}

#[test]
fn invalid_adjustment_exits_non_zero_with_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    let output = directory.path().join("output.png");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--contrast",
            "2",
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid contrast"));
    assert!(!output.exists());
}

#[test]
fn unknown_adjustment_exits_non_zero_with_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    let output = directory.path().join("output.png");
    let preset = directory.path().join("preset.json");
    fs::write(
        &preset,
        r#"{"id":"unknown","name":"Unknown","recipe":{"adjustments":{"clarity":0.5},"options":{}}}"#,
    )
    .unwrap();
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--preset",
            preset.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported adjustment `clarity`"));
}

#[test]
fn corrupt_raw_exits_non_zero_with_decode_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.arw");
    let output = directory.path().join("output.png");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("I/O error")
            || String::from_utf8_lossy(&result.stderr).contains("LibRaw")
    );
}

#[test]
fn sidecar_only_develop_reopens_and_creates_virtual_copy() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    assert!(cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap()
        .status
        .success());
    for (id, exposure) in [("vc-warm", "--exposure=1"), ("vc-cool", "--exposure=-1")] {
        let result = cli()
            .args([
                "develop",
                "--input",
                input.to_str().unwrap(),
                "--virtual-copy",
                id,
                exposure,
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let document = lumina_sidecar::load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.virtual_copies.len(), 3);
    assert_eq!(
        document.virtual_copies[1].recipe.adjustments["exposure"],
        1.0
    );
    assert_eq!(
        document.virtual_copies[2].recipe.adjustments["exposure"],
        -1.0
    );
}

/// Writes a repair-region definition JSON plus its RGBA8 replacement PNG into
/// `directory` and returns the two paths.  `region_values` are `u16` (0..=u16::MAX);
/// pixels `>= 32768` are replaced by the matching replacement pixel.
fn write_repair_region(
    directory: &tempfile::TempDir,
    id: &str,
    kind: &str,
    width: u32,
    height: u32,
    region_values: &[u16],
    replacement: &ImageFrame,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let replacement_path = directory.path().join(format!("{id}-replacement.png"));
    fs::write(
        &replacement_path,
        replacement.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    let region_csv = region_values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let definition = format!(
        "{{\"id\":\"{id}\",\"kind\":\"{kind}\",\"region_width\":{width},\"region_height\":{height},\"region_values\":[{region_csv}],\"replacement_path\":\"{}\"}}",
        replacement_path.to_str().unwrap()
    );
    let definition_path = directory.path().join(format!("{id}.json"));
    fs::write(&definition_path, definition).unwrap();
    (definition_path, replacement_path)
}

fn pixel_at(frame: &ImageFrame, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let index = ((y * frame.width + x) * 4) as usize;
    (
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
        frame.pixels[index + 3],
    )
}

#[test]
fn dust_removal_persists_action_and_renders_only_inside_region() {
    let directory = tempfile::tempdir().unwrap();
    // A flat gray 4x4 source so the replacement is easy to detect.
    let input = {
        let mut pixels = Vec::with_capacity(4 * 4 * 4);
        for _ in 0..16 {
            pixels.extend_from_slice(&[128u8, 128, 128, 255]);
        }
        let frame = ImageFrame::new(4, 4, pixels).unwrap();
        let path = directory.path().join("input.png");
        fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        path
    };
    let original_bytes = fs::read(&input).unwrap();

    // Import first so a sidecar exists for the command to extend.
    let import = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&import.stderr)
    );

    // Top-left 2x2 marked for replacement (>= 32768); rest kept.
    let region_values = [
        65535u16, 65535, 0, 0, 65535, 65535, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    // Replacement: red(200,0,0) in the replaced corners, black elsewhere.
    let replacement = ImageFrame::new(
        4,
        4,
        vec![
            200, 0, 0, 255, 200, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 200, 0, 0, 255, 200, 0, 0,
            255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0,
            255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
        ],
    )
    .unwrap();
    let (definition, _replacement_path) = write_repair_region(
        &directory,
        "repair-1",
        "dustremoval",
        4,
        4,
        &region_values,
        &replacement,
    );

    let render_out = directory.path().join("dust.png");
    let result = cli()
        .args([
            "dust-removal",
            "--input",
            input.to_str().unwrap(),
            "--repair-region",
            definition.to_str().unwrap(),
            "--render-out",
            render_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // The recipe now records the action with a relative bundle reference and a
    // checksum that matches the persisted artifact.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let spec: &SourceActionSpec = document.virtual_copies[0]
        .recipe
        .source_actions
        .first()
        .expect("source action should be persisted");
    assert_eq!(spec.artifact.id, "repair-1");
    assert_eq!(
        spec.artifact.relative_path, "input.png.lumina.zdata",
        "bundle reference must be relative, never absolute"
    );
    let expected = RepairRegionArtifact {
        id: "repair-1".into(),
        width: 4,
        height: 4,
        region: region_values.to_vec(),
        replacement: replacement.pixels.clone(),
    };
    assert_eq!(spec.artifact.checksum, expected.checksum());

    // The rendered output differs inside the region (replaced) vs outside
    // (source preserved).
    let rendered = ImageFrame::decode(&fs::read(&render_out).unwrap()).unwrap();
    let replaced = pixel_at(&rendered, 0, 0);
    let kept = pixel_at(&rendered, 3, 3);
    assert_eq!(replaced, (200, 0, 0, 255), "region pixel must be replaced");
    assert_eq!(
        kept,
        (128, 128, 128, 255),
        "outside pixel must be untouched"
    );
    assert_ne!(replaced, kept);

    // Gap closure: a plain `render` after the action must also apply the
    // persisted source action (process_selected now resolves recipe actions).
    let via_render = directory.path().join("render.png");
    let render_result = cli()
        .args([
            "render",
            "--input",
            input.to_str().unwrap(),
            "--output",
            via_render.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        render_result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&render_result.stderr)
    );
    let rendered2 = ImageFrame::decode(&fs::read(&via_render).unwrap()).unwrap();
    assert_eq!(pixel_at(&rendered2, 0, 0), (200, 0, 0, 255));
    assert_eq!(pixel_at(&rendered2, 3, 3), (128, 128, 128, 255));

    // The original is never modified.
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

#[test]
fn dust_removal_rejects_dimension_mismatch() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    assert!(cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap()
        .status
        .success());
    // 2x2 region but a 1x1 replacement image → mismatch must fail loudly.
    let replacement = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
    let (definition, _replacement_path) = write_repair_region(
        &directory,
        "repair-bad",
        "dustremoval",
        2,
        2,
        &[65535, 0, 0, 0],
        &replacement,
    );
    let result = cli()
        .args([
            "dust-removal",
            "--input",
            input.to_str().unwrap(),
            "--repair-region",
            definition.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("does not match region"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    // No action was persisted on failure.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].recipe.source_actions.is_empty());
}
