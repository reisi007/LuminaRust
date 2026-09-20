//! LRPAR-G03-MASKGROUP-03: pure document operations on mask groups.
//!
//! These functions mutate a [`VirtualCopy`] in place (no filesystem, no GUI, no
//! persistence) and are the tested core of the group feature:
//!
//! * late grouping ([`group_masks`]),
//! * dissolve/reorder/collapse,
//! * shared parameter offsets as one unit
//!   ([`apply_group_parameter_offsets`]),
//! * source deletion with reference materialization ([`delete_mask_node`]),
//! * the loud document validation ([`validate_mask_groups`],
//!   [`validate_mask_graph`]).
//!
//! Materialization is deliberately general: deleting a node re-points **every**
//! reference to it (derived-node inputs, group memberships and layers) to one
//! frozen deep copy — never a dangling reference and never a silent member
//! deletion. This is the local counterpart of the cross-copy materialization
//! rule in `feature/product/ai-masks.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::mask_group::{mask_groups_of, set_mask_groups, MaskGroup};
use crate::{invalid, MaskDefinition, MaskReference, SidecarDocument, SidecarError, VirtualCopy};

/// Late grouping: creates a group over existing mask nodes of `copy`. Every
/// member must exist and must not already belong to another group. The group id
/// is provided by the caller (the GUI derives it deterministically) and the
/// resulting group is validated before it is stored.
pub fn group_masks(
    copy: &mut VirtualCopy,
    group_id: impl Into<String>,
    name: impl Into<String>,
    member_ids: &[String],
) -> Result<MaskGroup, SidecarError> {
    let copy_id = copy.id.clone();
    let members: Vec<MaskReference> = member_ids
        .iter()
        .map(|mask_id| MaskReference {
            copy_id: copy_id.clone(),
            mask_id: mask_id.clone(),
            extras: crate::Extras::new(),
        })
        .collect();
    let group = MaskGroup::new(group_id, name, members)?;
    group.validate_for_copy(&copy_id)?;
    let mut groups = mask_groups_of(copy)?;
    if groups.iter().any(|existing| existing.id == group.id) {
        return Err(SidecarError::Invalid(format!(
            "mask group id `{}` already exists",
            group.id
        )));
    }
    for member in &group.members {
        if !copy
            .mask_library
            .iter()
            .any(|mask| mask.id == member.mask_id)
        {
            return Err(SidecarError::Invalid(format!(
                "mask group `{}` references unknown mask `{}/{}`",
                group.id, member.copy_id, member.mask_id
            )));
        }
        if let Some(owner) = groups.iter().find(|g| g.contains(&member.mask_id)) {
            return Err(SidecarError::Invalid(format!(
                "mask `{}` already belongs to group `{}`",
                member.mask_id, owner.id
            )));
        }
    }
    groups.push(group.clone());
    set_mask_groups(copy, groups)?;
    Ok(group)
}

/// Dissolves a group container; the member masks stay in the library untouched.
/// An unknown group id is a loud error.
pub fn dissolve_group(copy: &mut VirtualCopy, group_id: &str) -> Result<(), SidecarError> {
    let mut groups = mask_groups_of(copy)?;
    let before = groups.len();
    groups.retain(|group| group.id != group_id);
    if groups.len() == before {
        return invalid(format!("mask group `{group_id}` not found"));
    }
    set_mask_groups(copy, groups)
}

/// Persists the collapse state of one group. Unknown group ids are loud.
pub fn set_group_collapsed(
    copy: &mut VirtualCopy,
    group_id: &str,
    collapsed: bool,
) -> Result<(), SidecarError> {
    let mut groups = mask_groups_of(copy)?;
    let group = groups
        .iter_mut()
        .find(|group| group.id == group_id)
        .ok_or_else(|| SidecarError::Invalid(format!("mask group `{group_id}` not found")))?;
    group.collapsed = collapsed;
    set_mask_groups(copy, groups)
}

/// Moves the member `mask_id` by `delta` positions within its group
/// (clamped to `0..len-1`). Unknown group/member ids are loud.
pub fn move_group_member(
    copy: &mut VirtualCopy,
    group_id: &str,
    mask_id: &str,
    delta: isize,
) -> Result<(), SidecarError> {
    let mut groups = mask_groups_of(copy)?;
    let group = groups
        .iter_mut()
        .find(|group| group.id == group_id)
        .ok_or_else(|| SidecarError::Invalid(format!("mask group `{group_id}` not found")))?;
    let position = group
        .members
        .iter()
        .position(|member| member.mask_id == mask_id)
        .ok_or_else(|| {
            SidecarError::Invalid(format!(
                "mask `{mask_id}` is not a member of group `{group_id}`"
            ))
        })?;
    let target = (position as isize + delta).clamp(0, group.members.len() as isize - 1) as usize;
    if target != position {
        let member = group.members.remove(position);
        group.members.insert(target, member);
    }
    set_mask_groups(copy, groups)
}

/// Applies shared feather/density offsets to every layer that references a
/// member of `group_id`, as one unit. Values are clamped deterministically to
/// the valid ranges (`feather >= 0`, `density` in `0..=1`). Returns the number
/// of layers touched. Non-finite offsets are a loud error.
pub fn apply_group_parameter_offsets(
    copy: &mut VirtualCopy,
    group_id: &str,
    feather_delta: f32,
    density_delta: f32,
) -> Result<usize, SidecarError> {
    if !feather_delta.is_finite() || !density_delta.is_finite() {
        return Err(SidecarError::Invalid(
            "mask group parameter offsets must be finite".into(),
        ));
    }
    let groups = mask_groups_of(copy)?;
    let group = groups
        .iter()
        .find(|group| group.id == group_id)
        .ok_or_else(|| SidecarError::Invalid(format!("mask group `{group_id}` not found")))?;
    let member_ids: BTreeSet<&str> = group
        .members
        .iter()
        .map(|member| member.mask_id.as_str())
        .collect();
    let mut touched = 0usize;
    for layer in &mut copy.mask_layers {
        if layer.mask.copy_id == copy.id && member_ids.contains(layer.mask.mask_id.as_str()) {
            layer.feather = (layer.feather + feather_delta).max(0.0);
            layer.density = (layer.density + density_delta).clamp(0.0, 1.0);
            touched += 1;
        }
    }
    Ok(touched)
}

/// Outcome of [`delete_mask_node`] for loud logging / history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskDeletionOutcome {
    /// Frozen deep copies created (zero or one for one deleted node).
    pub frozen_copies: Vec<String>,
    /// Layers re-pointed to a frozen copy.
    pub layers_repointed: usize,
    /// Layers dropped because the deleted node had no materialized successor.
    pub layers_removed: usize,
}

/// Deletes a mask node, materializing every reference to it first.
///
/// Exactly **one** frozen deep copy of the deleted definition is created (when
/// any reference points at it) and all references — derived-node inputs, group
/// memberships and referencing layers — are re-pointed to the frozen copy. With
/// no references the node is removed plainly and referencing layers are
/// dropped. The returned [`MaskDeletionOutcome`] documents the materialization
/// so the caller can record it loudly (log + history).
pub fn delete_mask_node(
    copy: &mut VirtualCopy,
    mask_id: &str,
) -> Result<MaskDeletionOutcome, SidecarError> {
    let source = copy
        .mask_library
        .iter()
        .find(|mask| mask.id == mask_id)
        .cloned()
        .ok_or_else(|| SidecarError::Invalid(format!("mask `{mask_id}` not found")))?;

    let frozen_id = materialize_references(copy, mask_id, &source)?;

    let mut layers_repointed = 0usize;
    let mut layers_removed = 0usize;
    match &frozen_id {
        Some(target) => {
            for layer in &mut copy.mask_layers {
                if layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id {
                    layer.mask.mask_id = target.clone();
                    layers_repointed += 1;
                }
            }
        }
        None => {
            let before = copy.mask_layers.len();
            copy.mask_layers
                .retain(|layer| !(layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id));
            layers_removed = before - copy.mask_layers.len();
        }
    }
    copy.mask_library.retain(|mask| mask.id != mask_id);
    Ok(MaskDeletionOutcome {
        frozen_copies: frozen_id.into_iter().collect(),
        layers_repointed,
        layers_removed,
    })
}

/// Creates (if needed) one frozen deep copy of `source` and re-points every
/// reference to `source.id` in derived nodes and group memberships. Returns the
/// frozen copy id, or `None` when nothing referenced `source.id`.
fn materialize_references(
    copy: &mut VirtualCopy,
    source_id: &str,
    source: &MaskDefinition,
) -> Result<Option<String>, SidecarError> {
    let mut referenced = copy
        .mask_library
        .iter()
        .any(|mask| mask.references.iter().any(|r| r.mask_id == source_id));
    let mut groups = mask_groups_of(copy)?;
    referenced |= groups
        .iter()
        .any(|group| group.members.iter().any(|m| m.mask_id == source_id));
    if !referenced {
        return Ok(None);
    }
    let frozen_id = frozen_copy_id(copy, source_id);
    let mut frozen = source.clone();
    frozen.id = frozen_id.clone();
    frozen.name = format!("{} (frozen)", source.name);
    copy.mask_library.push(frozen);
    for mask in &mut copy.mask_library {
        for reference in &mut mask.references {
            if reference.copy_id == copy.id && reference.mask_id == source_id {
                reference.mask_id = frozen_id.clone();
            }
        }
    }
    for group in &mut groups {
        for member in &mut group.members {
            if member.copy_id == copy.id && member.mask_id == source_id {
                member.mask_id = frozen_id.clone();
            }
        }
    }
    set_mask_groups(copy, groups)?;
    Ok(Some(frozen_id))
}

/// Deterministic frozen-copy id for a deleted source node, made unique against
/// the current library with a numeric suffix if needed.
fn frozen_copy_id(copy: &VirtualCopy, source_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mask-group-freeze\0");
    hasher.update(copy.id.as_bytes());
    hasher.update(b"\0");
    hasher.update(source_id.as_bytes());
    let base = format!("mask-{}", &hasher.finalize().to_hex()[..32]);
    let mut candidate = base.clone();
    let mut suffix = 2;
    while copy.mask_library.iter().any(|mask| mask.id == candidate) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    candidate
}

/// LRPAR-G03-MASKGROUP-03: validates every group of every virtual copy (active
/// and archived): shape, same-copy membership, resolvable member nodes and the
/// at-most-one-group-per-node rule. Never normalizes silently.
pub fn validate_mask_groups(document: &SidecarDocument) -> Result<(), SidecarError> {
    for copy in document
        .virtual_copies
        .iter()
        .chain(document.deleted_virtual_copies.iter())
    {
        let groups = mask_groups_of(copy)?;
        let mut group_ids = BTreeSet::new();
        let mut member_owner: BTreeMap<String, String> = BTreeMap::new();
        for group in &groups {
            group.validate_for_copy(&copy.id)?;
            if !group_ids.insert(group.id.clone()) {
                return invalid(format!(
                    "duplicate mask group id `{}` in copy `{}`",
                    group.id, copy.id
                ));
            }
            for member in &group.members {
                if !copy
                    .mask_library
                    .iter()
                    .any(|mask| mask.id == member.mask_id)
                {
                    return invalid(format!(
                        "mask group `{}` references unknown mask `{}/{}`",
                        group.id, member.copy_id, member.mask_id
                    ));
                }
                if let Some(owner) = member_owner.insert(member.mask_id.clone(), group.id.clone()) {
                    return invalid(format!(
                        "mask `{}` belongs to more than one group (`{owner}` and `{}`)",
                        member.mask_id, group.id
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Validates the mask DAG of a document (moved out of `lib.rs` for the
/// file-size ratchet). Rejects unknown references, self-references and cycles
/// loudly.
pub(crate) fn validate_mask_graph(
    document: &SidecarDocument,
    copy_ids: &BTreeSet<&String>,
) -> Result<(), SidecarError> {
    let mut nodes = BTreeSet::new();
    let mut edges = BTreeMap::<(String, String), Vec<(String, String)>>::new();
    for copy in &document.virtual_copies {
        for mask in &copy.mask_library {
            let node = (copy.id.clone(), mask.id.clone());
            nodes.insert(node.clone());
            for reference in &mask.references {
                edges
                    .entry(node.clone())
                    .or_default()
                    .push((reference.copy_id.clone(), reference.mask_id.clone()));
            }
        }
    }
    for (from, targets) in &edges {
        for target in targets {
            if !copy_ids.contains(&target.0) || !nodes.contains(target) {
                return invalid(format!(
                    "mask `{}/{}' references unknown mask `{}/{}`",
                    from.0, from.1, target.0, target.1
                ));
            }
            if from == target {
                return invalid(format!("mask `{}/{}' references itself", from.0, from.1));
            }
        }
    }
    fn visit(
        node: &(String, String),
        edges: &BTreeMap<(String, String), Vec<(String, String)>>,
        visiting: &mut BTreeSet<(String, String)>,
        visited: &mut BTreeSet<(String, String)>,
    ) -> Result<(), SidecarError> {
        if visiting.contains(node) {
            return invalid(format!(
                "mask graph contains a cycle at `{}/{}`",
                node.0, node.1
            ));
        }
        if !visited.insert(node.clone()) {
            return Ok(());
        }
        visiting.insert(node.clone());
        if let Some(targets) = edges.get(node) {
            for target in targets {
                visit(target, edges, visiting, visited)?;
            }
        }
        visiting.remove(node);
        Ok(())
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in nodes {
        visit(&node, &edges, &mut visiting, &mut visited)?;
    }
    Ok(())
}
