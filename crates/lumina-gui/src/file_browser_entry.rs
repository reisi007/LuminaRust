//! GUI-LIB-SPLIT-65 Slice 9: `FileBrowserEntry` (the Library file-row data
//! record) and its `thumbnail_key` helper. Extracted for cohesion: the
//! structure is the single data contract shared by the Library scan, grid,
//! filmstrip, sort, filter, stack and culling modules.

use super::*;

#[derive(Debug, Clone)]
pub struct FileBrowserEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    /// Stable thumbnail key: canonicalized absolute path. Identical filenames
    /// in different folders must never share a thumbnail cell
    /// (REVIEW-GUI-THUMB-1).
    pub(crate) thumb_key: String,
    pub(crate) has_sidecar: bool,
    pub(crate) source_status: SourceStatus,
    pub(crate) conflict: bool,
    pub(crate) virtual_copies: usize,
    pub(crate) missing_models: usize,
    /// Star rating (`0..=5`, `0` = unrated) of the default virtual copy
    /// (LR-01). `0` when no sidecar exists or it carries no copies.
    pub(crate) rating: u8,
    /// Pick flag of the default virtual copy (LR-01); `Unflagged` without a
    /// sidecar.
    pub(crate) flag: lumina_sidecar::Flag,
    /// Color label (`0..=4`, `0` = none) of the default virtual copy (Welle 2),
    /// read from the copy's `extras["color_label"]`; `0` without a sidecar.
    pub(crate) color_label: u8,
    /// Source-level keywords of the sidecar (`SidecarDocument.keywords`,
    /// G-15 META-MVP Slice 3); empty without a sidecar. Powers the
    /// `keyword:` Library filter and the smart-collection evaluation.
    pub(crate) keywords: Vec<String>,
    /// Source-level static collection memberships
    /// (`SidecarDocument.collections`, G-15 META-MVP Slice 3); empty without
    /// a sidecar. Powers the `collection:` filter and the collection picker.
    pub(crate) collections: Vec<CollectionMembership>,
    /// `camera_make + camera_model` from `lumina_raw::read_metadata`
    /// (best effort; `None` when unreadable). Powers the `camera:` filter.
    pub(crate) camera: Option<String>,
    /// ISO from `lumina_raw::read_metadata` (best effort). Powers `iso:`.
    pub(crate) iso: Option<f32>,
    /// Focal length in mm from `lumina_raw::read_metadata` (best effort).
    /// Powers `focal:`/`focal_length:`.
    pub(crate) focal_length: Option<f32>,
    /// LRPAR-G09-SORT-09: EXIF capture timestamp (Unix seconds, best effort)
    /// from `lumina_raw::read_metadata`. Powers the `CaptureDate` sort; `None`
    /// when unreadable/absent (sorts after every known timestamp).
    pub(crate) capture_timestamp: Option<i64>,
    /// Relative subfolder of the entry vs. the listed directory (`""` for
    /// top-level files). Powers the Library grid path badge (F-100): the
    /// recursive aggregation shows subfolder images with their relative
    /// folder as badge; flat listings (tree click) always carry `""`.
    pub(crate) folder: String,
    /// LRPAR-G09-CULL-25 (GUI slice): scan-level assisted-culling badge
    /// (`None`/`keep`/`review`/`reject`/`stale`) from the sidecar's
    /// source-level `culling` section. Display only; the authoritative read
    /// state is computed on demand by `LuminaApp::culling_read_state`.
    pub(crate) cull_badge: cull_gui::CullBadge,
    /// LRPAR-G12-FACE-20 (S5): person names of the sidecar's source-level face
    /// analysis (empty without one). Powers the `person:` Library filter.
    /// No geotag/GPS data is ever read or stored here.
    pub(crate) face_persons: Vec<String>,
    /// LRPAR-G15-STACK-15: source-level image-stack membership
    /// (`SidecarDocument.stack`); `None` without a sidecar or outside a stack.
    /// Drives the Grid/Filmstrip collapse and the stack-as-unit selection.
    pub(crate) stack: Option<lumina_sidecar::StackMembership>,
}

/// REVIEW-GUI-THUMB-1: stable thumbnail cache key. The canonicalized absolute
/// path guarantees that the same filename in two folders maps to different
/// entries; a canonicalize failure (e.g. a missing file) falls back to the
/// lossy path string, which is still folder-scoped.
pub(crate) fn thumbnail_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}
