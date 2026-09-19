//! F-100 exhaustive GuiAction → button guard and audit tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// -----------------------------------------------------------------------
// F-100 Shortcut → Button audit (GUI-CLICK-ALL-17): no shortcut without a
// button. The central `GuiAction` table is the shortcut registry (every
// user shortcut is instrumented); this match is exhaustive over the enum,
// so a future shortcut fails compilation until its button is mapped, and
// the audit then fails unless that button is actually painted.
// -----------------------------------------------------------------------

/// The headless draw surface that hosts an action's clickable button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum F100Surface {
    Preview,
    Histogram,
    Rating,
    LibraryGrid,
    History,
    ModuleBar,
    Develop,
    Export,
    Basic,
    Masking,
    Spot,
    Merge,
    Geometry,
    Metadata,
    Detail,
    Optics,
    ToneCurve,
    Presets,
    // GUI-INSTRDBG-17c: Color (Point Color) and the generative canvas
    // buttons.
    Color,
    Generative,
    // GUI-INSTRDBG-17c-Rework F-1: the filmstrip selection buttons
    // (Sync Settings / Match Total Exposures / Previous Image).
    Filmstrip,
    // GUI-INSTRDBG-17c-Rest: the Library People view (per-face
    // "Use as mask" Develop bridge).
    People,
}

/// Exhaustive `GuiAction` → (`surface`, `button label`). No `_` arm.
fn f100_action_button(action: GuiAction) -> (F100Surface, String) {
    match action {
        GuiAction::ToggleBeforeAfter => (F100Surface::ModuleBar, Str::BeforeAfter.t().into()),
        GuiAction::ToggleSplitView => (F100Surface::Preview, Str::ViewToolbarSplit.t().into()),
        GuiAction::ToggleCropMode => (F100Surface::Preview, Str::ViewToolbarCrop.t().into()),
        GuiAction::ToggleClipping => (F100Surface::Preview, Str::ViewToolbarClipping.t().into()),
        GuiAction::ToggleSoftproof => (F100Surface::Histogram, Str::SoftproofToggle.t().into()),
        GuiAction::ToggleOriginalHistogram => (
            F100Surface::Histogram,
            Str::HistogramShowOriginal.t().into(),
        ),
        GuiAction::ToggleLightsOut => (F100Surface::Preview, Str::ViewToolbarLightsOut.t().into()),
        GuiAction::TogglePanelsHidden => (F100Surface::Preview, Str::ViewToolbarPanels.t().into()),
        GuiAction::ToggleAllPanelsHidden => {
            (F100Surface::Preview, Str::ViewToolbarAllPanels.t().into())
        }
        GuiAction::ToggleFullscreen => {
            (F100Surface::Preview, Str::ViewToolbarFullscreen.t().into())
        }
        GuiAction::ToggleFilterBar => (F100Surface::LibraryGrid, Str::FilterBar.t().into()),
        GuiAction::ToggleBlackWhite => (F100Surface::Basic, Str::TreatmentBlackWhite.t().into()),
        GuiAction::ToggleStackGroup => (F100Surface::History, Str::StackGroup.t().into()),
        GuiAction::CreateSnapshot => (F100Surface::History, Str::SnapshotButton.t().into()),
        GuiAction::DuplicateCopy => (F100Surface::History, Str::DuplicateCopy.t().into()),
        GuiAction::CopySettings => (F100Surface::History, Str::CopySettings.t().into()),
        GuiAction::PasteSettings => (F100Surface::History, Str::PasteSettings.t().into()),
        GuiAction::SetRating => (F100Surface::Rating, "1".into()),
        GuiAction::SetFlag => (F100Surface::Rating, flag_label(Flag::Pick).into()),
        GuiAction::SetColorLabel => (F100Surface::Rating, format!("1 {}", color_label_name(1))),
        GuiAction::SetMaskTool => (F100Surface::Masking, Str::MaskToolBrush.t().into()),
        GuiAction::SetSpotTool => (F100Surface::Spot, "Heal (Q)".into()),
        GuiAction::SetTreatment => (F100Surface::Basic, Str::TreatmentColor.t().into()),
        GuiAction::SetModule => (F100Surface::ModuleBar, Str::DevelopShortcut.format_arg("D")),
        GuiAction::SetLibraryView => (F100Surface::LibraryGrid, Str::LibraryGridOn.t().into()),
        GuiAction::SetZoomMode => (F100Surface::Preview, Str::ZoomFit.t().into()),
        GuiAction::RegenerateStale => (F100Surface::Develop, Str::RegenerateStale.t().into()),
        GuiAction::MatchExposure => (F100Surface::Develop, Str::MatchExposure.t().into()),
        GuiAction::AutoTone => (F100Surface::Basic, Str::Auto.t().into()),
        GuiAction::SaveRecipe => (F100Surface::Develop, Str::SaveRecipe.t().into()),
        GuiAction::Reset => (F100Surface::Develop, Str::Reset.t().into()),
        GuiAction::Render => (F100Surface::Develop, Str::RenderApply.t().into()),
        GuiAction::Export => (F100Surface::Export, Str::ExportRun.t().into()),
        GuiAction::StartMerge => (F100Surface::Merge, Str::MergeHdr.t().into()),
        // GUI-INSTRDBG-17b: Library compare, Geometry, Masking-layer and
        // Metadata section actions.
        GuiAction::ToggleCompareMode => {
            (F100Surface::LibraryGrid, Str::CompareModeCompare.t().into())
        }
        GuiAction::ClearCrop => (F100Surface::Geometry, Str::ClearCrop.t().into()),
        GuiAction::SetCropAspect => (F100Surface::Geometry, Str::Aspect.t().into()),
        GuiAction::RotateStep => (F100Surface::Geometry, Str::RotateLeft.t().into()),
        GuiAction::SetGeometryMirror => (F100Surface::Geometry, Str::MirrorHorizontal.t().into()),
        GuiAction::AnalyzeUpright => (F100Surface::Geometry, Str::UprightAnalyze.t().into()),
        GuiAction::SetUprightEnabled => (F100Surface::Geometry, Str::UprightEnable.t().into()),
        GuiAction::ClearUpright => (F100Surface::Geometry, Str::UprightClear.t().into()),
        GuiAction::CreateMask => (F100Surface::Masking, Str::NewMask.t().into()),
        GuiAction::SelectMask => (F100Surface::Masking, Str::SelectMask.t().into()),
        GuiAction::SetMaskVisible => (F100Surface::Masking, Str::MaskEye.t().into()),
        GuiAction::SetShowMaskOverlay => (F100Surface::Masking, Str::ShowOverlay.t().into()),
        GuiAction::SetOverlayColor => (F100Surface::Masking, Str::OverlayColor.t().into()),
        GuiAction::CreateAiMask => (F100Surface::Masking, Str::AddAiMask.t().into()),
        GuiAction::CreateLuminanceRangeMask => (F100Surface::Masking, Str::AddRange.t().into()),
        GuiAction::CreateColorRangeMask => (F100Surface::Masking, Str::AddRange.t().into()),
        GuiAction::CombineMasks => (F100Surface::Masking, Str::CombineAdd.t().into()),
        GuiAction::DuplicateMask => (F100Surface::Masking, Str::DuplicateMask.t().into()),
        GuiAction::SetOverlayMode => (F100Surface::Masking, Str::OverlayAlways.t().into()),
        GuiAction::SetPinVisibility => (F100Surface::Masking, Str::OverlayAlways.t().into()),
        GuiAction::SetSoloMode => (F100Surface::Masking, Str::SoloMode.t().into()),
        GuiAction::SetMaskInverted => (F100Surface::Masking, Str::Invert.t().into()),
        GuiAction::OfferMaskRecalculation => {
            (F100Surface::Masking, Str::OfferRecalculation.t().into())
        }
        GuiAction::AddKeyword => (F100Surface::Metadata, Str::AddKeyword.t().into()),
        GuiAction::RemoveKeyword => (F100Surface::Metadata, "✕".into()),
        GuiAction::CommitMetadataDraft => {
            (F100Surface::Metadata, Str::MetadataSaveDraft.t().into())
        }
        GuiAction::ClearMetadataDraft => {
            (F100Surface::Metadata, Str::MetadataClearDraft.t().into())
        }
        GuiAction::CopyMetadataDraft => (F100Surface::Metadata, Str::MetadataCopyDraft.t().into()),
        GuiAction::PasteMetadataDraft => {
            (F100Surface::Metadata, Str::MetadataPasteDraft.t().into())
        }
        GuiAction::ClearMetadataHistory => {
            (F100Surface::Metadata, Str::MetadataHistoryClear.t().into())
        }
        GuiAction::ApplyMetaPreset => (F100Surface::Metadata, Str::MetadataPresetApply.t().into()),
        GuiAction::SyncMetadata => (F100Surface::Metadata, Str::MetadataSyncButton.t().into()),
        // GUI-INSTRDBG-17b-REST: Spot extras, Detail/red-eye, Optics,
        // Tone Curve and Presets.
        GuiAction::SetSpotMode => (F100Surface::Spot, "Quick".into()),
        GuiAction::ClearSpotVisualize => (F100Surface::Spot, "Visualize off".into()),
        GuiAction::DetectSpotCandidates => (F100Surface::Spot, "Detect objects".into()),
        GuiAction::ApplyDetectedSpots => (F100Surface::Spot, "Apply detected".into()),
        GuiAction::RegenerateSpotVariant => (F100Surface::Spot, "Regenerate variant".into()),
        GuiAction::ClearSpotHeals => (F100Surface::Spot, "Clear spots".into()),
        GuiAction::DetectRedEye => (F100Surface::Detail, Str::RedEyeDetect.t().into()),
        GuiAction::ApplyDetectedRedEyes => {
            (F100Surface::Detail, Str::RedEyeApplyDetected.t().into())
        }
        GuiAction::RemoveRedEyeRegion => (F100Surface::Detail, Str::RedEyeRemove.t().into()),
        GuiAction::ClearRedEye => (F100Surface::Detail, Str::RedEyeClear.t().into()),
        GuiAction::SetLensProfile => (F100Surface::Optics, Str::LensProfile.t().into()),
        GuiAction::ClearLensProfile => (F100Surface::Optics, Str::LensProfile.t().into()),
        GuiAction::SetLensBlurBokeh => (F100Surface::Optics, Str::LensBlurBokehRound.t().into()),
        GuiAction::AddCurvePoint => (F100Surface::ToneCurve, Str::ToneCurveAddPoint.t().into()),
        GuiAction::RemoveCurvePoint => {
            (F100Surface::ToneCurve, Str::ToneCurveRemovePoint.t().into())
        }
        GuiAction::ApplyPreset => (F100Surface::Presets, Str::ApplyPreset.t().into()),
        GuiAction::SavePresetFile => (F100Surface::Presets, Str::SavePresetFile.t().into()),
        // GUI-INSTRDBG-17c: WB eyedropper, Point Color, Spot distraction,
        // Red-Eye picker, Presets refresh and the generative canvas.
        GuiAction::SetSpotDistraction => (F100Surface::Spot, "Dust".into()),
        GuiAction::SetRedEyePickMode => (F100Surface::Detail, Str::RedEyePickMode.t().into()),
        GuiAction::ReloadPresetEntries => (F100Surface::Presets, Str::Refresh.t().into()),
        GuiAction::ArmWbEyedropper => (F100Surface::Basic, Str::WbEyedropper.t().into()),
        GuiAction::AddPointColor => (F100Surface::Color, Str::PointColorAdd.t().into()),
        GuiAction::RemovePointColor => (F100Surface::Color, Str::PointColorRemove.t().into()),
        GuiAction::SetExpandCanvas => (
            F100Surface::Generative,
            "Apply Frame (drag) → Canvas".into(),
        ),
        GuiAction::GenerateCanvas => (F100Surface::Generative, Str::GenerateCanvas.t().into()),
        // GUI-INSTRDBG-17c-Rework: generative checkboxes, per-section
        // Previous/Reset and the lens-blur enable checkbox.
        GuiAction::SetExpandBeyondImage => {
            (F100Surface::Generative, Str::ExpandBeyondImage.t().into())
        }
        GuiAction::SetAutoFillTransparent => {
            (F100Surface::Generative, Str::AutoFillTransparent.t().into())
        }
        GuiAction::RestoreSectionPrevious => (F100Surface::Basic, Str::Previous.t().into()),
        GuiAction::ResetSection => (F100Surface::Basic, Str::Reset.t().into()),
        GuiAction::SetLensBlurEnabled => (F100Surface::Optics, Str::LensBlurEnable.t().into()),
        // GUI-INSTRDBG-17c-Rework F-1: the filmstrip selection-action row.
        GuiAction::SyncSettingsToSelection => {
            (F100Surface::Filmstrip, Str::SyncSettings.t().into())
        }
        GuiAction::MatchExposuresOfSelection => {
            (F100Surface::Filmstrip, Str::MatchSelection.t().into())
        }
        GuiAction::ApplyPreviousToSelection => {
            (F100Surface::Filmstrip, Str::PreviousImage.t().into())
        }
        // GUI-INSTRDBG-17c-Rework F-A: the KI-Denoise enable checkbox
        // (Detail section; recipe-mutating).
        GuiAction::SetDenoiseEnabled => (F100Surface::Detail, Str::DenoiseEnable.t().into()),
        // GUI-INSTRDBG-17c-Rest: the People-view per-face mask-source
        // button (creates a mask definition on the active virtual copy).
        GuiAction::CreateFaceMask => (F100Surface::People, Str::FaceUseAsMask.t().into()),
    }
}

/// Paint one F-100 button surface headless. `Develop` is painted before
/// `Basic`/`Masking` open their sections, so the footer surface cannot
/// gain extra section "Reset" texts.
pub(super) fn f100_surface_shapes(
    app: &mut LuminaApp,
    surface: F100Surface,
) -> Vec<egui::epaint::ClippedShape> {
    match surface {
        F100Surface::Preview => preview_area_shapes(app),
        F100Surface::Histogram => headless_shapes(app, |app, ui| app.draw_histogram_section(ui)),
        F100Surface::Rating => headless_click_labels(app, &[Str::Rating.t()], |app, ui| {
            app.draw_rating_section(ui)
        }),
        F100Surface::LibraryGrid => headless_shapes(app, |app, ui| {
            let ctx = ui.ctx().clone();
            app.draw_library_grid(&ctx, ui);
        }),
        F100Surface::History => headless_click_labels(app, &[Str::History.t()], |app, ui| {
            app.draw_history_section(ui)
        }),
        F100Surface::ModuleBar => headless_shapes(app, |app, ui| app.draw_module_bar(ui)),
        F100Surface::Develop => {
            headless_shapes_sized(app, 4096.0, |app, ui| app.draw_develop_panel(ui))
        }
        F100Surface::Export => {
            headless_shapes_sized(app, 2000.0, |app, ui| app.draw_export_panel(ui))
        }
        F100Surface::Basic => {
            app.set_section_open(SECTION_BASIC, true);
            headless_shapes_sized(app, 4096.0, |app, ui| app.draw_basic(ui))
        }
        F100Surface::Masking => {
            // Prepare the buttons that need a selected mask (the eye and
            // the recalculation offer). Selection is idempotent per frame.
            if let Ok(id) = app.create_mask("audit-mask") {
                let _ = app.select_mask(&id);
            }
            app.set_section_open(SECTION_MASKING, true);
            headless_shapes_sized(app, 4096.0, |app, ui| app.draw_masking(ui))
        }
        F100Surface::Spot => {
            headless_click_labels_sized(app, 6000.0, &["Dust Removal (Q)"], |app, ui| {
                app.draw_spot_heal(ui)
            })
        }
        F100Surface::Merge => headless_click_labels(app, &[Str::MergeSection.t()], |app, ui| {
            app.draw_merge_section(ui)
        }),
        F100Surface::Geometry => {
            // Prepare the conditional buttons: an active crop (Clear Crop)
            // and a persisted upright analysis (Clear Upright).
            let _ = app.set_crop_aspect("1:1");
            let _ = app.analyze_upright_now();
            app.set_section_open(SECTION_GEOMETRY, true);
            headless_shapes_sized(app, 4096.0, |app, ui| app.draw_geometry(ui))
        }
        F100Surface::Metadata => {
            // The panel's sub-sections start collapsed; open the ones that
            // host the audited buttons and add a keyword so the per-keyword
            // remove button paints.
            let _ = app.add_keyword("audit");
            let labels = [
                Str::MetadataDraftSection.t(),
                Str::KeywordsSection.t(),
                Str::History.t(),
                Str::MetadataPresetSection.t(),
                Str::MetadataSyncSection.t(),
            ];
            headless_click_labels_sized(app, 4096.0, &labels, |app, ui| {
                app.draw_library_metadata_panel(ui)
            })
        }
        F100Surface::Detail => {
            // A persisted red-eye region paints the per-region remove and
            // the "Clear all" button; the section is opened via state so the
            // tall panel keeps every control in one pass.
            let _ = app.add_red_eye_region(0.5, 0.5);
            app.set_section_open(SECTION_DETAIL, true);
            headless_shapes_sized(app, 8000.0, |app, ui| app.draw_detail(ui))
        }
        F100Surface::Optics => {
            // The bokeh radios live in the collapsed "Lens Blur" subgroup,
            // so click it open before asserting the shape of the panel.
            app.set_section_open(SECTION_OPTICS, true);
            headless_click_labels_sized(app, 8000.0, &[Str::LensBlur.t()], |app, ui| {
                app.draw_optics(ui)
            })
        }
        F100Surface::ToneCurve => {
            // One interior control point paints the per-point "Remove"
            // button (endpoints are mandatory and never removable).
            app.set_section_open(SECTION_TONE_CURVE, true);
            app.add_curve_point("master", 0.5, 0.5);
            headless_shapes_sized(app, 8000.0, |app, ui| app.draw_tone_curve(ui))
        }
        F100Surface::Presets => {
            // A path keeps the Refresh button painted while no real user
            // presets directory is scanned during the audit frame; the
            // Apply/Save buttons always paint.
            app.presets_dir = Some(std::path::PathBuf::from("instrdbg-presets"));
            headless_click_labels(app, &[Str::PresetsSection.t()], |app, ui| {
                app.draw_presets_section(ui)
            })
        }
        F100Surface::Color => {
            // One Point Color entry paints the per-entry "Remove" button;
            // the trailing "Add color" button always paints.
            app.set_section_open(SECTION_COLOR, true);
            app.add_point_color();
            headless_shapes_sized(app, 8000.0, |app, ui| app.draw_color(ui))
        }
        F100Surface::Generative => {
            // An active expand role with a canvas paints the "Apply Frame"
            // button; `generative_stage_active` then paints "Generate".
            let _ = app.set_expand_beyond_image(true);
            headless_click_labels_sized(app, 2000.0, &["Generative Expand"], |app, ui| {
                app.draw_generative_expand(ui)
            })
        }
        F100Surface::Filmstrip => {
            // The selection row paints with or without a selection; clear
            // the startup auto-selection so the plain (counter-free) Sync
            // label is audited, mirroring `f100_action_button`.
            app.filmstrip_selection.clear();
            headless_shapes(app, |app, ui| {
                let ctx = ui.ctx().clone();
                app.draw_filmstrip(&ctx, ui);
            })
        }
        F100Surface::People => {
            // GUI-INSTRDBG-17c-Rest: the People view needs a persisted,
            // valid face analysis (selected cluster + detection) and a
            // loaded frame so the per-face "Use as mask" button paints.
            // `seed_face` writes the exact embedding records the persisted
            // checksums reference, so the analysis is `valid` (not a
            // presence-only false positive).
            crate::face_gui::tests::seed_face(app);
            app.set_library_view(LibraryView::People);
            app.people_selected_cluster = "cluster-a".into();
            headless_shapes_sized(app, 4096.0, |app, ui| {
                let ctx = ui.ctx().clone();
                app.draw_library_grid(&ctx, ui);
            })
        }
    }
}

#[test]
fn f100_shortcut_audit_every_action_has_a_button() {
    let (_directory, mut app) = persistent_app();
    // Keep the Develop footer surface before Basic/Masking open sections.
    let order = [
        F100Surface::Preview,
        F100Surface::Histogram,
        F100Surface::Rating,
        F100Surface::LibraryGrid,
        F100Surface::History,
        F100Surface::ModuleBar,
        F100Surface::Develop,
        F100Surface::Export,
        F100Surface::Basic,
        F100Surface::Masking,
        F100Surface::Spot,
        F100Surface::Merge,
        F100Surface::Geometry,
        F100Surface::Metadata,
        F100Surface::Detail,
        F100Surface::Optics,
        F100Surface::ToneCurve,
        F100Surface::Presets,
        F100Surface::Color,
        F100Surface::Generative,
        F100Surface::Filmstrip,
        F100Surface::People,
    ];
    for surface in order {
        let shapes = f100_surface_shapes(&mut app, surface);
        for action in ALL_GUI_ACTIONS {
            let (action_surface, label) = f100_action_button(*action);
            assert!(!label.is_empty(), "{action:?} must map to a button label");
            if action_surface == surface {
                assert_fully_visible(&shapes, &label);
            }
        }
    }
    // Every instrumented action is in the audit table (paired with the
    // exhaustive match above: a new shortcut cannot compile unmapped).
    for action in ALL_GUI_ACTIONS {
        let (_, label) = f100_action_button(*action);
        assert!(!label.is_empty(), "{action:?} has no button");
    }
}
