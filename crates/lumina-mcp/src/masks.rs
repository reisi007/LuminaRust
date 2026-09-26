//! MCP-MASK-APPLY: mask resolution for the MCP render path.
//!
//! Every render-capable MCP tool resolves the persisted source-mask planes
//! from the sidecar's `.lumina.zdata` bundle through the SHARED
//! `lumina-stages` layer ([`load_persisted_mask_planes`]) and passes them to
//! `render_frame` as a [`MaskContext`] with the harmonized default policy
//! `warn` — the exact layers and policy the CLI's `process_selected` and the
//! shared `regenerate op="matching"` render use, so MCP and CLI render
//! byte-identical output for the same source + sidecar.
//!
//! No mask model is wired in `lumina-mcp` (it does not link `lumina-onnx`),
//! so a layer that cannot be resolved from the persistent bundle is skipped
//! with a render-time warning under `warn` policy — never silently replaced
//! and never a loud abort. This mirrors the CLI's `regenerate op="matching"`
//! render, which also loads persisted planes without re-inference.

use lumina_core::{MaskContext, MaskPolicy};
use lumina_sidecar::SidecarDocument;
use lumina_stages::pipeline::load_persisted_mask_planes;
use std::path::Path;

/// Builds the [`MaskContext`] for one render of `document`'s `active_copy_id`.
///
/// The planes come from [`load_persisted_mask_planes`] (the shared F-048/F-051
/// persistence layer): a MISSING bundle yields an empty map (nothing was
/// persisted), and a CORRUPT bundle surfaces as an explicit warning (both
/// through the shared layer's stderr channel and the logger) and is treated
/// as missing — never a silent fallback. The policy is [`MaskPolicy::Warn`]:
/// an unresolved layer is skipped with a render-time warning instead of
/// aborting the render.
pub fn render_mask_context<'a>(
    document: &'a SidecarDocument,
    active_copy_id: &'a str,
    zdata_path: &Path,
) -> MaskContext<'a> {
    let mut warnings = Vec::new();
    let planes = load_persisted_mask_planes(document, zdata_path, &mut warnings);
    for warning in &warnings {
        log::warn!("mcp mask render: {warning}");
    }
    MaskContext {
        copies: &document.virtual_copies,
        active_copy_id,
        planes,
        policy: MaskPolicy::Warn,
        source_roi: None,
    }
}
