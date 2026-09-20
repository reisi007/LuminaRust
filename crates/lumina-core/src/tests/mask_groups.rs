//! LRPAR-G03-MASKGROUP-03: core mask-graph integration of groups.
//!
//! Groups are metadata, not DAG nodes: a member is a pointer to a mask node.
//! These tests prove the two required semantics end-to-end at the graph level:
//! * **propagation** — a group member follows its source node (no decoupling),
//!   while a deep `Copy` (a distinct node) stays independent;
//! * **materialization** — deleting a referenced source node creates one frozen
//!   copy that the derived node, the group member and the layer all resolve
//!   through.

use super::*;
use lumina_sidecar::{
    delete_mask_node, group_masks, mask_groups_of, CoordinateSystem, DecodeFingerprint, Extras,
    GeometryFingerprint, MaskDefinition, MaskOperation, MaskReference, MaskStatus, ModelIdentity,
    Preprocessing, Resolution, SourceFingerprint, VirtualCopy,
};
use std::collections::BTreeMap;

fn definition(
    id: &str,
    operation: MaskOperation,
    references: Vec<MaskReference>,
) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "h".into(),
            byte_length: 1,
            extras: Extras::new(),
        },
        decode_context: DecodeFingerprint {
            decoder: "d".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        geometry_context: GeometryFingerprint {
            width: 2,
            height: 1,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: ModelIdentity {
            name: "m".into(),
            version: "1".into(),
            hash: "h".into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width: 2,
            height: 1,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status: MaskStatus::Valid,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: None,
        operation,
        references,
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    }
}

fn reference(copy_id: &str, mask_id: &str) -> MaskReference {
    MaskReference {
        copy_id: copy_id.into(),
        mask_id: mask_id.into(),
        extras: Extras::new(),
    }
}

fn copy_with(id: &str, definitions: Vec<MaskDefinition>) -> VirtualCopy {
    VirtualCopy {
        id: id.into(),
        name: id.into(),
        is_default: true,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: Default::default(),
        mask_library: definitions,
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }
}

fn plane(value: u16) -> MaskPlane {
    MaskPlane::new(2, 1, vec![value, value]).unwrap()
}

/// A group member pointer resolves to exactly the source node's plane, and a
/// later change of the source is visible to the member (propagation). A deep
/// `Copy` (distinct node id) stays independent — the Copy-vs.-Duplicate split.
#[test]
fn group_member_propagates_source_changes_while_copy_stays_independent() {
    let mut copy = copy_with(
        "vc",
        vec![
            definition("source", MaskOperation::Source, vec![]),
            definition("copy", MaskOperation::Source, vec![]),
        ],
    );
    group_masks(&mut copy, "g", "G", &["source".into()]).unwrap();
    let member = mask_groups_of(&copy).unwrap()[0].members[0].clone();
    assert_eq!(member.mask_id, "source");

    let copies = std::slice::from_ref(&copy);
    let sources = BTreeMap::from([
        (("vc".into(), "source".into()), plane(10)),
        (("vc".into(), "copy".into()), plane(10)),
    ]);
    let graph = MaskGraph::new(copies, sources);
    assert_eq!(graph.evaluate(&member).unwrap().values, vec![10, 10]);

    // Change the source plane; the member follows, the deep copy does not.
    let sources = BTreeMap::from([
        (("vc".into(), "source".into()), plane(42)),
        (("vc".into(), "copy".into()), plane(10)),
    ]);
    let graph = MaskGraph::new(copies, sources);
    assert_eq!(
        graph.evaluate(&member).unwrap().values,
        vec![42, 42],
        "the group member must follow the source (no silent decoupling)"
    );
    assert_eq!(
        graph.evaluate(&reference("vc", "copy")).unwrap().values,
        vec![10, 10],
        "the deep copy must stay independent"
    );
}

/// Deleting a referenced source node materializes one frozen copy; the derived
/// node, the group member and the referencing layer all resolve through it.
#[test]
fn deleting_source_materializes_through_graph() {
    let mut copy = copy_with(
        "vc",
        vec![
            definition("source", MaskOperation::Source, vec![]),
            definition(
                "derived",
                MaskOperation::Invert,
                vec![reference("vc", "source")],
            ),
        ],
    );
    group_masks(&mut copy, "g", "G", &["source".into()]).unwrap();
    let outcome = delete_mask_node(&mut copy, "source").unwrap();
    assert_eq!(outcome.frozen_copies.len(), 1);
    let frozen = outcome.frozen_copies[0].clone();

    // The group member now points at the frozen copy.
    let member = mask_groups_of(&copy).unwrap()[0].members[0].clone();
    assert_eq!(member.mask_id, frozen);

    let copies = std::slice::from_ref(&copy);
    let sources = BTreeMap::from([(("vc".into(), frozen.clone()), plane(7))]);
    let graph = MaskGraph::new(copies, sources);
    // The member resolves through the frozen source.
    assert_eq!(graph.evaluate(&member).unwrap().values, vec![7, 7]);
    // The derived node (invert of the re-pointed frozen source) still resolves.
    assert_eq!(
        graph.evaluate(&reference("vc", "derived")).unwrap().values,
        vec![u16::MAX - 7, u16::MAX - 7]
    );
    // No dangling reference to the deleted node remains.
    assert!(copy
        .mask_library
        .iter()
        .flat_map(|mask| mask.references.iter())
        .all(|r| r.mask_id != "source"));
}
