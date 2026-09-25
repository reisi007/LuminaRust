//! LRPAR-G03-MASKGROUP-03: mask-group schema + pure operation tests
//! (grouping, dissolve, reorder, offsets, source-deletion materialization,
//! loud validation, additive extras roundtrip without a `schema_version` bump).

use super::*;

fn reference(copy_id: &str, mask_id: &str) -> MaskReference {
    MaskReference {
        copy_id: copy_id.into(),
        mask_id: mask_id.into(),
        extras: Extras::new(),
    }
}

fn layer(copy_id: &str, mask_id: &str) -> MaskLayer {
    MaskLayer {
        id: format!("layer-{mask_id}"),
        mask: reference(copy_id, mask_id),
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: None,
        extras: Extras::new(),
    }
}

fn copy_with(ids: &[&str]) -> VirtualCopy {
    let mut copy = SidecarDocument::new(source(), "p").virtual_copies[0].clone();
    copy.mask_library = ids.iter().map(|id| mask(id)).collect();
    copy
}

#[test]
fn group_new_rejects_empty_duplicate_and_bad_ids() {
    assert!(MaskGroup::new("g", "G", vec![]).is_err());
    assert!(MaskGroup::new("g", "G", vec![reference("vc", "a"), reference("vc", "a")]).is_err());
    assert!(MaskGroup::new("", "G", vec![reference("vc", "a")]).is_err());
    assert!(MaskGroup::new(" g ", "G", vec![reference("vc", "a")]).is_err());
    assert!(MaskGroup::new("g", "  ", vec![reference("vc", "a")]).is_err());
    let group = MaskGroup::new("g", "G", vec![reference("vc", "a")]).unwrap();
    assert_eq!(group.version, MASK_GROUP_VERSION);
    assert!(!group.collapsed);
    assert!(group.contains("a"));
}

#[test]
fn group_id_is_deterministic_and_order_independent() {
    let a = MaskGroup::group_id_for_members("vc", &[reference("vc", "b"), reference("vc", "a")]);
    let b = MaskGroup::group_id_for_members("vc", &[reference("vc", "a"), reference("vc", "b")]);
    let c = MaskGroup::group_id_for_members("vc", &[reference("vc", "a")]);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a.starts_with("mask-group-"));
    validate_group_id(&a).unwrap();
}

#[test]
fn group_masks_requires_existing_unowned_members() {
    let mut copy = copy_with(&["a", "b"]);
    let group = group_masks(&mut copy, "g", "G", &["a".into()]).unwrap();
    assert_eq!(group.members, vec![reference(&copy.id, "a")]);
    // A node already in a group cannot join a second one.
    assert!(group_masks(&mut copy, "h", "H", &["a".into()]).is_err());
    // Unknown members are loud.
    assert!(group_masks(&mut copy, "h", "H", &["missing".into()]).is_err());
    // Duplicate group id is loud.
    assert!(group_masks(&mut copy, "g", "G2", &["b".into()]).is_err());
    assert_eq!(mask_groups_of(&copy).unwrap().len(), 1);
}

#[test]
fn dissolve_keeps_member_masks() {
    let mut copy = copy_with(&["a", "b"]);
    group_masks(&mut copy, "g", "G", &["a".into(), "b".into()]).unwrap();
    dissolve_group(&mut copy, "g").unwrap();
    assert!(mask_groups_of(&copy).unwrap().is_empty());
    assert_eq!(copy.mask_library.len(), 2);
    assert!(dissolve_group(&mut copy, "g").is_err());
}

#[test]
fn move_member_clamps_and_reorders() {
    let mut copy = copy_with(&["a", "b", "c"]);
    group_masks(&mut copy, "g", "G", &["a".into(), "b".into(), "c".into()]).unwrap();
    move_group_member(&mut copy, "g", "c", -1).unwrap();
    let members: Vec<String> = mask_groups_of(&copy).unwrap()[0]
        .members
        .iter()
        .map(|m| m.mask_id.clone())
        .collect();
    assert_eq!(members, vec!["a", "c", "b"]);
    // Clamp at the top edge is a no-op, not an error.
    move_group_member(&mut copy, "g", "b", 10).unwrap();
    assert!(move_group_member(&mut copy, "g", "missing", 1).is_err());
}

#[test]
fn parameter_offsets_apply_to_member_layers_only_and_clamp() {
    let mut copy = copy_with(&["a", "b", "c"]);
    copy.mask_layers = vec![layer(&copy.id, "a"), layer(&copy.id, "c")];
    copy.mask_layers[1].density = 0.9;
    group_masks(&mut copy, "g", "G", &["a".into(), "b".into()]).unwrap();
    let touched = apply_group_parameter_offsets(&mut copy, "g", 0.25, 0.25).unwrap();
    assert_eq!(touched, 1);
    assert!((copy.mask_layers[0].feather - 0.25).abs() < 1e-6);
    assert!((copy.mask_layers[0].density - 1.0).abs() < 1e-6);
    // Non-member layer untouched.
    assert!((copy.mask_layers[1].density - 0.9).abs() < 1e-6);
    assert!(apply_group_parameter_offsets(&mut copy, "g", f32::NAN, 0.0).is_err());
    assert!(apply_group_parameter_offsets(&mut copy, "missing", 0.0, 0.0).is_err());
}

#[test]
fn deleting_source_materializes_one_frozen_copy_and_repoints() {
    let mut copy = copy_with(&["a", "b"]);
    // `b` is a derived node referencing `a`, and `a` is a group member.
    copy.mask_library[1].operation = MaskOperation::Invert;
    copy.mask_library[1].references = vec![reference(&copy.id, "a")];
    copy.mask_layers = vec![layer(&copy.id, "a")];
    group_masks(&mut copy, "g", "G", &["a".into()]).unwrap();
    let outcome = delete_mask_node(&mut copy, "a").unwrap();
    assert_eq!(outcome.frozen_copies.len(), 1);
    assert_eq!(outcome.layers_repointed, 1);
    assert_eq!(outcome.layers_removed, 0);
    let frozen = outcome.frozen_copies[0].clone();
    assert!(!copy.mask_library.iter().any(|m| m.id == "a"));
    // The frozen copy carries the source definition (deep, independent).
    let frozen_def = copy.mask_library.iter().find(|m| m.id == frozen).unwrap();
    assert_eq!(frozen_def.operation, MaskOperation::Source);
    assert_eq!(frozen_def.status, MaskStatus::Valid);
    // Every reference (derived input, group member, layer) points to it.
    assert_eq!(
        copy.mask_library[0].references[0].mask_id, frozen,
        "derived node must be re-pointed"
    );
    assert!(mask_groups_of(&copy).unwrap()[0].contains(&frozen));
    assert_eq!(copy.mask_layers[0].mask.mask_id, frozen);
}

#[test]
fn deleting_unreferenced_mask_plainly_drops_its_layer() {
    let mut copy = copy_with(&["a"]);
    copy.mask_layers = vec![layer(&copy.id, "a")];
    let outcome = delete_mask_node(&mut copy, "a").unwrap();
    assert!(outcome.frozen_copies.is_empty());
    assert_eq!(outcome.layers_removed, 1);
    assert!(copy.mask_library.is_empty());
    assert!(copy.mask_layers.is_empty());
    assert!(delete_mask_node(&mut copy, "a").is_err());
}

#[test]
fn malformed_mask_groups_value_is_loud() {
    let mut copy = copy_with(&["a"]);
    copy.extras.insert(
        MASK_GROUPS_EXTRAS_KEY.into(),
        serde_json::Value::String("not an array".into()),
    );
    assert!(mask_groups_of(&copy).is_err());
}

#[test]
fn validate_rejects_cross_copy_member() {
    let mut document = SidecarDocument::new(source(), "p");
    let mut copy = copy_with(&["a", "b"]);
    group_masks(&mut copy, "g", "G", &["a".into()]).unwrap();
    let mut groups = mask_groups_of(&copy).unwrap();
    groups[0].members[0].copy_id = "other".into();
    set_mask_groups(&mut copy, groups).unwrap();
    document.virtual_copies[0] = copy;
    assert!(validate_mask_groups(&document).is_err());
}

#[test]
fn validate_rejects_double_grouping_and_unknown_member() {
    let mut document = SidecarDocument::new(source(), "p");
    let mut copy = copy_with(&["a", "b"]);
    group_masks(&mut copy, "g", "G", &["a".into()]).unwrap();
    // Inject a second group over the same node.
    let mut groups = mask_groups_of(&copy).unwrap();
    groups.push(MaskGroup::new("h", "H", vec![reference(&copy.id, "a")]).unwrap());
    set_mask_groups(&mut copy, groups).unwrap();
    document.virtual_copies[0] = copy;
    assert!(validate_mask_groups(&document).is_err());
}

/// E2E: groups persist through a file save/load with an unchanged
/// `schema_version`; an absent group list stays absent (legacy-stable).
#[test]
fn mask_groups_roundtrip_through_file_without_schema_bump() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("a.arw.lumina.json");
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    let mut copy = copy_with(&["a", "b"]);
    let group = group_masks(&mut copy, "g", "G", &["a".into(), "b".into()]).unwrap();
    document.virtual_copies[0] = copy;
    assert_eq!(document.schema_version, SCHEMA_VERSION);
    save_sidecar(&path, &document).unwrap();
    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(loaded.schema_version, SCHEMA_VERSION);
    let loaded_group = mask_groups_of(&loaded.virtual_copies[0]).unwrap();
    assert_eq!(loaded_group, vec![group]);

    // Legacy document without groups serializes back without the key.
    let legacy = SidecarDocument::new(source(), "pipeline-1");
    let json = legacy.to_json().unwrap();
    assert!(!json.contains(MASK_GROUPS_EXTRAS_KEY));
}
