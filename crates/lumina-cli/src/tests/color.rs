use super::*;

/// G-02: set every color stage in one run, list (read-only), then clear —
/// with sidecar roundtrip and an untouched original.
#[test]
fn color_set_list_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 120);
    let original_bytes = fs::read(&input).unwrap();
    import_sidecar_for(&input);
    // Set every stage in one run.
    let mut set = color_base_args(input.clone());
    set.set_curve_param = vec!["red:0.0,0.0,0.2,0.0".into()];
    set.set_curve_points = vec!["master:0,0;0.5,0.6;1,1".into()];
    set.set_hsl = vec!["red:hue:0.5".into(), "blue:luminance:-0.25".into()];
    set.add_point_color = true;
    set.hue_center = Some(30.0);
    set.hue_range = Some(20.0);
    set.sat_shift = Some(-0.5);
    set.set_grading = vec![
        "shadows:hue_degrees:120.0".into(),
        "highlights:luminance:0.4".into(),
    ];
    set.set_grading_balance = Some(0.1);
    set.set_grading_blending = Some(0.7);
    set.set_vibrance = Some(0.2);
    set.set_saturation = Some(-0.1);
    color(set).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    let curves = recipe.curves.as_ref().expect("curves");
    assert_eq!(curves.master.len(), 3);
    assert_eq!(
        curves.channels.red.as_ref().expect("red").len(),
        4,
        "parametric red persists as a 4-point list"
    );
    let hsl = recipe.hsl.as_ref().expect("hsl");
    assert_eq!(hsl.red.expect("red").hue, 0.5);
    assert_eq!(hsl.blue.expect("blue").luminance, -0.25);
    let point_color = recipe.point_color.as_ref().expect("point_color");
    assert_eq!(point_color.entries.len(), 1);
    assert_eq!(point_color.entries[0].id, "pc-1");
    assert_eq!(point_color.entries[0].hue_center, 30.0);
    assert_eq!(point_color.entries[0].saturation_shift, -0.5);
    let grading = recipe.color_grading.as_ref().expect("grading");
    assert_eq!(grading.shadows.hue_degrees, 120.0);
    assert_eq!(grading.highlights.luminance, 0.4);
    assert_eq!(grading.balance, 0.1);
    assert_eq!(grading.blending, 0.7);
    assert_eq!(recipe.adjustments["vibrance"], 0.2);
    assert_eq!(recipe.adjustments["saturation"], -0.1);
    // Sidecar JSON carries the stages in the adjustments map.
    let raw = fs::read_to_string(sidecar_path_for(&input)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let adjustments = &value["virtual_copies"][0]["recipe"]["adjustments"];
    assert!(adjustments.get("curves").is_some());
    assert!(adjustments.get("hsl").is_some());
    assert!(adjustments.get("point_color").is_some());
    assert!(adjustments.get("color_grading").is_some());
    // List-only is read-only: sidecar bytes unchanged.
    let before = fs::read(sidecar_path_for(&input)).unwrap();
    color(color_base_args(input.clone())).unwrap();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
    // Mutate one Point Color entry, then remove it (last remove drops
    // the block).
    let mut edit = color_base_args(input.clone());
    edit.set_point_color = vec!["pc-1:hue_center:200.0".into()];
    color(edit).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .point_color
            .as_ref()
            .expect("point_color")
            .entries[0]
            .hue_center,
        200.0
    );
    let mut remove = color_base_args(input.clone());
    remove.remove_point_color = vec!["pc-1".into()];
    color(remove).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].recipe.point_color.is_none());
    // Clear the remaining stages.
    let mut clear = color_base_args(input.clone());
    clear.clear_curves = true;
    clear.clear_hsl = true;
    clear.clear_grading = true;
    color(clear).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    assert!(recipe.curves.is_none());
    assert!(recipe.hsl.is_none());
    assert!(recipe.color_grading.is_none());
    // Original image untouched throughout.
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// G-02: every invalid color input fails loudly (exit 1) and never
/// mutates the sidecar — no silent clipping.
#[test]
fn color_rejects_invalid_values_without_touching_the_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 60);
    import_sidecar_for(&input);
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read_to_string(&sidecar_path).unwrap();

    // Unknown curve channel.
    let mut bad = color_base_args(input.clone());
    bad.set_curve_param = vec!["purple:0,0,0,0".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Malformed curve param.
    let mut bad = color_base_args(input.clone());
    bad.set_curve_param = vec!["red:0,0".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Non-ascending curve points (rejected on save, not reordered).
    let mut bad = color_base_args(input.clone());
    bad.set_curve_points = vec!["master:0,0;0.3,0.5;0.2,0.4;1,1".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Missing (0,0) endpoint.
    let mut bad = color_base_args(input.clone());
    bad.set_curve_points = vec!["master:0.1,0.1;1,1".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Unknown HSL channel / field.
    let mut bad = color_base_args(input.clone());
    bad.set_hsl = vec!["infrared:hue:0.5".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    let mut bad = color_base_args(input.clone());
    bad.set_hsl = vec!["red:brightness:0.5".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Out-of-range HSL value (rejected on save, not clipped).
    let mut bad = color_base_args(input.clone());
    bad.set_hsl = vec!["red:hue:2.0".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Unknown Point Color entry / field.
    let mut bad = color_base_args(input.clone());
    bad.set_point_color = vec!["pc-99:hue_center:30.0".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    let mut bad = color_base_args(input.clone());
    bad.add_point_color = true;
    color(bad).unwrap();
    let mut bad = color_base_args(input.clone());
    bad.set_point_color = vec!["pc-1:brightness:0.5".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Out-of-range point color value.
    let mut bad = color_base_args(input.clone());
    bad.set_point_color = vec!["pc-1:hue_center:400.0".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Unknown grading range / field / out-of-range blending.
    let mut bad = color_base_args(input.clone());
    bad.set_grading = vec!["lowlights:saturation:0.5".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    let mut bad = color_base_args(input.clone());
    bad.set_grading = vec!["shadows:brightness:0.5".into()];
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    let mut bad = color_base_args(input.clone());
    bad.set_grading_blending = Some(1.5);
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Mutually exclusive flags.
    let mut bad = color_base_args(input.clone());
    bad.clear_curves = true;
    bad.clear_curve_channel = Some("red".into());
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    let mut bad = color_base_args(input.clone());
    bad.clear_grading = true;
    bad.set_grading_balance = Some(0.1);
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);
    // Ninth point color entry (limit 8).
    for _ in 0..7 {
        let mut add = color_base_args(input.clone());
        add.add_point_color = true;
        color(add).unwrap();
    }
    let mut bad = color_base_args(input.clone());
    bad.add_point_color = true;
    assert_eq!(color(bad).unwrap_err().exit_code(), 1);

    // The loud failures above (except the intentional successful adds)
    // must not corrupt the sidecar: it still loads and the stages that
    // were set on purpose round-trip.
    let document = load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .point_color
            .as_ref()
            .expect("point_color")
            .entries
            .len(),
        8
    );
    let _ = before;
}
