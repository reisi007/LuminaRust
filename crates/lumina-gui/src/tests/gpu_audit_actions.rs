//! GUI-GPU-AUDIT-17: the exhaustive `GuiAction` → handler dispatch for the
//! headless routing audit. Extracted from `tests/gpu_audit.rs` so both files
//! stay inside the 500-line ratchet. The match has no `_` arm: a new
//! instrumented action cannot be added without being audited.
#![cfg(feature = "gpu")]

use super::*;

/// Synthetic source geometry: large enough for the spatial stages to act,
/// small enough that 100+ full renders stay a fast headless run. Shared with
/// the audit assertions in `tests/gpu_audit.rs`.
pub(super) const AUDIT_SRC_W: u32 = 64;
pub(super) const AUDIT_SRC_H: u32 = 48;

/// Drive one action through its real handler. Results are deliberately
/// ignored: this audit proves the routing consequence, not the action
/// semantics (those have dedicated tests). Preparation steps an action needs
/// (a selected mask, an upright analysis, an armed expand role) run inside the
/// same arm so the action is exercised on its real precondition.
///
/// The match is exhaustive over [`GuiAction`] with no `_` arm: a new
/// instrumented action cannot be added without being audited here.
pub(super) fn drive_action(app: &mut LuminaApp, action: GuiAction, export_path: &std::path::Path) {
    match action {
        // GUI-INSTRDBG-17 base surface.
        GuiAction::ToggleBeforeAfter => app.toggle_before_after(),
        GuiAction::ToggleSplitView => app.toggle_split_view(),
        GuiAction::ToggleCropMode => app.toggle_crop_mode(),
        GuiAction::ToggleClipping => app.toggle_clipping_overlay(),
        GuiAction::ToggleSoftproof => app.toggle_softproof_preview(),
        GuiAction::ToggleOriginalHistogram => app.toggle_original_histogram(),
        GuiAction::ToggleLightsOut => app.toggle_lights_out(),
        GuiAction::TogglePanelsHidden => app.toggle_panels_hidden(),
        GuiAction::ToggleAllPanelsHidden => app.toggle_all_panels_hidden(),
        GuiAction::ToggleFullscreen => app.toggle_fullscreen(),
        GuiAction::ToggleFilterBar => app.toggle_filter_bar(),
        GuiAction::ToggleBlackWhite => {
            let _ = app.toggle_black_white();
        }
        GuiAction::ToggleStackGroup => {
            let _ = app.toggle_stack_group();
        }
        GuiAction::CreateSnapshot => {
            let _ = app.create_snapshot("gpu-audit");
        }
        GuiAction::DuplicateCopy => {
            let _ = app.duplicate_active_copy();
        }
        GuiAction::CopySettings => {
            let _ = app.copy_settings();
        }
        GuiAction::PasteSettings => {
            let _ = app.paste_settings();
        }
        GuiAction::SetRating => {
            let _ = app.set_rating(3);
        }
        GuiAction::SetFlag => {
            let _ = app.set_flag(Flag::Pick);
        }
        GuiAction::SetColorLabel => {
            let _ = app.set_color_label(1);
        }
        GuiAction::SetMaskTool => app.set_mask_tool(MaskTool::Brush),
        GuiAction::SetSpotTool => app.set_spot_tool(SpotTool::Heal),
        GuiAction::SetTreatment => {
            let _ = app.set_treatment("color");
        }
        GuiAction::SetModule => app.set_module(Module::Develop),
        GuiAction::SetLibraryView => app.set_library_view(LibraryView::Grid),
        GuiAction::SetLibrarySort => {
            let _ = app.set_library_sort(LibrarySort::Name);
        }
        GuiAction::SetZoomMode => app.set_zoom_mode(ZoomMode::Fit),
        GuiAction::RegenerateStale => {
            let _ = app.regenerate_stale();
        }
        GuiAction::MatchExposure => {
            let _ = app.match_total_exposure(0.0);
        }
        GuiAction::AutoTone => {
            let _ = app.auto_tone();
        }
        GuiAction::SaveRecipe => app.save_recipe_action(),
        GuiAction::Reset => app.reset(),
        GuiAction::Render => {
            let _ = app.render();
        }
        GuiAction::Export => {
            let _ = app.export_to(export_path.to_path_buf());
        }
        GuiAction::StartMerge => {
            let _ = app.start_merge(lumina_sidecar::MergeMode::Hdr);
        }
        // GUI-INSTRDBG-17b: Library compare, Geometry, Masking-layer, Metadata.
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
            // The checkbox is only reachable with a persisted analysis.
            let _ = app.analyze_upright_now();
            let _ = app.set_upright_enabled(true);
        }
        GuiAction::ClearUpright => app.clear_upright(),
        GuiAction::CreateMask => {
            let _ = app.create_mask("gpu-audit-mask");
        }
        GuiAction::SelectMask => {
            if let Ok(id) = app.create_mask("gpu-audit-mask") {
                let _ = app.select_mask(&id);
            }
        }
        GuiAction::SetMaskVisible => {
            if let Ok(id) = app.create_mask("gpu-audit-mask") {
                let _ = app.set_mask_visible(&id, false);
            }
        }
        GuiAction::SetShowMaskOverlay => {
            let shown = app.show_mask_overlay();
            app.set_show_mask_overlay(!shown);
        }
        GuiAction::SetOverlayColor => app.set_overlay_color([1, 2, 3]),
        GuiAction::CreateAiMask => {
            let _ = app.create_ai_mask(AiSelectKind::Subject, None, "gpu-audit-ai");
        }
        GuiAction::CreateLuminanceRangeMask => {
            let _ = app.create_luminance_range_mask(0.1, 0.9, 0.2, "gpu-audit-lum");
        }
        GuiAction::CreateColorRangeMask => {
            let _ =
                app.create_color_range_mask(10.0, 20.0, 0.1, 0.9, 0.1, 0.9, 0.2, "gpu-audit-col");
        }
        GuiAction::CombineMasks => {
            if let Ok(id) = app.create_mask("gpu-audit-mask") {
                let _ = app.select_mask(&id);
                let _ = app.combine_masks(MaskOperation::Invert, "", "gpu-audit-combined");
            }
        }
        GuiAction::DuplicateMask => {
            if let Ok(id) = app.create_mask("gpu-audit-mask") {
                let _ = app.duplicate_mask(&id, "gpu-audit-copy");
            }
        }
        GuiAction::SetOverlayMode => app.set_overlay_mode(OverlayMode::Never),
        GuiAction::SetPinVisibility => app.set_pin_visibility(PinVisibility::Never),
        GuiAction::SetSoloMode => {
            let enabled = app.solo_mode();
            app.set_solo_mode(!enabled);
        }
        GuiAction::SetMaskInverted => {
            if let Ok(id) = app.create_mask("gpu-audit-mask") {
                let _ = app.select_mask(&id);
                let _ = app.set_mask_inverted(true);
            }
        }
        GuiAction::OfferMaskRecalculation => {
            if let Ok(id) = app.create_mask("gpu-audit-mask") {
                let _ = app.select_mask(&id);
                let _ = app.offer_mask_recalculation();
            }
        }
        GuiAction::AddKeyword => {
            let _ = app.add_keyword("gpu-audit");
        }
        GuiAction::RemoveKeyword => {
            let _ = app.add_keyword("gpu-audit");
            let _ = app.remove_keyword("gpu-audit");
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
            let _ = app.copy_metadata_draft();
            let _ = app.paste_metadata_draft();
        }
        GuiAction::ClearMetadataHistory => {
            let _ = app.clear_metadata_history_gui();
        }
        GuiAction::ApplyMetaPreset => {
            let _ = app.apply_meta_preset_loaded("gpu-audit-missing", &BTreeMap::new());
        }
        GuiAction::SyncMetadata => {
            let mut fields = BTreeSet::new();
            fields.insert("description".to_string());
            app.sync_metadata_to_selection(&fields);
        }
        // GUI-INSTRDBG-17b-REST: Spot extras, Detail/red-eye, Optics,
        // Tone Curve, Presets.
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
            let _ = app.regenerate_spot_variant("gpu-audit-missing");
        }
        GuiAction::ClearSpotHeals => app.clear_spot_heals(),
        // R5-DUST-23-FOLLOWUP: spot selection + per-spot editing.
        GuiAction::SelectSpot => {
            let _ = app.commit_spot_heal(
                lumina_sidecar::Point2 { x: 0.2, y: 0.2 },
                2.0,
                0.0,
                lumina_sidecar::Point2 { x: 0.1, y: 0.0 },
                1.0,
            );
            if let Some(entry) = app.spot_entries().into_iter().next() {
                if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                    let id = id.to_string();
                    let _ = app.select_spot(&id);
                }
            }
        }
        GuiAction::UpdateSpot => {
            let _ = app.commit_spot_heal(
                lumina_sidecar::Point2 { x: 0.2, y: 0.2 },
                2.0,
                0.0,
                lumina_sidecar::Point2 { x: 0.1, y: 0.0 },
                1.0,
            );
            if let Some(entry) = app.spot_entries().into_iter().next() {
                if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                    let id = id.to_string();
                    let _ = app.update_spot_heal(
                        &id,
                        3.0,
                        0.1,
                        0.9,
                        lumina_sidecar::Point2 { x: 0.05, y: 0.0 },
                    );
                }
            }
        }
        GuiAction::RemoveSpot => {
            let _ = app.commit_spot_heal(
                lumina_sidecar::Point2 { x: 0.2, y: 0.2 },
                2.0,
                0.0,
                lumina_sidecar::Point2 { x: 0.1, y: 0.0 },
                1.0,
            );
            if let Some(entry) = app.spot_entries().into_iter().next() {
                if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                    let id = id.to_string();
                    let _ = app.remove_spot(&id);
                }
            }
        }
        GuiAction::DetectRedEye => {
            let _ = app.detect_red_eye_candidates();
        }
        GuiAction::ApplyDetectedRedEyes => {
            let _ = app.apply_detected_red_eye_objects();
        }
        GuiAction::RemoveRedEyeRegion => {
            let _ = app.add_red_eye_region(0.5, 0.5);
            app.remove_red_eye_region("re-1");
        }
        GuiAction::ClearRedEye => {
            let _ = app.add_red_eye_region(0.5, 0.5);
            app.clear_red_eye();
        }
        GuiAction::SetLensProfile => {
            let _ = app.set_lens_profile("wide-light");
        }
        GuiAction::ClearLensProfile => app.clear_lens_profile(),
        GuiAction::SetLensBlurBokeh => app.set_lens_blur_bokeh(BokehShape::Hexagonal),
        GuiAction::AddCurvePoint => app.add_curve_point("master", 0.4, 0.4),
        GuiAction::RemoveCurvePoint => {
            app.add_curve_point("master", 0.5, 0.5);
            app.remove_curve_point("master", 1);
        }
        GuiAction::ApplyPreset => {
            if let Ok(preset) = app.create_preset("gpu-audit") {
                let _ = app.apply_preset(&preset);
            }
        }
        GuiAction::SavePresetFile => {
            app.preset_name = "gpu-audit".into();
            let _ = app.save_current_selection_as_preset_file();
        }
        // GUI-INSTRDBG-17c: WB eyedropper, Point Color, Spot distraction,
        // Red-Eye picker, Presets refresh, generative canvas.
        GuiAction::SetSpotDistraction => app.set_spot_distraction(SpotDistraction {
            reflections: true,
            people: false,
            dust: true,
            auto_mode: false,
        }),
        GuiAction::SetRedEyePickMode => app.set_red_eye_pick_mode(true),
        GuiAction::ReloadPresetEntries => app.reload_preset_entries(),
        GuiAction::ArmWbEyedropper => app.arm_wb_picker(),
        GuiAction::AddPointColor => app.add_point_color(),
        GuiAction::RemovePointColor => {
            app.add_point_color();
            app.remove_point_color("pc-1");
        }
        GuiAction::SetExpandCanvas => {
            let _ = app.set_expand_canvas(GenerativeCanvas {
                output_width: AUDIT_SRC_W + 4,
                output_height: AUDIT_SRC_H + 4,
                source_offset_x: 2,
                source_offset_y: 2,
                extras: Default::default(),
            });
        }
        GuiAction::GenerateCanvas => {
            let _ = app.set_expand_beyond_image(true);
            let _ = app.generate_generative_canvas();
        }
        // GUI-INSTRDBG-17c-Rework.
        GuiAction::SetExpandBeyondImage => {
            let _ = app.set_expand_beyond_image(true);
        }
        GuiAction::SetAutoFillTransparent => {
            let _ = app.set_auto_fill_transparent(true);
        }
        GuiAction::RestoreSectionPrevious => {
            let _ = app.restore_section_previous(SECTION_BASIC);
        }
        GuiAction::ResetSection => {
            let _ = app.reset_section(SECTION_BASIC);
        }
        GuiAction::SetLensBlurEnabled => app.set_lens_blur_enabled(true),
        // GUI-INSTRDBG-17c-Rework F-1: filmstrip selection buttons.
        GuiAction::SyncSettingsToSelection => {
            let _ = app.sync_settings_to_selection();
        }
        GuiAction::MatchExposuresOfSelection => {
            let _ = app.match_exposures_of_selection();
        }
        GuiAction::ApplyPreviousToSelection => {
            let _ = app.apply_previous_to_selection();
        }
        // GUI-INSTRDBG-17c-Rework F-A: KI-Denoise enable checkbox.
        GuiAction::SetDenoiseEnabled => {
            let _ = app.set_denoise_enabled(true);
        }
        // GUI-INSTRDBG-17c-Rest: People-view "Use as mask".
        GuiAction::CreateFaceMask => {
            let _ = app.create_face_mask("gpu-audit-detection", "gpu-audit-face-mask");
        }
    }
}
