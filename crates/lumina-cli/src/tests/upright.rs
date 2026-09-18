use super::*;

/// LRPAR-G06-UPRIGHT-15: analyze → disable → enable → clear roundtrip with
/// exactly one history entry per mutating call and an untouched original.
#[test]
fn upright_analyze_enable_disable_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("tilted.png");
    fs::write(
        &input,
        tilted_png_frame(128, 6.0)
            .encode(ImageFileFormat::Png)
            .unwrap(),
    )
    .unwrap();
    let original_bytes = fs::read(&input).unwrap();
    import_sidecar_for(&input);

    // Read-only list before analysis leaves the sidecar byte-identical.
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read(&sidecar_path).unwrap();
    upright(upright_base_args(input.clone())).unwrap();
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(document.virtual_copies[0].recipe.upright.is_none());

    // Analyze + persist (enabled by default).
    let mut analyze = upright_base_args(input.clone());
    analyze.analyze = true;
    upright(analyze).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let stage = document.virtual_copies[0]
        .recipe
        .upright
        .as_ref()
        .expect("upright persisted");
    assert!(stage.enabled);
    let analysis = stage.analysis.as_ref().expect("analysis persisted");
    assert_eq!(
        analysis.fingerprint.algorithm,
        lumina_core::UPRIGHT_ALGORITHM
    );
    assert!(analysis.line_count > 0);
    assert!(analysis.rotation.abs() > 0.0);
    assert!(document.virtual_copies[0]
        .recipe
        .effective_perspective()
        .is_some());
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    assert!(document.virtual_copies[0].history[0]
        .id
        .starts_with("upright-"));

    // Disable: the manual perspective returns, the analysis stays persisted.
    let mut disable = upright_base_args(input.clone());
    disable.disable = true;
    upright(disable).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let stage = document.virtual_copies[0].recipe.upright.as_ref().unwrap();
    assert!(!stage.enabled);
    assert!(stage.analysis.is_some());
    assert!(document.virtual_copies[0]
        .recipe
        .effective_perspective()
        .is_none());

    // Enable again without re-analyzing.
    let mut enable = upright_base_args(input.clone());
    enable.enable = true;
    upright(enable).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(
        document.virtual_copies[0]
            .recipe
            .upright
            .as_ref()
            .unwrap()
            .enabled
    );
    assert!(document.virtual_copies[0]
        .recipe
        .effective_perspective()
        .is_some());

    // Enable without analysis is loud (no silent identity render).
    let mut no_analysis = upright_base_args(input.clone());
    no_analysis.clear = true;
    upright(no_analysis.clone()).unwrap();
    no_analysis.clear = false;
    no_analysis.enable = true;
    let error = upright(no_analysis).unwrap_err().to_string();
    assert!(error.contains("no persisted upright analysis"), "{error}");

    // Clear removes the stage entirely; original untouched.
    let mut clear = upright_base_args(input.clone());
    clear.clear = true;
    upright(clear).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(document.virtual_copies[0].recipe.upright.is_none());
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// LRPAR-G06-UPRIGHT-15: `--list` is read-only; contradictory flags are loud.
#[test]
fn upright_rejects_contradictory_flags() {
    let mut args = upright_base_args(PathBuf::from("unused.png"));
    args.list = true;
    args.analyze = true;
    assert!(upright(args)
        .unwrap_err()
        .to_string()
        .contains("--list is read-only"));
    let mut args = upright_base_args(PathBuf::from("unused.png"));
    args.enable = true;
    args.disable = true;
    assert!(upright(args)
        .unwrap_err()
        .to_string()
        .contains("mutually exclusive"));
    let mut args = upright_base_args(PathBuf::from("unused.png"));
    args.analyze = true;
    args.clear = true;
    assert!(upright(args)
        .unwrap_err()
        .to_string()
        .contains("mutually exclusive"));
}
