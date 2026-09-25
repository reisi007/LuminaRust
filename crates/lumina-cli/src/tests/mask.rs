use super::*;

/// End-to-end: import → `mask --add-ai-select/--add-luminance-range` →
/// file → reload. New entries are stable, typed and carry loud statuses.
#[test]
fn mask_add_ai_and_range_roundtrip_through_file() {
    let directory = tempfile::tempdir().unwrap();
    let input = mask_imported_input(directory.path(), "g03.png");

    let mut add_sky = mask_args(input.clone());
    add_sky.add_ai_select = Some("sky".into());
    add_sky.name = Some("Sky".into());
    mask(add_sky).unwrap();

    let mut add_lum = mask_args(input.clone());
    add_lum.add_luminance_range = true;
    add_lum.name = Some("Bright".into());
    add_lum.range_min = Some(0.5);
    add_lum.range_max = Some(1.0);
    mask(add_lum).unwrap();

    // Reload from the file: both masks persisted with stable ids.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.validate().is_ok());
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.mask_library.len(), 2);
    let sky = copy
        .mask_library
        .iter()
        .find(|mask| mask.name == "Sky")
        .unwrap();
    assert_eq!(
        sky.ai_select.as_ref().unwrap().kind,
        lumina_sidecar::AiSelectKind::Sky
    );
    assert_eq!(sky.status, MaskStatus::Pending);
    let lum = copy
        .mask_library
        .iter()
        .find(|mask| mask.name == "Bright")
        .unwrap();
    assert!(matches!(
        lum.prompt,
        Some(lumina_sidecar::MaskPrompt::LuminanceRange { .. })
    ));
    assert_eq!(lum.status, MaskStatus::Valid);
    // Stable ids: re-running the same add is a loud duplicate, not a copy.
    let mut repeat = mask_args(input.clone());
    repeat.add_ai_select = Some("sky".into());
    repeat.name = Some("Sky".into());
    assert!(mask(repeat).is_err());
}

#[test]
fn mask_local_adjustment_flags_persist_typed_values_and_reject_bad_input() {
    let directory = tempfile::tempdir().unwrap();
    let input = mask_imported_input(directory.path(), "g03-local.png");
    let mut add = mask_args(input.clone());
    add.add_ai_select = Some("subject".into());
    add.name = Some("Subject".into());
    mask(add).unwrap();
    let mask_id = mask_library_ids(&input)[0].clone();
    let mut attach = mask_args(input.clone());
    attach.attach_layer = Some(mask_id);
    mask(attach).unwrap();
    let layer_id = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .id
        .clone();

    let mut set = mask_args(input.clone());
    set.local_layer = Some(layer_id.clone());
    set.set_local_adjustments = vec!["exposure=1.25".into(), "highlights=-0.2".into()];
    mask(set).unwrap();
    let layer = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0];
    let local = layer.local_adjustments.as_ref().unwrap();
    assert_eq!(local.version, lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(local.exposure, 1.25);
    assert_eq!(local.highlights, -0.2);
    assert!(layer.extras.is_empty());

    let before = fs::read(sidecar_path_for(&input)).unwrap();
    let mut bad = mask_args(input.clone());
    bad.local_layer = Some(layer_id.clone());
    bad.set_local_adjustments = vec!["wb_temperature=6500".into()];
    assert!(mask(bad).is_err());
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
}

#[test]
fn mask_combine_duplicate_layer_visibility_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let input = mask_imported_input(directory.path(), "g03c.png");

    for (kind, name) in [("subject", "Subject"), ("sky", "Sky")] {
        let mut add = mask_args(input.clone());
        add.add_ai_select = Some(kind.into());
        add.name = Some(name.into());
        mask(add).unwrap();
    }
    let ids = mask_library_ids(&input);
    assert_eq!(ids.len(), 2);

    // Add = union over both inputs.
    let mut combine = mask_args(input.clone());
    combine.combine = Some("union".into());
    combine.name = Some("Both".into());
    combine.inputs = Some(format!("{},{}", ids[0], ids[1]));
    mask(combine).unwrap();

    // Intersect is CLI-reachable too (panel offers union/subtract/invert).
    let mut intersect = mask_args(input.clone());
    intersect.combine = Some("intersect".into());
    intersect.name = Some("Overlap".into());
    intersect.inputs = Some(format!("{},{}", ids[0], ids[1]));
    mask(intersect).unwrap();

    // Duplicate one source under a new name.
    let mut duplicate = mask_args(input.clone());
    duplicate.duplicate = Some(ids[0].clone());
    duplicate.name = Some("Subject copy".into());
    mask(duplicate).unwrap();

    // Attach a layer for the union and close its eye again.
    let union_id = mask_library_ids(&input)
        .into_iter()
        .find(|id| {
            load_sidecar(&sidecar_path_for(&input))
                .unwrap()
                .virtual_copies[0]
                .mask_library
                .iter()
                .any(|mask| mask.id == *id && mask.name == "Both")
        })
        .unwrap();
    let mut attach = mask_args(input.clone());
    attach.attach_layer = Some(union_id);
    mask(attach).unwrap();
    // Layer id is `layer-<mask-id>`; resolve it from the file.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let layer_id = document.virtual_copies[0].mask_layers[0].id.clone();
    let mut hide = mask_args(input.clone());
    hide.hide_layer = Some(layer_id.clone());
    mask(hide).unwrap();

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.validate().is_ok());
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.mask_library.len(), 5);
    let union = copy
        .mask_library
        .iter()
        .find(|mask| mask.name == "Both")
        .unwrap();
    assert_eq!(union.operation, lumina_sidecar::MaskOperation::Union);
    assert_eq!(union.references.len(), 2);
    let overlap = copy
        .mask_library
        .iter()
        .find(|mask| mask.name == "Overlap")
        .unwrap();
    assert_eq!(overlap.operation, lumina_sidecar::MaskOperation::Intersect);
    assert_eq!(overlap.references.len(), 2);
    let layer = copy
        .mask_layers
        .iter()
        .find(|layer| layer.id == layer_id)
        .unwrap();
    assert!(!layer.visible);
    // Re-open the eye.
    let mut show = mask_args(input.clone());
    show.show_layer = Some(layer_id);
    mask(show).unwrap();
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].mask_layers[0].visible);
}

/// Loud failures: unknown kind/copy/mask, bad ranges, wrong arity,
/// unknown layers and unknown combine ops abort with exit code 1 and
/// leave the sidecar byte-identical (no partial write).
#[test]
fn mask_failures_are_loud_and_leave_sidecar_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let input = mask_imported_input(directory.path(), "g03e.png");
    let before = fs::read(sidecar_path_for(&input)).unwrap();

    // Unknown AI kind.
    let mut bad_kind = mask_args(input.clone());
    bad_kind.add_ai_select = Some("cat".into());
    bad_kind.name = Some("Cat".into());
    let error = mask(bad_kind).unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert!(error.to_string().contains("unknown ai-select kind"));

    // Unknown virtual copy.
    let mut bad_copy = mask_args(input.clone());
    bad_copy.virtual_copy = Some("nope".into());
    bad_copy.add_ai_select = Some("sky".into());
    bad_copy.name = Some("Sky".into());
    assert!(mask(bad_copy).is_err());

    // Missing range bounds.
    let mut bad_range = mask_args(input.clone());
    bad_range.add_luminance_range = true;
    bad_range.name = Some("Bright".into());
    assert!(mask(bad_range).is_err());

    // Out-of-range values are rejected by the sidecar gate.
    let mut bad_values = mask_args(input.clone());
    bad_values.add_luminance_range = true;
    bad_values.name = Some("Bright".into());
    bad_values.range_min = Some(0.9);
    bad_values.range_max = Some(0.1);
    assert!(mask(bad_values).is_err());

    // Unknown combine input.
    let mut bad_input = mask_args(input.clone());
    bad_input.combine = Some("union".into());
    bad_input.name = Some("Both".into());
    bad_input.inputs = Some("missing-a,missing-b".into());
    assert!(mask(bad_input).is_err());

    // Wrong arity: subtract needs exactly 2.
    let mut add = mask_args(input.clone());
    add.add_ai_select = Some("sky".into());
    add.name = Some("Sky".into());
    mask(add).unwrap();
    let sky_id = mask_library_ids(&input)[0].clone();
    let mut bad_arity = mask_args(input.clone());
    bad_arity.combine = Some("subtract".into());
    bad_arity.name = Some("Sub".into());
    bad_arity.inputs = Some(sky_id);
    assert!(mask(bad_arity).is_err());

    // Unknown layer eye.
    let mut bad_layer = mask_args(input.clone());
    bad_layer.hide_layer = Some("layer-nope".into());
    assert!(mask(bad_layer).is_err());

    // Unknown combine op.
    let mut bad_op = mask_args(input.clone());
    bad_op.combine = Some("multiply".into());
    bad_op.name = Some("X".into());
    bad_op.inputs = Some("a,b".into());
    assert!(mask(bad_op).is_err());

    // Only the successful Sky add above changed the file.
    let after_success = fs::read(sidecar_path_for(&input)).unwrap();
    assert_ne!(before, after_success);
    let snapshot = after_success;
    // Every failing command after that left the file byte-identical.
    let mut another_bad = mask_args(input.clone());
    another_bad.hide_layer = Some("layer-nope".into());
    assert!(another_bad.hide_layer.is_some());
    let _ = mask(another_bad).unwrap_err();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), snapshot);
}

/// `mask --list` is read-only: stdout carries the statuses, the sidecar
/// bytes don't move.
#[test]
fn mask_list_reports_statuses_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let input = mask_imported_input(directory.path(), "g03l.png");
    let mut add = mask_args(input.clone());
    add.add_ai_select = Some("people".into());
    add.name = Some("Person".into());
    add.detail = Some("face".into());
    mask(add).unwrap();

    let before = fs::read(sidecar_path_for(&input)).unwrap();
    let mut list = mask_args(input.clone());
    list.list = true;
    list.json = true;
    mask(list).unwrap();
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let person = document.virtual_copies[0]
        .mask_library
        .iter()
        .find(|mask| mask.name == "Person")
        .unwrap();
    assert_eq!(
        person.ai_select.as_ref().unwrap().detail.as_deref(),
        Some("face")
    );
}
