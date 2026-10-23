use lumina_core::{ImageFileFormat, ImageFrame};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

#[test]
fn malformed_repair_region_transport_never_partially_mutates_bundle() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let source = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&input, source.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let imported = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(imported.status.success());

    let sidecar = lumina_sidecar::sidecar_path_for(&input);
    let sidecar_before = fs::read(&sidecar).unwrap();
    let zdata = lumina_sidecar::zdata_path_for(&input);
    let replacement_path = directory.path().join("replacement.png");
    let replacement = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
    fs::write(
        &replacement_path,
        replacement.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    let plane = directory.path().join("region.u16le");
    fs::write(&plane, [1, 2]).unwrap();
    let definition = directory.path().join("region.json");
    let valid_json = format!(
        r#"{{"id":"repair-1","kind":"dust-removal","region_width":1,"region_height":1,"region_path":"{}","replacement_path":"{}"}}"#,
        plane.display(),
        replacement_path.display()
    );
    fs::write(&definition, &valid_json).unwrap();

    for bytes in [&[1][..], &[1, 2, 3][..]] {
        fs::write(&plane, bytes).unwrap();
        let result = run_dust_removal(&input, &definition);
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
        assert!(!zdata.exists());
    }

    fs::write(&plane, [1, 2]).unwrap();
    fs::write(
        &definition,
        valid_json.replace(
            "\"region_path\"",
            "\"region_values\":[32769],\"region_path\"",
        ),
    )
    .unwrap();
    let legacy = run_dust_removal(&input, &definition);
    assert_eq!(legacy.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&legacy.stderr).contains("unknown field `region_values`"));
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
    assert!(!zdata.exists());

    fs::write(&definition, vec![b' '; 64 * 1024 + 1]).unwrap();
    let oversized_json = run_dust_removal(&input, &definition);
    assert_eq!(oversized_json.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&oversized_json.stderr).contains("byte limit"));
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
    assert!(!zdata.exists());
}

fn run_dust_removal(input: &std::path::Path, definition: &std::path::Path) -> std::process::Output {
    cli()
        .args([
            "dust-removal",
            "--input",
            input.to_str().unwrap(),
            "--repair-region",
            definition.to_str().unwrap(),
        ])
        .output()
        .unwrap()
}
