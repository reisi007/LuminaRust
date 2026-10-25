//! Active virtual-copy resolution shared by CLI commands.

use super::{CliError, SidecarDocument};

/// Resolves the active virtual copy. Without `--virtual-copy` the first
/// (standard) copy is used; an unknown id is a loud error.
pub(super) fn active_copy_index(
    document: &SidecarDocument,
    virtual_copy: Option<&str>,
) -> Result<usize, CliError> {
    match virtual_copy {
        Some(id) => document
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`"))),
        None => {
            if document.virtual_copies.is_empty() {
                Err(CliError::Message(
                    "sidecar has no virtual copies; run `import` first".into(),
                ))
            } else {
                Ok(0)
            }
        }
    }
}
