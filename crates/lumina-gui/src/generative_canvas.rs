//! GEN-ONNX-1 Welle 2b: the GUI's generative-canvas cache (value types and
//! resolver helpers).
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 7, file-size ratchet). The
//! block is a single responsibility — how one GUI render carries the resolved
//! `generative_canvas` artifacts of its active roles, pinned to their identity
//! digest — with no GUI behaviour change.

use lumina_core::{GenerativeCanvasArtifact, GenerativeCanvasInput};
use lumina_onnx::GenerativeRole as OnnxGenerativeRole;

/// GEN-ONNX-1 Welle 2b: one resolved generative canvas plus the exact identity
/// digest of the run it belongs to. The digest is the
/// [`lumina_core::GenerativeCacheKey::digest`] the producing run published, so
/// a later render can prove the canvas still matches the current
/// source/recipe/seed/canvas/prompt before it is composited — a stale canvas is
/// never served silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedGenerativeCanvas {
    pub(crate) identity: String,
    pub(crate) artifact: GenerativeCanvasArtifact,
}

/// GEN-ONNX-1 Welle 2b: the caller-supplied generative canvases of one GUI
/// render (preview and export share this struct). Exactly one entry per active
/// role; the both-roles-at-once record keeps two entries (the SOLL allows a
/// single record to carry `auto_fill_transparent` + `expand_beyond_image`).
/// Each entry is pinned to its identity digest and re-validated on every
/// resolve, so a recipe/seed/canvas/prompt change invalidates it loudly instead
/// of compositing a stale canvas.
#[derive(Debug, Clone, Default)]
pub(crate) struct GenerativeArtifacts {
    pub(crate) auto_fill: Option<CachedGenerativeCanvas>,
    pub(crate) expand: Option<CachedGenerativeCanvas>,
}

impl GenerativeArtifacts {
    /// Borrow as the core render-hook input.
    pub(crate) fn input(&self) -> GenerativeCanvasInput<'_> {
        GenerativeCanvasInput {
            auto_fill: self.auto_fill.as_ref().map(|cached| &cached.artifact),
            expand: self.expand.as_ref().map(|cached| &cached.artifact),
        }
    }

    pub(crate) fn get(&self, role: OnnxGenerativeRole) -> Option<&CachedGenerativeCanvas> {
        match role {
            OnnxGenerativeRole::AutoFillTransparent => self.auto_fill.as_ref(),
            OnnxGenerativeRole::Expand => self.expand.as_ref(),
        }
    }

    pub(crate) fn set(&mut self, role: OnnxGenerativeRole, cached: CachedGenerativeCanvas) {
        match role {
            OnnxGenerativeRole::AutoFillTransparent => self.auto_fill = Some(cached),
            OnnxGenerativeRole::Expand => self.expand = Some(cached),
        }
    }

    /// The core role mirror of one ONNX role.
    pub(crate) fn core_role(role: OnnxGenerativeRole) -> lumina_core::GenerativeRole {
        match role {
            OnnxGenerativeRole::AutoFillTransparent => {
                lumina_core::GenerativeRole::AutoFillTransparent
            }
            OnnxGenerativeRole::Expand => lumina_core::GenerativeRole::Expand,
        }
    }
}

/// Deterministic bundle record id for a generative identity digest — identical
/// to the CLI's `generative_record_id`, so GUI and CLI address the same record.
pub(crate) fn generative_record_id(identity_digest: &str) -> String {
    let prefix = identity_digest.get(..16).unwrap_or(identity_digest);
    format!("generative_canvas:{prefix}")
}

/// GEN-ONNX-1 Welle 2b: visible status of one generative role, mirroring the
/// sidecar vocabulary `valid` | `stale` | `missing` | `corrupt` (SOLL
/// `feature/product/generative-expand.md`, "Statuswerte wie bei AI-Masken").
/// Recorded by the resolver for the last resolved recipe identity, so the panel
/// can show *why* a role is not ready instead of only ready/missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GenerativeRoleStatus {
    #[default]
    Missing,
    Valid,
    Stale,
    Corrupt,
}

impl GenerativeRoleStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Corrupt => "corrupt",
        }
    }
}

/// Stable index of one generative role in the per-role status array.
pub(crate) fn generative_role_index(role: OnnxGenerativeRole) -> usize {
    match role {
        OnnxGenerativeRole::AutoFillTransparent => 0,
        OnnxGenerativeRole::Expand => 1,
    }
}
