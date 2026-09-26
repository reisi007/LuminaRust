//! Virtual-copy resolution and mutable access, shared by the CLI and the MCP
//! server.
//!
//! This is the code that used to be `mask_copy_mut` / `resolve_mask_copy` in
//! `crates/lumina-cli/src/mask_local.rs`. Both callers resolve a copy
//! identically, so an MCP call and the equivalent CLI call can never disagree
//! about *which* copy is being edited — including on the unknown-copy path,
//! where both abort with the same text and write nothing.

use crate::error::StageError;
use lumina_sidecar::{SidecarDocument, VirtualCopy};

/// Resolves the requested virtual copy to its id.
///
/// `Some(id)` must match a copy **id** exactly (the CLI's `--virtual-copy`
/// contract; the MCP tools document the same id-only rule so parity cannot be
/// broken by a name-only selection). `None` selects the default copy, falling
/// back to the first copy. An unknown id is a loud error, never a silent
/// fallback to another copy.
pub fn resolve_copy(
    document: &SidecarDocument,
    requested: Option<&str>,
) -> Result<String, StageError> {
    if let Some(id) = requested {
        if document.virtual_copies.iter().any(|copy| copy.id == id) {
            return Ok(id.into());
        }
        return Err(StageError::Message(format!("unknown virtual copy `{id}`")));
    }
    if let Some(default) = document.virtual_copies.iter().find(|copy| copy.is_default) {
        return Ok(default.id.clone());
    }
    document
        .virtual_copies
        .first()
        .map(|copy| copy.id.clone())
        .ok_or_else(|| StageError::Message("sidecar has no virtual copies".into()))
}

/// Mutable access to one virtual copy (loud on unknown ids).
pub fn copy_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut VirtualCopy, StageError> {
    document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| StageError::Message(format!("unknown virtual copy `{copy_id}`")))
}

/// Read-only access to one virtual copy (loud on unknown ids).
pub fn copy_ref<'a>(
    document: &'a SidecarDocument,
    copy_id: &str,
) -> Result<&'a VirtualCopy, StageError> {
    document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| StageError::Message(format!("unknown virtual copy `{copy_id}`")))
}
