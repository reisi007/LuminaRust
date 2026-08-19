//! Single-image-scoped session state for the MCP server.
//!
//! Per the F-101 spec the server holds exactly one image in memory at a time.
//! A new `lumina_load` discards the previous state. The decoded source frame is
//! kept as-is; recipe edits are persisted to the sidecar and applied only at
//! render time (preview/save/analyze), never mutating the source pixels.

use crate::error::McpError;
use lumina_core::ImageFrame;
use lumina_raw::RawMetadata;
use lumina_sidecar::{SidecarDocument, VirtualCopy};
use std::path::PathBuf;

/// All session state for the currently loaded image.
pub struct ImageState {
    /// Process-local, stable id assigned at load time.
    pub id: String,
    /// Path of the source image (the original is never modified).
    pub source_path: PathBuf,
    /// Path of the `.lumina.json` sidecar next to the source.
    pub sidecar_path: PathBuf,
    /// Decoded source frame (unedited; recipes are applied at render time).
    pub frame: ImageFrame,
    /// RAW decode metadata, if the source was a RAW file.
    pub raw_metadata: Option<RawMetadata>,
    /// The loaded (or freshly created) sidecar document.
    pub document: SidecarDocument,
    /// `"loaded"` if an existing sidecar was read, `"created"` if one was
    /// materialized on load.
    pub sidecar_status: String,
}

impl ImageState {
    /// Resolves a virtual copy by name (or id). `None` selects the default copy.
    pub fn find_copy<'a>(&'a self, name: Option<&str>) -> Result<&'a VirtualCopy, McpError> {
        match name {
            Some(requested) => self
                .document
                .virtual_copies
                .iter()
                .find(|copy| copy.name == requested || copy.id == requested)
                .ok_or_else(|| McpError::UnknownCopy(requested.to_string())),
            None => self
                .document
                .virtual_copies
                .iter()
                .find(|copy| copy.is_default)
                .or_else(|| self.document.virtual_copies.first())
                .ok_or(McpError::NoImageLoaded),
        }
    }
}

/// Holds the currently loaded image, if any.
#[derive(Default)]
pub struct McpSession {
    pub current: Option<ImageState>,
}

impl McpSession {
    pub fn require(&mut self) -> Result<&mut ImageState, McpError> {
        self.current.as_mut().ok_or(McpError::NoImageLoaded)
    }

    pub fn require_ref(&self) -> Result<&ImageState, McpError> {
        self.current.as_ref().ok_or(McpError::NoImageLoaded)
    }

    /// Borrows the loaded image, rejecting unknown or missing ids.
    pub fn require_id(&self, image_id: &str) -> Result<&ImageState, McpError> {
        match &self.current {
            None => Err(McpError::NoImageLoaded),
            Some(state) if state.id == image_id => Ok(state),
            Some(_) => Err(McpError::UnknownImage(image_id.to_string())),
        }
    }

    /// Mutable borrow of the loaded image, rejecting unknown or missing ids.
    pub fn require_id_mut(&mut self, image_id: &str) -> Result<&mut ImageState, McpError> {
        match &mut self.current {
            None => Err(McpError::NoImageLoaded),
            Some(state) if state.id == image_id => Ok(state),
            Some(_) => Err(McpError::UnknownImage(image_id.to_string())),
        }
    }
}
