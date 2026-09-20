//! GUI-INSTRDBG-17 / -17b (User-Vorgabe 2026-09-17): debug instrumentation of
//! GUI actions. Every user-level command emits exactly one line in debug builds:
//!
//! ```text
//! action=<name> duration_ms=<n> gpu_route=<present|cpu-fallback|n/a>
//! ```
//!
//! The line is emitted by the RAII guard [`GuiActionTimer`] when the outermost
//! instrumented action scope ends. Nested instrumentation (an action calling
//! another instrumented command) is suppressed, so exactly one line per
//! user-visible action is logged — never one per internal sub-step. In release
//! builds the timer, the log and (via dead-code elimination) the guard are
//! compiled out: no logging, no timers, no behaviour difference.
//!
//! This module is the single source of truth for the action-name table. It is
//! extracted from `lib.rs` (GUI-INSTRDBG-17b) to keep the large GUI root file
//! within its file-size ratchet baseline while the remaining section actions
//! are instrumented. The macro is exported to the crate via `#[macro_use]` at
//! the crate root, so call sites stay `instrument_gui_action!(self, …)`.

/// GPU-route label: the frame used the VRAM present path (a GPU context is
/// bound and no route fallback is recorded).
pub const GPU_ROUTE_PRESENT: &str = "present";
/// GPU-route label: a GPU context is bound but the preview was routed to the
/// CPU (the visible `gpu_route_fallback` state).
pub const GPU_ROUTE_CPU_FALLBACK: &str = "cpu-fallback";
/// GPU-route label: no GPU context is bound (or the `gpu` feature is off).
pub const GPU_ROUTE_NA: &str = "n/a";

/// Central action-name table for the debug instrumentation (GUI-INSTRDBG-17).
///
/// Action names are snake_case and live here exactly once, so the button/
/// shortcut call sites carry no free-form literals. The table is the single
/// source of truth for both the log line and the headless format test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiAction {
    // GUI-INSTRDBG-17 (F-100 shortcut/action surface).
    ToggleBeforeAfter,
    ToggleSplitView,
    ToggleCropMode,
    ToggleClipping,
    ToggleSoftproof,
    ToggleOriginalHistogram,
    ToggleLightsOut,
    TogglePanelsHidden,
    ToggleAllPanelsHidden,
    ToggleFullscreen,
    ToggleFilterBar,
    ToggleBlackWhite,
    ToggleStackGroup,
    CreateSnapshot,
    DuplicateCopy,
    CopySettings,
    PasteSettings,
    SetRating,
    SetFlag,
    SetColorLabel,
    SetMaskTool,
    SetSpotTool,
    SetTreatment,
    SetModule,
    SetLibraryView,
    SetLibrarySort,
    SetZoomMode,
    RegenerateStale,
    MatchExposure,
    AutoTone,
    SaveRecipe,
    Reset,
    Render,
    Export,
    StartMerge,
    // GUI-INSTRDBG-17b (Library compare, Geometry, Masking-layer, Metadata).
    ToggleCompareMode,
    ClearCrop,
    SetCropAspect,
    RotateStep,
    SetGeometryMirror,
    AnalyzeUpright,
    SetUprightEnabled,
    ClearUpright,
    CreateMask,
    SelectMask,
    SetMaskVisible,
    SetShowMaskOverlay,
    SetOverlayColor,
    CreateAiMask,
    CreateLuminanceRangeMask,
    CreateColorRangeMask,
    CombineMasks,
    DuplicateMask,
    SetOverlayMode,
    SetPinVisibility,
    SetSoloMode,
    SetMaskInverted,
    OfferMaskRecalculation,
    AddKeyword,
    RemoveKeyword,
    CommitMetadataDraft,
    ClearMetadataDraft,
    CopyMetadataDraft,
    PasteMetadataDraft,
    ClearMetadataHistory,
    ApplyMetaPreset,
    SyncMetadata,
    // GUI-INSTRDBG-17b-REST (Spot extras, Detail/red-eye, Optics, Tone Curve,
    // Presets).
    SetSpotMode,
    ClearSpotVisualize,
    DetectSpotCandidates,
    ApplyDetectedSpots,
    RegenerateSpotVariant,
    ClearSpotHeals,
    DetectRedEye,
    ApplyDetectedRedEyes,
    RemoveRedEyeRegion,
    ClearRedEye,
    SetLensProfile,
    ClearLensProfile,
    SetLensBlurBokeh,
    AddCurvePoint,
    RemoveCurvePoint,
    ApplyPreset,
    SavePresetFile,
    // GUI-INSTRDBG-17c (WB eyedropper, Point Color, Spot distraction,
    // Red-Eye region picker, Presets refresh, generative canvas).
    SetSpotDistraction,
    SetRedEyePickMode,
    ReloadPresetEntries,
    ArmWbEyedropper,
    AddPointColor,
    RemovePointColor,
    SetExpandCanvas,
    GenerateCanvas,
    // GUI-INSTRDBG-17c-Rework: the remaining recipe-mutating controls the
    // verifier flagged (generative checkboxes, per-section Previous/Reset, the
    // lens-blur enable checkbox).
    SetExpandBeyondImage,
    SetAutoFillTransparent,
    RestoreSectionPrevious,
    ResetSection,
    SetLensBlurEnabled,
    // GUI-INSTRDBG-17c-Rework F-1: the three user-visible filmstrip buttons
    // that mutate the selected images' recipes (Lightroom "Sync Settings",
    // "Match Total Exposures", "Previous Image").
    SyncSettingsToSelection,
    MatchExposuresOfSelection,
    ApplyPreviousToSelection,
    // GUI-INSTRDBG-17c-Rework F-A: the KI-Denoise enable checkbox (Detail
    // section; recipe-mutating). The two denoise sliders
    // (`set_denoise_strength`/`set_denoise_preserve_detail`) are slider-class
    // and the fallback policy switch is session-only — both stay
    // uninstrumented by design.
    SetDenoiseEnabled,
    // GUI-INSTRDBG-17c REST (People view): the per-face "Use as mask" Develop
    // bridge. `create_face_mask` (`face_gui.rs`) turns a persisted, valid face
    // detection box into a mask definition on the active virtual copy
    // (`push_mask_definition`) and writes the sidecar — user-visible,
    // mask-/recipe-mutating.
    CreateFaceMask,
}

impl GuiAction {
    /// snake_case action name for the debug log line (GUI-INSTRDBG-17).
    pub const fn name(self) -> &'static str {
        match self {
            GuiAction::ToggleBeforeAfter => "toggle_before_after",
            GuiAction::ToggleSplitView => "toggle_split_view",
            GuiAction::ToggleCropMode => "toggle_crop_mode",
            GuiAction::ToggleClipping => "toggle_clipping",
            GuiAction::ToggleSoftproof => "toggle_softproof",
            GuiAction::ToggleOriginalHistogram => "toggle_original_histogram",
            GuiAction::ToggleLightsOut => "toggle_lights_out",
            GuiAction::TogglePanelsHidden => "toggle_panels_hidden",
            GuiAction::ToggleAllPanelsHidden => "toggle_all_panels_hidden",
            GuiAction::ToggleFullscreen => "toggle_fullscreen",
            GuiAction::ToggleFilterBar => "toggle_filter_bar",
            GuiAction::ToggleBlackWhite => "toggle_black_white",
            GuiAction::ToggleStackGroup => "toggle_stack_group",
            GuiAction::CreateSnapshot => "create_snapshot",
            GuiAction::DuplicateCopy => "duplicate_copy",
            GuiAction::CopySettings => "copy_settings",
            GuiAction::PasteSettings => "paste_settings",
            GuiAction::SetRating => "set_rating",
            GuiAction::SetFlag => "set_flag",
            GuiAction::SetColorLabel => "set_color_label",
            GuiAction::SetMaskTool => "set_mask_tool",
            GuiAction::SetSpotTool => "set_spot_tool",
            GuiAction::SetTreatment => "set_treatment",
            GuiAction::SetModule => "set_module",
            GuiAction::SetLibraryView => "set_library_view",
            GuiAction::SetLibrarySort => "set_library_sort",
            GuiAction::SetZoomMode => "set_zoom_mode",
            GuiAction::RegenerateStale => "regenerate_stale",
            GuiAction::MatchExposure => "match_total_exposure",
            GuiAction::AutoTone => "auto_tone",
            GuiAction::SaveRecipe => "save_recipe",
            GuiAction::Reset => "reset",
            GuiAction::Render => "render",
            GuiAction::Export => "export",
            GuiAction::StartMerge => "start_merge",
            GuiAction::ToggleCompareMode => "toggle_compare_mode",
            GuiAction::ClearCrop => "clear_crop",
            GuiAction::SetCropAspect => "set_crop_aspect",
            GuiAction::RotateStep => "rotate_step",
            GuiAction::SetGeometryMirror => "set_geometry_mirror",
            GuiAction::AnalyzeUpright => "analyze_upright",
            GuiAction::SetUprightEnabled => "set_upright_enabled",
            GuiAction::ClearUpright => "clear_upright",
            GuiAction::CreateMask => "create_mask",
            GuiAction::SelectMask => "select_mask",
            GuiAction::SetMaskVisible => "set_mask_visible",
            GuiAction::SetShowMaskOverlay => "set_show_mask_overlay",
            GuiAction::SetOverlayColor => "set_overlay_color",
            GuiAction::CreateAiMask => "create_ai_mask",
            GuiAction::CreateLuminanceRangeMask => "create_luminance_range_mask",
            GuiAction::CreateColorRangeMask => "create_color_range_mask",
            GuiAction::CombineMasks => "combine_masks",
            GuiAction::DuplicateMask => "duplicate_mask",
            GuiAction::SetOverlayMode => "set_overlay_mode",
            GuiAction::SetPinVisibility => "set_pin_visibility",
            GuiAction::SetSoloMode => "set_solo_mode",
            GuiAction::SetMaskInverted => "set_mask_inverted",
            GuiAction::OfferMaskRecalculation => "offer_mask_recalculation",
            GuiAction::AddKeyword => "add_keyword",
            GuiAction::RemoveKeyword => "remove_keyword",
            GuiAction::CommitMetadataDraft => "commit_metadata_draft",
            GuiAction::ClearMetadataDraft => "clear_metadata_draft",
            GuiAction::CopyMetadataDraft => "copy_metadata_draft",
            GuiAction::PasteMetadataDraft => "paste_metadata_draft",
            GuiAction::ClearMetadataHistory => "clear_metadata_history",
            GuiAction::ApplyMetaPreset => "apply_meta_preset",
            GuiAction::SyncMetadata => "sync_metadata",
            GuiAction::SetSpotMode => "set_spot_mode",
            GuiAction::ClearSpotVisualize => "clear_spot_visualize",
            GuiAction::DetectSpotCandidates => "detect_spot_candidates",
            GuiAction::ApplyDetectedSpots => "apply_detected_spots",
            GuiAction::RegenerateSpotVariant => "regenerate_spot_variant",
            GuiAction::ClearSpotHeals => "clear_spot_heals",
            GuiAction::DetectRedEye => "detect_red_eye_candidates",
            GuiAction::ApplyDetectedRedEyes => "apply_detected_red_eyes",
            GuiAction::RemoveRedEyeRegion => "remove_red_eye_region",
            GuiAction::ClearRedEye => "clear_red_eye",
            GuiAction::SetLensProfile => "set_lens_profile",
            GuiAction::ClearLensProfile => "clear_lens_profile",
            GuiAction::SetLensBlurBokeh => "set_lens_blur_bokeh",
            GuiAction::AddCurvePoint => "add_curve_point",
            GuiAction::RemoveCurvePoint => "remove_curve_point",
            GuiAction::ApplyPreset => "apply_preset",
            GuiAction::SavePresetFile => "save_preset_file",
            GuiAction::SetSpotDistraction => "set_spot_distraction",
            GuiAction::SetRedEyePickMode => "set_red_eye_pick_mode",
            GuiAction::ReloadPresetEntries => "reload_preset_entries",
            GuiAction::ArmWbEyedropper => "arm_wb_eyedropper",
            GuiAction::AddPointColor => "add_point_color",
            GuiAction::RemovePointColor => "remove_point_color",
            GuiAction::SetExpandCanvas => "set_expand_canvas",
            GuiAction::GenerateCanvas => "generate_generative_canvas",
            GuiAction::SetExpandBeyondImage => "set_expand_beyond_image",
            GuiAction::SetAutoFillTransparent => "set_auto_fill_transparent",
            GuiAction::RestoreSectionPrevious => "restore_section_previous",
            GuiAction::ResetSection => "reset_section",
            GuiAction::SetLensBlurEnabled => "set_lens_blur_enabled",
            GuiAction::SyncSettingsToSelection => "sync_settings_to_selection",
            GuiAction::MatchExposuresOfSelection => "match_exposures_of_selection",
            GuiAction::ApplyPreviousToSelection => "apply_previous_to_selection",
            GuiAction::SetDenoiseEnabled => "set_denoise_enabled",
            GuiAction::CreateFaceMask => "create_face_mask",
        }
    }
}

/// Every instrumented GUI action, in the order of [`GuiAction`]. Used by the
/// debug instrumentation test to pin the name table (uniqueness, snake_case).
pub const ALL_GUI_ACTIONS: &[GuiAction] = &[
    GuiAction::ToggleBeforeAfter,
    GuiAction::ToggleSplitView,
    GuiAction::ToggleCropMode,
    GuiAction::ToggleClipping,
    GuiAction::ToggleSoftproof,
    GuiAction::ToggleOriginalHistogram,
    GuiAction::ToggleLightsOut,
    GuiAction::TogglePanelsHidden,
    GuiAction::ToggleAllPanelsHidden,
    GuiAction::ToggleFullscreen,
    GuiAction::ToggleFilterBar,
    GuiAction::ToggleBlackWhite,
    GuiAction::ToggleStackGroup,
    GuiAction::CreateSnapshot,
    GuiAction::DuplicateCopy,
    GuiAction::CopySettings,
    GuiAction::PasteSettings,
    GuiAction::SetRating,
    GuiAction::SetFlag,
    GuiAction::SetColorLabel,
    GuiAction::SetMaskTool,
    GuiAction::SetSpotTool,
    GuiAction::SetTreatment,
    GuiAction::SetModule,
    GuiAction::SetLibraryView,
    GuiAction::SetLibrarySort,
    GuiAction::SetZoomMode,
    GuiAction::RegenerateStale,
    GuiAction::MatchExposure,
    GuiAction::AutoTone,
    GuiAction::SaveRecipe,
    GuiAction::Reset,
    GuiAction::Render,
    GuiAction::Export,
    GuiAction::StartMerge,
    GuiAction::ToggleCompareMode,
    GuiAction::ClearCrop,
    GuiAction::SetCropAspect,
    GuiAction::RotateStep,
    GuiAction::SetGeometryMirror,
    GuiAction::AnalyzeUpright,
    GuiAction::SetUprightEnabled,
    GuiAction::ClearUpright,
    GuiAction::CreateMask,
    GuiAction::SelectMask,
    GuiAction::SetMaskVisible,
    GuiAction::SetShowMaskOverlay,
    GuiAction::SetOverlayColor,
    GuiAction::CreateAiMask,
    GuiAction::CreateLuminanceRangeMask,
    GuiAction::CreateColorRangeMask,
    GuiAction::CombineMasks,
    GuiAction::DuplicateMask,
    GuiAction::SetOverlayMode,
    GuiAction::SetPinVisibility,
    GuiAction::SetSoloMode,
    GuiAction::SetMaskInverted,
    GuiAction::OfferMaskRecalculation,
    GuiAction::AddKeyword,
    GuiAction::RemoveKeyword,
    GuiAction::CommitMetadataDraft,
    GuiAction::ClearMetadataDraft,
    GuiAction::CopyMetadataDraft,
    GuiAction::PasteMetadataDraft,
    GuiAction::ClearMetadataHistory,
    GuiAction::ApplyMetaPreset,
    GuiAction::SyncMetadata,
    GuiAction::SetSpotMode,
    GuiAction::ClearSpotVisualize,
    GuiAction::DetectSpotCandidates,
    GuiAction::ApplyDetectedSpots,
    GuiAction::RegenerateSpotVariant,
    GuiAction::ClearSpotHeals,
    GuiAction::DetectRedEye,
    GuiAction::ApplyDetectedRedEyes,
    GuiAction::RemoveRedEyeRegion,
    GuiAction::ClearRedEye,
    GuiAction::SetLensProfile,
    GuiAction::ClearLensProfile,
    GuiAction::SetLensBlurBokeh,
    GuiAction::AddCurvePoint,
    GuiAction::RemoveCurvePoint,
    GuiAction::ApplyPreset,
    GuiAction::SavePresetFile,
    GuiAction::SetSpotDistraction,
    GuiAction::SetRedEyePickMode,
    GuiAction::ReloadPresetEntries,
    GuiAction::ArmWbEyedropper,
    GuiAction::AddPointColor,
    GuiAction::RemovePointColor,
    GuiAction::SetExpandCanvas,
    GuiAction::GenerateCanvas,
    GuiAction::SetExpandBeyondImage,
    GuiAction::SetAutoFillTransparent,
    GuiAction::RestoreSectionPrevious,
    GuiAction::ResetSection,
    GuiAction::SetLensBlurEnabled,
    GuiAction::SyncSettingsToSelection,
    GuiAction::MatchExposuresOfSelection,
    GuiAction::ApplyPreviousToSelection,
    GuiAction::SetDenoiseEnabled,
    GuiAction::CreateFaceMask,
];

/// Formats the single debug action line (GUI-INSTRDBG-17). Pure, so the
/// headless test can pin the exact format without a logger.
#[cfg(debug_assertions)]
pub fn gui_action_log_line(action: GuiAction, duration_ms: u128, gpu_route: &str) -> String {
    format!(
        "action={} duration_ms={duration_ms} gpu_route={gpu_route}",
        action.name()
    )
}

/// Debug-only sink for the instrumented action line: the `debug!` log plus,
/// under `cfg(test)`, a thread-local capture the headless tests drain to prove
/// exactly one line per action.
#[cfg(debug_assertions)]
fn emit_gui_action_log(line: String) {
    log::debug!("{line}");
    #[cfg(test)]
    GUI_ACTION_LOG.with(|log| log.borrow_mut().push(line));
}

// Thread-local capture of the debug action lines for headless tests.
#[cfg(all(debug_assertions, test))]
thread_local! {
    static GUI_ACTION_LOG: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Drains and returns the captured debug action lines of the current test
/// thread (headless instrumentation test helper).
#[cfg(all(debug_assertions, test))]
pub(crate) fn take_gui_action_log() -> Vec<String> {
    GUI_ACTION_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

// Outermost-action depth guard: nested instrumented commands are part of the
// enclosing action and must not log their own line (GUI-INSTRDBG-17).
#[cfg(debug_assertions)]
thread_local! {
    static GUI_ACTION_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// RAII timer for one instrumented GUI action (GUI-INSTRDBG-17). Construction
/// marks the action active on the current thread; `Drop` emits exactly one
/// line for the outermost action and releases the depth. Debug builds only.
#[cfg(debug_assertions)]
pub(crate) struct GuiActionTimer {
    action: GuiAction,
    start: std::time::Instant,
    gpu_route: &'static str,
    outermost: bool,
}

#[cfg(debug_assertions)]
impl GuiActionTimer {
    pub(crate) fn new(action: GuiAction, gpu_route: &'static str) -> Self {
        let outermost = GUI_ACTION_DEPTH.with(|depth| {
            let was = depth.get();
            depth.set(was + 1);
            was == 0
        });
        Self {
            action,
            start: std::time::Instant::now(),
            gpu_route,
            outermost,
        }
    }
}

#[cfg(debug_assertions)]
impl Drop for GuiActionTimer {
    fn drop(&mut self) {
        GUI_ACTION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        if self.outermost {
            emit_gui_action_log(gui_action_log_line(
                self.action,
                self.start.elapsed().as_millis(),
                self.gpu_route,
            ));
        }
    }
}

/// GUI-INSTRDBG-17: opens the debug action timer for `action` in the current
/// scope using the frame's GPU route. Expands to nothing in release builds
/// (no timer, no log, no binding), so the instrumented call sites are identical
/// in both profiles without per-button copy-paste.
macro_rules! instrument_gui_action {
    ($app:expr, $action:expr) => {
        #[cfg(debug_assertions)]
        let _gui_action_timer = $app.begin_gui_action($action);
        // GUI-JANKLOG-19: attach the jank record to the same outermost action
        // scope (no second timer). The record is emitted on drop only when the
        // scope is slow; nested render/dirty measurements fill this record.
        #[cfg(all(feature = "janklog", debug_assertions))]
        let _jank_scope = {
            let (route, badge) = $app.jank_route_and_badge();
            $crate::jank_log::JankScope::enter_action($action, route, badge)
        };
    };
}

// GUI-INSTRDBG-17: the debug instrumentation tests (format, nested suppression,
// name-table contract, release passthrough) live in
// `crate::tests::instrdbg_core` — extracted together with GUI-INSTRDBG-17c to
// keep this name-table module inside the file-size ratchet.
