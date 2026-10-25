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
// GUI-LIB-SPLIT-65 Slice 1: the F-103-N5 export module (target resolution,
// write guards, `export_to`, panel) is extracted (file-size ratchet).
mod export;
// GUI-LIB-SPLIT-65 Slice 3: G-11 edit pins (kind, record, prompt-to-anchor).
mod edit_pins;
pub use edit_pins::{pin_anchor_for_prompt, EditPin, EditPinKind};
// GUI-LIB-SPLIT-65 Slice 4: G-11 overlay/pin visibility modes and Str names.
mod overlay_modes;
pub use overlay_modes::{
    ai_select_kind_name, overlay_mode_name, pin_visibility_name, OverlayMode, PinVisibility,
};
// GUI-LIB-SPLIT-65 Slice 17: the G-11 overlay/pin visibility policy (which
// overlay and which edit pins the preview paints right now), extracted from the
// ratcheted crate root.
mod overlay_visibility;
// GUI-LIB-SPLIT-65 Slice 19: the working view's chrome-visibility policy (which
// frame parts the lights-out/panels-hide flags hide), extracted from the crate root.
mod view_chrome;
// GUI-LIB-SPLIT-65 Slice 8: pure display-diagnostic image statistics (shadow/
// highlight clipping fractions, normalized histogram L1 distance).
mod histogram_clip;
pub use histogram_clip::{clip_fractions, normalized_histogram_l1};
// GUI-LIB-SPLIT-65 Slice 9: the Library file-row data record
// (`FileBrowserEntry`) and its `thumbnail_key` helper.
mod file_browser_entry;
pub(crate) use file_browser_entry::thumbnail_key;
pub use file_browser_entry::FileBrowserEntry;
// GUI-LIB-SPLIT-65 Slice 5: Library folder/badge path helpers.
mod library_paths;
pub use library_paths::LIBRARY_BADGE_BG;
// `use` (not pub) keeps these items out of the public API while still
// exposing them to child modules via `super::*`.
#[allow(unused_imports)]
use library_paths::{
    entry_badge_text, folder_badge, folder_badge_display, folder_label, library_root,
    paint_entry_badge, paint_library_empty_icon, FOLDER_BADGE_MAX_CHARS, RENDER_STATE_DRAFT_COLOR,
    RENDER_STATE_STALE_COLOR,
};
// LRPAR-G14-DENOISE-IMPL-20 (GUI slice): Detail-section denoise controls +
// status badge; the stage itself runs in the shared core pipeline.
mod denoise_gui;
// GUI-INSTRDBG-17c-Rework F-A: the Detail-section denoise panel is extracted
// (file-size ratchet) while the enable checkbox gains its `GuiAction`.
mod denoise_panel;
// F-103-N6-DND-PERSIST-01: path-first native drop routing + regressions.
mod dropped_files;
// File-open routing policies extracted for the GUI file-size ratchet.
mod open_routing;
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
// LRPAR-G13-MERGE-15: read-only merge-bundle status view, extracted (file-size ratchet).
mod merge_status;
// GPU-LENSFUN-PARITY-1 (GUI-Wiring): build the CPU `LensfunMap` from the strict
// auto-corrector and bind it on the `GpuContext`, so a corrector recipe presents
// through the readback-free VRAM path instead of the documented CPU exception.
#[cfg(all(feature = "gpu", feature = "lensfun"))]
mod lensfun_gpu;
// LENSFUN-CALLER-37: the G-06 auto-corrector cache and the GUI's own Lensfun
// diagnostics sink, extracted from the crate root so the F2 reporting path has
// a cohesive home and the ratchet-baselined `lib.rs` does not grow.
#[cfg(feature = "lensfun")]
mod lensfun_auto;
#[cfg(feature = "lensfun")]
mod lensfun_diag;
// LENSFUN-CALLER-37: the cache's own value type moved to `lensfun_auto` with the
// logic that fills it; re-exported so the existing users (`gpu_routing`,
// `lensfun_gpu`, the `LuminaApp` field) are untouched.
#[cfg(feature = "lensfun")]
pub(crate) use lensfun_auto::CachedLensCorrector;
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
// MASK-LOCAL-P0: typed full-state transfer for cross-image Previous.
mod previous_state;
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
// AUTO-TONE-ENDPOINT-MIXED-7: the G-16 auto tone end points, extracted from the
// ratcheted crate root.
mod auto_endpoint;
// GUI-LIB-SPLIT-65 Slice 16: the G-14 red-eye GUI verbs (mark/edit/remove/clear
// a persisted region plus the deterministic pupil detection and its explicit
// apply), extracted from the ratcheted crate root.
mod red_eye;
// MASK-LOCAL-P1.1: capture lifecycle of the post-global/post-geometry stage that
// the mask-local white-balance picker samples (store + paired invalidation).
mod effective_source_stage;
// GUI-REFACTOR-W1-20 S1.4a: preview texture upload, the readback-free VRAM
// present path and the present target/source identity.
#[cfg(all(feature = "janklog", debug_assertions))]
mod jank_log;
mod present;
// GUI-PARITY-MASKGATE-1: the VRAM mask-plane lifecycle of the present
// composite (push the evaluated layers, keep a live brush plane, clear the
// plane when the frame carries no coverage at all).
#[cfg(feature = "gpu")]
mod present_mask_plane;
// The mutually exclusive arming state machine of the preview's competing input
// modes (WB eyedropper, local WB, red-eye, spot tool, mask tool).
mod preview_pickers;
// GUI-REFACTOR-W1-20 S1.4b (GPU only): present routing, VRAM/stage gate,
// Lensfun map bind, refusal classification and the parity diagnostic hooks.
#[cfg(feature = "gpu")]
mod gpu_routing;
// GUI-SRCACC-1: one strict resolver for persisted repair-region artifacts,
// shared by active preview/export and the navigator/neighbor/thumbnail paths.
mod source_actions;
// File-size-ratchet extraction (GPU-ROUTE-LOG-54): the three drag-gesture tool
// selectors. `pub use` keeps every existing path (`crate::MaskTool`, …) intact,
// so this is a relocation with no call-site change.
mod tool_selectors;
// File-size-ratchet extraction: the keyboard shortcut table ("which key does
// what"), previously scattered over four places in this file. `pub use` keeps
// every existing path (`crate::Module`, `crate::module_for_key`, …) intact, so
// this is a relocation with no call-site change in any crate.
mod idle_queue;
mod key_bindings;
pub use idle_queue::{IdleQueue, IdleTask};
pub use key_bindings::{
    all_panels_toggle_for_key, clipboard_action_for_key, color_label_for_key, color_label_name,
    flag_for_key, import_export_for_key, library_view_for_key, module_for_key,
    panel_toggle_for_key, rating_for_key, softproof_for_key, view_toggle_for_key, ClipboardAction,
    ImportExportAction, LibraryView, Module, PanelToggle, ViewToggle,
};
use source_actions::ResolvedSourceActions;
pub use tool_selectors::{MaskTool, SpotMode, SpotTool};
// THUMB-HASH-PERF-35: what a source file *is* — the one persisted
// `SourceIdentity` constructor (shared by `present` and the selection-sidecar
// path, replacing two field-for-field copies) plus the process-wide
// whole-file content memo keyed on `(path, mtime, ctime, len)` that stops the
// UI thread from re-hashing every visible RAW in every frame.
mod source_identity;
pub(crate) use source_identity::selection_source_identity;
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
mod brush_plane;
mod preview_masks;
// R5-BRUSH-24: size/softness/flow controls, `[`/`]` aliases and persisted
// per-dab snapshots.
mod brush_tool;
// R5-MASKVIS-25: mask-view gate, pins-only/full-selected display mode, the
// GPU-present selection check and the G-03 mask-visibility policy (Slice 20).
// The public mode enum is re-exported below.
mod mask_visibility;
pub use mask_visibility::MaskOverlayMode;
// R5-BRUSH-24 follow-up: one checked, rollback-capable transaction for
// definition + layer + selection management changes.
mod mask_persistence;
// MASK-LOCAL-P0: typed local-control transactions live outside the app root.
mod mask_local_controls;
// MASK-LOCAL-P1.2a/P1.2b/P1.2c/P1.2d: the mask-local tone-curve, colour,
// presence and detail editors and their setters.
mod mask_local_color;
mod mask_local_curves;
mod mask_local_detail;
mod mask_local_presence;
// R5-DUST-23: interactive Spot-Heal tool (dab + live-size cursor + `[`/`]`
// size shortcuts).
mod spot_tool;
// R5-DUST-23-FOLLOWUP: spot selection + per-spot editing (select/update/
// remove); `commit_spot_heal`/`clear_spot_heals` live here (ratchet).
mod spot_select;
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
mod timing_log_lines;
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
// GUI-LIB-SPLIT-65 Slice 14: Dateisystem- und Dateiformat-Erkennungshelfer.
mod fs_helpers;
pub(crate) use fs_helpers::{
    format_label, is_lumina_cache_path, is_raw_name, is_supported_image, subdirectories,
    FOLDER_SCAN_DEPTH,
};

// GUI-LIB-SPLIT-65 Slice 15: Werttypen der Hintergrund-Decode-Pipeline.
mod decode_pipeline;
pub(crate) use decode_pipeline::{
    lens_identity_from_metadata, DecodeRequestResult, DecodeResult, DecodedFrame,
    DirectoryOpenPolicy, LensIdentity, PendingDirectoryOpen,
};

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
use lumina_cache::DiskFolderCache;
use lumina_core::MaskPolicy;
// `export_image`/`ExportOptions` (Export module) and `rasterize_prompt` (mask overlay).
use lumina_core::{
    analyze_tone, analyze_tone_with_histogram, analyze_upright, apply_visualize_overlay,
    detect_red_eyes, detect_spots_heuristic, distraction_candidates, generative_input_frames,
    generative_variant_seed, has_transparent_pixels, match_total_exposure_masked,
    prepare_source_base, render::render_frame_from_base_with_source_stage,
    render_frame_from_base_with_generative_and_denoise, suggest_auto_tone, tone_fingerprint,
    upright_analysis, upright_input_fingerprint, AutoToneConfig, AutoToneResult, CacheStage,
    DetectedRedEye, DetectedSpot, DistractionKind, DistractionSetting, DistractionStatus,
    GenerativeCacheKey, GenerativeCanvasArtifact, GenerativeCanvasInput, GenerativeIdentity,
    ImageFileFormat, ImageFrame, LuminanceHistogram, MaskContext, MaskLayerResult, MaskPlane,
    OutputSpec, RenderContext, RenderKey, StageFrameCache, StageWork, RED_EYE_DETECT_ID_PREFIX,
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
    HistoryEntry, MaskDefinition, MaskLayer, MaskOperation, MaskPrompt, MaskReference,
    MaskStateSnapshot, MaskStatus, MetaPresetEntry, MetaPresetFile, MetadataHistoryEntry,
    ModelIdentity, Point2, Preprocessing, PromptTransform, Resolution, SidecarDocument,
    SmartCollectionDef, SmartRule, SourceFingerprint, SourceIdentity, SourceStatus, VirtualCopy,
    BW_STASH_KEY, DEVELOP_PROFILES, DEVELOP_PROFILE_KEY, TREATMENT_BW, TREATMENT_COLOR,
    TREATMENT_KEY,
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
use filmstrip::ThumbnailManager;
use i18n::Str;
// R3-DENOISE-1 (B4): the crate root no longer renders directly; the bare
// `render_frame` re-export is only consumed by the headless test modules.
#[cfg(test)]
use lumina_core::render_frame;
// R2-MODSWITCH-1 F7: the cache-aware thumbnail job/result types (worker pool).
use thumb_worker::{ThumbnailJob, ThumbnailResult};

// GUI-LIB-SPLIT-65 Slice 7: the GEN-ONNX-1 generative-canvas cache (value
// types + resolver helpers) is extracted (file-size ratchet).
mod generative_canvas;
pub(crate) use generative_canvas::{
    generative_record_id, generative_role_index, CachedGenerativeCanvas, GenerativeArtifacts,
    GenerativeRoleStatus,
};

// GUI-LIB-SPLIT-65 Slice 11: the Library panel value/decision helpers
// (`color_label_of`, `CompareMode`/`compare_mode_for_key`,
// `library_move_index`, `move_file_cross_volume`) are extracted (file-size
// ratchet); the Develop-section index block right below moved to
// `develop_sections` in Slice 12.
mod library_helpers;
pub(crate) use library_helpers::move_file_cross_volume;
pub use library_helpers::{color_label_of, compare_mode_for_key, library_move_index, CompareMode};

// GUI-LIB-SPLIT-65 Slice 12: the F-100 Develop-section index constants,
// the Basic-section recipe keys, and the `section_name` lookup are
// extracted (file-size ratchet). Slice 18 moved the G-11 section open state
// (solo mode) into the same module.
mod develop_sections;
pub(crate) use develop_sections::BASIC_TONE_KEYS;
pub use develop_sections::{
    section_name, SECTION_BASIC, SECTION_COLOR, SECTION_COUNT, SECTION_DETAIL, SECTION_EFFECTS,
    SECTION_GEOMETRY, SECTION_MASKING, SECTION_OPTICS, SECTION_TONE_CURVE,
};

// GUI-LIB-SPLIT-65 Slice 13: the pure, state-free helpers at the file edge
// (`AutoEndpoint`, `auto_endpoint_for_slider`, `masking_preview_for_slider`,
// `mask_tool_for_key`, `stars_for_rating`, `flag_label`) are extracted
// (file-size ratchet); the G-16 write path stays in `auto_endpoint`, the
// keyboard shortcut table stays in `key_bindings`.
mod pure_helpers;
pub use pure_helpers::{
    auto_endpoint_for_slider, flag_label, mask_tool_for_key, masking_preview_for_slider,
    stars_for_rating, AutoEndpoint,
};

// GUI-INSTRDBG-17/-17b: the debug action instrumentation core (GuiAction
// table, RAII `GuiActionTimer`, `instrument_gui_action!`) now lives in
// `gui_action.rs` (extracted to respect the file-size ratchet); the macro
// is imported with `#[macro_use]` below.

// GUI-LIB-SPLIT-65 Slice 6: G-15 smart rules, smart collections, and Library
// metadata field helpers (build_smart_rule, combine_smart_rules, stack_id_of,
// SMART_CATALOG_FORMAT, SmartCatalogFile, default_copy_id,
// default_meta_sync_fields, metadata_field_label, embedded_field_value).
mod library_smart;
pub use library_smart::{
    build_smart_rule, combine_smart_rules, default_copy_id, default_meta_sync_fields,
    embedded_field_value, metadata_field_label, stack_id_of, SmartCatalogFile,
    SMART_CATALOG_FORMAT,
};

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

#[derive(Debug, thiserror::Error)]
pub enum GuiError {
    #[error("{0}")]
    Core(#[from] lumina_core::CoreError),
    #[error("{0}")]
    Sidecar(#[from] lumina_sidecar::SidecarError),
    // AUTO-TONE-CLI-6: the shared Auto-Tone writer lives in `lumina-stages`, so
    // its failure reaches the GUI as a typed error rather than as a stringified
    // `Io` — a stage failure and a disk failure are different facts, and the
    // status line must not conflate them.
    #[error("{0}")]
    Stages(#[from] lumina_stages::StageError),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Raw(#[from] RawError),
}

// FILE-SIZE-1000-ALL-EXCEPTIONS-100: the application-state struct stays in the
// crate root — its fields are private and every split `impl LuminaApp` in the
// sibling modules relies on the Rust privacy rule that a child module sees the
// private fields of its parent. The definition is therefore kept *in this
// module* and only its text is included from `app_struct.rs`; `include!`
// expands right here, so no field gets a wider visibility.
include!("app_struct.rs");

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

/// LRPAR-G08-PREVIOUS: cross-image Previous reference — path, recipe, source
/// copy identity, and the complete typed mask-layer snapshot captured when
/// the image was displaced. Session-only, never persisted.
#[derive(Debug, Clone)]
pub struct PreviousReference {
    pub path: String,
    pub recipe: EditRecipe,
    /// Stable id of the source virtual copy. It is part of the transfer
    /// context, not persisted in the target sidecar.
    pub copy_id: String,
    /// Complete ordered layer state, including visibility and local values.
    pub mask_state: MaskStateSnapshot,
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

/// Draw method of one Develop section: `fn(&mut LuminaApp, &mut egui::Ui)`.
/// Factored out so [`LuminaApp::DEVELOP_SECTIONS`] stays readable.
type DevelopSectionDraw = fn(&mut LuminaApp, &mut egui::Ui);

// GUI-LIB-SPLIT-65 / FILE-SIZE-1000-ALL-EXCEPTIONS-100: the former single
// `impl LuminaApp` block (HEAD 1549-9119) is split into the sibling modules
// below. One inherent impl may be spread over several modules of the same
// crate; every method body is byte-identical to its `lib.rs` text and the
// struct above stays in the root (see `app_struct.rs`), so no field changes
// visibility. The only `lib.rs`-side wiring is this declaration list plus the
// `pub(crate)` tokens on the private helpers that a sibling module calls.
mod app_browser;
mod app_develop;
mod app_generative;
mod app_geometry;
mod app_library;
mod app_load;
mod app_masking;
mod app_metadata;
mod app_sections;
mod app_state;
mod app_tools;
// The per-frame `eframe::App` entry point (`ui`) lives in its own module.
mod app_impl;

// GUI-LIB-SPLIT-65 Slice 10: the parametric tone-curve region math (the
// Lightroom region deltas <-> the persisted `CurvePoint` list, plus the
// identity-curve and roundtrip-loss helpers) now lives in `tone_curve_regions`
// (file-size ratchet); re-exported so the Tone Curve panel, the recipe helpers
// and the headless tests keep calling it unqualified.
mod tone_curve_regions;
#[cfg(test)]
pub(crate) use tone_curve_regions::tone_curve_regions;
pub(crate) use tone_curve_regions::{
    build_tone_curve_points, identity_curve_points, tone_curve_channel_regions,
    tone_curve_roundtrip_is_lossy,
};

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
    // AUTO-TONE-2 / AUTO-TONE-ENDPOINT-MIXED-7: a stale fingerprint invalidates
    // the AUTO values. A present mirror that equals its slider marks the value
    // as auto-written, so it is removed together with the mirror. A slider that
    // differs from its mirror (a hand edit after the auto write) or that has no
    // mirror at all is a user value and survives — with it, a value the G-16 end
    // point claimed (or a hand value set after it) is not deleted on load.
    for (key, mirror) in [
        ("exposure", recipe.auto_features.auto_exposure),
        ("contrast", recipe.auto_features.auto_contrast),
        ("whites", recipe.auto_features.auto_whites),
        ("blacks", recipe.auto_features.auto_blacks),
        ("highlights", recipe.auto_features.auto_highlights),
        ("shadows", recipe.auto_features.auto_shadows),
    ] {
        if let Some(auto_value) = mirror {
            if recipe.adjustments.get(key).copied() == Some(auto_value) {
                recipe.adjustments.remove(key);
            }
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

#[cfg(test)]
mod tests;
