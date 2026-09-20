#![allow(
    clippy::identity_op,
    clippy::field_reassign_with_default,
    clippy::chunks_exact_to_as_chunks,
    unused_variables,
    unused_mut
)]
//! Shared eframe application for the native desktop GUI.

// Native-only capabilities (background thumbnail pool, disk-cache probes,
// filmstrip/navigator windowing math, file-backed presets, neighbor-preview
// controller with background threads + native file IO) live in their own
// modules without platform gates: the GUI is native-only
// (`feature/platform/cli-gui-wasm.md` § WASM-ENTFERNT).
// GUI-INSTRDBG-17/-17b: debug action instrumentation (name table, RAII timer,
// `instrument_gui_action!`). `#[macro_use]` re-exports the macro crate-wide.
#[macro_use]
mod gui_action;
// LRPAR-G14-DENOISE-IMPL-20 (GUI slice): Detail-section denoise controls +
// status badge; the stage itself runs in the shared core pipeline.
mod denoise_gui;
// GUI-INSTRDBG-17c-Rework F-A: the Detail-section denoise panel is extracted
// (file-size ratchet) while the enable checkbox gains its `GuiAction`.
mod denoise_panel;
// LRPAR-G09-CULL-25 (GUI slice): Library assisted-culling badges, filter and
// the explicit adopt action (source-level `document.culling` only).
mod cull_gui;
// LRPAR-G12-FACE-20 (FACE-20-S5): Library People view + confirm/split/merge.
mod face_gui;
// GUI-INSTRDBG-17c-Rest: the People view panel is extracted (file-size
// ratchet) while the per-face "Use as mask" bridge gains its `GuiAction`.
mod people_panel;
// LRPAR-G13-MERGE-15 (GUI slice): `merge-hdr`/`merge-pano` actions + DNG
// artifact status (same `lumina-merge` entry points as the CLI).
mod filmstrip;
mod i18n;
// LRPAR-G09-SORT-09: the Library-module strings (Filter/Compare/Survey, Stack,
// Sort) live here; `Str::t` delegates to them (file-size ratchet).
mod i18n_library;
// LRPAR-G03-MASKGROUP-03: the masking strings (G-03 + group panel) live here;
// `Str::t` delegates to them (file-size ratchet).
mod i18n_masking;
mod merge_gui;
// GPU-LENSFUN-PARITY-1 (GUI-Wiring): build the CPU `LensfunMap` from the strict
// auto-corrector and bind it on the `GpuContext`, so a corrector recipe presents
// through the readback-free VRAM path instead of the documented CPU exception.
#[cfg(all(feature = "gpu", feature = "lensfun"))]
mod lensfun_gpu;
// F-009: file-backed user presets (`<name>.lumina-preset.json`).
mod presets;
// UX-LOOK-HISTORY-18: the presets group tree (relative-folder grouping, own
// module for the file-size ratchet) and the readable history label /
// structured change diff.
mod history_changes;
mod preset_tree;
// LRPAR-G08-PREVIOUS / GUI-FILMSTRIP-SYNC-1: the filmstrip selection actions
// (Lightroom "Sync Settings", "Match Total Exposures", "Previous Image").
mod selection_actions;
// SIDECAR-REBASE-1: rebase a losing CAS save onto the current file instead of
// dropping it on a concurrent change (slider/crop/batch save paths).
mod sidecar_rebase;
// R3-RENDER-SIZE-1 (2026-09-20): the preview viewport cap (device-pixel budget
// for draft + full previews; export and the 1:1 loupe stay full resolution).
mod preview_size;
// R2-MODSWITCH-1 F8: the asynchronous folder scan (worker + main-thread drain)
// and the moved `scan_entry`/recursive driver.
mod library_scan;
// GUI-REFACTOR-W1-20 S1.1: the interactive draft render and the coalesced
// pointer-drag tick (Jank hot path) plus `DragTickTimings`.
mod render_tick;
// R2-JANK-1 F1/F4: frame-budget throttle for the draft render and cadence
// throttle for the draft analysis pass (pure, time-injected state).
mod draft_throttle;
// GUI-REFACTOR-W1-20 S1.3: recipe invalidation (`mark_dirty`/`mark_recipe_dirty`)
// and the single-adjustment default/reset path.
mod dirty;
// GUI-REFACTOR-W1-20 S1.4a: preview texture upload, the readback-free VRAM
// present path and the present target/source identity.
#[cfg(all(feature = "janklog", debug_assertions))]
mod jank_log;
mod present;
// GUI-REFACTOR-W1-20 S1.4b (GPU only): present routing, VRAM/stage gate,
// Lensfun map bind, refusal classification and the parity diagnostic hooks.
#[cfg(feature = "gpu")]
mod gpu_routing;
// GUI-REFACTOR-W1-20 S1.2a: source content hash + mask-plane loading.
mod render_source;
// GUI-REFACTOR-W1-20 S1.2b: the committed full-quality render entry points and
// the shared `render_from` pipeline hub (split: both files stay <= 500 lines).
mod render_entry;
mod render_pipeline;
// GUI-REFACTOR-W2-20 S2.1: the preview/canvas draw path (extracted verbatim).
mod preview_draws;
// GUI-REFACTOR-W2-20 S2.1: preview mask-tool interaction and the preview
// overlays (mask matte, G-11 pins, lens-blur/crop rects).
mod preview_masks;
// GUI-REFACTOR-W2-20 S2.2: the Develop section renderers, one module per
// F-100 section (plus the shared Basic-row helpers and the histogram).
mod develop_basic;
mod develop_color;
mod develop_detail;
mod develop_effects;
mod develop_generative;
mod develop_geometry;
mod develop_heal;
mod develop_histogram;
mod develop_masking;
mod develop_masking_g03;
// LRPAR-G03-MASKGROUP-03: Copy vs. Duplicate, mask groups + collapsible panel.
mod develop_masking_group_panel;
mod develop_masking_groups;
mod develop_optics;
mod develop_tone;
// GUI-REFACTOR-W2-20 S2.3: the Library folder tree/metadata/grid/loupe views.
mod library_grid;
mod library_metadata;
mod library_metadata_panel;
mod library_tree;
mod library_views;
// LRPAR-G15-STACK-15 (off-ratchet extraction): the Library `\`-filter
// predicates and the metadata batch-operation parser, re-exported below so
// existing call sites (`crate::library_entry_matches`, …) stay unchanged.
mod library_filter;
pub use library_filter::{
    collection_filter_matches_entry, library_entry_matches, library_filter_matches,
    parse_metadata_batch_op, CollectionFilter,
};
// LRPAR-G15-STACK-15: source-level image stacks (Grid/Filmstrip collapse,
// selection-as-unit, Sidecar-first persistence).
mod library_stacks;
// LRPAR-G09-SORT-09: Library sort modes + the portable custom-order file. The
// display-order methods (`raw_entry_indices`/`filtered_library_order`/
// `filmstrip_order`) moved here from `lib.rs` (file-size ratchet).
mod library_sort;
mod library_sort_file;
pub use library_sort::LibrarySort;
// GUI-REFACTOR-W2-20 S2.4-S2.8: Develop frame + ops sections, filmstrip frame,
// navigator rail and the top-level app-frame pieces (the `eframe::App::ui`
// frame stays at the crate root: > 500 lines, see its doc comment).
mod app_frame;
mod develop_frame;
// UX-LOOK-LAYOUT-18: the Develop left rail (own module, file-size ratchet).
mod develop_left_rail;
mod develop_ops;
mod filmstrip_frame;
// R2-MODSWITCH-1 F7: metadata-only folder preview index, the cache-aware
// thumbnail worker pool and the module-switch-aware render scheduler (split
// out of `lib.rs`, file-size ratchet).
mod render_schedule;
mod thumb_cache;
mod thumb_worker;
mod timing;
// R3-OPEN-1 / R3-WARMUP-1 (Release 1.0): Develop-switch selection open and the
// one-shot cold-start warmup (new logic in new files, file-size ratchet).
mod develop_open;
mod warmup;
// UX-LOOK-TOOLBAR-18: the icon tool strip + iconified Library view tabs (own
// module, file-size ratchet).
mod icon_toolbar;
mod navigator;
// GUI-REFACTOR-W2-20 S2.8: the preset/history, settings-clipboard, folder and
// snapshot ops.
mod ops_clipboard;
mod ops_folder;
mod ops_presets;
mod ops_snapshots;
// PREVIEW-CACHE-FEATURE: the neighbor-preview controller (worker pool + RAM/disk LRU).
mod preview_ctrl;
// R3-ROUTING-1/R3-DENOISE-1: neighbor-preview job planning + background worker.
mod preview_jobs;
mod slider;
mod theme;
mod viewport;

// LRPAR-MATRIX-RECIPE (Slice 2): the headless GUI matrix runner. Test-only: it drives
// `LuminaApp` on an egui context against the committed goldens and needs no window/GPU renderer.
#[cfg(test)]
mod matrix;

// GUI-INSTRDBG-17/-17b: re-exported instrumentation API (moved to `gui_action`).
#[cfg(debug_assertions)]
pub use gui_action::gui_action_log_line;
#[cfg(all(debug_assertions, test))]
pub(crate) use gui_action::take_gui_action_log;
#[cfg(debug_assertions)]
pub(crate) use gui_action::GuiActionTimer;
pub use gui_action::{
    GuiAction, ALL_GUI_ACTIONS, GPU_ROUTE_CPU_FALLBACK, GPU_ROUTE_NA, GPU_ROUTE_PRESENT,
};
// GUI-REFACTOR-W1-20 S1.1: `DragTickTimings` moved to `render_tick` (re-exported
// so the app-root field and headless tests keep the stable `lumina_gui` path).
pub use render_tick::DragTickTimings;

use draft_throttle::DraftThrottle;
use eframe::egui;
use lumina_core::cache::disk::DiskFolderCache;
use lumina_core::MaskPolicy;
// `export_image`/`ExportOptions` (Export module) and `rasterize_prompt` (mask overlay).
use lumina_core::{
    analyze_tone, analyze_tone_with_histogram, analyze_upright, apply_visualize_overlay,
    detect_red_eyes, detect_spots_heuristic, distraction_candidates, generative_input_frames,
    generative_variant_seed, has_transparent_pixels, match_total_exposure_masked,
    prepare_source_base, render_frame_from_base_with_generative_and_denoise, suggest_auto_tone,
    tone_fingerprint, upright_analysis, upright_input_fingerprint, AutoToneConfig, AutoToneResult,
    CacheStage, DetectedRedEye, DetectedSpot, DistractionKind, DistractionSetting,
    DistractionStatus, GenerativeCacheKey, GenerativeCanvasArtifact, GenerativeCanvasInput,
    GenerativeIdentity, ImageFileFormat, ImageFrame, LuminanceHistogram, MaskContext,
    MaskLayerResult, MaskPlane, OutputSpec, RenderContext, RenderKey, StageFrameCache, StageWork,
    RED_EYE_DETECT_ID_PREFIX,
};
use lumina_core::{
    export_image_with_generative, masks::rasterize_prompt, range_masks, ExportOptions,
};
use lumina_raw::RawError;
use lumina_sidecar::{apply_batch_op, validate_smart_collection_def, SMART_COLLECTION_VERSION};
use lumina_sidecar::{
    default_meta_presets_dir, document_revision, is_metadata_field, load_meta_preset_file,
    load_sidecar, now_rfc3339_utc, render_meta_preset, resolve_meta_preset_path,
    scan_meta_presets_dir, sidecar_path_for, validate_metadata_field_value,
    MAX_METADATA_HISTORY_ENTRIES, METADATA_FIELD_IDS,
};
use lumina_sidecar::{
    load_zdata, zdata_path_for, AiSelect, AiSelectKind, ArtifactStatus, BatchOp, BrushMark,
    BrushMarkSign, CollectionMembership, CoordinateSystem, DecodeFingerprint, GeometryFingerprint,
    HistoryEntry, MaskDefinition, MaskLayer, MaskOperation, MaskPrompt, MaskReference, MaskStatus,
    MetaPresetEntry, MetaPresetFile, MetadataHistoryEntry, ModelIdentity, Point2, Preprocessing,
    PromptTransform, Resolution, SidecarDocument, SmartCollectionDef, SmartRule, SourceFingerprint,
    SourceIdentity, SourceStatus, BW_STASH_KEY, DEVELOP_PROFILES, DEVELOP_PROFILE_KEY,
    TREATMENT_BW, TREATMENT_COLOR, TREATMENT_KEY,
};
use lumina_sidecar::{
    AnalysisFingerprint, AspectPreset, BokehShape, ColorGrading, ColorGradingRange, Crop,
    CurveChannels, CurvePoint, Curves, DenoiseModelIdentity, EditRecipe, Effects, Flag, FocusRect,
    GenerativeCanvas, GenerativeEdit, Geometry, Grain, HslAdjustments, HslChannel, LensBlur,
    LensCorrection, NoiseReduction, Perspective, PointColor, PointColorEntry, Presence, Preset,
    RedEyeCorrection, RedEyeRegion, Sharpening, SpotDistraction, Upright, Vignette,
    RED_EYE_MAX_REGIONS,
};
// GEN-ONNX-1 Welle 2b: the GUI resolves and (fixture-)produces the persisted
// `generative_canvas` artifact through the same documented ONNX/sidecar surface
// as the CLI: identical identity digest, identical capability/hash gate, the
// same durable `.lumina.zdata` record. The ONNX and core `GenerativeRole`
// enums are distinct layers and are aliased to keep that legible.
use lumina_onnx::{
    fixture_manifest, produce_canvas, GenerativeModelSource, GenerativeRole as OnnxGenerativeRole,
};
use lumina_sidecar::{
    generative_artifact_status, save_generative_canvas, GenerativeArtifactRef,
    GenerativeArtifactStatus, GenerativeCanvasArtifact as SidecarGenerativeCanvas,
};
// LRPAR-G15-IPTC-S8: read-only embedded IPTC display (JPEG IIM/XMP) in the
// Library Metadata panel. Display only — mutations always go through the
// sidecar draft helpers above (same path as the CLI).
use lumina_iptc::{extract_metadata, IptcMetadata};
use serde_json::Value;
use slider::{identity_spec, lr_slider, percent_spec, SliderAction, SliderSpec};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

// `debug!` is used by the thumbnail/decode paths.
use log::debug;
use log::{error, info, trace, warn};
use theme::apply_lightroom_dark;

// PERF-FILMSTRIP (thumbnail worker + navigator/preview paths).
use filmstrip::{downscale_rgba, ThumbnailManager};
use i18n::Str;
// R3-DENOISE-1 (B4): the crate root no longer renders directly; the bare
// `render_frame` re-export is only consumed by the headless test modules.
#[cfg(test)]
use lumina_core::render_frame;
// R2-MODSWITCH-1 F7: the cache-aware thumbnail job/result types (worker pool).
use thumb_worker::{ThumbnailJob, ThumbnailResult};

/// GEN-ONNX-1 Welle 2b: one resolved generative canvas plus the exact identity
/// digest of the run it belongs to. The digest is the
/// [`lumina_core::GenerativeCacheKey::digest`] the producing run published, so
/// a later render can prove the canvas still matches the current
/// source/recipe/seed/canvas/prompt before it is composited — a stale canvas is
/// never served silently.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedGenerativeCanvas {
    identity: String,
    artifact: GenerativeCanvasArtifact,
}

/// GEN-ONNX-1 Welle 2b: the caller-supplied generative canvases of one GUI
/// render (preview and export share this struct). Exactly one entry per active
/// role; the both-roles-at-once record keeps two entries (the SOLL allows a
/// single record to carry `auto_fill_transparent` + `expand_beyond_image`).
/// Each entry is pinned to its identity digest and re-validated on every
/// resolve, so a recipe/seed/canvas/prompt change invalidates it loudly instead
/// of compositing a stale canvas.
#[derive(Debug, Clone, Default)]
struct GenerativeArtifacts {
    auto_fill: Option<CachedGenerativeCanvas>,
    expand: Option<CachedGenerativeCanvas>,
}

impl GenerativeArtifacts {
    /// Borrow as the core render-hook input.
    fn input(&self) -> GenerativeCanvasInput<'_> {
        GenerativeCanvasInput {
            auto_fill: self.auto_fill.as_ref().map(|cached| &cached.artifact),
            expand: self.expand.as_ref().map(|cached| &cached.artifact),
        }
    }

    fn get(&self, role: OnnxGenerativeRole) -> Option<&CachedGenerativeCanvas> {
        match role {
            OnnxGenerativeRole::AutoFillTransparent => self.auto_fill.as_ref(),
            OnnxGenerativeRole::Expand => self.expand.as_ref(),
        }
    }

    fn set(&mut self, role: OnnxGenerativeRole, cached: CachedGenerativeCanvas) {
        match role {
            OnnxGenerativeRole::AutoFillTransparent => self.auto_fill = Some(cached),
            OnnxGenerativeRole::Expand => self.expand = Some(cached),
        }
    }

    /// The core role mirror of one ONNX role.
    fn core_role(role: OnnxGenerativeRole) -> lumina_core::GenerativeRole {
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
fn generative_record_id(identity_digest: &str) -> String {
    let prefix = identity_digest.get(..16).unwrap_or(identity_digest);
    format!("generative_canvas:{prefix}")
}

/// GEN-ONNX-1 Welle 2b: visible status of one generative role, mirroring the
/// sidecar vocabulary `valid` | `stale` | `missing` | `corrupt` (SOLL
/// `feature/product/generative-expand.md`, "Statuswerte wie bei AI-Masken").
/// Recorded by the resolver for the last resolved recipe identity, so the panel
/// can show *why* a role is not ready instead of only ready/missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GenerativeRoleStatus {
    #[default]
    Missing,
    Valid,
    Stale,
    Corrupt,
}

impl GenerativeRoleStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Corrupt => "corrupt",
        }
    }
}

/// Stable index of one generative role in the per-role status array.
fn generative_role_index(role: OnnxGenerativeRole) -> usize {
    match role {
        OnnxGenerativeRole::AutoFillTransparent => 0,
        OnnxGenerativeRole::Expand => 1,
    }
}

/// Work which may be performed when the GUI has no interactive input.
///
/// Queueing is deliberately separate from mask status: a missing/pending mask
/// is never inserted here implicitly.  The caller must enqueue it as the
/// result of an explicit user action (or a future CLI/GUI command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdleTask {
    MaskInference { mask_id: String },
    Thumbnail { source: PathBuf, name: String },
}

/// Top-level module selected in the module bar (Library / Develop / Export).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Library,
    Develop,
    Export,
}

/// Maps a Lightroom-style module-switch keyboard shortcut to its target module.
///
/// This is a pure function so the mapping can be unit-tested without an
/// [`egui::Context`]. The mapping mirrors Lightroom's module keys:
///
/// * `G` switches to `Library` (Grid).
/// * `D` switches to `Develop`.
/// * `E` is Lightroom's "Loupe" shortcut. Lumina has no separate Loupe module,
///   so `E` is treated as an alias for `Library` (documented here so the alias
///   is intentional and not a silent fallback).
///
/// Keys that are not module shortcuts — in particular the existing `Y`
/// Before/After toggle and `Esc` eyedropper-cancel — return `None` and keep
/// their own, separate handling.
pub fn module_for_key(key: egui::Key) -> Option<Module> {
    match key {
        egui::Key::G => Some(Module::Library),
        egui::Key::D => Some(Module::Develop),
        egui::Key::E => Some(Module::Library),
        _ => None,
    }
}

/// Maps a number key to a Lightroom-style star rating (LR-01).
///
/// `1`–`5` set the rating of the active virtual copy, `0` clears it back to
/// unrated. This is a pure function so the mapping can be unit-tested without
/// an [`egui::Context`]. Note this intentionally shadows the previous zoom
/// bindings on `Num1`/`Num2` (1:1/2:1 stay reachable through the preview
/// toolbar buttons); ratings are the documented MVP priority (gap plan
/// LR-01) and sharing the keys would make one of the two a silent victim.
pub fn rating_for_key(key: egui::Key) -> Option<u8> {
    match key {
        egui::Key::Num0 => Some(0),
        egui::Key::Num1 => Some(1),
        egui::Key::Num2 => Some(2),
        egui::Key::Num3 => Some(3),
        egui::Key::Num4 => Some(4),
        egui::Key::Num5 => Some(5),
        _ => None,
    }
}

/// Maps a key to a Lightroom-style pick flag (LR-01): `P` pick, `X` reject,
/// `U` unflag. Pure function, unit-tested without an [`egui::Context`].
pub fn flag_for_key(key: egui::Key) -> Option<Flag> {
    match key {
        egui::Key::P => Some(Flag::Pick),
        egui::Key::X => Some(Flag::Reject),
        egui::Key::U => Some(Flag::Unflagged),
        _ => None,
    }
}

/// Maps a number key to a Lightroom-style color label (Welle 2, LR-17 light):
/// `6`–`9` select label `1`–`4` (red/yellow/green/blue, see
/// [`color_label_name`]), stored in the active copy's `extras["color_label"]`
/// so no sidecar schema change is needed. Pure function, unit-tested without
/// an [`egui::Context`].
pub fn color_label_for_key(key: egui::Key) -> Option<u8> {
    match key {
        egui::Key::Num6 => Some(1),
        egui::Key::Num7 => Some(2),
        egui::Key::Num8 => Some(3),
        egui::Key::Num9 => Some(4),
        _ => None,
    }
}

/// User-visible name of a color label (`0` = none). Routed through [`Str`] so
/// no panel carries a free-form literal.
pub fn color_label_name(label: u8) -> &'static str {
    match label {
        1 => Str::ColorRed.t(),
        2 => Str::ColorYellow.t(),
        3 => Str::ColorGreen.t(),
        4 => Str::ColorBlue.t(),
        _ => Str::ColorLabel.t(),
    }
}

/// Read a color label (`0..=4`, `0` = none) from a virtual copy's `extras`
/// map. Missing, non-numeric or out-of-range values read as `0` (none): the
/// field is a forward-compatible cosmetic annotation, while the strict
/// `0..=4` gate lives on the [`LuminaApp::set_color_label`] write path.
/// Shared by the Library scan and the rating section so both read one path.
pub fn color_label_of(extras: &BTreeMap<String, serde_json::Value>) -> u8 {
    extras
        .get("color_label")
        .and_then(serde_json::Value::as_u64)
        .filter(|&n| n <= 4)
        .unwrap_or(0) as u8
}

/// Copy/paste-settings clipboard action (Welle 2, LR-09): `Cmd/Ctrl+Shift+C`
/// copies the session recipe, `Cmd/Ctrl+Shift+V` pastes it onto the active
/// virtual copy. Pure function, unit-tested without an [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardAction {
    Copy,
    Paste,
}

pub fn clipboard_action_for_key(
    key: egui::Key,
    command: bool,
    shift: bool,
) -> Option<ClipboardAction> {
    if !(command && shift) {
        return None;
    }
    match key {
        egui::Key::C => Some(ClipboardAction::Copy),
        egui::Key::V => Some(ClipboardAction::Paste),
        _ => None,
    }
}

/// Display-only Develop view toggle (Welle 2): `V` black-&-white treatment
/// (recipe-backed, restores on second press), `J` clipping warnings (badge
/// computed from preview pixels), `L` lights-out (hides side panels and the
/// filmstrip, header stays). Pure function, unit-tested without an
/// [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewToggle {
    BlackWhite,
    Clipping,
    LightsOut,
}

pub fn view_toggle_for_key(key: egui::Key) -> Option<ViewToggle> {
    match key {
        egui::Key::V => Some(ViewToggle::BlackWhite),
        egui::Key::J => Some(ViewToggle::Clipping),
        egui::Key::L => Some(ViewToggle::LightsOut),
        _ => None,
    }
}

/// Panel-visibility toggle (Welle 2): `R` arms/disarms the crop mode badge
/// (edits stay in the Geometry Crop controls), `Tab` hides/shows the side
/// panels (the filmstrip stays; `L` lights-out hides that too). Pure
/// function, unit-tested without an [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelToggle {
    CropMode,
    PanelsHidden,
}

pub fn panel_toggle_for_key(key: egui::Key) -> Option<PanelToggle> {
    match key {
        egui::Key::R => Some(PanelToggle::CropMode),
        egui::Key::Tab => Some(PanelToggle::PanelsHidden),
        _ => None,
    }
}

/// All-panels toggle (G-11, LRPAR-G11-OVERLAYS): `Shift+Tab` hides/shows the
/// side panels, the navigator rail AND the filmstrip (header/module bar and
/// preview stay). Plain `Tab` keeps the filmstrip (see [`panel_toggle_for_key`]);
/// the shift-aware dispatch in `update` prefers this branch. Pure function,
/// unit-tested without an [`egui::Context`].
pub fn all_panels_toggle_for_key(key: egui::Key, shift: bool) -> bool {
    matches!(key, egui::Key::Tab) && shift
}

/// Tool-overlay mode (G-11): how the mask-matte overlay behaves for the
/// masking/retouch tools. Global across tools (one predictable switch, F-100
/// G-11 SOLL). `Always` is the historical behaviour (overlay whenever a prompt
/// exists) and therefore the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayMode {
    #[default]
    Always,
    Auto,
    Never,
}

/// Edit-pin visibility (G-11): whether numbered edit pins (mask anchors + spot
/// centres) are painted. Default `Auto` (pins only while a tool is armed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinVisibility {
    Always,
    #[default]
    Auto,
    Never,
}

/// User-visible name of an [`OverlayMode`], routed through [`Str`] so no panel
/// carries a free-form literal.
pub fn overlay_mode_name(mode: OverlayMode) -> &'static str {
    match mode {
        OverlayMode::Always => Str::OverlayAlways.t(),
        OverlayMode::Auto => Str::OverlayAuto.t(),
        OverlayMode::Never => Str::OverlayNever.t(),
    }
}

/// User-visible name of a [`PinVisibility`], routed through [`Str`].
pub fn pin_visibility_name(visibility: PinVisibility) -> &'static str {
    match visibility {
        PinVisibility::Always => Str::OverlayAlways.t(),
        PinVisibility::Auto => Str::OverlayAuto.t(),
        PinVisibility::Never => Str::OverlayNever.t(),
    }
}

/// User-visible name of an [`AiSelectKind`] (G-03), routed through [`Str`].
/// Covers every variant (DoD §3: no sampling over the class).
pub fn ai_select_kind_name(kind: AiSelectKind) -> &'static str {
    match kind {
        AiSelectKind::Subject => Str::AiSubject.t(),
        AiSelectKind::Sky => Str::AiSky.t(),
        AiSelectKind::Background => Str::AiBackground.t(),
        AiSelectKind::Objects => Str::AiObjects.t(),
        AiSelectKind::People => Str::AiPeople.t(),
    }
}

/// Number of F-100 Develop sections in [`LuminaApp::DEVELOP_SECTIONS`] order
/// (Basic, Tone Curve, Color, Detail, Effects, Optics, Geometry, Masking):
/// the G-11 solo-mode scope.
pub const SECTION_COUNT: usize = 8;
pub const SECTION_BASIC: usize = 0;
pub const SECTION_TONE_CURVE: usize = 1;
pub const SECTION_COLOR: usize = 2;
pub const SECTION_DETAIL: usize = 3;
pub const SECTION_EFFECTS: usize = 4;
pub const SECTION_OPTICS: usize = 5;
pub const SECTION_GEOMETRY: usize = 6;
pub const SECTION_MASKING: usize = 7;

/// LRPAR-G01-BASIC: flat recipe keys owned by the Basic section (panel-
/// Previous/Reset scope). `vibrance`/`saturation` live in the Color section
/// and are only touched by Basic through the B&W treatment stash path.
const BASIC_TONE_KEYS: &[&str] = &[
    "wb_temperature",
    "wb_tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
];

/// F-100 label of a Develop section index (G-11, routed through [`Str`]).
/// `None` for out-of-range indices.
pub fn section_name(index: usize) -> Option<&'static str> {
    match index {
        SECTION_BASIC => Some(Str::Basic.t()),
        SECTION_TONE_CURVE => Some(Str::ToneCurve.t()),
        SECTION_COLOR => Some(Str::Color.t()),
        SECTION_DETAIL => Some(Str::Detail.t()),
        SECTION_EFFECTS => Some(Str::Effects.t()),
        SECTION_OPTICS => Some(Str::Optics.t()),
        SECTION_GEOMETRY => Some(Str::Geometry.t()),
        SECTION_MASKING => Some(Str::Masking.t()),
        _ => None,
    }
}

/// What an edit pin marks (G-11): a mask-library entry or a spot heal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPinKind {
    Mask,
    Spot,
}

/// One headless-testable edit pin (G-11): normalized source-space anchor
/// (`0..=1`), selection flag and a stable id. The painter loop paints exactly
/// this list, so pin behaviour is covered without pixel assertions.
#[derive(Debug, Clone, PartialEq)]
pub struct EditPin {
    pub id: String,
    pub label: String,
    pub pos: (f32, f32),
    pub selected: bool,
    pub kind: EditPinKind,
}

/// Derive a pin anchor (normalized source space) from a mask prompt (G-11).
/// Box → rect centre; Brush → first mark; Polygon → first vertex; Ellipse →
/// centre; Gradient → midpoint of the start→end stretch along `angle_deg`
/// around the frame centre, clamped to `0..=1`. Prompts without geometry
/// (empty brush/polygon, G-03 range stages) yield `None`: no pin instead of
/// an invented position.
/// Pure function, unit-tested headless.
pub fn pin_anchor_for_prompt(prompt: &MaskPrompt) -> Option<(f32, f32)> {
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    match prompt {
        MaskPrompt::Box { rect, .. } => {
            if !rect.x.is_finite()
                || !rect.y.is_finite()
                || !rect.width.is_finite()
                || !rect.height.is_finite()
            {
                return None;
            }
            Some((
                clamp01(rect.x + rect.width / 2.0),
                clamp01(rect.y + rect.height / 2.0),
            ))
        }
        MaskPrompt::Brush { marks, .. } => {
            let first = marks.first()?;
            if !first.x.is_finite() || !first.y.is_finite() {
                return None;
            }
            Some((clamp01(first.x), clamp01(first.y)))
        }
        MaskPrompt::Polygon { points, .. } => {
            let first = points.first()?;
            if !first.x.is_finite() || !first.y.is_finite() {
                return None;
            }
            Some((clamp01(first.x), clamp01(first.y)))
        }
        MaskPrompt::Ellipse { center, .. } => {
            if !center.x.is_finite() || !center.y.is_finite() {
                return None;
            }
            Some((clamp01(center.x), clamp01(center.y)))
        }
        MaskPrompt::Gradient {
            angle_deg,
            start,
            end,
            ..
        } => {
            if !angle_deg.is_finite() || !start.is_finite() || !end.is_finite() {
                return None;
            }
            let mid = (start + end) / 2.0;
            let radians = angle_deg.to_radians();
            Some((
                clamp01(0.5 + radians.cos() * (mid - 0.5)),
                clamp01(0.5 + radians.sin() * (mid - 0.5)),
            ))
        }
        // G-03 range stages are parameter boxes, not spatial prompts: no pin.
        MaskPrompt::ColorRange { .. } | MaskPrompt::LuminanceRange { .. } => None,
    }
}

/// Library compare/survey view (Welle 3, LR-20 light): `C` shows the
/// full-frame Before image through the existing [`LuminaApp::before_after`]
/// path (compare proxy), `N` jumps to the Library grid (survey proxy over
/// the file-browser entries). Pure function, unit-tested without an
/// [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareMode {
    Compare,
    Survey,
}

pub fn compare_mode_for_key(key: egui::Key) -> Option<CompareMode> {
    match key {
        egui::Key::C => Some(CompareMode::Compare),
        egui::Key::N => Some(CompareMode::Survey),
        _ => None,
    }
}

/// Library view (G-09, LRPAR-G09-LIB): the four Library views sharing one
/// selection (`filmstrip_selection`) and one filter (`\` query + active
/// collection). `Grid` is the default thumbnail raster; `Loupe` shows the
/// active selection large; `Compare` shows the active image's Before/After
/// proxy (existing `before_after` path); `Survey` shows the multi-selection
/// side by side (falls back to the filtered raster below two selections).
/// Pure display state — never recipe/sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryView {
    #[default]
    Grid,
    Loupe,
    Compare,
    Survey,
    /// LRPAR-G12-FACE-20 (S5): the Library People view (cluster/person list and
    /// the confirm/split/merge actions). Display-only selection state; reached
    /// through the Library view selector (no new global shortcut is reserved —
    /// FACE-20 §3).
    People,
}

/// Maps a Lightroom-style Library view key to its view (G-09): `G` grid,
/// `E` loupe (module alias documented in [`module_for_key`]), `C` compare,
/// `N` survey. Pure function, unit-tested without an [`egui::Context`].
pub fn library_view_for_key(key: egui::Key) -> Option<LibraryView> {
    match key {
        egui::Key::G => Some(LibraryView::Grid),
        egui::Key::E => Some(LibraryView::Loupe),
        egui::Key::C => Some(LibraryView::Compare),
        egui::Key::N => Some(LibraryView::Survey),
        _ => None,
    }
}

/// Clamped Library raster navigation (G-09): move `current` by `delta`
/// entries over `count` entries. Clamps at both ends (Lightroom-conform, no
/// wrap); empty listings and out-of-range starts clamp to `0`. Pure
/// function, unit-tested headless.
pub fn library_move_index(current: usize, delta: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let current = current.min(count - 1);
    current.saturating_add_signed(delta).min(count - 1)
}

/// Move one file to `target`, tolerating a cross-filesystem move: `rename`
/// fails with `CrossesDevices` (EXDEV) when source and target live on
/// different volumes, so fall back to copy + remove. Loud on error; a failed
/// source removal cleans up the copied target again (the source still exists,
/// so no data is lost).
fn move_file_cross_volume(source: &Path, target: &Path) -> std::io::Result<()> {
    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(source, target)?;
            if let Err(remove_error) = std::fs::remove_file(source) {
                let _ = std::fs::remove_file(target);
                return Err(remove_error);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Import/export module shortcut (Welle 3, LR-13 light):
/// `Cmd/Ctrl+Shift+I` jumps to Library (import lives there),
/// `Cmd/Ctrl+Shift+E` jumps to Export. The shortcuts only switch the module
/// and announce it via the status line — file dialogs and the actual export
/// stay manual. Pure function, unit-tested without an [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportExportAction {
    Import,
    Export,
}

pub fn import_export_for_key(
    key: egui::Key,
    command: bool,
    shift: bool,
) -> Option<ImportExportAction> {
    if !(command && shift) {
        return None;
    }
    match key {
        egui::Key::I => Some(ImportExportAction::Import),
        egui::Key::E => Some(ImportExportAction::Export),
        _ => None,
    }
}

/// Power-shortcut rest (G-16, LRPAR-G16-POWER): `Shift`+double-click on a
/// slider label applies the auto end point of exactly that slider, reusing the
/// existing `suggest_auto_tone` path (no second algorithm). Scope decision
/// (Lightroom-conform, F-100): only `whites` (auto white point) and `blacks`
/// (auto black point) map; every other key — and any press without `Shift` or
/// without a double-click — yields `None` (callers fall back to the normal
/// single-control reset). Pure function, unit-tested without an
/// [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoEndpoint {
    White,
    Black,
}

pub fn auto_endpoint_for_slider(
    key: &str,
    shift: bool,
    double_clicked: bool,
) -> Option<AutoEndpoint> {
    if !(shift && double_clicked) {
        return None;
    }
    match key {
        "whites" => Some(AutoEndpoint::White),
        "blacks" => Some(AutoEndpoint::Black),
        _ => None,
    }
}

/// Power-shortcut rest (G-16): tone-slider scope of the `Alt`+slider masking
/// preview. While `Alt` is held during a track/scroll value edit on one of
/// these Basic tone keys, the preview header shows the clipping badge through
/// the existing `J` clipping path (display-only, never recipe). Pure function,
/// unit-tested headless. Note the disambiguation: `Alt`-click on the *label*
/// stays the single-control reset (see `label_reset_requested`), `Alt`-scroll
/// stays the fine step — the preview is purely additive.
pub fn masking_preview_for_slider(key: &str) -> bool {
    matches!(
        key,
        "exposure" | "contrast" | "highlights" | "shadows" | "whites" | "blacks"
    )
}

/// Power-shortcut rest (G-16): plain `S` toggles the display-only softproof
/// preview. This reserves the `S` binding claimed by LRPAR-G10-VIEWER (still
/// open) instead of blocking it: the toggle is display-only, the full
/// print/gamut simulation stays G-10 follow-up work. Any modifier (Ctrl/Cmd
/// for copy-settings-adjacent chords, Alt for the `Cmd/Ctrl+Alt+S` snapshot,
/// Shift) yields `false`, so no existing chord is hijacked. Pure function,
/// unit-tested without an [`egui::Context`].
pub fn softproof_for_key(key: egui::Key, ctrl_or_command: bool, alt: bool, shift: bool) -> bool {
    matches!(key, egui::Key::S) && !ctrl_or_command && !alt && !shift
}

// GUI-INSTRDBG-17/-17b: the debug action instrumentation core (GuiAction
// table, RAII `GuiActionTimer`, `instrument_gui_action!`) now lives in
// `gui_action.rs` (extracted to respect the file-size ratchet); the macro
// is imported with `#[macro_use]` below.

/// Build one [`SmartRule`] from the Library smart-editor inputs (G-15
/// META-MVP, Slice 3). `kind` is one of `all`, `none`, `keyword`
/// (`value` = exact case-sensitive keyword), `rating_at_least`,
/// `rating_equals` (`value` = `0..=5`), `flag` (`value` =
/// `pick|reject|unflagged`). Anything else is a loud `Err`. Pure function,
/// unit-tested headless; `And`/`Or`/`Not` composition happens on the
/// caller-held rule stack ([`combine_smart_rules`]).
pub fn build_smart_rule(kind: &str, value: &str) -> Result<SmartRule, String> {
    match kind {
        "all" => Ok(SmartRule::All),
        "none" => Ok(SmartRule::None),
        "keyword" => Ok(SmartRule::Keyword {
            keyword: value.to_string(),
        }),
        "rating_at_least" => {
            let rating: u8 = value
                .trim()
                .parse()
                .map_err(|_| format!("invalid rating `{value}`: expected 0..=5"))?;
            if rating > 5 {
                return Err(format!("invalid rating `{value}`: expected 0..=5"));
            }
            Ok(SmartRule::RatingAtLeast { rating })
        }
        "rating_equals" => {
            let rating: u8 = value
                .trim()
                .parse()
                .map_err(|_| format!("invalid rating `{value}`: expected 0..=5"))?;
            if rating > 5 {
                return Err(format!("invalid rating `{value}`: expected 0..=5"));
            }
            Ok(SmartRule::RatingEquals { rating })
        }
        "flag" => {
            let flag = match value.trim().to_lowercase().as_str() {
                "pick" => Flag::Pick,
                "reject" => Flag::Reject,
                "unflagged" => Flag::Unflagged,
                _ => {
                    return Err(format!(
                        "invalid flag `{value}`: expected pick|reject|unflagged"
                    ));
                }
            };
            Ok(SmartRule::Flag { flag })
        }
        _ => Err(format!("unknown smart-rule kind `{kind}`")),
    }
}

/// Combine caller-held [`SmartRule`] stack entries (G-15 META-MVP, Slice 3):
/// `and`/`or` need ≥2 entries, `not` needs ≥1 (pops it, pushes the
/// negation). Underflow is a loud `Err`. Pure function, unit-tested
/// headless.
pub fn combine_smart_rules(stack: &mut Vec<SmartRule>, op: &str) -> Result<(), String> {
    match op {
        "and" | "or" => {
            if stack.len() < 2 {
                return Err(format!("cannot combine `{op}`: need at least 2 rules"));
            }
            let rules = std::mem::take(stack);
            stack.push(if op == "and" {
                SmartRule::And { rules }
            } else {
                SmartRule::Or { rules }
            });
            Ok(())
        }
        "not" => {
            let rule = stack
                .pop()
                .ok_or_else(|| "cannot negate: no rule on the stack".to_string())?;
            stack.push(SmartRule::Not {
                rule: Box::new(rule),
            });
            Ok(())
        }
        _ => Err(format!("unknown smart-rule combinator `{op}`")),
    }
}

/// Read the stack-group proxy id (Welle 3, LR-17 light) from a virtual
/// copy's `extras["stack_group"]` — no sidecar schema change. Missing,
/// non-string or empty values read as `None`: the field is a
/// forward-compatible grouping annotation, while the write path
/// ([`LuminaApp::toggle_stack_group`]) is the only place ids are minted.
/// Shared by the toggle and headless tests so both read one path.
pub fn stack_id_of(extras: &BTreeMap<String, serde_json::Value>) -> Option<String> {
    extras
        .get("stack_group")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Format marker of the portable smart-collection catalog file (G-15
/// META-MVP, Slice 3) — identical to the CLI (`SmartCatalogFile` there), so
/// both sides read and write the same bytes. The file carries versioned
/// rule data only, never absolute paths.
pub const SMART_CATALOG_FORMAT: &str = "lumina-smart-catalog";

/// Portable smart-collection catalog file (G-15 META-MVP, Slice 3),
/// CLI-identical envelope. Serialized with `version =
/// SMART_COLLECTION_VERSION` and validated per definition with
/// `validate_smart_collection_def` on load and before save.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SmartCatalogFile {
    format: String,
    version: u8,
    collections: Vec<SmartCollectionDef>,
}

/// The default virtual copy id of `document` (first copy when no default is
/// flagged); `None` only when the document carries no copies. Used to
/// resolve selector batch ops (`SetRating`/`SetFlag` with empty `copy_id`)
/// per target file.
fn default_copy_id(document: &SidecarDocument) -> Option<String> {
    document
        .virtual_copies
        .iter()
        .find(|copy| copy.is_default)
        .or_else(|| document.virtual_copies.first())
        .map(|copy| copy.id.clone())
}

/// LRPAR-G15-IPTC-S8: default field selection for
/// [`LuminaApp::sync_metadata_to_selection`]: every registry draft field plus
/// `keywords` (SOLL §10: Default alle Draft-Felder + Keywords). Unchecked
/// fields are left untouched on the targets (no mirror for them).
fn default_meta_sync_fields() -> BTreeMap<String, bool> {
    let mut fields = BTreeMap::new();
    for id in METADATA_FIELD_IDS {
        fields.insert((*id).to_string(), true);
    }
    fields.insert("keywords".to_string(), true);
    fields
}

/// LRPAR-G15-IPTC-S8: user-visible label of a registry draft field ID,
/// routed through [`Str`] so no panel carries a free-form literal. `None`
/// for unknown IDs (callers reject those loudly before painting).
fn metadata_field_label(field: &str) -> Option<&'static str> {
    match field {
        "title" => Some(Str::MetadataFieldTitle.t()),
        "headline" => Some(Str::MetadataFieldHeadline.t()),
        "description" => Some(Str::MetadataFieldDescription.t()),
        "copyright_notice" => Some(Str::MetadataFieldCopyrightNotice.t()),
        "creator" => Some(Str::MetadataFieldCreator.t()),
        "credit" => Some(Str::MetadataFieldCredit.t()),
        "source" => Some(Str::MetadataFieldSource.t()),
        "city" => Some(Str::MetadataFieldCity.t()),
        "state_province" => Some(Str::MetadataFieldStateProvince.t()),
        "country" => Some(Str::MetadataFieldCountry.t()),
        "date_created" => Some(Str::MetadataFieldDateCreated.t()),
        _ => None,
    }
}

/// LRPAR-G15-IPTC-S8: maps a registry field ID onto the embedded value of a
/// JPEG (display-only, same mapping as the CLI `meta inspect`; IDs come from
/// `METADATA_FIELD_IDS`, no second registry).
fn embedded_field_value<'a>(meta: &'a IptcMetadata, id: &str) -> Option<&'a str> {
    match id {
        "title" => meta.title.as_deref(),
        "headline" => meta.headline.as_deref(),
        "description" => meta.description.as_deref(),
        "copyright_notice" => meta.copyright_notice.as_deref(),
        "creator" => meta.creator.as_deref(),
        "credit" => meta.credit.as_deref(),
        "source" => meta.source.as_deref(),
        "city" => meta.city.as_deref(),
        "state_province" => meta.state_province.as_deref(),
        "country" => meta.country.as_deref(),
        "date_created" => meta.date_created.as_deref(),
        _ => None,
    }
}

/// Shadow/highlight clipping fractions (`0..=1`) of a frame: a pixel counts
/// as shadow-clipped when all channels are `0`, as highlight-clipped when
/// all are `255`. Pure display diagnostic for the `J` overlay badge — it
/// never feeds back into the recipe or render. Empty frames report `(0, 0)`.
pub fn clip_fractions(frame: &ImageFrame) -> (f64, f64) {
    let total = (frame.width as usize) * (frame.height as usize);
    if total == 0 || frame.pixels.len() < total * 4 {
        return (0.0, 0.0);
    }
    let mut shadow = 0usize;
    let mut highlight = 0usize;
    for px in frame.pixels.chunks_exact(4) {
        if px[0] == 0 && px[1] == 0 && px[2] == 0 {
            shadow += 1;
        } else if px[0] == 255 && px[1] == 255 && px[2] == 255 {
            highlight += 1;
        }
    }
    (
        shadow as f64 / total as f64,
        highlight as f64 / total as f64,
    )
}

/// Normalized L1 distance of two equal-length histograms (`0` = identical
/// distributions, `2` = disjoint). Scale-free so a full-res preview and the
/// unedited decode compare directly. Returns `None` on length mismatch or
/// empty inputs — never a silent `0`. Pure function, unit-tested headless;
/// feeds the G-10 "Original Photo" histogram delta.
pub fn normalized_histogram_l1(a: &[u64], b: &[u64]) -> Option<f64> {
    if a.len() != b.len() {
        return None;
    }
    let sum_a: u64 = a.iter().sum();
    let sum_b: u64 = b.iter().sum();
    if sum_a == 0 || sum_b == 0 {
        return None;
    }
    Some(
        a.iter()
            .zip(b.iter())
            .map(|(&x, &y)| (x as f64 / sum_a as f64 - y as f64 / sum_b as f64).abs())
            .sum(),
    )
}
/// Maps a key (+ Shift state) to an interactive masking tool (LR-10):
/// `K` brush, `M` linear gradient, `Shift+M` radial gradient. Pure function,
/// unit-tested without an [`egui::Context`]. Arming itself still goes through
/// [`LuminaApp::set_mask_tool`] so the geometry block stays enforced.
pub fn mask_tool_for_key(key: egui::Key, shift: bool) -> Option<MaskTool> {
    match (key, shift) {
        (egui::Key::K, _) => Some(MaskTool::Brush),
        (egui::Key::M, false) => Some(MaskTool::LinearGradient),
        (egui::Key::M, true) => Some(MaskTool::Radial),
        _ => None,
    }
}

/// Renders a star rating as a fixed-width 5-glyph badge (LR-01), e.g.
/// `3 → "★★★☆☆"`, `0 → "☆☆☆☆☆"`. Pure display helper shared by the Library
/// grid badge and the rating section; unit-tested headless.
pub fn stars_for_rating(rating: u8) -> String {
    let rating = rating.min(5) as usize;
    "★".repeat(rating) + &"☆".repeat(5 - rating)
}

/// User-visible label for a pick flag (LR-01), routed through [`Str`] so no
/// panel carries a free-form literal.
pub fn flag_label(flag: Flag) -> &'static str {
    match flag {
        Flag::Pick => Str::Pick.t(),
        Flag::Reject => Str::Reject.t(),
        Flag::Unflagged => Str::Unflagged.t(),
    }
}

/// Active interactive masking tool (F-103-N4). `None` means the preview accepts
/// the ordinary click/eyedropper interactions; any other variant arms the
/// preview for a drag gesture that builds a [`MaskPrompt`] for the selected
/// mask.  The tool only chooses *how* the drag is interpreted; persistence goes
/// through the existing sidecar paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpotTool {
    #[default]
    None,
    Heal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpotMode {
    #[default]
    Heuristic,
    Generative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskTool {
    #[default]
    None,
    Brush,
    LinearGradient,
    Radial,
}

/// Preview zoom behaviour (Lightroom-like). `Fit` is object-contain (the
/// previous default); the absolute modes resolve to an effective scale derived
/// from the pane each frame so they survive window resizes. `Custom` is set by
/// modifier-wheel / `+/-` zoom and pins an explicit relative-to-fit multiplier
/// that is no longer re-derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    Fit,
    /// 25 % effective scale (relative-to-fit `0.25 / fit`).
    Quarter,
    /// 50 % effective scale (relative-to-fit `0.5 / fit`).
    Half,
    /// 75 % effective scale (relative-to-fit `0.75 / fit`).
    ThreeQuarter,
    OneToOne,
    TwoHundred,
    FitWidth,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedIdleTask {
    id: u64,
    priority: u8,
    task: IdleTask,
}

/// Small, bounded priority queue for work that is safe to defer until idle.
#[derive(Debug, Clone)]
pub struct IdleQueue {
    capacity: usize,
    next_id: u64,
    tasks: Vec<QueuedIdleTask>,
}

impl IdleQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next_id: 0,
            tasks: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Maximum number of queued tasks before [`enqueue`](Self::enqueue) starts
    /// dropping jobs.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Enqueue only explicit requests. Returns a stable cancellation handle.
    pub fn enqueue(&mut self, task: IdleTask, priority: u8) -> Option<u64> {
        if self.tasks.len() >= self.capacity {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.tasks.push(QueuedIdleTask { id, priority, task });
        Some(id)
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        self.tasks.len() != before
    }

    /// Takes the highest-priority task. Equal priorities retain FIFO order
    /// (REVIEW-GUI-N4): ties are broken by the monotonically increasing
    /// enqueue id, so the *first*-enqueued task of a priority class wins.
    /// (`Iterator::max_by_key` alone would pick the *last* maximum — LIFO.)
    pub fn pop_next(&mut self) -> Option<(u64, IdleTask)> {
        let index = self
            .tasks
            .iter()
            .enumerate()
            .min_by_key(|(_, task)| (std::cmp::Reverse(task.priority), task.id))?
            .0;
        let task = self.tasks.remove(index);
        Some((task.id, task.task))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GuiError {
    #[error("{0}")]
    Core(#[from] lumina_core::CoreError),
    #[error("{0}")]
    Sidecar(#[from] lumina_sidecar::SidecarError),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Raw(#[from] RawError),
}

pub struct LuminaApp {
    original: Option<ImageFrame>,
    preview: Option<ImageFrame>,
    source_bytes: Option<Vec<u8>>,
    source_is_raw: bool,
    raw_orientation: u8,
    camera_white_balance: Option<[f32; 4]>,
    /// EXIF lens identity of the loaded source (G-06): snapshot from the
    /// decoder metadata at load time (`None` for raster sources / missing
    /// EXIF). Drives the Lensfun auto-profile resolution (status line +
    /// render corrector). Reset on every source change (REVIEW-GUI-N3).
    loaded_lens_identity: Option<LensIdentity>,
    /// Cached Lensfun auto-corrector for the loaded source (G-06, only
    /// with the `lensfun` feature): rebuilt when the identity/dimensions
    /// key changes, reused across preview/export renders so the system DB
    /// is not re-loaded per slider tick. `None` = no profile applies
    /// (manual model) or no EXIF. Never crosses threads (preview,
    /// navigator and export render on the UI thread; thumbnails render
    /// without auto-lens by design — documented in the G-06 status).
    #[cfg(feature = "lensfun")]
    lensfun_cache: Option<CachedLensCorrector>,
    source_name: String,
    path: String,
    directory: String,
    entries: Vec<FileBrowserEntry>,
    recipe: EditRecipe,
    /// GEN-ONNX-1 Welle 2b: the generative canvas artifacts the render hook
    /// feeds into the shared pipeline for the current source/copy. Populated by
    /// the explicit `Generative Expand → Generieren` action (fixture model
    /// pre-integration) and/or by loading the persisted `generative_canvas`
    /// record for the current identity. Every active role without a matching
    /// artifact is a loud render error — never a silent "as if not generated".
    generative_artifacts: GenerativeArtifacts,
    /// GEN-ONNX-1 Welle 2b: per-role status (`valid`/`stale`/`missing`/
    /// `corrupt`) recorded by the resolver for the current source/recipe, so the
    /// Generative panel can name the precise cause. Indexed by
    /// [`generative_role_index`].
    generative_role_status: [GenerativeRoleStatus; 2],
    /// Memo key (resolved source hash + recipe digest) for which
    /// [`Self::generative_artifacts`] were last resolved. Recomputing the
    /// generative input frames is a full pipeline head, so an unchanged
    /// source/recipe reuses the resolved canvases without re-hashing the frames.
    /// A stale key never serves a canvas: the per-role identity digest is still
    /// compared on every resolve.
    generative_memo: Option<String>,
    texture: Option<egui::TextureHandle>,
    /// R2-GUIMOD-02: identity of the pixels currently held by
    /// [`Self::texture`] — `(preview generation, before_after, [w, h])`.
    /// The CPU present path re-uploads only when this differs from what would
    /// be displayed, instead of rebuilding a full-screen [`egui::ColorImage`]
    /// and re-creating the texture on every repaint (mousemoves over panels
    /// used to pay a full-frame memcpy + upload per frame).
    texture_identity: Option<(u64, bool, [usize; 2])>,
    /// KITTEST-COVERAGE-OVERLAYS-1: retained texture for the CPU mask-overlay
    /// matte painted by [`Self::draw_mask_overlay`]. Previously the matte was
    /// uploaded with a per-frame `load_texture` into a local handle that was
    /// dropped at the end of the same frame; egui then queued the texture `set`
    /// and its `free` in one `TexturesDelta`, the backend processed `set` before
    /// `free`, and the paint sampled an already-freed texture → the overlay was
    /// invisible. Keeping the handle alive (exactly like [`Self::texture`] and
    /// the navigator/thumbnail textures) means no per-frame alloc/free and the
    /// matte actually reaches the screen.
    mask_overlay_texture: Option<egui::TextureHandle>,
    /// GUI-NAV-RECT-1: overview texture of the FULL source for the navigator
    /// viewport (never the ROI-cropped preview texture) + its cache key
    /// `(path, full_w, full_h)`. Rebuilt only on source change.
    navigator_texture: Option<egui::TextureHandle>,
    navigator_texture_key: Option<(String, u32, u32)>,
    /// GUI-NAV-RECT-1: cached downscaled full-frame overview render (current
    /// recipe) for the zoomed navigator + its key
    /// `(path, full_w, full_h, recipe_digest)`. Recomputed only when source
    /// or recipe change; at Fit the preview texture is reused instead.
    navigator_overview: Option<ImageFrame>,
    navigator_overview_key: Option<(String, u32, u32, String)>,
    /// R2-GUIMOD-02: bumped whenever `self.preview` receives new content.
    /// Part of [`Self::texture_identity`]; together with the Before/After flag
    /// and pixel size it gates the CPU texture upload to actual content
    /// changes. A monotonic counter is deliberate: draft and full renders of
    /// the same source can produce identical render keys while their pixels
    /// differ (masks are skipped for drafts), so key-based identity would be
    /// unsound.
    ///
    /// INVARIANT: every site that assigns `self.preview` MUST bump this
    /// counter — otherwise the CPU present path keeps serving the previous
    /// upload (`texture_identity` still matches). The only production
    /// assignment today is `render_from`; a new source clears `preview`
    /// implicitly by resetting render state before the next render bumps the
    /// generation again.
    preview_generation: u64,
    status: String,
    error: Option<String>,
    /// KITTEST-COVERAGE-STATES-1: whether the current `error` should be shown
    /// as the popup dialog (explicit user-action failure) or only as the
    /// header banner (background decode/listing failure). Reset by
    /// [`LuminaApp::show_error_banner`], set by [`LuminaApp::show_error`].
    error_dialog: bool,
    /// Crash-Fix Runde 2 (F5): message of the last draft-render failure that
    /// was surfaced. A repeated identical failure in a following draft tick is
    /// downgraded to one `warn!` instead of re-emitting `error!` and re-arming
    /// the dialog every frame (45 MP drag ticks made that a log/abort storm).
    /// Cleared once a draft render succeeds ([`LuminaApp::clear_draft_error_dedup`]).
    draft_error_dedup: Option<String>,
    /// Crash-Fix Runde 2 (F5): whether the one-time repeat `warn!` for the
    /// current [`Self::draft_error_dedup`] message was already emitted. Keeps a
    /// persistent failure from spamming `warn!` once per frame, too.
    draft_error_repeat_warned: bool,
    render_key: Option<RenderKey>,
    tone_analysis: Option<lumina_core::ToneAnalysis>,
    /// 256-bin luminance histogram of the full-frame render (GUI-HISTOGRAM-FULL-1,
    /// F-100): always computed from the un-cropped full frame — never from the
    /// ROI-cropped viewport texture — stored together with
    /// [`Self::tone_analysis`] from the single shared
    /// `analyze_tone_with_histogram` pass. Feeds the filled Painter curve;
    /// `None` until the first render (panel shows `NotCurrent`).
    preview_histogram: Option<LuminanceHistogram>,
    /// Pending slider commit awaiting the debounced full render
    /// (GUI-SLIDER-SAVE-1): `(recipe_key, value)` recorded by
    /// [`Self::set_adjustment`] / [`Self::set_presence`] /
    /// [`Self::reset_single_adjustment`]. Consumed by
    /// [`Self::commit_pending_slider_save`], which renders, saves the sidecar
    /// and logs `<key>=<value> saved`. Zoom/pan state is deliberately never
    /// recorded here — it stays GUI session state, never recipe.
    pending_slider_commit: Option<(String, f64)>,
    /// Pending geometry history step (G-06, LRPAR-G06-GEO): armed by the
    /// geometry setters (`set_geometry_*`, `set_crop_*`, `set_straighten`,
    /// `set_perspective_value`, `set_lens_correction_value`,
    /// `set_lens_profile`) alongside the slider commit. Consumed by
    /// [`Self::save_sidecar`], which appends exactly one history entry
    /// (`geometry-<n>`, final recipe) per saved commit — slider drags
    /// coalesce to one step per debounce, discrete actions to one step
    /// each. `None` outside geometry edits: all other sliders keep the
    /// established no-history-commit behaviour.
    pending_history_step: Option<String>,
    /// Effective mask layers of the last [`Self::render`] (F-041): the
    /// measurement domain of `Match Total Exposure` is the rendered preview
    /// weighted by these planes. Empty whenever the render produced no layers.
    render_mask_layers: Vec<MaskLayerResult>,
    document: Option<SidecarDocument>,
    virtual_copy_id: String,
    selected_mask_id: Option<String>,
    mask_name_input: String,
    mask_tool: MaskTool,
    /// Normalized brush radius (0..=1 in source space). Driven by a slider.
    brush_radius: f32,
    /// When true, brush marks use the negative (eraser) sign.
    brush_eraser: bool,
    /// Marks accumulated during an in-progress brush drag (cleared on release).
    pending_brush_marks: Vec<BrushMark>,
    /// Drag start/current normalized points for gradient/radial gestures.
    drag_start: Option<Point2>,
    drag_current: Option<Point2>,
    /// True while a mask-tool drag is in progress (drives the live overlay).
    drawing: bool,
    spot_tool: SpotTool,
    spot_mode: SpotMode,
    spot_radius: f32,
    spot_feather: f32,
    spot_opacity: f32,
    /// LRPAR-G04-REMOVE session/panel state (display inputs, never recipe —
    /// except where noted): `spot_detect_threshold` is the input for heuristic
    /// Detect-Objects (`0..=1`); `spot_detect_status` holds the last detection
    /// outcome text (candidates listed, never silently applied);
    /// `spot_gen_prompt`/`spot_gen_seed`/`spot_gen_variant` are the inputs for
    /// generative variant regeneration (persisted per spot on Regenerate);
    /// `spot_gen_status` holds the last regeneration outcome text and
    /// `spot_gen_target` the target spot id (panel input, session state).
    /// The visualize threshold itself is recipe-backed (see
    /// [`Self::set_spot_visualize`]); the distraction switches are
    /// recipe-backed too (see [`Self::set_spot_distraction`]).
    spot_detect_threshold: f32,
    spot_detect_status: String,
    spot_gen_prompt: String,
    spot_gen_seed: u64,
    spot_gen_variant: u64,
    spot_gen_status: String,
    /// Target spot id for variant regeneration (panel input, session state).
    spot_gen_target: String,
    preset_name: String,
    preset_fields: BTreeMap<String, bool>,
    preset_relative_exposure: bool,
    /// F-009: user-global presets directory; `None` means the platform config
    /// base could not be determined and file presets are shown as unavailable
    /// (no silent fallback directory).
    presets_dir: Option<std::path::PathBuf>,
    /// F-009: current snapshot of the presets directory. Failed files stay in
    /// the list with their error text instead of being skipped silently.
    preset_entries: Vec<presets::PresetEntry>,
    idle_queue: IdleQueue,
    /// PERF-FILMSTRIP: dedicated background thread pool for filmstrip
    /// thumbnails. `thumbnail_tx` enqueues jobs (unbounded mpsc, no capacity
    /// gate); `thumbnail_rx` delivers rendered frames to be textured on the
    /// main thread.
    thumbnail_tx: mpsc::Sender<ThumbnailJob>,
    thumbnail_rx: mpsc::Receiver<ThumbnailResult>,
    /// R2-MODSWITCH-1 F7: metadata-only per-folder preview index the thumbnail
    /// scheduler probes (memoized settings + `read_dir`, no per-cell disk read).
    thumbnail_cache: thumb_cache::PreviewIndexCache,
    /// Active top-level module (Library / Develop / Export).
    active_module: Module,
    /// R2-MODSWITCH-1 F7: module the render scheduler last ran for. The first
    /// run after a change defers a due full render past the switch frame.
    last_scheduled_module: Option<Module>,
    /// R3-OPEN-1: display path of the background decode currently in flight
    /// (`None` when idle). Set by `begin_load_path`, cleared by the decode
    /// drain; lets the Develop-switch open reuse an already-started decode
    /// instead of starting a duplicate one.
    pending_load_path: Option<String>,
    /// R3-WARMUP-1: one-shot cold-start warmup state (scheduling only).
    warmup: warmup::WarmupState,
    /// R3-LOG-1: cross-frame timing anchors (module switch event, in-flight
    /// decode). Measurement only — never read for logic.
    timing: timing::TimingState,
    /// Export module UI state (F-103-N5). The target path is chosen via a
    /// native save dialog; the format/quality drive the shared export path.
    export_path: String,
    export_format: ImageFileFormat,
    export_quality: u8,
    /// Before/After toggle state. Never mutates the recipe.
    before_after: bool,
    /// Welle 2 (LR-09): session-only copy/paste-settings clipboard. Holds the
    /// recipe snapshot taken by `Cmd/Ctrl+Shift+C`; `None` until the first
    /// copy. Never persisted (Lightroom behaviour) — paste applies it to the
    /// active copy through the normal save/render path. Native-only: clipboard
    /// and sidecar persistence are file-system capabilities.
    settings_clipboard: Option<EditRecipe>,
    /// LRPAR-G08-PREVIOUS: cross-image Previous reference (the image edited
    /// immediately before the current one). Captured on every successful
    /// image switch in [`Self::finish_decode`]; applied by
    /// [`Self::apply_previous_to_selection`]. Session-only, never persisted
    /// (like `settings_clipboard` above).
    previous_reference: Option<PreviousReference>,
    /// UX-LOOK-HISTORY-18: session-only clock override for deterministic
    /// history timestamps in headless/kittest tests. Never persisted; `None`
    /// uses the real UTC clock.
    history_timestamp_override: Option<String>,
    /// Welle 2 display-only view flags (`J` clipping overlay, `L` lights-out,
    /// `Tab` panel hide, `R` crop mode). None of them mutates the recipe; the
    /// B&W `V` treatment is recipe-backed instead (see `toggle_black_white`).
    clipping_overlay: bool,
    lights_out: bool,
    panels_hidden: bool,
    crop_mode: bool,
    /// G-11 (LRPAR-G11-OVERLAYS) session-only display state. Never persisted
    /// to the sidecar and never part of the recipe (like `Tab`/`L`/`F` above):
    /// * `all_panels_hidden`: `Shift+Tab` hides side panels, navigator rail
    ///   AND filmstrip (header/module bar + preview stay).
    /// * `overlay_mode`: global tool-overlay mode (mask matte tint).
    /// * `pin_visibility`: global edit-pin visibility (mask anchors + spots).
    /// * `solo_mode` + `section_open`: solo collapses the 8 Develop sections
    ///   to a single open one; `section_open` is the explicit open state so
    ///   solo stays headless-testable (no egui-implicit collapsing memory).
    all_panels_hidden: bool,
    overlay_mode: OverlayMode,
    pin_visibility: PinVisibility,
    solo_mode: bool,
    section_open: [bool; SECTION_COUNT],
    /// G-03 (LRPAR-G03-MASK) mask-overlay + panel session state. Display-only
    /// (never recipe/sidecar, like G-11 above) except `MaskLayer.visible`,
    /// which is persisted per virtual copy through
    /// [`Self::set_mask_visible`]:
    /// * `show_mask_overlay`: master Show switch ANDed with `overlay_mode`.
    /// * `overlay_color`: matte tint RGB (default Lightroom-red).
    /// * the remaining fields are panel inputs for AI-select / range adds and
    ///   combine/duplicate (kind, part detail, range parameters, other-mask
    ///   reference, duplicate name).
    show_mask_overlay: bool,
    overlay_color: [u8; 3],
    ai_select_kind: AiSelectKind,
    ai_detail_input: String,
    ai_name_input: String,
    lum_name_input: String,
    lum_min: f32,
    lum_max: f32,
    lum_feather: f32,
    col_name_input: String,
    col_hue_center: f32,
    col_hue_width: f32,
    col_sat_min: f32,
    col_sat_max: f32,
    col_lum_min: f32,
    col_lum_max: f32,
    col_feather: f32,
    combine_other_id: String,
    combine_name_input: String,
    duplicate_name_input: String,
    /// LRPAR-G03-MASKGROUP-03 session state: the group marked selected in the
    /// panel, the pending group name, the masks marked for late grouping and the
    /// shared feather/density offsets applied to the selected group's members.
    /// Display-only; groups themselves persist in the sidecar.
    selected_group_id: Option<String>,
    group_name_input: String,
    group_member_selection: BTreeSet<String>,
    group_feather_offset: f32,
    group_density_offset: f32,
    /// G-02 (LRPAR-G02-COLOR) tone-curve panel session state. Display-only
    /// (never recipe/sidecar): the selected curve channel (`0` master,
    /// `1..=3` red/green/blue). UX-LOOK-TONECURVE-18 edits the curve through
    /// the interactive graph, so no add-point input buffers are kept.
    tone_curve_channel: usize,
    /// Welle 3 (LR-13/LR-20/LR-09/LR-12/LR-17 light) display/session state.
    /// All of these are display-only or `extras`/history-backed, so no
    /// sidecar schema change was needed:
    /// * `filter_bar_visible` + `library_filter`: `\` Library drawer (text
    ///   filter over the scanned entry metadata + Quick Develop sliders).
    /// * `compare_mode`: `C` compare / `N` survey proxy reusing
    ///   `before_after` (compare) and the Library grid (survey).
    /// * `before_after_split`: `Shift+Y` split-view marker (full-frame
    ///   Before proxy; side-by-side render is follow-up work).
    /// * `fullscreen`: `F` fullscreen preview (hides the same chrome as
    ///   lights-out and settles the zoom on Fit).
    filter_bar_visible: bool,
    library_filter: String,
    compare_mode: Option<CompareMode>,
    /// G-09 (LRPAR-G09-LIB) Library view: Grid/Loupe/Compare/Survey over the
    /// same selection and filter. Display-only (never recipe/sidecar); a
    /// reload restores the default (`Grid`).
    library_view: LibraryView,
    /// LRPAR-G09-SORT-09: active Library sort mode (Name/CaptureDate/Custom).
    /// Display-only, but persisted per folder together with the custom order in
    /// `lumina-sort.json` in `.lumina/` (see `library_sort`). Default `Name`.
    library_sort: LibrarySort,
    /// LRPAR-G09-SORT-09: custom order as relative keys of the listed folder
    /// (stable names, never array positions). Empty when unused.
    library_sort_order: Vec<String>,
    before_after_split: bool,
    fullscreen: bool,
    /// G-16 (LRPAR-G16-POWER) session-only display state. Never persisted to
    /// the sidecar and never part of the recipe (like `Tab`/`L`/`F`):
    /// * `softproof_preview`: plain `S` softproof-preview badge (the G-10
    ///   binding reservation; full print/gamut simulation is G-10 follow-up).
    /// * `show_original_histogram`: G-10 "Original Photo" histogram compare —
    ///   shows the unedited Original-Decode measurement instead of the edited
    ///   render (same `analyze_tone`/`LuminanceHistogram` path as Before/After,
    ///   no second analysis path).
    /// * `masking_preview`: `Alt`-held tone-slider edit transiently shows the
    ///   clipping badge through the `J` path; holds the slider key while armed.
    softproof_preview: bool,
    show_original_histogram: bool,
    masking_preview: Option<String>,
    /// LRPAR-G01-BASIC: "Reset Sliders Automatically" (Develop footer
    /// checkbox, default off). Folder-inherited via `.lumina/settings.json`
    /// (`FolderCacheSettings::reset_sliders_automatically`): when armed,
    /// switching images discards an armed-but-uncommitted slider edit;
    /// otherwise it is flushed to the previous image's sidecar. Never part
    /// of a recipe or sidecar (edit behaviour, not image state).
    reset_sliders_automatically: bool,
    /// LRPAR-G01-BASIC: Previous baseline (recipe snapshot captured at image
    /// load and after every successful save). `Previous` restores one
    /// section's fields from this snapshot; `Reset` sets the section's
    /// documented defaults. Masking layers travel separately in
    /// `mask_baseline` (they live on the virtual copy, not the recipe).
    recipe_baseline: Option<EditRecipe>,
    /// LRPAR-G01-BASIC: mask-layer baseline of the active virtual copy for
    /// the Masking section Previous/Reset (same capture points as above).
    mask_baseline: Vec<MaskLayer>,
    /// White-balance eyedropper armed state.
    wb_pick_mode: bool,
    /// LRPAR-G14-REDEYE-15: red-eye region picker armed state (click the
    /// preview to mark a pupil; regions are persisted explicitly).
    red_eye_pick_mode: bool,
    /// LRPAR-G14-REDEYE-AUTO-15: last detection outcome text (candidates
    /// listed, never silently applied).
    red_eye_detect_status: String,
    /// Generated filmstrip thumbnail textures.
    thumbnails: ThumbnailManager,
    /// GUI-FILMSTRIP-SYNC-1: multi-selection of filmstrip entries
    /// (Lightroom-like). Paths are stored as display strings (the same key
    /// [`Self::open_file`] takes), never indices — entries re-sort on rescan.
    filmstrip_selection: BTreeSet<String>,
    /// Anchor for Shift-Click range selection (last plain/toggle click).
    filmstrip_anchor: Option<String>,
    // ---- PERF-GUI-* (CPU interactivity quick-wins, no GPU) ----
    /// True while the preview shows a low-resolution draft (rendered from
    /// `draft_original` during a slider drag); cleared on the full render.
    preview_is_draft: bool,
    /// Source downscaled to draft resolution, cached on load so draft renders
    /// never re-allocate during a slider drag (PERF-GUI-3 "zero alloc").
    draft_original: Option<ImageFrame>,
    /// R2-JANK-1 F1/F4: frame-budget throttle for the interactive draft render
    /// (F1) and cadence throttle for the draft analysis pass (F4). Time-state
    /// only; the visible pending marker is derived from it.
    draft_throttle: DraftThrottle,
    /// PERF-GUI-1: RAM cache of prepared base stages (`prepare_source_base`
    /// output: post decode/source-actions/ROI-crop, pre-adjustment), keyed by
    /// `CacheStage::Base` digests. Recipe-blind keys mean every slider change
    /// reuses the cached demosaiced base and re-renders only the adjustment
    /// stage downstream; a new source clears it in [`Self::apply_decoded_frame`].
    base_stage_cache: StageFrameCache,
    /// PERF-GUI-1: memoized blake3 content hash of `source_bytes`. Hashing the
    /// whole RAW file per render tick was part of the old hot path; it is now
    /// computed once per loaded source and invalidated with it.
    source_hash_memo: Option<String>,
    /// PERF-GUI-1: stage work counters of the last completed render
    /// (diagnostics + cache-hit tests). `None` until the first staged render.
    last_stage_work: Option<StageWork>,
    /// R2-GUIMOD-04a: milliseconds of the `analyze_tone_with_histogram` pass
    /// inside the last `render_from` (measurement only, never read for logic).
    last_analysis_ms: f64,
    /// R2-GUIMOD-04a: per-tick drag-render timings of the last coalesced
    /// pointer-drag tick (measurement only, feeds F-103-N6).
    last_drag_tick: Option<DragTickTimings>,
    /// R3-RENDER-SIZE-1: preview viewport-cap state (dpr, draft edge, cache).
    preview_cap_state: preview_size::PreviewCapState,
    /// Timestamp (egui `ctx.input(|i| i.time)`) of the last frame that still
    /// had pending edits; drives the 150 ms idle debounce (PERF-GUI-3/4).
    last_edit_time: f64,
    /// Preview zoom factor (1.0 = fit). >1 zooms into the centre; the render
    /// path crops to the visible source bounding box (ROI, PERF-GUI-5).
    preview_zoom: f32,
    /// Active zoom mode driving `preview_zoom` (see [`ZoomMode`]). Non-`Custom`
    /// modes re-derive `preview_zoom` from the pane each frame (so they survive
    /// resizes); `Custom` pins an explicit relative-to-fit multiplier.
    zoom_mode: ZoomMode,
    /// Screen-space pan offset (px) from the centred position, applied when the
    /// preview is zoomed beyond fit so the image can be dragged (hand tool).
    preview_pan: egui::Vec2,
    /// Source ROI `(x, y, w, h)` of the currently displayed texture. `None`
    /// means the whole frame. Pointer→source mapping accounts for this crop so
    /// the WB eyedropper and mask tools stay accurate while zoomed/panned.
    /// The rect is expressed in *render-source* pixels (see
    /// [`Self::preview_render_src`]): identical to full-source pixels for a
    /// full render, downscaled for a draft render.
    preview_roi: Option<[u32; 4]>,
    /// Render-source dimensions `(w, h)` backing the currently displayed
    /// texture (GUI-DRAFT-JUMP-1): the `source.width/height` that
    /// [`Self::render_from`] consumed for the last render — the full original
    /// on the non-draft path, the downscaled `draft_original` on the draft
    /// path. `draw_preview` scales the texture back into full-source geometry
    /// with it (`draw = tex_dims · (full/render_src) · scale`), so a draft
    /// and its full render share the exact on-screen placement instead of the
    /// draft drawing too small with a pan offset error. `None` until the first
    /// render (legacy/empty state: no rescaling).
    preview_render_src: Option<(u32, u32)>,
    /// Cached geometry from the last [`Self::draw_preview`] so the next frame's
    /// [`Self::sync_zoom`] can derive absolute zoom modes correctly.
    ///
    /// `preview_base_fit_scale` is the object-contain fit of the pane against
    /// the **un-cropped source dimensions**, never against the currently
    /// displayed texture: at zoom > 1 that texture is an ROI crop whose fit
    /// scale depends on the zoom itself, so deriving absolute modes (100% /
    /// 200% / Fit Width) from it oscillates frame-by-frame
    /// (REVIEW-GUI-ZOOMLOOP-1).
    preview_base_fit_scale: f32,
    preview_pane_w: f32,
    preview_pane_h: f32,
    /// Un-cropped source dimensions backing the displayed texture (cached so
    /// `sync_zoom` can compute Fit-Width and the base fit without borrowing
    /// `self.original`).
    preview_src_w: f32,
    preview_src_h: f32,
    /// Effective on-screen scale (screen px per source px) for the zoom readout.
    preview_effective_scale: f32,
    /// KITTEST-PARITY-PATHS-1: on-screen rect of the preview image quad as
    /// painted by the last [`Self::draw_preview`] — the same `rect` both present
    /// paths (CPU texture / GPU VRAM user texture) blit into. Pure geometry
    /// readout for the path-parity framework; never painted as text/badge.
    /// `None` before the first painted frame and while the empty state shows.
    preview_screen_rect: Option<egui::Rect>,
    /// KITTEST-PARITY-PATHS-1: the pane rect the preview was fitted into for the
    /// last painted frame (companion of [`Self::preview_screen_rect`]).
    preview_pane_rect: Option<egui::Rect>,
    /// KITTEST-PARITY-PATHS-1: the full-source overlay rect (`full_rect`) the
    /// last [`Self::draw_preview`] mapped every overlay (mask matte, pins,
    /// lens-blur focus, crop) onto. At Fit with no ROI this equals the painted
    /// preview rect; storing it lets the framework assert absolutely that
    /// overlays land on the photo instead of only comparing CPU↔GPU parity.
    overlay_full_rect: Option<egui::Rect>,
    /// Whether the left thumbnail navigator rail is open.
    navigator_open: bool,
    /// Library module: expanded folder-tree nodes, keyed by absolute path.
    open_folders: BTreeSet<String>,
    /// Library module: lazy per-folder children cache, filled via `read_dir`
    /// the first time a folder node is expanded.
    folder_children: BTreeMap<String, Vec<String>>,
    /// Library module: depth-limited RAW count + "has supported image"
    /// per folder node for R4-LIB-1 pruning (computed once per folder).
    folder_raw_counts: BTreeMap<String, library_tree::FolderTreeInfo>,
    /// UX-SLICE-2 (F2): injectable native folder-picker seam. `None`
    /// (production) opens the real `rfd` dialog; headless tests install a
    /// closure (no display server) so the empty-state CTA wiring can be
    /// asserted end to end. Session state, never persisted.
    folder_picker: Option<Box<dyn FnMut() -> Option<PathBuf>>>,
    /// Library module: current thumbnail cell size (px) for the center grid,
    /// driven by a toolbar slider (Lightroom-like resizable library thumbs).
    library_thumb_size: f32,
    /// G-09 (LRPAR-G09-LIB) Library grid columns of the last laid-out grid
    /// (drives ArrowUp/ArrowDown navigation by one row; headless default 4
    /// until the first draw measures the real width).
    library_cols: usize,
    /// G-15 META-MVP (Slice 3) Library metadata session state. The persisted
    /// truth stays Sidecar-first (`SidecarDocument.keywords` /
    /// `.collections` via `apply_batch_op` + `save_sidecar`); these fields
    /// are panel inputs + catalog session state only:
    /// * `keyword_input`: text field for adding a keyword to the loaded image.
    /// * `collection_id_input`/`collection_name_input`: `id` + display `name`
    ///   for adding the loaded image to a static collection (`id=name` also
    ///   accepted in the id field — split at the first `=`).
    /// * `active_collection`: grid filter selection (`None` = all images).
    /// * `smart_catalog`/`smart_catalog_path`: portable smart-collection
    ///   catalog in the CLI-identical `lumina-smart-catalog` v1 format;
    ///   persisted only via explicit Load/Save (never implicitly).
    /// * `smart_id_input`/`smart_name_input`/`smart_rule_kind`/
    ///   `smart_rule_value`: smart-definition editor inputs.
    /// * `smart_rule_stack`: pushed rules awaiting `And`/`Or`/`Not`
    ///   composition before Create.
    /// * `batch_kind`/`batch_value`: batch-bar inputs selecting exactly one
    ///   `BatchOp` for [`Self::apply_metadata_batch`].
    keyword_input: String,
    collection_id_input: String,
    collection_name_input: String,
    active_collection: Option<CollectionFilter>,
    smart_catalog: Vec<SmartCollectionDef>,
    smart_catalog_path: String,
    smart_id_input: String,
    smart_name_input: String,
    smart_rule_kind: String,
    smart_rule_value: String,
    smart_rule_stack: Vec<SmartRule>,
    batch_kind: String,
    batch_value: String,
    /// LRPAR-G15-IPTC-S8: Library Metadata panel (right column) session
    /// state. The persisted truth stays Sidecar-first
    /// (`SidecarDocument.metadata` draft + history, `keywords`); these
    /// fields are panel inputs + dialog state only:
    /// * `meta_buffers`/`meta_buffers_key`: per-field draft text inputs,
    ///   synced from the loaded document (`(path, latest_rev)` key); an
    ///   empty buffer on commit removes the field (S1 draft semantics).
    /// * `meta_presets_dir_override`: explicit presets directory for
    ///   headless tests (production uses the user-global directory).
    /// * `meta_preset_entries`: last scanned meta-presets (failed files
    ///   stay visible, never skipped silently).
    /// * `selected_meta_preset`: preset spec for Apply (display name or
    ///   path, same resolution as the CLI).
    /// * `meta_preset_dialog`: open prompt dialog for dynamic presets
    ///   (one required input per placeholder; Cancel discards it).
    /// * `meta_sync_fields`: field checkbox selection for
    ///   [`Self::sync_metadata_to_selection`] (default: all on).
    meta_buffers: BTreeMap<String, String>,
    meta_buffers_key: Option<(String, u64)>,
    /// LRPAR-G15-IPTC-S8: true once the user edited a draft buffer since
    /// the last sync — [`Self::ensure_meta_buffers`] then keeps the
    /// keystrokes and only fills missing fields, instead of resyncing from
    /// the document (which would wipe the edit, e.g. when the document is
    /// first created by the commit itself).
    meta_buffers_dirty: bool,
    meta_presets_dir_override: Option<PathBuf>,
    meta_preset_entries: Vec<MetaPresetEntry>,
    selected_meta_preset: String,
    meta_preset_dialog: Option<MetaPresetDialog>,
    meta_sync_fields: BTreeMap<String, bool>,
    /// KITTEST-COVERAGE-STATES-1: session clipboard for the metadata panel's
    /// own copy/paste system (separate from the Develop settings clipboard).
    /// Holds the non-empty draft field values copied from an image; `None`
    /// until the first copy. Session-only — never persisted (Sidecar-first).
    meta_clipboard: Option<BTreeMap<String, String>>,
    /// LRPAR-G15-IPTC-S8: cached embedded IPTC of the loaded image
    /// (read-only display). Keyed by `(path, length, mtime)` so the panel
    /// never re-reads the file per frame; `Err` text is the loud
    /// unreadable-JPEG error (never a silent skip).
    meta_embedded_cache: Option<EmbeddedCache>,
    /// Develop history section: currently selected (last restored) history
    /// entry id of the active virtual copy.
    history_selected: Option<String>,
    /// PERF-GUI-7: receiver for a background RAW/raster decode. `Some` while a
    /// decode is in flight on a worker thread.
    decode_rx: Option<std::sync::mpsc::Receiver<DecodeResult>>,
    /// R2-MODSWITCH-1 F8: receiver for a background folder scan. `Some` while a
    /// scan is in flight on its worker thread; drained by `poll_scan`.
    scan_rx: Option<std::sync::mpsc::Receiver<library_scan::ScanResult>>,
    /// R2-MODSWITCH-1 F8: latest-wins generation tag for the async folder scan;
    /// a result tagged with an older generation is dropped (never merged).
    scan_generation: u64,
    /// R2-MODSWITCH-1 F8: a scan is in flight — drives the visible loading
    /// status so the grid never stalls silently.
    scan_pending: bool,
    /// REVIEW-GUI-N1: revision (BLAKE3 over the JSON) of the on-disk sidecar
    /// that the in-memory `document` lineage is based on. `None` means no
    /// sidecar file existed when this lineage started (fresh document). Passed
    /// to the compare-and-swap write in [`Self::save_sidecar`] so an
    /// externally modified sidecar surfaces as a visible conflict instead of
    /// being silently overwritten; refreshed after every successful save.
    sidecar_revision: Option<String>,
    /// True while an edit (slider drag, presence change, etc.) needs a
    /// full-quality render. Drives the debounced full render after a pointer
    /// drag settles (PERF-GUI-3/4). Cleared once the full render runs.
    pending_full_render: bool,
    /// Set once the directory-auto-load has begun a background decode, so
    /// `list_directory` never re-triggers it every time the directory is
    /// rescanned (the decode is async and `original` stays `None` until it
    /// finishes). Left unset while no RAW entry exists, so a later scan of a
    /// now-populated directory can still auto-load. Native-only (directory
    /// auto-load is a native file-system capability).
    auto_load_attempted: bool,
    /// GUI-60FPS-1: optional GPU context for the desktop. `None` when no adapter
    /// is bound (CPU fallback remains fully functional).
    #[cfg(feature = "gpu")]
    gpu: Option<lumina_gpu::GpuContext>,
    /// GUI-60FPS-1 H1: persistent R16 mask plane (Vec<u16> u16-LE, row-major,
    /// `width × height`) backing the interactive brush. Kept CPU-side so each
    /// dirty 512² tile can be (re-)stamped incrementally via
    /// `lumina_core::mask_tiles::stamp_brush_mark` and then uploaded with
    /// `queue.write_texture` (`bytemuck::cast_slice` → `&[u8]`). Only dirty tiles
    /// are uploaded per stroke (no whole-plane rewrite, no dummy zeros).
    #[cfg(feature = "gpu")]
    brush_mask_plane: Option<Vec<u16>>,
    #[cfg(feature = "gpu")]
    brush_mask_plane_dims: Option<(u32, u32)>,
    /// GUI-WGPU-PRESENT-1: the eframe wgpu renderer's shared state. When
    /// present, `lumina-gpu` was constructed on the *same* Device/Queue
    /// (see `attach_wgpu_render_state`), so the VRAM overlay composite can be
    /// registered as an egui user texture and presented without any CPU
    /// readback.
    #[cfg(feature = "gpu")]
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    /// Offscreen target the VRAM overlay pass composites into; registered once
    /// as an egui user texture and re-created only when dimensions change.
    #[cfg(feature = "gpu")]
    present_target: Option<PresentTarget>,
    /// True while the VRAM output corresponds to the current recipe/source:
    /// set right after a successful `render_to_vram`, cleared by every edit
    /// ([`Self::mark_dirty`]) so a stale tone result can never be presented.
    /// R2-GUIMOD-01: also cleared by every completed **full-quality** CPU
    /// render (`render_from` on the non-draft path) — otherwise the debounced
    /// full render after a drag would compute sharp pixels that are then never
    /// shown because the gate kept presenting the superseded VRAM draft.
    #[cfg(feature = "gpu")]
    vram_fresh: bool,
    /// R2-GUIMOD-05: memoized `unsupported_gpu_stages(&self.recipe)` verdict,
    /// keyed by the [`RenderKey`] of the render it was computed for. The gate
    /// used to rebuild this `Vec<String>` (with `format!` allocations) every
    /// frame although recipe/render identity rarely changes. `None` while no
    /// key-backed verdict is stored; queried without a render key (dirty
    /// preview) deliberately bypasses the memo because the recipe may have
    /// drifted since the last render.
    #[cfg(feature = "gpu")]
    gpu_stage_gate: Option<GpuStageGate>,
    /// True while the VRAM mask texture carries the pipeline-*evaluated* layer
    /// planes (pushed after a full render) rather than only live brush stamps —
    /// then the shader overlay already shows what the CPU overlay would paint.
    #[cfg(feature = "gpu")]
    vram_mask_is_evaluated: bool,
    /// The egui user-texture id + size of the GPU-presented preview for THIS
    /// frame (recomputed in `update_texture`, consumed in `draw_preview`).
    #[cfg(feature = "gpu")]
    gpu_present_frame: Option<(egui::TextureId, [usize; 2])>,
    /// R2-GUIMOD-06: visible (non-stderr) feedback for the GPU→CPU routing
    /// fallback. `Some(reason)` when a GPU context is available and usable but
    /// the recipe references stages the VRAM tone path cannot evaluate, so the
    /// preview is computed on the CPU — a silent fallback before this fix.
    /// `None` while the GPU present path is usable (or when no GPU context
    /// exists at all, in which case there is no "fallback" to report). Surfaced
    /// as a status badge in the preview HUD; it never affects rendered pixels.
    #[cfg(feature = "gpu")]
    gpu_route_fallback: Option<String>,
    /// GUI-LENSFUN-GATE-3 (F1): the precise present-refusal reason of the last
    /// failed [`lumina_gpu::GpuContext::render_to_vram`] attempt. The recipe
    /// gate ([`Self::gpu_unsupported_stage_reasons`]) does not cover
    /// dimension-changing geometry (`render_to_vram` refuses it after the gate
    /// passed) and was therefore the only CPU route without a visible badge.
    /// When set, [`Self::routing_fallback_reason`] surfaces it as the badge
    /// **only if** the gate itself is empty. Diagnostic only: never consulted
    /// for routing/presentation; cleared by any edit (`mark_dirty`/
    /// `set_adjustment`), a new source, adopting a neighbor frame, or a
    /// successful VRAM render.
    #[cfg(feature = "gpu")]
    vram_render_refusal: Option<String>,
    /// PARITY-PATHS-2: diagnostic override for the adapter-availability probe
    /// ([`Self::gpu_adapter_available`]). `Some(false)` lets the parity matrix
    /// exercise its adapterless SKIP branch on a machine that does have a
    /// Metal adapter; `None` (default) reports the real GPU context.
    /// Presentation/routing logic does **not** consult this — it only changes
    /// the probe the SKIP contract keys on.
    #[cfg(feature = "gpu")]
    gpu_adapter_override: Option<bool>,
    /// GUI-SCROLL-200-1: per-frame diagnostic counters for `LUMINA_PERF_LOG=1`.
    /// `frame_thumb_enqueued` counts worker jobs enqueued (or cached previews
    /// loaded) this frame, `frame_thumbs_ready` counts worker results applied.
    /// Both are reset at the start of [`Self::update`]; a scroll spike while
    /// thumbnail jobs run shows up as large values in the same frame that
    /// exceeds the 16.7 ms budget.
    frame_thumb_enqueued: usize,
    frame_thumbs_ready: usize,
    /// PREVIEW-CACHE-FEATURE: neighbor-preview controller (worker pool + RAM/disk
    /// LRU + prefetch window). Lazy-created on first navigation so unit tests
    /// that never schedule neighbors stay thread-free.
    preview_ctrl: Option<preview_ctrl::PreviewController>,
    /// PREVIEW-CACHE-FEATURE: per-frame counters for the neighbor-preview work
    /// (LUMINA_PERF_LOG diagnostics).
    frame_previews_enqueued: usize,
    frame_previews_ready: usize,
    /// GUI-TOAST-OVERLAP-1: transient overlay toast (message + egui-time
    /// deadline). Shown in its own [`egui::Area`] so it never takes layout
    /// width or covers thumbnails persistently; auto-dismissed after
    /// [`TOAST_TIMEOUT_SECONDS`], manually dismissible via its button.
    toast_message: Option<String>,
    toast_until: f64,
    /// LRPAR-G14-DENOISE-IMPL-20 (GUI slice): resolved status/policy of the
    /// active `denoise_ai` stage. Session state (never persisted — the recipe
    /// is); refreshed from recipe + `.lumina.zdata` by
    /// [`denoise_gui::LuminaApp::refresh_denoise_gui`].
    denoise_gui: denoise_gui::DenoiseGuiState,
    /// Explicit integration/test seam for the live denoiser model context.
    /// Production leaves it `None`: no licence-cleared weights are bundled
    /// (F-078 gate), so the resolved status is the honest `unavailable`.
    denoise_live_model: Option<DenoiseModelIdentity>,
    /// Set by every denoise-relevant edit (recipe/source/copy); the panel and
    /// the render then refresh the resolved state once instead of reading the
    /// `.lumina.zdata` bundle every frame.
    denoise_gui_dirty: bool,
    /// LRPAR-G09-CULL-25 (GUI slice): session-only People-view filter for the
    /// Library module (`person:<name>` token). Display state, never persisted.
    people_filter: String,
    /// LRPAR-G12-FACE-20 (S5): selected cluster id of the People view (empty =
    /// none selected). Session-only, never persisted.
    people_selected_cluster: String,
    /// LRPAR-G12-FACE-20 (S5): cached face-crop textures of the People view,
    /// keyed by `"<path>|<detection_id>"` (session-only, rebuilt on demand).
    face_crop_textures: BTreeMap<String, egui::TextureHandle>,
    /// LRPAR-G09-CULL-25 (GUI slice): selected cluster/person inputs for the
    /// People view actions (confirm name, split subset, merge target).
    /// Session-only panel inputs.
    people_name_input: String,
    people_split_subset: String,
    people_merge_target: String,
    /// LRPAR-G13-MERGE-15 (GUI slice): running merge job receiver.
    merge_job: Option<merge_gui::MergeJob>,
    /// LRPAR-G13-MERGE-15 (GUI slice): last merge outcome text (status line).
    merge_status: String,
}

/// Long edge (px) of the cached zoomed-navigator overview render
/// (GUI-NAV-RECT-1): thumbnail-grade, full-frame, current recipe.
const NAVIGATOR_OVERVIEW_MAX_DIM: u32 = 256;

/// GUI-TOAST-OVERLAP-1: seconds a toast stays visible without interaction
/// before it auto-dismisses.
const TOAST_TIMEOUT_SECONDS: f64 = 4.0;

/// CAMERA-WB-WELLE / GPU-LENSFUN-PARITY-1: the memoized GPU-stage verdict is
/// keyed by render identity and the As-Shot WB context. The WB context is
/// caller-owned like the (former) Lensfun corrector flag, so it must be part of
/// the key — otherwise a verdict computed with one context would be served for
/// another. The Lensfun corrector is no longer part of the key: its map bind is
/// not a recipe-gate reason anymore (see [`LuminaApp::gpu_unsupported_reasons`]).
///
/// GUI-LENSFUN-GATE-2: the value is the precise reason list (not just a bool),
/// so the visible routing badge can name *why* the CPU route was taken while
/// the per-frame hot path still hits this memo.
#[cfg(feature = "gpu")]
type GpuStageGate = ((RenderKey, Option<[f32; 4]>), Vec<String>);

/// GUI-WGPU-PRESENT-1: offscreen present target + its egui registration.
///
/// The overlay pass composites the VRAM tone output and mask plane into
/// `texture`; `texture` is registered with the eframe wgpu renderer as a user
/// texture (`register_native_texture`) so `painter().image(id, ..)` draws it
/// directly on screen. Re-created only when the VRAM dimensions change; the
/// old registration is freed to avoid leaking GPU-side bind groups.
#[cfg(feature = "gpu")]
struct PresentTarget {
    texture: eframe::wgpu::Texture,
    #[allow(dead_code)]
    view: eframe::wgpu::TextureView,
    id: egui::TextureId,
    dims: (u32, u32),
}

/// GUI-WGPU-PRESENT-1: hand the eframe wgpu renderer's shared state to the app.
///
/// Called from `run_native`'s builder with `CreationContext::wgpu_render_state`.
/// When present, [`lumina_gpu::GpuContext`] resources are re-based onto that
/// device/queue so VRAM textures are shareable with the presenting surface.
/// R2-GUIMOD-09: this function is also the single construction point for the
/// context — `LuminaApp::new` deliberately leaves `gpu` empty so startup
/// performs at most one adapter/device request. When no renderer state is
/// handed over, a standalone context is created here (same capability as the
/// old eager constructor) so non-present GPU paths keep working; headless
/// callers that never invoke this function simply stay CPU-only.
/// No-op without the `gpu` feature (CPU present path stays).
#[cfg(feature = "gpu")]
pub fn attach_wgpu_render_state(
    app: &mut LuminaApp,
    state: Option<eframe::egui_wgpu::RenderState>,
) {
    app.wgpu_render_state = state.clone();
    // R2-GUIMOD-09: this is now the ONLY place that constructs a
    // `GpuContext` during startup. `LuminaApp::new` leaves `gpu` empty, so a
    // shared-device context is built exactly once here instead of paying a
    // standalone init first and discarding it moments later.
    // Re-base the GPU context onto the renderer's device/queue so VRAM
    // textures share the presenting surface's device (the whole point of the
    // migration). If that fails we fall back to a standalone context and log
    // loudly — no silent capability downgrade.
    if let Some(rs) = &state {
        match lumina_gpu::GpuContext::from_parts(
            rs.instance.clone(),
            rs.adapter.clone(),
            rs.device.clone(),
            rs.queue.clone(),
        ) {
            Ok(ctx) => {
                log::info!(
                    "GPU present path: sharing eframe wgpu device ({})",
                    rs.adapter.get_info().name
                );
                app.gpu = Some(ctx);
            }
            Err(err) => {
                log::warn!(
                    "GPU present path: shared-device context unavailable ({err}); \
                     falling back to the CPU present upload"
                );
                // Preserve the historical capability of the eager standalone
                // context for non-present GPU paths (`render_to_vram`), but
                // create it here so startup still performs only ONE adapter/
                // device request.
                app.gpu = lumina_gpu::GpuContext::new().ok();
            }
        }
    } else {
        // No renderer state was handed over (headless harnesses, tests). The
        // historical eager constructor would have produced a standalone
        // context; keep that capability available at the single construction
        // point without any extra init cost when it is never used.
        app.gpu = lumina_gpu::GpuContext::new().ok();
    }
    // CAMERA-WB-WELLE (R2-MCP-01): a source may already be loaded when the
    // context is attached (or created standalone), so re-bind the current
    // decoder As-Shot context. Invalid metadata is sanitized to `None` on
    // decode, so a rejection here is logged loudly, never silently dropped.
    if let Some(gpu) = app.gpu.as_ref() {
        if let Err(error) = gpu.set_camera_white_balance(app.camera_white_balance) {
            log::warn!("GPU As-Shot white-balance bind rejected at attach: {error}");
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileBrowserEntry {
    path: PathBuf,
    name: String,
    /// Stable thumbnail key: canonicalized absolute path. Identical filenames
    /// in different folders must never share a thumbnail cell
    /// (REVIEW-GUI-THUMB-1).
    thumb_key: String,
    has_sidecar: bool,
    source_status: SourceStatus,
    conflict: bool,
    virtual_copies: usize,
    missing_models: usize,
    /// Star rating (`0..=5`, `0` = unrated) of the default virtual copy
    /// (LR-01). `0` when no sidecar exists or it carries no copies.
    rating: u8,
    /// Pick flag of the default virtual copy (LR-01); `Unflagged` without a
    /// sidecar.
    flag: lumina_sidecar::Flag,
    /// Color label (`0..=4`, `0` = none) of the default virtual copy (Welle 2),
    /// read from the copy's `extras["color_label"]`; `0` without a sidecar.
    color_label: u8,
    /// Source-level keywords of the sidecar (`SidecarDocument.keywords`,
    /// G-15 META-MVP Slice 3); empty without a sidecar. Powers the
    /// `keyword:` Library filter and the smart-collection evaluation.
    keywords: Vec<String>,
    /// Source-level static collection memberships
    /// (`SidecarDocument.collections`, G-15 META-MVP Slice 3); empty without
    /// a sidecar. Powers the `collection:` filter and the collection picker.
    collections: Vec<CollectionMembership>,
    /// `camera_make + camera_model` from `lumina_raw::read_metadata`
    /// (best effort; `None` when unreadable). Powers the `camera:` filter.
    camera: Option<String>,
    /// ISO from `lumina_raw::read_metadata` (best effort). Powers `iso:`.
    iso: Option<f32>,
    /// Focal length in mm from `lumina_raw::read_metadata` (best effort).
    /// Powers `focal:`/`focal_length:`.
    focal_length: Option<f32>,
    /// LRPAR-G09-SORT-09: EXIF capture timestamp (Unix seconds, best effort)
    /// from `lumina_raw::read_metadata`. Powers the `CaptureDate` sort; `None`
    /// when unreadable/absent (sorts after every known timestamp).
    capture_timestamp: Option<i64>,
    /// Relative subfolder of the entry vs. the listed directory (`""` for
    /// top-level files). Powers the Library grid path badge (F-100): the
    /// recursive aggregation shows subfolder images with their relative
    /// folder as badge; flat listings (tree click) always carry `""`.
    folder: String,
    /// LRPAR-G09-CULL-25 (GUI slice): scan-level assisted-culling badge
    /// (`None`/`keep`/`review`/`reject`/`stale`) from the sidecar's
    /// source-level `culling` section. Display only; the authoritative read
    /// state is computed on demand by `LuminaApp::culling_read_state`.
    cull_badge: cull_gui::CullBadge,
    /// LRPAR-G12-FACE-20 (S5): person names of the sidecar's source-level face
    /// analysis (empty without one). Powers the `person:` Library filter.
    /// No geotag/GPS data is ever read or stored here.
    face_persons: Vec<String>,
    /// LRPAR-G15-STACK-15: source-level image-stack membership
    /// (`SidecarDocument.stack`); `None` without a sidecar or outside a stack.
    /// Drives the Grid/Filmstrip collapse and the stack-as-unit selection.
    stack: Option<lumina_sidecar::StackMembership>,
}

/// REVIEW-GUI-THUMB-1: stable thumbnail cache key. The canonicalized absolute
/// path guarantees that the same filename in two folders maps to different
/// entries; a canonicalize failure (e.g. a missing file) falls back to the
/// lossy path string, which is still folder-scoped.
fn thumbnail_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// LRPAR-G15-IPTC-S8: open prompt dialog for a dynamic meta preset — one
/// required input per declared placeholder. Session-only; Cancel discards
/// the whole dialog without touching any sidecar.
#[derive(Debug, Clone)]
pub struct MetaPresetDialog {
    pub spec: String,
    pub name: String,
    pub placeholders: Vec<(String, String)>,
    pub vars: BTreeMap<String, String>,
    pub error: Option<String>,
}

/// LRPAR-G15-IPTC-S8: cached embedded IPTC read for the Metadata panel
/// (see `LuminaApp::meta_embedded_cache`). Session-only, never persisted.
#[derive(Debug, Clone)]
struct EmbeddedCache {
    path: String,
    len: u64,
    mtime: Option<std::time::SystemTime>,
    result: Result<Option<IptcMetadata>, String>,
}

/// GUI-FILMSTRIP-SYNC-1: per-image outcome of a selection sync/match run.
/// `applied` holds the display-string paths whose sidecar was written;
/// `failed` holds `(path, message)` pairs — every failure is loud (surfaced
/// via `error!` at the call site and summarized in the status line), and a
/// failure never aborts the remaining targets.
#[derive(Debug, Clone, Default)]
pub struct SelectionSyncReport {
    pub applied: Vec<String>,
    pub failed: Vec<(String, String)>,
}

impl SelectionSyncReport {
    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }

    pub fn failed_count(&self) -> usize {
        self.failed.len()
    }
}

/// LRPAR-G08-PREVIOUS: cross-image Previous reference — the image edited
/// immediately before the current one (path + recipe snapshot taken when the
/// image was displaced by a successful load). Session-only, never persisted
/// (like the copy/paste `settings_clipboard`); an explicit Vorbild is chosen
/// by opening it (open Vorbild, then open the target).
#[derive(Debug, Clone)]
pub struct PreviousReference {
    pub path: String,
    pub recipe: EditRecipe,
}

/// LRPAR-G08-PREVIOUS: portable history extras for a Previous step —
/// `step = "previous"` plus the reference *file name* (`source`). Only the
/// file name is stored, never a path (absolute paths are forbidden in
/// persistent recipe data) — same contract as the CLI `previous` command.
fn previous_history_extras(source_path: &str) -> BTreeMap<String, Value> {
    let source = Path::new(source_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("reference")
        .to_string();
    let mut extras = BTreeMap::new();
    extras.insert("step".into(), Value::String("previous".into()));
    extras.insert("source".into(), Value::String(source));
    extras
}

/// The default virtual copy of `document` (first copy when no default is
/// flagged). `None` only when the document carries no copies at all.
fn default_copy_mut(document: &mut SidecarDocument) -> Option<&mut lumina_sidecar::VirtualCopy> {
    if document.virtual_copies.is_empty() {
        return None;
    }
    let index = document
        .virtual_copies
        .iter()
        .position(|copy| copy.is_default)
        .unwrap_or(0);
    document.virtual_copies.get_mut(index)
}

/// Decode a selection target: `(raw bytes, frame, orientation)`. RAW names go
/// through the native LibRaw adapter, everything else through the raster
/// decoder. Errors are message strings so the per-image report stays loud
/// without a `GuiError` roundtrip.
fn decode_selection_frame(path: &Path) -> Result<(Vec<u8>, ImageFrame, u8), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if is_raw_name(name) {
        let image = lumina_raw::decode_bytes(&bytes, name).map_err(|error| error.to_string())?;
        let orientation = image.metadata.orientation;
        Ok((bytes, image.frame, orientation))
    } else {
        let frame = ImageFrame::decode(&bytes).map_err(|error| error.to_string())?;
        Ok((bytes, frame, 1))
    }
}

/// Source identity for a freshly created selection sidecar, mirroring
/// [`LuminaApp::source_identity`] without requiring loaded-app state.
fn selection_source_identity(
    name: &str,
    bytes: &[u8],
    frame: &ImageFrame,
    orientation: u8,
    source_is_raw: bool,
) -> SourceIdentity {
    SourceIdentity {
        relative_name: name.to_string(),
        content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        byte_length: bytes.len() as u64,
        modified_at: None,
        raw_format: Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("raster")
            .to_ascii_uppercase(),
        orientation,
        decode_fingerprint: DecodeFingerprint {
            decoder: decoder_identity(source_is_raw).into(),
            version: if source_is_raw {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").into()
            },
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    }
}

impl FileBrowserEntry {
    fn status_label(&self) -> &'static str {
        if self.conflict {
            Str::StatusConflict.t()
        } else if self.is_offline() {
            Str::StatusOffline.t()
        } else if self.has_sidecar {
            Str::Sidecar.t()
        } else {
            Str::StatusWithout.t()
        }
    }
    fn is_offline(&self) -> bool {
        matches!(self.source_status, SourceStatus::Missing)
    }
    /// Stable thumbnail key (canonicalized absolute path). Read-only access for
    /// headless scheduling tests/diagnostics (GUI-SCROLL-200-1).
    pub fn thumb_key(&self) -> &str {
        &self.thumb_key
    }

    /// UX-SLICE-2 (F4): the LR-01 rating/flag/color-label badge text painted
    /// over this cell (Library grid + filmstrip), or `None` for a clean cell.
    /// Read-only access so the pixel golden's non-vacuous guard can assert the
    /// rated fixture really carries badges before the snapshot.
    pub fn badge_text(&self) -> Option<String> {
        entry_badge_text(self)
    }
}

/// PERF-GUI-7: result of a background RAW/raster decode. Carries the decoded
/// frame plus the metadata needed to apply it on the main thread (and to load
/// the matching sidecar). `Err` carries the (path, message) so the GUI can show
/// the decode failure without blocking the worker thread.
struct DecodedFrame {
    path: String,
    name: String,
    bytes: Vec<u8>,
    frame: ImageFrame,
    orientation: u8,
    camera_white_balance: Option<[f32; 4]>,
    source_is_raw: bool,
    /// EXIF lens identity for the Lensfun auto-profile resolution (G-06):
    /// populated for RAW sources from the decoder metadata, `None` for
    /// raster sources (no EXIF) and when the metadata is unusable. Best
    /// effort — never a decode failure.
    lens_identity: Option<LensIdentity>,
}

/// EXIF lens identity snapshot driving the Lensfun auto-profile resolution
/// (G-06, LRPAR-G06-GEO). Plain data (no lensfun types) so the decode path
/// stays feature-independent; the corrector is built from it at render
/// time behind the `lensfun` feature.
#[derive(Debug, Clone, PartialEq)]
struct LensIdentity {
    camera_make: Option<String>,
    camera_model: Option<String>,
    lens: Option<String>,
    focal_length: Option<f32>,
    aperture: Option<f32>,
}

/// Snapshot helper: `None` when the metadata carries no lens-relevant EXIF
/// at all (raster-equivalent); otherwise the identity, even partial (the
/// corrector build enforces the required completeness strictly).
fn lens_identity_from_metadata(metadata: &lumina_raw::RawMetadata) -> Option<LensIdentity> {
    let identity = LensIdentity {
        camera_make: metadata.camera_make.clone(),
        camera_model: metadata.camera_model.clone(),
        lens: metadata.lens.clone(),
        focal_length: metadata.focal_length,
        aperture: metadata.aperture,
    };
    if identity.camera_make.is_none()
        && identity.camera_model.is_none()
        && identity.lens.is_none()
        && identity.focal_length.is_none()
        && identity.aperture.is_none()
    {
        None
    } else {
        Some(identity)
    }
}

/// Cached Lensfun auto-corrector pair (G-06, `lensfun` feature only).
/// The database handle is kept alive alongside the corrector (the modifier
/// references DB-owned lens data); field order matters — `corrector`
/// (modifier destroy) drops before `_db`.
#[cfg(feature = "lensfun")]
struct CachedLensCorrector {
    corrector: lumina_lensfun::Corrector,
    _db: lumina_lensfun::LensfunDb,
    /// Identity + frame dimensions this corrector was built for.
    key: (
        Option<String>,
        Option<String>,
        Option<String>,
        u32,
        u32,
        u32,
        u32,
    ),
    /// GUI-LENSFUN-GATE-1 / GPU-LENSFUN-PARITY-1: whether this corrector
    /// changes pixels (`!Corrector::is_identity()`, probing
    /// distortion/vignetting/TCA). Computed once at build time so neither the
    /// present gate nor the per-frame map bind pays the FFI probe; mirrors the
    /// CLI's `lensfun_corrector_active`. An inactive (identity) corrector is a
    /// no-op on the CPU oracle and binds no map, so the manual model stays in
    /// effect on both paths. GPU-only: the non-GPU build has no bind path that
    /// could consume it.
    #[cfg(feature = "gpu")]
    active: bool,
    /// GPU-LENSFUN-PARITY-1: CPU-precomputed warp/gain map for this corrector at
    /// the dimensions it was last built for (`LensfunMap::from_corrector`).
    /// Built lazily by [`lensfun_gpu::bind`] on the GPU present path and reused
    /// across frames/tool moves (the per-pixel FFI build is expensive); `None`
    /// until first use. Fine to keep on the CPU: the map is a pure derived
    /// artifact, the recipe/sidecar stay authoritative.
    #[cfg(feature = "gpu")]
    gpu_map: Option<lumina_core::LensfunMap>,
}

type DecodeResult = Result<DecodedFrame, (String, String)>;

/// Draw method of one Develop section: `fn(&mut LuminaApp, &mut egui::Ui)`.
/// Factored out so [`LuminaApp::DEVELOP_SECTIONS`] stays readable.
type DevelopSectionDraw = fn(&mut LuminaApp, &mut egui::Ui);

impl LuminaApp {
    /// KITTEST-COVERAGE-STATES-1: set the Export panel's output format. Pure
    /// display/panel state (the combo's own handler performs the same
    /// assignment); never a recipe or sidecar write.
    pub fn set_export_format(&mut self, format: ImageFileFormat) {
        trace!("GUI interaction: set_export_format {:?}", format);
        self.export_format = format;
    }

    pub fn new(_ctx: egui::Context) -> Self {
        // PERF-FILMSTRIP: spin up the dedicated thumbnail thread pool
        // (R2-MODSWITCH-1 F7: moved to `thumb_worker::spawn_thumbnail_pool`).
        let (thumbnail_tx, thumbnail_rx) = thumb_worker::spawn_thumbnail_pool();
        Self {
            original: None,
            preview: None,
            source_bytes: None,
            source_is_raw: false,
            raw_orientation: 1,
            camera_white_balance: None,
            source_name: String::new(),
            path: String::new(),
            directory: ".".into(),
            entries: Vec::new(),
            recipe: EditRecipe::default(),
            generative_artifacts: GenerativeArtifacts::default(),
            generative_role_status: [GenerativeRoleStatus::Missing; 2],
            generative_memo: None,
            texture: None,
            // R2-GUIMOD-02: no CPU pixels uploaded yet (see `texture_identity`).
            texture_identity: None,
            // KITTEST-COVERAGE-OVERLAYS-1: overlay matte uploaded lazily.
            mask_overlay_texture: None,
            navigator_texture: None,
            navigator_texture_key: None,
            navigator_overview: None,
            navigator_overview_key: None,
            preview_generation: 0,
            status: Str::ReadyForImage.t().into(),
            error: None,
            error_dialog: false,
            draft_error_dedup: None,
            draft_error_repeat_warned: false,
            render_key: None,
            tone_analysis: None,
            preview_histogram: None,
            pending_slider_commit: None,
            pending_history_step: None,
            loaded_lens_identity: None,
            #[cfg(feature = "lensfun")]
            lensfun_cache: None,
            render_mask_layers: Vec::new(),
            document: None,
            virtual_copy_id: "vc-original".into(),
            selected_mask_id: None,
            mask_name_input: String::new(),
            mask_tool: MaskTool::None,
            brush_radius: 0.05,
            brush_eraser: false,
            pending_brush_marks: Vec::new(),
            drag_start: None,
            drag_current: None,
            drawing: false,
            spot_tool: SpotTool::None,
            spot_mode: SpotMode::Heuristic,
            spot_radius: 18.0,
            spot_feather: 0.5,
            spot_opacity: 1.0,
            spot_detect_threshold: 0.5,
            spot_detect_status: String::new(),
            spot_gen_prompt: String::new(),
            spot_gen_seed: 7,
            spot_gen_variant: 1,
            spot_gen_status: String::new(),
            spot_gen_target: String::new(),
            preset_name: String::new(),
            preset_fields: BTreeMap::from([
                ("exposure".into(), true),
                ("contrast".into(), true),
                ("highlights".into(), false),
                ("shadows".into(), false),
            ]),
            preset_relative_exposure: false,
            presets_dir: presets::default_presets_dir(),
            // F-009: initial directory scan so saved presets survive restarts.
            // A scan error surfaces through the entry list, never silently.
            preset_entries: presets::default_presets_dir()
                .as_deref()
                .map(presets::scan_presets_dir)
                .unwrap_or_default(),
            idle_queue: IdleQueue::new(32),
            thumbnail_tx,
            thumbnail_rx,
            // R2-MODSWITCH-1 F7: empty per-folder preview index (built lazily).
            thumbnail_cache: thumb_cache::PreviewIndexCache::default(),
            active_module: Module::Develop,
            // R2-MODSWITCH-1 F7: no scheduler run yet.
            last_scheduled_module: None,
            pending_load_path: None,
            warmup: warmup::WarmupState::default(),
            timing: timing::TimingState::default(),
            export_path: String::new(),
            export_format: ImageFileFormat::Png,
            export_quality: 90,
            before_after: false,
            settings_clipboard: None,
            previous_reference: None,
            history_timestamp_override: None,
            clipping_overlay: false,
            lights_out: false,
            panels_hidden: false,
            crop_mode: false,
            all_panels_hidden: false,
            overlay_mode: OverlayMode::Always,
            pin_visibility: PinVisibility::Auto,
            solo_mode: false,
            section_open: [false; SECTION_COUNT],
            show_mask_overlay: true,
            overlay_color: [255, 0, 0],
            ai_select_kind: AiSelectKind::Subject,
            ai_detail_input: String::new(),
            ai_name_input: String::new(),
            lum_name_input: String::new(),
            lum_min: 0.0,
            lum_max: 1.0,
            lum_feather: 0.0,
            col_name_input: String::new(),
            col_hue_center: 0.0,
            col_hue_width: 60.0,
            col_sat_min: 0.0,
            col_sat_max: 1.0,
            col_lum_min: 0.0,
            col_lum_max: 1.0,
            col_feather: 0.0,
            combine_other_id: String::new(),
            combine_name_input: String::new(),
            duplicate_name_input: String::new(),
            selected_group_id: None,
            group_name_input: String::new(),
            group_member_selection: BTreeSet::new(),
            group_feather_offset: 0.0,
            group_density_offset: 0.0,
            tone_curve_channel: 0,
            filter_bar_visible: false,
            library_filter: String::new(),
            compare_mode: None,
            library_view: LibraryView::Grid,
            library_sort: LibrarySort::Name,
            library_sort_order: Vec::new(),
            before_after_split: false,
            fullscreen: false,
            softproof_preview: false,
            show_original_histogram: false,
            masking_preview: None,
            reset_sliders_automatically: false,
            recipe_baseline: None,
            mask_baseline: Vec::new(),
            wb_pick_mode: false,
            red_eye_pick_mode: false,
            red_eye_detect_status: String::new(),
            thumbnails: ThumbnailManager::new(),
            filmstrip_selection: BTreeSet::new(),
            filmstrip_anchor: None,
            preview_is_draft: false,
            draft_original: None,
            draft_throttle: DraftThrottle::default(),
            base_stage_cache: StageFrameCache::new(BASE_STAGE_CACHE_MAX_BYTES),
            source_hash_memo: None,
            last_stage_work: None,
            last_analysis_ms: 0.0,
            last_drag_tick: None,
            preview_cap_state: preview_size::PreviewCapState::default(),
            last_edit_time: 0.0,
            preview_zoom: 1.0,
            zoom_mode: ZoomMode::Fit,
            preview_pan: egui::Vec2::ZERO,
            preview_roi: None,
            preview_render_src: None,
            preview_base_fit_scale: 1.0,
            preview_pane_w: 800.0,
            preview_pane_h: 600.0,
            preview_src_w: 1.0,
            preview_src_h: 1.0,
            preview_effective_scale: 1.0,
            preview_screen_rect: None,
            preview_pane_rect: None,
            overlay_full_rect: None,
            // GUI-VIEW-2 (N6): the navigator rail (overview + viewport
            // rectangle, F-100) is visible by default — Lightroom-like — and
            // stays collapsible via the preview toolbar toggle. Default-hidden
            // made the viewport rectangle unfindable.
            navigator_open: true,
            open_folders: BTreeSet::new(),
            folder_children: BTreeMap::new(),
            folder_raw_counts: BTreeMap::new(),
            folder_picker: None,
            library_thumb_size: 132.0,
            library_cols: 4,
            keyword_input: String::new(),
            collection_id_input: String::new(),
            collection_name_input: String::new(),
            active_collection: None,
            smart_catalog: Vec::new(),
            smart_catalog_path: String::new(),
            smart_id_input: String::new(),
            smart_name_input: String::new(),
            smart_rule_kind: "keyword".to_string(),
            smart_rule_value: String::new(),
            smart_rule_stack: Vec::new(),
            batch_kind: "add_keyword".to_string(),
            batch_value: String::new(),
            meta_buffers: BTreeMap::new(),
            meta_buffers_key: None,
            meta_buffers_dirty: false,
            meta_presets_dir_override: None,
            meta_preset_entries: Vec::new(),
            selected_meta_preset: String::new(),
            meta_preset_dialog: None,
            meta_sync_fields: default_meta_sync_fields(),
            meta_clipboard: None,
            meta_embedded_cache: None,
            history_selected: None,
            decode_rx: None,
            scan_rx: None,
            scan_generation: 0,
            scan_pending: false,
            sidecar_revision: None,
            pending_full_render: false,
            auto_load_attempted: false,
            #[cfg(feature = "gpu")]
            // R2-GUIMOD-09: deliberately `None` here. Constructing a standalone
            // `GpuContext` performs a blocking adapter/device request that
            // `attach_wgpu_render_state` immediately replaced with the
            // renderer-shared context — two full GPU inits per startup. The
            // context is now created exactly once, inside
            // [`attach_wgpu_render_state`] (native entry point wires it right
            // after construction; headless tests stay GPU-free).
            gpu: None,
            #[cfg(feature = "gpu")]
            wgpu_render_state: None,
            #[cfg(feature = "gpu")]
            present_target: None,
            #[cfg(feature = "gpu")]
            vram_fresh: false,
            #[cfg(feature = "gpu")]
            gpu_stage_gate: None,
            #[cfg(feature = "gpu")]
            vram_mask_is_evaluated: false,
            #[cfg(feature = "gpu")]
            gpu_present_frame: None,
            #[cfg(feature = "gpu")]
            // R2-GUIMOD-06: no routing fallback until a present decision runs.
            gpu_route_fallback: None,
            #[cfg(feature = "gpu")]
            // GUI-LENSFUN-GATE-3 (F1): no VRAM render was attempted yet.
            vram_render_refusal: None,
            #[cfg(feature = "gpu")]
            // PARITY-PATHS-2: report the real adapter state until a test forces it.
            gpu_adapter_override: None,
            #[cfg(feature = "gpu")]
            brush_mask_plane: None,
            #[cfg(feature = "gpu")]
            brush_mask_plane_dims: None,
            frame_thumb_enqueued: 0,
            frame_thumbs_ready: 0,
            // PREVIEW-CACHE-FEATURE: lazy — no worker pool until the first
            // neighbor prefetch (keeps headless tests thread-free).
            preview_ctrl: None,
            frame_previews_enqueued: 0,
            frame_previews_ready: 0,
            // GUI-TOAST-OVERLAP-1: no toast until the first background event.
            toast_message: None,
            toast_until: 0.0,
            // LRPAR-G14-DENOISE-IMPL-20: no active stage until the recipe
            // carries one; the default policy is `Warn` (CLI parity, §6).
            denoise_gui: denoise_gui::DenoiseGuiState::inactive(
                lumina_core::DenoisePolicy::Warn,
            ),
            denoise_live_model: None,
            denoise_gui_dirty: false,
            people_filter: String::new(),
            people_selected_cluster: String::new(),
            face_crop_textures: BTreeMap::new(),
            people_name_input: String::new(),
            people_split_subset: String::new(),
            people_merge_target: String::new(),
            merge_job: None,
            merge_status: String::new(),
        }
    }

    pub fn recipe(&self) -> &EditRecipe {
        &self.recipe
    }

    /// Monotonic counter of how many times `self.preview` received new content
    /// (bumped in `render_from`). Exposed read-only for headless integration
    /// tests (F-103-N9 interaction tests) to assert that an edit re-renders.
    pub fn preview_generation(&self) -> u64 {
        self.preview_generation
    }

    /// GUI-SIDECAR-READ-1: flush an armed slider commit before the loaded
    /// source changes. `apply_decoded_frame` drops `pending_slider_commit`
    /// (fresh lineage), so switching images with an uncommitted drag — or
    /// edits made while a background decode is in flight — would silently
    /// lose the edit. Flushing here renders the current state and saves it
    /// to the *currently loaded* path (which is still adopted at this
    /// point). No-op unless a commit is armed on a loaded file-backed image.
    fn flush_pending_edit(&mut self) {
        if self.pending_slider_commit.is_none()
            || self.original.is_none()
            || self.path.trim().is_empty()
        {
            return;
        }
        // LRPAR-G01-BASIC: with "Reset Sliders Automatically" armed, an image
        // switch discards the armed-but-uncommitted edit (sliders reset)
        // instead of flushing it to the previous image's sidecar.
        // Persisted state is untouched either way.
        if self.reset_sliders_automatically {
            self.pending_slider_commit = None;
            self.pending_history_step = None;
            info!("{}", Str::ResetSlidersDropped.t());
            self.status = Str::ResetSlidersDropped.t().into();
            return;
        }
        trace!("GUI save: flushing pending edit before source change");
        self.commit_pending_slider_save([0, 0]);
    }

    pub fn open_file(&mut self, path: impl Into<String>) {
        let p = path.into();
        trace!("GUI interaction: open_file {}", p);
        // REVIEW-GUI-PATHDESYNC-1: `self.path` is NOT committed here. The decode
        // runs asynchronously; adopting the new path before `finish_decode`
        // would let Save Recipe / Export / mask fingerprints write the still-
        // loaded image-A state under the new path B (phantom sidecar) — and on
        // a failed decode the path would point at a file that never loaded.
        // `finish_decode` commits the path only after a successful decode, so
        // every write path stays consistent with original/document/recipe.
        // GUI-SIDECAR-READ-1: flush an armed commit to the still-loaded image
        // before the switch starts — otherwise the drag edit is dropped by
        // `apply_decoded_frame` when the new frame lands.
        self.flush_pending_edit();
        // Populate the file browser with the directory containing the opened file.
        // GUI-VIEW-2: rescan only when actually navigating (new directory or
        // no entries yet). A same-folder switch (filmstrip clicks) reuses the
        // live entries — our own saves keep them fresh via `refresh_entry` —
        // instead of re-reading + re-hashing every source (the N6 stall:
        // ~224 ms per switch with hashed sidecars). External folder changes
        // still surface via Open/Refresh/`set_directory` rescans.
        if let Some(parent) = Path::new(&p).parent() {
            let dir = parent.display().to_string();
            // LRPAR-G01-BASIC: the reset-sliders flag is folder-inherited —
            // refresh it for the target folder on every open (no-op without
            // a settings file).
            self.refresh_reset_sliders_flag(parent);
            if dir != self.directory || self.entries.is_empty() {
                self.directory = dir;
                // GUI-STARTUP-SELECTION-1: an explicit open discharges the
                // startup load itself — the scan's auto-load is suppressed so
                // it can neither start a second decode nor select a different
                // first entry (selection and the loading path stay consistent,
                // like the click path, which sets the selection beforehand).
                // Seeding keeps any multi-selection; `p` is ensured a member
                // (the file dialog / drop path never sets it).
                if !self.filmstrip_selection.contains(&p) {
                    self.filmstrip_selection.insert(p.clone());
                    self.filmstrip_anchor = Some(p.clone());
                }
                self.auto_load_attempted = true;
                self.list_directory();
            } else {
                self.directory = dir;
            }
        }
        // PERF-GUI-7: decode off the main thread so switching files never
        // blocks the UI; the decoded frame is delivered via `decode_rx` and
        // applied in `update()`/`poll_decode()`.
        self.begin_load_path(p);
    }

    /// Navigate to `directory` (folder tree click, `Open`, startup workdir).
    /// R4-LIB-1(a): the grid aggregates subfolder images (F-100), so every
    /// navigation path uses the recursive listing (depth-limited; `.lumina/`
    /// excluded) — the former flat tree-click listing hid subfolder images.
    pub fn set_directory(&mut self, directory: impl Into<String>) {
        self.directory = directory.into();
        info!("directory set: {}", self.directory);
        // LRPAR-G01-BASIC: keep the folder-inherited reset-sliders flag in
        // sync on explicit navigation (same refresh as `open_file`).
        let folder = PathBuf::from(self.directory.trim());
        self.refresh_reset_sliders_flag(&folder);
        self.list_directory();
    }

    /// Current working directory (read-only accessor for the `main()` startup
    /// wiring and headless tests; mirrors [`Self::set_directory`]).
    pub fn directory(&self) -> &str {
        &self.directory
    }

    /// UX-SLICE-2 (F2): install a deterministic folder-picker seam for headless
    /// tests (a real `rfd` dialog needs a display server). Production leaves
    /// this unset and [`Self::open_folder`] opens the native dialog.
    pub fn set_folder_picker(&mut self, picker: impl FnMut() -> Option<PathBuf> + 'static) {
        self.folder_picker = Some(Box::new(picker));
    }

    /// UX-SLICE-2 (F2): the truthful "Open Folder" action — pick a directory
    /// through the native dialog (or the injected test seam) and re-list it.
    /// A cancelled dialog is a deliberate no-op: the previous directory and its
    /// state stay untouched, and no status/error is fabricated.
    pub fn open_folder(&mut self) {
        let picked = match self.folder_picker.as_mut() {
            Some(picker) => picker(),
            None => rfd::FileDialog::new().pick_folder(),
        };
        let Some(folder) = picked else {
            trace!("GUI interaction: open folder cancelled");
            return;
        };
        info!("open folder: {}", folder.display());
        self.set_directory(folder.display().to_string());
    }

    /// Recursive aggregation (F-100 Library): images of the chosen folder
    /// *including* subfolders, symlink-/loop-safe via a canonical visited
    /// set, depth-limited by `FOLDER_SCAN_DEPTH`. Each entry carries its
    /// relative subfolder in [`FileBrowserEntry::folder`] for the grid path
    /// badge. The RAW-only grid decision is unchanged — only aggregation.
    pub fn list_directory(&mut self) {
        self.request_scan(true);
    }

    /// Flat single-folder listing behind [`Self::set_directory`].
    pub fn list_directory_flat(&mut self) {
        self.request_scan(false);
    }

    /// R2-MODSWITCH-1 F8: request a folder scan. Production runs it on the
    /// worker thread ([`Self::begin_scan`], drained by [`Self::poll_scan`]) so
    /// the UI thread never blocks. Headless tests have no event loop, so under
    /// `cfg(test)` the same scan engine runs synchronously
    /// ([`Self::scan_directory_blocking`]) — a test seam, not a production
    /// fallback. The async worker path itself is covered by the dedicated
    /// `begin_scan`/`poll_scan` tests (`tests/r3_f8_scan.rs`).
    fn request_scan(&mut self, recursive: bool) {
        #[cfg(test)]
        self.scan_directory_blocking(recursive);
        #[cfg(not(test))]
        {
            let _ = self.begin_scan(recursive);
        }
    }

    fn apply_listing(&mut self, directory: std::path::PathBuf, mut entries: Vec<FileBrowserEntry>) {
        debug!("listing directory: {}", directory.display());
        // REVIEW-GUI-THUMB-1: drop cached thumbnails of a previous folder so
        // they neither resurface nor accumulate unboundedly across a session.
        self.thumbnails
            .ensure_directory(&directory.to_string_lossy());
        // R2-MODSWITCH-1 F7: same for the metadata-only preview index.
        self.thumbnail_cache
            .ensure_directory(&directory.to_string_lossy());
        // PREVIEW-CACHE-FEATURE: a *directory change* invalidates the neighbor
        // cache state (RAM LRU, in-flight, failures) — stale entries of another
        // folder must neither resurface nor ever be shown. Relisting the same
        // directory during navigation keeps the warm LRU so a change-of-active
        // is served as an instant cache hit (A1).
        if let Some(ctrl) = self.preview_ctrl.as_mut() {
            ctrl.ensure_directory(self.directory.trim());
        }
        match std::fs::read_dir(&directory) {
            Ok(_) => {
                // LRPAR-G09-SORT-09: restore the folder's persisted sort mode +
                // custom order (loud on a corrupt file; defaults Name/empty).
                let sort_error = self.load_library_sort_for(&directory);
                crate::library_sort::sort_entries_in(
                    &directory,
                    &mut entries,
                    self.library_sort,
                    &self.library_sort_order,
                );
                // GUI-STARTUP-SELECTION-1: remember the grid position of a
                // single selection so a rescan that prunes it (e.g. the
                // selected file was deleted on disk) can fall back to its
                // successor instead of going empty while images remain.
                let removed_index = if self.filmstrip_selection.len() == 1 {
                    let selected = self
                        .filmstrip_selection
                        .iter()
                        .next()
                        .expect("single selection has one element");
                    self.entries
                        .iter()
                        .map(|e| e.path.display().to_string())
                        .position(|path| &path == selected)
                } else {
                    None
                };
                self.entries = entries;
                self.status = Str::ImagesInDirectory.format_arg(&self.entries.len().to_string());
                // LRPAR-G09-SORT-09: a corrupt sort-order file is surfaced
                // visibly here (it was logged loudly in `load_library_sort_for`)
                // instead of silently falling back to `Name`.
                if let Some(message) = sort_error {
                    self.status = message;
                }
                self.stabilize_selection(removed_index);
                // PERF-GUI-6: when no specific file was requested (e.g. the user
                // picked a directory, not a single image) and nothing is loaded
                // yet, auto-load the first grid entry so the Develop module
                // shows an image immediately — no manual click required.
                //
                // GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): this covers
                // ALL supported formats (not just RAW — `entries` only ever
                // holds `is_supported_image` paths) and selects exactly like a
                // plain click (single selection + anchor) so selection and the
                // loading path can never desync. Decode failures surface loudly
                // through `finish_decode`/`show_error` — never a silent
                // fallback.
                //
                // Robustness guards:
                // * `!self.auto_load_attempted` — run the auto-load at most once
                //   per session so rescanning the directory never restarts a
                //   decode that is already in flight.
                // * `self.decode_rx.is_none()` — a decode is already pending
                //   (async), so we must not start a second one; `original` stays
                //   `None` until the in-flight decode's `finish_decode` runs.
                // If no entry exists yet we deliberately leave
                // `auto_load_attempted` unset so a later, now-populated scan can
                // still auto-load.
                if !self.auto_load_attempted
                    && self.path.is_empty()
                    && self.original.is_none()
                    && self.decode_rx.is_none()
                    && !self.entries.is_empty()
                {
                    let first = self.entries[0].path.display().to_string();
                    debug!("auto-loading first entry after list_directory: {}", first);
                    self.filmstrip_selection = BTreeSet::from([first.clone()]);
                    self.filmstrip_anchor = Some(first.clone());
                    self.begin_load_path(first);
                    self.auto_load_attempted = true;
                }
            }
            Err(error) => {
                self.entries.clear();
                self.status = Str::DirectoryNotReadable.format_arg(&error.to_string());
            }
        }
    }

    /// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): keep the selection
    /// non-empty while images exist. Prunes paths that no longer list
    /// (deleted/moved on disk), then — only when the prune emptied the
    /// selection — re-selects: the still-listed loaded image first (path vs.
    /// selection consistency, without triggering a resync decode), otherwise
    /// the successor at the removed grid position (clamped to the new last
    /// entry), otherwise the first grid entry. Clears everything only when no
    /// entries remain. Never starts a decode itself; loading stays with the
    /// auto-load in [`Self::apply_listing`] and the explicit
    /// [`Self::open_file`] path.
    fn stabilize_selection(&mut self, removed_index: Option<usize>) {
        let live: BTreeSet<String> = self
            .entries
            .iter()
            .map(|entry| entry.path.display().to_string())
            .collect();
        self.filmstrip_selection.retain(|path| live.contains(path));
        if self
            .filmstrip_anchor
            .as_ref()
            .is_some_and(|anchor| !live.contains(anchor))
        {
            self.filmstrip_anchor = None;
        }
        if self.entries.is_empty() {
            self.filmstrip_selection.clear();
            self.filmstrip_anchor = None;
            return;
        }
        if !self.filmstrip_selection.is_empty() {
            return;
        }
        if !self.path.is_empty() && live.contains(&self.path) {
            self.filmstrip_anchor = Some(self.path.clone());
            self.filmstrip_selection.insert(self.path.clone());
            return;
        }
        let index = removed_index.unwrap_or(0).min(self.entries.len() - 1);
        let target = self.entries[index].path.display().to_string();
        self.filmstrip_anchor = Some(target.clone());
        self.filmstrip_selection.insert(target);
    }

    /// Re-scan a single file into `self.entries` (in place, order-preserving).
    /// Used after a save so the browser reflects the new sidecar state
    /// without a full directory rescan — `list_directory` re-reads and
    /// re-hashes *every* source file via `source_status`, which stalls the UI
    /// on folders with large RAWs on every slider-commit save (GUI-VIEW-2,
    /// N6 Develop→Library/save stall class).
    fn refresh_entry(&mut self, path: &Path) {
        let Some(mut scanned) = library_scan::scan_entry(path) else {
            return;
        };
        // VIEW-2 single-file refresh: keep the subfolder badge consistent
        // with a full listing (no rescan here — that is the point).
        scanned.folder = folder_badge(&PathBuf::from(self.directory.trim()), path);
        if let Some(slot) = self.entries.iter_mut().find(|e| e.path == scanned.path) {
            *slot = scanned;
        } else {
            self.entries.push(scanned);
            // LRPAR-G09-SORT-09: keep the active sort order for a new entry.
            self.sort_entries_now();
        }
    }

    pub fn entries(&self) -> &[FileBrowserEntry] {
        &self.entries
    }

    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn idle_queue(&self) -> &IdleQueue {
        &self.idle_queue
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    /// KITTEST-COVERAGE-STATES-2 (a): whether the current error is surfaced as
    /// the blocking popup dialog (explicit user-action failure) rather than only
    /// the header banner (background decode/listing failure). Read-only
    /// diagnostic for headless tests; the rendering itself lives in
    /// [`Self::draw_error_dialog`].
    pub fn error_dialog_open(&self) -> bool {
        self.error_dialog
    }
    pub fn preview(&self) -> Option<&ImageFrame> {
        self.preview.as_ref()
    }
    /// REVIEW-GUI-N5: whether the current preview is a low-resolution draft
    /// (slider drag in flight). Consumers of the preview pixels (histogram,
    /// exposure matching) must check this so a draft is never silently
    /// measured as if it were the final render.
    pub fn preview_is_draft(&self) -> bool {
        self.preview_is_draft
    }
    /// KITTEST-PARITY-PATHS-1: the on-screen rect the preview image was last
    /// painted into, or `None` before the first painted frame / while the empty
    /// state is shown. The rect is identical for the CPU-texture and the
    /// GPU-VRAM present path (both blit the same `rect`), so it is the absolute
    /// geometry anchor the path-parity framework asserts against. Pure readout:
    /// it is never rendered as text or a badge.
    #[must_use]
    pub fn preview_screen_rect(&self) -> Option<egui::Rect> {
        self.preview_screen_rect
    }
    /// KITTEST-PARITY-PATHS-1: the pane rect the preview was fitted into for the
    /// last painted frame (companion of [`Self::preview_screen_rect`]).
    #[must_use]
    pub fn preview_pane_rect(&self) -> Option<egui::Rect> {
        self.preview_pane_rect
    }
    /// KITTEST-PARITY-PATHS-1: the full-source rect the last painted frame
    /// mapped its overlays (mask matte, edit pins, lens-blur focus, crop) onto.
    /// At Fit with no ROI it equals [`Self::preview_screen_rect`]; the absolute
    /// overlay-on-photo check compares the two so a both-paths-wrong placement
    /// (which CPU↔GPU parity alone cannot catch) still fails.
    #[must_use]
    pub fn overlay_full_rect(&self) -> Option<egui::Rect> {
        self.overlay_full_rect
    }
    /// R2-GUIMOD-04a: milliseconds of the analysis pass inside the last render.
    pub fn last_analysis_ms(&self) -> f64 {
        self.last_analysis_ms
    }
    pub fn render_key(&self) -> Option<&RenderKey> {
        self.render_key.as_ref()
    }

    /// UX-SLICE-2 (F1): whether the header render hash is meaningful right now.
    /// The Library grid is RAW-only, so a loaded non-RAW image has no Library
    /// representation; a hash above the "No images" empty state would
    /// contradict it. The hash is therefore hidden in Library exactly while the
    /// visible raster is empty. UX-SLICE-3 (F1 follow-up): "empty" is the same
    /// predicate the empty state itself uses ([`Self::filtered_library_order`]
    /// — RAW-only display order narrowed by the active collection view and the
    /// `\` query), so a zero-match filter hides the hash just like an empty
    /// listing; keying on the unfiltered `entries` previously let the hash
    /// reappear above the filtered empty state. Develop/Export and any
    /// non-empty raster keep it. The underlying `render_key` is never cleared
    /// — this is a pure display gate (the loaded render stays valid).
    pub fn render_hash_visible(&self) -> bool {
        if self.render_key.is_none() {
            return false;
        }
        !(self.active_module == Module::Library && self.filtered_library_order().is_empty())
    }

    /// GUI-DEBUG-SWEEP-1: the internal render key is no longer painted as
    /// header text — it is exposed as a tooltip on the app status line. Returns
    /// the tooltip text when the hash is meaningful (same gate as
    /// [`Self::render_hash_visible`]), else `None`. Kept as a pure readout so
    /// the gate stays pinned by tests without hovering.
    pub fn render_hash_tooltip(&self) -> Option<String> {
        if !self.render_hash_visible() {
            return None;
        }
        self.render_key
            .as_ref()
            .map(|key| Str::RenderStateCurrent.format_arg(&key.digest()[..12]))
    }

    /// PERF-GUI-1: number of cached base-stage frames (diagnostics/tests).
    pub fn base_stage_cache_len(&self) -> usize {
        self.base_stage_cache.len()
    }

    /// PERF-GUI-1: drops every cached base-stage frame. Pure memory-pressure
    /// hygiene — the next render rebuilds the base from the decoded source,
    /// which changes no pixels (cache-miss is a performance event, never a
    /// fallback).
    pub fn clear_preview_stage_cache(&mut self) {
        self.base_stage_cache.clear();
    }

    /// PERF-GUI-1: stage work counters of the last completed render.
    pub fn last_stage_work(&self) -> Option<StageWork> {
        self.last_stage_work
    }
    pub fn tone_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        self.tone_analysis
    }

    pub fn duplicate_virtual_copy(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<(), GuiError> {
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        document.duplicate_virtual_copy(&self.virtual_copy_id, id, name)?;
        Ok(())
    }

    /// Set the star rating (`0..=5`, `0` = unrated) of the active virtual copy
    /// (LR-01). Persists through [`Self::save_sidecar`] so the value survives
    /// restarts; values `> 5` are rejected loudly, never clamped.
    pub fn set_rating(&mut self, rating: u8) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetRating);
        if rating > 5 {
            return Err(GuiError::Io(Str::InvalidRating.t().to_string()));
        }
        self.ensure_document_loaded()?;
        self.active_copy_mut()?.rating = rating;
        self.save_sidecar();
        self.status = Str::RatingSetPattern.format_arg(&rating.to_string());
        Ok(())
    }

    /// Set the pick flag of the active virtual copy (LR-01). Persists through
    /// [`Self::save_sidecar`] so the value survives restarts.
    pub fn set_flag(&mut self, flag: Flag) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetFlag);
        self.ensure_document_loaded()?;
        self.active_copy_mut()?.flag = flag;
        self.save_sidecar();
        self.status = Str::FlagSetPattern.format_arg(flag_label(flag));
        Ok(())
    }

    /// Current color label (`0..=4`, `0` = none) of the active virtual copy
    /// (Welle 2): read from the copy's `extras["color_label"]` — a plain
    /// cosmetic annotation, no sidecar schema change. Returns `None` when no
    /// document is loaded; read-only accessor for the rating section, the
    /// Library badge and headless tests.
    pub fn color_label(&self) -> Option<u8> {
        self.document.as_ref().and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
                .map(|copy| color_label_of(&copy.extras))
        })
    }

    /// Set the color label (`0..=4`, `0` = none) of the active virtual copy
    /// (Welle 2, keys `6`–`9` select `1`–`4`). Persists through
    /// [`Self::save_sidecar`] so the value survives restarts; values `> 4`
    /// are rejected loudly, never clamped.
    pub fn set_color_label(&mut self, label: u8) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetColorLabel);
        if label > 4 {
            return Err(GuiError::Io(Str::InvalidColorLabel.t().to_string()));
        }
        self.ensure_document_loaded()?;
        self.active_copy_mut()?
            .extras
            .insert("color_label".into(), serde_json::Value::from(label));
        self.save_sidecar();
        self.status = Str::ColorLabelSetPattern.format_arg(color_label_name(label));
        Ok(())
    }

    /// Toggle the clipping-warning overlay badge (`J`, Welle 2). Display-only:
    /// while armed, the preview header shows shadow/highlight clipping
    /// fractions computed from the displayed pixels (see
    /// [`clip_fractions`]). Never mutates the recipe.
    pub fn toggle_clipping_overlay(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleClipping);
        self.clipping_overlay = !self.clipping_overlay;
        info!(
            "GUI interaction: toggle_clipping_overlay -> {}",
            self.clipping_overlay
        );
        self.status = if self.clipping_overlay {
            Str::ClippingOn.t().into()
        } else {
            Str::ClippingOff.t().into()
        };
    }

    /// Clipping fractions of the currently displayed frame for the `J` badge:
    /// the original while Before/After is held, otherwise the last preview.
    /// `None` when no frame is displayed yet.
    pub fn clipping_detail(&self) -> Option<(f64, f64)> {
        if self.before_after {
            self.original.as_ref().map(clip_fractions)
        } else {
            self.preview.as_ref().map(clip_fractions)
        }
    }

    /// Whether the clipping badge paints right now (G-16): the armed `J`
    /// overlay OR the transient `Alt`+tone-slider masking preview. Single
    /// draw-path gate shared by the preview header, so the preview reuses the
    /// `J` rendering exactly. Read-only accessor for headless tests.
    pub fn clipping_effective(&self) -> bool {
        self.clipping_overlay || self.masking_preview.is_some()
    }

    /// Toggle the display-only softproof preview (plain `S`, G-16). Display-
    /// only session state: advertises the badge in the preview header, never
    /// mutates the recipe or the sidecar. This reserves the `S` binding for
    /// LRPAR-G10-VIEWER (full print/gamut simulation is G-10 follow-up).
    pub fn toggle_softproof_preview(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleSoftproof);
        self.softproof_preview = !self.softproof_preview;
        info!(
            "GUI interaction: toggle_softproof_preview -> {}",
            self.softproof_preview
        );
        self.status = if self.softproof_preview {
            Str::SoftproofOn.t().into()
        } else {
            Str::SoftproofOff.t().into()
        };
    }

    /// Whether the softproof preview badge is armed (read-only accessor for
    /// headless tests).
    pub fn softproof_preview(&self) -> bool {
        self.softproof_preview
    }

    /// GUI-INSTRDBG-17: the GPU route of the last painted frame for the debug
    /// action log. `present` when a GPU context is bound and the VRAM present
    /// path was used, `cpu-fallback` when the bound context routed the preview
    /// to the CPU, `n/a` when no context exists (or the `gpu` feature is off).
    /// Diagnostic only; never consulted for routing.
    #[cfg(debug_assertions)]
    pub(crate) fn gpu_route_label(&self) -> &'static str {
        #[cfg(feature = "gpu")]
        {
            if self.gpu.is_some() {
                return if self.gpu_route_fallback.is_some() {
                    GPU_ROUTE_CPU_FALLBACK
                } else {
                    GPU_ROUTE_PRESENT
                };
            }
        }
        GPU_ROUTE_NA
    }

    /// GUI-INSTRDBG-17: starts the debug action timer for `action` (see the
    /// [`GuiActionTimer`] RAII guard). Debug builds only; release expands the
    /// `instrument_gui_action!` call site to nothing.
    #[cfg(debug_assertions)]
    pub(crate) fn begin_gui_action(&self, action: GuiAction) -> GuiActionTimer {
        GuiActionTimer::new(action, self.gpu_route_label())
    }

    /// Toggle the G-10 "Original Photo" histogram compare (histogram-panel
    /// switch). Display-only session state: shows the unedited
    /// Original-Decode measurement instead of the edited render, never
    /// mutates the recipe or the sidecar.
    pub fn toggle_original_histogram(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleOriginalHistogram);
        self.show_original_histogram = !self.show_original_histogram;
        info!(
            "GUI interaction: toggle_original_histogram -> {}",
            self.show_original_histogram
        );
        self.status = if self.show_original_histogram {
            Str::HistogramOriginalOn.t().into()
        } else {
            Str::HistogramOriginalOff.t().into()
        };
    }

    /// Whether the original (unedited-decode) histogram is shown instead of
    /// the edited render histogram (read-only accessor for headless tests).
    pub fn show_original_histogram(&self) -> bool {
        self.show_original_histogram
    }

    /// Tone analysis of the unedited Original-Decode (`self.original`),
    /// measured on the fly through the same `analyze_tone` path the
    /// Before/After view uses — no second analysis path. `None` without a
    /// loaded image (loud `NotCurrent` in the panel, never a silent zero).
    pub fn original_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        self.original.as_ref().map(analyze_tone)
    }

    /// 256-bin luminance histogram of the unedited Original-Decode, measured
    /// on the fly through the same `LuminanceHistogram` path the
    /// Before/After view uses. `None` without a loaded image.
    pub fn original_histogram_data(&self) -> Option<LuminanceHistogram> {
        self.original.as_ref().map(LuminanceHistogram::new)
    }

    /// Original-vs-edited histogram delta from real analysis values:
    /// `(edited_mean - original_mean, normalized_bin_l1)`. The edited side
    /// is the stored full-frame render measurement (`tone_analysis` /
    /// `preview_histogram`, never a viewport/ROI slice); the original side
    /// is the unedited decode. `None` when either side is missing (no image
    /// yet, or no committed render) — never a silent `(0, 0)`.
    pub fn histogram_delta(&self) -> Option<(f64, f64)> {
        let original = self.original.as_ref()?;
        let edited_analysis = self.tone_analysis.as_ref()?;
        let edited_histogram = self.preview_histogram.as_ref()?;
        let original_analysis = analyze_tone(original);
        let original_histogram = LuminanceHistogram::new(original);
        let mean_delta = edited_analysis.mean - original_analysis.mean;
        let l1 = normalized_histogram_l1(&original_histogram.bins, &edited_histogram.bins)?;
        Some((mean_delta, l1))
    }

    /// LRPAR-G01-BASIC: both histogram sides at once — the unedited
    /// Original-Decode histogram plus the edited full-frame render histogram
    /// (`preview_histogram`, never a viewport/ROI slice). Same
    /// `LuminanceHistogram` measurement path as Before/After on both sides;
    /// `None` when either side is missing (no image yet, or no committed
    /// render). Deterministic: two calls over the same state are identical.
    pub fn histogram_compare_data(&self) -> Option<(LuminanceHistogram, LuminanceHistogram)> {
        let original = self.original.as_ref().map(LuminanceHistogram::new)?;
        let edited = self.preview_histogram.clone()?;
        Some((original, edited))
    }

    /// LRPAR-G01-BASIC: set the Develop treatment through the shared sidecar
    /// path (same stash semantics as the `V` toggle and
    /// `lumina develop --treatment`). Only `"color"` and `"bw"` are
    /// accepted — anything else fails loudly. Commits through the normal
    /// save/render path (history, `preview_generation`-bump, `info!`-log).
    /// A no-change call succeeds without saving.
    pub fn set_treatment(&mut self, treatment: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetTreatment);
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if treatment != TREATMENT_COLOR && treatment != TREATMENT_BW {
            return Err(GuiError::Io(Str::InvalidTreatment.t().to_string()));
        }
        let changed = self
            .recipe
            .apply_treatment(treatment)
            .map_err(GuiError::from)?;
        info!("GUI interaction: set_treatment -> {treatment}");
        self.status = Str::TreatmentSetPattern.format_arg(treatment);
        if !changed {
            return Ok(());
        }
        self.mark_recipe_dirty("treatment", f64::from(treatment == TREATMENT_BW));
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// LRPAR-G01-BASIC: set the Develop profile through the shared sidecar
    /// path (same whitelist as `lumina develop --profile`; `"default"`
    /// removes the key). Unknown names fail loudly. MVP renders every known
    /// profile identically (persisted selection intent — see
    /// `feature/architecture/pipeline.md` § G-01). Commits like a slider.
    pub fn set_profile(&mut self, profile: &str) -> Result<(), GuiError> {
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if !DEVELOP_PROFILES.contains(&profile) {
            return Err(GuiError::Io(Str::InvalidProfile.t().to_string()));
        }
        let position = DEVELOP_PROFILES
            .iter()
            .position(|p| *p == profile)
            .unwrap_or(0);
        let changed = self
            .recipe
            .apply_develop_profile(profile)
            .map_err(GuiError::from)?;
        info!("GUI interaction: set_profile -> {profile}");
        self.status = Str::ProfileSetPattern.format_arg(profile);
        if !changed {
            return Ok(());
        }
        self.mark_recipe_dirty("profile", position as f64);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// LRPAR-G01-BASIC: snapshot the current Previous baseline (recipe +
    /// active-copy mask layers). Captured at image load and after every
    /// successful save, so `Previous` always means "last saved state".
    fn capture_section_baselines(&mut self) {
        self.recipe_baseline = Some(self.recipe.clone());
        self.mask_baseline = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| copy.mask_layers.clone())
            .unwrap_or_default();
    }

    /// LRPAR-G01-BASIC: restore one Develop section from the Previous
    /// baseline (panel-local undo to the last saved state — explicitly NOT a
    /// cross-image Previous, which is LRPAR-G08-PREVIOUS). Out-of-range
    /// indices and a missing baseline fail loudly. Commits through the
    /// normal save/render path.
    pub fn restore_section_previous(&mut self, index: usize) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::RestoreSectionPrevious);
        let label = section_name(index)
            .ok_or_else(|| GuiError::Io(format!("unknown develop section {index}")))?;
        let baseline = self
            .recipe_baseline
            .clone()
            .ok_or_else(|| GuiError::Io(Str::SectionPreviousUnavailable.t().to_string()))?;
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if index == SECTION_MASKING {
            let id = self.virtual_copy_id.clone();
            let layers = self.mask_baseline.clone();
            let document = self
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            copy.mask_layers = layers;
        } else {
            Self::restore_recipe_section(&mut self.recipe, &baseline, index);
        }
        info!("GUI interaction: restore_section_previous {label}");
        self.status = Str::SectionPreviousPattern.format_arg(label);
        self.mark_recipe_dirty("section_previous", index as f64);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// LRPAR-G01-BASIC: reset one Develop section to its documented defaults
    /// (Basic tone keys to slider defaults, nested blocks to absent, profile
    /// to default, Masking layers of the active copy removed). Treatment is
    /// exited via the stash path (never stranded at `-1`); a Color reset
    /// leaves B&W-owned `-1` values untouched while the treatment is active
    /// (ownership of the toggle). Commits through the normal save/render path.
    pub fn reset_section(&mut self, index: usize) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::ResetSection);
        let label = section_name(index)
            .ok_or_else(|| GuiError::Io(format!("unknown develop section {index}")))?;
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if index == SECTION_MASKING {
            let id = self.virtual_copy_id.clone();
            let document = self
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            copy.mask_layers.clear();
        } else {
            Self::reset_recipe_section(&mut self.recipe, index).map_err(GuiError::from)?;
        }
        info!("GUI interaction: reset_section {label}");
        self.status = Str::SectionResetPattern.format_arg(label);
        self.mark_recipe_dirty("section_reset", index as f64);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// LRPAR-G01-BASIC: copy one section's recipe fields from `src` to `dst`
    /// (exact restore, absence included). Masking is handled by the caller
    /// (layers live on the virtual copy, not the recipe).
    fn restore_recipe_section(dst: &mut EditRecipe, src: &EditRecipe, index: usize) {
        match index {
            SECTION_BASIC => {
                for key in BASIC_TONE_KEYS {
                    match src.adjustments.get(*key) {
                        Some(value) => {
                            dst.adjustments.insert((*key).to_string(), *value);
                        }
                        None => {
                            dst.adjustments.remove(*key);
                        }
                    }
                }
                // Treatment marker + stash travel together (toggle-owned pair).
                for key in [TREATMENT_KEY, BW_STASH_KEY] {
                    match src.extras.get(key) {
                        Some(value) => {
                            dst.extras.insert(key.to_string(), value.clone());
                        }
                        None => {
                            dst.extras.remove(key);
                        }
                    }
                }
                match src.options.get(DEVELOP_PROFILE_KEY) {
                    Some(value) => {
                        dst.options
                            .insert(DEVELOP_PROFILE_KEY.into(), value.clone());
                    }
                    None => {
                        dst.options.remove(DEVELOP_PROFILE_KEY);
                    }
                }
            }
            SECTION_TONE_CURVE => dst.curves.clone_from(&src.curves),
            SECTION_COLOR => {
                dst.hsl.clone_from(&src.hsl);
                dst.point_color.clone_from(&src.point_color);
                dst.color_grading.clone_from(&src.color_grading);
                dst.presence.clone_from(&src.presence);
                for key in ["vibrance", "saturation"] {
                    match src.adjustments.get(key) {
                        Some(value) => {
                            dst.adjustments.insert(key.to_string(), *value);
                        }
                        None => {
                            dst.adjustments.remove(key);
                        }
                    }
                }
            }
            SECTION_DETAIL => {
                dst.sharpening.clone_from(&src.sharpening);
                dst.noise_reduction.clone_from(&src.noise_reduction);
                dst.red_eye.clone_from(&src.red_eye);
            }
            SECTION_EFFECTS => dst.effects.clone_from(&src.effects),
            SECTION_OPTICS => {
                dst.lens_correction.clone_from(&src.lens_correction);
                dst.lens_blur.clone_from(&src.lens_blur);
            }
            SECTION_GEOMETRY => {
                dst.geometry.clone_from(&src.geometry);
                dst.perspective.clone_from(&src.perspective);
                dst.upright.clone_from(&src.upright);
            }
            _ => {}
        }
    }

    /// LRPAR-G01-BASIC: reset one section's recipe fields to documented
    /// defaults (pure helper; Masking and I/O handled by the caller).
    fn reset_recipe_section(
        recipe: &mut EditRecipe,
        index: usize,
    ) -> Result<(), lumina_sidecar::SidecarError> {
        match index {
            SECTION_BASIC => {
                for key in BASIC_TONE_KEYS {
                    recipe
                        .adjustments
                        .insert((*key).to_string(), Self::default_for_adjustment(key));
                }
                // Exit B&W through the stash path (restores pre-B&W values,
                // never strands `-1`); a no-op when already color.
                recipe.apply_treatment(TREATMENT_COLOR)?;
                recipe.options.remove(DEVELOP_PROFILE_KEY);
            }
            SECTION_TONE_CURVE => recipe.curves = None,
            SECTION_COLOR => {
                recipe.hsl = None;
                recipe.point_color = None;
                recipe.color_grading = None;
                recipe.presence = None;
                // B&W-owned `-1` values stay while the treatment is active
                // (ownership of the toggle); otherwise back to identity.
                if recipe.treatment() != TREATMENT_BW {
                    recipe.adjustments.remove("vibrance");
                    recipe.adjustments.remove("saturation");
                }
            }
            SECTION_DETAIL => {
                recipe.sharpening = None;
                recipe.noise_reduction = None;
                // G-14 red-eye lives in this panel; a section reset clears it
                // too (visible recipe edit, no hidden state).
                recipe.red_eye = None;
            }
            SECTION_EFFECTS => recipe.effects = None,
            SECTION_OPTICS => {
                recipe.lens_correction = None;
                recipe.lens_blur = None;
            }
            SECTION_GEOMETRY => {
                recipe.geometry = None;
                recipe.perspective = None;
                // LRPAR-G06-UPRIGHT-15 lives in this panel.
                recipe.upright = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether "Reset Sliders Automatically" is armed (read-only accessor
    /// for the footer checkbox and headless tests).
    pub fn reset_sliders_automatically(&self) -> bool {
        self.reset_sliders_automatically
    }

    /// Set "Reset Sliders Automatically" (LRPAR-G01-BASIC). Persists
    /// folder-inherited in the current folder's `.lumina/settings.json`
    /// (other folder flags are preserved via an effective-settings
    /// read-modify-write); a persistence failure stays visible via
    /// `show_error` instead of silently diverging from disk.
    pub fn set_reset_sliders_automatically(&mut self, enabled: bool) {
        if self.reset_sliders_automatically == enabled {
            return;
        }
        self.reset_sliders_automatically = enabled;
        info!("GUI interaction: set_reset_sliders_automatically -> {enabled}");
        self.status = if enabled {
            Str::ResetSlidersOn.t().into()
        } else {
            Str::ResetSlidersOff.t().into()
        };
        match self.persist_reset_sliders_flag() {
            Ok(folder) => {
                // R2-MODSWITCH-1 F7: the folder settings changed — drop the
                // memoized preview index for that folder so the new effective
                // options take effect (visible invalidation, never a silent
                // stale gate).
                self.thumbnail_cache.invalidate_folder(&folder);
            }
            Err(error) => self.show_error(error),
        }
    }

    /// Write the current flag into the current folder's settings file,
    /// preserving the other inherited flags (read-modify-write over the
    /// effective settings). Returns the folder written to, so the caller can
    /// invalidate its memoized preview index visibly.
    fn persist_reset_sliders_flag(&self) -> Result<PathBuf, GuiError> {
        let folder = if self.path.trim().is_empty() {
            PathBuf::from(self.directory.trim())
        } else {
            Path::new(self.path.trim())
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(self.directory.trim()))
        };
        let cache = DiskFolderCache::in_folder(&folder)
            .map_err(|error| GuiError::Io(format!("cannot open folder settings: {error}")))?;
        let mut settings = cache
            .effective_settings()
            .map_err(|error| GuiError::Io(format!("cannot read folder settings: {error}")))?;
        settings.reset_sliders_automatically = self.reset_sliders_automatically;
        cache
            .save_settings(&settings)
            .map_err(|error| GuiError::Io(format!("cannot save folder settings: {error}")))?;
        Ok(folder)
    }

    /// Refresh the flag from the given folder's inherited settings (called
    /// on folder navigation). Folders without a settings file keep the
    /// current value; unreadable files warn loudly and keep the current
    /// value instead of silently resetting it.
    fn refresh_reset_sliders_flag(&mut self, folder: &Path) {
        if !folder.join(".lumina").join("settings.json").exists() {
            return;
        }
        match DiskFolderCache::in_folder(folder).map_err(|error| error.to_string()) {
            Ok(cache) => match cache.effective_settings() {
                Ok(settings) => {
                    self.reset_sliders_automatically = settings.reset_sliders_automatically;
                }
                Err(error) => {
                    warn!(
                        "cannot read folder settings for {}: {error}",
                        folder.display()
                    );
                }
            },
            Err(error) => {
                warn!(
                    "cannot open folder settings for {}: {error}",
                    folder.display()
                );
            }
        }
    }

    /// Arm or disarm the transient `Alt`+tone-slider masking preview (G-16).
    /// `Some(key)` shows the clipping badge through [`Self::clipping_effective`]
    /// while `Alt` is held during a tone-slider edit; `None` hides it again.
    /// Display-only session state: never touches the recipe or the sidecar.
    /// Only tone-scope keys arm (see [`masking_preview_for_slider`]) — other
    /// keys are refused loudly instead of arming a nameless preview.
    pub fn set_masking_preview(&mut self, key: Option<&str>) -> Result<(), GuiError> {
        match key {
            Some(key) => {
                if !masking_preview_for_slider(key) {
                    return Err(GuiError::Io(Str::UnknownAdjustment.format_arg(key)));
                }
                let changed = self.masking_preview.as_deref() != Some(key);
                self.masking_preview = Some(key.to_string());
                if changed {
                    info!("GUI interaction: masking_preview -> {key}");
                    self.status = Str::MaskingPreviewPattern.format_arg(key);
                }
                Ok(())
            }
            None => {
                self.masking_preview = None;
                Ok(())
            }
        }
    }

    /// Which tone slider currently arms the masking preview, if any
    /// (read-only accessor for headless tests).
    pub fn masking_preview_key(&self) -> Option<&str> {
        self.masking_preview.as_deref()
    }

    /// Shared `suggest_auto_tone` evaluation over the loaded source frame
    /// (G-16): the single auto-tone path used by both [`Self::auto_tone`]
    /// (all six sliders) and [`Self::apply_auto_endpoint`] (one end point).
    /// Returns the result plus the analysis input fingerprint. Loud without a
    /// loaded image — never a silent no-op.
    fn compute_auto_tone(&self) -> Result<(AutoToneResult, String), GuiError> {
        let Some(frame) = &self.original else {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        };
        let config = AutoToneConfig {
            target_luminance: self.recipe.auto_features.target_luminance,
            ..Default::default()
        };
        let result = suggest_auto_tone(frame, config)?;
        Ok((result, tone_fingerprint(frame, config)))
    }

    /// Apply one auto end point (G-16, `Shift`+double-click on the
    /// `whites`/`blacks` label): evaluates the shared auto-tone path and
    /// persists exactly that field (value + auto mirror + analysis
    /// fingerprint) through the normal save/render commit. Loud without a
    /// loaded image.
    pub fn apply_auto_endpoint(&mut self, endpoint: AutoEndpoint) -> Result<(), GuiError> {
        let (result, input_fingerprint) = self.compute_auto_tone()?;
        let (key, value) = match endpoint {
            AutoEndpoint::White => ("whites", result.whites),
            AutoEndpoint::Black => ("blacks", result.blacks),
        };
        self.recipe.adjustments.insert(key.into(), value);
        self.recipe.auto_features.enable_auto_tone = true;
        match endpoint {
            AutoEndpoint::White => self.recipe.auto_features.auto_whites = Some(value),
            AutoEndpoint::Black => self.recipe.auto_features.auto_blacks = Some(value),
        }
        self.recipe.auto_features.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint,
            extras: BTreeMap::new(),
        });
        info!("GUI interaction: apply_auto_endpoint {key}={value}");
        self.status = Str::AutoEndpointAppliedPattern.format_arg(key);
        // Same commit discipline as `auto_tone` (GUI-AUTOTONE-SAVE-1 /
        // GUI-SIDECAR-READ-1): record + synchronously persist (CAS, loud
        // conflicts) instead of stranding the save on a later edit.
        self.mark_recipe_dirty("auto_endpoint", value);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// Toggle lights-out (`L`, Welle 2). Display-only: hides the side panels
    /// and the filmstrip; header, module bar and preview stay so status and
    /// errors remain visible. Never mutates the recipe.
    pub fn toggle_lights_out(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleLightsOut);
        self.lights_out = !self.lights_out;
        info!("GUI interaction: toggle_lights_out -> {}", self.lights_out);
        self.status = if self.lights_out {
            Str::LightsOutOn.t().into()
        } else {
            Str::LightsOutOff.t().into()
        };
    }

    /// Toggle side-panel visibility (`Tab`, Welle 2). Display-only: hides the
    /// left/right panels; the filmstrip stays (unlike `L` lights-out).
    /// Never mutates the recipe.
    pub fn toggle_panels_hidden(&mut self) {
        instrument_gui_action!(self, GuiAction::TogglePanelsHidden);
        self.panels_hidden = !self.panels_hidden;
        info!(
            "GUI interaction: toggle_panels_hidden -> {}",
            self.panels_hidden
        );
        self.status = if self.panels_hidden {
            Str::PanelsHiddenOn.t().into()
        } else {
            Str::PanelsHiddenOff.t().into()
        };
    }

    /// Toggle all panels (`Shift+Tab`, G-11). Display-only: hides the side
    /// panels, the navigator rail and the filmstrip; header/module bar and
    /// preview stay so status and errors remain visible. Never mutates the
    /// recipe or the sidecar (session-only like `Tab`).
    pub fn toggle_all_panels_hidden(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleAllPanelsHidden);
        self.all_panels_hidden = !self.all_panels_hidden;
        log::info!(
            "GUI interaction: toggle_all_panels_hidden -> {}",
            self.all_panels_hidden
        );
        self.status = if self.all_panels_hidden {
            Str::AllPanelsHiddenOn.t().into()
        } else {
            Str::AllPanelsHiddenOff.t().into()
        };
    }

    /// Whether `Shift+Tab` all-panels-hide is armed (read-only accessor for
    /// headless tests).
    pub fn all_panels_hidden(&self) -> bool {
        self.all_panels_hidden
    }

    /// Whether any side chrome is hidden: plain `Tab` panels-hide, `L`
    /// lights-out, `F` fullscreen or `Shift+Tab` all-panels-hide. Shared by
    /// the side-panel and navigator draw gates; [`Self::chrome_hidden`] keeps
    /// its historical meaning (without the G-11 flag) for compatibility.
    pub fn side_chrome_hidden(&self) -> bool {
        self.chrome_hidden() || self.all_panels_hidden
    }

    /// Current tool-overlay mode (G-11, read-only accessor for headless tests).
    pub fn overlay_mode(&self) -> OverlayMode {
        self.overlay_mode
    }

    /// Set the tool-overlay mode (G-11). Display-only session state: never
    /// touches the recipe or the sidecar.
    pub fn set_overlay_mode(&mut self, mode: OverlayMode) {
        instrument_gui_action!(self, GuiAction::SetOverlayMode);
        if self.overlay_mode == mode {
            return;
        }
        self.overlay_mode = mode;
        log::info!("GUI interaction: set_overlay_mode -> {mode:?}");
        self.status = Str::OverlayModeSetPattern.format_arg(overlay_mode_name(mode));
    }

    /// Whether the mask-matte overlay paints right now (G-11): `Always` shows
    /// whenever a prompt exists, `Never` hides, `Auto` shows only while a
    /// masking/retouch tool is armed or a drag is in progress. Consumed by
    /// [`Self::effective_overlay_prompt`] (the single draw-path gate), so the
    /// default `Always` preserves the historical behaviour exactly.
    pub fn overlay_visible(&self) -> bool {
        match self.overlay_mode {
            OverlayMode::Always => true,
            OverlayMode::Never => false,
            OverlayMode::Auto => {
                self.mask_tool != MaskTool::None || self.spot_tool != SpotTool::None || self.drawing
            }
        }
    }

    /// The overlay prompt gated by [`Self::overlay_visible`] (G-11): `None`
    /// when the current mode hides the overlay, otherwise the live drag or
    /// the selected mask's saved prompt. Single draw-path gate for
    /// `draw_mask_overlay`, headless-testable without pixels.
    fn effective_overlay_prompt(&self) -> Option<MaskPrompt> {
        if !self.overlay_visible() {
            return None;
        }
        self.current_overlay_prompt()
    }

    /// Current edit-pin visibility mode (G-11, read-only accessor).
    pub fn pin_visibility(&self) -> PinVisibility {
        self.pin_visibility
    }

    /// Set the edit-pin visibility (G-11). Display-only session state: never
    /// touches the recipe or the sidecar.
    pub fn set_pin_visibility(&mut self, visibility: PinVisibility) {
        instrument_gui_action!(self, GuiAction::SetPinVisibility);
        if self.pin_visibility == visibility {
            return;
        }
        self.pin_visibility = visibility;
        log::info!("GUI interaction: set_pin_visibility -> {visibility:?}");
        self.status = Str::PinVisibilitySetPattern.format_arg(pin_visibility_name(visibility));
    }

    /// Whether edit pins paint right now (G-11): `Always` shows, `Never`
    /// hides, `Auto` shows only while a masking/retouch tool is armed.
    pub fn pins_visible(&self) -> bool {
        match self.pin_visibility {
            PinVisibility::Always => true,
            PinVisibility::Never => false,
            PinVisibility::Auto => {
                self.mask_tool != MaskTool::None || self.spot_tool != SpotTool::None
            }
        }
    }

    /// The edit pins to paint (G-11): one pin per mask of the active copy with
    /// a derivable anchor ([`pin_anchor_for_prompt`]) plus one pin per stored
    /// spot heal with finite `0..=1` centre coordinates. Empty unless
    /// [`Self::pins_visible`]. Pins are Painter-content (invisible to
    /// AccessKit per HARNESS-2), so this getter is the testable model state;
    /// the painter loop paints exactly this list in order (labels `1..=n`).
    pub fn visible_edit_pins(&self) -> Vec<EditPin> {
        if !self.pins_visible() {
            return Vec::new();
        }
        let mut pins = Vec::new();
        let document = match self.document.as_ref() {
            Some(document) => document,
            None => return pins,
        };
        let copy = match document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
        {
            Some(copy) => copy,
            None => return pins,
        };
        for mask in &copy.mask_library {
            let Some(prompt) = mask.prompt.as_ref() else {
                continue;
            };
            let Some((x, y)) = pin_anchor_for_prompt(prompt) else {
                continue;
            };
            pins.push(EditPin {
                id: format!("mask:{}", mask.id),
                label: (pins.len() + 1).to_string(),
                pos: (x, y),
                selected: self.selected_mask_id.as_deref() == Some(mask.id.as_str()),
                kind: EditPinKind::Mask,
            });
        }
        let spots: Vec<serde_json::Value> = self
            .recipe
            .extras
            .get("spot_removals")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        for spot in &spots {
            let centre = spot
                .get("center_x")
                .and_then(serde_json::Value::as_f64)
                .zip(spot.get("center_y").and_then(serde_json::Value::as_f64));
            let Some((x, y)) = centre else {
                continue;
            };
            if !x.is_finite()
                || !y.is_finite()
                || !(0.0..=1.0).contains(&x)
                || !(0.0..=1.0).contains(&y)
            {
                continue;
            }
            let id = spot
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            pins.push(EditPin {
                id: format!("spot:{id}"),
                label: (pins.len() + 1).to_string(),
                pos: (x as f32, y as f32),
                selected: false,
                kind: EditPinKind::Spot,
            });
        }
        pins
    }

    /// Whether solo mode is armed (G-11, read-only accessor).
    pub fn solo_mode(&self) -> bool {
        self.solo_mode
    }

    /// Set solo mode (G-11). Display-only session state. Enabling with several
    /// open sections deterministically keeps the first (lowest index) and
    /// closes the rest.
    pub fn set_solo_mode(&mut self, enabled: bool) {
        instrument_gui_action!(self, GuiAction::SetSoloMode);
        if self.solo_mode == enabled {
            return;
        }
        self.solo_mode = enabled;
        if enabled {
            if let Some(first) = (0..SECTION_COUNT).find(|&i| self.section_open[i]) {
                for i in 0..SECTION_COUNT {
                    self.section_open[i] = i == first;
                }
            }
        }
        log::info!("GUI interaction: set_solo_mode -> {enabled}");
        self.status = if enabled {
            Str::SoloModeOn.t().into()
        } else {
            Str::SoloModeOff.t().into()
        };
    }

    /// Whether Develop section `index` is open (G-11, read-only accessor).
    /// Out-of-range indices read as closed.
    pub fn is_section_open(&self, index: usize) -> bool {
        self.section_open.get(index).copied().unwrap_or(false)
    }

    /// Set a Develop section open state (G-11). With solo mode on, opening one
    /// section closes the other seven. Out-of-range indices are refused loudly
    /// (warn, no state change). Display-only session state: never touches the
    /// recipe or the sidecar.
    pub fn set_section_open(&mut self, index: usize, open: bool) {
        if index >= SECTION_COUNT {
            log::warn!("GUI interaction: set_section_open refused for index {index}");
            return;
        }
        if open && self.solo_mode {
            for i in 0..SECTION_COUNT {
                self.section_open[i] = false;
            }
        }
        if self.section_open[index] != open {
            self.section_open[index] = open;
            log::info!("GUI interaction: set_section_open {index} -> {open}");
        }
    }

    /// Toggle the crop-mode badge (`R`, Welle 2). Display-only: while armed,
    /// the preview header advertises the mode; edits stay in the Geometry
    /// Crop controls. Never mutates the recipe.
    pub fn toggle_crop_mode(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleCropMode);
        self.crop_mode = !self.crop_mode;
        // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): arming the geometry tool
        // disarms the source-coordinate tools (mask / WB / red-eye / spot)
        // instead of leaving them armed-but-refused. The reverse direction
        // (geometry -> mask/WB) commits the crop draft first, see
        // `commit_outgoing_tool_for_switch`.
        if self.crop_mode {
            self.set_mask_tool(MaskTool::None);
            self.cancel_armed_preview_tools();
        }
        // UX-LOOK-CROP-18b: arming/leaving crop mode changes the preview
        // texture (full frame vs. committed crop) — invalidate.
        self.mark_dirty();
        info!("GUI interaction: toggle_crop_mode -> {}", self.crop_mode);
        self.set_crop_mode_status();
    }

    /// Toggle the Library filter drawer (`\`, Welle 3, LR-13 light).
    /// Display-only: shows/hides the text filter + Quick Develop sliders in
    /// the Library grid. Never mutates the recipe.
    pub fn toggle_filter_bar(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleFilterBar);
        self.filter_bar_visible = !self.filter_bar_visible;
        info!(
            "GUI interaction: toggle_filter_bar -> {}",
            self.filter_bar_visible
        );
        self.status = if self.filter_bar_visible {
            Str::FilterShown.t().into()
        } else {
            Str::FilterHidden.t().into()
        };
    }

    /// Set the Library text filter query (Welle 3, LR-13 light). Display-only;
    /// matched by [`library_filter_matches`] against the scanned entry
    /// metadata. Never mutates the recipe.
    pub fn set_library_filter(&mut self, query: impl Into<String>) {
        self.library_filter = query.into();
        trace!(
            "GUI interaction: set_library_filter {:?}",
            self.library_filter
        );
    }

    /// Active compare/survey proxy mode (Welle 3, LR-20 light). Read-only
    /// accessor for badges and headless tests.
    pub fn compare_mode(&self) -> Option<CompareMode> {
        self.compare_mode
    }

    /// Toggle a compare/survey view (Welle 3, LR-20 light). `Compare` (`C`)
    /// reuses the existing Before/After path (full-frame Before proxy, never
    /// a recipe mutation); `Survey` (`N`) jumps to the Library grid (survey
    /// proxy over the file-browser entries) and clears Before/After. A repeat
    /// press leaves the view. Never mutates the recipe.
    pub fn toggle_compare_mode(&mut self, mode: CompareMode) {
        instrument_gui_action!(self, GuiAction::ToggleCompareMode);
        trace!("GUI interaction: toggle_compare_mode {:?}", mode);
        match mode {
            CompareMode::Compare => {
                if self.compare_mode == Some(CompareMode::Compare) && self.before_after {
                    self.compare_mode = None;
                    self.before_after = false;
                    self.status = Str::CompareOff.t().into();
                } else {
                    self.compare_mode = Some(CompareMode::Compare);
                    self.before_after = true;
                    self.status = Str::CompareOnPattern.format_arg(Str::CompareModeCompare.t());
                }
            }
            CompareMode::Survey => {
                if self.compare_mode == Some(CompareMode::Survey) {
                    self.compare_mode = None;
                    self.library_view = LibraryView::Grid;
                    self.status = Str::CompareOff.t().into();
                } else {
                    self.compare_mode = Some(CompareMode::Survey);
                    self.before_after = false;
                    self.set_module(Module::Library);
                    self.library_view = LibraryView::Survey;
                    self.status = Str::SurveyOn.t().into();
                }
            }
        }
    }

    /// Active Library view (G-09, LRPAR-G09-LIB). Read-only accessor for
    /// badges and headless tests.
    pub fn library_view(&self) -> LibraryView {
        self.library_view
    }

    /// Set the Library view deterministically (G-09). Display-only like
    /// [`Self::toggle_compare_mode`]: `Compare` holds the Before image via
    /// the existing `before_after` path, `Survey`/`Grid`/`Loupe` clear it;
    /// `Survey` and `Grid` and `Loupe` all live in the Library module, so
    /// they switch `active_module` there. Never mutates the recipe.
    pub fn set_library_view(&mut self, view: LibraryView) {
        instrument_gui_action!(self, GuiAction::SetLibraryView);
        trace!("GUI interaction: set_library_view {:?}", view);
        self.library_view = view;
        match view {
            LibraryView::Grid | LibraryView::Loupe | LibraryView::Survey => {
                self.set_module(Module::Library);
                self.before_after = false;
                self.compare_mode = match view {
                    LibraryView::Survey => Some(CompareMode::Survey),
                    _ => None,
                };
                self.status = match view {
                    LibraryView::Survey => Str::SurveyOn.t().into(),
                    LibraryView::Loupe => Str::LoupeOn.t().into(),
                    _ => Str::LibraryGridOn.t().into(),
                };
            }
            LibraryView::Compare => {
                self.set_module(Module::Library);
                self.compare_mode = Some(CompareMode::Compare);
                self.before_after = true;
                self.status = Str::CompareOnPattern.format_arg(Str::CompareModeCompare.t());
            }
            LibraryView::People => {
                self.set_module(Module::Library);
                self.before_after = false;
                self.compare_mode = None;
                self.status = Str::FacePeople.t().into();
            }
        }
    }

    /// Toggle the split Before/After marker (`Shift+Y`, Welle 3, LR-09
    /// light). Display-only: enabling also holds the Before image via the
    /// existing `before_after` path (full-frame Before proxy — a true
    /// side-by-side split render is documented follow-up work, see
    /// `feature/platform/cli-gui-wasm.md`). Never mutates the recipe.
    pub fn toggle_split_view(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleSplitView);
        self.before_after_split = !self.before_after_split;
        if self.before_after_split {
            self.before_after = true;
        }
        info!(
            "GUI interaction: toggle_split_view -> {}",
            self.before_after_split
        );
        self.status = if self.before_after_split {
            Str::SplitViewOn.t().into()
        } else {
            Str::SplitViewOff.t().into()
        };
    }

    /// Toggle the fullscreen preview (`F`, Welle 3). Display-only: hides the
    /// same chrome as lights-out (see [`Self::chrome_hidden`]) and settles
    /// the zoom on Fit when enabling, so the previous `F`-zoom-to-fit
    /// behaviour is preserved on entry. Never mutates the recipe.
    pub fn toggle_fullscreen(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleFullscreen);
        self.fullscreen = !self.fullscreen;
        info!("GUI interaction: toggle_fullscreen -> {}", self.fullscreen);
        if self.fullscreen {
            self.set_zoom_mode(ZoomMode::Fit);
        }
        self.status = if self.fullscreen {
            Str::FullscreenOn.t().into()
        } else {
            Str::FullscreenOff.t().into()
        };
    }

    /// Set the fullscreen preview deterministically (F-100 Startverhalten,
    /// `--fullscreen` CLI flag). Display-only like [`Self::toggle_fullscreen`]:
    /// hides the same chrome as lights-out (see [`Self::chrome_hidden`]) and
    /// settles the zoom on Fit when enabling. Never mutates the recipe.
    /// No-op when already in the requested state (so a default `false` at
    /// startup leaves the status line untouched).
    pub fn set_fullscreen(&mut self, enabled: bool) {
        if self.fullscreen == enabled {
            return;
        }
        trace!("GUI interaction: set_fullscreen -> {enabled}");
        self.fullscreen = enabled;
        if enabled {
            self.set_zoom_mode(ZoomMode::Fit);
        }
        self.status = if enabled {
            Str::FullscreenOn.t().into()
        } else {
            Str::FullscreenOff.t().into()
        };
    }

    /// Whether the fullscreen working view is armed (read-only accessor for
    /// the `main()` startup wiring and headless tests; mirrors
    /// [`Self::set_fullscreen`]).
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Whether side chrome (panels, navigator, filmstrip) is hidden: `Tab`
    /// panels-hide, `L` lights-out or `F` fullscreen (Welle 3). Shared by the
    /// draw paths so fullscreen hides exactly the lights-out chrome — and,
    /// with `fullscreen == false`, every condition evaluates exactly as
    /// before (no default-layout pixel change).
    pub fn chrome_hidden(&self) -> bool {
        self.panels_hidden || self.lights_out || self.fullscreen
    }

    /// Whether the bottom filmstrip is drawn for the current module state
    /// (F-100: visible in Library, Develop AND Export; `Tab` panels-hide
    /// keeps it, `L` lights-out and `F` fullscreen hide it). Single source
    /// of truth shared by the draw path and the headless regression tests
    /// (GUI-VISION-1: Export deliberately shows the filmstrip — there is no
    /// Export-specific reason to hide it, the old
    /// `Library | Develop`-only gate was an oversight against the F-100 norm
    /// "Der Filmstreifen ist in allen drei Modulen sichtbar").
    pub fn shows_filmstrip(&self) -> bool {
        matches!(
            self.active_module,
            Module::Library | Module::Develop | Module::Export
        ) && !self.lights_out
            && !self.fullscreen
            && !self.all_panels_hidden
    }

    /// Current stack-group proxy id of the active virtual copy (Welle 3,
    /// LR-17 light), read tolerantly via [`stack_id_of`]. Returns `None`
    /// when no document is loaded. Read-only accessor for headless tests.
    pub fn stack_group_id(&self) -> Option<String> {
        self.document.as_ref().and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
                .and_then(|copy| stack_id_of(&copy.extras))
        })
    }

    /// Toggle stack-group membership of the active virtual copy (`Cmd/Ctrl+G`,
    /// Welle 3, LR-17 light). Grouping proxy without a schema change: the
    /// first press mints a `stack-<n>` id (unique across the loaded
    /// document's copies) into the copy's `extras["stack_group"]`, the second
    /// press removes it again. Persists through [`Self::save_sidecar`].
    pub fn toggle_stack_group(&mut self) -> Result<Option<String>, GuiError> {
        instrument_gui_action!(self, GuiAction::ToggleStackGroup);
        self.ensure_document_loaded()?;
        if stack_id_of(&self.active_copy_mut()?.extras).is_some() {
            self.active_copy_mut()?.extras.remove("stack_group");
            self.save_sidecar();
            self.status = Str::StackUngrouped.t().into();
            info!("GUI interaction: toggle_stack_group -> ungrouped");
            return Ok(None);
        }
        let mut counter = 0usize;
        for copy in &self
            .document
            .as_ref()
            .expect("document was ensured")
            .virtual_copies
        {
            if let Some(id) = stack_id_of(&copy.extras) {
                if let Some(n) = id
                    .strip_prefix("stack-")
                    .and_then(|rest| rest.parse::<usize>().ok())
                {
                    counter = counter.max(n);
                }
            }
        }
        let new_id = loop {
            counter += 1;
            let candidate = format!("stack-{counter}");
            let taken = self
                .document
                .as_ref()
                .expect("document was ensured")
                .virtual_copies
                .iter()
                .any(|copy| stack_id_of(&copy.extras).as_deref() == Some(&candidate));
            if !taken {
                break candidate;
            }
        };
        self.active_copy_mut()?
            .extras
            .insert("stack_group".into(), Value::String(new_id.clone()));
        self.save_sidecar();
        self.status = Str::StackGroupedPattern.format_arg(&new_id);
        info!("GUI interaction: toggle_stack_group -> {new_id}");
        Ok(Some(new_id))
    }

    /// Source-level keywords of the loaded document (G-15 META-MVP, Slice 3).
    /// Empty without a loaded document. Read-only accessor for the Library
    /// panel and headless tests.
    pub fn keywords(&self) -> Vec<String> {
        self.document
            .as_ref()
            .map(|document| document.keywords.clone())
            .unwrap_or_default()
    }

    /// Add a keyword to the loaded image (G-15 META-MVP, Slice 3) via the
    /// Slice-1 `BatchOp::AddKeyword` language + [`Self::save_sidecar`]
    /// (CAS, atomar). Idempotent: an existing keyword succeeds unchanged.
    /// Invalid keywords fail loudly, never silently normalised.
    pub fn add_keyword(&mut self, keyword: &str) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::AddKeyword);
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::AddKeyword {
                keyword: keyword.to_string(),
            },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("keyword `{keyword}` added to {}", self.path.trim());
            self.status = Str::KeywordAddedPattern.format_arg(keyword);
        } else {
            info!(
                "keyword `{keyword}` already present on {}",
                self.path.trim()
            );
            self.status = Str::KeywordUnchangedPattern.format_arg(keyword);
        }
        Ok(changed)
    }

    /// Remove a keyword from the loaded image (G-15 META-MVP, Slice 3),
    /// mirroring [`Self::add_keyword`]. Absent keywords succeed unchanged.
    pub fn remove_keyword(&mut self, keyword: &str) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::RemoveKeyword);
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::RemoveKeyword {
                keyword: keyword.to_string(),
            },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("keyword `{keyword}` removed from {}", self.path.trim());
            self.status = Str::KeywordRemovedPattern.format_arg(keyword);
        } else {
            info!("keyword `{keyword}` absent on {}", self.path.trim());
            self.status = Str::KeywordUnchangedPattern.format_arg(keyword);
        }
        Ok(changed)
    }

    /// Source-level static collection memberships of the loaded document
    /// (G-15 META-MVP, Slice 3). Empty without a loaded document.
    pub fn collections(&self) -> Vec<CollectionMembership> {
        self.document
            .as_ref()
            .map(|document| document.collections.clone())
            .unwrap_or_default()
    }

    /// Split a collection assignment `id=name` at the first `=` (CLI-compat,
    /// G-15 META-MVP Slice 2 `split_collection_assignment`). A missing `=`
    /// is a loud error, never a silent `id == name`. Pure helper shared by
    /// the panel and headless tests.
    pub fn split_collection_assignment(value: &str) -> Result<(String, String), GuiError> {
        value.split_once('=').map_or_else(
            || {
                Err(GuiError::Io(
                    Str::InvalidCollectionAssignment.format_arg(value),
                ))
            },
            |(id, name)| Ok((id.to_string(), name.to_string())),
        )
    }

    /// Add the loaded image to a static collection (G-15 META-MVP, Slice 3)
    /// via `BatchOp::AddToCollection` + [`Self::save_sidecar`]. An existing
    /// `id` refreshes the display `name` (rename, like the CLI). Loud
    /// validation, `info!` logging, entry refresh — like [`Self::add_keyword`].
    pub fn add_to_collection(&mut self, id: &str, name: &str) -> Result<bool, GuiError> {
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::AddToCollection {
                id: id.to_string(),
                name: name.to_string(),
            },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("collection `{id}` ({name}) joined by {}", self.path.trim());
            self.status = Str::CollectionJoinedPattern.format_arg(name);
        } else {
            info!("collection `{id}` unchanged for {}", self.path.trim());
            self.status = Str::CollectionUnchangedPattern.format_arg(name);
        }
        Ok(changed)
    }

    /// Remove the loaded image from a static collection (G-15 META-MVP,
    /// Slice 3), mirroring [`Self::add_to_collection`].
    pub fn remove_from_collection(&mut self, id: &str) -> Result<bool, GuiError> {
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::RemoveFromCollection { id: id.to_string() },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("collection `{id}` left by {}", self.path.trim());
            self.status = Str::CollectionLeftPattern.format_arg(id);
        } else {
            info!("collection `{id}` absent on {}", self.path.trim());
            self.status = Str::CollectionUnchangedPattern.format_arg(id);
        }
        Ok(changed)
    }

    /// Static collections aggregated from the scanned entries (G-15 META-MVP,
    /// Slice 3): `(id, name, member_count)`, sorted by name. Sidecar-first —
    /// rebuilt from the scan on every call, never a second store. A
    /// divergent `id → name` (renamed in some sidecars only) keeps the
    /// first-seen name; renaming across all sidecars is a batch operation
    /// (see [`Self::apply_metadata_batch`]).
    pub fn static_collections(&self) -> Vec<(String, String, usize)> {
        let mut aggregated: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for entry in &self.entries {
            for membership in &entry.collections {
                aggregated
                    .entry(membership.id.clone())
                    .and_modify(|(_, count)| *count += 1)
                    .or_insert_with(|| (membership.name.clone(), 1));
            }
        }
        let mut out: Vec<(String, String, usize)> = aggregated
            .into_iter()
            .map(|(id, (name, count))| (id, name, count))
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    /// Select the Library collection filter (G-15 META-MVP, Slice 3):
    /// display-only, never persisted. `None` shows all images.
    pub fn set_active_collection(&mut self, filter: Option<CollectionFilter>) {
        trace!("GUI interaction: set_active_collection {filter:?}");
        self.active_collection = filter;
    }

    /// Apply one [`BatchOp`] to every image of the filmstrip selection
    /// (G-15 META-MVP, Slice 3 — Stapel-Vollfunktion). An empty selection
    /// falls back to the loaded image so the action is never a silent
    /// no-op. Per file: `load_sidecar` → `apply_batch_op` → CAS+rebase save
    /// (`validate` runs inside the write). `SetRating`/`SetFlag` with an empty `copy_id`
    /// (see [`parse_metadata_batch_op`]) resolve against the target's
    /// default copy. Failures are loud per image (`error!` + report entry)
    /// and never abort the rest — same pattern as
    /// [`Self::sync_settings_to_selection`]. Recipes, masks and history are
    /// never touched; the original bytes are never read, let alone written.
    pub fn apply_metadata_batch(&mut self, op: &BatchOp) -> SelectionSyncReport {
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() && !self.path.trim().is_empty() {
            targets.push(self.path.trim().to_string());
        }
        let mut report = SelectionSyncReport::default();
        if targets.is_empty() {
            self.status = Str::NoImagesSelected.t().into();
            return report;
        }
        for target in &targets {
            match Self::apply_metadata_op_to_path(Path::new(target), op) {
                Ok(true) => {
                    info!("batch-meta: `{target}` updated");
                    self.refresh_entry(Path::new(target));
                    // Keep the loaded document in sync when the batch touched
                    // the open image (otherwise the panel shows stale data).
                    if self.path.trim() == target.as_str() {
                        self.sidecar_revision = None;
                        self.reload_document_for_batch_target(Path::new(target));
                    }
                    report.applied.push(target.clone());
                }
                Ok(false) => {
                    info!("batch-meta: `{target}` unchanged");
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("batch-meta failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = Str::BatchAppliedPattern.format_arg(&report.applied.len().to_string());
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(format!(
                "Batch failed for {} image(s): {joined}",
                report.failed.len()
            ));
        }
        report
    }

    /// One atomic batch step for `path` (G-15 META-MVP, Slice 3); returns
    /// `Ok(changed)`. Missing sidecar/invented identity stays a loud error.
    fn apply_metadata_op_to_path(path: &Path, op: &BatchOp) -> Result<bool, String> {
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        let mut document = lumina_sidecar::load_sidecar(&sidecar_path)
            .map_err(|error| format!("{}: {error}", sidecar_path.display()))?;
        // Resolve selector ops (empty `copy_id` from the batch bar) against
        // the target's default copy — loudly when the document is empty.
        let resolved = match op {
            BatchOp::SetRating { copy_id, rating } if copy_id.is_empty() => {
                let id = default_copy_id(&document).ok_or_else(|| {
                    format!("{}: no virtual copy for set_rating", sidecar_path.display())
                })?;
                BatchOp::SetRating {
                    copy_id: id,
                    rating: *rating,
                }
            }
            BatchOp::SetFlag { copy_id, flag } if copy_id.is_empty() => {
                let id = default_copy_id(&document).ok_or_else(|| {
                    format!("{}: no virtual copy for set_flag", sidecar_path.display())
                })?;
                BatchOp::SetFlag {
                    copy_id: id,
                    flag: *flag,
                }
            }
            _ => op.clone(),
        };
        let base = document.clone();
        let changed =
            apply_batch_op(&mut document, &resolved).map_err(|error| error.to_string())?;
        if changed {
            let expected =
                lumina_sidecar::document_revision(&base).map_err(|error| error.to_string())?;
            sidecar_rebase::save_rebased_unit(&sidecar_path, &base, &document, Some(&expected))
                .map_err(|error| error.to_string())?;
        }
        Ok(changed)
    }

    /// Re-read the loaded document after a batch touched the open image, so
    /// the panel never shows stale keywords/collections/rating. The session
    /// recipe follows the reloaded active copy; failures stay visible via
    /// `show_error` (the in-memory lineage is kept).
    fn reload_document_for_batch_target(&mut self, path: &Path) {
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        match lumina_sidecar::load_sidecar(&sidecar_path) {
            Ok(document) => {
                if let Some(copy) = document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                {
                    self.recipe = copy.recipe.clone();
                }
                // REVIEW-GUI-N1: re-anchor the CAS revision to the reloaded
                // lineage so the next `save_sidecar` compares against the
                // batch-written file instead of conflicting with it.
                self.sidecar_revision = lumina_sidecar::document_revision(&document).ok();
                self.document = Some(document);
            }
            Err(error) => self.show_error(error),
        }
    }

    /// Load a portable smart-collection catalog (G-15 META-MVP, Slice 3) in
    /// the CLI-identical format
    /// (`{"format":"lumina-smart-catalog","version":1,"collections":[...]}`).
    /// Every deviation (unreadable file, invalid JSON, wrong format/version,
    /// invalid definition) is a loud error — never a silent empty catalog.
    pub fn load_smart_catalog(&mut self, path: &str) -> Result<usize, GuiError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            GuiError::Io(format!("cannot read smart catalog `{path}`: {error}"))
        })?;
        let catalog: SmartCatalogFile = serde_json::from_str(&text)
            .map_err(|error| GuiError::Io(format!("invalid smart catalog `{path}`: {error}")))?;
        if catalog.format != SMART_CATALOG_FORMAT {
            return Err(GuiError::Io(
                Str::InvalidSmartCatalogFormat.format_arg(&catalog.format),
            ));
        }
        if catalog.version != SMART_COLLECTION_VERSION {
            return Err(GuiError::Io(
                Str::InvalidSmartCatalogVersion.format_arg(&catalog.version.to_string()),
            ));
        }
        for def in &catalog.collections {
            validate_smart_collection_def(def).map_err(GuiError::from)?;
        }
        let count = catalog.collections.len();
        self.smart_catalog = catalog.collections;
        self.smart_catalog_path = path.to_string();
        info!("smart catalog `{path}` loaded ({count} collection(s))");
        self.status = Str::SmartCatalogLoadedPattern.format_arg(&count.to_string());
        Ok(count)
    }

    /// Save the session smart catalog to `path` (explicit user action only,
    /// G-15 META-MVP Slice 3). The file carries versioned rule data only —
    /// never absolute paths. Atomic write via `write_atomically`.
    pub fn save_smart_catalog(&mut self, path: &str) -> Result<(), GuiError> {
        for def in &self.smart_catalog {
            validate_smart_collection_def(def).map_err(GuiError::from)?;
        }
        let catalog = SmartCatalogFile {
            format: SMART_CATALOG_FORMAT.to_string(),
            version: SMART_COLLECTION_VERSION,
            collections: self.smart_catalog.clone(),
        };
        let text = serde_json::to_string_pretty(&catalog)
            .map_err(|error| GuiError::Io(format!("cannot encode smart catalog: {error}")))?;
        lumina_sidecar::write_atomically(Path::new(path), text.as_bytes())?;
        self.smart_catalog_path = path.to_string();
        info!(
            "smart catalog `{path}` saved ({} collection(s))",
            self.smart_catalog.len()
        );
        self.status =
            Str::SmartCatalogSavedPattern.format_arg(&self.smart_catalog.len().to_string());
        Ok(())
    }

    /// Create a smart collection from the caller-held rule stack (G-15
    /// META-MVP, Slice 3). Requires exactly one combined rule on the stack
    /// (see [`combine_smart_rules`]); `id`/`name` validate like static
    /// collections (non-empty, no surrounding whitespace, `id` without
    /// `/`, `\`, `:`). Duplicate `id`s and invalid rules fail loudly.
    pub fn create_smart_collection(&mut self, id: &str, name: &str) -> Result<(), GuiError> {
        if self.smart_rule_stack.len() != 1 {
            return Err(GuiError::Io(Str::SmartNeedsSingleRule.t().to_string()));
        }
        if self.smart_catalog.iter().any(|def| def.id == id) {
            return Err(GuiError::Io(Str::SmartDuplicateId.format_arg(id)));
        }
        let def = SmartCollectionDef {
            version: SMART_COLLECTION_VERSION,
            id: id.to_string(),
            name: name.to_string(),
            rule: self
                .smart_rule_stack
                .pop()
                .expect("stack length was checked"),
        };
        validate_smart_collection_def(&def).map_err(GuiError::from)?;
        self.smart_catalog.push(def);
        info!("smart collection `{id}` created");
        self.status = Str::SmartCreatedPattern.format_arg(id);
        Ok(())
    }

    /// Delete a smart collection by `id` (G-15 META-MVP, Slice 3). Unknown
    /// ids fail loudly; an active filter on the deleted id is cleared so the
    /// grid never filters on a ghost.
    pub fn delete_smart_collection(&mut self, id: &str) -> Result<(), GuiError> {
        let before = self.smart_catalog.len();
        self.smart_catalog.retain(|def| def.id != id);
        if self.smart_catalog.len() == before {
            return Err(GuiError::Io(Str::SmartUnknownId.format_arg(id)));
        }
        if self.active_collection == Some(CollectionFilter::Smart { id: id.to_string() }) {
            self.active_collection = None;
        }
        info!("smart collection `{id}` deleted");
        self.status = Str::SmartDeletedPattern.format_arg(id);
        Ok(())
    }

    /// Push one [`build_smart_rule`] rule onto the composition stack (G-15
    /// META-MVP, Slice 3). Loud on unknown kind / bad value.
    pub fn push_smart_rule(&mut self, kind: &str, value: &str) -> Result<(), GuiError> {
        let rule = build_smart_rule(kind, value)
            .map_err(|message| GuiError::Io(format!("invalid smart rule: {message}")))?;
        self.smart_rule_stack.push(rule);
        trace!("GUI interaction: push_smart_rule {kind}");
        Ok(())
    }

    /// Combine the rule stack with `and`/`or`/`not` (see
    /// [`combine_smart_rules`]). Loud on underflow / unknown op.
    pub fn combine_smart_stack(&mut self, op: &str) -> Result<(), GuiError> {
        combine_smart_rules(&mut self.smart_rule_stack, op)
            .map_err(|message| GuiError::Io(format!("invalid smart-rule combine: {message}")))?;
        trace!("GUI interaction: combine_smart_stack {op}");
        Ok(())
    }

    // ---- LRPAR-G15-IPTC-S8: Library Metadata panel -----------------------
    //
    // Every mutation below travels the same sidecar path as the CLI (`meta
    // draft`, `meta preset apply`, `meta sync`): load → mutate a clone with
    // the S1 helpers (`apply_metadata_draft`, history handling) → validate →
    // CAS + atomic rebase (`save_rebased`): concurrent changes merge
    // field-selectively; a persistent conflict is a loud error, never silent
    // last-write-wins. `keywords` in preset `fields` stays loudly rejected
    // (S4 semantics); an empty draft value removes the field (S1 semantics).
    // No second metadata logic lives in the GUI.

    /// Origin marker for history entries written through this panel.
    const META_ORIGIN_GUI: &'static str = "gui";

    /// Current draft values of the loaded document (empty without one).
    /// Read-only accessor for the panel and headless tests.
    pub fn metadata_draft(&self) -> BTreeMap<String, String> {
        self.document
            .as_ref()
            .map(|document| document.metadata.draft.clone())
            .unwrap_or_default()
    }

    /// Current metadata history of the loaded document, newest first (empty
    /// without one). Read-only accessor for the panel and headless tests.
    pub fn metadata_history(&self) -> Vec<MetadataHistoryEntry> {
        self.document
            .as_ref()
            .map(|document| document.metadata.history.clone())
            .unwrap_or_default()
    }

    /// Re-sync the draft text buffers from the loaded document when the
    /// lineage changed (other image, or a new `rev` after commit/batch).
    /// Buffers the user edited since the last sync (`meta_buffers_dirty`)
    /// keep their keystrokes — only missing fields are filled — so the
    /// commit (which creates the document first) never wipes the edit it
    /// is about to save. Call [`Self::resync_meta_buffers`] after
    /// operations that intentionally replace the document content.
    fn ensure_meta_buffers(&mut self) {
        let key = self
            .document
            .as_ref()
            .map(|document| (self.path.trim().to_string(), document.metadata.latest_rev()));
        if self.meta_buffers_key == key {
            return;
        }
        if self.meta_buffers_dirty {
            let draft = self.metadata_draft();
            for id in METADATA_FIELD_IDS {
                self.meta_buffers
                    .entry((*id).to_string())
                    .or_insert_with(|| draft.get(*id).cloned().unwrap_or_default());
            }
            self.meta_buffers_key = key;
            return;
        }
        self.resync_meta_buffers();
    }

    /// Rebuild every draft buffer from the loaded document, discarding
    /// unsaved keystrokes. Used after operations that replace the document
    /// content (clear, preset apply, batch/sync reload of the open image).
    fn resync_meta_buffers(&mut self) {
        let draft = self.metadata_draft();
        self.meta_buffers.clear();
        for id in METADATA_FIELD_IDS {
            self.meta_buffers.insert(
                (*id).to_string(),
                draft.get(*id).cloned().unwrap_or_default(),
            );
        }
        self.meta_buffers_key = self
            .document
            .as_ref()
            .map(|document| (self.path.trim().to_string(), document.metadata.latest_rev()));
        self.meta_buffers_dirty = false;
    }

    /// Set one draft text buffer (panel input + headless tests). Unknown
    /// IDs — including `keywords`, which stays the document `keywords`
    /// field — fail loudly before anything is stored.
    pub fn set_metadata_buffer(&mut self, field: &str, value: String) -> Result<(), GuiError> {
        validate_metadata_field_value(field, "").map_err(GuiError::from)?;
        self.ensure_meta_buffers();
        self.meta_buffers.insert(field.to_string(), value);
        self.meta_buffers_dirty = true;
        Ok(())
    }

    /// Commit the draft buffers of the loaded image (diffed against the
    /// document: changed values are set, emptied buffers remove their
    /// field). All-or-nothing with `origin = "gui"` through
    /// [`SidecarDocument::apply_metadata_draft`] + [`Self::save_sidecar`]
    /// (CAS, atomar). Returns `Ok(true)` on update, `Ok(false)` for
    /// idempotent no-ops. Loud on invalid values — nothing is written then.
    pub fn commit_metadata_draft(&mut self) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::CommitMetadataDraft);
        self.ensure_document_loaded()?;
        self.ensure_meta_buffers();
        let current = self.metadata_draft();
        let mut fields = BTreeMap::new();
        for id in METADATA_FIELD_IDS {
            let buffered = self.meta_buffers.get(*id).cloned().unwrap_or_default();
            let stored = current.get(*id);
            if buffered.is_empty() {
                if stored.is_some() {
                    fields.insert((*id).to_string(), String::new());
                }
            } else if stored.is_none_or(|value| value != &buffered) {
                fields.insert((*id).to_string(), buffered);
            }
        }
        for (field, value) in &fields {
            validate_metadata_field_value(field, value)?;
        }
        if fields.is_empty() {
            info!("metadata draft for {} unchanged", self.path.trim());
            self.status = Str::MetadataDraftUnchanged.t().into();
            return Ok(false);
        }
        let timestamp = now_rfc3339_utc();
        let changed = {
            let document = self.document.as_mut().expect("document was ensured");
            document.apply_metadata_draft(&fields, Self::META_ORIGIN_GUI, &timestamp)?
        };
        if !changed {
            info!("metadata draft for {} unchanged", self.path.trim());
            self.status = Str::MetadataDraftUnchanged.t().into();
            return Ok(false);
        }
        let rev = self
            .document
            .as_ref()
            .map(|document| document.metadata.latest_rev())
            .unwrap_or(0);
        self.meta_buffers_key = Some((self.path.trim().to_string(), rev));
        self.meta_buffers_dirty = false;
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!(
                "metadata draft for {} updated (rev {rev})",
                self.path.trim()
            );
            self.status = Str::MetadataDraftSaved.t().into();
        }
        Ok(true)
    }

    /// KITTEST-COVERAGE-STATES-1: copy the current draft buffer values
    /// (non-empty fields, as shown in the editor — including unsaved
    /// keystrokes) into the metadata panel's own session clipboard. Pure
    /// session state: never touches the sidecar. Returns the field count.
    pub fn copy_metadata_draft(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::CopyMetadataDraft);
        self.ensure_document_loaded()?;
        self.ensure_meta_buffers();
        let copied: BTreeMap<String, String> = METADATA_FIELD_IDS
            .iter()
            .filter_map(|id| {
                let value = self.meta_buffers.get(*id).cloned().unwrap_or_default();
                (!value.trim().is_empty()).then(|| ((*id).to_string(), value))
            })
            .collect();
        let count = copied.len();
        self.meta_clipboard = Some(copied);
        info!("metadata copy for {} ({count} field(s))", self.path.trim());
        self.status = Str::MetadataCopiedPattern.format_arg(&count.to_string());
        Ok(count)
    }

    /// KITTEST-COVERAGE-STATES-1: paste the metadata clipboard onto the loaded
    /// image through the same commit path as a manual draft edit (origin
    /// `gui`, CAS + atomar). The pasted values are written into the editor
    /// buffers first so the panel shows them. Loud without a prior copy —
    /// never a silent no-op.
    pub fn paste_metadata_draft(&mut self) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::PasteMetadataDraft);
        let clipboard = self
            .meta_clipboard
            .clone()
            .ok_or_else(|| GuiError::Io(Str::MetadataNothingToPaste.t().to_string()))?;
        self.ensure_document_loaded()?;
        self.ensure_meta_buffers();
        for (field, value) in &clipboard {
            self.meta_buffers.insert(field.clone(), value.clone());
        }
        self.meta_buffers_dirty = true;
        let changed = self.commit_metadata_draft()?;
        info!(
            "metadata paste for {} ({} field(s), changed: {changed})",
            self.path.trim(),
            clipboard.len()
        );
        Ok(changed)
    }

    /// Clear draft fields (and/or `keywords`) on the loaded image, mirroring
    /// the CLI `meta draft clear --field` (one history entry with
    /// `origin = "gui"`, CAS + atomar). Unknown IDs fail loudly with
    /// all-or-nothing semantics; already-absent fields are an idempotent
    /// no-op. Returns `Ok(true)` on update.
    pub fn clear_metadata_fields(&mut self, fields: &[String]) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::ClearMetadataDraft);
        for id in fields {
            if id != "keywords" && !is_metadata_field(id) {
                return Err(GuiError::Io(
                    Str::MetadataUnknownFieldPattern.format_arg(id),
                ));
            }
        }
        self.ensure_document_loaded()?;
        let timestamp = now_rfc3339_utc();
        let removed = {
            let document = self.document.as_mut().expect("document was ensured");
            let mut removed = BTreeSet::new();
            for id in fields {
                if id == "keywords" {
                    if !document.keywords.is_empty() {
                        document.keywords.clear();
                        removed.insert("keywords".to_string());
                    }
                } else if document.metadata.draft.remove(id).is_some() {
                    removed.insert(id.clone());
                }
            }
            if removed.is_empty() {
                info!("metadata clear for {} unchanged", self.path.trim());
                self.status = Str::MetadataDraftUnchanged.t().into();
                return Ok(false);
            }
            let removed_list: Vec<String> = removed.into_iter().collect();
            let rev = document.metadata.latest_rev() + 1;
            document.metadata.history.insert(
                0,
                MetadataHistoryEntry {
                    rev,
                    timestamp,
                    origin: Self::META_ORIGIN_GUI.to_string(),
                    changed: removed_list,
                },
            );
            document
                .metadata
                .history
                .truncate(MAX_METADATA_HISTORY_ENTRIES);
            document.validate()?;
            document
                .metadata
                .history
                .first()
                .map(|entry| entry.changed.join(", "))
                .unwrap_or_default()
        };
        self.resync_meta_buffers();
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!("metadata clear for {} removed {removed}", self.path.trim());
            self.status = Str::MetadataDraftCleared.t().into();
        }
        Ok(true)
    }

    /// Clear the whole draft (`--all`, history is kept and gains one entry),
    /// mirroring the CLI. Idempotent no-op on an already-empty draft.
    pub fn clear_metadata_draft_all(&mut self) -> Result<bool, GuiError> {
        self.ensure_document_loaded()?;
        let timestamp = now_rfc3339_utc();
        let removed = {
            let document = self.document.as_mut().expect("document was ensured");
            if !document.clear_metadata_draft(Self::META_ORIGIN_GUI, &timestamp)? {
                info!("metadata clear for {} unchanged", self.path.trim());
                self.status = Str::MetadataDraftUnchanged.t().into();
                return Ok(false);
            }
            document
                .metadata
                .history
                .first()
                .map(|entry| entry.changed.join(", "))
                .unwrap_or_default()
        };
        self.resync_meta_buffers();
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!("metadata clear for {} removed {removed}", self.path.trim());
            self.status = Str::MetadataDraftCleared.t().into();
        }
        Ok(true)
    }

    /// Explicitly clear the whole metadata history (the only way to empty
    /// it; draft values are kept). Returns the number of removed entries.
    pub fn clear_metadata_history_gui(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::ClearMetadataHistory);
        self.ensure_document_loaded()?;
        let removed = {
            let document = self.document.as_mut().expect("document was ensured");
            let removed = document.metadata.history.len();
            document.clear_metadata_history();
            document.validate()?;
            removed
        };
        if removed == 0 {
            self.status = Str::MetadataDraftUnchanged.t().into();
            return Ok(0);
        }
        self.save_sidecar();
        if self.error().is_none() {
            info!("metadata history for {} cleared", self.path.trim());
            self.status = Str::MetadataHistoryClearedPattern.format_arg(&removed.to_string());
        }
        Ok(removed)
    }

    /// Embedded IPTC of the loaded image (JPEG IIM/XMP, read-only).
    /// Non-JPEG sources yield `Ok(None)` ("nicht verfügbar"); present-but-
    /// broken JPEG segments are a loud error, never a silent skip.
    pub fn embedded_metadata(&self) -> Result<Option<IptcMetadata>, GuiError> {
        let path = self.path.trim().to_string();
        if path.is_empty() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| GuiError::Io(format!("cannot read `{path}`: {error}")))?;
        if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
            return Ok(None);
        }
        extract_metadata(&bytes).map(Some).map_err(|error| {
            GuiError::Io(Str::MetadataEmbeddedUnreadablePattern.format_arg(&error.to_string()))
        })
    }

    /// Cached [`Self::embedded_metadata`] for the panel: the file is only
    /// re-read when `(path, length, mtime)` changed, so hovering the panel
    /// never pays per-frame file IO. Failures stay visible as `Err` text.
    fn embedded_cached(&mut self) -> Result<Option<IptcMetadata>, String> {
        let path = self.path.trim().to_string();
        let fingerprint = std::fs::metadata(&path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()));
        let fresh = match (&self.meta_embedded_cache, &fingerprint) {
            (
                Some(EmbeddedCache {
                    path: cached_path,
                    len,
                    mtime,
                    ..
                }),
                Some((current_len, current_mtime)),
            ) if cached_path == &path && len == current_len && mtime == current_mtime => true,
            (None, None) if path.is_empty() => true,
            _ => false,
        };
        if !fresh {
            let result = self.embedded_metadata().map_err(|error| error.to_string());
            let (len, mtime) = fingerprint.unwrap_or((0, None));
            self.meta_embedded_cache = Some(EmbeddedCache {
                path,
                len,
                mtime,
                result: result.clone(),
            });
            return result;
        }
        self.meta_embedded_cache
            .as_ref()
            .map(|cache| cache.result.clone())
            .unwrap_or(Ok(None))
    }

    /// Override the user-global meta-presets directory (headless tests).
    /// `None` restores the production default.
    pub fn set_meta_presets_dir(&mut self, dir: Option<PathBuf>) {
        self.meta_presets_dir_override = dir;
    }

    /// Effective meta-presets directory: the test override or the
    /// user-global directory shared with edit presets (SOLL §5). `None`
    /// means the platform config base is unavailable (loud, no fallback).
    fn meta_presets_dir(&self) -> Option<PathBuf> {
        self.meta_presets_dir_override
            .clone()
            .or_else(default_meta_presets_dir)
    }

    /// Re-scan the meta-presets directory. A missing directory means "no
    /// presets saved yet" (not an error); broken files stay visible as
    /// failed entries — never skipped silently.
    pub fn refresh_meta_presets(&mut self) {
        match self.meta_presets_dir() {
            Some(dir) => {
                self.meta_preset_entries = scan_meta_presets_dir(&dir);
            }
            None => {
                self.meta_preset_entries.clear();
                self.status = Str::MetadataNoPresetDir.t().into();
            }
        }
    }

    /// Display names of the available (valid) meta presets, sorted.
    pub fn meta_preset_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for entry in &self.meta_preset_entries {
            if let MetaPresetEntry::Available { preset, .. } = entry {
                names.push(preset.name.clone());
            }
        }
        names.sort();
        names
    }

    /// Declared placeholders `(name, description)` of one preset.
    pub fn meta_preset_placeholders(&self, spec: &str) -> Result<Vec<(String, String)>, GuiError> {
        let (_, preset) = self.load_gui_meta_preset(spec)?;
        Ok(preset
            .placeholders
            .iter()
            .map(|placeholder| (placeholder.name.clone(), placeholder.description.clone()))
            .collect())
    }

    /// Resolve a preset spec (display name against the effective directory,
    /// or an explicit file path — same resolution as the CLI) and load it.
    /// `keywords` in `fields` and every other deviation fail loudly here,
    /// before any target is touched.
    fn load_gui_meta_preset(&self, spec: &str) -> Result<(PathBuf, MetaPresetFile), GuiError> {
        let explicit = self.meta_presets_dir_override.clone();
        let path = resolve_meta_preset_path(spec, explicit.as_deref()).map_err(|error| {
            GuiError::Io(format!("meta preset `{spec}` cannot be resolved: {error}"))
        })?;
        load_meta_preset_file(&path)
            .map(|preset| (path.clone(), preset))
            .map_err(|error| {
                GuiError::Io(format!(
                    "meta preset `{}` rejected: {error}",
                    path.display()
                ))
            })
    }

    /// Apply a meta preset to the loaded image: render upfront (every
    /// placeholder variable required; missing/unknown variables and limit
    /// violations abort with nothing written), then mutate via
    /// [`SidecarDocument::apply_metadata_draft`] (`origin =
    /// "preset:<name>"`) + [`Self::save_sidecar`]. Idempotent
    /// re-application reports `Ok(false)` without a history entry.
    pub fn apply_meta_preset_loaded(
        &mut self,
        spec: &str,
        vars: &BTreeMap<String, String>,
    ) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyMetaPreset);
        let (path, preset) = self.load_gui_meta_preset(spec)?;
        let resolved = render_meta_preset(&preset, vars, &path.display().to_string())
            .map_err(|error| GuiError::Io(format!("meta preset apply rejected: {error}")))?;
        self.ensure_document_loaded()?;
        let origin = format!("preset:{}", preset.name);
        let timestamp = now_rfc3339_utc();
        let changed = {
            let document = self.document.as_mut().expect("document was ensured");
            document.apply_metadata_draft(&resolved, &origin, &timestamp)?
        };
        if !changed {
            info!(
                "meta preset `{}` unchanged for {}",
                preset.name,
                self.path.trim()
            );
            self.status = Str::MetadataPresetUnchangedPattern.format_arg(&preset.name);
            return Ok(false);
        }
        self.resync_meta_buffers();
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!(
                "meta preset `{}` applied to {}",
                preset.name,
                self.path.trim()
            );
            self.status = Str::MetadataPresetAppliedPattern.format_arg(&preset.name);
        }
        Ok(true)
    }

    /// Apply a meta preset to every image of the filmstrip selection (empty
    /// selection falls back to the loaded image). The preset renders once
    /// upfront — a render failure aborts everything with nothing written
    /// (loud). Per target: CAS + atomic save; failures are loud per image
    /// (`error!` + report entry) and never abort the rest (Stapel-Muster,
    /// `metadata.md` §4).
    pub fn apply_meta_preset_to_selection(
        &mut self,
        spec: &str,
        vars: &BTreeMap<String, String>,
    ) -> SelectionSyncReport {
        let report = SelectionSyncReport::default();
        let (path, preset) = match self.load_gui_meta_preset(spec) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.show_error(error);
                return report;
            }
        };
        let resolved = match render_meta_preset(&preset, vars, &path.display().to_string()) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.show_error(format!("meta preset apply rejected: {error}"));
                return report;
            }
        };
        let origin = format!("preset:{}", preset.name);
        self.apply_resolved_to_selection(&resolved, &origin, "preset", &preset.name)
    }

    /// Field-selective metadata sync from the loaded image onto the
    /// filmstrip selection (empty selection falls back to the loaded image
    /// itself, like [`Self::apply_metadata_batch`]). Mirror semantics per
    /// target (SOLL §6): a source-absent field is removed on the target,
    /// `keywords` are replaced wholesale; non-selected fields stay
    /// untouched. `fields` must be non-empty (registry IDs, `keywords`
    /// allowed) — no silent transfer-all. A missing sidecar (source or
    /// target) is a loud error ("first open/import the image"), never a
    /// silent creation. Recipes, masks and edit history are never touched.
    pub fn sync_metadata_to_selection(&mut self, fields: &BTreeSet<String>) -> SelectionSyncReport {
        instrument_gui_action!(self, GuiAction::SyncMetadata);
        let report = SelectionSyncReport::default();
        if fields.is_empty() {
            self.show_error(Str::MetadataSyncNeedsFields.t());
            return report;
        }
        for id in fields {
            if id != "keywords" && !is_metadata_field(id) {
                self.show_error(Str::MetadataUnknownFieldPattern.format_arg(id));
                return report;
            }
        }
        let source = self.path.trim().to_string();
        if source.is_empty() {
            self.show_error(Str::NoImageLoaded.t());
            return report;
        }
        let source_sidecar = sidecar_path_for(Path::new(&source));
        let source_document = match load_sidecar(&source_sidecar) {
            Ok(document) => document,
            Err(_) => {
                self.show_error(Str::MetadataNoSidecarPattern.format_arg(&source));
                return report;
            }
        };
        let source_name = Path::new(&source)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty());
        let Some(source_name) = source_name else {
            self.show_error(Str::NoImageLoaded.t());
            return report;
        };
        let origin = format!("sync:{source_name}");
        let source_draft = source_document.metadata.draft.clone();
        let source_keywords = source_document.keywords.clone();
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() {
            targets.push(source.clone());
        }
        let mut report = SelectionSyncReport::default();
        for target in &targets {
            match Self::apply_meta_sync_to_target(
                Path::new(target),
                &source_draft,
                &source_keywords,
                fields,
                &origin,
            ) {
                Ok(true) => {
                    info!("metadata sync: `{target}` updated (from `{source_name}`)");
                    self.refresh_entry(Path::new(target));
                    if self.path.trim() == target.as_str() {
                        self.sidecar_revision = None;
                        self.reload_document_for_batch_target(Path::new(target));
                        self.resync_meta_buffers();
                    }
                    report.applied.push(target.clone());
                }
                Ok(false) => {
                    info!("metadata sync: `{target}` unchanged");
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("metadata sync failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status =
                Str::MetadataSyncAppliedPattern.format_arg(&report.applied.len().to_string());
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(
                Str::MetadataSyncFailedPattern
                    .t()
                    .replacen("{}", &report.failed.len().to_string(), 1)
                    .replacen("{}", &joined, 1),
            );
        }
        report
    }

    /// Shared per-target loop for a pre-rendered draft (`preset:<name>`
    /// origin): CAS + atomic save per file, failures never abort the rest.
    /// `kind`/`name` only feed the status line and `info!` logs.
    fn apply_resolved_to_selection(
        &mut self,
        resolved: &BTreeMap<String, String>,
        origin: &str,
        kind: &str,
        name: &str,
    ) -> SelectionSyncReport {
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() && !self.path.trim().is_empty() {
            targets.push(self.path.trim().to_string());
        }
        let mut report = SelectionSyncReport::default();
        if targets.is_empty() {
            self.status = Str::NoImagesSelected.t().into();
            return report;
        }
        for target in &targets {
            match Self::apply_preset_to_target(Path::new(target), resolved, origin) {
                Ok(true) => {
                    info!("metadata {kind} `{name}`: `{target}` updated");
                    self.refresh_entry(Path::new(target));
                    if self.path.trim() == target.as_str() {
                        self.sidecar_revision = None;
                        self.reload_document_for_batch_target(Path::new(target));
                        self.resync_meta_buffers();
                    }
                    report.applied.push(target.clone());
                }
                Ok(false) => {
                    info!("metadata {kind} `{name}`: `{target}` unchanged");
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("metadata {kind} `{name}` failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = Str::MetadataPresetBatchPattern
                .t()
                .replacen("{}", name, 1)
                .replacen("{}", &report.applied.len().to_string(), 1);
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(
                Str::MetadataSyncFailedPattern
                    .t()
                    .replacen("{}", &report.failed.len().to_string(), 1)
                    .replacen("{}", &joined, 1),
            );
        }
        report
    }

    /// One atomic preset step for `target` (mirrors the CLI
    /// `apply_meta_preset_to_target`): load, mutate a clone via
    /// `apply_metadata_draft`, validate, CAS + atomic save. A missing
    /// sidecar is a loud per-target error, never a silent creation.
    fn apply_preset_to_target(
        target: &Path,
        resolved: &BTreeMap<String, String>,
        origin: &str,
    ) -> Result<bool, String> {
        let sidecar = sidecar_path_for(target);
        let document = match load_sidecar(&sidecar) {
            Ok(document) => document,
            Err(lumina_sidecar::SidecarError::Missing(_)) => {
                return Err(format!(
                    "no sidecar for `{}`; run `import` first",
                    target.display()
                ));
            }
            Err(error) => return Err(error.to_string()),
        };
        let expected = document_revision(&document).map_err(|error| error.to_string())?;
        let timestamp = now_rfc3339_utc();
        let mut candidate = document.clone();
        if !candidate
            .apply_metadata_draft(resolved, origin, &timestamp)
            .map_err(|error| error.to_string())?
        {
            return Ok(false);
        }
        sidecar_rebase::save_rebased_unit(&sidecar, &document, &candidate, Some(&expected))
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// One atomic sync step for `target` (mirrors the CLI
    /// `apply_meta_sync_to_target`): mirror the selected source fields onto
    /// a clone (source-absent fields removed, `keywords` replaced
    /// wholesale), exactly one history entry (`origin`), validate, CAS +
    /// atomic save. Idempotent no-ops report `Ok(false)` without a history
    /// entry or write.
    fn apply_meta_sync_to_target(
        target: &Path,
        source_draft: &BTreeMap<String, String>,
        source_keywords: &[String],
        fields: &BTreeSet<String>,
        origin: &str,
    ) -> Result<bool, String> {
        let sidecar = sidecar_path_for(target);
        let document = match load_sidecar(&sidecar) {
            Ok(document) => document,
            Err(lumina_sidecar::SidecarError::Missing(_)) => {
                return Err(format!(
                    "no sidecar for `{}`; run `import` first",
                    target.display()
                ));
            }
            Err(error) => return Err(error.to_string()),
        };
        let expected = document_revision(&document).map_err(|error| error.to_string())?;
        let timestamp = now_rfc3339_utc();
        let mut candidate = document.clone();
        let mut changed = BTreeSet::new();
        for id in fields {
            if id == "keywords" {
                if candidate.keywords != source_keywords {
                    candidate.keywords = source_keywords.to_vec();
                    changed.insert(id.clone());
                }
            } else if let Some(value) = source_draft.get(id) {
                if candidate.metadata.draft.get(id).map(String::as_str) != Some(value.as_str()) {
                    candidate.metadata.draft.insert(id.clone(), value.clone());
                    changed.insert(id.clone());
                }
            } else if candidate.metadata.draft.remove(id).is_some() {
                changed.insert(id.clone());
            }
        }
        if changed.is_empty() {
            return Ok(false);
        }
        let changed_list: Vec<String> = changed.into_iter().collect();
        let rev = candidate.metadata.latest_rev() + 1;
        candidate.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp,
                origin: origin.to_string(),
                changed: changed_list,
            },
        );
        candidate
            .metadata
            .history
            .truncate(MAX_METADATA_HISTORY_ENTRIES);
        candidate.validate().map_err(|error| error.to_string())?;
        sidecar_rebase::save_rebased_unit(&sidecar, &document, &candidate, Some(&expected))
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// Quick Develop (Welle 3, LR-13 light): set one of
    /// `exposure`/`contrast`/`highlights`/`shadows` on the session recipe and
    /// persist it through the normal save/render path (so the preview
    /// generation bumps and the sidecar keeps the result). Backs both the
    /// Library Quick Develop drawer and
    /// [`Self::apply_adjustment_to_selection`]'s key gate. Unknown keys, a
    /// missing image or a path-less (byte-drop) session fail loudly — never
    /// a silent no-op.
    pub fn apply_quick_develop(&mut self, key: &str, value: f64) -> Result<(), GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") {
            return Err(GuiError::Io(Str::UnknownAdjustment.format_arg(key)));
        }
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        if self.path.trim().is_empty() {
            return Err(GuiError::Io(Str::SaveNeedsLocalPath.t().to_string()));
        }
        self.ensure_document_loaded()?;
        trace!("GUI interaction: apply_quick_develop {key}={value}");
        self.recipe.adjustments.insert(key.into(), value);
        self.mark_dirty();
        self.save_sidecar();
        self.render()?;
        // `render` overwrites the status ("Preview current"); restore the
        // quick-develop message so the action stays visible.
        self.status = Str::QuickDevelopAppliedPattern.format_arg(key);
        Ok(())
    }

    /// Current rating and flag of the active virtual copy (LR-01). Returns
    /// `None` when no document is loaded; read-only accessor for the rating
    /// section and headless tests.
    pub fn active_rating_flag(&self) -> Option<(u8, Flag)> {
        self.document.as_ref().and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
                .map(|copy| (copy.rating, copy.flag))
        })
    }

    /// Duplicate the active virtual copy under a fresh stable id (LR-09,
    /// `Cmd/Ctrl+'` shortcut path). Unstored session edits are saved first so
    /// the duplicate inherits the currently visible recipe rather than the
    /// last saved one; the new copy is then selected (Lightroom behaviour).
    /// Fails loudly when no image/document is loaded.
    pub fn duplicate_active_copy(&mut self) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::DuplicateCopy);
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        // Persist unsaved edits first: `duplicate_virtual_copy` clones the
        // *stored* copy, so without this save the duplicate would silently
        // drop what the user currently sees.
        self.save_sidecar();
        let new_id = {
            let document = self.document.as_ref().expect("document was ensured");
            let mut counter = document.virtual_copies.len() + document.deleted_virtual_copies.len();
            loop {
                counter += 1;
                let candidate = format!("vc-copy-{counter}");
                let taken = document
                    .virtual_copies
                    .iter()
                    .any(|copy| copy.id == candidate)
                    || document
                        .deleted_virtual_copies
                        .iter()
                        .any(|copy| copy.id == candidate);
                if !taken {
                    break candidate;
                }
            }
        };
        let source_name = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| format!("{} copy", copy.name))
            .unwrap_or_else(|| new_id.clone());
        self.duplicate_virtual_copy(new_id.clone(), source_name)?;
        self.save_sidecar();
        self.select_virtual_copy(&new_id)?;
        self.status = Str::VirtualCopyDuplicatedPattern.format_arg(&new_id);
        info!("GUI interaction: duplicate_active_copy -> {new_id}");
        Ok(new_id)
    }

    /// Switch the active virtual copy (REVIEW-GUI-VCSWITCH-1).
    ///
    /// Switching adopts the target copy's stored recipe. Session state that
    /// belonged to the previous copy is reset: the history selection and any
    /// in-progress mask-tool gesture. Unsaved edits of the previous copy are
    /// **discarded** by design (the stored recipe is authoritative); this is
    /// made visible through a distinct status message plus a `warn!` log —
    /// never silently.
    ///
    /// Errors (no sidecar, unknown id) are returned to the caller; UI call
    /// sites must surface them via `show_error` instead of discarding them.
    pub fn select_virtual_copy(&mut self, id: &str) -> Result<(), GuiError> {
        let Some(document) = &self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        // Dirty check against the copy we are leaving, BEFORE adopting the new
        // recipe.
        let discarded_unsaved = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .is_some_and(|previous| previous.recipe != self.recipe);
        let previous_id = self.virtual_copy_id.clone();
        self.virtual_copy_id = copy.id.clone();
        self.recipe = copy.recipe.clone();
        // GEN-ONNX-1 Welle 2b: the generative artifacts belong to the previous
        // copy's recipe/source identity; drop them so the new copy resolves its
        // own (loudly if none is available).
        self.generative_artifacts = GenerativeArtifacts::default();
        self.generative_role_status = [GenerativeRoleStatus::Missing; 2];
        self.generative_memo = None;
        // G04-FOLLOWUP-1: per-copy session default — the detect input tracks
        // the newly adopted recipe's visualize threshold (else 0.5).
        self.spot_detect_threshold = self.recipe.spot_visualize_threshold().unwrap_or(0.5);
        self.selected_mask_id = copy
            .mask_layers
            .first()
            .map(|layer| layer.mask.mask_id.clone());
        // Per-copy session state resets (REVIEW-GUI-VCSWITCH-1): a history
        // selection or an in-progress drag of the previous copy must never
        // leak into the newly selected one.
        {
            self.history_selected = None;
            self.pending_brush_marks.clear();
            self.drag_start = None;
            self.drag_current = None;
            self.drawing = false;
        }
        if discarded_unsaved {
            warn!(
                "virtual-copy switch from `{previous_id}` to `{}` discarded unsaved edits",
                self.virtual_copy_id
            );
        }
        // The status is set *after* `render` because a successful render
        // overwrites it ("Preview current"); on a render failure the error
        // path keeps its own visible state.
        let outcome = self.render();
        if outcome.is_ok() {
            self.status = if discarded_unsaved {
                format!(
                    "Switched to copy `{}` — unsaved edits of `{previous_id}` were discarded",
                    self.virtual_copy_id
                )
            } else {
                format!("Switched to copy `{}`", self.virtual_copy_id)
            };
        }
        outcome
    }

    /// Select a mask from the active copy's library and make it the active layer.
    /// The matte is only referenced; no payload is copied or modified.
    pub fn select_mask(&mut self, mask_id: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SelectMask);
        self.ensure_document_loaded()?;
        let document = self.document.as_mut().expect("document was ensured");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
            return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
        }
        if let Some(layer) = copy.mask_layers.first_mut() {
            layer.mask = MaskReference {
                copy_id: copy.id.clone(),
                mask_id: mask_id.into(),
                extras: BTreeMap::new(),
            };
        } else {
            copy.mask_layers.push(MaskLayer {
                id: "layer-1".into(),
                mask: MaskReference {
                    copy_id: copy.id.clone(),
                    mask_id: mask_id.into(),
                    extras: BTreeMap::new(),
                },
                inverted: false,
                feather: 0.0,
                blur: 0.0,
                density: 1.0,
                extras: BTreeMap::new(),
                visible: true,
            });
        }
        self.selected_mask_id = Some(mask_id.into());
        // LRPAR-G03-MASKGROUP-03: selecting a single mask leaves group mode.
        self.selected_group_id = None;
        self.render_key = None;
        self.status = Str::MaskSelected.format_arg(mask_id);
        Ok(())
    }

    pub fn selected_mask_id(&self) -> Option<&str> {
        self.selected_mask_id.as_deref()
    }

    /// Create a pending library entry. Inference is deliberately not started here.
    pub fn create_mask(&mut self, name: impl Into<String>) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::CreateMask);
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let id = format!("mask-{}", blake3::hash(name.as_bytes()).to_hex());
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        let source_hash = self
            .source_bytes
            .as_ref()
            .map(|b| format!("blake3:{}", blake3::hash(b).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        let source_byte_length = self.source_bytes.as_ref().map_or(0, |b| b.len() as u64);
        let document = self.document.as_mut().expect("document was ensured");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if copy.mask_library.iter().any(|mask| mask.id == id) {
            return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
        }
        copy.mask_library.push(MaskDefinition {
            id: id.clone(),
            name,
            source_fingerprint: SourceFingerprint {
                content_hash: source_hash,
                byte_length: source_byte_length,
                extras: BTreeMap::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "pending".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_context: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            model: ModelIdentity {
                name: "unavailable".into(),
                version: "pending".into(),
                hash: "pending".into(),
                extras: BTreeMap::new(),
            },
            inference_resolution: Resolution {
                width: frame.width,
                height: frame.height,
                extras: BTreeMap::new(),
            },
            preprocessing: Preprocessing {
                name: "pending".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Pending,
            created_at: "pending".into(),
            generator_version: env!("CARGO_PKG_VERSION").into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            extras: BTreeMap::new(),
            ai_select: None,
        });
        self.select_mask(&id)?;
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    pub fn rename_mask(&mut self, mask_id: &str, name: impl Into<String>) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let copy = self.active_copy_mut()?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.name = name;
        self.status = Str::MaskRenamed.t().into();
        Ok(())
    }

    // ---- G-03 Maskierungs-Parität: AI-select, Range-Stufen, Kombinatorik ----

    /// Build a fresh source [`MaskDefinition`] for the active copy from the
    /// loaded frame (dimensions + source identity). Shared by all G-03 mask
    /// constructors so geometry/model placeholders stay identical.
    fn new_source_mask_template(
        &self,
        id: &str,
        name: &str,
        status: MaskStatus,
    ) -> Result<MaskDefinition, GuiError> {
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        let source_hash = self
            .source_bytes
            .as_ref()
            .map(|b| format!("blake3:{}", blake3::hash(b).to_hex()))
            .unwrap_or_else(|| "blake3:unknown".into());
        let source_byte_length = self.source_bytes.as_ref().map_or(0, |b| b.len() as u64);
        Ok(MaskDefinition {
            id: id.into(),
            name: name.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: source_hash,
                byte_length: source_byte_length,
                extras: BTreeMap::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "gui-mask".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_context: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            model: ModelIdentity {
                name: "unavailable".into(),
                version: "pending".into(),
                hash: "pending".into(),
                extras: BTreeMap::new(),
            },
            inference_resolution: Resolution {
                width: frame.width,
                height: frame.height,
                extras: BTreeMap::new(),
            },
            preprocessing: Preprocessing {
                name: "pending".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status,
            created_at: "pending".into(),
            generator_version: env!("CARGO_PKG_VERSION").into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            ai_select: None,
            extras: BTreeMap::new(),
        })
    }

    /// Push a library definition with validate-then-save and in-memory
    /// rollback: a rejected definition never reaches the file (loud, never
    /// partial). Returns the new mask id.
    fn push_mask_definition(&mut self, definition: MaskDefinition) -> Result<String, GuiError> {
        self.ensure_document_loaded()?;
        let id = definition.id.clone();
        let copy_id = self.virtual_copy_id.clone();
        {
            let document = self.document.as_mut().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            if copy.mask_library.iter().any(|mask| mask.id == id) {
                return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
            }
            copy.mask_library.push(definition);
        }
        // Validate outside the copy borrow; roll the push back in memory when
        // the graph (arity, cycles, ranges, ai_select placement) rejects it.
        if let Err(error) = self
            .document
            .as_ref()
            .expect("document was ensured")
            .validate()
        {
            self.document
                .as_mut()
                .expect("document was ensured")
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .expect("copy was found above")
                .mask_library
                .retain(|mask| mask.id != id);
            return Err(GuiError::Io(error.to_string()));
        }
        self.save_sidecar();
        self.mark_dirty();
        Ok(id)
    }

    /// Create an AI-select source mask (G-03). Status `Pending` until a model
    /// infers the matte; a missing model stays loudly missing (never a
    /// geometric fallback — see `lumina-core::masks`).
    pub fn create_ai_mask(
        &mut self,
        kind: AiSelectKind,
        detail: Option<String>,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::CreateAiMask);
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        if let Some(detail) = &detail {
            if detail.len() > 64
                || detail.trim().is_empty()
                || detail != detail.trim()
                || detail.chars().any(|c| c.is_control())
            {
                return Err(GuiError::Io(
                    "Detail must be trimmed, non-empty, free of control characters and at most 64 chars".into(),
                ));
            }
        }
        let id = format!(
            "mask-{}",
            blake3::hash(format!("ai-select\0{}\0{name}", kind.as_str()).as_bytes()).to_hex()
        );
        let mut definition = self.new_source_mask_template(&id, &name, MaskStatus::Pending)?;
        definition.ai_select = Some(AiSelect {
            kind,
            detail,
            extras: BTreeMap::new(),
        });
        let id = self.push_mask_definition(definition)?;
        self.select_mask(&id)?;
        info!("GUI interaction: create_ai_mask {kind:?} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    /// Create a deterministic luminance-range source mask (G-03). Usable
    /// immediately (`Valid`): no model, no cache, pure function of the
    /// source pixels.
    pub fn create_luminance_range_mask(
        &mut self,
        min: f32,
        max: f32,
        feather: f32,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::CreateLuminanceRangeMask);
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let id = format!(
            "mask-{}",
            blake3::hash(format!("luminance-range\0{name}").as_bytes()).to_hex()
        );
        let mut definition = self.new_source_mask_template(&id, &name, MaskStatus::Valid)?;
        definition.prompt = Some(MaskPrompt::LuminanceRange {
            min,
            max,
            feather,
            transformation: PromptTransform::default(),
        });
        let id = self.push_mask_definition(definition)?;
        self.select_mask(&id)?;
        info!("GUI interaction: create_luminance_range_mask {min}/{max}/{feather} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    /// Create a deterministic color-range source mask (G-03). Usable
    /// immediately (`Valid`), like the luminance range.
    #[allow(clippy::too_many_arguments)]
    pub fn create_color_range_mask(
        &mut self,
        hue_center: f32,
        hue_width: f32,
        sat_min: f32,
        sat_max: f32,
        lum_min: f32,
        lum_max: f32,
        feather: f32,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::CreateColorRangeMask);
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let id = format!(
            "mask-{}",
            blake3::hash(format!("color-range\0{name}").as_bytes()).to_hex()
        );
        let mut definition = self.new_source_mask_template(&id, &name, MaskStatus::Valid)?;
        definition.prompt = Some(MaskPrompt::ColorRange {
            hue_center,
            hue_width,
            sat_min,
            sat_max,
            lum_min,
            lum_max,
            feather,
            transformation: PromptTransform::default(),
        });
        let id = self.push_mask_definition(definition)?;
        self.select_mask(&id)?;
        info!("GUI interaction: create_color_range_mask {hue_center}/{hue_width} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    /// Combine the selected mask with another mask of the active copy (G-03):
    /// `Union` (Add), `Subtract` (selected first = basis) or `Invert`
    /// (selected only, `other_id` ignored). Unknown ids, `Source` and wrong
    /// arity are loud errors; cycles are rejected by validation with
    /// rollback. The new node starts `Pending` until every input resolves.
    pub fn combine_masks(
        &mut self,
        operation: MaskOperation,
        other_id: &str,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::CombineMasks);
        // The panel combines with Add (union), Subtract and Invert; source
        // masks are created, not combined, and intersect stays CLI-only.
        if !matches!(
            operation,
            MaskOperation::Union | MaskOperation::Subtract | MaskOperation::Invert
        ) {
            return Err(GuiError::Io(
                "Combine needs union, subtract or invert (source masks are created, not combined)"
                    .into(),
            ));
        }
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        let selected = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let copy_id = self.virtual_copy_id.clone();
        // Both inputs must exist on the active copy (panel scope). Invert
        // uses the selection only — `other_id` is ignored, never validated.
        self.ensure_document_loaded()?;
        {
            let document = self.document.as_ref().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            let mut required: Vec<&str> = vec![&selected];
            if operation != MaskOperation::Invert {
                required.push(other_id);
            }
            for id in required {
                if !copy.mask_library.iter().any(|mask| mask.id == id) {
                    return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
                }
            }
        }
        let references = match operation {
            MaskOperation::Invert => vec![MaskReference {
                copy_id: copy_id.clone(),
                mask_id: selected.clone(),
                extras: BTreeMap::new(),
            }],
            MaskOperation::Union | MaskOperation::Subtract => {
                if other_id == selected {
                    return Err(GuiError::Io("Combine needs two different masks".into()));
                }
                [selected.clone(), other_id.to_string()]
                    .into_iter()
                    .map(|mask_id| MaskReference {
                        copy_id: copy_id.clone(),
                        mask_id,
                        extras: BTreeMap::new(),
                    })
                    .collect()
            }
            _ => {
                return Err(GuiError::Io(
                    "Only union, subtract and invert are combinable in the panel".into(),
                ));
            }
        };
        let id = format!(
            "mask-{}",
            blake3::hash(
                format!("combine-{operation:?}\0{selected}\0{other_id}\0{name}").as_bytes()
            )
            .to_hex()
        );
        let mut definition = self.new_source_mask_template(&id, &name, MaskStatus::Pending)?;
        definition.operation = operation;
        definition.references = references;
        let id = self.push_mask_definition(definition)?;
        self.select_mask(&id)?;
        info!("GUI interaction: combine_masks {operation:?} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    /// Visibility eye of the mask list (G-03). Sets `visible` on every layer
    /// of the active copy that references `mask_id`; when no layer references
    /// it yet, a layer is created (never an invented matte — only the
    /// reference). Persisted per virtual copy, loud on unknown masks.
    pub fn set_mask_visible(&mut self, mask_id: &str, visible: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetMaskVisible);
        self.ensure_document_loaded()?;
        let copy_id = self.virtual_copy_id.clone();
        {
            let document = self.document.as_mut().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
                return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
            }
            // Snapshot for rollback: the eye must never leave a half-written
            // layer list behind when validation rejects the result.
            let snapshot: Vec<(String, bool)> = copy
                .mask_layers
                .iter()
                .map(|layer| (layer.id.clone(), layer.visible))
                .collect();
            let layer_count = copy.mask_layers.len();
            let mut touched = false;
            for layer in copy
                .mask_layers
                .iter_mut()
                .filter(|layer| layer.mask.copy_id == copy_id && layer.mask.mask_id == mask_id)
            {
                layer.visible = visible;
                touched = true;
            }
            if !touched {
                let mut layer_id = format!("layer-{mask_id}");
                let mut suffix = 2;
                while copy.mask_layers.iter().any(|layer| layer.id == layer_id) {
                    layer_id = format!("layer-{mask_id}-{suffix}");
                    suffix += 1;
                }
                copy.mask_layers.push(MaskLayer {
                    id: layer_id,
                    mask: MaskReference {
                        copy_id: copy_id.clone(),
                        mask_id: mask_id.into(),
                        extras: BTreeMap::new(),
                    },
                    inverted: false,
                    feather: 0.0,
                    blur: 0.0,
                    density: 1.0,
                    visible,
                    extras: BTreeMap::new(),
                });
            }
            if let Err(error) = document.validate() {
                let copy = document
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == copy_id)
                    .expect("copy was found above");
                copy.mask_layers.truncate(layer_count);
                for layer in copy.mask_layers.iter_mut() {
                    if let Some((_, was)) = snapshot.iter().find(|(id, _)| id == &layer.id) {
                        layer.visible = *was;
                    }
                }
                return Err(GuiError::Io(error.to_string()));
            }
        }
        self.save_sidecar();
        self.mark_dirty();
        info!("GUI interaction: set_mask_visible {mask_id} -> {visible}");
        Ok(())
    }

    /// Eye state of the mask list (G-03): true when no layer references the
    /// mask yet (vacuous) or at least one referencing layer is visible.
    pub fn mask_visible(&self, mask_id: &str) -> bool {
        let Some(document) = self.document.as_ref() else {
            return true;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            return true;
        };
        let mut any = false;
        let mut visible = false;
        for layer in &copy.mask_layers {
            if layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id {
                any = true;
                visible = visible || layer.visible;
            }
        }
        !any || visible
    }

    /// Selected mask status for the panel status line (G-03): status plus the
    /// persisted error text, if any. `None` without a selection.
    pub fn selected_mask_status(&self) -> Option<(MaskStatus, Option<String>)> {
        let id = self.selected_mask_id.as_deref()?;
        let document = self.document.as_ref()?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)?;
        let mask = copy.mask_library.iter().find(|mask| mask.id == id)?;
        Some((mask.status.clone(), mask.error_text.clone()))
    }

    /// Master Show switch of the mask overlay (G-03). Display-only session
    /// state (never recipe/sidecar), ANDed with the G-11 overlay mode.
    pub fn show_mask_overlay(&self) -> bool {
        self.show_mask_overlay
    }

    pub fn set_show_mask_overlay(&mut self, shown: bool) {
        instrument_gui_action!(self, GuiAction::SetShowMaskOverlay);
        if self.show_mask_overlay == shown {
            return;
        }
        self.show_mask_overlay = shown;
        info!("GUI interaction: set_show_mask_overlay -> {shown}");
    }

    /// Matte tint of the mask overlay (G-03). Display-only session state.
    pub fn overlay_color(&self) -> [u8; 3] {
        self.overlay_color
    }

    pub fn set_overlay_color(&mut self, color: [u8; 3]) {
        instrument_gui_action!(self, GuiAction::SetOverlayColor);
        if self.overlay_color == color {
            return;
        }
        self.overlay_color = color;
        info!(
            "GUI interaction: set_overlay_color -> #{:02X}{:02X}{:02X}",
            color[0], color[1], color[2]
        );
    }

    /// Whether the selected mask's matte paints right now (G-03): the Show
    /// switch AND the G-11 mode must allow it, plus the mask's own eye — a
    /// mask whose layers are all invisible paints no overlay (the mask is
    /// "off"). A live in-progress gesture always paints; without a selection
    /// there is nothing to show.
    pub fn mask_overlay_allowed(&self) -> bool {
        if !self.show_mask_overlay || !self.overlay_visible() {
            return false;
        }
        if self.drawing && self.mask_tool != MaskTool::None {
            return true;
        }
        let Some(id) = self.selected_mask_id.as_deref() else {
            return false;
        };
        self.mask_visible(id)
    }

    pub fn set_mask_inverted(&mut self, inverted: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetMaskInverted);
        let layer = self.active_layer_mut()?;
        layer.inverted = inverted;
        // REVIEW-GUI-MASKRENDER-1: layer edits change the evaluated matte, so
        // the preview must actually re-render — route through `mark_dirty`
        // (which also schedules the debounced render), not just invalidate
        // the key.
        self.mark_dirty();
        Ok(())
    }

    pub fn set_mask_feather(&mut self, feather: f32) -> Result<(), GuiError> {
        if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
            return Err(GuiError::Io(Str::FeatheringMustBeBetween.t().to_string()));
        }
        self.active_layer_mut()?.feather = feather;
        // REVIEW-GUI-MASKRENDER-1: see `set_mask_inverted`. GUI-SLIDER-SAVE-1:
        // the feather slider commits like any other slider (CAS save at
        // debounce, loud conflicts).
        self.mark_recipe_dirty("mask.feather", f64::from(feather));
        Ok(())
    }

    /// Store a local adjustment as declarative layer metadata. Applying it to pixels
    /// requires the not-yet-implemented masked core pipeline; it is never baked in.
    pub fn set_mask_local_adjustment(&mut self, key: &str, value: f64) -> Result<(), GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") || !value.is_finite()
        {
            return Err(GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()));
        }
        self.active_layer_mut()?
            .extras
            .insert(format!("adjustment_{key}"), Value::from(value));
        // GUI-SLIDER-SAVE-1: a local adjustment is recipe data — it must arm
        // the re-render AND the debounced save (previously neither happened).
        self.mark_recipe_dirty(&format!("mask.local.{key}"), value);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    pub fn offer_mask_recalculation(&mut self) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::OfferMaskRecalculation);
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        let offered = !matches!(mask.status, MaskStatus::Valid);
        self.status = if offered {
            Str::MaskStaleRecalc.t()
        } else {
            Str::MaskCurrentNoRecalc.t()
        }
        .into();
        Ok(offered)
    }

    pub fn mark_mask_for_recalculation(&mut self) -> Result<(), GuiError> {
        let mask_id = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        self.mark_mask_pending(&mask_id)?;
        self.status = Str::RecalcRequested.t().into();
        Ok(())
    }

    /// Marks exactly one mask as explicitly pending re-inference.
    ///
    /// GUI-GEN-GRANULAR-10: the shared primitive of the per-mask button
    /// ([`Self::mark_mask_for_recalculation`]) and the collective regeneration
    /// ([`Self::regenerate_stale`]). It never runs inference itself: the
    /// request is visible (`Pending` + explanation) and consumed explicitly.
    fn mark_mask_pending(&mut self, mask_id: &str) -> Result<(), GuiError> {
        let mask = self
            .active_copy_mut()?
            .mask_library
            .iter_mut()
            .find(|m| m.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.status = MaskStatus::Pending;
        mask.error_text = Some(Str::ExplicitRecalcRequested.t().to_string());
        let queued = self.idle_queue.enqueue(
            IdleTask::MaskInference {
                mask_id: mask_id.to_string(),
            },
            100,
        );
        if queued.is_none() {
            return Err(GuiError::Io(Str::IdleQueueFull.t().to_string()));
        }
        Ok(())
    }

    /// The collective regeneration action of F-100 ("alle veralteten/fehlenden
    /// neu generieren").
    ///
    /// Runs exactly the per-module actions for the values that are stale or
    /// missing and skips everything fresh, so a fully fresh state is a no-op.
    /// It is only reachable from the explicit button — never implicit. Returns
    /// the module names that were regenerated (empty when nothing was stale).
    pub fn regenerate_stale(&mut self) -> Result<Vec<&'static str>, GuiError> {
        instrument_gui_action!(self, GuiAction::RegenerateStale);
        let mut regenerated: Vec<&'static str> = Vec::new();
        // Module `masks`: every non-`Valid` source mask is stale or missing.
        // Range masks are deterministic and always `Valid`; they are skipped
        // automatically by the status filter.
        let stale_masks: Vec<String> = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| {
                copy.mask_library
                    .iter()
                    .filter(|mask| matches!(mask.operation, MaskOperation::Source))
                    .filter(|mask| !matches!(mask.status, MaskStatus::Valid))
                    .map(|mask| mask.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        if !stale_masks.is_empty() {
            for mask_id in &stale_masks {
                self.mark_mask_pending(mask_id)?;
            }
            regenerated.push("masks");
        }
        // Module `auto-tone`: stale = enabled, but the full AUTO-TONE-2
        // six-mirror contract is not persisted (e.g. a stale fingerprint
        // cleared it, or a historic two-slider `process --auto-tone` artifact
        // is loaded). Mirrors are the marker of auto-written values, so an
        // incomplete set means regenerate.
        let auto_tone_stale = {
            let auto = &self.recipe.auto_features;
            auto.enable_auto_tone
                && (auto.auto_exposure.is_none()
                    || auto.auto_contrast.is_none()
                    || auto.auto_whites.is_none()
                    || auto.auto_blacks.is_none()
                    || auto.auto_highlights.is_none()
                    || auto.auto_shadows.is_none())
        };
        if auto_tone_stale {
            self.auto_tone()?;
            regenerated.push("auto-tone");
        }
        // Module `matching`: missing = enabled, but no persisted value.
        if self.recipe.auto_features.match_total_exposure
            && self.recipe.auto_features.matched_exposure.is_none()
        {
            self.match_total_exposure(0.5)?;
            regenerated.push("matching");
        }
        info!("GUI interaction: regenerate_stale -> {regenerated:?}");
        Ok(regenerated)
    }

    // ---- F-103-N4: interactive mask tools (Brush / Linear / Radial) ----

    /// Arm or disarm an interactive masking tool. Disarming returns the preview
    /// to its ordinary click/eyedropper behaviour and cancels any in-progress
    /// drag.
    ///
    /// R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): the former hard refusal while
    /// recipe geometry is active is replaced by the tool switch committing the
    /// active geometry draft (see [`Self::commit_outgoing_tool_for_switch`]).
    /// The callers route through that helper; this setter only arms.
    pub fn set_mask_tool(&mut self, tool: MaskTool) {
        instrument_gui_action!(self, GuiAction::SetMaskTool);
        self.mask_tool = tool;
        self.pending_brush_marks.clear();
        self.drag_start = None;
        self.drag_current = None;
        self.drawing = false;
    }

    /// G-14 (H1): arm the WB eyedropper and disarm the red-eye region picker.
    /// Both pickers consume the same preview click, so they are mutually
    /// exclusive — a single click must never sample a white balance *and* mark
    /// a pupil.
    fn arm_wb_picker(&mut self) {
        instrument_gui_action!(self, GuiAction::ArmWbEyedropper);
        self.wb_pick_mode = true;
        self.red_eye_pick_mode = false;
        info!("GUI interaction: white-balance pick mode armed");
    }

    /// G-14 (H1): arm/disarm the red-eye region picker. Arming disarms the WB
    /// eyedropper (see [`Self::arm_wb_picker`]).
    fn set_red_eye_pick_mode(&mut self, armed: bool) {
        instrument_gui_action!(self, GuiAction::SetRedEyePickMode);
        self.red_eye_pick_mode = armed;
        if armed {
            self.wb_pick_mode = false;
        }
        info!("GUI interaction: red-eye pick mode -> {armed}");
    }

    /// Disarm both preview pickers (image switch and `Esc`). The recipe and the
    /// persisted state are never touched.
    fn disarm_preview_pickers(&mut self) {
        self.wb_pick_mode = false;
        self.red_eye_pick_mode = false;
    }

    /// `Esc` cancels an armed WB eyedropper / red-eye region picker and an
    /// armed spot tool (F-103-N3 / G-14 / SPOT). The recipe stays untouched.
    fn cancel_armed_preview_tools(&mut self) {
        self.disarm_preview_pickers();
        self.spot_tool = SpotTool::None;
    }

    /// `Esc` key wiring: read the frame's input and cancel the armed preview
    /// tools. Extracted so a headless test can drive the real key event.
    fn handle_escape_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_armed_preview_tools();
        }
    }

    pub fn set_spot_tool(&mut self, tool: SpotTool) {
        instrument_gui_action!(self, GuiAction::SetSpotTool);
        self.spot_tool = tool;
        if tool != SpotTool::None {
            self.mask_tool = MaskTool::None;
        }
    }
    pub fn spot_tool(&self) -> SpotTool {
        self.spot_tool
    }
    pub fn set_spot_mode(&mut self, mode: SpotMode) {
        instrument_gui_action!(self, GuiAction::SetSpotMode);
        self.spot_mode = mode;
    }
    pub fn spot_mode(&self) -> SpotMode {
        self.spot_mode
    }
    pub fn commit_spot_heal(
        &mut self,
        center: lumina_sidecar::Point2,
        radius: f32,
        feather: f32,
        offset: lumina_sidecar::Point2,
        opacity: f32,
    ) -> Result<(), GuiError> {
        if !center.x.is_finite()
            || !center.y.is_finite()
            || !(0.0..=1.0).contains(&center.x)
            || !(0.0..=1.0).contains(&center.y)
        {
            return Err(GuiError::Io("Spot center must be 0..=1".into()));
        }
        if !radius.is_finite() || !(1.0..=512.0).contains(&radius) {
            return Err(GuiError::Io("Spot radius must be 1..=512".into()));
        }
        if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
            return Err(GuiError::Io("Spot feather must be 0..=1".into()));
        }
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(GuiError::Io("Spot opacity must be 0..=1".into()));
        }
        let id = format!(
            "spot-{}",
            blake3::hash(format!("{:.6},{:.6},{:.2}", center.x, center.y, radius).as_bytes())
                .to_hex()
        );
        let spot = serde_json::json!({"id": id, "version": 1, "mode": "heuristic", "center_x": center.x, "center_y": center.y, "radius": radius, "feather": feather, "offset_dx": offset.x, "offset_dy": offset.y, "opacity": opacity, "status": "valid"});
        let mut spots: Vec<serde_json::Value> = self
            .recipe
            .extras
            .get("spot_removals")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        spots.push(spot);
        self.recipe
            .extras
            .insert("spot_removals".into(), serde_json::to_value(spots).unwrap());
        self.mark_dirty();
        self.save_sidecar();
        // GEN-ONNX-1 Welle 2b (F4/F7): never swallow a render error — surface
        // it loudly via the visible error dialog (the spot itself is already
        // persisted; the render failure must not be discarded).
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        Ok(())
    }
    pub fn clear_spot_heals(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearSpotHeals);
        self.recipe.extras.remove("spot_removals");
        self.mark_dirty();
        self.save_sidecar();
        // F4/F7: surface the render result instead of discarding it.
        if let Err(error) = self.render() {
            self.show_error(error);
        }
    }

    // ---- LRPAR-G04-REMOVE (G-04 Remove-Parität) ---------------------------

    /// Recipe-backed visualize threshold (`None` = off). Read-only accessor
    /// for the panel slider and headless tests.
    pub fn spot_visualize_threshold(&self) -> Option<f32> {
        self.recipe.spot_visualize_threshold()
    }

    /// Set (`Some(0..=1)`) or clear (`None`) the visualize threshold (G-04).
    /// Recipe-backed: persists through the debounced slider-save path
    /// ([`Self::commit_pending_slider_save`], `info!`-logged), so headless
    /// tests drive it without a timer. Loud on out-of-range values.
    pub fn set_spot_visualize(&mut self, threshold: Option<f32>) -> Result<(), GuiError> {
        if let Some(t) = threshold {
            if !t.is_finite() || !(0.0..=1.0).contains(&t) {
                return Err(GuiError::Io("Visualize threshold must be 0..=1".into()));
            }
        }
        self.recipe
            .set_spot_visualize_threshold(threshold)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        match threshold {
            Some(t) => self.mark_recipe_dirty("spot.visualize", f64::from(t)),
            None => self.mark_recipe_dirty("spot.visualize", -1.0),
        }
        info!("GUI interaction: set_spot_visualize -> {threshold:?}");
        Ok(())
    }

    /// GUI-INSTRDBG-17b-REST: the "Visualize off" button command. Instrumented
    /// separately from [`Self::set_spot_visualize`] because the visualize
    /// *slider* commits through the same setter and a slider drag is not a
    /// button action (same split as `save_recipe_action`).
    fn clear_spot_visualize(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::ClearSpotVisualize);
        self.set_spot_visualize(None)
    }

    /// Active visualize threshold for the preview gate (G04-FOLLOWUP-1):
    /// the recipe threshold ANDed with the G-11 overlay mode (`Always` = on
    /// as soon as the threshold is set, `Never` = off, `Auto` = only while a
    /// mask/spot tool is armed or a drag runs). Single gate for `render_from`
    /// (preview only — render/export/CLI stay untinted); headless-testable
    /// without pixels.
    pub fn spot_visualize_overlay_threshold(&self) -> Option<f32> {
        let threshold = self.recipe.spot_visualize_threshold()?;
        if self.overlay_visible() {
            Some(threshold)
        } else {
            None
        }
    }

    /// Effective heuristic detect threshold (G04-FOLLOWUP-1): the recipe
    /// visualize threshold when set, else the session input (default 0.5).
    /// The recipe value wins while set (it is persisted user intent); the
    /// session slider — synced from the recipe on load — applies after
    /// Clear or when no recipe value was ever set.
    pub fn spot_detect_effective_threshold(&self) -> f32 {
        self.recipe
            .spot_visualize_threshold()
            .unwrap_or(self.spot_detect_threshold)
    }

    /// Detection threshold input for heuristic Detect-Objects (`0..=1`).
    /// Session display state (never recipe); loud on bad values.
    pub fn set_spot_detect_threshold(&mut self, threshold: f32) -> Result<(), GuiError> {
        if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
            return Err(GuiError::Io("Detect threshold must be 0..=1".into()));
        }
        self.spot_detect_threshold = threshold;
        info!("GUI interaction: set_spot_detect_threshold -> {threshold}");
        Ok(())
    }

    /// Heuristic Detect-Objects (G-04, stage 1, no model): lists candidates on
    /// the loaded frame without persisting anything. The outcome text lands in
    /// `spot_detect_status` (visible, never silent); the candidates are
    /// returned for an explicit [`Self::apply_detected_spots`].
    pub fn detect_spot_candidates(&mut self) -> Result<Vec<DetectedSpot>, GuiError> {
        instrument_gui_action!(self, GuiAction::DetectSpotCandidates);
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        // G04-FOLLOWUP-1: the recipe visualize threshold is the default
        // when set (else the session input); the used value lands in the
        // status line so it is never silent.
        let threshold = self.spot_detect_effective_threshold();
        let candidates = detect_spots_heuristic(&frame, threshold, 32)?;
        info!(
            "GUI interaction: detect_spot_candidates -> {} candidate(s) at threshold {}",
            candidates.len(),
            threshold
        );
        self.spot_detect_status = format!(
            "Detected {} candidate(s) at threshold {:.2} (not applied — use Apply)",
            candidates.len(),
            threshold
        );
        Ok(candidates)
    }

    /// Persist detected candidates as heuristic spots (G-04, explicit only):
    /// one atomic recipe update + save + render. Never called implicitly —
    /// the panel wires it to an "Apply detected" button, `auto` modes only
    /// list.
    pub fn apply_detected_spots(&mut self, candidates: &[DetectedSpot]) -> Result<usize, GuiError> {
        if candidates.is_empty() {
            self.spot_detect_status = "No candidates to apply".into();
            return Ok(0);
        }
        let mut spots: Vec<serde_json::Value> = self
            .recipe
            .extras
            .get("spot_removals")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        for candidate in candidates {
            if !candidate.x.is_finite()
                || !candidate.y.is_finite()
                || !(0.0..=1.0).contains(&candidate.x)
                || !(0.0..=1.0).contains(&candidate.y)
            {
                return Err(GuiError::Io(
                    "Detected candidate has invalid coordinates".into(),
                ));
            }
            let id = format!(
                "spot-{}",
                blake3::hash(
                    format!(
                        "{:.6},{:.6},{:.2}",
                        candidate.x, candidate.y, candidate.radius
                    )
                    .as_bytes()
                )
                .to_hex()
            );
            spots.push(serde_json::json!({
                "id": id, "version": 1, "mode": "heuristic",
                "center_x": candidate.x, "center_y": candidate.y,
                "radius": candidate.radius.clamp(1.0, 512.0),
                "feather": 0.0, "offset_dx": 0.05, "offset_dy": 0.0,
                "opacity": 1.0, "status": "valid",
            }));
        }
        let applied = candidates.len();
        self.recipe
            .extras
            .insert("spot_removals".into(), serde_json::to_value(spots).unwrap());
        self.mark_dirty();
        self.save_sidecar();
        // F4/F7: no swallowed render error on the spot-apply path — surface it
        // visibly (the applied spots are already persisted).
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: apply_detected_spots -> {applied} spot(s)");
        self.spot_detect_status = format!("Applied {applied} detected spot(s)");
        Ok(applied)
    }

    /// GUI-INSTRDBG-17b-REST: the "Apply detected" button command. One outer
    /// instrumented action around the detector + persistence: the nested
    /// `detect_spot_candidates` is part of this action and must not add a
    /// second log line (the depth guard suppresses it).
    fn apply_detected_spot_objects(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyDetectedSpots);
        let candidates = self.detect_spot_candidates()?;
        self.apply_detected_spots(&candidates)
    }

    /// Recipe-backed distraction switches (G-04). `auto_mode` only lists —
    /// applying stays explicit via [`Self::apply_detected_spots`] (the panel
    /// Apply button); enabling `auto` never persists spots by itself.
    pub fn spot_distraction(&self) -> SpotDistraction {
        self.recipe.spot_distraction()
    }

    pub fn set_spot_distraction(&mut self, setting: SpotDistraction) {
        instrument_gui_action!(self, GuiAction::SetSpotDistraction);
        self.recipe.set_spot_distraction(setting);
        self.mark_recipe_dirty(
            "spot.distraction",
            f64::from(setting.reflections as u8)
                + 2.0 * f64::from(setting.people as u8)
                + 4.0 * f64::from(setting.dust as u8)
                + 8.0 * f64::from(setting.auto_mode as u8),
        );
        info!("GUI interaction: set_spot_distraction -> {setting:?}");
    }

    /// Visible distraction status (G-04): per-kind outcome of
    /// [`distraction_candidates`] on the loaded frame — heuristic dust
    /// candidates or an explicit `needs model (F-078 gate)` marker for
    /// reflections/people. Never a silent substitute.
    pub fn distraction_status(&self) -> Vec<(String, String)> {
        let Some(frame) = self.original.as_ref() else {
            return vec![("none".into(), "no image loaded".into())];
        };
        let setting = self.recipe.spot_distraction();
        let core_setting = DistractionSetting {
            reflections: setting.reflections,
            people: setting.people,
            dust: setting.dust,
            auto_mode: setting.auto_mode,
        };
        let threshold = self.spot_detect_effective_threshold();
        let Ok(outcomes) = distraction_candidates(frame, core_setting, threshold, 32) else {
            return vec![("error".into(), "invalid threshold".into())];
        };
        outcomes
            .into_iter()
            .map(|(kind, status)| {
                let name = match kind {
                    DistractionKind::Reflections => "reflections",
                    DistractionKind::People => "people",
                    DistractionKind::Dust => "dust",
                }
                .to_string();
                let text = match status {
                    DistractionStatus::Ready(spots) => format!("{} candidate(s)", spots.len()),
                    DistractionStatus::NeedsModel { reason, .. } => {
                        format!("needs model (F-078 gate): {reason}")
                    }
                };
                (name, text)
            })
            .collect()
    }

    /// Session inputs for generative variant regeneration (G-04). Display
    /// state (never recipe until Regenerate); loud on empty prompts.
    pub fn set_spot_gen_inputs(&mut self, prompt: String, seed: u64, variant: u64) {
        self.spot_gen_prompt = prompt;
        self.spot_gen_seed = seed;
        self.spot_gen_variant = variant;
        info!("GUI interaction: set_spot_gen_inputs seed={seed} variant={variant}");
    }

    /// Regenerate a generative spot variant (G-04, explicit only): sets
    /// `seed = variant_seed(base, variant)` (+ `variant`, `prompt`) on the
    /// named extras entry, then saves. Heuristic entries and unknown ids fail
    /// loudly — a variant never silently retargets another spot.
    pub fn regenerate_spot_variant(&mut self, spot_id: &str) -> Result<u64, GuiError> {
        instrument_gui_action!(self, GuiAction::RegenerateSpotVariant);
        let derived = generative_variant_seed(self.spot_gen_seed, self.spot_gen_variant);
        let mut spots: Vec<serde_json::Value> = self
            .recipe
            .extras
            .get("spot_removals")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let mut found = false;
        for entry in &mut spots {
            let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            if id != spot_id {
                continue;
            }
            let mode = entry
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("heuristic");
            if mode != "generative" {
                return Err(GuiError::Io(format!(
                    "Spot `{spot_id}` is not generative (mode `{mode}`)"
                )));
            }
            entry["seed"] = serde_json::json!(derived);
            entry["variant"] = serde_json::json!(self.spot_gen_variant);
            entry["base_seed"] = serde_json::json!(self.spot_gen_seed);
            if !self.spot_gen_prompt.is_empty() {
                entry["prompt"] = serde_json::json!(self.spot_gen_prompt);
            }
            found = true;
        }
        if !found {
            return Err(GuiError::Io(format!("Unknown spot `{spot_id}`")));
        }
        self.recipe
            .extras
            .insert("spot_removals".into(), serde_json::to_value(spots).unwrap());
        self.mark_dirty();
        self.save_sidecar();
        // F4/F7: no swallowed render error on the spot-variant path — surface
        // it visibly (the regenerated seed is already persisted).
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: regenerate_spot_variant {spot_id} -> seed {derived}");
        self.spot_gen_status = format!(
            "Regenerated `{spot_id}` variant {} (seed {derived})",
            self.spot_gen_variant
        );
        Ok(derived)
    }

    /// Set the normalized brush radius. Rejected (no state change) if not finite
    /// or outside the open-closed `(0, 1]` range.
    pub fn set_brush_radius(&mut self, radius: f32) -> Result<(), GuiError> {
        if !radius.is_finite() || !(0.0..=1.0).contains(&radius) || radius <= 0.0 {
            return Err(GuiError::Io(
                "Brush radius must be finite and within (0, 1]".into(),
            ));
        }
        self.brush_radius = radius;
        // GUI-SLIDER-SAVE-1: the brush-size slider arms a save commit like any
        // other slider (the radius itself is tool session state; the commit
        // persists the recipe loudly instead of dropping it).
        self.mark_recipe_dirty("mask.brush_radius", f64::from(radius));
        Ok(())
    }

    /// Toggle the brush eraser (negative) sign.
    pub fn set_brush_eraser(&mut self, eraser: bool) {
        self.brush_eraser = eraser;
    }

    /// Set the spot-heal radius tool default (GUI-SLIDER-SAVE-1). Tool-only
    /// session state (not recipe): still records a save commit so the
    /// debounced path persists loudly instead of dropping concurrent edits.
    /// Visible in the spot-heal panel.
    pub fn set_spot_radius(&mut self, radius: f32) {
        trace!("GUI interaction: set_spot_radius {}", radius);
        self.spot_radius = radius;
        self.mark_recipe_dirty("spot.radius", f64::from(radius));
    }

    /// Set the spot-heal feather tool default (GUI-SLIDER-SAVE-1, see
    /// [`Self::set_spot_radius`]).
    pub fn set_spot_feather(&mut self, feather: f32) {
        trace!("GUI interaction: set_spot_feather {}", feather);
        self.spot_feather = feather;
        self.mark_recipe_dirty("spot.feather", f64::from(feather));
    }

    /// Set the spot-heal opacity tool default (GUI-SLIDER-SAVE-1, see
    /// [`Self::set_spot_radius`]).
    pub fn set_spot_opacity(&mut self, opacity: f32) {
        trace!("GUI interaction: set_spot_opacity {}", opacity);
        self.spot_opacity = opacity;
        self.mark_recipe_dirty("spot.opacity", f64::from(opacity));
    }

    /// Set the blur of the selected mask layer (0..=1).
    pub fn set_mask_blur(&mut self, blur: f32) -> Result<(), GuiError> {
        if !blur.is_finite() || !(0.0..=1.0).contains(&blur) {
            return Err(GuiError::Io("Blur must be between 0 and 1".into()));
        }
        self.active_layer_mut()?.blur = blur;
        // REVIEW-GUI-MASKRENDER-1: see `set_mask_inverted`. GUI-SLIDER-SAVE-1:
        // the blur slider commits like any other slider.
        self.mark_recipe_dirty("mask.blur", f64::from(blur));
        Ok(())
    }

    /// Set the density of the selected mask layer (0..=1).
    pub fn set_mask_density(&mut self, density: f32) -> Result<(), GuiError> {
        if !density.is_finite() || !(0.0..=1.0).contains(&density) {
            return Err(GuiError::Io("Density must be between 0 and 1".into()));
        }
        self.active_layer_mut()?.density = density;
        // REVIEW-GUI-MASKRENDER-1: see `set_mask_inverted`. GUI-SLIDER-SAVE-1:
        // the density slider commits like any other slider.
        self.mark_recipe_dirty("mask.density", f64::from(density));
        Ok(())
    }

    pub fn set_expand_beyond_image(&mut self, expand: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetExpandBeyondImage);
        let mut ge = self
            .recipe
            .generative_edit
            .clone()
            .unwrap_or(GenerativeEdit {
                version: 1,
                canvas: None,
                artifact: None,
                keep_generative_content: None,
                auto_fill_transparent: None,
                expand_beyond_image: None,
                seed: None,
                prompt: None,
                extras: Default::default(),
            });
        ge.expand_beyond_image = Some(expand);
        if !expand {
            ge.canvas = None;
        } else if ge.canvas.is_none() {
            let (w, h) = self
                .original
                .as_ref()
                .map(|f| (f.width, f.height))
                .unwrap_or((8, 8));
            ge.canvas = Some(GenerativeCanvas {
                output_width: w + 4,
                output_height: h + 4,
                source_offset_x: 2,
                source_offset_y: 2,
                extras: Default::default(),
            });
        }
        let mut tmp_recipe = self.recipe.clone();
        tmp_recipe.generative_edit = Some(ge.clone());
        let mut doc = lumina_sidecar::SidecarDocument::new(
            lumina_sidecar::SourceIdentity {
                relative_name: "x".into(),
                content_hash: "h".into(),
                byte_length: 1,
                modified_at: None,
                raw_format: "PNG".into(),
                orientation: 1,
                decode_fingerprint: lumina_sidecar::DecodeFingerprint {
                    decoder: "d".into(),
                    version: "1".into(),
                    parameters: Default::default(),
                    extras: Default::default(),
                },
                geometry_fingerprint: lumina_sidecar::GeometryFingerprint {
                    width: 1,
                    height: 1,
                    orientation: 1,
                    pixel_aspect_ratio: 1.0,
                    extras: Default::default(),
                },
                extras: Default::default(),
            },
            "p",
        );
        doc.virtual_copies[0].recipe = tmp_recipe.clone();
        doc.validate().map_err(|e| GuiError::Io(e.to_string()))?;
        if expand {
            if let Some(canvas) = &ge.canvas {
                if let Some(frame) = &self.original {
                    canvas
                        .validate_with_source(frame.width, frame.height)
                        .map_err(|e| GuiError::Io(e.to_string()))?;
                }
            }
        }
        self.recipe.generative_edit = Some(ge);
        self.mark_dirty();
        {
            if self.document.is_some() {
                self.save_sidecar();
            }
        }
        info!("GUI interaction: set_expand_beyond_image -> {expand}");
        if self.original.is_some() {
            // GEN-ONNX-1 Welle 2b (F4): propagate the render result instead of
            // swallowing it. An active `expand_beyond_image` without a matching
            // canvas artifact is a loud render error and must reach the caller
            // (the panel calls `show_error`); a silent `let _ =` used to hide
            // exactly the "generative stage unavailable" failure.
            self.render()?;
        }
        Ok(())
    }

    pub fn set_expand_canvas(&mut self, canvas: GenerativeCanvas) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetExpandCanvas);
        let mut ge = self
            .recipe
            .generative_edit
            .clone()
            .unwrap_or(GenerativeEdit {
                version: 1,
                canvas: None,
                artifact: None,
                keep_generative_content: None,
                auto_fill_transparent: None,
                expand_beyond_image: Some(true),
                seed: None,
                prompt: None,
                extras: Default::default(),
            });
        if let Some(frame) = &self.original {
            canvas
                .validate_with_source(frame.width, frame.height)
                .map_err(|e| GuiError::Io(e.to_string()))?;
        } else {
            canvas.validate().map_err(|e| GuiError::Io(e.to_string()))?;
        }
        ge.expand_beyond_image = Some(true);
        let (out_width, out_height) = (canvas.output_width, canvas.output_height);
        ge.canvas = Some(canvas);
        self.recipe.generative_edit = Some(ge);
        self.mark_dirty();
        {
            if self.document.is_some() {
                self.save_sidecar();
            }
        }
        info!("GUI interaction: set_expand_canvas -> {out_width}x{out_height}");
        if self.original.is_some() {
            // GEN-ONNX-1 Welle 2b (F4): loud render result, never swallowed.
            self.render()?;
        }
        Ok(())
    }

    // ---- GEN-ONNX-1 Welle 2b: generative canvas hook (preview/export/GPU) ----
    //
    // GPU decision (F3): the GUI has **no GPU readback render path**. Preview
    // and export render on the CPU artifact-aware path
    // (`render_frame_from_base_with_generative` / `export_image_with_generative`,
    // both wrapping the same shared core pipeline), while the GPU is used only
    // for the readback-free VRAM **present** (tone stage) via `render_to_vram`.
    // That present path is artifact-blind by design — `lumina-gpu` Welle 2a
    // documents that there is no VRAM generative injection point without a
    // readback — so the GUI deliberately does **not** call
    // `GpuContext::render_with_gpu_and_generative`: no GUI render site consumes
    // its readback frame, and adding one would only introduce a device→host copy
    // without changing the presented pixels. The refusal is classified
    // (`classify_vram_refusal`) into a visible routing badge, so the CPU route is
    // never silent. A future VRAM generative present path would replace this
    // refusal with an artifact-aware VRAM entry; until then CPU is the complete
    // reference (Agents.md: GPU is acceleration only, never a requirement).

    /// GEN-ONNX-1 Welle 2b: enable/disable the auto-fill-transparent role
    /// (fills transparent pixels after Lens correction). The role flag alone
    /// changes no geometry; the render is loud while no canvas artifact exists
    /// (press "Generate"), and an opaque post-lens frame needs no artifact
    /// (caller convention). Persisted like every other recipe edit.
    pub fn set_auto_fill_transparent(&mut self, auto_fill: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetAutoFillTransparent);
        let mut ge = self
            .recipe
            .generative_edit
            .clone()
            .unwrap_or(GenerativeEdit {
                version: 1,
                canvas: None,
                artifact: None,
                keep_generative_content: None,
                auto_fill_transparent: None,
                expand_beyond_image: None,
                seed: None,
                prompt: None,
                extras: Default::default(),
            });
        ge.auto_fill_transparent = Some(auto_fill);
        self.recipe.generative_edit = Some(ge);
        self.mark_dirty();
        if self.document.is_some() {
            self.save_sidecar();
        }
        info!("GUI interaction: set_auto_fill_transparent -> {auto_fill}");
        if self.original.is_some() {
            // GEN-ONNX-1 Welle 2b (F4): a failed render (e.g. an active
            // auto-fill without a canvas artifact on a transparent frame) must
            // surface, never be swallowed.
            self.render()?;
        }
        Ok(())
    }

    /// Whether the active recipe carries an active generative role
    /// (`auto_fill_transparent` or `expand_beyond_image`).
    fn generative_stage_active(&self) -> bool {
        self.recipe.generative_edit.as_ref().is_some_and(|edit| {
            edit.effective_expand() || edit.auto_fill_transparent.unwrap_or(false)
        })
    }

    /// User-visible per-role status of the active generative edit: the last
    /// resolver outcome mirroring the sidecar vocabulary
    /// `valid`/`stale`/`missing`/`corrupt` (SOLL
    /// `feature/product/generative-expand.md`). Display only — the render itself
    /// still verifies the identity digest and is loud when a canvas is absent or
    /// stale.
    fn generative_status_text(&self) -> String {
        let Some(edit) = self.recipe.generative_edit.as_ref() else {
            return String::new();
        };
        let mut roles = Vec::new();
        if edit.effective_expand() {
            roles.push(OnnxGenerativeRole::Expand);
        }
        if edit.auto_fill_transparent.unwrap_or(false) {
            roles.push(OnnxGenerativeRole::AutoFillTransparent);
        }
        if roles.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = roles
            .iter()
            .map(|role| {
                format!(
                    "{role:?}: {}",
                    self.generative_role_status[generative_role_index(*role)].label()
                )
            })
            .collect();
        format!("Generative canvas — {}", parts.join(", "))
    }

    /// The prompt/model identity of a persisted generative edit for one role.
    ///
    /// Mirrors the CLI's `generative_identity`: the fixture manifest supplies
    /// the real, pinned `model_hash`, and the persisted prompt/negative prompt
    /// are the exact producer inputs. No guessing from model names.
    fn generative_identity_for(
        role: OnnxGenerativeRole,
        edit: &GenerativeEdit,
    ) -> GenerativeIdentity {
        GenerativeIdentity {
            model_hash: fixture_manifest(role).model_hash,
            prompt: edit.prompt.clone().unwrap_or_default(),
            negative_prompt: edit.negative_prompt().map(str::to_owned),
        }
    }

    /// Produce one role's composited canvas with the deterministic fixture
    /// model. The persisted edit may carry both roles; `produce_canvas` reads a
    /// single role from the flags, so a role-scoped copy is passed — the other
    /// role's flag never silently changes which canvas is produced.
    fn produce_generative_canvas(
        input_frame: &ImageFrame,
        edit: &GenerativeEdit,
        role: OnnxGenerativeRole,
    ) -> Result<lumina_onnx::GenerativeCanvasOutput, GuiError> {
        let mut scoped = edit.clone();
        match role {
            OnnxGenerativeRole::Expand => {
                scoped.expand_beyond_image = Some(true);
                scoped.auto_fill_transparent = Some(false);
            }
            OnnxGenerativeRole::AutoFillTransparent => {
                scoped.expand_beyond_image = Some(false);
                scoped.auto_fill_transparent = Some(true);
            }
        }
        let model = GenerativeModelSource::Fixture(role);
        produce_canvas(input_frame, &scoped, &model)
            .map_err(|error| GuiError::Io(format!("generative {role:?} canvas failed: {error}")))
    }

    /// Persist one produced canvas into the sidecar `.lumina.zdata` bundle and
    /// return its portable recipe link. Same record id, checksum and identity
    /// digest as the CLI (`generative_record_id`, `with_identity`), so GUI and
    /// CLI address the same record. `replace = true` is the explicit
    /// regeneration path; records of the other generative role are preserved.
    fn persist_generative_canvas(
        zdata_path: &Path,
        relative_path: &str,
        output: &lumina_onnx::GenerativeCanvasOutput,
    ) -> Result<GenerativeArtifactRef, GuiError> {
        let record = SidecarGenerativeCanvas {
            id: generative_record_id(&output.identity_digest),
            width: output.width,
            height: output.height,
            pixels: output.pixels.clone(),
        };
        save_generative_canvas(zdata_path, record.clone(), true).map_err(|error| {
            GuiError::Io(format!(
                "could not write generative canvas bundle `{}`: {error}",
                zdata_path.display()
            ))
        })?;
        Ok(GenerativeArtifactRef::from_generative_canvas(
            &record,
            relative_path,
            output.identity_digest.clone(),
        ))
    }

    /// The frame entering the expand role when the auto-fill role produced a
    /// canvas: the auto-filled frame with the perspective stage applied,
    /// mirroring the render order `Lens → auto-fill → Perspective → expand`.
    /// Without an applied auto-fill canvas this is exactly `after_perspective`.
    ///
    /// Only `lumina-core`'s own public stage function is used — the GUI owns no
    /// image math and never re-implements the pipeline.
    fn generative_expand_input(
        after_perspective: &ImageFrame,
        auto_fill_frame: Option<&ImageFrame>,
        lens: Option<&lumina_sidecar::LensCorrection>,
        perspective: Option<&lumina_sidecar::Perspective>,
        lensfun: Option<lumina_core::LensfunCorrectorRef<'_>>,
    ) -> Result<ImageFrame, GuiError> {
        let Some(filled) = auto_fill_frame else {
            return Ok(after_perspective.clone());
        };
        let mut frame = filled.clone();
        #[cfg(feature = "lensfun")]
        frame.apply_perspective_stage(lens, perspective, lensfun.map(|reference| reference.0))?;
        #[cfg(not(feature = "lensfun"))]
        {
            let _ = lensfun;
            frame.apply_perspective_stage(lens, perspective)?;
        }
        Ok(frame)
    }

    /// Cheap memo key of the state the resolved generative canvases depend on:
    /// resolved source hash + recipe digest. The per-role identity digest is
    /// still compared on every resolve, so this only skips recomputation — it
    /// never serves a stale canvas.
    fn generative_memo_key(&mut self) -> String {
        let source_hash = self.resolved_source_hash();
        let recipe = serde_json::to_vec(&self.recipe).unwrap_or_default();
        format!("{source_hash}|blake3:{}", blake3::hash(&recipe).to_hex())
    }

    /// GEN-ONNX-1 Welle 2b: the explicit GUI "Generieren" action.
    ///
    /// Produces the deterministic fixture canvas for every active generative
    /// role, persists it into the sidecar `.lumina.zdata` bundle (durable
    /// wiring — same identity digest and record id as the CLI), links it in the
    /// recipe and renders. Neither role active, or a persisted double role, is
    /// handled without any silent fallback:
    ///
    /// * `auto_fill_transparent` with no transparent pixels after lens needs no
    ///   artifact (caller convention) — it is skipped and logged, not faked.
    /// * A record carrying both roles produces both canvases; the canvas-
    ///   defining `expand` link is the one persisted in the single recipe
    ///   `artifact` field (the auto-fill record stays addressable by its
    ///   deterministic bundle id).
    pub fn generate_generative_canvas(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::GenerateCanvas);
        let frame = self
            .original
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        if self.path.trim().is_empty() {
            return Err(GuiError::Io(
                "Generative generation needs a local file path (the canvas is persisted in the \
                 sidecar bundle)"
                    .into(),
            ));
        }
        let mut edit = self.recipe.generative_edit.clone().ok_or_else(|| {
            GuiError::Io("No generative edit in the recipe; enable the Expand mode first".into())
        })?;
        let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
        let expand_active = edit.effective_expand();
        if !auto_fill_active && !expand_active {
            return Err(GuiError::Io(
                "Generative generation requested but neither `auto_fill_transparent` nor \
                 `expand_beyond_image` is active (no silent canvas)"
                    .into(),
            ));
        }
        // The frames entering each role, computed exactly like the render does
        // (same corrector, same white balance, same empty source actions).
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(frame.width, frame.height);
        #[cfg(feature = "lensfun")]
        let lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let lensfun = None;
        let (after_lens, after_perspective) = generative_input_frames(
            &frame,
            &self.recipe,
            self.camera_white_balance,
            &[],
            lensfun,
        )?;
        let zdata_path = zdata_path_for(Path::new(self.path.trim()));
        let relative_path = zdata_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "generative.zdata".into());

        let mut artifacts = GenerativeArtifacts::default();
        let mut auto_fill_link: Option<GenerativeArtifactRef> = None;
        let mut expand_link: Option<GenerativeArtifactRef> = None;
        // Auto-fill first: its transparent mask is derived before the expand.
        let mut auto_fill_frame: Option<ImageFrame> = None;
        if auto_fill_active {
            if !has_transparent_pixels(&after_lens) {
                // Caller convention (normative): no transparent pixels after
                // lens → `auto_fill = None` (identity), no artifact required.
                info!(
                    "generative: auto_fill active but no transparent pixels after lens; \
                     no auto-fill canvas produced (identity)"
                );
            } else {
                let produced = Self::produce_generative_canvas(
                    &after_lens,
                    &edit,
                    OnnxGenerativeRole::AutoFillTransparent,
                )?;
                auto_fill_link = Some(Self::persist_generative_canvas(
                    &zdata_path,
                    &relative_path,
                    &produced,
                )?);
                let frame = produced.to_frame().map_err(|error| {
                    GuiError::Io(format!("generative auto-fill frame invalid: {error}"))
                })?;
                auto_fill_frame = Some(frame.clone());
                artifacts.set(
                    OnnxGenerativeRole::AutoFillTransparent,
                    CachedGenerativeCanvas {
                        identity: produced.identity_digest.clone(),
                        artifact: GenerativeCanvasArtifact::new(
                            lumina_core::GenerativeRole::AutoFillTransparent,
                            frame,
                        ),
                    },
                );
                info!(
                    "generative: auto-fill canvas produced {}x{} identity={}",
                    produced.width, produced.height, produced.identity_digest
                );
            }
        }
        if expand_active {
            // SOLL order `Lens → auto-fill → Perspective → expand`: when the
            // auto-fill role produced a canvas, the expand canvas is built from
            // that composited frame with the perspective stage applied (the
            // core's own public stage), so transparent pixels filled by the
            // auto-fill survive the authoritative expand canvas.
            let expand_input = Self::generative_expand_input(
                &after_perspective,
                auto_fill_frame.as_ref(),
                self.recipe.lens_correction.as_ref(),
                self.recipe.effective_perspective().as_ref(),
                lensfun,
            )?;
            let produced =
                Self::produce_generative_canvas(&expand_input, &edit, OnnxGenerativeRole::Expand)?;
            expand_link = Some(Self::persist_generative_canvas(
                &zdata_path,
                &relative_path,
                &produced,
            )?);
            let frame = produced.to_frame().map_err(|error| {
                GuiError::Io(format!("generative expand frame invalid: {error}"))
            })?;
            artifacts.set(
                OnnxGenerativeRole::Expand,
                CachedGenerativeCanvas {
                    identity: produced.identity_digest.clone(),
                    artifact: GenerativeCanvasArtifact::new(
                        lumina_core::GenerativeRole::Expand,
                        frame,
                    ),
                },
            );
            info!(
                "generative: expand canvas produced {}x{} identity={}",
                produced.width, produced.height, produced.identity_digest
            );
        }
        // The single recipe link is the canvas-defining expand role when both
        // are active (SOLL: Lens → GenerativeEdit → Perspective → Crop); the
        // auto-fill record stays addressable by its deterministic bundle id.
        edit.artifact = expand_link.or(auto_fill_link);
        self.recipe.generative_edit = Some(edit);
        self.generative_artifacts = artifacts;
        self.mark_dirty();
        // Persist the recipe link durably: a byte-drop session may not have a
        // document yet, so it is created from the source identity first (the
        // `.lumina.zdata` bundle was already written above).
        self.ensure_document_loaded()?;
        self.save_sidecar();
        // Render now so the produced canvas is visible; the resolver finds the
        // freshly installed session artifacts (identity match) and does not
        // touch the disk again.
        self.render()?;
        Ok(())
    }

    /// Resolve the generative canvases for one render from the full-resolution
    /// source. Every active role is resolved either from the in-memory session
    /// store (freshly generated / previously resolved) or from the persisted
    /// `.lumina.zdata` bundle by its deterministic identity id. An active role
    /// without a matching canvas is a loud error — never a silent unexpanded
    /// render. The auto-fill caller convention is honoured: a post-lens frame
    /// without transparent pixels resolves `auto_fill = None` (identity, no
    /// artifact required).
    fn resolve_generative_artifacts(
        &mut self,
        full_source: &ImageFrame,
    ) -> Result<GenerativeArtifacts, GuiError> {
        let Some(edit) = self.recipe.generative_edit.clone() else {
            return Ok(GenerativeArtifacts::default());
        };
        let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
        let expand_active = edit.effective_expand();
        if !auto_fill_active && !expand_active {
            return Ok(GenerativeArtifacts::default());
        }
        // The input frames (and therefore the identity digests) are expensive
        // to rebuild; reuse them while neither source nor recipe changed.
        let memo_key = self.generative_memo_key();
        if self.generative_memo.as_deref() == Some(memo_key.as_str()) {
            return Ok(self.generative_artifacts.clone());
        }
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(full_source.width, full_source.height);
        #[cfg(feature = "lensfun")]
        let lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let lensfun = None;
        let (after_lens, after_perspective) = generative_input_frames(
            full_source,
            &self.recipe,
            self.camera_white_balance,
            &[],
            lensfun,
        )?;
        let seed = edit.seed.unwrap_or(0);
        // Per-role status is collected locally and committed to `self` only
        // after `lensfun` (a shared borrow of `self`) is released, so the loud
        // error path also records the precise `valid`/`stale`/`missing`/
        // `corrupt` verdict the panel shows.
        let mut role_status = [GenerativeRoleStatus::Missing; 2];
        let mut resolved = GenerativeArtifacts::default();
        let mut deferred_error: Option<GuiError> = None;
        let mut auto_fill_frame: Option<ImageFrame> = None;
        if auto_fill_active {
            if has_transparent_pixels(&after_lens) {
                let identity =
                    Self::generative_identity_for(OnnxGenerativeRole::AutoFillTransparent, &edit);
                let digest = GenerativeCacheKey::auto_fill(&after_lens, seed, &identity).digest();
                let (status, result) =
                    self.resolve_generative_role(OnnxGenerativeRole::AutoFillTransparent, &digest);
                role_status[generative_role_index(OnnxGenerativeRole::AutoFillTransparent)] =
                    status;
                match result {
                    Ok(cached) => {
                        auto_fill_frame = Some(cached.artifact.frame.clone());
                        resolved.auto_fill = Some(cached);
                    }
                    Err(error) => deferred_error = Some(error),
                }
            } else {
                // Caller convention (normative): no transparent pixels after
                // lens → `auto_fill = None` (identity) and the role is valid
                // without an artifact.
                role_status[generative_role_index(OnnxGenerativeRole::AutoFillTransparent)] =
                    GenerativeRoleStatus::Valid;
            }
        }
        if expand_active && deferred_error.is_none() {
            let canvas = match edit.canvas.clone() {
                Some(canvas) => Some(canvas),
                None => {
                    deferred_error = Some(GuiError::Io(
                        "`expand_beyond_image` requires a `canvas` (output_* + offsets)".into(),
                    ));
                    None
                }
            };
            if let Some(canvas) = canvas {
                // Mirror the render order `Lens → auto-fill → Perspective →
                // expand` so the expand identity matches the canvas the producer
                // built (the expand canvas is authoritative and must embed the
                // auto-filled pixels). Without an applied auto-fill canvas the
                // frame entering expand is exactly `after_perspective`.
                match Self::generative_expand_input(
                    &after_perspective,
                    auto_fill_frame.as_ref(),
                    self.recipe.lens_correction.as_ref(),
                    self.recipe.effective_perspective().as_ref(),
                    lensfun,
                ) {
                    Ok(expand_input) => {
                        let identity =
                            Self::generative_identity_for(OnnxGenerativeRole::Expand, &edit);
                        let digest =
                            GenerativeCacheKey::expand(&expand_input, &canvas, seed, &identity)
                                .digest();
                        let (status, result) =
                            self.resolve_generative_role(OnnxGenerativeRole::Expand, &digest);
                        role_status[generative_role_index(OnnxGenerativeRole::Expand)] = status;
                        match result {
                            Ok(cached) => resolved.expand = Some(cached),
                            Err(error) => deferred_error = Some(error),
                        }
                    }
                    Err(error) => deferred_error = Some(error),
                }
            }
        }
        // `lensfun` is no longer used past this point: commit the collected
        // role verdicts before surfacing any deferred loud error.
        self.generative_role_status = role_status;
        if let Some(error) = deferred_error {
            return Err(error);
        }
        self.generative_artifacts = resolved.clone();
        self.generative_memo = Some(memo_key);
        Ok(resolved)
    }

    /// Resolve one role's canvas: session store first, then the persisted
    /// bundle addressed by the deterministic identity record id (this also
    /// covers the unlinked second role of a double-role record), then a loud
    /// diagnosis through the recipe link. The result is pinned to `digest`.
    ///
    /// Returns the visible [`GenerativeRoleStatus`] alongside the result so the
    /// panel can distinguish `valid`/`stale`/`missing`/`corrupt`.
    fn resolve_generative_role(
        &self,
        role: OnnxGenerativeRole,
        digest: &str,
    ) -> (
        GenerativeRoleStatus,
        Result<CachedGenerativeCanvas, GuiError>,
    ) {
        if let Some(cached) = self.generative_artifacts.get(role) {
            if cached.identity == digest {
                return (GenerativeRoleStatus::Valid, Ok(cached.clone()));
            }
        }
        let path = self.path.trim();
        let link = self
            .recipe
            .generative_edit
            .as_ref()
            .and_then(|edit| edit.artifact.as_ref());
        if !path.is_empty() {
            let zdata_path = zdata_path_for(Path::new(path));
            let bundle_root = zdata_path.parent().unwrap_or_else(|| Path::new("."));
            // The record id is derived from the identity digest, so a matching
            // record is current by construction (role/seed/canvas/prompt/model/
            // input are all in the digest, and zdata load verifies checksums).
            if zdata_path.exists() {
                if let Ok(container) = load_zdata(&zdata_path) {
                    if let Ok(record) = container.generative_canvas(&generative_record_id(digest)) {
                        return match ImageFrame::new(record.width, record.height, record.pixels) {
                            Ok(frame) => (
                                GenerativeRoleStatus::Valid,
                                Ok(CachedGenerativeCanvas {
                                    identity: digest.to_string(),
                                    artifact: GenerativeCanvasArtifact::new(
                                        GenerativeArtifacts::core_role(role),
                                        frame,
                                    ),
                                }),
                            ),
                            Err(error) => {
                                (GenerativeRoleStatus::Corrupt, Err(GuiError::Core(error)))
                            }
                        };
                    }
                }
            }
            if let Some(link) = link {
                let status = generative_artifact_status(bundle_root, link, digest);
                let role_status = match status {
                    GenerativeArtifactStatus::Available => GenerativeRoleStatus::Corrupt,
                    GenerativeArtifactStatus::Stale => GenerativeRoleStatus::Stale,
                    GenerativeArtifactStatus::Missing => GenerativeRoleStatus::Missing,
                    GenerativeArtifactStatus::Corrupt => GenerativeRoleStatus::Corrupt,
                };
                let error = match status {
                    // The link claims to be current but its record is unreadable
                    // — never render "as if not generated".
                    GenerativeArtifactStatus::Available => GuiError::Io(format!(
                        "generative {role:?} link `{}` is current but its bundle record is \
                         unreadable; refusing to render (no silent fallback)",
                        link.id
                    )),
                    other => GuiError::Io(format!(
                        "generative {role:?} canvas `{}` is {other:?} for the current identity; \
                         run \"Generate\" to rebuild it (no silent fallback)",
                        link.id
                    )),
                };
                return (role_status, Err(error));
            }
        }
        (
            GenerativeRoleStatus::Missing,
            Err(GuiError::Io(format!(
                "generative {role:?} is active but no canvas artifact is available; run \
                 \"Generate\" (no silent fallback)"
            ))),
        )
    }

    /// Returns the active virtual copy's source dimensions, used as the brush
    /// prompt resolution and overlay rasterization size.
    fn image_dims(&self) -> Result<(u32, u32), GuiError> {
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        Ok((frame.width, frame.height))
    }

    /// Ensure a mask is selected; create a default one if the active copy has
    /// none yet so a drawn prompt always has a home.
    fn ensure_selected_mask(&mut self) -> Result<String, GuiError> {
        if let Some(id) = self.selected_mask_id.clone() {
            return Ok(id);
        }
        let count = self
            .document
            .as_ref()
            .and_then(|d| {
                d.virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .map_or(0, |c| c.mask_library.len());
        self.create_mask(format!("Mask {}", count + 1))
    }

    /// Persist a finished [`MaskPrompt`] onto the selected mask and write the
    /// sidecar. A hand-drawn prompt mask is complete without a model — the
    /// geometric rasterizer (F-079) supplies the matte — so it is marked
    /// `Valid` (the file browser would otherwise report a phantom "missing
    /// model"). No silent fallback: a missing sidecar/document is a hard error.
    fn apply_mask_prompt(&mut self, prompt: MaskPrompt) -> Result<(), GuiError> {
        let mask_id = self.ensure_selected_mask()?;
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|mask| mask.id == mask_id)
            .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
        mask.prompt = Some(prompt);
        mask.status = MaskStatus::Valid;
        mask.error_text = None;
        self.render_key = None;
        self.save_sidecar();
        self.status = Str::MaskPromptSaved.format_arg(&mask_id);
        Ok(())
    }

    /// Finalize a brush stroke. An empty stroke is a hard error and writes
    /// nothing (no silent fallback). Every mark is validated against the F-079
    /// prompt rules before persistence.
    pub fn commit_brush_stroke(&mut self, marks: Vec<BrushMark>) -> Result<(), GuiError> {
        if marks.is_empty() {
            return Err(GuiError::Io("A brush mask needs at least one mark".into()));
        }
        for mark in &marks {
            if !mark.x.is_finite()
                || !mark.y.is_finite()
                || !mark.radius.is_finite()
                || !(0.0..=1.0).contains(&mark.x)
                || !(0.0..=1.0).contains(&mark.y)
                || !(0.0..=1.0).contains(&mark.radius)
                || mark.radius <= 0.0
            {
                return Err(GuiError::Io(
                    "Brush marks must have finite normalized coordinates within 0..=1 and a positive radius".into(),
                ));
            }
        }
        let (w, h) = self.image_dims()?;
        let prompt = MaskPrompt::Brush {
            marks,
            resolution: (w, h),
            transformation: PromptTransform::default(),
        };
        self.apply_mask_prompt(prompt)
    }

    /// Build a linear-gradient prompt from a drag (start→end, normalized 0..=1).
    ///
    /// Behaviour (documented): both endpoints are clamped to `0..=1` before use,
    /// so a drag that leaves the image still yields a well-defined angle from
    /// the clamped segment. The drag *direction* sets `angle_deg`
    /// (`atan2(dy, dx)`, normalized to `[0, 360)`); `start`/`end` are the matte
    /// values (1.0 → 0.0) along that axis, matching the F-079 geometric
    /// rasterizer (`start` + t·(end−start) across the normalized projection).
    pub fn gradient_prompt_from_drag(a: Point2, b: Point2) -> MaskPrompt {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        let dx = (b.x - a.x) as f64;
        let dy = (b.y - a.y) as f64;
        let mut angle = dy.atan2(dx).to_degrees();
        if angle < 0.0 {
            angle += 360.0;
        }
        MaskPrompt::Gradient {
            angle_deg: angle as f32,
            start: 1.0,
            end: 0.0,
            transformation: PromptTransform::default(),
        }
    }

    /// Finalize a linear-gradient drag. A zero-length drag (the two endpoints
    /// coincide within tolerance) is rejected.
    pub fn commit_gradient(&mut self, a: Point2, b: Point2) -> Result<(), GuiError> {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        if (b.x - a.x).abs() < 1e-4 && (b.y - a.y).abs() < 1e-4 {
            return Err(GuiError::Io("Drag a gradient across the image".into()));
        }
        self.apply_mask_prompt(Self::gradient_prompt_from_drag(a, b))
    }

    /// Build a radial-gradient (ellipse) prompt from a drag. The drag defines
    /// the ellipse bounding box: `center` is the segment midpoint and `radii`
    /// are half the absolute (clamped) deltas, clamped to `(0, 1]`.
    pub fn ellipse_prompt_from_drag(a: Point2, b: Point2) -> MaskPrompt {
        let a = Point2 {
            x: a.x.clamp(0.0, 1.0),
            y: a.y.clamp(0.0, 1.0),
        };
        let b = Point2 {
            x: b.x.clamp(0.0, 1.0),
            y: b.y.clamp(0.0, 1.0),
        };
        let center = Point2 {
            x: (a.x + b.x) / 2.0,
            y: (a.y + b.y) / 2.0,
        };
        let rx = ((b.x - a.x).abs() / 2.0).clamp(1e-3, 1.0);
        let ry = ((b.y - a.y).abs() / 2.0).clamp(1e-3, 1.0);
        MaskPrompt::Ellipse {
            center,
            radii: Point2 { x: rx, y: ry },
            transformation: PromptTransform::default(),
        }
    }

    /// Finalize a radial-gradient drag. A zero-size drag (both radii below
    /// tolerance) is rejected.
    pub fn commit_radial(&mut self, a: Point2, b: Point2) -> Result<(), GuiError> {
        let rx = ((b.x - a.x).abs() / 2.0).clamp(1e-3, 1.0);
        let ry = ((b.y - a.y).abs() / 2.0).clamp(1e-3, 1.0);
        if rx < 1e-4 && ry < 1e-4 {
            return Err(GuiError::Io("Drag a radial mask across the image".into()));
        }
        self.apply_mask_prompt(Self::ellipse_prompt_from_drag(a, b))
    }

    /// Finish the in-progress mask-tool drag, dispatching to the right commit
    /// based on the active tool. Errors are surfaced as visible [`GuiError`]s.
    fn finish_drawing(&mut self) {
        let tool = self.mask_tool;
        let start = self.drag_start;
        let end = self.drag_current;
        let marks = std::mem::take(&mut self.pending_brush_marks);
        self.drawing = false;
        self.drag_start = None;
        self.drag_current = None;
        let result = match tool {
            MaskTool::None => return,
            MaskTool::Brush => self.commit_brush_stroke(marks),
            MaskTool::LinearGradient => match (start, end) {
                (Some(a), Some(b)) => self.commit_gradient(a, b),
                _ => Ok(()),
            },
            MaskTool::Radial => match (start, end) {
                (Some(a), Some(b)) => self.commit_radial(a, b),
                _ => Ok(()),
            },
        };
        if let Err(error) = result {
            self.show_error(error);
        }
    }

    fn ensure_document_loaded(&mut self) -> Result<(), GuiError> {
        if self.document.is_none() {
            let frame = self
                .original
                .as_ref()
                .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
                .clone();
            self.document = Some(SidecarDocument::new(
                self.source_identity(&frame),
                "raster-mvp-1",
            ));
        }
        Ok(())
    }

    fn active_copy_mut(&mut self) -> Result<&mut lumina_sidecar::VirtualCopy, GuiError> {
        self.document
            .as_mut()
            .and_then(|d| {
                d.virtual_copies
                    .iter_mut()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))
    }

    fn active_layer_mut(&mut self) -> Result<&mut MaskLayer, GuiError> {
        self.active_copy_mut()?
            .mask_layers
            .first_mut()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))
    }

    /// GUI-FILMSTRIP-SYNC-1: pure filmstrip click semantics (Lightroom-like),
    /// headless-testable without an [`egui::Context`].
    ///
    /// * plain click → the selection is exactly `clicked`, anchor becomes `clicked`;
    /// * `toggle` (Cmd/Ctrl-Click) → `clicked` is added or removed, anchor becomes `clicked`;
    /// * `range` (Shift-Click) → the inclusive span from the anchor (or `clicked`
    ///   when there is no usable anchor) to `clicked` over `order` is added to
    ///   the selection; the anchor is kept so repeated Shift-Clicks extend from
    ///   the same origin.
    ///
    /// Clicking a path that is not in `order` leaves selection and anchor unchanged.
    pub fn apply_filmstrip_click(
        order: &[String],
        selection: &BTreeSet<String>,
        anchor: Option<&str>,
        clicked: &str,
        toggle: bool,
        range: bool,
    ) -> (BTreeSet<String>, Option<String>) {
        let end = order.iter().position(|path| path == clicked);
        let Some(end) = end else {
            return (selection.clone(), anchor.map(str::to_string));
        };
        if range {
            let start = anchor
                .and_then(|known| order.iter().position(|path| path == known))
                .unwrap_or(end);
            let (low, high) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            let mut next = selection.clone();
            for path in &order[low..=high] {
                next.insert(path.clone());
            }
            return (next, anchor.map(str::to_string));
        }
        if toggle {
            let mut next = selection.clone();
            if !next.remove(clicked) {
                next.insert(clicked.to_string());
            }
            return (next, Some(clicked.to_string()));
        }
        (
            BTreeSet::from([clicked.to_string()]),
            Some(clicked.to_string()),
        )
    }

    /// Currently selected filmstrip paths, sorted.
    pub fn filmstrip_selection(&self) -> Vec<String> {
        self.filmstrip_selection.iter().cloned().collect()
    }

    /// GUI-FILMSTRIP-SYNC-1: update the multi-selection for a click WITHOUT
    /// opening the image (Library grid single-click selects; opening stays
    /// on double-click). Selection bookkeeping is shared with
    /// [`Self::handle_filmstrip_click`] so every entry point syncs
    /// identically (GUI-FILMSTRIP-DUP-1).
    pub fn select_filmstrip_path(&mut self, path: String, toggle: bool, range: bool) {
        let order = self.filmstrip_order();
        let (next, anchor) = Self::apply_filmstrip_click(
            &order,
            &self.filmstrip_selection,
            self.filmstrip_anchor.as_deref(),
            &path,
            toggle,
            range,
        );
        // LRPAR-G15-STACK-15: a stack selects (and deselects) as one unit.
        let next = self.apply_stack_selection(next, &path, toggle);
        self.filmstrip_selection = next;
        self.filmstrip_anchor = anchor;
        trace!(
            "GUI interaction: filmstrip select {} (toggle={toggle}, range={range}, selected={})",
            path,
            self.filmstrip_selection.len()
        );
    }

    /// GUI-FILMSTRIP-SYNC-1: update the multi-selection for a filmstrip click
    /// and open the clicked image. Selection bookkeeping is synchronous (it
    /// never waits for the background decode started by [`Self::open_file`]).
    pub fn handle_filmstrip_click(&mut self, path: String, toggle: bool, range: bool) {
        self.select_filmstrip_path(path.clone(), toggle, range);
        trace!(
            "GUI interaction: filmstrip click {} (selected={})",
            path,
            self.filmstrip_selection.len()
        );
        self.open_file(path);
    }

    // LRPAR-G08-PREVIOUS: the filmstrip selection actions (Sync/Match/Previous)
    // and their sidecar helpers moved to `crate::selection_actions`.
    pub fn load_bytes(&mut self, bytes: Vec<u8>, name: impl Into<String>) -> Result<(), GuiError> {
        let name = name.into();
        // GUI-SIDECAR-READ-1: same flush as `open_file` — a dropped file
        // replaces the source through `apply_decoded_frame`, which drops an
        // armed commit (no-op without a file-backed image loaded).
        self.flush_pending_edit();
        let source_is_raw = is_raw_name(&name);
        let (frame, orientation, camera_white_balance, lens_identity) = if source_is_raw {
            let image = lumina_raw::decode_bytes(&bytes, &name)?;
            let wb = image.metadata.camera_white_balance;
            let camera_white_balance = if wb.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                warn!(
                    "As-Shot white balance invalid {:?} for {} — dropping to None (recipe WB remains, image will load)",
                    wb, name
                );
                None
            } else {
                Some(wb)
            };
            let lens_identity = lens_identity_from_metadata(&image.metadata);
            let orientation = image.metadata.orientation;
            (
                image.frame,
                orientation,
                camera_white_balance,
                lens_identity,
            )
        } else {
            (ImageFrame::decode(&bytes)?, 1, None, None)
        };
        // PERF-GUI-7: shared post-decode setup (also used by the async path).
        self.apply_decoded_frame(
            &frame,
            orientation,
            camera_white_balance,
            &name,
            &bytes,
            source_is_raw,
            lens_identity,
        );
        if let Err(e) = self.render() {
            error!("render after load failed for {}: {e}", self.source_name);
            self.show_error(e);
        }
        Ok(())
    }

    /// PERF-GUI-7: shared post-decode setup used by both the synchronous
    /// `load_bytes` (byte drops / tests) and the asynchronous `finish_decode`
    /// (background file decode). Sets the source frame, clears the sidecar
    /// document, resets the recipe and — crucially — caches the draft
    /// (viewport-resolution) source once per load so draft renders during a
    /// slider drag never re-allocate (PERF-GUI-3 "zero alloc during
    /// interaction").
    ///
    /// Eight arguments by design (like `Corrector::for_camera`): this is the
    /// single construction funnel for a new source — bundling would churn
    /// both decode paths for no behaviour gain.
    #[allow(clippy::too_many_arguments)]
    fn apply_decoded_frame(
        &mut self,
        frame: &ImageFrame,
        orientation: u8,
        camera_white_balance: Option<[f32; 4]>,
        name: &str,
        bytes: &[u8],
        source_is_raw: bool,
        lens_identity: Option<LensIdentity>,
    ) {
        self.source_name = name.to_string();
        self.source_bytes = Some(bytes.to_vec());
        self.source_is_raw = source_is_raw;
        self.raw_orientation = orientation;
        self.camera_white_balance = camera_white_balance;
        self.loaded_lens_identity = lens_identity;
        // CAMERA-WB-WELLE (R2-MCP-01): the decoder As-Shot context is an
        // explicit GPU input; bind it on the context (like the Lensfun
        // corrector / depth plane) so the VRAM path validates it with the
        // oracle's error instead of silently ignoring it. This is validation
        // state only — the gains are never re-applied (the decoder already
        // multiplied them in). The decode path above sanitizes invalid metadata
        // to `None`, so a rejection here is a programming error, logged loudly.
        #[cfg(feature = "gpu")]
        if let Some(gpu) = self.gpu.as_ref() {
            if let Err(error) = gpu.set_camera_white_balance(camera_white_balance) {
                warn!("GPU As-Shot white-balance bind rejected: {error}");
            }
        }
        #[cfg(feature = "lensfun")]
        {
            self.lensfun_cache = None;
        }
        {
            self.document = None;
            self.virtual_copy_id = "vc-original".into();
            self.selected_mask_id = None;
            // REVIEW-GUI-N1: a new image starts a fresh sidecar lineage.
            self.sidecar_revision = None;
            // REVIEW-GUI-N3: per-image session state must never leak from the
            // previous file into this one.
            self.history_selected = None;
            // LRPAR-G15-IPTC-S8: draft buffers + embedded cache belong to the
            // previous file too — unsaved keystrokes never carry over.
            self.meta_buffers.clear();
            self.meta_buffers_key = None;
            self.meta_buffers_dirty = false;
            self.meta_embedded_cache = None;
            self.pending_brush_marks.clear();
            self.drag_start = None;
            self.drag_current = None;
            self.drawing = false;
        }
        // REVIEW-GUI-N3 (viewport + interaction state): a new image opens at
        // fit with no pan, no zoom ROI, no Before/After hold, no armed WB
        // eyedropper and no stale render bookkeeping — otherwise image B
        // opened in an 8× crop of image A.
        self.preview_zoom = 1.0;
        self.zoom_mode = ZoomMode::Fit;
        self.preview_pan = egui::Vec2::ZERO;
        self.preview_roi = None;
        self.preview_render_src = None;
        // GUI-NAV-RECT-1: the overview belongs to the previous source.
        self.navigator_texture = None;
        self.navigator_texture_key = None;
        self.navigator_overview = None;
        self.navigator_overview_key = None;
        self.before_after = false;
        self.disarm_preview_pickers();
        self.render_mask_layers.clear();
        // GEN-ONNX-1 Welle 2b: a new source invalidates every session/persisted
        // generative canvas — the identity digests are source-bound.
        self.generative_artifacts = GenerativeArtifacts::default();
        self.generative_role_status = [GenerativeRoleStatus::Missing; 2];
        self.generative_memo = None;
        self.render_key = None;
        self.tone_analysis = None;
        self.preview_histogram = None;
        self.pending_slider_commit = None;
        self.pending_history_step = None;
        self.pending_full_render = false;
        self.last_edit_time = 0.0;
        self.original = Some(frame.clone());
        self.recipe = EditRecipe::default();
        // LRPAR-G01-BASIC: a new image starts from the default baseline
        // (finish_decode re-captures after adopting a persisted recipe).
        self.capture_section_baselines();
        self.error = None;
        self.preview_is_draft = false;
        // PERF-GUI-1: a new source identity invalidates every cached stage at
        // once (the coarsest invalidation level of the stage DAG). Recipe
        // changes never reach this point — they keep the base cache.
        self.base_stage_cache.clear();
        self.source_hash_memo = None;
        self.last_stage_work = None;
        // PERF-GUI-3: cache a downscaled source for fast draft renders; R3-RENDER-SIZE-1
        // resets the preview-cap state (built edge + capped-preview cache + warning).
        self.preview_cap_state = preview_size::PreviewCapState::new();
        self.draft_original = Some(frame.downscale(self.preview_cap_state.draft_max_dim));
        #[cfg(feature = "gpu")]
        {
            // H1: invalidate the persistent R16 brush plane — a new source size needs
            // a fresh zeroed plane; stale dimensions would mis-align tile uploads.
            self.brush_mask_plane = None;
            self.brush_mask_plane_dims = None;
            // R2-GUI-FOLLOWUP: a source switch must not reuse VRAM/present state
            // from the previous image. `vram_fresh = false` drops any stale
            // VRAM tone result and forces a fresh full render through the
            // present gate; `gpu_stage_gate = None` clears the memoized
            // `unsupported_gpu_stages` verdict so it is recomputed against the
            // new recipe/source identity instead of serving a long-gone verdict.
            self.vram_fresh = false;
            self.gpu_stage_gate = None;
            // GUI-LENSFUN-GATE-3 (F1): no present refusal from the previous
            // source may leak into the new one.
            self.vram_render_refusal = None;
        }
        self.status = Str::Loaded.format_arg(&self.source_name);
        info!(
            "loaded image {} (raw={}, camera_white_balance={:?})",
            self.source_name, source_is_raw, self.camera_white_balance
        );
    }

    pub fn set_adjustment(&mut self, name: &str, value: f64) {
        trace!(
            "GUI interaction: set_adjustment {}={} (before render)",
            name,
            value
        );
        self.recipe.adjustments.insert(name.into(), value);
        // GUI-SLIDER-SAVE-1: remember the commit so the debounced full render
        // can save the sidecar and log `<key>=<value> saved`. Zoom/pan state
        // is deliberately never recorded here — it stays GUI session state.
        self.pending_slider_commit = Some((name.to_string(), value));
        // PERF-GUI-1 stepwise invalidation: an adjustment is downstream of the
        // base stage, so only the render identity and the derived tone panel
        // are invalidated here. The cached demosaiced base
        // (`base_stage_cache`) stays — its `CacheStage::Base` digest is
        // recipe-blind, so the next render hits it and recomputes exactly the
        // Adjustments(+geometry/masks) stages.
        self.render_key = None;
        self.tone_analysis = None;
        // Coalesce: the slider drag renders a draft live; the full render is
        // deferred to pointer release (PERF-GUI-3/4).
        self.pending_full_render = true;
        self.status = Str::ChangePending.t().into();
        self.error = None;
        // GFX-SLIDER-VRAM-FRESH: an adjustment is an edit, exactly like
        // `mark_dirty` — the VRAM tone result no longer matches the recipe and
        // must never be presented until the drag path re-renders it. `mark_dirty`
        // cleared `vram_fresh` here, but `set_adjustment` (the interactive slider
        // path) did not, so a stale VRAM frame could keep being presented after a
        // slider change that did not immediately re-run `render_to_vram` (e.g.
        // in headless tests / programmatic `set_adjustment` with no pointer drag).
        #[cfg(feature = "gpu")]
        {
            self.vram_fresh = false;
            self.vram_mask_is_evaluated = false;
            // GUI-LENSFUN-GATE-3 (F1): the recipe changed, so a present refusal
            // captured for the previous recipe is no longer known to apply.
            self.vram_render_refusal = None;
        }
    }

    /// Set a single Presence field (`texture`, `clarity` or `dehaze`). The value
    /// is stored in the normative `-1..=1` domain (shown as `-100..+100`); the
    /// sidecar validation is the source of truth for the domain, so out-of-range
    /// values are stored as given and rejected on save rather than silently
    /// clamped (no silent fallback).
    pub fn set_presence(&mut self, field: &str, value: f64) {
        let mut presence = self.recipe.presence.unwrap_or(Presence {
            version: 1,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
        });
        match field {
            "texture" => presence.texture = value as f32,
            "clarity" => presence.clarity = value as f32,
            "dehaze" => presence.dehaze = value as f32,
            _ => return,
        }
        self.recipe.presence = Some(presence);
        trace!("GUI interaction: set_presence {}={}", field, value);
        // GUI-SLIDER-SAVE-1: presence sliders commit like flat adjustments.
        self.pending_slider_commit = Some((format!("presence.{field}"), value));
        self.mark_dirty();
    }

    /// Validate curve points like the core (`2..=32` points, finite values in
    /// `0..=1`, strictly ascending inputs, `(0,0)`/`(1,1)` endpoints). `None`
    /// is valid with the offending description.
    fn validate_curve_points(points: &[CurvePoint]) -> Option<String> {
        if !(2..=32).contains(&points.len()) {
            return Some(format!("need 2..=32 points, got {}", points.len()));
        }
        for (index, point) in points.iter().enumerate() {
            for (field, value) in [("input", point.input), ("output", point.output)] {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Some(format!("points[{index}].{field} out of range"));
                }
            }
            if index > 0 && point.input <= points[index - 1].input {
                return Some(format!("points[{index}].input not ascending"));
            }
        }
        let (first, last) = (&points[0], &points[points.len() - 1]);
        if first.input != 0.0 || first.output != 0.0 {
            return Some("first point must be (0,0)".into());
        }
        if last.input != 1.0 || last.output != 1.0 {
            return Some("last point must be (1,1)".into());
        }
        None
    }

    /// Set one free point-curve control point (`input`/`output`) of one channel
    /// (G-02) and record the save commit. The interactive graph
    /// (UX-LOOK-TONECURVE-18) is the production editor; this per-field path is
    /// test-owned (`#[cfg(test)]`, loud-refusal coverage). Violations are
    /// refused loudly (status + no save), never clipped silently.
    #[cfg(test)]
    fn set_curve_point(&mut self, channel: &str, index: usize, field: &str, value: f64) {
        if !matches!(field, "input" | "output") {
            warn!("set_curve_point: unknown field {field}");
            return;
        }
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("set_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = self.recipe.curves.clone().unwrap_or_else(|| Curves {
            version: 1,
            master: identity_curve_points(),
            channels: CurveChannels::default(),
        });
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate
                .channels
                .red
                .get_or_insert_with(identity_curve_points),
            "green" => candidate
                .channels
                .green
                .get_or_insert_with(identity_curve_points),
            "blue" => candidate
                .channels
                .blue
                .get_or_insert_with(identity_curve_points),
            _ => unreachable!(),
        };
        let Some(point) = slot.get_mut(index) else {
            warn!("set_curve_point: {channel}[{index}] out of bounds");
            return;
        };
        match field {
            "input" => point.input = value as f32,
            "output" => point.output = value as f32,
            _ => unreachable!(),
        }
        if let Some(reason) = Self::validate_curve_points(slot) {
            self.status = Str::ToneCurveInvalidPattern.format_arg(&reason);
            warn!("set_curve_point: {channel}[{index}].{field} refused ({reason})");
            return;
        }
        self.recipe.curves = Some(candidate);
        self.mark_recipe_dirty(&format!("curves.{channel}.points[{index}].{field}"), value);
    }

    /// Insert a free point-curve control point into one channel (G-02),
    /// sorted by input. Refuses invalid/duplicate inputs loudly (status +
    /// no save). Replaces that channel's parametric list (Last-Write-Wins).
    fn add_curve_point(&mut self, channel: &str, input: f64, output: f64) {
        instrument_gui_action!(self, GuiAction::AddCurvePoint);
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("add_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = self.recipe.curves.clone().unwrap_or_else(|| Curves {
            version: 1,
            master: identity_curve_points(),
            channels: CurveChannels::default(),
        });
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate
                .channels
                .red
                .get_or_insert_with(identity_curve_points),
            "green" => candidate
                .channels
                .green
                .get_or_insert_with(identity_curve_points),
            "blue" => candidate
                .channels
                .blue
                .get_or_insert_with(identity_curve_points),
            _ => unreachable!(),
        };
        slot.push(CurvePoint {
            input: input as f32,
            output: output as f32,
        });
        slot.sort_by(|a, b| a.input.total_cmp(&b.input));
        if let Some(reason) = Self::validate_curve_points(slot) {
            self.status = Str::ToneCurveInvalidPattern.format_arg(&reason);
            warn!("add_curve_point: {channel} ({input},{output}) refused ({reason})");
            return;
        }
        self.recipe.curves = Some(candidate);
        info!("GUI interaction: curves.{channel} add point ({input},{output})");
        self.mark_recipe_dirty(&format!("curves.{channel}.add"), input);
    }

    /// Remove a free point-curve control point from one channel (G-02).
    /// Refuses loudly when fewer than 3 points remain or an endpoint
    /// (`(0,0)`/`(1,1)`) is targeted — endpoints are mandatory.
    fn remove_curve_point(&mut self, channel: &str, index: usize) {
        instrument_gui_action!(self, GuiAction::RemoveCurvePoint);
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("remove_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = match self.recipe.curves.clone() {
            Some(curves) => curves,
            None => {
                warn!("remove_curve_point: no curves for {channel}");
                return;
            }
        };
        let len = match channel {
            "master" => candidate.master.len(),
            "red" => candidate.channels.red.as_ref().map_or(0, Vec::len),
            "green" => candidate.channels.green.as_ref().map_or(0, Vec::len),
            "blue" => candidate.channels.blue.as_ref().map_or(0, Vec::len),
            _ => unreachable!(),
        };
        if len <= 2 {
            self.status = Str::ToneCurveInvalidPattern.format_arg("need 2..=32 points");
            warn!("remove_curve_point: {channel} already minimal");
            return;
        }
        if index == 0 || index + 1 >= len {
            self.status =
                Str::ToneCurveInvalidPattern.format_arg("endpoints (0,0)/(1,1) are mandatory");
            warn!("remove_curve_point: {channel}[{index}] is an endpoint");
            return;
        }
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate.channels.red.as_mut().expect("len>0"),
            "green" => candidate.channels.green.as_mut().expect("len>0"),
            "blue" => candidate.channels.blue.as_mut().expect("len>0"),
            _ => unreachable!(),
        };
        if index >= slot.len() {
            warn!("remove_curve_point: {channel}[{index}] out of bounds");
            return;
        }
        slot.remove(index);
        self.recipe.curves = Some(candidate);
        info!("GUI interaction: curves.{channel} remove point {index}");
        self.mark_recipe_dirty(&format!("curves.{channel}.remove"), index as f64);
    }

    /// Set one HSL mixer channel field (`red`…`magenta` × `hue`/`saturation`/
    /// `luminance`) and record the save commit (GUI-SLIDER-SAVE-1). Unknown
    /// names are ignored loudly — all call sites pass literals.
    fn set_hsl_value(&mut self, channel: &str, field: &str, value: f64) {
        if !matches!(
            channel,
            "red" | "orange" | "yellow" | "green" | "cyan" | "blue" | "violet" | "magenta"
        ) {
            warn!("set_hsl_value: unknown channel {channel}");
            return;
        }
        let mut hsl = self.recipe.hsl.clone().unwrap_or_default();
        // The derived `Default` carries `version: 0`, which the sidecar
        // validation rejects (`unsupported hsl version`) — a fresh HSL block
        // is always version 1 (same class of explicit-version construction as
        // every other struct setter here).
        hsl.version = 1;
        match field {
            "hue" => hsl_channel_mut(&mut hsl, channel).hue = value as f32,
            "saturation" => hsl_channel_mut(&mut hsl, channel).saturation = value as f32,
            "luminance" => hsl_channel_mut(&mut hsl, channel).luminance = value as f32,
            _ => {
                warn!("set_hsl_value: unknown field {field}");
                return;
            }
        }
        self.recipe.hsl = Some(hsl);
        self.mark_recipe_dirty(&format!("hsl.{channel}.{field}"), value);
    }

    /// Set one color-grading range field (`shadows`/`midtones`/`highlights` ×
    /// `hue_degrees`/`saturation`/`luminance`) and record the save commit
    /// (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly.
    fn set_color_grading_value(&mut self, range: &str, field: &str, value: f64) {
        let mut cg = self
            .recipe
            .color_grading
            .clone()
            .unwrap_or_else(ColorGrading::neutral);
        let slot = match range {
            "shadows" => &mut cg.shadows,
            "midtones" => &mut cg.midtones,
            "highlights" => &mut cg.highlights,
            _ => {
                warn!("set_color_grading_value: unknown range {range}");
                return;
            }
        };
        match field {
            "hue_degrees" => slot.hue_degrees = value as f32,
            "saturation" => slot.saturation = value as f32,
            "luminance" => slot.luminance = value as f32,
            _ => {
                warn!("set_color_grading_value: unknown field {field}");
                return;
            }
        }
        self.recipe.color_grading = Some(cg);
        self.mark_recipe_dirty(&format!("color_grading.{range}.{field}"), value);
    }

    /// Set the color-grading balance and record the save commit
    /// (GUI-SLIDER-SAVE-1).
    fn set_color_grading_balance(&mut self, value: f64) {
        let mut cg = self
            .recipe
            .color_grading
            .clone()
            .unwrap_or_else(ColorGrading::neutral);
        cg.balance = value as f32;
        self.recipe.color_grading = Some(cg);
        self.mark_recipe_dirty("color_grading.balance", value);
    }

    /// Set the color-grading blending (G-02 Feinschliff, `0..=1`) and record
    /// the save commit (GUI-SLIDER-SAVE-1).
    fn set_color_grading_blending(&mut self, value: f64) {
        let mut cg = self
            .recipe
            .color_grading
            .clone()
            .unwrap_or_else(ColorGrading::neutral);
        cg.blending = value as f32;
        self.recipe.color_grading = Some(cg);
        self.mark_recipe_dirty("color_grading.blending", value);
    }

    /// Add a Point Color entry (G-02, F-090b) with neutral shifts and record
    /// the save commit. The id is the next stable `pc-<n>` over the current
    /// recipe state (never positional). At most 8 entries; a ninth is
    /// refused loudly via the status line.
    fn add_point_color(&mut self) {
        instrument_gui_action!(self, GuiAction::AddPointColor);
        let mut block = self.recipe.point_color.clone().unwrap_or(PointColor {
            version: 1,
            entries: Vec::new(),
        });
        block.version = 1;
        if block.entries.len() >= 8 {
            self.status = Str::PointColorFull.t().to_string();
            warn!("add_point_color: entry limit (8) reached");
            return;
        }
        let id = PointColorEntry::next_id(&block.entries);
        block.entries.push(PointColorEntry {
            id: id.clone(),
            hue_center: 0.0,
            hue_range: 30.0,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            luminance_shift: 0.0,
        });
        self.recipe.point_color = Some(block);
        info!("GUI interaction: point_color add {id}");
        self.mark_recipe_dirty(
            "point_color.add",
            self.recipe
                .point_color
                .as_ref()
                .map_or(0.0, |block| block.entries.len() as f64),
        );
    }

    /// Remove one Point Color entry by stable id (G-02). Unknown ids are
    /// refused loudly; removing the last entry drops the whole block
    /// (absent = identity).
    fn remove_point_color(&mut self, id: &str) {
        instrument_gui_action!(self, GuiAction::RemovePointColor);
        let Some(block) = self.recipe.point_color.clone() else {
            warn!("remove_point_color: no point_color block for id {id}");
            return;
        };
        let len = block.entries.len();
        let entries: Vec<PointColorEntry> =
            block.entries.into_iter().filter(|e| e.id != id).collect();
        if entries.len() == len {
            warn!("remove_point_color: unknown id {id}");
            return;
        }
        self.recipe.point_color = if entries.is_empty() {
            None
        } else {
            Some(PointColor {
                version: 1,
                entries,
            })
        };
        info!("GUI interaction: point_color remove {id}");
        self.mark_recipe_dirty("point_color.remove", len as f64);
    }

    /// Set one Point Color entry field (`hue_center`/`hue_range`/
    /// `hue_shift`/`saturation_shift`/`luminance_shift`) and record the save
    /// commit (GUI-SLIDER-SAVE-1). Unknown ids/fields are ignored loudly.
    /// Out-of-range values are refused loudly (status + no save) instead of
    /// being clipped silently.
    fn set_point_color_value(&mut self, id: &str, field: &str, value: f64) {
        let Some(mut block) = self.recipe.point_color.clone() else {
            warn!("set_point_color_value: no point_color block for id {id}");
            return;
        };
        let Some(entry) = block.entries.iter_mut().find(|e| e.id == id) else {
            warn!("set_point_color_value: unknown id {id}");
            return;
        };
        let (lo, hi) = match field {
            "hue_center" => (0.0, 360.0),
            "hue_range" => (0.0, 180.0),
            "hue_shift" | "saturation_shift" | "luminance_shift" => (-1.0, 1.0),
            _ => {
                warn!("set_point_color_value: unknown field {field}");
                return;
            }
        };
        if !value.is_finite() || !(lo..=hi).contains(&value) {
            self.status = Str::PointColorRangePattern.format_arg(&format!("{id}.{field}"));
            warn!("set_point_color_value: {id}.{field}={value} out of range");
            return;
        }
        match field {
            "hue_center" => entry.hue_center = value as f32,
            "hue_range" => entry.hue_range = value as f32,
            "hue_shift" => entry.hue_shift = value as f32,
            "saturation_shift" => entry.saturation_shift = value as f32,
            "luminance_shift" => entry.luminance_shift = value as f32,
            _ => unreachable!(),
        }
        self.recipe.point_color = Some(block);
        self.mark_recipe_dirty(&format!("point_color.{id}.{field}"), value);
    }

    /// Set one effects field (`vignette` × `amount`/`midpoint`/`roundness`/
    /// `feather`, `grain` × `amount`/`size`/`roughness`/`seed`) and record the
    /// save commit (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly.
    fn set_effects_value(&mut self, group: &str, field: &str, value: f64) {
        let mut effects = self.recipe.effects.clone().unwrap_or(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: 0.0,
                midpoint: 0.5,
                roundness: 0.0,
                feather: 0.0,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.0,
                size: 0.0,
                roughness: 0.0,
                seed: 0,
            }),
        });
        match group {
            "vignette" => {
                let slot = effects.vignette.get_or_insert(Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.5,
                    roundness: 0.0,
                    feather: 0.0,
                });
                match field {
                    "amount" => slot.amount = value as f32,
                    "midpoint" => slot.midpoint = value as f32,
                    "roundness" => slot.roundness = value as f32,
                    "feather" => slot.feather = value as f32,
                    _ => {
                        warn!("set_effects_value: unknown vignette field {field}");
                        return;
                    }
                }
            }
            "grain" => {
                let slot = effects.grain.get_or_insert(Grain {
                    version: 1,
                    amount: 0.0,
                    size: 0.0,
                    roughness: 0.0,
                    seed: 0,
                });
                match field {
                    "amount" => slot.amount = value as f32,
                    "size" => slot.size = value as f32,
                    "roughness" => slot.roughness = value as f32,
                    "seed" => slot.seed = value as u64,
                    _ => {
                        warn!("set_effects_value: unknown grain field {field}");
                        return;
                    }
                }
            }
            _ => {
                warn!("set_effects_value: unknown group {group}");
                return;
            }
        }
        self.recipe.effects = Some(effects);
        self.mark_recipe_dirty(&format!("effects.{group}.{field}"), value);
    }

    /// Set one sharpening field (`amount`/`radius`/`detail`/`masking`) and
    /// record the save commit (GUI-SLIDER-SAVE-1). Unknown names are ignored
    /// loudly. Public for the headless KITTEST-PARITY-PATHS-1 matrix, which
    /// needs a Detail-scope recipe built through the same setter the panel uses.
    pub fn set_sharpening_value(&mut self, field: &str, value: f64) {
        let mut sh = self.recipe.sharpening.unwrap_or(Sharpening {
            version: 1,
            amount: 0.0,
            radius: 0.5,
            detail: 0.0,
            masking: 0.0,
        });
        match field {
            "amount" => sh.amount = value as f32,
            "radius" => sh.radius = value as f32,
            "detail" => sh.detail = value as f32,
            "masking" => sh.masking = value as f32,
            _ => {
                warn!("set_sharpening_value: unknown field {field}");
                return;
            }
        }
        self.recipe.sharpening = Some(sh);
        self.mark_recipe_dirty(&format!("sharpening.{field}"), value);
    }

    /// Set one noise-reduction field (`luminance`/`color`) and record the save
    /// commit (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly. Public for
    /// the headless KITTEST-PARITY-PATHS-1 matrix, which needs a Detail-scope
    /// recipe built through the same setter the panel uses.
    pub fn set_noise_reduction_value(&mut self, field: &str, value: f64) {
        let mut nr = self.recipe.noise_reduction.unwrap_or(NoiseReduction {
            version: 1,
            luminance: 0.0,
            color: 0.0,
        });
        match field {
            "luminance" => nr.luminance = value as f32,
            "color" => nr.color = value as f32,
            _ => {
                warn!("set_noise_reduction_value: unknown field {field}");
                return;
            }
        }
        self.recipe.noise_reduction = Some(nr);
        self.mark_recipe_dirty(&format!("noise_reduction.{field}"), value);
    }

    /// Remove the manual lens profile (keeps the coefficients). Arms one
    /// G-06 history step.
    pub fn clear_lens_profile(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearLensProfile);
        if let Some(lens) = self.recipe.lens_correction.as_mut() {
            lens.profile = None;
        }
        self.mark_recipe_dirty("lens_correction.profile_clear", 0.0);
        self.pending_history_step = Some("lens_correction.profile_clear".into());
        info!("GUI interaction: clear_lens_profile");
    }

    /// Set one lens-correction field (`distortion_k1`…`ca_blue`) and record the
    /// save commit (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly.
    /// Arms one G-06 history step.
    fn set_lens_correction_value(&mut self, field: &str, value: f64) {
        let mut lc = self
            .recipe
            .lens_correction
            .clone()
            .unwrap_or(LensCorrection {
                version: 1,
                profile: None,
                distortion_k1: None,
                distortion_k2: None,
                distortion_k3: None,
                vignette_c0: None,
                vignette_c1: None,
                vignette_c2: None,
                ca_red: None,
                ca_blue: None,
            });
        let slot = match field {
            "distortion_k1" => &mut lc.distortion_k1,
            "distortion_k2" => &mut lc.distortion_k2,
            "distortion_k3" => &mut lc.distortion_k3,
            "vignette_c0" => &mut lc.vignette_c0,
            "vignette_c1" => &mut lc.vignette_c1,
            "vignette_c2" => &mut lc.vignette_c2,
            "ca_red" => &mut lc.ca_red,
            "ca_blue" => &mut lc.ca_blue,
            _ => {
                warn!("set_lens_correction_value: unknown field {field}");
                return;
            }
        };
        *slot = Some(value as f32);
        self.recipe.lens_correction = Some(lc);
        self.mark_recipe_dirty(&format!("lens_correction.{field}"), value);
        self.pending_history_step = Some(format!("lens_correction.{field}"));
        info!("GUI interaction: set_lens_correction_value {field} -> {value}");
    }

    /// Current lens-blur stage (G-05), if the active virtual copy carries one.
    pub fn lens_blur(&self) -> Option<LensBlur> {
        self.recipe.lens_blur.clone()
    }

    /// User-visible Lensfun auto status (G-06): EXIF snapshot plus the
    /// manual profile state. Always visible, never implied; DB-backed
    /// profile matching itself happens at render time (CLI
    /// `--lensfun-status` resolves it on demand for one file).
    pub fn lensfun_auto_status_text(&self) -> String {
        #[cfg(not(feature = "lensfun"))]
        {
            "unavailable in this build (manual correction applies)".into()
        }
        #[cfg(feature = "lensfun")]
        {
            let exif = match &self.loaded_lens_identity {
                Some(identity) => {
                    let mut parts = Vec::new();
                    if let Some(make) = &identity.camera_make {
                        parts.push(make.clone());
                    }
                    if let Some(model) = &identity.camera_model {
                        parts.push(model.clone());
                    }
                    if let Some(lens) = &identity.lens {
                        parts.push(format!("({lens})"));
                    }
                    if parts.is_empty() {
                        "partial EXIF".into()
                    } else {
                        parts.join(" ")
                    }
                }
                None => "no EXIF".into(),
            };
            let manual = match self
                .recipe
                .lens_correction
                .as_ref()
                .and_then(|lens| lens.profile.as_deref())
            {
                Some(name) => format!("manual profile `{name}`"),
                None => "no manual profile".to_string(),
            };
            format!("{exif}; {manual}; auto applies at render when a system profile matches")
        }
    }

    /// Lensfun auto-corrector cache refresh for a render at `width`×`height`
    /// (G-06, `lensfun` feature only): rebuilds the cached corrector when
    /// the identity/dimensions key changed. Split from
    /// [`Self::lensfun_render_ref`] so renders can refresh under `&mut`
    /// first and then build the `RenderContext` under shared borrows.
    #[cfg(feature = "lensfun")]
    fn ensure_lensfun_cache(&mut self, width: u32, height: u32) {
        let Some(identity) = self.loaded_lens_identity.clone() else {
            return;
        };
        let (Some(make), Some(model), Some(focal), Some(aperture)) = (
            identity.camera_make.clone(),
            identity.camera_model.clone(),
            identity.focal_length.filter(|v| v.is_finite()),
            identity.aperture.filter(|v| v.is_finite()),
        ) else {
            return;
        };
        let key = (
            Some(make.clone()),
            Some(model.clone()),
            identity.lens.clone(),
            width,
            height,
            focal.to_bits(),
            aperture.to_bits(),
        );
        let fresh = match &self.lensfun_cache {
            Some(cached) => cached.key != key,
            None => true,
        };
        if !fresh {
            return;
        }
        // Rebuild: a new source (or new dimensions) needs a new modifier.
        // A rebuild that finds no profile caches NOTHING, so every render
        // retries the lookup instead of pinning a stale miss across a DB
        // install — the lookup itself is strict (never a guessed
        // correction, same contract as the CLI `build_lensfun_corrector`).
        let Some(db) = lumina_lensfun::LensfunDb::load_system() else {
            return;
        };
        let Some(corrector) = db.for_camera(
            &make,
            &model,
            identity.lens.as_deref(),
            width,
            height,
            focal,
            aperture,
            10.0,
        ) else {
            return;
        };
        info!(
            "lensfun auto: profile matched for {make} {model} (distortion={} vignetting={} tca={})",
            corrector.has_distortion(),
            corrector.has_vignetting(),
            corrector.has_tca()
        );
        // GUI-LENSFUN-GATE-1 / GPU-LENSFUN-PARITY-1: snapshot the
        // pixel-relevance once. A non-identity corrector is bound on the GPU as
        // a precomputed `LensfunMap` (`lensfun_gpu::bind`); only an identity
        // one is a no-op that leaves the manual model in effect on both paths.
        #[cfg(feature = "gpu")]
        let active = !corrector.is_identity();
        self.lensfun_cache = Some(CachedLensCorrector {
            corrector,
            _db: db,
            key,
            #[cfg(feature = "gpu")]
            active,
            #[cfg(feature = "gpu")]
            gpu_map: None,
        });
    }

    /// Shared borrow of the cached Lensfun auto-corrector for a render
    /// (G-06, `lensfun` feature only). Call [`Self::ensure_lensfun_cache`]
    /// first so the cache matches the rendered frame.
    #[cfg(feature = "lensfun")]
    fn lensfun_render_ref(&self) -> Option<lumina_core::LensfunCorrectorRef<'_>> {
        self.lensfun_cache
            .as_ref()
            .map(|cached| lumina_core::LensfunCorrectorRef(&cached.corrector))
    }

    /// User-visible lens-blur depth status (G-05): `off`, `heuristic active`
    /// or `missing depth artifact`. The GUI never resolves external depth
    /// files (no depth format in v1), so a referenced artifact reports
    /// `missing` until a loader exists — identical to the CLI contract.
    /// DEPTH-PLUMBING-1 (Entscheid 2026-09-16): runtime binding of external
    /// depth is deliberately Post-MVP; v1 persists/reports the reserved
    /// reference and a set reference aborts the render loudly.
    /// (`feature/architecture/pipeline.md` § „External-Depth-Bindung“.)
    pub fn lens_blur_status_text(&self) -> String {
        lumina_core::lens_blur_status(self.recipe.lens_blur.as_ref(), false).into()
    }

    /// Focus-rectangle overlay rect (G-05) in preview-image coordinates:
    /// the normalized recipe rect mapped into `img_rect`. `None` when no
    /// enabled stage with a valid rect exists. Pure helper so the mapping is
    /// unit-testable headless.
    pub fn lens_blur_focus_overlay(
        img_rect: egui::Rect,
        blur: Option<&LensBlur>,
    ) -> Option<egui::Rect> {
        let b = blur.filter(|b| b.enabled)?;
        let r = &b.focus_rect;
        if !(0.0..=1.0).contains(&r.x)
            || !(0.0..=1.0).contains(&r.y)
            || r.width <= 0.0
            || r.height <= 0.0
            || r.x + r.width > 1.0
            || r.y + r.height > 1.0
        {
            return None;
        }
        let min = img_rect.min + egui::vec2(r.x * img_rect.width(), r.y * img_rect.height());
        let max = img_rect.min
            + egui::vec2(
                (r.x + r.width) * img_rect.width(),
                (r.y + r.height) * img_rect.height(),
            );
        Some(egui::Rect::from_min_max(min, max))
    }

    /// Crop-rectangle overlay rect (G-06) in preview-image coordinates: the
    /// normalized recipe crop mapped into `img_rect`. Aspect presets resolve
    /// to the same centered rectangle as the core `crop_rect` (needs the
    /// source dimensions); free rects map directly. `None` when no crop is
    /// set or the rect is invalid. Pure helper so the mapping is
    /// unit-testable headless.
    pub fn crop_overlay_rect(
        img_rect: egui::Rect,
        crop: Option<&Crop>,
        src_w: u32,
        src_h: u32,
    ) -> Option<egui::Rect> {
        let (x, y, w, h) = match crop? {
            Crop::Free {
                x,
                y,
                width,
                height,
            } => (*x, *y, *width, *height),
            Crop::Aspect { preset } => {
                if src_w == 0 || src_h == 0 {
                    return None;
                }
                let ratio = match preset {
                    AspectPreset::Original => f64::from(src_w) / f64::from(src_h),
                    AspectPreset::OneToOne => 1.0,
                    AspectPreset::FourToFive => 4.0 / 5.0,
                    AspectPreset::FiveToFour => 5.0 / 4.0,
                    AspectPreset::ThreeToTwo => 3.0 / 2.0,
                    AspectPreset::TwoToThree => 2.0 / 3.0,
                    AspectPreset::FourToThree => 4.0 / 3.0,
                    AspectPreset::ThreeToFour => 3.0 / 4.0,
                    AspectPreset::SixteenToNine => 16.0 / 9.0,
                    AspectPreset::NineToSixteen => 9.0 / 16.0,
                };
                let source_ratio = f64::from(src_w) / f64::from(src_h);
                if source_ratio > ratio {
                    (
                        ((1.0 - ratio / source_ratio) / 2.0) as f32,
                        0.0,
                        (ratio / source_ratio) as f32,
                        1.0,
                    )
                } else {
                    (
                        0.0,
                        ((1.0 - source_ratio / ratio) / 2.0) as f32,
                        1.0,
                        (source_ratio / ratio) as f32,
                    )
                }
            }
        };
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return None;
        }
        if !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
            || w <= 0.0
            || h <= 0.0
            || x + w > 1.0
            || y + h > 1.0
        {
            return None;
        }
        let min = img_rect.min + egui::vec2(x * img_rect.width(), y * img_rect.height());
        let max =
            img_rect.min + egui::vec2((x + w) * img_rect.width(), (y + h) * img_rect.height());
        Some(egui::Rect::from_min_max(min, max))
    }
    /// stage with centered defaults when none exists (same defaults as the
    /// CLI `lens-blur` command: touching lens blur enables it).
    fn lens_blur_mut(&mut self) -> &mut LensBlur {
        self.recipe.lens_blur.get_or_insert(LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.0,
            focal_far: 0.2,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        })
    }

    /// Enable/disable the lens-blur stage (G-05). Values are kept, so
    /// disabling renders identity and re-enabling restores the look.
    /// Recipe-backed: persists through the debounced slider-save path
    /// ([`Self::commit_pending_slider_save`], `info!`-logged).
    pub fn set_lens_blur_enabled(&mut self, enabled: bool) {
        instrument_gui_action!(self, GuiAction::SetLensBlurEnabled);
        self.lens_blur_mut().enabled = enabled;
        self.mark_recipe_dirty("lens_blur.enabled", f64::from(enabled as u8));
        info!("GUI interaction: set_lens_blur_enabled -> {enabled}");
    }

    /// Set the blur strength `0..=1` (G-05, 0 is identity). Loud on
    /// out-of-range/non-finite values; the recipe is untouched then.
    pub fn set_lens_blur_amount(&mut self, amount: f64) -> Result<(), GuiError> {
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
            return Err(GuiError::Io(format!(
                "Lens blur amount must be 0..=1, got {amount}"
            )));
        }
        self.lens_blur_mut().blur_amount = amount as f32;
        self.mark_recipe_dirty("lens_blur.blur_amount", amount);
        info!("GUI interaction: set_lens_blur_amount -> {amount}");
        Ok(())
    }

    /// Set the sharp depth band `[near, far]` in `0..=1` (G-05). Loud when
    /// either edge is out of range or `near > far`; the recipe is untouched
    /// then.
    pub fn set_lens_blur_focal(&mut self, near: f64, far: f64) -> Result<(), GuiError> {
        for (name, v) in [("focal_near", near), ("focal_far", far)] {
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return Err(GuiError::Io(format!(
                    "Lens blur {name} must be 0..=1, got {v}"
                )));
            }
        }
        if near > far {
            return Err(GuiError::Io(format!(
                "Lens blur focal range must satisfy near <= far, got {near} > {far}"
            )));
        }
        let blur = self.lens_blur_mut();
        blur.focal_near = near as f32;
        blur.focal_far = far as f32;
        self.mark_recipe_dirty("lens_blur.focal_near", near);
        self.mark_recipe_dirty("lens_blur.focal_far", far);
        info!("GUI interaction: set_lens_blur_focal -> {near}..={far}");
        Ok(())
    }

    /// Set the bokeh kernel shape (G-05). All three shapes are deterministic
    /// integer kernels (no randomness).
    pub fn set_lens_blur_bokeh(&mut self, bokeh: BokehShape) {
        instrument_gui_action!(self, GuiAction::SetLensBlurBokeh);
        self.lens_blur_mut().bokeh = bokeh;
        self.mark_recipe_dirty(
            "lens_blur.bokeh",
            match bokeh {
                BokehShape::Round => 0.0,
                BokehShape::Elliptical => 1.0,
                BokehShape::Hexagonal => 2.0,
            },
        );
        info!("GUI interaction: set_lens_blur_bokeh -> {bokeh:?}");
    }

    /// Set the focus rectangle in normalized `0..=1` coordinates (G-05).
    /// Loud on degenerate/out-of-bounds rectangles; the recipe is untouched
    /// then (the save-time validator rejects them too — never a silent
    /// reinterpretation).
    pub fn set_lens_blur_focus_rect(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), GuiError> {
        for (name, v) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            if !v.is_finite() {
                return Err(GuiError::Io(format!(
                    "Lens blur focus rect {name} must be finite, got {v}"
                )));
            }
        }
        if !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
            || width <= 0.0
            || height <= 0.0
            || x + width > 1.0
            || y + height > 1.0
        {
            return Err(GuiError::Io(format!(
                "Lens blur focus rect must be a positive area inside 0..=1, got {x},{y},{width},{height}"
            )));
        }
        self.lens_blur_mut().focus_rect = FocusRect {
            x: x as f32,
            y: y as f32,
            width: width as f32,
            height: height as f32,
        };
        self.mark_recipe_dirty("lens_blur.focus_rect", x + y + width + height);
        info!("GUI interaction: set_lens_blur_focus_rect -> {x},{y},{width},{height}");
        Ok(())
    }

    /// Remove the whole lens-blur stage (G-05, back to identity).
    pub fn clear_lens_blur(&mut self) {
        self.recipe.lens_blur = None;
        self.mark_recipe_dirty("lens_blur.clear", 0.0);
        info!("GUI interaction: clear_lens_blur");
    }

    /// Set the geometry rotation and record the save commit
    /// (GUI-SLIDER-SAVE-1). Public so headless/integration harnesses and
    /// future shortcuts drive the same path as the Geometry slider
    /// (GUI-ROTATE-1: one wired path, no shadow state). Arms one G-06
    /// history step consumed at save time.
    pub fn set_geometry_rotation(&mut self, degrees: f64) {
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.rotation_degrees = degrees as f32;
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty("geometry.rotation_degrees", degrees);
        self.pending_history_step = Some("geometry.rotation".into());
        info!("GUI interaction: set_geometry_rotation -> {degrees}");
    }

    /// Straighten angle (G-06, LRPAR-G06-GEO): documented alias of
    /// [`Self::set_geometry_rotation`] — same field
    /// (`geometry.rotation_degrees`), same validation, same render effect.
    /// Commits through the rotation path so slider, button and straighten
    /// control can never diverge; the history step is labelled
    /// `geometry.straighten`.
    pub fn set_straighten(&mut self, degrees: f64) {
        self.set_geometry_rotation(degrees);
        self.pending_history_step = Some("geometry.straighten".into());
        info!("GUI interaction: set_straighten -> {degrees}");
    }

    /// Set the crop to an aspect preset (G-06). Unknown names are rejected
    /// loudly without touching the recipe. Arms one G-06 history step.
    pub fn set_crop_aspect(&mut self, preset: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetCropAspect);
        let parsed = match preset {
            "original" => AspectPreset::Original,
            "1:1" => AspectPreset::OneToOne,
            "4:5" => AspectPreset::FourToFive,
            "5:4" => AspectPreset::FiveToFour,
            "3:2" => AspectPreset::ThreeToTwo,
            "2:3" => AspectPreset::TwoToThree,
            "4:3" => AspectPreset::FourToThree,
            "3:4" => AspectPreset::ThreeToFour,
            "16:9" => AspectPreset::SixteenToNine,
            "9:16" => AspectPreset::NineToSixteen,
            _ => {
                return Err(GuiError::Io(format!(
                    "Unknown aspect preset `{preset}` (expected original|1:1|4:5|5:4|3:2|2:3|4:3|3:4|16:9|9:16)"
                )));
            }
        };
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.crop = Some(Crop::Aspect { preset: parsed });
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty("geometry.crop_aspect", 0.0);
        self.pending_history_step = Some("geometry.crop_aspect".into());
        info!("GUI interaction: set_crop_aspect -> {preset}");
        Ok(())
    }

    /// Set a free crop rectangle in normalized `0..=1` coordinates (G-06).
    /// Non-finite inputs are rejected loudly without touching the recipe;
    /// deeper rect validation (positive area, frame bounds) runs at save
    /// time via the sidecar validator. Arms one G-06 history step.
    pub fn set_crop_free(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), GuiError> {
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            if !value.is_finite() {
                return Err(GuiError::Io(format!(
                    "Crop rect {name} must be finite, got {value}"
                )));
            }
        }
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.crop = Some(Crop::Free {
            x: x as f32,
            y: y as f32,
            width: width as f32,
            height: height as f32,
        });
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty("geometry.crop_free", width);
        self.pending_history_step = Some("geometry.crop_free".into());
        info!("GUI interaction: set_crop_free -> {x},{y},{width},{height}");
        Ok(())
    }

    /// Remove the crop (back to the full frame, keeps rotation/mirrors).
    /// Arms one G-06 history step.
    pub fn clear_crop(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearCrop);
        if let Some(geo) = self.recipe.geometry.as_mut() {
            geo.crop = None;
        }
        self.mark_recipe_dirty("geometry.crop_clear", 0.0);
        self.pending_history_step = Some("geometry.crop_clear".into());
        info!("GUI interaction: clear_crop");
    }

    /// Set the manual lens profile by name (G-06). Only the Core whitelist
    /// is accepted; anything else is rejected loudly without touching the
    /// recipe (the sidecar validator is the second gate at save time).
    pub fn set_lens_profile(&mut self, profile: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetLensProfile);
        if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
            return Err(GuiError::Io(format!(
                "Unknown lens profile `{profile}` (expected wide-light|tele-light|standard-neutral)"
            )));
        }
        let mut lc = self
            .recipe
            .lens_correction
            .clone()
            .unwrap_or(LensCorrection {
                version: 1,
                profile: None,
                distortion_k1: None,
                distortion_k2: None,
                distortion_k3: None,
                vignette_c0: None,
                vignette_c1: None,
                vignette_c2: None,
                ca_red: None,
                ca_blue: None,
            });
        lc.profile = Some(profile.into());
        self.recipe.lens_correction = Some(lc);
        self.mark_recipe_dirty("lens_correction.profile", 0.0);
        self.pending_history_step = Some("lens_correction.profile".into());
        info!("GUI interaction: set_lens_profile -> {profile}");
        Ok(())
    }

    /// Rotate by a relative step in degrees (GUI-ROTATE-1: the ±90° quick
    /// buttons). Normalizes into `(-180.0, 180.0]` and commits through
    /// [`Self::set_geometry_rotation`] so button, slider and (future)
    /// shortcut share one save path.
    pub fn rotate_step(&mut self, delta_degrees: f64) {
        instrument_gui_action!(self, GuiAction::RotateStep);
        let current = self
            .recipe
            .geometry
            .as_ref()
            .map(|g| f64::from(g.rotation_degrees))
            .unwrap_or(0.0);
        let mut next = (current + delta_degrees) % 360.0;
        if next <= -180.0 {
            next += 360.0;
        } else if next > 180.0 {
            next -= 360.0;
        }
        info!("GUI interaction: rotate_step {delta_degrees:+} -> {next}");
        self.set_geometry_rotation(next);
    }

    /// Set the geometry mirror flags and record the save commit
    /// (GUI-SLIDER-SAVE-1). Public for the same reason as
    /// [`Self::set_geometry_rotation`]. Arms one G-06 history step.
    pub fn set_geometry_mirror(&mut self, horizontal: bool, vertical: bool) {
        instrument_gui_action!(self, GuiAction::SetGeometryMirror);
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.mirror_horizontal = horizontal;
        geo.mirror_vertical = vertical;
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty(
            "geometry.mirror",
            f64::from(u8::from(horizontal) * 2 + u8::from(vertical)),
        );
        self.pending_history_step = Some("geometry.mirror".into());
        info!("GUI interaction: set_geometry_mirror -> h={horizontal} v={vertical}");
    }

    /// Set one perspective field (`vertical`/`horizontal`/`rotation`/`scale`/
    /// `aspect_ratio`/`shift_x`/`shift_y`) and record the save commit
    /// (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly. Arms one G-06
    /// history step.
    fn set_perspective_value(&mut self, field: &str, value: f64) {
        let mut persp = self.recipe.perspective.unwrap_or(Perspective {
            version: 1,
            vertical: 0.0,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        match field {
            "vertical" => persp.vertical = value as f32,
            "horizontal" => persp.horizontal = value as f32,
            "rotation" => persp.rotation = value as f32,
            "scale" => persp.scale = value as f32,
            "aspect_ratio" => persp.aspect_ratio = value as f32,
            "shift_x" => persp.shift_x = value as f32,
            "shift_y" => persp.shift_y = value as f32,
            _ => {
                warn!("set_perspective_value: unknown field {field}");
                return;
            }
        }
        self.recipe.perspective = Some(persp);
        self.mark_recipe_dirty(&format!("perspective.{field}"), value);
        self.pending_history_step = Some(format!("perspective.{field}"));
        info!("GUI interaction: set_perspective_value {field} -> {value}");
    }

    /// LRPAR-G06-UPRIGHT-15: run the deterministic `upright-lines-v1` analysis
    /// on the currently loaded source frame and persist it, bound to the source
    /// identity fingerprint. Enables the stage (the analysis is meant to be
    /// applied); the manual perspective stays persisted and returns on disable.
    pub fn analyze_upright_now(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::AnalyzeUpright);
        let Some(frame) = self.original.clone() else {
            return Ok(());
        };
        let source_hash = self.resolved_source_hash();
        let fingerprint = upright_input_fingerprint(
            &source_hash,
            frame.width,
            frame.height,
            self.raw_orientation,
        );
        let suggestion = analyze_upright(&frame);
        let current = self
            .recipe
            .upright
            .as_ref()
            .map(|stage| stage.enabled)
            .unwrap_or(true);
        self.recipe.upright = Some(Upright {
            version: 1,
            enabled: current,
            analysis: Some(upright_analysis(suggestion, fingerprint)),
        });
        self.mark_recipe_dirty("upright.analyze", f64::from(suggestion.confidence));
        self.pending_history_step = Some("upright.analyze".into());
        info!(
            "GUI interaction: analyze_upright_now -> lines={} confidence={:.3} \
             vertical={:.3} horizontal={:.3} rotation={:.3}",
            suggestion.line_count,
            suggestion.confidence,
            suggestion.vertical,
            suggestion.horizontal,
            suggestion.rotation
        );
        Ok(())
    }

    /// LRPAR-G06-UPRIGHT-15: apply or stop applying the persisted analysis.
    /// Enabling without an analysis is refused loudly (no silent identity
    /// render); the manual perspective is authoritative while disabled.
    pub fn set_upright_enabled(&mut self, enabled: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetUprightEnabled);
        let stage = self.recipe.upright.clone().unwrap_or(Upright {
            version: 1,
            enabled: false,
            analysis: None,
        });
        if enabled && stage.analysis.is_none() {
            return Err(GuiError::Io(
                "no persisted upright analysis; run Analyze first".into(),
            ));
        }
        let mut stage = stage;
        stage.enabled = enabled;
        self.recipe.upright = Some(stage);
        self.mark_recipe_dirty("upright.enabled", f64::from(u8::from(enabled)));
        self.pending_history_step = Some("upright.enabled".into());
        info!("GUI interaction: set_upright_enabled -> {enabled}");
        Ok(())
    }

    /// LRPAR-G06-UPRIGHT-15: remove the whole upright stage (the manual
    /// perspective, if any, becomes authoritative again).
    pub fn clear_upright(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearUpright);
        self.recipe.upright = None;
        self.mark_recipe_dirty("upright.clear", 0.0);
        self.pending_history_step = Some("upright.clear".into());
        info!("GUI interaction: clear_upright");
    }

    /// LRPAR-G06-UPRIGHT-15: `fresh`/`stale`/`none` status of the persisted
    /// analysis for the currently loaded source (visible, never a silent
    /// recompute). Pure read; the source hash is memoized.
    pub fn upright_status(&mut self) -> &'static str {
        let Some(persisted) = self
            .recipe
            .upright
            .as_ref()
            .and_then(|stage| stage.analysis.as_ref())
            .map(|analysis| analysis.fingerprint.input_fingerprint.clone())
        else {
            return "none";
        };
        let Some((width, height)) = self
            .original
            .as_ref()
            .map(|frame| (frame.width, frame.height))
        else {
            return "none";
        };
        let source_hash = self.resolved_source_hash();
        let current = upright_input_fingerprint(&source_hash, width, height, self.raw_orientation);
        if persisted == current {
            "fresh"
        } else {
            "stale"
        }
    }

    /// G-14: mark one red-eye region at normalized source coordinates (the
    /// preview picker path). The next free `re-N` id is used; the new region
    /// starts with the panel's default strengths and the persisted default
    /// radius. Loud when the 32-region cap is reached.
    ///
    /// L1: the coordinates are validated loudly against `0..=1` (finite) and
    /// never silently clipped — consistent with the pipeline's no-clipping
    /// rule. The preview picker already maps/clamps through
    /// [`Self::to_normalized`], so its clicks stay valid.
    pub fn add_red_eye_region(&mut self, x: f32, y: f32) -> Result<(), GuiError> {
        if !x.is_finite() || !(0.0..=1.0).contains(&x) {
            return Err(GuiError::Io(format!("red-eye x `{x}` out of 0..=1")));
        }
        if !y.is_finite() || !(0.0..=1.0).contains(&y) {
            return Err(GuiError::Io(format!("red-eye y `{y}` out of 0..=1")));
        }
        let mut correction = self.recipe.red_eye.clone().unwrap_or(RedEyeCorrection {
            version: 1,
            regions: Vec::new(),
        });
        if correction.regions.len() >= RED_EYE_MAX_REGIONS {
            return Err(GuiError::Io(format!(
                "red-eye region limit of {RED_EYE_MAX_REGIONS} reached"
            )));
        }
        let mut index = 1u32;
        let id = loop {
            let candidate = format!("re-{index}");
            if correction
                .regions
                .iter()
                .all(|region| region.id != candidate)
            {
                break candidate;
            }
            index += 1;
        };
        correction.regions.push(RedEyeRegion {
            id: id.clone(),
            x,
            y,
            radius: 0.05,
            desaturate: 0.8,
            darken: 0.4,
        });
        self.recipe.red_eye = Some(correction);
        self.mark_recipe_dirty("red_eye.add", f64::from(x) + f64::from(y));
        self.pending_history_step = Some("red_eye.add".into());
        info!("GUI interaction: add_red_eye_region {id} at ({x:.3},{y:.3})");
        Ok(())
    }

    /// G-14: set one field (`radius`/`desaturate`/`darken`) of one persisted
    /// region by its stable id. Unknown ids/fields are ignored loudly.
    pub fn set_red_eye_region_value(&mut self, id: &str, field: &str, value: f64) {
        let Some(correction) = self.recipe.red_eye.as_mut() else {
            warn!("set_red_eye_region_value: no red-eye stage");
            return;
        };
        let Some(region) = correction.regions.iter_mut().find(|region| region.id == id) else {
            warn!("set_red_eye_region_value: unknown region {id}");
            return;
        };
        match field {
            "radius" => region.radius = value as f32,
            "desaturate" => region.desaturate = value as f32,
            "darken" => region.darken = value as f32,
            _ => {
                warn!("set_red_eye_region_value: unknown field {field}");
                return;
            }
        }
        self.mark_recipe_dirty(&format!("red_eye.{id}.{field}"), value);
        self.pending_history_step = Some(format!("red_eye.{id}.{field}"));
        info!("GUI interaction: set_red_eye_region_value {id}.{field} -> {value}");
    }

    /// G-14: remove one persisted region by id. Unknown ids are ignored loudly.
    pub fn remove_red_eye_region(&mut self, id: &str) {
        instrument_gui_action!(self, GuiAction::RemoveRedEyeRegion);
        let Some(correction) = self.recipe.red_eye.as_mut() else {
            return;
        };
        let before = correction.regions.len();
        correction.regions.retain(|region| region.id != id);
        if correction.regions.len() == before {
            warn!("remove_red_eye_region: unknown region {id}");
            return;
        }
        self.mark_recipe_dirty("red_eye.remove", 0.0);
        self.pending_history_step = Some("red_eye.remove".into());
        info!("GUI interaction: remove_red_eye_region {id}");
    }

    /// G-14: remove the whole red-eye stage (identity).
    pub fn clear_red_eye(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearRedEye);
        self.recipe.red_eye = None;
        self.mark_recipe_dirty("red_eye.clear", 0.0);
        self.pending_history_step = Some("red_eye.clear".into());
        info!("GUI interaction: clear_red_eye");
    }

    /// LRPAR-G14-REDEYE-AUTO-15: run the deterministic, model-free pupil
    /// detection on the loaded source frame and list the candidates. This is
    /// display-only and never persists anything; the outcome text lands in
    /// `red_eye_detect_status` (visible, never silent). Applying stays explicit
    /// via [`Self::apply_detected_red_eyes`].
    pub fn detect_red_eye_candidates(&mut self) -> Result<Vec<DetectedRedEye>, GuiError> {
        instrument_gui_action!(self, GuiAction::DetectRedEye);
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        let detection = detect_red_eyes(&frame);
        let count = detection.candidates.len();
        let dropped = detection.dropped;
        info!(
            "GUI interaction: detect_red_eye_candidates -> {count} candidate(s), {dropped} dropped"
        );
        self.red_eye_detect_status = if count == 0 {
            "No red pupils detected".into()
        } else if dropped > 0 {
            format!(
                "Detected {count} pupil candidate(s); {dropped} dropped above the \
                 {RED_EYE_MAX_REGIONS}-region cap (not applied — use Apply)"
            )
        } else {
            format!("Detected {count} pupil candidate(s) (not applied — use Apply)")
        };
        Ok(detection.candidates)
    }

    /// LRPAR-G14-REDEYE-AUTO-15: persist detected pupils (explicit only). The
    /// automatic `auto-re-` regions are replaced by the fresh detection;
    /// manually marked regions are never touched. Loud when the 32-region cap
    /// would be exceeded (no silent truncation). One recipe edit + the shared
    /// debounced save/history path, exactly like [`Self::add_red_eye_region`].
    pub fn apply_detected_red_eyes(
        &mut self,
        candidates: &[DetectedRedEye],
    ) -> Result<usize, GuiError> {
        let before = self.recipe.red_eye.clone();
        let mut correction = before.clone().unwrap_or(RedEyeCorrection {
            version: 1,
            regions: Vec::new(),
        });
        correction
            .regions
            .retain(|region| !region.id.starts_with(RED_EYE_DETECT_ID_PREFIX));
        if correction.regions.len() + candidates.len() > RED_EYE_MAX_REGIONS {
            return Err(GuiError::Io(format!(
                "red-eye detection would exceed the {RED_EYE_MAX_REGIONS}-region cap: \
                 {} manual region(s) + {} detected",
                correction.regions.len(),
                candidates.len()
            )));
        }
        for candidate in candidates {
            let region = candidate.to_region();
            match correction
                .regions
                .iter_mut()
                .find(|existing| existing.id == region.id)
            {
                Some(existing) => *existing = region,
                None => correction.regions.push(region),
            }
        }
        let after = if correction.regions.is_empty() {
            None
        } else {
            Some(correction)
        };
        if before == after {
            self.red_eye_detect_status = format!(
                "No change: {} auto region(s) already applied",
                candidates.len()
            );
            return Ok(0);
        }
        self.recipe.red_eye = after;
        self.mark_recipe_dirty("red_eye.detect_apply", candidates.len() as f64);
        self.pending_history_step = Some("red_eye.detect_apply".into());
        self.red_eye_detect_status = format!("Applied {} detected pupil(s)", candidates.len());
        info!(
            "GUI interaction: apply_detected_red_eyes -> {} region(s)",
            candidates.len()
        );
        Ok(candidates.len())
    }

    /// GUI-INSTRDBG-17b-REST: the red-eye "Apply detected" button command. One
    /// outer instrumented action around detection + persistence; the nested
    /// `detect_red_eye_candidates` must not add a second log line (depth guard).
    fn apply_detected_red_eye_objects(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyDetectedRedEyes);
        let candidates = self.detect_red_eye_candidates()?;
        self.apply_detected_red_eyes(&candidates)
    }

    pub fn auto_tone(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::AutoTone);
        if self.original.is_none() {
            return Ok(());
        }
        // G-16: single shared evaluation path (see `compute_auto_tone`) —
        // `apply_auto_endpoint` reuses exactly this algorithm + fingerprint.
        let (result, input_fingerprint) = self.compute_auto_tone()?;
        // AUTO-TONE-2: all six sliders persist 1:1 into `recipe.adjustments`
        // (domains match the sidecar validation: exposure ±10 EV, the other
        // five `-1..=1`).
        for (key, value) in [
            ("exposure", result.exposure),
            ("contrast", result.contrast),
            ("whites", result.whites),
            ("blacks", result.blacks),
            ("highlights", result.highlights),
            ("shadows", result.shadows),
        ] {
            self.recipe.adjustments.insert(key.into(), value);
        }
        self.recipe.auto_features.enable_auto_tone = true;
        self.recipe.auto_features.auto_exposure = Some(result.exposure);
        self.recipe.auto_features.auto_contrast = Some(result.contrast);
        // AUTO-TONE-2: the four end/balance mirrors mark these adjustments as
        // auto-written (parallel to `adjustments`); `clear_stale_auto_tone`
        // uses them to tell auto values apart from manual edits.
        self.recipe.auto_features.auto_whites = Some(result.whites);
        self.recipe.auto_features.auto_blacks = Some(result.blacks);
        self.recipe.auto_features.auto_highlights = Some(result.highlights);
        self.recipe.auto_features.auto_shadows = Some(result.shadows);
        self.recipe.auto_features.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint,
            extras: BTreeMap::new(),
        });
        // GUI-AUTOTONE-SAVE-1: record the save commit so the debounced path
        // (`commit_pending_slider_save`) persists the sidecar (CAS, loud
        // conflicts) with an INFO log — same as GUI-SLIDER-SAVE-1. The
        // exposure is the log representative (contrast persists alongside).
        // GUI-SIDECAR-READ-1: commit synchronously (render + save + log) — a
        // bare `render()` would clear `pending_full_render` while the commit
        // stays armed, stranding the save until an unrelated later edit
        // (N6: `auto_tone saved` only fired via a later pan).
        self.mark_recipe_dirty("auto_tone", result.exposure);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    pub fn match_total_exposure(&mut self, target: f64) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::MatchExposure);
        // REVIEW-GUI-N5: never measure a draft. If the preview is currently a
        // low-resolution drag draft, commit the pending full-quality render
        // first so the measurement domain is the final visible render.
        if self.preview_is_draft {
            self.render_full([0, 0], None)?;
        }
        let Some(frame) = &self.preview else {
            return Ok(());
        };
        debug_assert!(
            !self.preview_is_draft,
            "measurement must run on the full render, never a draft"
        );
        // F-041: measure the final visible domain — the rendered preview
        // (post crop/geometry, same frame that is displayed) weighted by the
        // effective mask planes of the last render; the empty slice keeps the
        // raster measurement when no layers exist.
        let mask_planes: Vec<MaskPlane> = self
            .render_mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        let value = match_total_exposure_masked(frame, target, &mask_planes)?;
        let exposure = self
            .recipe
            .adjustments
            .get("exposure")
            .copied()
            .unwrap_or(0.0)
            + value;
        self.recipe.adjustments.insert("exposure".into(), exposure);
        self.recipe.auto_features.match_total_exposure = true;
        self.recipe.auto_features.target_luminance = target;
        self.recipe.auto_features.matched_exposure = Some(value);
        // GUI-AUTOTONE-SAVE-1: record the save commit so the debounced path
        // (`commit_pending_slider_save`) persists the sidecar (CAS, loud
        // conflicts) with an INFO log — same as GUI-SLIDER-SAVE-1. Zoom/pan
        // view state is never recorded here; it stays GUI session state.
        // GUI-SIDECAR-READ-1: commit synchronously (see `auto_tone`) — a bare
        // `render()` would strand the armed commit (N6 lost-edit class).
        self.mark_recipe_dirty("match_total_exposure", value);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    pub fn reset(&mut self) {
        instrument_gui_action!(self, GuiAction::Reset);
        self.recipe = EditRecipe::default();
        // GUI-SIDECAR-READ-1: the recipe was replaced wholesale — a commit
        // armed by a pre-reset edit is stale (its `<key>=<value> saved` log
        // would misattribute the reset state). Drop it; the reset itself
        // re-renders below and persists on the next committed edit.
        self.pending_slider_commit = None;
        self.pending_history_step = None;
        if self.original.is_some() {
            // F4/F7: surface a render failure instead of discarding it.
            if let Err(error) = self.render() {
                self.show_error(error);
            }
        }
    }

    /// Compute the source ROI `(x, y, w, h)` that is visible in the preview
    /// pane for a zoom factor `> 1.0` (PERF-GUI-5, REVIEW-GUI-PANROI-1).
    ///
    /// The visible window follows the pan offset: the drawn image centre sits
    /// at `pane.center() + pan`, so the source point currently behind the pane
    /// centre is the image centre shifted by `-pan / scale` where
    /// `scale = fit(w, h) * zoom` is the on-screen scale (screen points per
    /// source pixel). The returned rect is that window expanded by
    /// [`PREVIEW_ROI_MARGIN`] so the hand tool always has off-screen content
    /// to drag without an immediate re-render, and it is clamped to the image
    /// bounds — which is what makes borders/corners reachable at any zoom.
    ///
    /// Returns `None` at fit/zoom-out or when the window already covers the
    /// whole frame, so the entire image is rendered.
    fn roi_from_zoom(
        w: u32,
        h: u32,
        zoom: f32,
        pan: egui::Vec2,
        pane_w: f32,
        pane_h: f32,
    ) -> Option<[u32; 4]> {
        if zoom <= 1.0 || w == 0 || h == 0 {
            return None;
        }
        let (w, h) = (f64::from(w), f64::from(h));
        let (pane_w, pane_h) = (f64::from(pane_w), f64::from(pane_h));
        let fit = (pane_w / w).min(pane_h / h);
        if fit <= 0.0 {
            return None;
        }
        let scale = f64::from(zoom) * fit;
        // Visible window in source pixels, with margin for panning headroom.
        let vw = pane_w / scale * PREVIEW_ROI_MARGIN;
        let vh = pane_h / scale * PREVIEW_ROI_MARGIN;
        if vw >= w || vh >= h {
            return None;
        }
        let zw = (vw.floor() as u32).clamp(1, w as u32);
        let zh = (vh.floor() as u32).clamp(1, h as u32);
        // Source point under the pane centre (window centre), clamped so the
        // window never leaves the frame.
        let cx = w / 2.0 - f64::from(pan.x) / scale;
        let cy = h / 2.0 - f64::from(pan.y) / scale;
        let x = ((cx - vw / 2.0).floor() as i64).clamp(0, (w as u32 - zw) as i64) as u32;
        let y = ((cy - vh / 2.0).floor() as i64).clamp(0, (h as u32 - zh) as i64) as u32;
        Some([x, y, zw, zh])
    }

    /// Effective texture dimensions in *full-source* pixels (GUI-DRAFT-JUMP-1).
    ///
    /// A draft texture lives in downscaled render-source space, so drawing it
    /// at `tex_dims · scale` (full-source scale) comes out too small; scaling
    /// by `full/render_src` per axis restores the exact on-screen size of the
    /// equivalent full render. Full renders (`render_src == full`) and the
    /// `None` legacy state pass the dimensions through unchanged. Pure helper
    /// so the draft-vs-full placement roundtrip is unit-testable headless.
    fn preview_draw_dims(
        tex_w: f32,
        tex_h: f32,
        full_w: f32,
        full_h: f32,
        render_src: Option<(u32, u32)>,
    ) -> (f32, f32) {
        match render_src {
            Some((sw, sh)) if sw > 0 && sh > 0 && full_w > 0.0 && full_h > 0.0 => {
                (tex_w * full_w / sw as f32, tex_h * full_h / sh as f32)
            }
            _ => (tex_w, tex_h),
        }
    }

    /// Translate a render-source-space ROI into full-source pixels
    /// (GUI-DRAFT-JUMP-1): the inverse of the draft downscale, so pointer→
    /// source mapping and the mask overlay consume full-space rects regardless
    /// of which path rendered the texture. A `None`/degenerate source passes
    /// the rect through; results are clamped to the full frame. Pure helper
    /// for the headless draft-vs-full geometry test.
    fn roi_in_full_pixels(
        roi: [u32; 4],
        full_w: u32,
        full_h: u32,
        render_src: Option<(u32, u32)>,
    ) -> [u32; 4] {
        match render_src {
            Some((sw, sh)) if sw > 0 && sh > 0 && full_w > 0 && full_h > 0 => {
                let sx = full_w as f64 / sw as f64;
                let sy = full_h as f64 / sh as f64;
                let x = ((roi[0] as f64 * sx).round() as u32).min(full_w - 1);
                let y = ((roi[1] as f64 * sy).round() as u32).min(full_h - 1);
                let w = ((roi[2] as f64 * sx).round() as u32).clamp(1, full_w - x);
                let h = ((roi[3] as f64 * sy).round() as u32).clamp(1, full_h - y);
                [x, y, w, h]
            }
            _ => roi,
        }
    }

    /// Whether a pan gesture (modifier-free wheel over the preview, hand-tool
    /// drag) may pin the zoom mode to `Custom` (GUI-ZOOM-CUSTOM-1, F-100):
    /// only when actually zoomed in (`zoom > 1.0`) AND the drawn image
    /// overflows the pane. At Fit (or zoomed out) there is nothing to pan,
    /// so the gesture must never flip the readout to `Custom` — `Custom`
    /// arises solely from explicit zoom/pan of a magnified view. Pure helper
    /// so the Fit-guard is unit-testable headless.
    fn pan_gesture_pins_custom(
        zoom: f32,
        draw_w: f32,
        draw_h: f32,
        pane_w: f32,
        pane_h: f32,
    ) -> bool {
        zoom > 1.0 && (draw_w > pane_w + 0.5 || draw_h > pane_h + 0.5)
    }

    /// Open or collapse the navigator rail (GUI-PREVIEW-NAV-1). Pure view
    /// state — never touches the recipe or the sidecar.
    pub fn set_navigator_open(&mut self, open: bool) {
        trace!("GUI interaction: set_navigator_open {}", open);
        self.navigator_open = open;
    }

    /// Switch the preview zoom mode. Non-`Custom` modes re-derive `preview_zoom`
    /// from the current pane each frame (so they survive resizes); switching
    /// always re-centres the pan and triggers a re-render so the ROI crop
    /// matches the on-screen zoom. The re-render replaces the stale texture
    /// (GUI-FIT-1 texture-ROI identity: a Custom crop texture is never valid
    /// under Fit — `mark_dirty` arms its replacement, and `draw_preview`
    /// neutralizes any stale pan in non-`Custom` modes until it lands).
    pub fn set_zoom_mode(&mut self, mode: ZoomMode) {
        instrument_gui_action!(self, GuiAction::SetZoomMode);
        trace!("GUI interaction: set_zoom_mode {:?}", mode);
        self.zoom_mode = mode;
        self.preview_pan = egui::Vec2::ZERO;
        self.mark_dirty();
        // PREVIEW-CACHE-FEATURE (A6): a zoom-mode change to 1:1 (or back to Fit)
        // changes the neighbor preview kind/resolution; the current source's
        // cached neighbors no longer match the new key, so the +4/−2 window is
        // re-planned at the new resolution. Only native (the neighbor cache is a
        // native capability); no-op while nothing is loaded.
        if !self.path.is_empty() && self.original.is_some() {
            let active = self.path.clone();
            self.schedule_neighbor_previews(&active);
        }
    }

    /// Scroll-wheel / keyboard continuous zoom (relative-to-fit multiplier).
    /// Pins the mode to `Custom` so the next frame does not re-derive
    /// `preview_zoom`.
    pub fn zoom_step(&mut self, factor: f32) {
        let next = (self.preview_zoom * factor).clamp(0.05, 32.0);
        trace!(
            "GUI interaction: zoom_step factor={:.3} -> {:.3}",
            factor,
            next
        );
        self.preview_zoom = next;
        self.zoom_mode = ZoomMode::Custom;
        self.mark_dirty();
    }

    /// Re-derive `preview_zoom` (and reset pan) for non-`Custom` modes using
    /// the pane geometry and **un-cropped source dimensions** cached by the
    /// previous [`Self::draw_preview`] (REVIEW-GUI-ZOOMLOOP-1: deriving from
    /// the ROI-cropped texture's fit scale made 100%/200%/Fit-Width wrong and
    /// oscillate frame-by-frame). Called once per frame before the render logic
    /// so the ROI crop matches the on-screen zoom even on the frame a mode
    /// button/shortcut is pressed.
    fn sync_zoom(&mut self) {
        use ZoomMode::*;
        if self.zoom_mode == Custom {
            return;
        }
        // Fit of the pane against the un-cropped source, not the current
        // (possibly ROI-cropped) texture.
        let fit = self.preview_base_fit_scale.max(1e-6);
        let src_w = self.preview_src_w.max(1.0);
        self.preview_pan = egui::Vec2::ZERO;
        self.preview_zoom = match self.zoom_mode {
            Fit => 1.0,
            Quarter => 0.25 / fit,
            Half => 0.5 / fit,
            ThreeQuarter => 0.75 / fit,
            OneToOne => 1.0 / fit,
            TwoHundred => 2.0 / fit,
            FitWidth => (self.preview_pane_w / src_w) / fit,
            Custom => 1.0,
        };
    }

    /// Viewport rectangle for the navigator overview (GUI-PREVIEW-NAV-1): the
    /// currently visible Develop working area mapped into `nav_rect`, which
    /// shows the whole source (`src_w × src_h`) contain-fitted without
    /// letterboxing (the caller sizes it to the source aspect).
    ///
    /// `scale` is the on-screen preview scale (screen points per source pixel)
    /// and `pan` the preview pan offset. At fit (or degenerate geometry) the
    /// whole frame is visible and the returned rect equals `nav_rect`. Pure
    /// helper so the pan-rectangle roundtrip is unit-testable headless.
    fn navigator_viewport_rect(
        nav_rect: egui::Rect,
        src_w: f32,
        src_h: f32,
        pane_w: f32,
        pane_h: f32,
        scale: f32,
        pan: egui::Vec2,
    ) -> egui::Rect {
        if src_w <= 0.0 || src_h <= 0.0 || scale <= 0.0 || pane_w <= 0.0 || pane_h <= 0.0 {
            return nav_rect;
        }
        // Visible window in source pixels, centred on the source point behind
        // the pane centre (mirrors `roi_from_zoom` without the pan margin).
        let vw = (pane_w / scale).min(src_w);
        let vh = (pane_h / scale).min(src_h);
        if vw >= src_w && vh >= src_h {
            return nav_rect;
        }
        let cx = (src_w / 2.0 - pan.x / scale).clamp(vw / 2.0, src_w - vw / 2.0);
        let cy = (src_h / 2.0 - pan.y / scale).clamp(vh / 2.0, src_h - vh / 2.0);
        let to_nav_x = |x: f32| nav_rect.min.x + x / src_w * nav_rect.width();
        let to_nav_y = |y: f32| nav_rect.min.y + y / src_h * nav_rect.height();
        egui::Rect::from_min_max(
            egui::pos2(to_nav_x(cx - vw / 2.0), to_nav_y(cy - vh / 2.0)),
            egui::pos2(to_nav_x(cx + vw / 2.0), to_nav_y(cy + vh / 2.0)),
        )
    }

    /// Map a navigator drag onto the preview pan offset (GUI-PREVIEW-NAV-1):
    /// dragging the viewport rectangle by `drag_nav` (navigator points) moves
    /// the visible window with the cursor. `nav_scale` is navigator points per
    /// source pixel, `preview_scale` the on-screen preview scale. Pure helper
    /// for the headless pan-rectangle roundtrip test.
    fn pan_for_navigator_drag(
        pan: egui::Vec2,
        drag_nav: egui::Vec2,
        nav_scale: f32,
        preview_scale: f32,
    ) -> egui::Vec2 {
        if nav_scale <= 0.0 || preview_scale <= 0.0 {
            return pan;
        }
        pan - drag_nav * (preview_scale / nav_scale)
    }

    /// Composite zdata tile-record id shared with the CLI (REVIEW-CLI-N1):
    /// `"{copy_id}/{mask_id}"`. The field order mirrors the
    /// `(copy_id, mask_id)` planes key of `MaskContext`.
    fn zdata_tile_record_id(copy_id: &str, mask_id: &str) -> String {
        format!("{copy_id}/{mask_id}")
    }

    /// Surface a user-visible failure loudly (DoD §4): the message goes to the
    /// log at `error!` level, the status line switches to "Error" and the
    /// message is shown both in the header and in the error popup dialog
    /// (KITTEST-COVERAGE-STATES-1). Explicit user-action failures pop the
    /// dialog; background failures use [`Self::show_error_banner`].
    fn show_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        error!("{message}");
        self.status = Str::Error.t().into();
        self.error = Some(message);
        self.error_dialog = true;
    }

    /// Background/automatic failures (decode, listing) stay a header banner +
    /// log without stealing focus with a dialog; the loud signal is not lost
    /// (status + `self.error` + `error!`), only the modal surface is skipped.
    fn show_error_banner(&mut self, error: impl ToString) {
        let message = error.to_string();
        error!("{message}");
        self.status = Str::Error.t().into();
        self.error = Some(message);
        self.error_dialog = false;
    }

    /// KITTEST-COVERAGE-STATES-1: the error popup dialog. Drawn as a floating
    /// window from `update` (like the toast and the meta-preset dialog) so an
    /// explicit-action failure is visible as a dialog and in the log — not only
    /// as a header tint. `self.error` is the message source; the Close button
    /// closes the dialog (the header banner stays until the next success).
    fn draw_error_dialog(&mut self, ctx: &egui::Context) {
        if !self.error_dialog {
            return;
        }
        let Some(message) = self.error.clone() else {
            self.error_dialog = false;
            return;
        };
        let mut close = false;
        egui::Window::new(Str::Error.t())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -80.0))
            .show(ctx, |ui| {
                ui.label(&message);
                if ui.button(Str::ErrorDialogClose.t()).clicked() {
                    close = true;
                }
            });
        if close {
            self.close_error_dialog();
        }
    }

    /// KITTEST-COVERAGE-STATES-2 (c): close the error popup dialog. A
    /// user-visible action (DoD §4) is logged at `info!`; the header banner /
    /// `self.error` stays until the next success. Split out so the log level
    /// and state transition are directly reviewable/testable.
    fn close_error_dialog(&mut self) {
        if self.error_dialog {
            info!("error dialog closed");
        }
        self.error_dialog = false;
    }

    // ---- PERF-GUI-7: asynchronous (off-main-thread) file decode ----
    //
    // `begin_load_path` starts a background decode and returns immediately so
    // switching files never freezes the UI. The decoded frame is delivered via
    // `decode_rx` and applied on the main thread by `poll_decode` (driven from
    // `update`). `is_supported_image` keeps the RAW-only / raster filter.
    /// Start a background decode of `path`. The previous preview stays on screen
    /// until the decoded frame arrives; failures are surfaced via `show_error`.
    fn begin_load_path(&mut self, path: String) {
        if path.trim().is_empty() {
            return;
        }
        self.note_decode_start(&path);
        // R3-OPEN-1: remember which path this decode targets so the
        // Develop-switch open can reuse it instead of starting a duplicate.
        self.pending_load_path = Some(path.clone());
        self.status = format!(
            "Decoding {}",
            Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
        );
        // PREVIEW-CACHE-FEATURE (A1/A4): if the target being navigated to was
        // already prepared as a neighbor preview (change-of-active, same session
        // or earlier prefetch), paint it immediately — RAM-LRU or disk hit —
        // so the first frame shows no decode/render wait. The full-resolution
        // decode below still runs in the background and `finish_decode` replaces
        // this with the full render; a miss keeps the standard loading path.
        self.paint_cached_neighbor_preview(&path);
        let (tx, rx) = std::sync::mpsc::channel();
        self.decode_rx = Some(rx);
        std::thread::spawn(move || {
            let result: DecodeResult = (|| {
                let p = std::path::PathBuf::from(&path);
                let bytes = std::fs::read(&p)
                    .map_err(|e| (path.clone(), format!("{}: {}", p.display(), e)))?;
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image")
                    .to_string();
                let source_is_raw = is_raw_name(&name);
                let (frame, orientation, camera_white_balance, lens_identity) = if source_is_raw {
                    let image = lumina_raw::decode_bytes(&bytes, &name)
                        .map_err(|e| (path.clone(), e.to_string()))?;
                    let wb = image.metadata.camera_white_balance;
                    let camera_white_balance = if wb.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                        warn!(
                            "As-Shot white balance invalid {:?} for {} — dropping to None",
                            wb, name
                        );
                        None
                    } else {
                        Some(wb)
                    };
                    let lens_identity = lens_identity_from_metadata(&image.metadata);
                    let orientation = image.metadata.orientation;
                    (
                        image.frame,
                        orientation,
                        camera_white_balance,
                        lens_identity,
                    )
                } else {
                    (
                        ImageFrame::decode(&bytes).map_err(|e| (path.clone(), e.to_string()))?,
                        1,
                        None,
                        None,
                    )
                };
                Ok(DecodedFrame {
                    path,
                    name,
                    bytes,
                    frame,
                    orientation,
                    camera_white_balance,
                    source_is_raw,
                    lens_identity,
                })
            })();
            let _ = tx.send(result);
        });
    }

    /// Apply a completed background decode: set the source, then restore the
    /// sidecar recipe for that path (mirroring the old synchronous `load_path`).
    fn finish_decode(&mut self, result: DecodeResult) {
        // R3-OPEN-1: the in-flight decode is settled either way.
        self.pending_load_path = None;
        match result {
            Ok(frame) => {
                self.note_decode_finish(frame.frame.width, frame.frame.height);
                // GUI-SIDECAR-READ-1: edits made while this decode was in
                // flight target the still-loaded image — flush them to its
                // path now, before the new path is adopted below (a flush
                // afterwards would write the old recipe under the new path).
                self.flush_pending_edit();
                // LRPAR-G08-PREVIOUS: a successful switch to a different
                // image displaces the current one — stash it (path + recipe
                // snapshot) as the cross-image Previous reference before the
                // new path is adopted below. Same-path reloads and switches
                // with nothing loaded leave the reference untouched, so
                // "no previously edited image" stays a loud error instead of
                // silently applying defaults.
                let displaced = (!self.path.trim().is_empty()
                    && self.path != frame.path
                    && self.original.is_some())
                .then(|| PreviousReference {
                    path: self.path.clone(),
                    recipe: self.recipe.clone(),
                });
                self.path = frame.path.clone();
                if let Some(reference) = displaced {
                    info!(
                        "previous reference: {} (displaced by {})",
                        reference.path, self.path
                    );
                    self.previous_reference = Some(reference);
                }
                // PREVIEW-CACHE-FEATURE: the active image just changed — plan
                // the +4/−2 neighbor window around it (lazy, on workers).
                let active_path = self.path.clone();
                self.schedule_neighbor_previews(&active_path);
                self.apply_decoded_frame(
                    &frame.frame,
                    frame.orientation,
                    frame.camera_white_balance,
                    &frame.name,
                    &frame.bytes,
                    frame.source_is_raw,
                    frame.lens_identity,
                );
                if let Err(e) = self.render() {
                    error!("render after load failed for {}: {e}", self.source_name);
                    self.show_error(e);
                }
                let path = std::path::PathBuf::from(self.path.trim());
                if let Ok(document) =
                    lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&path))
                {
                    // REVIEW-GUI-N1: remember the revision this document was
                    // loaded from — the save path compares against it (CAS).
                    self.sidecar_revision = lumina_sidecar::document_revision(&document).ok();
                    // REVIEW-GUI-N2: resolve the copy by identity, never
                    // positionally. `apply_decoded_frame` reset the session to
                    // the default copy id ("vc-original"); prefer that id,
                    // then the document's default copy, then the first entry —
                    // and adopt whichever id actually resolved so subsequent
                    // edits/saves target the same copy even when the JSON
                    // array was reordered.
                    let resolved = document
                        .virtual_copies
                        .iter()
                        .find(|copy| copy.id == self.virtual_copy_id)
                        .or_else(|| document.virtual_copies.iter().find(|copy| copy.is_default))
                        .or_else(|| document.virtual_copies.first())
                        .cloned();
                    if let Some(copy) = resolved {
                        let candidate = copy.recipe.clone();
                        self.virtual_copy_id = copy.id.clone();
                        self.selected_mask_id = copy
                            .mask_layers
                            .first()
                            .map(|layer| layer.mask.mask_id.clone());
                        self.document = Some(document);
                        let config = AutoToneConfig {
                            target_luminance: candidate.auto_features.target_luminance,
                            ..Default::default()
                        };
                        let fingerprint =
                            tone_fingerprint(self.original.as_ref().expect("loaded frame"), config);
                        let valid = candidate
                            .auto_features
                            .analysis_fingerprint
                            .as_ref()
                            .is_some_and(|stored| is_current_tone_analysis(stored, &fingerprint));
                        self.recipe = candidate;
                        // LRPAR-G01-BASIC: the persisted recipe is the new
                        // Previous baseline (panel-Previous = last saved state).
                        self.capture_section_baselines();
                        // G04-FOLLOWUP-1: the session detect input defaults
                        // to the recipe visualize threshold (else 0.5) so the
                        // panel slider shows the restored default after reload.
                        self.spot_detect_threshold =
                            self.recipe.spot_visualize_threshold().unwrap_or(0.5);
                        let stale_auto_tone = self.recipe.auto_features.enable_auto_tone && !valid;
                        if stale_auto_tone {
                            clear_stale_auto_tone(&mut self.recipe);
                            self.status = Str::AutoToneStale.t().into();
                        }
                        if let Err(error) = self.render() {
                            self.show_error(error);
                        } else if stale_auto_tone {
                            self.status = Str::AutoToneStale.t().into();
                        }
                    } else {
                        // A sidecar without any virtual copy cannot be
                        // rendered from; surface it instead of silently
                        // keeping an unrelated recipe.
                        warn!("sidecar for {} has no virtual copies", path.display());
                        self.show_error(GuiError::Io(Str::VirtualCopyNotFound.t().to_string()));
                    }
                }
            }
            Err((path, message)) => {
                self.note_decode_failed();
                // REVIEW-GUI-PATHDESYNC-1: a failed decode must NOT adopt the
                // new path — original/document/recipe still belong to the
                // previously loaded image, so writes would otherwise produce a
                // phantom sidecar under a path that never loaded. Surface the
                // failure visibly instead. Background decode failures stay a
                // header banner + log (KITTEST-COVERAGE-STATES-1: the popup
                // dialog is reserved for explicit user actions, so browsing a
                // folder of corrupt files cannot stack dialogs).
                // KITTEST-COVERAGE-STATES-2: `show_error_banner` already logs
                // at `error!`; the previous explicit `error!` here duplicated
                // the same failure, so the contextual prefix moved into the
                // single banner message.
                self.show_error_banner(GuiError::Io(format!(
                    "background decode failed for {path}: {message}"
                )));
            }
        }
    }

    /// Drain any completed background decode (PERF-GUI-7). Called every frame
    /// from `update` so the UI stays responsive while decoding.
    fn poll_decode(&mut self) {
        let Some(rx) = &self.decode_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.decode_rx = None;
                self.finish_decode(result);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.decode_rx = None;
                // R3-OPEN-1: a dropped decode sender must not leave the
                // in-flight anchor behind (it would suppress a later open).
                self.pending_load_path = None;
            }
        }
    }

    // ---- PREVIEW-CACHE-FEATURE: neighbor-preview prefetch (native) ----
    //
    // The active image is always the full GPU/CPU texture; the neighbors in the
    // +4/−2 window are prepared as WebP previews on background workers and kept
    // in a RAM LRU + disk tier (see `feature/quality/preview-cache.md` and the
    // `lumina_core::preview_cache` primitives). A miss is a *visible*
    // preparation state, never a silently wrong/upscaled fallback.

    /// Drain completed neighbor-preview results and request a repaint when work
    /// arrived. Non-blocking; called every frame from `update`.
    fn poll_neighbor_previews(&mut self, ctx: &egui::Context) {
        let Some(ctrl) = self.preview_ctrl.as_mut() else {
            return;
        };
        let before = ctrl.lru().len();
        ctrl.poll();
        let ready = ctrl.lru().len().saturating_sub(before);
        self.frame_previews_ready += ready;
        // PREVIEW-CACHE-FEATURE: worker failures are never swallowed — they are
        // surfaced visibly (the neighbor-preview cell UI shows them via the
        // probe → message mapping) and logged here for the current slice.
        let mut failure_count = 0;
        for (probe, message) in ctrl.drain_failures() {
            warn!("neighbor preview failed for {probe}: {message}");
            timing::note_neighbor_failure_warn();
            failure_count += 1;
        }
        // A2: a ready frame or a (visible) failure changes per-cell badges — the
        // next frame must redraw the navigator cells.
        if ready > 0 || failure_count > 0 {
            ctx.request_repaint();
        }
        // GUI-TOAST-OVERLAP-1: a batch of freshly prepared neighbor previews
        // raises the transient overlay toast (coalesced while one is
        // visible) instead of a persistent per-cell badge over a thumbnail.
        if ready > 0 {
            let now = ctx.input(|i| i.time);
            self.show_toast(Str::ToastPreviewReady.t().to_string(), now);
        }
    }

    /// PREVIEW-CACHE-FEATURE (A1/A4): paint a cached neighbor preview for the
    /// path being navigated to, so the first frame of a change-of-active shows
    /// no decode/render wait. Serves first from the RAM LRU, then from the disk
    /// tier; a miss is a genuine miss (the standard lazy loading path applies —
    /// never a silently wrong/upscaled image).
    /// Adopt a cached neighbor frame as the *transient* main preview
    /// (GUI-PREVIEW-NOISE-1): a neighbor frame is a low-resolution stand-in
    /// rendered with the neighbor window's recipe — never the committed render
    /// of this image. It is therefore bookkept as a draft: the placement math
    /// (`preview_render_src`), the HUD draft badge (`preview_is_draft`) and
    /// the derived analysis state (tone/histogram/render key) all describe
    /// this frame honestly until `finish_decode` replaces it with the full
    /// render. Painting it as a "current" full render instead showed an
    /// upscaled low-res image with a "Preview current" status while the
    /// navigator thumbnail (separate pipeline) stayed correct.
    fn adopt_neighbor_preview_frame(&mut self, frame: ImageFrame) {
        let (width, height) = (frame.width, frame.height);
        self.preview = Some(frame);
        self.preview_generation += 1;
        // Force `update_texture` to (re-)upload the new pixels this frame.
        self.texture_identity = None;
        self.preview_render_src = Some((width, height));
        self.preview_roi = None;
        self.preview_is_draft = true;
        self.tone_analysis = None;
        self.preview_histogram = None;
        self.render_key = None;
        self.render_mask_layers.clear();
        // GUI-LENSFUN-GATE-4: a present refusal captured for the *previous*
        // frame/recipe is not known to apply to this adopted stand-in — the
        // next `update_texture` would otherwise surface a transient, stale
        // CPU-routing badge. The neighbor pipeline never runs `render_to_vram`,
        // so no fresh refusal can be produced here either.
        #[cfg(feature = "gpu")]
        {
            self.vram_render_refusal = None;
        }
    }

    fn paint_cached_neighbor_preview(&mut self, path: &str) {
        let canonical = Path::new(path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .into_owned();
        let source = PathBuf::from(path);
        // Ask the controller for the cached frame; the borrow on `preview_ctrl`
        // ends here so the assignment to `self.preview` below is allowed.
        let cached = self.preview_ctrl.as_mut().and_then(|ctrl| {
            match ctrl.neighbor_preview(&canonical, &source) {
                Ok(Some(frame)) => Some(frame),
                Ok(None) => None,
                Err(message) => {
                    // A cache read failure is surfaced, not silently swallowed
                    // (no silent fallback): it still proceeds with the real decode,
                    // but the neighbour-preview error is logged visibly.
                    log::warn!("neighbor preview cache read failed for {path}: {message}");
                    None
                }
            }
        });
        if let Some(frame) = cached {
            log::debug!("neighbor preview cache-hit, painting immediately: {path}");
            self.adopt_neighbor_preview_frame(frame);
        }
    }

    /// Whether the overlay toast is currently visible at egui-time `now`
    /// (GUI-TOAST-OVERLAP-1). Pure so the show/dismiss/timeout state machine
    /// is unit-testable headless without an event loop.
    pub fn toast_visible(&self, now: f64) -> bool {
        self.toast_message.is_some() && now <= self.toast_until
    }

    /// Show the overlay toast until `now + TOAST_TIMEOUT_SECONDS`
    /// (GUI-TOAST-OVERLAP-1). A visible toast is never stacked — the call is
    /// a no-op while one is showing, so a burst of background completions
    /// produces a single transient notice instead of a queue.
    pub fn show_toast(&mut self, message: String, now: f64) {
        if self.toast_visible(now) {
            return;
        }
        info!("toast: {message}");
        self.toast_message = Some(message);
        self.toast_until = now + TOAST_TIMEOUT_SECONDS;
    }

    /// Manually dismiss the overlay toast (its ✕ button).
    pub fn dismiss_toast(&mut self) {
        info!("GUI interaction: toast dismissed");
        self.toast_message = None;
        self.toast_until = 0.0;
    }

    /// Fixed toast anchor for a `viewport` (GUI-TOAST-OVERLAP-1):
    /// top-center over the preview canvas, just below the preview-area header
    /// (zoom toolbar). Every chrome row at the top (header, module bar, panel
    /// headers incl. the histogram header) is occupied, so a toast anchored at
    /// bar height inevitably covers clickable chrome; the canvas below the
    /// preview header is the only region without controls. The toast still
    /// covers photo pixels while visible, but it is transient (4 s timeout +
    /// ✕ dismiss) and blocks no panel chrome. Pure so the placement is
    /// unit-testable headless.
    fn toast_anchor(viewport: egui::Vec2) -> egui::Pos2 {
        egui::pos2((viewport.x * 0.5 - 150.0).max(0.0), 100.0)
    }

    /// Expire the toast past its deadline and keep a visible one alive across
    /// frames by scheduling the repaint exactly at its deadline
    /// (GUI-TOAST-OVERLAP-1): without the timed repaint egui would sleep and
    /// the toast would linger until the next unrelated input.
    fn update_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if self.toast_message.is_none() {
            return;
        }
        if self.toast_visible(now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                (self.toast_until - now).max(0.0),
            ));
        } else {
            trace!("toast auto-dismissed after timeout");
            self.dismiss_toast();
        }
    }

    /// Draw the transient overlay toast in its own [`egui::Area`]
    /// (GUI-TOAST-OVERLAP-1): an overlay takes no layout width, so it can
    /// neither shift nor cover thumbnails the way the old in-cell badge did.
    /// The ✕ button dismisses it immediately.
    /// KITTEST-COVERAGE-STATES-2 (e): the toast paints its own popup
    /// background — since the anchor moved over the preview canvas, bare text
    /// would be unreadable against bright photo content.
    fn draw_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if !self.toast_visible(now) {
            return;
        }
        let message = self.toast_message.clone().unwrap_or_default();
        let viewport = ctx.input(|i| i.viewport_rect().size());
        let mut dismissed = false;
        egui::Area::new(egui::Id::new("lumina-toast"))
            .fixed_pos(Self::toast_anchor(viewport))
            .order(egui::Order::Foreground)
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(&message);
                        if ui.button(Str::ToastDismiss.t()).clicked() {
                            dismissed = true;
                        }
                    });
                });
            });
        if dismissed {
            self.dismiss_toast();
        }
    }

    /// PREVIEW-CACHE-FEATURE (A2): a visible badge (label + color) for a source's
    /// neighbor-preview state, or `None` when no state applies (e.g. the probe
    /// was consumed/active). Maps the controller's per-probe state to a cell
    /// overlay so „wird vorbereitet / Veraltet / Fehler" is never only a log.
    ///
    /// GUI-TOAST-OVERLAP-1: a `Ready` probe shows NO badge — the transient
    /// overlay toast owns that signal now. The old green "ready" badge sat on
    /// top of the thumbnail cell indefinitely (no timeout, no dismiss) and
    /// covered the image it announced.
    fn neighbor_preview_badge(&self, probe_id: &str) -> Option<(String, egui::Color32)> {
        let ctrl = self.preview_ctrl.as_ref()?;
        // The active image is never displayed via the neighbor cache — skip the
        // badge there (SOLL: the active image stays a full texture).
        if ctrl.active_probe_id() == Some(probe_id) {
            return None;
        }
        let (label, color) = match ctrl.probe_state(probe_id) {
            preview_ctrl::PreviewProbeState::Miss => return None,
            preview_ctrl::PreviewProbeState::Ready => return None,
            preview_ctrl::PreviewProbeState::Loading => (
                Str::NeighborLoading.t().to_owned(),
                egui::Color32::from_rgb(0x44, 0x66, 0x88),
            ),
            preview_ctrl::PreviewProbeState::Stale => (
                Str::NeighborStale.t().to_owned(),
                egui::Color32::from_rgb(0xb0, 0x8a, 0x00),
            ),
            preview_ctrl::PreviewProbeState::Failed => (
                Str::NeighborFailedPattern
                    .format_arg(ctrl.failure(probe_id).unwrap_or("unbekannt")),
                egui::Color32::from_rgb(0xb0, 0x2a, 0x2a),
            ),
        };
        Some((label, color))
    }

    /// Persist the active virtual copy's recipe into the sidecar.
    ///
    /// Debounce-commit for slider edits (GUI-SLIDER-SAVE-1): runs the pending
    /// full-quality render, then — only when a slider/presence commit is
    /// pending — saves the sidecar through the CAS API and logs
    /// `<key>=<value> saved` at INFO with the "Sidecar saved" status.
    /// Failures stay loud (`show_error`, no silent loss); the edit itself
    /// remains in the recipe so a retry keeps the value. Zoom/pan state is
    /// deliberately never saved — it is GUI session state, never recipe.
    fn commit_pending_slider_save(&mut self, viewport: [u32; 2]) {
        if let Err(error) = self.render_full(viewport, None) {
            self.show_error(error);
            return;
        }
        if let Some((key, value)) = self.pending_slider_commit.take() {
            self.save_sidecar();
            if self.error().is_none() {
                info!("{key}={value} saved");
            }
        }
    }

    /// GUI-CLICK-ALL-17 + GUI-INSTRDBG-17: the Save Recipe footer button's
    /// action. Instrumented separately from the shared [`Self::save_sidecar`]
    /// helper because slider-debounce commits call that helper too, and a
    /// slider commit is not a button action.
    fn save_recipe_action(&mut self) {
        instrument_gui_action!(self, GuiAction::SaveRecipe);
        self.save_sidecar();
    }

    /// GUI-CLICK-ALL-17 + GUI-INSTRDBG-17: the Render/Apply footer button's
    /// action. Instrumented separately from [`Self::render`] because many
    /// section actions call `render` as a sub-step (they are instrumented
    /// themselves; nested suppression keeps exactly one line per action).
    fn render_action(&mut self) {
        instrument_gui_action!(self, GuiAction::Render);
        if let Err(error) = self.render() {
            self.show_error(error);
        }
    }

    /// REVIEW-GUI-SAVEMSG-1: the "Sidecar saved" status is set **only** on
    /// success; a failed write keeps the error visible instead of being
    /// overwritten by a success message.
    ///
    /// REVIEW-GUI-N1 + SIDECAR-REBASE-1: the write goes through the CAS API
    /// with the revision this lineage was loaded from; an externally modified
    /// sidecar is first rebased (local edits applied field-selectively) and
    /// only a conflict that persists over the retries is reported loudly.
    /// Additionally, `document.source` of an already-loaded document is kept
    /// as loaded — recomputing it from the live bytes would silently launder a
    /// source/conflict state (the fresh identity is only set for documents
    /// newly created in this session).
    fn save_sidecar(&mut self) {
        if self.path.trim().is_empty() {
            self.show_error(Str::SaveNeedsLocalPath.t());
            return;
        }
        let path = std::path::PathBuf::from(self.path.trim());
        let Some(frame) = &self.original else {
            self.show_error(Str::NoImageLoaded.t());
            return;
        };
        let sidecar_path = lumina_sidecar::sidecar_path_for(&path);
        // REVIEW-GUI-N1 + SIDECAR-REBASE-1: compare-and-swap against the
        // revision this document lineage was loaded from (`self.sidecar_revision`,
        // captured at load time and refreshed after each successful save). An
        // overtaking save is rebased onto the current file (field-selectively);
        // a conflict that persists over the bounded retries stays visible
        // instead of being silently overwritten. `None` expects the file not to
        // exist yet (fresh document): a concurrently appearing file is refused.
        let expected_revision = self.sidecar_revision.clone();
        let mut document = self
            .document
            .take()
            .unwrap_or_else(|| SidecarDocument::new(self.source_identity(frame), "raster-mvp-1"));
        // REVIEW-GUI-N1: the identity of an already-loaded document stays
        // exactly as loaded — recomputing it here from the live bytes would
        // silently launder an externally changed source (conflict laundering).
        // A document newly created above already carries the current identity
        // via `SidecarDocument::new(self.source_identity(frame), ..)`.
        // SIDECAR-REBASE-1: the pre-edit state is the three-way-merge ancestor
        // for a `Conflict` rebase (local edits survive on the current file).
        let base_document = document.clone();
        let Some(copy) = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            self.show_error(Str::VirtualCopyNotFound.t());
            self.document = Some(document);
            return;
        };
        let previous_recipe = copy.recipe.clone();
        copy.recipe = self.recipe.clone();
        // G-06 (LRPAR-G06-GEO): an armed geometry edit becomes exactly one
        // visible history step. The entry stores the saved (final) recipe,
        // like the CLI `geometry` command; slider drags coalesce because the
        // label is armed once per debounce window and consumed here.
        if let Some(step) = self.pending_history_step.take() {
            let mut counter = copy.history.len() + 1;
            while copy
                .history
                .iter()
                .any(|entry| entry.id == format!("geometry-{counter}"))
            {
                counter += 1;
            }
            let mut extras = BTreeMap::new();
            extras.insert("step".into(), Value::String("geometry".into()));
            extras.insert("action".into(), Value::String(step));
            // UX-LOOK-HISTORY-18: persist the readable step (control + old→new
            // + time) so the history panel shows more than a machine id.
            let mut entry = HistoryEntry {
                id: format!("geometry-{counter}"),
                recipe: copy.recipe.clone(),
                recorded_at: Some(self.history_timestamp()),
                extras,
            };
            if let Err(error) = entry.set_changes(history_changes::recipe_changes(
                &previous_recipe,
                &self.recipe,
            )) {
                error!("geometry history changes rejected: {error}");
            }
            copy.history.push(entry);
        }
        match sidecar_rebase::save_rebased(
            &sidecar_path,
            &base_document,
            &document,
            expected_revision.as_deref(),
            sidecar_rebase::MAX_REBASE_ATTEMPTS,
        ) {
            // LRPAR-G01-BASIC / GUI-VIEW-2: the shared finish helper moves the
            // Previous baseline and refreshes the single entry on success.
            Ok(saved) => self.finish_sidecar_save(&path, saved),
            Err(save_error) => {
                error!("sidecar save failed for {}: {save_error}", path.display());
                self.show_error(save_error);
                // Keep the local document so the failed edit is not lost; the
                // conflict stays visible until resolved.
                self.document = Some(document);
            }
        }
    }

    // ---- F-103-N5: Export module -------------------------------------------
    //
    // The export path is byte-identical to the CLI: it renders the current
    // recipe through the *same* `lumina_core::export_image` function (render +
    // encode) and writes the artifact through the *same* `lumina_sidecar::
    // write_atomically` helper. No encode logic is duplicated in the GUI.

    /// Resolve the effective export target and enforce the non-destructive
    /// write guards (REVIEW-GUI-EXPORT-1). The format extension is applied
    /// **first**, then the final path is checked against the loaded source and
    /// its persistent artefacts (`<source>.lumina.json` sidecar and
    /// `<source>.lumina.zdata` mask bundle) so an export can never overwrite
    /// the original or its sidecar data — e.g. target `/d/photo` with format
    /// PNG must be refused when `/d/photo.png` is the loaded source, which a
    /// pre-extension check would miss. Pure helper, unit-tested headless.
    fn resolve_export_target(
        source: &str,
        output: PathBuf,
        extension: &str,
    ) -> Result<PathBuf, GuiError> {
        let output = output.with_extension(extension);
        if source.trim().is_empty() {
            return Ok(output);
        }
        let source = Path::new(source);
        let sidecar = lumina_sidecar::sidecar_path_for(source);
        let zdata = lumina_sidecar::zdata_path_for(source);
        // `zdata_path_for` requires the sidecar crate's `zdata` feature, which
        // the GUI enables.
        let protected: Vec<(&Path, &str)> = vec![
            (source, "the original image"),
            (sidecar.as_path(), "its sidecar"),
            (zdata.as_path(), "its mask bundle"),
        ];
        for (protected_path, kind) in &protected {
            if Self::paths_resolve_equal_symmetric(protected_path, &output)
                .map_err(|error| GuiError::Io(error.to_string()))?
            {
                return Err(GuiError::Io(format!(
                    "export target {} resolves to {}, {}; refusing to overwrite it",
                    output.display(),
                    kind,
                    protected_path.display()
                )));
            }
        }
        Ok(output)
    }

    /// Same-path check that tolerates either side not existing yet
    /// (`lumina_sidecar::paths_resolve_equal` canonicalizes its first argument,
    /// which fails ENOENT for a sidecar/zdata artefact that was never written).
    /// Existing paths are canonicalized directly; a missing path is resolved
    /// against its parent directory so name collisions are still caught.
    fn paths_resolve_equal_symmetric(a: &Path, b: &Path) -> std::io::Result<bool> {
        let resolve = |path: &Path| -> std::io::Result<std::path::PathBuf> {
            if path.exists() {
                std::fs::canonicalize(path)
            } else {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                Ok(std::fs::canonicalize(parent)?.join(path.file_name().unwrap_or_default()))
            }
        };
        Ok(resolve(a)? == resolve(b)?)
    }

    /// Export the currently loaded image to `output` using the shared render +
    /// encode chain. The output file extension is forced to the format's
    /// canonical extension so the chosen format (not a typed extension) is
    /// authoritative — mirroring the CLI's `output.with_extension(...)`.
    ///
    /// The original source is never overwritten: if `self.path` is set and
    /// resolves equal to `output`, the export is rejected as a [`GuiError`]
    /// (no silent fallback). The artifact is written atomically; the declarative
    /// recipe/sidecar is left untouched by the export itself (the user saves
    /// the recipe explicitly via "Save Recipe / Sidecar").
    pub fn export_to(&mut self, output: PathBuf) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::Export);
        // G-06: read the source dimensions BEFORE borrowing `original` —
        // the Lensfun cache refresh below needs `&mut`.
        let (export_w, export_h) = self
            .original
            .as_ref()
            .map(|frame| (frame.width, frame.height))
            .unwrap_or((0, 0));
        // LRPAR-G14-DENOISE-IMPL-20: loud export gate before any work. The
        // shared export entry point has no denoise-stage input yet, so a
        // `ready` stage or a `Strict` non-ready stage refuses visibly instead
        // of exporting a frame that diverges from the preview; a `Warn`
        // fallback is surfaced (log + status below), never silent.
        if let Err(error) = self.guard_denoise_export() {
            let message = error.to_string();
            error!("export refused: {message}");
            self.show_error(message);
            return Err(error);
        }
        // GEN-ONNX-1 Welle 2b: resolve the generative canvases before any
        // shared borrow of `self` (the resolver needs `&mut self` for the
        // corrector cache). Only done when a generative role is active, so a
        // plain export never pays for the clone.
        let generative = if self.generative_stage_active() {
            let original = self
                .original
                .clone()
                .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
            self.resolve_generative_artifacts(&original)?
        } else {
            GenerativeArtifacts::default()
        };
        // G-06: Lensfun auto-corrector for the exported source (same
        // cached lookup as the preview render — export and preview share
        // the correction, no second pipeline).
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(export_w, export_h);
        #[cfg(feature = "lensfun")]
        let export_lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let export_lensfun = None;
        let Some(original) = self.original.as_ref() else {
            return Err(GuiError::Io(
                "No image loaded; open or drop an image first".into(),
            ));
        };
        let format = self.export_format;
        let quality = self.export_quality;
        // Apply the format extension FIRST, then enforce the non-destructive
        // write guards against the resolved target (REVIEW-GUI-EXPORT-1).
        let output = Self::resolve_export_target(
            &self.path,
            output,
            self.export_format.default_extension(),
        )?;
        let options = ExportOptions {
            format,
            quality,
            dither: false,
            ..Default::default()
        };
        options.validate().map_err(GuiError::Core)?;
        // Build the identical render context used by `render()` (what the user
        // currently sees) so the export matches the preview.
        let masks_context = {
            let planes = self.load_mask_planes();
            self.document.as_ref().and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                    .map(|_| MaskContext {
                        copies: &document.virtual_copies,
                        active_copy_id: &self.virtual_copy_id,
                        planes,
                        policy: MaskPolicy::Warn,
                    })
            })
        };
        // G-06: Lensfun auto-corrector for the exported source (same
        // cached lookup as the preview render — export and preview share
        // the correction, no second pipeline).
        let context = RenderContext {
            recipe: &self.recipe,
            camera_white_balance: self.camera_white_balance,
            source_actions: &[],
            masks: masks_context,
            lensfun: export_lensfun,
            depth: None,
        };
        // GEN-ONNX-1 Welle 2b: `export_image_with_generative` renders via the
        // shared artifact-aware path (`Lens → [auto-fill] → Perspective →
        // [expand] → Crop`). No post-render expand runs here — a second expand
        // would fail `validate_with_source` (canvas no longer larger) and abort
        // the export.
        let encoded = export_image_with_generative(original, &context, options, generative.input())
            .map_err(GuiError::Core)?;
        lumina_sidecar::write_atomically(&output, &encoded).map_err(GuiError::Sidecar)?;
        self.error = None;
        self.status = format!(
            "Exported {} ({:?} @ q{})",
            output.display(),
            format,
            quality
        );
        Ok(())
    }

    /// Suggested export file name derived from the source name and the selected
    /// format (e.g. `photo.jpg` from `photo.png`). Used to prefill the save
    /// dialog and the path field.
    fn suggested_export_name(&self) -> String {
        let base = if self.source_name.is_empty() {
            "export".to_string()
        } else {
            self.source_name.clone()
        };
        let stem = Path::new(&base)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "export".to_string());
        format!("{}.{}", stem, self.export_format.default_extension())
    }

    /// Draw the Export module controls.
    fn draw_export_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Export.t());
        ui.label(Str::ExportTarget.t());
        // GUI-VISION-1: button-first (right-to-left) row so the Choose button
        // keeps its natural width at the panel edge and the field takes the
        // rest. An unbounded edit claimed the full row and pushed the button
        // past the panel edge (kittest `export_module` golden).
        //
        // KITTEST-COVERAGE-STATES-1: the `right_to_left(Align::Center)` layout
        // must live inside a `ui.horizontal` row. Without it the layout used
        // the whole remaining panel height as its cross axis and vertically
        // centred the row, pushing Format / Quality / Export below the fold
        // (confirmed: the quality label laid out at y=722..784 of a 720px
        // viewport). The `horizontal` wrapper constrains the cross axis to the
        // row, so the destination field sits directly under its label and all
        // controls stay pixel-visible.
        let mut choose_clicked = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                choose_clicked = ui.button(Str::ExportChoose.t()).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.text_edit_singleline(&mut self.export_path);
                });
            });
        });
        if choose_clicked {
            let suggested = self.suggested_export_name();
            if let Some(path) = rfd::FileDialog::new().set_file_name(&suggested).save_file() {
                self.export_path = path.display().to_string();
            }
        }
        ui.horizontal(|ui| {
            ui.label(Str::ExportFormatLabel.t());
            egui::ComboBox::from_label("")
                .selected_text(format_label(self.export_format))
                .show_ui(ui, |ui| {
                    for (candidate, label) in [
                        (ImageFileFormat::Png, "PNG"),
                        (ImageFileFormat::Jpeg, "JPEG"),
                        (ImageFileFormat::WebP, "WebP"),
                    ] {
                        if ui
                            .selectable_label(self.export_format == candidate, label)
                            .clicked()
                        {
                            self.export_format = candidate;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(Str::ExportQualityLabel.t());
            let mut quality = self.export_quality as f64;
            if ui
                .add(egui::Slider::new(&mut quality, 1.0..=100.0).show_value(true))
                .changed()
            {
                self.export_quality = quality as u8;
            }
        });
        if self.export_format == ImageFileFormat::Png {
            ui.label(Str::ExportQualityUnused.t());
        }
        ui.horizontal(|ui| {
            if ui.button(Str::ExportUseSuggested.t()).clicked() {
                self.export_path = self.suggested_export_name();
            }
            if ui.button(Str::ExportRun.t()).clicked() {
                let path = PathBuf::from(self.export_path.trim());
                if path.as_os_str().is_empty() {
                    self.show_error(GuiError::Io("Choose an export target first".into()));
                } else if let Err(error) = self.export_to(path) {
                    error!("export failed to {}: {error}", self.export_path.trim());
                    self.show_error(error);
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        ui.label(&self.status);
    }

    /// A small sample RGBA PNG for headless snapshot / integration tests
    /// (F-103-N9). Pure helper with no app side effects; the bytes decode via
    /// [`Self::load_bytes`].
    pub fn sample_image_png() -> Vec<u8> {
        ImageFrame::new(
            4,
            3,
            vec![
                20, 30, 40, 255, 200, 180, 160, 255, 255, 255, 255, 255, 10, 10, 10, 255, 50, 60,
                70, 255, 90, 100, 110, 255, 120, 130, 140, 255, 200, 20, 20, 255, 1, 2, 3, 255, 80,
                90, 100, 255, 150, 160, 170, 255, 240, 240, 240, 255,
            ],
        )
        .expect("sample image dimensions are valid")
        .encode(ImageFileFormat::Png)
        .expect("sample image encodes to PNG")
    }

    /// Toggle Before/After. Deliberately does not touch the recipe.
    pub fn toggle_before_after(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleBeforeAfter);
        self.before_after = !self.before_after;
        trace!(
            "GUI interaction: toggle_before_after -> {}",
            self.before_after
        );
    }

    /// Derive a deterministic WB (temperature, tint) from a picked sRGB point so
    /// the channel means become neutral. `None` for non-positive channels.
    pub fn white_balance_from_point(r: f64, g: f64, b: f64) -> Option<(f64, f64)> {
        if r <= 0.0 || g <= 0.0 || b <= 0.0 {
            return None;
        }
        let l = (r + g + b) / 3.0;
        let gr = l / r;
        let gg = l / g;
        let gb = l / b;
        // Pipeline gains: R = 1 - warmth*0.35, G = 1 - tint*0.20, B = 1 + warmth*0.35.
        let warmth = (((1.0 - gr) + (gb - 1.0)) / 2.0) / 0.35;
        let tint = (1.0 - gg) / 0.20;
        let temperature = (6500.0 + warmth * 5500.0).clamp(1500.0, 12000.0);
        let tint = tint.clamp(-1.0, 1.0);
        Some((temperature, tint))
    }

    /// Set the `wb_temperature`/`wb_tint` recipe fields from a picked point
    /// (Core F-036-N1 path: `render_frame` applies them via the sRGB model).
    pub fn set_white_balance_from_point(&mut self, r: f64, g: f64, b: f64) -> Result<(), GuiError> {
        let Some((temp, tint)) = Self::white_balance_from_point(r, g, b) else {
            return Err(GuiError::Io(
                "Cannot derive white balance from this point".into(),
            ));
        };
        self.recipe
            .adjustments
            .insert("wb_temperature".into(), temp);
        self.recipe.adjustments.insert("wb_tint".into(), tint);
        self.wb_pick_mode = false;
        // GUI-SLIDER-SAVE-1: the eyedropper pick commits like a slider (both
        // fields persist; the temperature is the log representative).
        // GUI-SIDECAR-READ-1: commit synchronously — a bare `render()` would
        // clear `pending_full_render` while the commit stays armed, stranding
        // the save (same lost-edit class as `auto_tone` in N6). Render
        // failures stay loud via `show_error` inside the commit path.
        self.mark_recipe_dirty("wb_temperature", temp);
        self.status = "White balance set from picked point".into();
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// The eyedropper reads source pixels from the loaded frame.
    fn pick_white_balance_at(&mut self, nx: f64, ny: f64) {
        trace!(
            "GUI interaction: pick_white_balance_at nx={:.4} ny={:.4}",
            nx,
            ny
        );
        let Some(frame) = &self.original else {
            return;
        };
        let x = ((nx * frame.width as f64) as u32).min(frame.width.saturating_sub(1));
        let y = ((ny * frame.height as f64) as u32).min(frame.height.saturating_sub(1));
        let idx = ((y * frame.width + x) * 4) as usize;
        let px = &frame.pixels[idx..idx + 4];
        let r = px[0] as f64 / 255.0;
        let g = px[1] as f64 / 255.0;
        let b = px[2] as f64 / 255.0;
        if let Err(e) = self.set_white_balance_from_point(r, g, b) {
            error!("white balance pick failed at ({nx:.3},{ny:.3}): {e}");
            self.show_error(e);
        }
    }
}

/// The two-point identity curve `[(0,0),(1,1)]`.
fn identity_curve_points() -> Vec<CurvePoint> {
    vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ]
}

/// The four Lightroom parametric tone-curve regions (Shadows, Darks, Lights,
/// Highlights) of one curve channel as the GUI's source of truth. They are
/// persisted as that channel's point list via [`build_tone_curve_points`];
/// the read-back keeps the slider values stable for typical (unclamped)
/// adjustments. A channel without an explicit list reads as all-zero deltas.
fn tone_curve_channel_regions(recipe: &EditRecipe, channel: &str) -> (f64, f64, f64, f64) {
    let points: &[CurvePoint] = match channel {
        "red" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.red.as_deref())
            .unwrap_or(&[]),
        "green" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.green.as_deref())
            .unwrap_or(&[]),
        "blue" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.blue.as_deref())
            .unwrap_or(&[]),
        _ => recipe
            .curves
            .as_ref()
            .map(|c| c.master.as_slice())
            .unwrap_or(&[]),
    };
    tone_curve_regions_from_points(points)
}

/// Master-channel regions (backwards-compatible wrapper over
/// [`tone_curve_channel_regions`]; test-owned, the panel binds channels).
#[cfg(test)]
fn tone_curve_regions(recipe: &EditRecipe) -> (f64, f64, f64, f64) {
    tone_curve_channel_regions(recipe, "master")
}

/// Read-back of the four region deltas from stored curve points
/// (REVIEW-GUI-CURVE-1): kept separate so the roundtrip-loss detection can
/// evaluate the exact same math without constructing an [`EditRecipe`].
fn tone_curve_regions_from_points(points: &[CurvePoint]) -> (f64, f64, f64, f64) {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let mut out = (0.0f64, 0.0, 0.0, 0.0);
    let vals: [f64; 4] = std::array::from_fn(|i| {
        let bx = base[i];
        let out_v = points.get(i).map(|p| p.output as f64).unwrap_or(bx);
        (out_v - bx).clamp(-1.0, 1.0)
    });
    out.0 = vals[0];
    out.1 = vals[1];
    out.2 = vals[2];
    out.3 = vals[3];
    out
}

/// Persist the four region values as a [`Curves`] point list.  Outputs
/// stay in `[0,1]` so the render pipeline never sees an out-of-range control
/// point; extreme region values are clamped (a documented MVP simplification).
fn build_tone_curve_points(
    shadows: f64,
    darks: f64,
    lights: f64,
    highlights: f64,
) -> Vec<CurvePoint> {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let deltas = [shadows, darks, lights, highlights];
    base.iter()
        .zip(deltas.iter())
        .map(|(bx, d)| CurvePoint {
            input: *bx as f32,
            output: ((bx + d).clamp(0.0, 1.0)) as f32,
        })
        .collect()
}

/// Persist the four region values as a master [`Curves`] point list (see
/// [`build_tone_curve_points`]).
fn build_tone_curve(shadows: f64, darks: f64, lights: f64, highlights: f64) -> Curves {
    Curves {
        version: 1,
        master: build_tone_curve_points(shadows, darks, lights, highlights),
        channels: CurveChannels::default(),
    }
}

/// REVIEW-GUI-CURVE-1: true when building the master curve from the four
/// region deltas loses part of a delta because the clamped `[0,1]` output
/// absorbs it — most visibly Shadows, whose base point is `0.0`, so any
/// negative delta clamps to "no change" and the slider would snap back to 0.
/// Storing raw deltas would be a recipe-schema change (`Curves` outputs are
/// normatively `[0,1]`), so the GUI instead surfaces this limit explicitly in
/// the UI instead of letting the slider move silently.
///
/// The comparison carries an epsilon because outputs are stored as `f32`:
/// read-back noise (~1e-7) must not be reported as clamp loss; real losses
/// are multiples of the slider step (≥1e-2).
fn tone_curve_roundtrip_is_lossy(shadows: f64, darks: f64, lights: f64, highlights: f64) -> bool {
    const EPSILON: f64 = 1e-3;
    let curve = build_tone_curve(shadows, darks, lights, highlights);
    let (rs, rd, rl, rh) = tone_curve_regions_from_points(&curve.master);
    (rs - shadows).abs() > EPSILON
        || (rd - darks).abs() > EPSILON
        || (rl - lights).abs() > EPSILON
        || (rh - highlights).abs() > EPSILON
}

// REVIEW-GUI-DEBOUNCE-1 / R2-JANK-1: the debounce decision helper moved to
// `render_tick.rs` (cohesive with the draft-tick hot path); re-exported here so
// the app root and the headless tests keep calling it unqualified.
pub(crate) use render_tick::full_render_debounce_remaining;

/// Mutable reference to one HSL mixer channel, creating the `Option` slot on
/// first use so the GUI never has to special-case `None`.
fn hsl_channel_mut<'a>(hsl: &'a mut HslAdjustments, ch: &str) -> &'a mut HslChannel {
    let slot = match ch {
        "red" => &mut hsl.red,
        "orange" => &mut hsl.orange,
        "yellow" => &mut hsl.yellow,
        "green" => &mut hsl.green,
        "cyan" => &mut hsl.cyan,
        "blue" => &mut hsl.blue,
        "violet" => &mut hsl.violet,
        "magenta" => &mut hsl.magenta,
        _ => &mut hsl.red,
    };
    slot.get_or_insert_with(HslChannel::default)
}

/// True when `path` lies inside a `.lumina/` cache directory (exact
/// directory name `.lumina`, any level). Pure lexical path logic, no I/O:
/// `.lumina/` holds only deletable cache and settings (F-100 Library,
/// GUI-LIBRARY-LUMINA-DIR-1), so the Library scan must never surface files
/// below it as images — flat or recursive, on every level.
fn is_lumina_cache_path(path: &Path) -> bool {
    path.components().any(
        |component| matches!(component, std::path::Component::Normal(name) if name == ".lumina"),
    )
}

fn is_supported_image(path: &Path) -> bool {
    // The file browser lists all editable formats; the filmstrip display applies
    // its own RAW-only filter (see `draw_filmstrip`). v1: PNG/JPEG/WebP plus the
    // RAW extensions.
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp") => true,
        _ => is_raw_name(&path.display().to_string()),
    }
}

/// Human-readable label for an [`ImageFileFormat`] used by the Export panel.
fn format_label(format: ImageFileFormat) -> &'static str {
    match format {
        ImageFileFormat::Png => "PNG",
        ImageFileFormat::Jpeg => "JPEG",
        ImageFileFormat::WebP => "WebP",
    }
}

fn is_raw_name(name: &str) -> bool {
    // Single source of truth: delegate to `lumina_raw::is_raw_extension`
    // (RAW_EXTENSIONS lives there). R2-CLI-01 already consolidated the same
    // list for the CLI path; keeping one canonical extension list prevents the
    // two crates from drifting and re-introducing the silent 9-of-18 skip the
    // review flagged. Matching stays ASCII-case-insensitive.
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(lumina_raw::is_raw_extension)
}

/// Root of the Library folder tree — the **workdir** itself (the current
/// `directory` field), per Lightroom-parity: the Folders panel shows the
/// working directory as the root, not the whole `$HOME` tree. Pure path logic
/// so headless tests can exercise it without mutating process environment
/// state. Previously this rooted at `$HOME` (or a grandparent); the user asked
/// for root = workdir so the Library only ever browses the opened folder.
fn library_root(directory: &str) -> PathBuf {
    let dir = Path::new(directory).to_path_buf();
    if dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        dir
    }
}

/// Short display label of a folder node: path relative to the tree root, or
/// the final component for the root itself.
fn folder_label(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        _ => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
    }
}

/// Grid path badge of an entry vs. the listed `root`: `""` for top-level
/// files, otherwise the parent directory relative to `root` (F-100 Library:
/// recursive aggregation shows subfolder images with their relative folder
/// as badge). Pure lexical path logic, no I/O.
fn folder_badge(root: &Path, path: &Path) -> String {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    match parent.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        _ => String::new(),
    }
}

/// Maximum display width of the Library grid path badge, in characters.
/// The badge box is fixed at 118px (`vec2(118.0, 16.0)`) with monospace
/// 11.0; a monospace glyph advances ~6.6px, so 17 chars (~112px) fit with
/// padding to spare. Longer badges are middle-truncated (see below); the
/// stored [`FileBrowserEntry::folder`] keeps the full path.
const FOLDER_BADGE_MAX_CHARS: usize = 17;

/// Library grid badge chip background (GUI-LIBRARY-BADGE-CONTRAST-1): a
/// solid mid-grey instead of translucent black. Over dark thumbnails the old
/// chip melted into the image ("dark on dark") while the white 11px monospace
/// text needs AA contrast — pinned by `library_badge_contrast_meets_aa`.
/// Shared by the path badge and the rating badge (same chip style).
/// UX-SLICE-2 (F4): public so the rated-badge pixel golden asserts against the
/// exact painted fill instead of a duplicated literal.
pub const LIBRARY_BADGE_BG: egui::Color32 = egui::Color32::from_rgb(0x42, 0x42, 0x42);

/// UX-SLICE-1 (UXG-07): color-coded render-state badges at the preview edge
/// (the hash text lives in the app status line, drawn in `LuminaApp`'s `ui`).
/// Amber = an in-flight low-res draft; red-orange = stale/pending (no
/// `render_key` yet).
const RENDER_STATE_DRAFT_COLOR: egui::Color32 = egui::Color32::from_rgb(0xE6, 0xB4, 0x32);
const RENDER_STATE_STALE_COLOR: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x6C, 0x3C);

/// UX-SLICE-1 (P5): deterministic Library empty-state icon (a framed picture
/// with a sun and a mountain). Painted with primitives instead of an emoji so
/// no system font coverage can shift the golden.
fn paint_library_empty_icon(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 54.0), egui::Sense::hover());
    let stroke = ui.visuals().weak_text_color();
    let line = egui::Stroke::new(1.5, stroke);
    ui.painter()
        .rect_stroke(rect, 4.0, line, egui::StrokeKind::Inside);
    ui.painter()
        .circle_filled(rect.left_top() + egui::vec2(18.0, 16.0), 4.0, stroke);
    let mountain = vec![
        rect.left_bottom() + egui::vec2(6.0, -6.0),
        rect.center_bottom() + egui::vec2(2.0, -24.0),
        rect.right_bottom() + egui::vec2(-6.0, -6.0),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        mountain,
        stroke,
        egui::Stroke::NONE,
    ));
}

/// UX-SLICE-1 (UXG-09): the LR-01 rating/flag/color-label badge text for an
/// entry, or `None` when the cell stays clean (unrated, unflagged and
/// unlabeled). Pure and unit-testable; shared by the Library grid and the
/// filmstrip so both views present the same `FileBrowserEntry` data.
fn entry_badge_text(entry: &FileBrowserEntry) -> Option<String> {
    if entry.rating == 0 && entry.flag == lumina_sidecar::Flag::Unflagged && entry.color_label == 0
    {
        return None;
    }
    let mut badge = match entry.flag {
        lumina_sidecar::Flag::Pick => format!("{} P", stars_for_rating(entry.rating)),
        lumina_sidecar::Flag::Reject => format!("{} X", stars_for_rating(entry.rating)),
        lumina_sidecar::Flag::Unflagged => stars_for_rating(entry.rating),
    };
    if entry.color_label > 0 {
        badge.push_str(&format!(" ●{}", color_label_name(entry.color_label)));
    }
    Some(badge)
}

/// UX-SLICE-1 (UXG-09): paint [`entry_badge_text`] over the bottom-left edge
/// of `rect` (Library grid + filmstrip share this presentation).
fn paint_entry_badge(ui: &egui::Ui, rect: egui::Rect, entry: &FileBrowserEntry) {
    let Some(badge) = entry_badge_text(entry) else {
        return;
    };
    let badge_pos = rect.left_bottom() + egui::vec2(4.0, -16.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(badge_pos - egui::vec2(2.0, 2.0), egui::vec2(118.0, 16.0)),
        2.0,
        LIBRARY_BADGE_BG,
    );
    ui.painter().text(
        badge_pos,
        egui::Align2::LEFT_TOP,
        badge,
        egui::FontId::monospace(11.0),
        egui::Color32::WHITE,
    );
}

/// Display text for the folder badge: the full badge when it fits, otherwise
/// middle-truncated with `…` (`head…tail`) so the painted text never
/// overflows the fixed 118px box. The full name stays available via hover.
/// Pure string logic (char-based, unicode-safe), no I/O.
fn folder_badge_display(badge: &str) -> String {
    let len = badge.chars().count();
    if len <= FOLDER_BADGE_MAX_CHARS {
        return badge.to_owned();
    }
    let tail_len = (FOLDER_BADGE_MAX_CHARS - 1) / 2;
    let head_len = FOLDER_BADGE_MAX_CHARS - 1 - tail_len;
    let head: String = badge.chars().take(head_len).collect();
    let tail: String = badge.chars().skip(len - tail_len).collect();
    format!("{head}…{tail}")
}

/// How many directory levels the RAW-count scan descends at most. Keeps the
/// per-folder count cheap even under large trees.
const FOLDER_SCAN_DEPTH: usize = 3;

/// How much larger than the strictly visible window the zoom ROI is rendered
/// (REVIEW-GUI-PANROI-1): the extra border is panning headroom so the hand
/// tool always has off-screen content to drag into view without waiting for a
/// re-render. 1.0 would render exactly the visible window (no pan slack);
/// larger values trade render cost for smoother panning.
const PREVIEW_ROI_MARGIN: f64 = 1.3;

/// PERF-GUI-1: byte budget of the in-RAM base-stage cache
/// ([`lumina_core::StageFrameCache`]). Holds prepared, pre-adjustment frames
/// (post decode/source-actions/ROI-crop) so an exposure/color slider change
/// re-renders only the adjustment stage instead of re-running the crop +
/// source-action head and re-hashing the whole source file per tick. The
/// desktop cache budget is generous (512 MiB of prepared frames).
const BASE_STAGE_CACHE_MAX_BYTES: usize = 512 * 1024 * 1024;

/// Immediate subdirectories of `dir`, sorted; empty when unreadable so a
/// permission error degrades to "no children" instead of a broken node.
fn subdirectories(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// Maps the recipe's normalized free-crop rectangle (`Crop::Free` coordinates
/// are `0..=1`) into an on-screen image rect for a display-only crop overlay.
/// Returns `None` for no crop / aspect presets (whose normalized rect depends
/// on the decoded aspect ratio and is not tracked here).
///
/// GUI-RIGHT-THUMB-1: the right-panel thumbnail that consumed this helper was
/// removed (it tripled the preview and showed ROI crops as full frames); the
/// helper stays as a `cfg(test)`-gated math pin for future crop UI.
#[cfg(test)]
fn crop_overlay_rect(
    crop: Option<&lumina_sidecar::Crop>,
    img_rect: egui::Rect,
) -> Option<egui::Rect> {
    let Some(lumina_sidecar::Crop::Free {
        x,
        y,
        width,
        height,
    }) = crop
    else {
        return None;
    };
    if *width <= 0.0 || *height <= 0.0 {
        return None;
    }
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    let min = img_rect.min
        + egui::vec2(
            clamp01(*x) * img_rect.width(),
            clamp01(*y) * img_rect.height(),
        );
    let max = img_rect.min
        + egui::vec2(
            clamp01(x + width) * img_rect.width(),
            clamp01(y + height) * img_rect.height(),
        );
    Some(egui::Rect::from_min_max(min, max))
}

// NOTE (GUI-DOUBLE-EXPAND-FIX): the GUI-local checker-fill `apply_generative_expand`
// was removed. `GenerativeEdit(expand)` runs once inside the shared core
// pipeline (`render_frame` / `render_frame_from_base`,
// `Lens → Fill → Perspective → Expand → Crop`); preview and export use that
// core frame directly. A second post-render expand is a double-expand bug:
// the canvas is no longer larger than the frame, so `validate_with_source`
// fails and the export aborts.

fn clear_stale_auto_tone(recipe: &mut EditRecipe) {
    // AUTO-TONE-2: a present mirror marks the adjustment as auto-written, so
    // a stale fingerprint removes exactly those values (adjustment + mirror).
    // Manual edits carry no mirror and survive the clear.
    for (key, mirror) in [
        ("exposure", recipe.auto_features.auto_exposure),
        ("contrast", recipe.auto_features.auto_contrast),
        ("whites", recipe.auto_features.auto_whites),
        ("blacks", recipe.auto_features.auto_blacks),
        ("highlights", recipe.auto_features.auto_highlights),
        ("shadows", recipe.auto_features.auto_shadows),
    ] {
        if mirror.is_some() {
            recipe.adjustments.remove(key);
        }
    }
    recipe.auto_features.auto_exposure = None;
    recipe.auto_features.auto_contrast = None;
    recipe.auto_features.auto_whites = None;
    recipe.auto_features.auto_blacks = None;
    recipe.auto_features.auto_highlights = None;
    recipe.auto_features.auto_shadows = None;
}

fn is_current_tone_analysis(stored: &AnalysisFingerprint, input_fingerprint: &str) -> bool {
    stored.input_fingerprint == input_fingerprint
}

fn decoder_identity(source_is_raw: bool) -> &'static str {
    if source_is_raw {
        "libraw"
    } else {
        "image"
    }
}

impl eframe::App for LuminaApp {
    // eframe 0.36: `update(&mut self, ctx, frame)` was replaced by
    // `ui(&mut self, ui, frame)`; the context is cloned off the root `Ui`.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let perf_t0 = if std::env::var("LUMINA_PERF_LOG").as_deref() == Ok("1") {
            Some(std::time::Instant::now())
        } else {
            None
        };
        // Apply the Lumina dark theme once per frame. `egui` only re-applies the
        // fields that changed, so this is cheap and keeps the Lightroom feeling
        // consistent across modules.
        apply_lightroom_dark(&ctx);

        // Keyboard: `Y` toggles Before/After (which never mutates the recipe);
        // `Shift+Y` toggles the split Before/After marker (Welle 3, same
        // recipe-free guarantee). `Esc` cancels an armed white-balance
        // eyedropper; the interactive crop tool handles its own Enter/Esc.
        let shift_held = ctx.input(|i| i.modifiers.shift);
        if ctx.input(|i| i.key_pressed(egui::Key::Y)) {
            if shift_held {
                self.toggle_split_view();
            } else {
                self.toggle_before_after();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Q)) && !ctx.egui_wants_keyboard_input() {
            // UX-LOOK-TOOLBAR-18: shared with the icon toolbar (same status).
            self.toggle_spot_heal_tool();
        }
        self.handle_crop_shortcuts(&ctx);

        // Module-switch shortcuts (`G` Library grid, `D` Develop, `E`
        // Library loupe). They are ignored while a widget wants keyboard
        // input — e.g. a focused text field for mask/preset names — so they
        // cannot hijack typing. Switching modules never mutates the recipe
        // or sidecar. `G`/`E` route through the G-09 Library view so the
        // grid and the loupe are distinct, headless-testable states.
        if !ctx.egui_wants_keyboard_input() {
            if let Some(module) = ctx.input(|i| {
                for key in [egui::Key::G, egui::Key::D, egui::Key::E] {
                    if i.key_pressed(key) {
                        if let Some(target) = module_for_key(key) {
                            return Some(target);
                        }
                    }
                }
                None
            }) {
                if ctx.input(|i| i.key_pressed(egui::Key::E)) {
                    self.set_library_view(LibraryView::Loupe);
                } else if ctx.input(|i| i.key_pressed(egui::Key::G)) {
                    self.set_library_view(LibraryView::Grid);
                } else {
                    self.set_module(module);
                }
            }
        }

        // Consume idle tasks only while there is no interactive pointer input.
        // Only mask inference remains here; filmstrip thumbnails are produced by
        // the dedicated background thread pool (handled just below, without a
        // pointer gate, so switching the filmstrip never freezes).
        if !ctx.input(|input| input.pointer.any_down()) {
            if let Some((_id, task)) = self.idle_queue.pop_next() {
                match task {
                    IdleTask::MaskInference { mask_id } => {
                        self.status = Str::InferenceWaiting.format_arg(&mask_id);
                    }
                    IdleTask::Thumbnail { .. } => {
                        // Thumbnails are no longer enqueued on the idle queue
                        // (the thread pool owns their generation); kept only for
                        // an exhaustive match.
                    }
                }
            }
        }

        // Zoom shortcuts (Lightroom-like). Ignored while a widget wants keyboard
        // input so they never hijack typing. These set the zoom mode / a custom
        // multiplier; the actual `preview_zoom` is derived per-frame in
        // `sync_zoom()` so the ROI crop matches the on-screen view.
        // LR-01: `Num1`/`Num2` no longer zoom — they set the star rating (1:1
        // and 2:1 stay reachable through the preview toolbar buttons).
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
                self.zoom_step(1.2);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
                self.zoom_step(1.0 / 1.2);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F)) {
                // Welle 3 (LR-09 light): `F` is the fullscreen preview (hides
                // the lights-out chrome and settles the zoom on Fit when
                // enabling, so the previous zoom-to-fit role is preserved).
                self.toggle_fullscreen();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                // `Num0` clears the rating only when a document is loaded;
                // without an image it keeps its historical zoom-to-fit role.
                if self.document.is_some() {
                    if let Err(error) = self.set_rating(0) {
                        self.show_error(error);
                    }
                } else {
                    self.set_zoom_mode(ZoomMode::Fit);
                }
            }
        }

        // LR-01 / LR-09 / LR-10 rating, flag, mask-tool and duplicate shortcuts.
        // Ignored while a widget wants keyboard input so typing (mask names,
        // preset names, paths) is never hijacked. Failures surface via
        // `show_error`, never silently.
        if !ctx.egui_wants_keyboard_input() {
            for key in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ] {
                if ctx.input(|i| i.key_pressed(key)) {
                    if let Some(rating) = rating_for_key(key) {
                        if let Err(error) = self.set_rating(rating) {
                            self.show_error(error);
                        }
                    }
                }
            }
            for key in [egui::Key::P, egui::Key::X, egui::Key::U] {
                if ctx.input(|i| i.key_pressed(key)) {
                    if let Some(flag) = flag_for_key(key) {
                        if let Err(error) = self.set_flag(flag) {
                            self.show_error(error);
                        }
                    }
                }
            }
            let shift = ctx.input(|i| i.modifiers.shift);
            for key in [egui::Key::K, egui::Key::M] {
                // Cmd/Ctrl+M is the G-13 panorama-merge chord below; the mask
                // tool is modifier-free (see the F-100 shortcut table).
                if ctx.input(|i| i.key_pressed(key) && !i.modifiers.ctrl && !i.modifiers.command) {
                    if let Some(tool) = mask_tool_for_key(key, shift) {
                        // R5-TOOLFLOW-1: commit the active tool before arming.
                        self.commit_outgoing_tool_for_switch(&ctx);
                        self.set_mask_tool(tool);
                    }
                }
            }
            // LRPAR-G13-MERGE-15: `Cmd/Ctrl+H` / `Cmd/Ctrl+M` start the HDR /
            // panorama merge on the filmstrip selection (job control; the
            // shared `lumina-merge` entry points, no GUI image logic).
            for (key, mode) in [
                (egui::Key::H, lumina_sidecar::MergeMode::Hdr),
                (egui::Key::M, lumina_sidecar::MergeMode::Panorama),
            ] {
                if ctx.input(|i| i.key_pressed(key) && (i.modifiers.ctrl || i.modifiers.command)) {
                    if let Err(error) = self.start_merge(mode) {
                        self.show_error(error);
                    }
                }
            }
            if ctx.input(|i| {
                i.key_pressed(egui::Key::Quote) && (i.modifiers.ctrl || i.modifiers.command)
            }) {
                if let Err(error) = self.duplicate_active_copy() {
                    self.show_error(error);
                }
            }
            // Welle 2: color labels `6`–`9` (extras, no schema change).
            for key in [
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ] {
                if ctx.input(|i| i.key_pressed(key)) {
                    if let Some(label) = color_label_for_key(key) {
                        if let Err(error) = self.set_color_label(label) {
                            self.show_error(error);
                        }
                    }
                }
            }
            // Welle 2 (LR-09): copy/paste settings `Cmd/Ctrl+Shift+C/V` for
            // the active virtual copy.
            for key in [egui::Key::C, egui::Key::V] {
                if ctx.input(|i| {
                    i.key_pressed(key)
                        && (i.modifiers.ctrl || i.modifiers.command)
                        && i.modifiers.shift
                }) {
                    match clipboard_action_for_key(key, true, true) {
                        Some(ClipboardAction::Copy) => {
                            if let Err(error) = self.copy_settings() {
                                self.show_error(error);
                            }
                        }
                        Some(ClipboardAction::Paste) => {
                            if let Err(error) = self.paste_settings() {
                                self.show_error(error);
                            }
                        }
                        None => {}
                    }
                }
            }
            // Welle 2: B&W treatment `V` (recipe-backed, restores on repeat).
            if ctx.input(|i| i.key_pressed(egui::Key::V)) {
                if let Err(error) = self.toggle_black_white() {
                    self.show_error(error);
                }
            }
            // G-16: plain `S` toggles the display-only softproof preview
            // (G-10 binding reservation; full simulation is G-10 follow-up).
            // Strictly modifier-free so the `Cmd/Ctrl+Alt+S` snapshot chord
            // above never double-fires.
            if ctx.input(|i| {
                i.key_pressed(egui::Key::S)
                    && !i.modifiers.ctrl
                    && !i.modifiers.command
                    && !i.modifiers.alt
                    && !i.modifiers.shift
            }) {
                self.toggle_softproof_preview();
            }
            // Welle 3 (LR-17 light): stack-group proxy `Cmd/Ctrl+G` for the
            // active virtual copy. Failures surface via `show_error`, never
            // silently.
            if ctx
                .input(|i| i.key_pressed(egui::Key::G) && (i.modifiers.ctrl || i.modifiers.command))
            {
                match self.toggle_stack_group() {
                    Ok(_) => {}
                    Err(error) => self.show_error(error),
                }
            }
            // Welle 3 (LR-12 light): snapshot `Cmd/Ctrl+Alt+S` freezes the
            // session recipe under an auto name (`Snapshot <n>`).
            if ctx.input(|i| {
                i.key_pressed(egui::Key::S)
                    && (i.modifiers.ctrl || i.modifiers.command)
                    && i.modifiers.alt
            }) {
                self.create_snapshot_auto();
            }
        }

        // Welle 2 display-only view toggles (`J` clipping, `L` lights-out,
        // `R` crop mode, `Tab` side panels). Recipe-free by construction, so
        // they stay available globally.
        if !ctx.egui_wants_keyboard_input() {
            for key in [egui::Key::J, egui::Key::L] {
                if ctx.input(|i| i.key_pressed(key)) {
                    match view_toggle_for_key(key) {
                        Some(ViewToggle::Clipping) => self.toggle_clipping_overlay(),
                        Some(ViewToggle::LightsOut) => self.toggle_lights_out(),
                        Some(ViewToggle::BlackWhite) | None => {}
                    }
                }
            }
            for key in [egui::Key::R, egui::Key::Tab] {
                if ctx.input(|i| i.key_pressed(key)) {
                    // G-11: `Shift+Tab` hides all panels (incl. filmstrip);
                    // plain `Tab` keeps the filmstrip. Disambiguated here so
                    // the shift variant never falls into the plain branch.
                    if all_panels_toggle_for_key(key, ctx.input(|i| i.modifiers.shift)) {
                        self.toggle_all_panels_hidden();
                    } else {
                        match panel_toggle_for_key(key) {
                            Some(PanelToggle::CropMode) => self.toggle_crop_mode(),
                            Some(PanelToggle::PanelsHidden) => self.toggle_panels_hidden(),
                            None => {}
                        }
                    }
                }
            }
            // Welle 3 (LR-13 light): `\` toggles the Library filter drawer
            // (text filter + Quick Develop). Recipe-free like the other view
            // toggles, so it stays available on every platform.
            if ctx.input(|i| i.key_pressed(egui::Key::Backslash)) {
                self.toggle_filter_bar();
            }
            // Welle 3 (LR-20 light) + G-09: `C` compare reuses Before/After,
            // `N` survey jumps to the Library grid. Plain presses only —
            // `Cmd/Ctrl+Shift+C` stays copy-settings (native block below).
            // The G-09 Library view follows the compare proxy so both stay
            // in sync (repeat press leaves the view back to Grid).
            for key in [egui::Key::C, egui::Key::N] {
                if ctx.input(|i| i.key_pressed(key) && !i.modifiers.ctrl && !i.modifiers.command) {
                    if let Some(mode) = compare_mode_for_key(key) {
                        self.toggle_compare_mode(mode);
                        self.library_view = match self.compare_mode {
                            Some(CompareMode::Compare) => LibraryView::Compare,
                            Some(CompareMode::Survey) => LibraryView::Survey,
                            None => LibraryView::Grid,
                        };
                    }
                }
            }
            // G-09 (LRPAR-G09-LIB) Library keyboard navigation: arrows move
            // the selection over the filtered raster (no open), `Home`/`End`
            // jump to the ends, `Enter` opens the active image in Loupe,
            // `Esc` returns to Grid. Ignored outside the Library module and
            // while a widget wants keyboard input, like every F-100 shortcut.
            if self.active_module == Module::Library {
                let cols = self.library_cols.max(1) as isize;
                let mut delta: Option<isize> = None;
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    delta = Some(1);
                } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    delta = Some(-1);
                } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    delta = Some(cols);
                } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    delta = Some(-cols);
                } else if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                    delta = Some(isize::MIN / 2);
                } else if ctx.input(|i| i.key_pressed(egui::Key::End)) {
                    delta = Some(isize::MAX / 2);
                }
                if let Some(step) = delta {
                    self.move_library_selection(step);
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.open_library_selection();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    && self.library_view != LibraryView::Grid
                {
                    self.set_library_view(LibraryView::Grid);
                }
            }
            // Welle 3 (LR-13 light): `Cmd/Ctrl+Shift+I` jumps to Library
            // (import lives there), `Cmd/Ctrl+Shift+E` jumps to Export. Both
            // only switch the module and announce it — dialogs stay manual.
            for key in [egui::Key::I, egui::Key::E] {
                if ctx.input(|i| {
                    i.key_pressed(key)
                        && (i.modifiers.ctrl || i.modifiers.command)
                        && i.modifiers.shift
                }) {
                    if let Some(action) = import_export_for_key(key, true, true) {
                        self.apply_import_export_action(action);
                    }
                }
            }
        }

        // PERF-FILMSTRIP: drain completed thumbnails from the background pool and
        // build their textures on the main thread. This runs every frame
        // *regardless of pointer state* — thumbnails stream in while the user
        // scrolls/clicks the filmstrip, so switching directories no longer blocks
        // on a synchronous decode+render on the UI thread.
        self.poll_thumbnails(&ctx);

        // PREVIEW-CACHE-FEATURE: per-frame LUMINA_PERF_LOG counters reset before any
        // neighbor work of this frame (a schedule inside `poll_decode` counts).
        {
            self.frame_previews_enqueued = 0;
            self.frame_previews_ready = 0;
        }

        // PERF-GUI-7: drain any completed background RAW/raster decode without
        // blocking the UI (non-blocking `try_recv`). The decoded frame is applied
        // on the main thread here, so a slow decode never freezes interaction.
        self.poll_decode();

        // R2-MODSWITCH-1 F8: drain a completed background folder scan and apply
        // it atomically (visible loading status until it lands).
        self.poll_scan();

        // PREVIEW-CACHE-FEATURE: drain neighbor-preview worker results (RAM LRU
        // insert + visible failure states) on the main thread; the prefetch
        // itself runs on dedicated background workers, never the IdleQueue.
        self.poll_neighbor_previews(&ctx);

        // LRPAR-G13-MERGE-15: job control for the HDR/panorama merge — the
        // worker thread result is applied on the main thread (status/toast/
        // error + targeted entry refresh), never silently.
        self.poll_merge_job(ctx.input(|i| i.time));

        // Derive `preview_zoom` from the active mode using the geometry cached by
        // the previous frame's `draw_preview`, so the render's ROI crop matches
        // the on-screen zoom (even on the frame a mode button/shortcut fires).
        self.sync_zoom();

        // R3-RENDER-SIZE-1: refresh dpr + rebuild the draft source if the
        // viewport cap changed (resize / display move), before the scheduler.
        self.refresh_preview_cap(ctx.pixels_per_point());

        // PERF-GUI-3/4 + R2-JANK-1 F1/F4 + R2-MODSWITCH-1 F7: the per-frame
        // render scheduling (draft tick, debounce, module-switch deferral) lives
        // in `render_schedule` (file-size ratchet).
        // R3-WARMUP-1: the one-shot cold-start warmup runs on the existing
        // background paths while the UI is idle (armed by the native entry
        // point). It runs before the scheduler so a render it arms is committed
        // by the existing debounce path in the same frame.
        self.maybe_run_startup_warmup(&ctx);
        self.schedule_render(&ctx);

        // Dropped files (path or bytes) load a new source (native only).
        // egui 0.36: dropped files are trait objects (`DroppedFileHandle`)
        // whose contents are read synchronously via `bytes() -> Result`.
        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            match file.bytes() {
                Ok(bytes) => {
                    let name = file
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if let Err(error) = self.load_bytes(bytes, name) {
                        self.show_error(error);
                    }
                }
                Err(read_error) => {
                    // No silent fallback: a dropped file that cannot be read is
                    // surfaced as a visible error.
                    log::warn!("dropped file could not be read: {read_error}");
                    self.show_error(format!("dropped file unreadable: {read_error}"));
                }
            }
            if !file.path().as_os_str().is_empty() {
                // REVIEW-GUI-PATHDESYNC-1: no immediate `self.path` commit;
                // `finish_decode` adopts the path after a successful decode.
                self.begin_load_path(file.path().display().to_string());
            }
        }

        // Top: brand + status/error.
        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lumina");
                ui.separator();
                // GUI-DEBUG-SWEEP-1: the render hash is an internal key and is
                // no longer painted as header text. It stays available as a
                // tooltip on the status line (UX-SLICE-1/2's "hash in the
                // status line" is narrowed to "hash in the status tooltip").
                let status_response = ui.label(&self.status);
                if let Some(hash) = self.render_hash_tooltip() {
                    status_response.on_hover_text(hash);
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });

        // Top: module bar (Library / Develop / Export) + the Before/After toggle.
        // The histogram lives in its own collapsible Develop-panel section
        // (GUI-HISTOGRAM-1), not in the module bar. The module
        // labels advertise their Lightroom keyboard shortcuts (`G`, `D`).
        egui::Panel::top("modules").show(ui, |ui| self.draw_module_bar(ui));

        // Left: Lightroom-like Library folder tree. Develop/Export leave the
        // left edge to the navigator/preview working area. Hidden under `Tab`
        // panels-hide, `Shift+Tab` all-panels-hide, `L` lights-out and `F`
        // fullscreen (Welle 2/3, G-11); the
        // header/module bar stay so status and errors remain visible.
        if self.active_module == Module::Library && !self.side_chrome_hidden() {
            egui::Panel::left("folders")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| self.draw_folder_tree(ui));
        }

        // Left: Develop left rail (Navigator + Presets + Snapshots + History)
        // or the plain Navigator panel (Export). The Library module keeps its
        // text file-browser on the left instead (R4-UX-1: no duplicate rail;
        // the bottom filmstrip is the single selection surface everywhere).
        if self.navigator_open
            && !matches!(self.active_module, Module::Library)
            && !self.side_chrome_hidden()
        {
            self.draw_left_rail_panel(&ctx, ui);
        }

        // Right: Develop controls (eight sections), the Library Metadata
        // panel (LRPAR-G15-IPTC-S8, SOLL §10), or nothing extra for
        // Export (placeholder shown centrally). Hidden under
        // `Tab`/`Shift+Tab`/`L`/`F` like the left panels.
        if !self.side_chrome_hidden() {
            egui::Panel::right("controls")
                .resizable(true)
                .default_size(320.0)
                .show(ui, |ui| match self.active_module {
                    Module::Develop => self.draw_develop_panel(ui),
                    Module::Library => {
                        // UX-SLICE-1 (Layout-Bruch, mapper P4): a vertical `ScrollArea`
                        // shrinks horizontally to its content by default, so on
                        // the first frame the resizable panel frame anchored to
                        // the wrong edge and left a transparent/white strip to
                        // its right. `auto_shrink([false, _])` makes the scroll
                        // area claim the panel width, so the panel fills its
                        // `default_size` on every frame.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                self.draw_library_metadata_panel(ui);
                            });
                    }
                    Module::Export => {
                        self.draw_export_panel(ui);
                    }
                });
        }

        // Bottom: filmstrip in all three modules (F-100, see
        // `shows_filmstrip`). Generated thumbnails are produced by the
        // background worker pool (miss -> background job).
        // `L` lights-out and `F` fullscreen hide it (Welle 2/3); `Tab`
        // panels-hide keeps it.
        if self.shows_filmstrip() {
            egui::Panel::bottom("filmstrip").show(ui, |ui| self.draw_filmstrip(&ctx, ui));
        }

        // Central: the large preview/navigator. The Export module shows the
        // current render (what will be exported); the controls live in the
        // right-side Export panel.
        egui::CentralPanel::default().show(ui, |ui| match self.active_module {
            Module::Export => {
                self.draw_preview_area(&ctx, ui);
            }
            // Library: Lightroom-like grid view (folders tree left, RAW
            // thumbnail grid center); Develop/Export keep the large preview.
            Module::Library => {
                self.draw_library_grid(&ctx, ui);
            }
            _ => self.draw_preview_area(&ctx, ui),
        });
        // GUI-TOAST-OVERLAP-1: transient overlay toast (own Area, auto-dismiss
        // + manual ✕) — drawn last so it floats above the panels without
        // taking layout width.
        self.update_toast(&ctx);
        self.draw_toast(&ctx);
        // KITTEST-COVERAGE-STATES-1: error popup dialog (own floating window,
        // Close dismisses). Drawn with the other floating overlays.
        self.draw_error_dialog(&ctx);
        // LRPAR-G15-IPTC-S8: dynamic-preset prompt dialog (floating window,
        // Cancel discards without touching any sidecar).
        self.draw_meta_preset_dialog(&ctx);
        self.note_first_paint_after_switch();
        if let Some(t0) = perf_t0 {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            // GUI-SCROLL-200-1: `slow_frame` flags every frame over the 16.7 ms
            // (60 Hz) budget so scrolling spikes are greppable while thumbnail
            // jobs run; `thumb_jobs_enqueued`/`thumbs_ready` correlate a spike
            // with same-frame thumbnail work.
            let slow_frame = ms > 16.7;
            let counters = (
                self.frame_thumb_enqueued,
                self.frame_thumbs_ready,
                self.frame_previews_enqueued,
                self.frame_previews_ready,
            );
            log::info!(
                "LUMINA_PERF frame={:.2}ms pointer_down={} thumb_jobs_enqueued={} thumbs_ready={} neighbor_previews_enqueued={} neighbor_previews_ready={} slow_frame={}",
                ms,
                ctx.input(|i| i.pointer.any_down()),
                counters.0,
                counters.1,
                counters.2,
                counters.3,
                slow_frame
            );
            eprintln!(
                "LUMINA_PERF frame={:.2}ms thumb_jobs_enqueued={} thumbs_ready={} neighbor_previews_enqueued={} neighbor_previews_ready={} slow_frame={}",
                ms, counters.0, counters.1, counters.2, counters.3, slow_frame
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // GUI-INSTRDBG-17b: the remaining section-action logging tests, extracted
    // to keep this root test module within the file-size ratchet.
    mod instrdbg;
    // GUI-INSTRDBG-17b-REST: the click tests for the Spot/Detail/Optics/
    // Tone-Curve/Presets buttons (second extracted slice).
    mod instrdbg_rest;
    // GUI-INSTRDBG-17c: core instrumentation tests (moved out of gui_action.rs)
    // plus the last-button click tests (WB eyedropper, Point Color, Spot
    // distraction, Red-Eye picker, Presets refresh, generative canvas).
    mod instrdbg_core;
    mod instrdbg_last;
    mod instrdbg_rework;
    // GUI-INSTRDBG-17c-Rest: the People-view "Use as mask" click test.
    mod instrdbg_face;
    // GUI-GPU-AUDIT-17: headless audit over every `GuiAction` (Metal, --ignored).
    mod gpu_audit;
    mod gpu_audit_actions;
    // GUI-REFACTOR-W3-20 / UX-LOOK-TOOLBAR-18: thematic split of the former
    // monolithic root test module. The shared draw/click helpers live in
    // `support.rs` (see the ratchet comment there); each file pulls them in via
    // `use super::*` and keeps its test bodies otherwise unchanged.
    mod support;
    use support::*;
    mod badges;
    mod basic_commit;
    mod brush_gradient;
    mod distortion;
    mod export;
    mod f100_audit;
    mod f100_buttons;
    mod f100_shortcuts;
    mod g01_release;
    mod g15_batch;
    mod g15_collections;
    mod g15_stacks;
    mod g16_shortcuts;
    mod generative_expand;
    mod generative_render;
    mod generative_status;
    mod geometry;
    mod geometry_session;
    mod gpu_routing;
    // R3-Runde-3: routing/denoise fixes (R3-ROUTING-1/-DENOISE-1/-DENOISE-2).
    mod gpu_state;
    mod histogram;
    mod r3_fixes;
    // UX-LOOK-HISTORY-18: readable/clickable history entries + presets tree.
    mod history_presets_look;
    mod iptc;
    mod layout;
    mod lens_blur;
    mod library_scan;
    mod library_sort;
    mod library_sync;
    mod library_tree_r4;
    mod library_views;
    // R2-MODSWITCH-1 F7: module-switch latency (off-thread thumbnail cache,
    // metadata-only probe, deferred full render).
    mod masking_g03;
    mod masking_g11;
    mod masking_groups;
    mod modswitch;
    mod navigator;
    mod navigator_r4;
    mod optics;
    mod panels;
    mod preview_placement;
    mod preview_render;
    // R3-RENDER-SIZE-1: the preview viewport cap (draft + full) and its
    // export / 1:1-loupe exemptions.
    mod r3_render_size;
    // R2-MODSWITCH-1 F8: the asynchronous folder scan (worker + drain).
    mod r3_f8_scan;
    // R3-CONFLICT-1: the CAS-rebase path under a real two-writer race.
    mod r3_conflict;
    mod recipe_session;
    mod red_eye;
    mod render_cache;
    mod render_dirty;
    mod scheduling;
    mod selection;
    mod shortcuts;
    mod sidecar;
    mod sidecar_restore;
    mod sliders_commit;
    mod sliders_domain;
    mod sliders_filmstrip;
    mod spot_heal;
    mod spot_visualize;
    mod startup;
    // R3-WARMUP-1: the one-shot cold-start warmup tests.
    mod startup_warmup;
    mod toast;
    // UX-LOOK-TONECURVE-18: interactive tone-curve graph tests.
    mod tone_curve_graph;
    // R3-LOG-1: wall-clock switch/decode/render instrumentation tests.
    mod timing_instrumentation;
    // UX-LOOK-TOOLBAR-18: icon tool strip + Library view-tab paint/click tests.
    mod toolbar_icons;
    mod w3_release;
    mod zoom;
    mod zoom_steps;
    use lumina_core::ImageFileFormat;
    use lumina_sidecar::{
        BokehShape, BrushMark, BrushMarkSign, CoordinateSystem, Crop, DecodeFingerprint,
        DepthArtifactRef, GenerativeCanvas, GenerativeEdit, GeometryFingerprint, LensCorrection,
        MaskDefinition, MaskOperation, MaskPrompt, MaskStatus, ModelIdentity, NormalizedRect,
        Point2, Preprocessing, PromptTransform, Resolution, SourceFingerprint, SourceStatus,
    };
    /// GUI-STARTUP-FOLLOWUP-1 (B4): JPEG fixture through the real encoder so
    /// the startup test below decodes genuine JPEG bytes (not a renamed PNG).
    fn jpeg() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Jpeg)
            .unwrap()
    }
    /// GUI-STARTUP-FOLLOWUP-1 (B4): WebP fixture through the real encoder so
    /// the startup test below decodes genuine WebP bytes (not a renamed PNG).
    fn webp() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::WebP)
            .unwrap()
    }
    /// GUI-STARTUP-SELECTION-1: pump the background decode until the directory
    /// auto-load settles (loaded frame or loud error). Mirrors
    /// `open_and_decode` without opening a file first — the load was started
    /// by the scan itself.
    fn drain_auto_load(app: &mut LuminaApp) {
        for _ in 0..2000 {
            app.poll_decode();
            if app.original.is_some() || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    // -----------------------------------------------------------------------
    // F-100 Klickbarkeit (GUI-CLICK-ALL-17): per-button click/toggle tests.
    // Paint-only is not enough (DoD §3/§5): every new button must flip its
    // state on a real headless click, exactly like the Crop/Lights-Out anchors.
    // -----------------------------------------------------------------------

    /// Load the in-memory sample used by the display-only toolbar tests.
    fn toolbar_app() -> LuminaApp {
        let mut app = new_app();
        app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
            .unwrap();
        app
    }

    /// A real file on disk so the persisting History buttons (duplicate,
    /// snapshot, stack) can save their sidecar.
    fn persistent_app() -> (tempfile::TempDir, LuminaApp) {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app.ensure_document_loaded().unwrap();
        (directory, app)
    }

    /// Raw listing fixture: `scan_entry`/`list_directory` only need a
    /// supported extension (+ optional sidecar) — no decode runs during a
    /// directory scan, so a few sentinel bytes suffice.
    fn save_raw(path: &Path) {
        std::fs::write(path, b"lumina-raw-fixture").unwrap();
    }

    // ---- G-11 overlay/panel comfort (LRPAR-G11-OVERLAYS) ----

    /// Create a mask and inject a box prompt so overlay/pin tests have
    /// deterministic geometry without a pointer drag. Returns the mask id;
    /// the mask is selected afterwards.
    fn mask_with_box_prompt(app: &mut LuminaApp, name: &str, rect: (f32, f32, f32, f32)) -> String {
        let mask_id = app.create_mask(name).unwrap();
        {
            let virtual_copy_id = app.virtual_copy_id.clone();
            let document = app.document.as_mut().expect("document loaded");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == virtual_copy_id)
                .expect("active copy");
            let mask = copy
                .mask_library
                .iter_mut()
                .find(|mask| mask.id == mask_id)
                .expect("mask");
            mask.prompt = Some(MaskPrompt::Box {
                rect: NormalizedRect {
                    x: rect.0,
                    y: rect.1,
                    width: rect.2,
                    height: rect.3,
                },
                transformation: PromptTransform::default(),
            });
        }
        app.select_mask(&mask_id).unwrap();
        mask_id
    }

    // ---- PERF-GUI-1: staged base cache (hit/miss, stepwise invalidation,
    // pixel identity) ----

    /// Deterministic RGBA gradient PNG so the render stages exercise real
    /// per-pixel math over more than a handful of samples.
    fn gradient_png(width: u32, height: u32) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width) as u8;
                let g = ((y * 255) / height) as u8;
                let b = ((x + y) % 256) as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        ImageFrame::new(width, height, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    fn save_png(path: &Path) {
        let png = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    /// LRPAR-G08-PREVIOUS: like [`open_and_decode`], but waits for an image
    /// *switch* — `open_and_decode` returns immediately when any frame is
    /// loaded, so a second open would assert against the still-loaded first
    /// image. Pumps until the new path is adopted (or a loud error lands).
    fn open_and_decode_switch(app: &mut LuminaApp, path: &str) {
        app.open_file(path.to_string());
        for _ in 0..2000 {
            app.poll_decode();
            if app.path == path || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Shared commit-save-assert for the struct-backed slider classes
    /// (GUI-SLIDER-SAVE-1, native only): runs the debounced commit, loads the
    /// written sidecar document and fails loudly when no file was written.
    fn commit_and_load_doc(
        app: &mut LuminaApp,
        source: &std::path::Path,
    ) -> lumina_sidecar::SidecarDocument {
        app.commit_pending_slider_save([0, 0]);
        assert!(
            app.error().is_none(),
            "commit must not fail, got {:?}",
            app.error()
        );
        let sidecar = lumina_sidecar::sidecar_path_for(source);
        assert!(sidecar.is_file(), "Sidecar must be written");
        lumina_sidecar::load_sidecar(&sidecar).unwrap()
    }

    /// Reopen a source in a fresh app (DoD §1: values survive restarts).
    fn reopen_app(source: &std::path::Path) -> LuminaApp {
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());
        app
    }

    // ---- LRPAR-G06-UPRIGHT-15 / G-14 (LRPAR-G14-REDEYE-15) ----

    /// A `size`×`size` grid (bright bars on dark) tilted by `angle_deg`, so the
    /// deterministic upright analysis has a real line signal.
    fn save_tilted_png(path: &Path, angle_deg: f32) {
        let size = 128u32;
        let (sa, ca) = angle_deg.to_radians().sin_cos();
        let period = 0.4f32;
        let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
        for y in 0..size {
            for x in 0..size {
                let nx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let ny = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let rx = ca * nx + sa * ny;
                let dy = (rx / period - (rx / period).round()).abs() * period;
                let value = if dy < 0.05 { 235u8 } else { 20u8 };
                let i = ((y * size + x) as usize) * 4;
                pixels[i] = value;
                pixels[i + 1] = value;
                pixels[i + 2] = value;
                pixels[i + 3] = 255;
            }
        }
        let frame = ImageFrame::new(size, size, pixels).unwrap();
        std::fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    }

    /// Grey RGBA PNG with solid red rectangles — the deterministic GUI fixture
    /// for LRPAR-G14-REDEYE-AUTO-15.
    fn save_red_pupil_png(path: &Path, width: u32, height: u32, pupils: &[(u32, u32, u32, u32)]) {
        let mut frame = ImageFrame::new(
            width,
            height,
            [120u8, 120, 120, 255]
                .iter()
                .copied()
                .cycle()
                .take((width * height * 4) as usize)
                .collect(),
        )
        .unwrap();
        for &(x0, y0, x1, y1) in pupils {
            for y in y0..y1 {
                for x in x0..x1 {
                    let index = ((y * width + x) as usize) * 4;
                    frame.pixels[index..index + 4].copy_from_slice(&[220, 30, 40, 255]);
                }
            }
        }
        std::fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    }

    // ---- GUI-SCROLL-200-1: visible-first thumbnail scheduling (headless) ----

    /// Build an app browsing a folder with `count` supported images and return
    /// the app plus all entry indices (the scheduling helpers are format-
    /// agnostic; the RAW filter only applies to which views show entries).
    fn app_with_entries(count: usize) -> (LuminaApp, tempfile::TempDir, Vec<usize>) {
        let directory = tempfile::tempdir().unwrap();
        for i in 0..count {
            save_png(&directory.path().join(format!("img{i:03}.png")));
        }
        let mut app = new_app();
        app.set_directory(directory.path().display().to_string());
        let indices: Vec<usize> = (0..app.entries().len()).collect();
        assert_eq!(indices.len(), count, "every image must become an entry");
        (app, directory, indices)
    }

    // ---- GPU-LENSFUN-PARITY-1: a corrector recipe presents on the GPU route ----

    /// Minimal version_1 Lensfun fixture database (same shape as the
    /// `lumina-core` row tests): one camera + one lens with distortion and
    /// vignetting calibration. Embedded so the test builds a genuinely
    /// non-identity corrector deterministically, without depending on the
    /// system profile DB.
    #[cfg(all(feature = "gpu", feature = "lensfun"))]
    const LENSFUN_GATE_FIXTURE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Test Body</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Distortion+Vignetting 50mm f/2.8</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
        </calibration>
    </lens>
</lensdatabase>
"#;

    #[cfg(all(feature = "gpu", feature = "lensfun"))]
    fn synthetic_lensfun_corrector(
        directory: &std::path::Path,
    ) -> (lumina_lensfun::Corrector, lumina_lensfun::LensfunDb) {
        let path = directory.join("lensfun-gate-fixture.xml");
        std::fs::write(&path, LENSFUN_GATE_FIXTURE_XML).expect("write fixture database");
        let db = lumina_lensfun::LensfunDb::load_file(&path).expect("fixture database must load");
        let corrector = lumina_lensfun::Corrector::for_camera(
            &db,
            "Lumina Test Corp",
            "Lumina Test Body",
            None,
            640,
            480,
            50.0,
            2.8,
            10.0,
        )
        .expect("fixture profile must yield a corrector");
        (corrector, db)
    }

    // ---- F-103-INTEGRATION-PREVIEW-SIDECAR: headless UI integration ----

    fn synthetic_8x8_png() -> (Vec<u8>, ImageFrame) {
        // 8×8 checkerboard: 4×4 blocks alternating dark/mid, alpha 255.
        // Block color A = 32, B = 180 — both well inside 0..255 so exposure
        // brightening is measurable without clipping immediately.
        let mut pixels = Vec::with_capacity(8 * 8 * 4);
        for y in 0..8 {
            for x in 0..8 {
                let is_a = ((x / 4) + (y / 4)) % 2 == 0;
                let v = if is_a { 32u8 } else { 180u8 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let png = frame.encode(ImageFileFormat::Png).unwrap();
        (png, frame)
    }

    fn avg_luminance(frame: &ImageFrame) -> f64 {
        let sum: u64 = frame
            .pixels
            .chunks_exact(4)
            .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
            .sum();
        sum as f64 / (frame.width as f64 * frame.height as f64 * 3.0)
    }

    // ---- GEN-FILL-02 / GEN-ONNX-1 Welle 2b: Artefakt-Compositing ----

    /// GEN-ONNX-1 Welle 2b: build the deterministic fixture canvas for `edit`'s
    /// active role over `input` and wrap it as the core render-hook artifact.
    fn fixture_canvas_artifact(
        input: &ImageFrame,
        edit: &GenerativeEdit,
        role: lumina_onnx::GenerativeRole,
    ) -> GenerativeCanvasArtifact {
        let output = lumina_onnx::produce_canvas(
            input,
            edit,
            &lumina_onnx::GenerativeModelSource::Fixture(role),
        )
        .unwrap();
        let core_role = match role {
            lumina_onnx::GenerativeRole::Expand => lumina_core::GenerativeRole::Expand,
            lumina_onnx::GenerativeRole::AutoFillTransparent => {
                lumina_core::GenerativeRole::AutoFillTransparent
            }
        };
        GenerativeCanvasArtifact::new(core_role, output.to_frame().unwrap())
    }

    /// GEN-ONNX-1 Welle 2b: give `app` a real source path (writing `png` there)
    /// so the generative action can persist its `.lumina.zdata` bundle. The
    /// returned tempdir must stay alive for the test.
    fn app_source_path(app: &mut LuminaApp, png: &[u8], name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join(name);
        std::fs::write(&source, png).unwrap();
        app.path = source.display().to_string();
        dir
    }

    // ---- LRPAR-G04-REMOVE (G-04 Remove-Parität) ---------------------------
    /// 16×16 fixture with a dark 8×8 block (top-left): exactly one heuristic
    /// 8×8 cell at threshold 0.5, deterministic across runs.
    fn dark_block_png() -> Vec<u8> {
        let mut pixels = vec![255u8; 16 * 16 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let idx = (y * 16 + x) as usize * 4;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
            }
        }
        ImageFrame::new(16, 16, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    /// 32x32 vertical-stripe fixture (G-05): even columns black, odd columns
    /// white — high contrast so any bokeh blur visibly changes pixels.
    fn lens_blur_striped_png() -> Vec<u8> {
        let mut pixels = Vec::with_capacity(32 * 32 * 4);
        for _ in 0..32 {
            for x in 0..32 {
                let v = if x % 2 == 0 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        ImageFrame::new(32, 32, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    /// GUI-PREVIEW-NOISE-1: a synthetic 64×40 RGB gradient (content spread
    /// over the whole luminance range, like a real photo — never a flat
    /// field). Pure helper, no app side effects.
    fn synthetic_gradient_png() -> (Vec<u8>, ImageFrame) {
        let (w, h) = (64u32, 40u32);
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let r = (x * 255 / (w - 1)) as u8;
                let g = (y * 255 / (h - 1)) as u8;
                let b = ((x + y) * 255 / (w - 1 + h - 1)) as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let frame = ImageFrame::new(w, h, pixels).unwrap();
        let png = frame.encode(ImageFileFormat::Png).unwrap();
        (png, frame)
    }

    /// Normalized L1 distance of two 256-bin histograms (`0` = identical
    /// distributions, `2` = disjoint). Scale-free so a full-res preview and a
    /// downscaled thumbnail of the same content compare directly.
    fn histogram_l1(a: &[u64], b: &[u64]) -> f64 {
        assert_eq!(a.len(), b.len());
        let sum_a: u64 = a.iter().sum();
        let sum_b: u64 = b.iter().sum();
        assert!(sum_a > 0 && sum_b > 0, "histograms must be non-empty");
        a.iter()
            .zip(b.iter())
            .map(|(&x, &y)| (x as f64 / sum_a as f64 - y as f64 / sum_b as f64).abs())
            .sum()
    }

    /// Build a synthetic RAW-only browser entry (no disk IO — the name
    /// extension alone drives the RAW filter).
    fn raw_entry(dir: &std::path::Path, name: &str) -> FileBrowserEntry {
        let path = dir.join(name);
        FileBrowserEntry {
            thumb_key: path.display().to_string(),
            name: name.to_string(),
            path,
            has_sidecar: false,
            source_status: SourceStatus::Missing,
            conflict: false,
            virtual_copies: 0,
            missing_models: 0,
            rating: 0,
            flag: lumina_sidecar::Flag::Unflagged,
            color_label: 0,
            keywords: Vec::new(),
            collections: Vec::new(),
            camera: None,
            iso: None,
            focal_length: None,
            capture_timestamp: None,
            folder: String::new(),
            cull_badge: cull_gui::CullBadge::None,
            face_persons: Vec::new(),
            stack: None,
        }
    }

    /// Read one manual optics field from a lens block (DoD-§3
    /// Klassen-Vollständigkeit: all eight fields share one assertion path).
    fn lens_field(lens: &LensCorrection, field: &str) -> Option<f32> {
        match field {
            "distortion_k1" => lens.distortion_k1,
            "distortion_k2" => lens.distortion_k2,
            "distortion_k3" => lens.distortion_k3,
            "vignette_c0" => lens.vignette_c0,
            "vignette_c1" => lens.vignette_c1,
            "vignette_c2" => lens.vignette_c2,
            "ca_red" => lens.ca_red,
            "ca_blue" => lens.ca_blue,
            _ => None,
        }
    }

    /// Test-Eintrag mit allen erweiterten Filterdaten (Keywords,
    /// Sammlungen, EXIF). Felder sind privat, aber die Tests leben im
    /// selben Modul.
    fn filter_entry() -> FileBrowserEntry {
        FileBrowserEntry {
            path: PathBuf::from("/tmp/a.png"),
            name: "a.png".to_string(),
            thumb_key: "a".to_string(),
            has_sidecar: true,
            source_status: SourceStatus::Unchanged,
            conflict: false,
            virtual_copies: 1,
            missing_models: 0,
            rating: 4,
            flag: Flag::Pick,
            color_label: 1,
            keywords: vec!["portrait".to_string(), "Studio".to_string()],
            collections: vec![CollectionMembership {
                id: "best".to_string(),
                name: "Best Of".to_string(),
            }],
            camera: Some("Canon EOS R5".to_string()),
            iso: Some(400.0),
            focal_length: Some(50.0),
            capture_timestamp: Some(1_700_000_000),
            folder: String::new(),
            cull_badge: cull_gui::CullBadge::Review,
            face_persons: vec!["Alex".to_string()],
            stack: None,
        }
    }

    // ---- LRPAR-G15-IPTC-S8: Library Metadata panel -----------------------
    //
    // Jede Aktion folgt der Kette Edit → Commit → Datei → Reload (DoD §7);
    // das Original bleibt dabei byte-identisch.

    fn save_jpeg(path: &Path) {
        std::fs::write(path, jpeg()).unwrap();
    }

    fn write_meta_preset(
        dir: &Path,
        file: &str,
        name: &str,
        fields: &[(&str, &str)],
        placeholders: &[(&str, &str)],
    ) -> PathBuf {
        let preset = serde_json::json!({
            "format": "lumina-meta-preset",
            "version": 1,
            "name": name,
            "fields": fields.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect::<std::collections::BTreeMap<String, String>>(),
            "placeholders": placeholders.iter().map(|(n, d)| serde_json::json!({"name": n, "description": d})).collect::<Vec<_>>(),
        });
        let path = dir.join(file);
        std::fs::write(&path, serde_json::to_vec_pretty(&preset).unwrap()).unwrap();
        path
    }

    fn seed_sidecar(source: &Path) {
        let mut setup = new_app();
        open_and_decode(&mut setup, source.display().to_string());
        setup.add_keyword("seed").unwrap();
    }
}
