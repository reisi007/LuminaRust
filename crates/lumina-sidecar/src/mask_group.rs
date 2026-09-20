//! LRPAR-G03-MASKGROUP-03: per-virtual-copy mask groups (Copy vs. Duplicate) —
//! schema, loud validation and the additive extras accessors.
//!
//! A *group* is a named logical container whose members are **references
//! (pointers) to mask nodes** of the owning virtual copy. Editing the source
//! node is visible to every member — there is no silent decoupling. Groups are
//! pure metadata: they never participate in the mask DAG evaluation, so they
//! cannot introduce cycles. The pure document operations live in
//! [`crate::mask_group_ops`]; this module owns the schema only (no GUI, no
//! filesystem, no recipe logic).
//!
//! Contract:
//! * `schema_version` is deliberately **not** bumped — the additive flattened
//!   top-level `mask_groups` array lives in the virtual copy's `extras` (the
//!   same additive-optional pattern as the generative negative prompt), so no
//!   `VirtualCopy` struct literal outside this module breaks. A present but
//!   malformed value is rejected loudly; the loader never normalizes silently.
//! * Members always reference the **owning copy** (`member.copy_id == copy.id`);
//!   cross-copy group membership is not part of v1.
//! * A mask node belongs to at most one group per copy; a second membership is
//!   rejected loudly (never silently re-homed).
//! * `id` is stable and deterministic (derived once from `copy_id` + sorted
//!   member keys) and is never recomputed on rename/reorder.
//! * `collapsed` is the persisted collapse state (default `false`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::{invalid, Extras, MaskReference, SidecarError, VirtualCopy};

/// Extras key of the additive flattened top-level `mask_groups` array.
pub const MASK_GROUPS_EXTRAS_KEY: &str = "mask_groups";

/// LRPAR-G03-MASKGROUP-03: current schema version of a persisted group.
/// Independent of `SCHEMA_VERSION`; an unknown `version` is rejected during
/// validation rather than silently ignored.
pub const MASK_GROUP_VERSION: u8 = 1;

/// Maximum number of members in one group. Bounds hostile JSON.
pub const MAX_MASK_GROUP_MEMBERS: usize = 256;

/// Maximum character length of a group id.
pub const MAX_MASK_GROUP_ID_CHARS: usize = 128;

/// Maximum character length of a group name.
pub const MAX_MASK_GROUP_NAME_CHARS: usize = 256;

/// LRPAR-G03-MASKGROUP-03: a named container of mask-node references of one
/// virtual copy. `members` order is user-visible (reorder action) and therefore
/// preserved; duplicates are rejected loudly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskGroup {
    /// [`MASK_GROUP_VERSION`] pin; unknown versions are rejected loudly.
    pub version: u8,
    /// Stable identity within the copy ([`Self::group_id_for_members`]).
    pub id: String,
    /// Non-empty, trimmed display name.
    pub name: String,
    /// Member references (pointers) to mask nodes of the owning copy.
    pub members: Vec<MaskReference>,
    /// Persisted collapse state (`false` = expanded). Absent reads as `false`.
    #[serde(default)]
    pub collapsed: bool,
    /// Additive extras for forward-compatible, unknown-field roundtrips.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

impl MaskGroup {
    /// Builds a validated group. `members` must be non-empty and unique;
    /// `id`/`name` are validated loudly. `collapsed` starts expanded.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        members: Vec<MaskReference>,
    ) -> Result<Self, SidecarError> {
        let group = Self {
            version: MASK_GROUP_VERSION,
            id: id.into(),
            name: name.into(),
            members,
            collapsed: false,
            extras: Extras::new(),
        };
        group.validate_shape()?;
        Ok(group)
    }

    /// Deterministic group id from the owning copy and the (sorted,
    /// deduplicated) member keys. Re-grouping the identical set yields the
    /// identical id (tests stay stable, no clock/RNG dependency). The id is
    /// stored once at creation and never recomputed afterwards.
    pub fn group_id_for_members(copy_id: &str, members: &[MaskReference]) -> String {
        let mut keys: Vec<String> = members
            .iter()
            .map(|member| format!("{}\u{0}{}", member.copy_id, member.mask_id))
            .collect();
        keys.sort();
        keys.dedup();
        let mut hasher = blake3::Hasher::new();
        hasher.update(copy_id.as_bytes());
        hasher.update(b"\0");
        for key in &keys {
            hasher.update(key.as_bytes());
            hasher.update(b"\n");
        }
        format!("mask-group-{}", &hasher.finalize().to_hex()[..16])
    }

    /// Shape validation independent of the owning copy (version, id, name,
    /// member count, uniqueness).
    pub fn validate_shape(&self) -> Result<(), SidecarError> {
        if self.version != MASK_GROUP_VERSION {
            return invalid(format!(
                "unsupported mask group version {} (expected {MASK_GROUP_VERSION})",
                self.version
            ));
        }
        validate_group_id(&self.id)?;
        validate_group_name(&self.name)?;
        if self.members.is_empty() {
            return invalid("mask group must have at least one member");
        }
        if self.members.len() > MAX_MASK_GROUP_MEMBERS {
            return invalid(format!(
                "mask group exceeds member limit of {MAX_MASK_GROUP_MEMBERS}"
            ));
        }
        let mut seen = BTreeSet::new();
        for member in &self.members {
            validate_reference_names(member)?;
            if !seen.insert((member.copy_id.clone(), member.mask_id.clone())) {
                return invalid("mask group members must be unique (no duplicates)");
            }
        }
        Ok(())
    }

    /// Shape plus same-copy ownership validation.
    pub fn validate_for_copy(&self, copy_id: &str) -> Result<(), SidecarError> {
        self.validate_shape()?;
        for member in &self.members {
            if member.copy_id != copy_id {
                return invalid(format!(
                    "mask group `{}` member `{}/{}` must belong to the owning copy `{copy_id}`",
                    self.id, member.copy_id, member.mask_id
                ));
            }
        }
        Ok(())
    }

    /// True when `mask_id` is a member of this group.
    pub fn contains(&self, mask_id: &str) -> bool {
        self.members.iter().any(|member| member.mask_id == mask_id)
    }

    /// Returns a copy with the persisted collapse state set.
    #[must_use]
    pub fn with_collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

/// A group id is a stable, portable identity — never a path. Non-empty,
/// trimmed, control-character free and within the length limit.
pub fn validate_group_id(id: &str) -> Result<(), SidecarError> {
    if id.is_empty() || id.trim() != id {
        return invalid("mask group id must be non-empty and without leading/trailing whitespace");
    }
    if id.chars().any(char::is_control) {
        return invalid("mask group id must not contain control characters");
    }
    if id.chars().count() > MAX_MASK_GROUP_ID_CHARS {
        return invalid(format!(
            "mask group id exceeds limit of {MAX_MASK_GROUP_ID_CHARS} characters"
        ));
    }
    Ok(())
}

/// A group name must be non-empty, trimmed and control-character free.
pub fn validate_group_name(name: &str) -> Result<(), SidecarError> {
    if name.is_empty() || name.trim() != name {
        return invalid(
            "mask group name must be non-empty and without leading/trailing whitespace",
        );
    }
    if name.chars().any(char::is_control) {
        return invalid("mask group name must not contain control characters");
    }
    if name.chars().count() > MAX_MASK_GROUP_NAME_CHARS {
        return invalid(format!(
            "mask group name exceeds limit of {MAX_MASK_GROUP_NAME_CHARS} characters"
        ));
    }
    Ok(())
}

fn validate_reference_names(reference: &MaskReference) -> Result<(), SidecarError> {
    if reference.copy_id.trim().is_empty() || reference.mask_id.trim().is_empty() {
        return invalid("mask group member reference must carry a non-empty copy_id and mask_id");
    }
    Ok(())
}

/// Reads the additive flattened `mask_groups` array from the copy's extras.
/// Absent (or explicit `null`) is the valid "no groups" state. A present but
/// malformed value is a hard error — never a silent empty fallback.
pub fn mask_groups_of(copy: &VirtualCopy) -> Result<Vec<MaskGroup>, SidecarError> {
    match copy.extras.get(MASK_GROUPS_EXTRAS_KEY) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => serde_json::from_value::<Vec<MaskGroup>>(value.clone()).map_err(|error| {
            SidecarError::Invalid(format!("invalid `{MASK_GROUPS_EXTRAS_KEY}` value: {error}"))
        }),
    }
}

/// Writes the additive flattened `mask_groups` array into the copy's extras.
/// An empty list removes the key, so legacy documents stay byte-stable.
pub fn set_mask_groups(copy: &mut VirtualCopy, groups: Vec<MaskGroup>) -> Result<(), SidecarError> {
    if groups.is_empty() {
        copy.extras.remove(MASK_GROUPS_EXTRAS_KEY);
        return Ok(());
    }
    let value = serde_json::to_value(&groups)
        .map_err(|error| SidecarError::Invalid(format!("mask_groups serialization: {error}")))?;
    copy.extras.insert(MASK_GROUPS_EXTRAS_KEY.into(), value);
    Ok(())
}

/// Id of the group that owns `mask_id` in `copy`, if any.
pub fn group_id_for_mask(
    copy: &VirtualCopy,
    mask_id: &str,
) -> Result<Option<String>, SidecarError> {
    Ok(mask_groups_of(copy)?
        .into_iter()
        .find(|group| group.contains(mask_id))
        .map(|group| group.id))
}
