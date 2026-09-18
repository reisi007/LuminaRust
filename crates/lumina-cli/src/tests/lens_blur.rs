use super::*;

/// G-05: `--clear` short-circuits the setters, so combining it with one
/// must fail loudly instead of silently dropping the requested edit.
#[test]
fn lens_blur_clear_rejects_companion_mutation() {
    let mut clear = lens_blur_base_args(PathBuf::from("unused.png"));
    clear.clear = true;
    clear.set_amount = Some(0.8);
    let error = lens_blur(clear).unwrap_err().to_string();
    assert!(
        error.contains("--clear removes the whole lens-blur stage"),
        "{error}"
    );
    let mut clear = lens_blur_base_args(PathBuf::from("unused.png"));
    clear.clear = true;
    clear.enable = true;
    assert!(lens_blur(clear).is_err());
}

/// G-05: set fields, list (read-only), clear — with sidecar roundtrip and
/// an untouched original.
#[test]
fn lens_blur_set_list_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    let original_bytes = fs::read(&input).unwrap();
    import_sidecar_for(&input);
    // Set every field in one run.
    let mut set = lens_blur_base_args(input.clone());
    set.set_amount = Some(0.75);
    set.set_focal_near = Some(0.1);
    set.set_focal_far = Some(0.5);
    set.set_bokeh = Some("hexagonal".into());
    set.set_focus_rect = Some("0.2,0.3,0.4,0.25".into());
    lens_blur(set).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert!(blur.enabled);
    assert_eq!(blur.blur_amount, 0.75);
    assert_eq!((blur.focal_near, blur.focal_far), (0.1, 0.5));
    assert_eq!(blur.bokeh, BokehShape::Hexagonal);
    assert_eq!(
        (
            blur.focus_rect.x,
            blur.focus_rect.y,
            blur.focus_rect.width,
            blur.focus_rect.height
        ),
        (0.2, 0.3, 0.4, 0.25)
    );
    // Sidecar JSON carries the stage at the recipe root.
    let raw = fs::read_to_string(sidecar_path_for(&input)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value["virtual_copies"][0]["recipe"]["lens_blur"]["bokeh"],
        "hexagonal"
    );
    // List-only is read-only: sidecar bytes unchanged.
    let before = fs::read(sidecar_path_for(&input)).unwrap();
    lens_blur(lens_blur_base_args(input.clone())).unwrap();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
    // Disable keeps values but reports off.
    let mut disable = lens_blur_base_args(input.clone());
    disable.disable = true;
    lens_blur(disable).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert!(!blur.enabled);
    assert_eq!(blur.blur_amount, 0.75);
    // B1: the --enable success path re-enables while keeping values.
    let mut enable = lens_blur_base_args(input.clone());
    enable.enable = true;
    lens_blur(enable).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert!(blur.enabled);
    assert_eq!(blur.blur_amount, 0.75);
    assert_eq!(
        lumina_core::lens_blur_status(Some(blur), false),
        "heuristic active"
    );
    // B1: set a depth artifact, then clear it — the reference is gone
    // and the status falls back to the heuristic.
    let mut set_depth = lens_blur_base_args(input.clone());
    set_depth.set_depth_artifact = Some("depth/map.bin:sha256:abc".into());
    lens_blur(set_depth).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert_eq!(
        blur.depth_artifact.as_ref().unwrap().relative_path,
        "depth/map.bin"
    );
    assert_eq!(
        lumina_core::lens_blur_status(Some(blur), false),
        "missing depth artifact"
    );
    let mut clear_depth = lens_blur_base_args(input.clone());
    clear_depth.clear_depth_artifact = true;
    lens_blur(clear_depth).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert!(blur.depth_artifact.is_none());
    assert_eq!(
        lumina_core::lens_blur_status(Some(blur), false),
        "heuristic active"
    );
    // Clear removes the stage.
    let mut clear = lens_blur_base_args(input.clone());
    clear.clear = true;
    lens_blur(clear).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].recipe.lens_blur.is_none());
    // Original image untouched throughout.
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// G-05: every invalid lens-blur input fails loudly (exit 1) and never
/// mutates the sidecar — no silent clipping.
#[test]
fn lens_blur_rejects_invalid_values_without_touching_the_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 60);
    import_sidecar_for(&input);
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read_to_string(&sidecar_path).unwrap();

    // Out-of-range amount.
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_amount = Some(2.0);
    let error = lens_blur(bad).unwrap_err();
    assert_eq!(error.exit_code(), 1);
    // Unknown bokeh shape.
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_bokeh = Some("swirly".into());
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // Malformed focus rect.
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_focus_rect = Some("0.1,0.2,oops".into());
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // Out-of-bounds focus rect (rejected on save, not clipped).
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_focus_rect = Some("0.8,0.8,0.5,0.5".into());
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // Inverted focal range.
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_focal_near = Some(0.8);
    bad.set_focal_far = Some(0.2);
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // Malformed depth reference.
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_depth_artifact = Some("no-separator-here".into());
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // Absolute depth path (portable sidecars stay relative).
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_depth_artifact = Some("/abs/depth.bin:sha256:abc".into());
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // Mutually exclusive flags.
    let mut bad = lens_blur_base_args(input.clone());
    bad.enable = true;
    bad.disable = true;
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
    // B1: the --set-depth-artifact + --clear-depth-artifact conflict arm.
    let mut bad = lens_blur_base_args(input.clone());
    bad.set_depth_artifact = Some("depth/map.bin:sha256:abc".into());
    bad.clear_depth_artifact = true;
    assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);

    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), before);
}

/// G-05: a referenced-but-missing depth artifact fails the render loudly
/// (exit 1) instead of silently rendering the heuristic.
#[test]
fn lens_blur_missing_depth_artifact_fails_render_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    let mut set = lens_blur_base_args(input.clone());
    set.set_depth_artifact = Some("depth/map.bin:sha256:abc".into());
    lens_blur(set).unwrap();
    // The reference round-trips and reports `missing`.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert_eq!(
        blur.depth_artifact.as_ref().unwrap().relative_path,
        "depth/map.bin"
    );
    assert_eq!(
        lumina_core::lens_blur_status(Some(blur), false),
        "missing depth artifact"
    );
    // Rendering aborts loudly (exit 1), no output file appears.
    let output = directory.path().join("out.png");
    let mut warnings = Vec::new();
    let error = process_selected(
        ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        },
        90,
        None,
        MaskPolicy::Warn,
        &mut warnings,
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("lens_blur"));
    assert!(!output.exists());
}
