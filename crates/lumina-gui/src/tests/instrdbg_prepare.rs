//! Preparation steps for the remaining action-instrumentation tests.

use super::*;

/// State an action needs before its trigger. Runs *before* the capture is
/// drained, so a preparation step (which itself may be an instrumented
/// action) can never masquerade as the trigger's line.
#[cfg(debug_assertions)]
pub(super) fn prepare_rest_action(app: &mut LuminaApp, action: GuiAction) -> Option<String> {
    match action {
        GuiAction::SelectMask
        | GuiAction::SetMaskVisible
        | GuiAction::CombineMasks
        | GuiAction::DuplicateMask
        | GuiAction::RenameMask
        | GuiAction::DeleteMask
        | GuiAction::GroupDuplicateMask
        | GuiAction::SetMaskInverted
        | GuiAction::OfferMaskRecalculation => {
            let id = app.create_mask("audit-mask").ok()?;
            let _ = app.select_mask(&id);
            Some(id)
        }
        GuiAction::MoveMask => {
            let first = app.create_mask("audit-mask-first").ok()?;
            let _ = app.create_mask("audit-mask-second");
            Some(first)
        }
        GuiAction::PasteMetadataDraft => {
            let _ = app.copy_metadata_draft();
            None
        }
        GuiAction::RemoveRedEyeRegion | GuiAction::ClearRedEye => {
            let _ = app.add_red_eye_region(0.5, 0.5);
            None
        }
        // R5-DUST-23-FOLLOWUP: selection/editing need a committed spot; the
        // returned id drives the trigger below (like the mask actions).
        GuiAction::SelectSpot | GuiAction::UpdateSpot | GuiAction::RemoveSpot => {
            let _ = app.commit_spot_heal(
                lumina_sidecar::Point2 { x: 0.2, y: 0.2 },
                2.0,
                0.0,
                lumina_sidecar::Point2 { x: 0.1, y: 0.0 },
                1.0,
            );
            app.spot_entries()
                .into_iter()
                .find_map(|entry| entry.get("id").and_then(|v| v.as_str()).map(str::to_string))
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
        GuiAction::RemovePointColor => {
            app.add_point_color();
            None
        }
        GuiAction::GenerateCanvas => {
            let _ = app.set_expand_beyond_image(true);
            None
        }
        GuiAction::ToggleBeforeAfter
        | GuiAction::ToggleSplitView
        | GuiAction::ToggleCropMode
        | GuiAction::ToggleClipping
        | GuiAction::ToggleSoftproof
        | GuiAction::ToggleOriginalHistogram
        | GuiAction::ToggleLightsOut
        | GuiAction::TogglePanelsHidden
        | GuiAction::ToggleAllPanelsHidden
        | GuiAction::ToggleFullscreen
        | GuiAction::ToggleFilterBar
        | GuiAction::ToggleBlackWhite
        | GuiAction::ToggleStackGroup
        | GuiAction::CreateSnapshot
        | GuiAction::DuplicateCopy
        | GuiAction::CopySettings
        | GuiAction::PasteSettings
        | GuiAction::SetRating
        | GuiAction::SetFlag
        | GuiAction::SetColorLabel
        | GuiAction::SetMaskTool
        | GuiAction::SetSpotTool
        | GuiAction::SetTreatment
        | GuiAction::SetModule
        | GuiAction::SetLibraryView
        | GuiAction::SetLibrarySort
        | GuiAction::SetZoomMode
        | GuiAction::RegenerateStale
        | GuiAction::MatchExposure
        | GuiAction::AutoTone
        | GuiAction::SaveRecipe
        | GuiAction::Reset
        | GuiAction::Render
        | GuiAction::Export
        | GuiAction::StartMerge
        | GuiAction::SetDenoiseEnabled
        | GuiAction::ToggleCompareMode
        | GuiAction::ClearCrop
        | GuiAction::SetCropAspect
        | GuiAction::RotateStep
        | GuiAction::SetGeometryMirror
        | GuiAction::AnalyzeUpright
        | GuiAction::SetUprightEnabled
        | GuiAction::ClearUpright
        | GuiAction::CreateMask
        | GuiAction::SetShowMaskOverlay
        | GuiAction::SetOverlayColor
        | GuiAction::CreateAiMask
        | GuiAction::CreateLuminanceRangeMask
        | GuiAction::CreateColorRangeMask
        | GuiAction::SetOverlayMode
        | GuiAction::SetPinVisibility
        | GuiAction::SetSoloMode
        | GuiAction::AddKeyword
        | GuiAction::RemoveKeyword
        | GuiAction::CommitMetadataDraft
        | GuiAction::ClearMetadataDraft
        | GuiAction::CopyMetadataDraft
        | GuiAction::ClearMetadataHistory
        | GuiAction::ApplyMetaPreset
        | GuiAction::SyncMetadata
        | GuiAction::SetSpotMode
        | GuiAction::ClearSpotVisualize
        | GuiAction::DetectSpotCandidates
        | GuiAction::ApplyDetectedSpots
        | GuiAction::RegenerateSpotVariant
        | GuiAction::ClearSpotHeals
        | GuiAction::DetectRedEye
        | GuiAction::ApplyDetectedRedEyes
        | GuiAction::SetLensProfile
        | GuiAction::ClearLensProfile
        | GuiAction::SetLensBlurBokeh
        | GuiAction::AddCurvePoint
        | GuiAction::ApplyPreset
        | GuiAction::SetSpotDistraction
        | GuiAction::SetRedEyePickMode
        | GuiAction::ReloadPresetEntries
        | GuiAction::ArmWbEyedropper
        | GuiAction::AddPointColor
        | GuiAction::SetExpandCanvas
        | GuiAction::SetExpandBeyondImage
        | GuiAction::SetAutoFillTransparent
        | GuiAction::RestoreSectionPrevious
        | GuiAction::ResetSection
        | GuiAction::SetLensBlurEnabled
        | GuiAction::SyncSettingsToSelection
        | GuiAction::MatchExposuresOfSelection
        | GuiAction::ApplyPreviousToSelection
        | GuiAction::CreateFaceMask => None,
    }
}
