use super::*;

/// G-06: `--list` is the read-only view; combined with a mutation flag it
/// must fail loudly instead of being ignored.
#[test]
fn geometry_list_rejects_mutation() {
    let mut args = geometry_base_args(PathBuf::from("unused.png"));
    args.list = true;
    args.set_rotation = Some(10.0);
    let error = geometry(args).unwrap_err().to_string();
    assert!(error.contains("--list is read-only"), "{error}");
}

/// G-06: set crop/straighten/mirror/lens/perspective, list (read-only),
/// clear — with sidecar roundtrip, exactly one history entry per
/// mutating call, and an untouched original.
#[test]
fn geometry_set_list_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    let original_bytes = fs::read(&input).unwrap();
    import_sidecar_for(&input);
    // Set every stage in one run.
    let mut set = geometry_base_args(input.clone());
    set.set_crop_aspect = Some("16:9".into());
    set.straighten = Some(2.5);
    set.set_mirror = Some("h".into());
    set.set_lens_profile = Some("wide-light".into());
    set.set_lens = vec!["distortion_k1:0.1".into(), "ca_red:0.01".into()];
    set.set_perspective = vec!["vertical:0.2".into(), "scale:1.1".into()];
    geometry(set).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    assert!(matches!(
        recipe.geometry.as_ref().and_then(|g| g.crop.as_ref()),
        Some(Crop::Aspect {
            preset: AspectPreset::SixteenToNine
        })
    ));
    assert_eq!(
        recipe.geometry.as_ref().map(|g| g.rotation_degrees),
        Some(2.5)
    );
    assert_eq!(
        (
            recipe.geometry.as_ref().map(|g| g.mirror_horizontal),
            recipe.geometry.as_ref().map(|g| g.mirror_vertical)
        ),
        (Some(true), Some(false))
    );
    let lens = recipe.lens_correction.as_ref().unwrap();
    assert_eq!(lens.profile.as_deref(), Some("wide-light"));
    assert_eq!(lens.distortion_k1, Some(0.1));
    assert_eq!(lens.ca_red, Some(0.01));
    let perspective = recipe.perspective.as_ref().unwrap();
    assert_eq!(perspective.vertical, 0.2);
    assert_eq!(perspective.scale, 1.1);
    // Exactly one history entry for the mutating call (G-06 step rule).
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    let entry = &document.virtual_copies[0].history[0];
    assert!(entry.id.starts_with("geometry-"), "got {}", entry.id);
    assert_eq!(entry.recipe, *recipe);
    // A second mutating call appends a second, uniquely-id'd entry.
    let mut second = geometry_base_args(input.clone());
    second.set_rotation = Some(-15.0);
    geometry(second).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.virtual_copies[0].history.len(), 2);
    assert_ne!(
        document.virtual_copies[0].history[0].id,
        document.virtual_copies[0].history[1].id
    );
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .map(|g| g.rotation_degrees),
        Some(-15.0)
    );
    // List-only mode is read-only: no new entry, bytes unchanged.
    let before = fs::read(sidecar_path_for(&input)).unwrap();
    let list = geometry_base_args(input.clone());
    geometry(list).unwrap();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
    // Clear everything back to identity.
    let mut clear = geometry_base_args(input.clone());
    clear.clear_geometry = true;
    clear.clear_lens = true;
    clear.clear_perspective = true;
    geometry(clear).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    assert!(recipe.geometry.is_none());
    assert!(recipe.lens_correction.is_none());
    assert!(recipe.perspective.is_none());
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// G-06: free-crop rects and the straighten alias round-trip; `--list`
/// reports the stored stages.
#[test]
fn geometry_free_crop_and_straighten_alias_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 90);
    import_sidecar_for(&input);
    let mut set = geometry_base_args(input.clone());
    set.set_crop_free = Some("0.1,0.2,0.5,0.5".into());
    set.set_rotation = Some(45.0);
    geometry(set).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(matches!(
        document.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .and_then(|g| g.crop.as_ref()),
        Some(Crop::Free { .. })
    ));
    // `--straighten` commits the same field as `--set-rotation`.
    let mut straight = geometry_base_args(input.clone());
    straight.straighten = Some(-3.0);
    geometry(straight).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .map(|g| g.rotation_degrees),
        Some(-3.0)
    );
}

/// G-06: unknown presets/fields/words and out-of-range values abort
/// loudly (exit 1) without touching the sidecar.
#[test]
fn geometry_rejects_invalid_values_without_touching_the_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 60);
    import_sidecar_for(&input);
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read_to_string(&sidecar_path).unwrap();
    // Unknown aspect preset.
    let mut bad = geometry_base_args(input.clone());
    bad.set_crop_aspect = Some("21:9".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Malformed free rect.
    let mut bad = geometry_base_args(input.clone());
    bad.set_crop_free = Some("0.1,0.2".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Out-of-range free rect (rejected on save, not clipped).
    let mut bad = geometry_base_args(input.clone());
    bad.set_crop_free = Some("0.0,0.0,2.0,1.0".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Out-of-range rotation (rejected on save, not clipped).
    let mut bad = geometry_base_args(input.clone());
    bad.set_rotation = Some(270.0);
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Non-finite straighten.
    let mut bad = geometry_base_args(input.clone());
    bad.straighten = Some(f64::NAN);
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Unknown mirror word.
    let mut bad = geometry_base_args(input.clone());
    bad.set_mirror = Some("diagonal".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Unknown lens profile (rejected on save).
    let mut bad = geometry_base_args(input.clone());
    bad.set_lens_profile = Some("fisheye-extreme".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Unknown lens field / non-number / out-of-range coefficient.
    let mut bad = geometry_base_args(input.clone());
    bad.set_lens = vec!["distortion_k9:0.1".into()];
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.set_lens = vec!["ca_red:much".into()];
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.set_lens = vec!["ca_red:0.5".into()];
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Unknown perspective field / out-of-range value.
    let mut bad = geometry_base_args(input.clone());
    bad.set_perspective = vec!["tilt:0.5".into()];
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.set_perspective = vec!["scale:99.0".into()];
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Mutually exclusive flags.
    let mut bad = geometry_base_args(input.clone());
    bad.set_rotation = Some(10.0);
    bad.straighten = Some(10.0);
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.set_crop_aspect = Some("1:1".into());
    bad.set_crop_free = Some("0,0,1,1".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.clear_crop = true;
    bad.set_crop_aspect = Some("1:1".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.clear_lens = true;
    bad.set_lens = vec!["distortion_k1:0.1".into()];
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.clear_geometry = true;
    bad.set_mirror = Some("h".into());
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    let mut bad = geometry_base_args(input.clone());
    bad.set_rotation = Some(10.0);
    bad.lensfun_status = true;
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // Unknown virtual copy.
    let mut bad = geometry_base_args(input.clone());
    bad.virtual_copy = Some("no-such-copy".into());
    bad.set_rotation = Some(10.0);
    assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
    // The loud failures above must not touch the sidecar.
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), before);
}

/// G-06: `--lensfun-status` is read-only (no save, no history entry)
/// and reports a usable status line for raster inputs without EXIF.
#[test]
fn geometry_lensfun_status_is_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 70);
    import_sidecar_for(&input);
    let before = fs::read(sidecar_path_for(&input)).unwrap();
    let mut status = geometry_base_args(input.clone());
    status.lensfun_status = true;
    geometry(status).unwrap();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].history.is_empty());
    // The resolver itself names the fallback loudly: without the
    // `lensfun` feature the missing capability, otherwise the
    // missing-EXIF/manual reason.
    let report = resolve_lensfun_report(&input);
    assert!(
        report.contains("manual model") || report.contains("unavailable"),
        "raster input must report the manual fallback or the missing capability, got `{report}`"
    );
}
