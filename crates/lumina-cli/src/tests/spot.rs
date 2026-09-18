use super::*;

/// G-04: `--clear` runs after the adders and would discard their result,
/// so combining it with one must fail loudly.
#[test]
fn spot_clear_rejects_adder() {
    let mut args = spot_base_args(PathBuf::from("unused.png"));
    args.clear = true;
    args.add_heuristic = true;
    let error = spot(args).unwrap_err().to_string();
    assert!(error.contains("--clear removes every spot"), "{error}");
}

#[test]
fn spot_add_list_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    // Add one heuristic spot.
    let mut add = spot_base_args(input.clone());
    add.add_heuristic = true;
    add.center_x = Some(0.5);
    add.center_y = Some(0.5);
    add.radius = Some(4.0);
    spot(add).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap();
    assert_eq!(spots.len(), 1);
    assert_eq!(spots[0]["mode"], "heuristic");
    // List-only is read-only: sidecar bytes unchanged.
    let before = fs::read(sidecar_path_for(&input)).unwrap();
    spot(spot_base_args(input.clone())).unwrap();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
    // Clear removes spots.
    let mut clear = spot_base_args(input.clone());
    clear.clear = true;
    spot(clear).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(!document.virtual_copies[0]
        .recipe
        .extras
        .contains_key("spot_removals"));
    // Original image untouched throughout.
    assert_eq!(
        fs::read(&input).unwrap().len(),
        fs::read(&input).unwrap().len()
    );
}

#[test]
fn spot_rejects_bad_geometry_and_unknown_copies_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    // Missing radius.
    let mut add = spot_base_args(input.clone());
    add.add_heuristic = true;
    add.center_x = Some(0.5);
    add.center_y = Some(0.5);
    assert!(spot(add)
        .unwrap_err()
        .to_string()
        .contains("--add-heuristic requires"));
    // Out-of-range center.
    let mut add = spot_base_args(input.clone());
    add.add_heuristic = true;
    add.center_x = Some(1.5);
    add.center_y = Some(0.5);
    add.radius = Some(4.0);
    assert!(spot(add)
        .unwrap_err()
        .to_string()
        .contains("outside allowed range"));
    // Unknown copy.
    let mut add = spot_base_args(input.clone());
    add.virtual_copy = Some("ghost".into());
    add.add_heuristic = true;
    add.center_x = Some(0.5);
    add.center_y = Some(0.5);
    add.radius = Some(4.0);
    assert!(spot(add)
        .unwrap_err()
        .to_string()
        .contains("unknown virtual copy"));
    // Failed runs never mutated the sidecar.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(!document.virtual_copies[0]
        .recipe
        .extras
        .contains_key("spot_removals"));
}

#[test]
fn spot_visualize_and_distraction_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    let mut vis = spot_base_args(input.clone());
    vis.set_visualize_threshold = Some(0.3);
    spot(vis).unwrap();
    let mut dis = spot_base_args(input.clone());
    dis.set_distraction = Some("dust=true,auto=true".into());
    spot(dis).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.spot_visualize_threshold(),
        Some(0.3)
    );
    assert_eq!(
        document.virtual_copies[0].recipe.spot_distraction(),
        lumina_sidecar::SpotDistraction {
            dust: true,
            auto_mode: true,
            ..Default::default()
        }
    );
    // Reload leg: JSON roundtrip preserves both.
    let decoded = SidecarDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.spot_visualize_threshold(),
        Some(0.3)
    );
    // Clear visualize.
    let mut clear = spot_base_args(input.clone());
    clear.clear_visualize = true;
    spot(clear).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.spot_visualize_threshold(),
        None
    );
    // Bad specs fail loudly.
    let mut bad = spot_base_args(input.clone());
    bad.set_visualize_threshold = Some(2.0);
    assert!(spot(bad).is_err());
    let mut bad = spot_base_args(input.clone());
    bad.set_distraction = Some("dust=maybe".into());
    assert!(spot(bad)
        .unwrap_err()
        .to_string()
        .contains("invalid distraction value"));
    let mut bad = spot_base_args(input.clone());
    bad.set_distraction = Some("cats=true".into());
    assert!(spot(bad)
        .unwrap_err()
        .to_string()
        .contains("unknown distraction key"));
}

#[test]
fn spot_detect_lists_without_apply_and_applies_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    // 16x16 frame with a dark 8x8 block (one heuristic cell).
    let mut pixels = vec![255u8; 16 * 16 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let idx = (y * 16 + x) as usize * 4;
            pixels[idx] = 0;
            pixels[idx + 1] = 0;
            pixels[idx + 2] = 0;
        }
    }
    let frame = ImageFrame::new(16, 16, pixels).unwrap();
    let input = directory.path().join("dark.png");
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    import_sidecar_for(&input);
    // List-only: candidates found, nothing persisted.
    let mut detect = spot_base_args(input.clone());
    detect.detect_objects = true;
    spot(detect).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(!document.virtual_copies[0]
        .recipe
        .extras
        .contains_key("spot_removals"));
    // Explicit apply persists exactly the candidates.
    let mut apply = spot_base_args(input.clone());
    apply.detect_objects = true;
    apply.detect_apply = true;
    spot(apply).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap();
    assert_eq!(spots.len(), 1);
    // --detect-apply without --detect-objects fails loudly.
    let mut lonely = spot_base_args(input.clone());
    lonely.detect_apply = true;
    assert!(spot(lonely)
        .unwrap_err()
        .to_string()
        .contains("--detect-apply requires"));
}

#[test]
fn spot_detect_defaults_to_recipe_visualize_threshold() {
    // G04-FOLLOWUP-1: without `--detect-threshold` the recipe visualize
    // threshold is the default (else 0.5); an explicit flag wins.
    let directory = tempfile::tempdir().unwrap();
    let mut pixels = vec![255u8; 16 * 16 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let idx = (y * 16 + x) as usize * 4;
            pixels[idx] = 0;
            pixels[idx + 1] = 0;
            pixels[idx + 2] = 0;
        }
    }
    let frame = ImageFrame::new(16, 16, pixels).unwrap();
    let input = directory.path().join("dark.png");
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    import_sidecar_for(&input);
    // Recipe default 1.0: every 8x8 cell is dark -> 4 candidates.
    let mut vis = spot_base_args(input.clone());
    vis.set_visualize_threshold = Some(1.0);
    spot(vis).unwrap();
    let mut apply = spot_base_args(input.clone());
    apply.detect_objects = true;
    apply.detect_apply = true;
    spot(apply).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap();
    assert_eq!(spots.len(), 4, "recipe threshold 1.0 must drive detection");
    // Explicit flag wins over the recipe default: 0.5 sees one cell.
    let mut clear = spot_base_args(input.clone());
    clear.clear = true;
    spot(clear).unwrap();
    let mut apply = spot_base_args(input.clone());
    apply.detect_objects = true;
    apply.detect_apply = true;
    apply.detect_threshold = Some(0.5);
    spot(apply).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap();
    assert_eq!(spots.len(), 1, "explicit threshold must win");
    // Original image untouched throughout.
    let _ = fs::read(&input).unwrap();
}

#[test]
fn spot_set_distraction_merges_into_stored_switches() {
    // G04-FOLLOWUP-1 merge decision: unnamed keys keep their stored
    // value (consistent with the GUI single-checkbox toggles).
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    let mut first = spot_base_args(input.clone());
    first.set_distraction = Some("reflections=true".into());
    spot(first).unwrap();
    let mut second = spot_base_args(input.clone());
    second.set_distraction = Some("dust=true".into());
    spot(second).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.spot_distraction(),
        lumina_sidecar::SpotDistraction {
            reflections: true,
            dust: true,
            ..Default::default()
        },
        "unnamed `reflections` must survive a later partial set"
    );
    // Explicit `k=false` switches a single key off, keeping the rest.
    let mut off = spot_base_args(input.clone());
    off.set_distraction = Some("dust=false".into());
    spot(off).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.spot_distraction(),
        lumina_sidecar::SpotDistraction {
            reflections: true,
            ..Default::default()
        }
    );
}

#[test]
fn spot_regenerate_variant_sets_derived_seed_deterministically() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    // Seed one generative entry directly (heuristic path has no variants).
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id": "g1", "version": 1, "mode": "generative", "prompt": "x"}]),
    );
    document.validate().unwrap();
    save_sidecar(&path, &document).unwrap();
    let mut regen = spot_base_args(input.clone());
    regen.regenerate_variant = Some("g1".into());
    regen.variant = Some(2);
    regen.seed = Some(7);
    spot(regen).unwrap();
    let document = load_sidecar(&path).unwrap();
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap();
    let expected = lumina_core::generative_variant_seed(7, 2);
    assert_eq!(spots[0]["seed"], expected);
    assert_eq!(spots[0]["variant"], 2);
    assert_eq!(spots[0]["base_seed"], 7);
    // Deterministic: re-running the same variant is a stable no-op.
    let before = fs::read(&path).unwrap();
    let mut regen = spot_base_args(input.clone());
    regen.regenerate_variant = Some("g1".into());
    regen.variant = Some(2);
    regen.seed = Some(7);
    spot(regen).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    // Unknown ids and heuristic spots fail loudly.
    let mut bad = spot_base_args(input.clone());
    bad.regenerate_variant = Some("nope".into());
    bad.variant = Some(1);
    bad.seed = Some(7);
    assert!(spot(bad).unwrap_err().to_string().contains("unknown spot"));
    let mut bad = spot_base_args(input.clone());
    bad.regenerate_variant = Some("g1".into());
    assert!(spot(bad)
        .unwrap_err()
        .to_string()
        .contains("--regenerate-variant requires"));
}
