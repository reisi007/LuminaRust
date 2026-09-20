//! G-15 META-MVP (Slice 1): the metadata batch-operation language.
//!
//! Moved verbatim from `lib.rs` (file-size ratchet, LRPAR-G15-STACK-15): the
//! batch language is cohesive with the source-level metadata it mutates.
//!
//! Each [`BatchOp`] variant applies to one sidecar document via
//! [`apply_batch_op`]; CLI/GUI follow-up slices iterate it over sidecar files
//! (one atomic write per file). Mutations are limited to keywords, static
//! collection memberships, ratings and flags — recipes, masks and history are
//! never touched.

use serde::{Deserialize, Serialize};

use crate::{
    invalid, validate_collection_id, validate_collection_name, validate_keyword,
    CollectionMembership, Flag, SidecarDocument, SidecarError, MAX_COLLECTIONS_PER_DOCUMENT,
    MAX_KEYWORDS_PER_DOCUMENT,
};

/// G-15 META-MVP (Slice 1): the metadata batch-operation language. Each
/// variant applies to one sidecar document via [`apply_batch_op`]; CLI/GUI
/// follow-up slices iterate it over sidecar files (one atomic write per
/// file). Mutations are limited to keywords, static collection memberships,
/// ratings and flags — recipes, masks and history are never touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchOp {
    /// Adds `keyword` when absent (idempotent; present → unchanged).
    AddKeyword { keyword: String },
    /// Removes `keyword` when present (idempotent; absent → unchanged).
    RemoveKeyword { keyword: String },
    /// Adds membership `{ id, name }` when `id` is absent; refreshes `name`
    /// when the `id` already exists under a different name (rename propagation).
    AddToCollection { id: String, name: String },
    /// Removes membership `id` when present (idempotent).
    RemoveFromCollection { id: String },
    /// Sets the rating (`0..=5`) of one virtual copy.
    SetRating { copy_id: String, rating: u8 },
    /// Sets the flag of one virtual copy.
    SetFlag { copy_id: String, flag: Flag },
}

/// Applies one metadata batch operation to `document`. Returns `Ok(true)`
/// when the document changed and `Ok(false)` for idempotent no-ops. Every
/// invalid input (bad keyword, bad collection id/name, unknown `copy_id`,
/// `rating > 5`) fails loudly; a rejected operation leaves the document
/// unchanged.
pub fn apply_batch_op(document: &mut SidecarDocument, op: &BatchOp) -> Result<bool, SidecarError> {
    match op {
        BatchOp::AddKeyword { keyword } => {
            validate_keyword(keyword)?;
            if document.keywords.iter().any(|k| k == keyword) {
                return Ok(false);
            }
            if document.keywords.len() >= MAX_KEYWORDS_PER_DOCUMENT {
                return invalid(format!(
                    "keyword list exceeds limit of {MAX_KEYWORDS_PER_DOCUMENT}"
                ))
                .map(|()| false);
            }
            document.keywords.push(keyword.clone());
            Ok(true)
        }
        BatchOp::RemoveKeyword { keyword } => {
            validate_keyword(keyword)?;
            let before = document.keywords.len();
            document.keywords.retain(|k| k != keyword);
            Ok(document.keywords.len() != before)
        }
        BatchOp::AddToCollection { id, name } => {
            validate_collection_id(id)?;
            validate_collection_name(name)?;
            if let Some(existing) = document.collections.iter_mut().find(|m| m.id == *id) {
                if existing.name == *name {
                    return Ok(false);
                }
                existing.name = name.clone();
                return Ok(true);
            }
            if document.collections.len() >= MAX_COLLECTIONS_PER_DOCUMENT {
                return invalid(format!(
                    "collection list exceeds limit of {MAX_COLLECTIONS_PER_DOCUMENT}"
                ))
                .map(|()| false);
            }
            document.collections.push(CollectionMembership {
                id: id.clone(),
                name: name.clone(),
            });
            Ok(true)
        }
        BatchOp::RemoveFromCollection { id } => {
            validate_collection_id(id)?;
            let before = document.collections.len();
            document.collections.retain(|m| m.id != *id);
            Ok(document.collections.len() != before)
        }
        BatchOp::SetRating { copy_id, rating } => {
            if *rating > 5 {
                return invalid(format!(
                    "virtual copy `{copy_id}` rating must be 0..=5, got {rating}"
                ))
                .map(|()| false);
            }
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == *copy_id)
                .ok_or_else(|| {
                    SidecarError::Invalid(format!("unknown virtual copy `{copy_id}`"))
                })?;
            if copy.rating == *rating {
                return Ok(false);
            }
            copy.rating = *rating;
            Ok(true)
        }
        BatchOp::SetFlag { copy_id, flag } => {
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == *copy_id)
                .ok_or_else(|| {
                    SidecarError::Invalid(format!("unknown virtual copy `{copy_id}`"))
                })?;
            if copy.flag == *flag {
                return Ok(false);
            }
            copy.flag = *flag;
            Ok(true)
        }
    }
}
