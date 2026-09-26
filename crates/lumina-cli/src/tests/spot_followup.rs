//! R5-DUST-23-FOLLOWUP CLI regressions: reference-preserving status views,
//! single-entry edits, contradiction preflight, and duplicate/unknown IDs.

use super::*;

#[test]
fn spot_list_keeps_null_and_missing_generative_entries_visible() {
    let entries = vec![
        serde_json::json!({"id":"null-ref","version":1,"mode":"generative","artifact":null,"status":"valid"}),
        serde_json::json!({"id":"missing-ref","version":1,"mode":"generative","status":"valid",
            "artifact":{"id":"record-1","relative_path":"missing.lumina.zdata","format":"lumina-zdata",
            "checksum":"blake3:abc","width":1,"height":1,"channels":"rgba8","data_version":"1"}}),
    ];
    // MCP-PARITY-A: the spot mutation ops moved to the shared `lumina-stages`.
    let displayed = lumina_stages::spot_ops::display_spot_entries(&entries, Path::new("."));
    assert_eq!(
        displayed.len(),
        2,
        "status resolution must not drop entries"
    );
    assert_eq!(displayed[0]["id"], "null-ref");
    assert_eq!(displayed[0]["status"], "missing");
    assert_eq!(displayed[1]["id"], "missing-ref");
    assert_eq!(displayed[1]["status"], "missing");
}

/// `--spot-id` + `--set-*` edits one heuristic spot in place and
/// `--remove-spot` deletes exactly one entry; both roundtrip through the
/// sidecar file and leave the original bytes unchanged.
#[test]
fn spot_update_and_remove_single_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    let original_bytes = fs::read(&input).unwrap();
    for (cx, radius) in [(0.25, 2.0), (0.75, 3.0)] {
        let mut add = spot_base_args(input.clone());
        add.add_heuristic = true;
        add.center_x = Some(cx);
        add.center_y = Some(0.5);
        add.radius = Some(radius);
        spot(add).unwrap();
    }
    let read_spots = || -> Vec<serde_json::Value> {
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap()
    };
    assert_eq!(read_spots().len(), 2);
    let first_id = read_spots()[0]["id"].as_str().unwrap().to_string();
    let second_id = read_spots()[1]["id"].as_str().unwrap().to_string();
    let mut update = spot_base_args(input.clone());
    update.spot_id = Some(first_id.clone());
    update.set_radius = Some(5.0);
    update.set_feather = Some(0.4);
    update.set_opacity = Some(0.6);
    update.set_offset_dx = Some(0.2);
    update.set_offset_dy = Some(0.1);
    spot(update).unwrap();
    let spots = read_spots();
    assert_eq!(spots.len(), 2);
    assert_eq!(spots[0]["radius"], 5.0);
    let approx = |spots: &[serde_json::Value], key: &str, want: f64| {
        let got = spots[0][key].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() < 1e-6, "{key}: got {got}, want {want}");
    };
    approx(&spots, "feather", 0.4);
    approx(&spots, "opacity", 0.6);
    approx(&spots, "offset_dx", 0.2);
    approx(&spots, "offset_dy", 0.1);
    assert_eq!(spots[0]["id"].as_str().unwrap(), first_id);
    assert_eq!(spots[0]["mode"], "heuristic");
    assert_eq!(spots[1]["radius"], 3.0, "untouched entry keeps its params");
    let mut remove = spot_base_args(input.clone());
    remove.remove_spot = Some(first_id.clone());
    spot(remove).unwrap();
    let spots = read_spots();
    assert_eq!(spots.len(), 1);
    assert_eq!(spots[0]["id"].as_str().unwrap(), second_id);
    let mut remove = spot_base_args(input.clone());
    remove.remove_spot = Some(second_id);
    spot(remove).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(!document.virtual_copies[0]
        .recipe
        .extras
        .contains_key("spot_removals"));
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// Every remove/list mutation is mutually exclusive. Each failure is loud,
/// exits with code 1, and leaves the sidecar byte-identical.
#[test]
fn spot_update_and_remove_reject_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    import_sidecar_for(&input);
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id": "g1", "version": 1, "mode": "generative", "prompt": "x"}]),
    );
    document.validate().unwrap();
    save_sidecar(&path, &document).unwrap();
    let before_conflicts = fs::read(&path).unwrap();

    let mut add_and_remove = spot_base_args(input.clone());
    add_and_remove.add_heuristic = true;
    add_and_remove.center_x = Some(0.5);
    add_and_remove.center_y = Some(0.5);
    add_and_remove.radius = Some(4.0);
    add_and_remove.remove_spot = Some("g1".into());
    let error = spot(add_and_remove).unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("--remove-spot"));
    assert_eq!(fs::read(&path).unwrap(), before_conflicts);

    let mut update_and_remove = spot_base_args(input.clone());
    update_and_remove.spot_id = Some("g1".into());
    update_and_remove.set_radius = Some(5.0);
    update_and_remove.remove_spot = Some("g1".into());
    let error = spot(update_and_remove).unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("--remove-spot"));
    assert_eq!(fs::read(&path).unwrap(), before_conflicts);

    let mut regenerate_and_remove = spot_base_args(input.clone());
    regenerate_and_remove.regenerate_variant = Some("g1".into());
    regenerate_and_remove.variant = Some(2);
    regenerate_and_remove.seed = Some(7);
    regenerate_and_remove.remove_spot = Some("g1".into());
    let error = spot(regenerate_and_remove).unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("--remove-spot"));
    assert_eq!(fs::read(&path).unwrap(), before_conflicts);

    let mut detect_and_remove = spot_base_args(input.clone());
    detect_and_remove.detect_objects = true;
    detect_and_remove.detect_apply = true;
    detect_and_remove.remove_spot = Some("g1".into());
    let error = spot(detect_and_remove).unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("--remove-spot"));
    assert_eq!(fs::read(&path).unwrap(), before_conflicts);

    let mut bad = spot_base_args(input.clone());
    bad.remove_spot = Some("nope".into());
    assert!(spot(bad).unwrap_err().to_string().contains("unknown spot"));
    let mut bad = spot_base_args(input.clone());
    bad.spot_id = Some("nope".into());
    bad.set_radius = Some(5.0);
    assert!(spot(bad).unwrap_err().to_string().contains("unknown spot"));
    let mut bad = spot_base_args(input.clone());
    bad.spot_id = Some("g1".into());
    bad.set_radius = Some(5.0);
    assert!(spot(bad).unwrap_err().to_string().contains("not heuristic"));
    let mut bad = spot_base_args(input.clone());
    bad.spot_id = Some("g1".into());
    bad.set_opacity = Some(2.0);
    assert!(spot(bad).is_err());
    let mut bad = spot_base_args(input.clone());
    bad.set_radius = Some(5.0);
    assert!(spot(bad).unwrap_err().to_string().contains("--spot-id"));
    let mut bad = spot_base_args(input.clone());
    bad.spot_id = Some("g1".into());
    assert!(spot(bad).unwrap_err().to_string().contains("--set-"));
    let mut bad = spot_base_args(input.clone());
    bad.clear = true;
    bad.remove_spot = Some("g1".into());
    assert!(spot(bad).unwrap_err().to_string().contains("--remove-spot"));
    let mut bad = spot_base_args(input.clone());
    bad.clear = true;
    bad.spot_id = Some("g1".into());
    bad.set_radius = Some(5.0);
    assert!(spot(bad)
        .unwrap_err()
        .to_string()
        .contains("--clear removes every spot"));

    let document = load_sidecar(&path).unwrap();
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(document.virtual_copies[0].recipe.extras["spot_removals"].clone())
            .unwrap();
    assert_eq!(spots.len(), 1);
    assert_eq!(spots[0]["id"], "g1");
}
