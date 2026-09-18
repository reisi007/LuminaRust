use super::*;

/// G-14: explicit marking (`--set`) → `--remove` → `--clear` roundtrip,
/// stable ids (replace, never duplicate), one history entry per call, and a
/// loud rejection of out-of-range/invalid region specs. Original untouched.
#[test]
fn red_eye_set_remove_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    let original_bytes = fs::read(&input).unwrap();
    import_sidecar_for(&input);
    let sidecar_path = sidecar_path_for(&input);

    // Read-only list before marking.
    red_eye(red_eye_base_args(input.clone())).unwrap();
    assert!(load_sidecar(&sidecar_path).unwrap().virtual_copies[0]
        .recipe
        .red_eye
        .is_none());

    // Mark two regions.
    let mut set = red_eye_base_args(input.clone());
    set.set = vec![
        "re-1:0.25,0.35,0.05,0.8,0.4".into(),
        "re-2:0.6,0.4,0.03,1.0,0.5".into(),
    ];
    red_eye(set).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .unwrap()
        .regions;
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].id, "re-1");
    assert_eq!(regions[0].desaturate, 0.8);
    assert_eq!(regions[0].darken, 0.4);
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    assert!(document.virtual_copies[0].history[0]
        .id
        .starts_with("red-eye-"));

    // Re-marking an existing id replaces it (stable identity, no growth).
    let mut replace = red_eye_base_args(input.clone());
    replace.set = vec!["re-1:0.3,0.3,0.04,0.5,0.5".into()];
    red_eye(replace).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .unwrap()
        .regions;
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].id, "re-1");
    assert_eq!(regions[0].radius, 0.04);

    // Remove by id; unknown ids are loud and write nothing.
    let mut remove = red_eye_base_args(input.clone());
    remove.remove = vec!["re-2".into()];
    red_eye(remove).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .red_eye
            .as_ref()
            .unwrap()
            .regions
            .len(),
        1
    );
    let before = fs::read(&sidecar_path).unwrap();
    let mut unknown = red_eye_base_args(input.clone());
    unknown.remove = vec!["re-missing".into()];
    assert!(red_eye(unknown)
        .unwrap_err()
        .to_string()
        .contains("unknown red-eye region"));
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);

    // Clear back to identity.
    let mut clear = red_eye_base_args(input.clone());
    clear.clear = true;
    red_eye(clear).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(document.virtual_copies[0].recipe.red_eye.is_none());
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// G-14: every malformed/out-of-range region spec is rejected loudly.
#[test]
fn red_eye_region_specs_are_validated_loudly() {
    for (spec, needle) in [
        ("no-separator", "expected `ID:"),
        (":0.1,0.1,0.1,1,1", "must not be empty"),
        ("re-1:0.1,0.1,0.1,1", "expected 5 values"),
        ("re-1:1.5,0.1,0.1,1,1", "x"),
        ("re-1:0.1,-1.0,0.1,1,1", "y"),
        ("re-1:0.1,0.1,0.0,1,1", "radius"),
        ("re-1:0.1,0.1,1.5,1,1", "radius"),
        ("re-1:0.1,0.1,0.1,1.5,1", "desaturate"),
        ("re-1:0.1,0.1,0.1,1,-0.2", "darken"),
        ("re-1:a,0.1,0.1,1,1", "x"),
    ] {
        let error = parse_red_eye_region(spec).unwrap_err().to_string();
        assert!(
            error.contains(needle),
            "spec `{spec}` → `{error}` (expected `{needle}`)"
        );
    }
    assert!(parse_red_eye_region("re-1:0.25,0.35,0.05,0.8,0.4").is_ok());
}

/// LRPAR-G14-REDEYE-AUTO-15: `--detect` is read-only, `--detect-apply`
/// persists `auto-re-` regions idempotently, replaces only auto regions and
/// leaves manually marked ones untouched. The original stays byte-identical.
#[test]
fn red_eye_detect_lists_readonly_and_applies_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("pupil.png");
    fs::write(&input, red_pupil_png(64, 64, &[(30, 30, 36, 36)])).unwrap();
    let original_bytes = fs::read(&input).unwrap();
    import_sidecar_for(&input);
    let sidecar_path = sidecar_path_for(&input);

    // `--detect` alone is read-only: sidecar byte-identical, stage absent.
    let before = fs::read(&sidecar_path).unwrap();
    let mut detect = red_eye_base_args(input.clone());
    detect.detect = true;
    red_eye(detect).unwrap();
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
    assert!(load_sidecar(&sidecar_path).unwrap().virtual_copies[0]
        .recipe
        .red_eye
        .is_none());

    // `--detect-apply` persists exactly the listed candidates.
    let mut apply = red_eye_base_args(input.clone());
    apply.detect = true;
    apply.detect_apply = true;
    red_eye(apply).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .unwrap()
        .regions;
    assert_eq!(regions.len(), 1, "{regions:?}");
    assert!(regions[0].id.starts_with(RED_EYE_DETECT_ID_PREFIX));
    assert_eq!(regions[0].desaturate, 0.8);
    assert_eq!(regions[0].darken, 0.4);
    assert!(regions[0].x > 0.4 && regions[0].x < 0.6, "{regions:?}");
    assert!(regions[0].y > 0.4 && regions[0].y < 0.6, "{regions:?}");
    assert_eq!(document.virtual_copies[0].history.len(), 1);

    // Re-running is state-idempotent: no duplicate and no new history step.
    let mut again = red_eye_base_args(input.clone());
    again.detect = true;
    again.detect_apply = true;
    red_eye(again).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .red_eye
            .as_ref()
            .unwrap()
            .regions
            .len(),
        1
    );
    assert_eq!(document.virtual_copies[0].history.len(), 1);

    // Manual regions survive a re-detection (only `auto-re-` is replaced).
    let mut manual = red_eye_base_args(input.clone());
    manual.set = vec!["re-1:0.1,0.1,0.05,0.8,0.4".into()];
    red_eye(manual).unwrap();
    let mut apply_again = red_eye_base_args(input.clone());
    apply_again.detect = true;
    apply_again.detect_apply = true;
    red_eye(apply_again).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .unwrap()
        .regions;
    assert_eq!(regions.len(), 2);
    assert!(regions.iter().any(|region| region.id == "re-1"));
    assert!(regions
        .iter()
        .any(|region| region.id.starts_with(RED_EYE_DETECT_ID_PREFIX)));
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// LRPAR-G14-REDEYE-AUTO-15: detection never runs implicitly, finds no
/// stage when there are no red pupils, requires `--detect` for
/// `--detect-apply` and refuses a cap overflow loudly without writing.
#[test]
fn red_eye_detect_requires_detect_for_apply_and_never_prefills() {
    let directory = tempfile::tempdir().unwrap();
    // No red pupils: detection finds nothing and never creates a stage.
    let grey = directory.path().join("grey.png");
    fs::write(&grey, red_pupil_png(64, 64, &[])).unwrap();
    import_sidecar_for(&grey);
    let sidecar_path = sidecar_path_for(&grey);
    let before = fs::read(&sidecar_path).unwrap();
    let mut apply = red_eye_base_args(grey.clone());
    apply.detect = true;
    apply.detect_apply = true;
    red_eye(apply).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(document.virtual_copies[0].recipe.red_eye.is_none());
    // Nothing changed, so not even a history step is written.
    assert!(document.virtual_copies[0].history.is_empty());
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);

    // `--detect-apply` without `--detect` is loudly refused, writes nothing.
    let mut lonely = red_eye_base_args(grey.clone());
    lonely.detect_apply = true;
    assert!(red_eye(lonely)
        .unwrap_err()
        .to_string()
        .contains("requires --detect"));
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);

    // Cap: 32 manual regions + one detection would exceed the limit →
    // loud refusal, no silent truncation, nothing written.
    let pupil = directory.path().join("pupil.png");
    fs::write(&pupil, red_pupil_png(64, 64, &[(30, 30, 36, 36)])).unwrap();
    import_sidecar_for(&pupil);
    let specs: Vec<String> = (0..RED_EYE_MAX_REGIONS)
        .map(|index| format!("re-{index}:0.5,0.5,0.02,0.8,0.4"))
        .collect();
    let mut fill = red_eye_base_args(pupil.clone());
    fill.set = specs;
    red_eye(fill).unwrap();
    let sidecar_path = sidecar_path_for(&pupil);
    let before = fs::read(&sidecar_path).unwrap();
    let mut overflow = red_eye_base_args(pupil.clone());
    overflow.detect = true;
    overflow.detect_apply = true;
    let error = red_eye(overflow).unwrap_err().to_string();
    assert!(error.contains("exceed the"), "{error}");
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
}
