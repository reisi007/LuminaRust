//! LRPAR-G15-STACK-15: source-level image-stack membership.
//!
//! A *stack* groups at least two source images of the **same folder** into a
//! collapsible unit (Lightroom-style "Stack"). Membership is persisted
//! sidecar-first in every member's sidecar as the additive, optional top-level
//! `stack` section on [`crate::SidecarDocument`]. This module owns the schema,
//! the loud validation and the deterministic stack-id derivation; it contains
//! no GUI, no filesystem and no recipe logic.
//!
//! Contract:
//! * `schema_version` is deliberately **not** bumped — the field is additive
//!   and optional exactly like `keywords`/`collections`/`face`/`culling`.
//! * Members and the cover are plain file names of the same folder. Path
//!   separators, `.`/`..` and absolute paths are rejected loudly: a portable
//!   sidecar never carries a path into a stack.
//! * `members` is sorted and unique and always contains `cover`.
//! * `collapsed` is the persisted collapse state; a missing value reads as
//!   `false` (expanded). Concurrent writers follow last-writer-wins (the
//!   GUI/CLI decide the write order; the schema stores one value).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{invalid, Extras, SidecarError};

/// LRPAR-G15-STACK-15: current schema version of a persisted stack section.
/// Independent of `SCHEMA_VERSION`; an unknown `version` is rejected during
/// validation rather than silently ignored.
pub const STACK_SCHEMA_VERSION: u8 = 1;

/// Maximum number of members in one stack (cover included). Bounds hostile
/// JSON; a legitimate stack stays far below this.
pub const MAX_STACK_MEMBERS: usize = 256;

/// Maximum character length of a stack id.
pub const MAX_STACK_ID_CHARS: usize = 128;

/// Maximum character length of one member/cover file name.
pub const MAX_STACK_MEMBER_CHARS: usize = 512;

/// LRPAR-G15-STACK-15: the sidecar-first stack section shared by every member
/// of the stack. Absent (`None` on [`crate::SidecarDocument`]) is the
/// legitimate "not stacked" state and serializes back absent, so legacy
/// documents stay byte-stable.
///
/// The same [`StackMembership`] value is written to **every** member's sidecar
/// (identical `stack_id`, `cover`, sorted `members` and `collapsed`), so the
/// stack is fully reconstructible from any single member — there is no second
/// (catalogue) source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackMembership {
    /// `STACK_SCHEMA_VERSION` pin; unknown versions are rejected loudly.
    pub version: u8,
    /// Stable identity within the folder. Derived deterministically from the
    /// sorted member names at creation ([`Self::stack_id_for_members`]) and
    /// never recomputed on later reads, so it survives renames/reorder.
    pub stack_id: String,
    /// Relative file name of the stack's top/cover image (same folder,
    /// `members` member).
    pub cover: String,
    /// Sorted, unique relative file names of all members (cover included).
    pub members: Vec<String>,
    /// Persisted collapse state (`false` = expanded). Absent reads as `false`.
    #[serde(default)]
    pub collapsed: bool,
    /// Additive extras for forward-compatible, unknown-field roundtrips.
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

impl StackMembership {
    /// Builds a validated stack section. `members` is sorted deterministically;
    /// duplicates, a missing `cover`, a foreign/absolute member name or a
    /// member count outside `2..=MAX_STACK_MEMBERS` fail loudly and leave no
    /// half-built value. `collapsed` starts expanded.
    pub fn new(
        stack_id: impl Into<String>,
        cover: impl Into<String>,
        members: Vec<String>,
    ) -> Result<Self, SidecarError> {
        let mut members = members;
        members.sort();
        let section = Self {
            version: STACK_SCHEMA_VERSION,
            stack_id: stack_id.into(),
            cover: cover.into(),
            members,
            collapsed: false,
            extras: Extras::new(),
        };
        section.validate()?;
        Ok(section)
    }

    /// Deterministic, folder-scoped stack id from the (sorted, deduplicated)
    /// member file names. Re-stacking the identical set yields the identical
    /// id (tests stay stable, no clock/RNG dependency). The id is stored once
    /// at creation and never recomputed afterwards.
    pub fn stack_id_for_members(members: &[String]) -> String {
        let mut sorted = members.to_vec();
        sorted.sort();
        sorted.dedup();
        let mut hasher = blake3::Hasher::new();
        for member in &sorted {
            hasher.update(member.as_bytes());
            hasher.update(b"\n");
        }
        let hex = hasher.finalize().to_hex();
        format!("stack-{}", &hex[..16])
    }

    /// Loud validation of the persisted contract (version pin, id shape, member
    /// names, uniqueness and `cover ∈ members`). Never normalizes or clamps.
    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.version != STACK_SCHEMA_VERSION {
            return invalid(format!(
                "unsupported stack version {} (expected {STACK_SCHEMA_VERSION})",
                self.version
            ));
        }
        validate_stack_id(&self.stack_id)?;
        validate_stack_member_name("stack.cover", &self.cover)?;
        if self.members.len() < 2 {
            return invalid("stack must have at least two members");
        }
        if self.members.len() > MAX_STACK_MEMBERS {
            return invalid(format!("stack exceeds member limit of {MAX_STACK_MEMBERS}"));
        }
        let mut previous: Option<&str> = None;
        for member in &self.members {
            validate_stack_member_name("stack.members", member)?;
            if let Some(prev) = previous {
                if member.as_str() <= prev {
                    return invalid("stack.members must be sorted and unique (no duplicates)");
                }
            }
            previous = Some(member.as_str());
        }
        if !self.members.iter().any(|member| member == &self.cover) {
            return invalid("stack.cover must be listed in stack.members");
        }
        Ok(())
    }

    /// True when `name` (a bare file name) is the stack cover.
    pub fn is_cover(&self, name: &str) -> bool {
        name == self.cover
    }

    /// True when `name` is a member of this stack.
    pub fn contains(&self, name: &str) -> bool {
        self.members.iter().any(|member| member == name)
    }

    /// Returns a copy with the persisted collapse state set. The caller is
    /// responsible for writing the whole stack (all members) so readers see a
    /// consistent value; last writer wins.
    #[must_use]
    pub fn with_collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

/// A stack id is a stable, portable identity — never a path. It must be
/// non-empty, trimmed, control-character free and within the length limit.
pub fn validate_stack_id(id: &str) -> Result<(), SidecarError> {
    if id.is_empty() || id.trim() != id {
        return invalid("stack id must be non-empty and without leading/trailing whitespace");
    }
    if id.chars().any(char::is_control) {
        return invalid("stack id must not contain control characters");
    }
    if id.chars().count() > MAX_STACK_ID_CHARS {
        return invalid(format!(
            "stack id exceeds limit of {MAX_STACK_ID_CHARS} characters"
        ));
    }
    Ok(())
}

/// A member/cover name must be a plain file name of the stack's folder. Path
/// separators, `.`/`..` and absolute paths are rejected loudly so a sidecar can
/// never smuggle a path (same-folder invariant, portable bundle).
pub fn validate_stack_member_name(field: &str, name: &str) -> Result<(), SidecarError> {
    if name.is_empty() || name.trim() != name {
        return invalid(format!(
            "{field} must be non-empty and without leading/trailing whitespace"
        ));
    }
    if name.chars().any(char::is_control) {
        return invalid(format!("{field} must not contain control characters"));
    }
    if name.chars().count() > MAX_STACK_MEMBER_CHARS {
        return invalid(format!(
            "{field} exceeds limit of {MAX_STACK_MEMBER_CHARS} characters"
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return invalid(format!(
            "{field} must be a bare file name (same-folder stack, no path separators)"
        ));
    }
    if name == "." || name == ".." {
        return invalid(format!("{field} must be a file name, not `{name}`"));
    }
    Ok(())
}
