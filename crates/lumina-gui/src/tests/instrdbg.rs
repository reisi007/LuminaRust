//! GUI-INSTRDBG-17b: headless tests for the remaining section actions
//! (Library compare, Geometry, Masking-layer, Metadata). Each logs exactly
//! one line. The core format/name/release tests live in `gui_action.rs`
//! next to the extracted action table.
#![cfg(debug_assertions)]

use super::*;

/// GUI-INSTRDBG-17b / -17b-REST: the 49 actions added by the follow-up slices
/// (Library compare, Geometry, Masking-layer, Metadata, Spot extras, Detail,
/// Optics, Tone Curve, Presets).
const INSTRDBG_REST_ACTIONS: [GuiAction; 49] = [
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
];

/// State an action needs before its trigger. Runs *before* the capture is
/// drained, so a preparation step (which itself may be an instrumented
/// action) can never masquerade as the trigger's line.
#[cfg(debug_assertions)]
fn prepare_rest_action(app: &mut LuminaApp, action: GuiAction) -> Option<String> {
    match action {
        GuiAction::SelectMask
        | GuiAction::SetMaskVisible
        | GuiAction::CombineMasks
        | GuiAction::DuplicateMask
        | GuiAction::SetMaskInverted
        | GuiAction::OfferMaskRecalculation => {
            let id = app.create_mask("audit-mask").ok()?;
            let _ = app.select_mask(&id);
            Some(id)
        }
        GuiAction::PasteMetadataDraft => {
            let _ = app.copy_metadata_draft();
            None
        }
        GuiAction::RemoveRedEyeRegion | GuiAction::ClearRedEye => {
            let _ = app.add_red_eye_region(0.5, 0.5);
            None
        }
        GuiAction::RemoveCurvePoint => {
            // One interior control point paints the per-point remove button.
            app.add_curve_point("master", 0.5, 0.5);
            None
        }
        GuiAction::SavePresetFile => {
            app.preset_name = "instrdbg-preset".into();
            None
        }
        _ => None,
    }
}

/// One trigger per GUI-INSTRDBG-17b action. Results are ignored: this test
/// proves the logging wiring, not the action semantics (covered by the
/// dedicated tests).
#[cfg(debug_assertions)]
fn trigger_rest_action(app: &mut LuminaApp, action: GuiAction, mask_id: Option<&str>) {
    match action {
        GuiAction::ToggleCompareMode => app.toggle_compare_mode(CompareMode::Compare),
        GuiAction::ClearCrop => app.clear_crop(),
        GuiAction::SetCropAspect => {
            let _ = app.set_crop_aspect("1:1");
        }
        GuiAction::RotateStep => app.rotate_step(90.0),
        GuiAction::SetGeometryMirror => app.set_geometry_mirror(true, false),
        GuiAction::AnalyzeUpright => {
            let _ = app.analyze_upright_now();
        }
        GuiAction::SetUprightEnabled => {
            let _ = app.set_upright_enabled(false);
        }
        GuiAction::ClearUpright => app.clear_upright(),
        GuiAction::CreateMask => {
            let _ = app.create_mask("new-mask");
        }
        GuiAction::SelectMask => {
            if let Some(id) = mask_id {
                let _ = app.select_mask(id);
            }
        }
        GuiAction::SetMaskVisible => {
            if let Some(id) = mask_id {
                let _ = app.set_mask_visible(id, false);
            }
        }
        GuiAction::SetShowMaskOverlay => {
            let shown = app.show_mask_overlay();
            app.set_show_mask_overlay(!shown);
        }
        GuiAction::SetOverlayColor => app.set_overlay_color([1, 2, 3]),
        GuiAction::CreateAiMask => {
            let _ = app.create_ai_mask(AiSelectKind::Subject, None, "ai-mask");
        }
        GuiAction::CreateLuminanceRangeMask => {
            let _ = app.create_luminance_range_mask(0.1, 0.9, 0.2, "lum-mask");
        }
        GuiAction::CreateColorRangeMask => {
            let _ = app.create_color_range_mask(10.0, 20.0, 0.1, 0.9, 0.1, 0.9, 0.2, "col-mask");
        }
        GuiAction::CombineMasks => {
            let _ = app.combine_masks(MaskOperation::Invert, "", "combined");
        }
        GuiAction::DuplicateMask => {
            if let Some(id) = mask_id {
                let _ = app.duplicate_mask(id, "copy-mask");
            }
        }
        GuiAction::SetOverlayMode => app.set_overlay_mode(OverlayMode::Never),
        GuiAction::SetPinVisibility => app.set_pin_visibility(PinVisibility::Never),
        GuiAction::SetSoloMode => {
            let enabled = app.solo_mode();
            app.set_solo_mode(!enabled);
        }
        GuiAction::SetMaskInverted => {
            let _ = app.set_mask_inverted(true);
        }
        GuiAction::OfferMaskRecalculation => {
            let _ = app.offer_mask_recalculation();
        }
        GuiAction::AddKeyword => {
            let _ = app.add_keyword("audit");
        }
        GuiAction::RemoveKeyword => {
            let _ = app.remove_keyword("audit");
        }
        GuiAction::CommitMetadataDraft => {
            let _ = app.commit_metadata_draft();
        }
        GuiAction::ClearMetadataDraft => {
            let _ = app.clear_metadata_fields(&["description".to_string()]);
        }
        GuiAction::CopyMetadataDraft => {
            let _ = app.copy_metadata_draft();
        }
        GuiAction::PasteMetadataDraft => {
            let _ = app.paste_metadata_draft();
        }
        GuiAction::ClearMetadataHistory => {
            let _ = app.clear_metadata_history_gui();
        }
        GuiAction::ApplyMetaPreset => {
            let _ = app.apply_meta_preset_loaded("no-such-preset", &BTreeMap::new());
        }
        GuiAction::SyncMetadata => {
            let mut fields = BTreeSet::new();
            fields.insert("description".to_string());
            app.sync_metadata_to_selection(&fields);
        }
        GuiAction::SetSpotMode => app.set_spot_mode(SpotMode::Generative),
        GuiAction::ClearSpotVisualize => {
            let _ = app.clear_spot_visualize();
        }
        GuiAction::DetectSpotCandidates => {
            let _ = app.detect_spot_candidates();
        }
        GuiAction::ApplyDetectedSpots => {
            let _ = app.apply_detected_spot_objects();
        }
        GuiAction::RegenerateSpotVariant => {
            let _ = app.regenerate_spot_variant("spot-missing");
        }
        GuiAction::ClearSpotHeals => app.clear_spot_heals(),
        GuiAction::DetectRedEye => {
            let _ = app.detect_red_eye_candidates();
        }
        GuiAction::ApplyDetectedRedEyes => {
            let _ = app.apply_detected_red_eye_objects();
        }
        GuiAction::RemoveRedEyeRegion => app.remove_red_eye_region("re-1"),
        GuiAction::ClearRedEye => app.clear_red_eye(),
        GuiAction::SetLensProfile => {
            let _ = app.set_lens_profile("wide-light");
        }
        GuiAction::ClearLensProfile => app.clear_lens_profile(),
        GuiAction::SetLensBlurBokeh => app.set_lens_blur_bokeh(BokehShape::Hexagonal),
        GuiAction::AddCurvePoint => app.add_curve_point("master", 0.4, 0.4),
        GuiAction::RemoveCurvePoint => app.remove_curve_point("master", 1),
        GuiAction::ApplyPreset => {
            if let Ok(preset) = app.create_preset("instrdbg") {
                let _ = app.apply_preset(&preset);
            }
        }
        GuiAction::SavePresetFile => {
            let _ = app.save_current_selection_as_preset_file();
        }
        _ => panic!("not a GUI-INSTRDBG-17b action: {action:?}"),
    }
}

#[test]
#[cfg(debug_assertions)]
fn instrdbg_rest_actions_log_exactly_one_line() {
    for action in INSTRDBG_REST_ACTIONS {
        let (directory, mut app) = persistent_app();
        // Keep the file-preset actions off the real user directory.
        let presets = directory.path().join("presets");
        let _ = std::fs::create_dir_all(&presets);
        app.presets_dir = Some(presets);
        let mask_id = prepare_rest_action(&mut app, action);
        let _ = take_gui_action_log();
        trigger_rest_action(&mut app, action, mask_id.as_deref());
        let lines = take_gui_action_log();
        assert_eq!(
            lines.len(),
            1,
            "{action:?} must log exactly one line, got {lines:?}"
        );
        let expected = format!("action={} ", action.name());
        assert!(
            lines[0].starts_with(&expected),
            "{action:?}: expected {expected:?}, got {:?}",
            lines[0]
        );
        assert!(lines[0].contains(" duration_ms="), "{}", lines[0]);
        let route = lines[0]
            .rsplit_once("gpu_route=")
            .expect("gpu_route field")
            .1;
        assert!(
            route == GPU_ROUTE_PRESENT || route == GPU_ROUTE_CPU_FALLBACK || route == GPU_ROUTE_NA,
            "unexpected gpu_route in {}",
            lines[0]
        );
    }
}
