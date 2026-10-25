//! Construction, session/status accessors, clipping/histogram readouts and the
//! small quick-edit verbs (quick develop, rating/flag, duplicate copy).
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

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
            effective_source_stage: None,
            effective_source_stage_digest: None,
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
            pending_mask_state_before: None,
            loaded_lens_identity: None,
            #[cfg(feature = "lensfun")]
            lensfun_cache: None,
            render_mask_layers: Vec::new(),
            document: None,
            virtual_copy_id: "vc-original".into(),
            selected_mask_id: None,
            mask_name_input: String::new(),
            mask_rename_input: String::new(),
            mask_rename_inputs: BTreeMap::new(),
            mask_tool: MaskTool::None,
            brush_radius: 0.05,
            brush_softness: 0.0,
            brush_flow: 1.0,
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
            selected_spot_id: None,
            spot_detect_threshold: 0.5,
            spot_detect_status: String::new(),
            spot_gen_prompt: String::new(),
            spot_gen_seed: 7,
            spot_gen_variant: 1,
            spot_gen_status: String::new(),
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
            mask_overlay_mode: MaskOverlayMode::default(),
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
            local_wb_pick_mode: false,
            mask_local_curve_channel: 0,
            mask_local_hsl_band: 0,
            mask_local_grading_range: 0,
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
            decode_generation: 0,
            scan_rx: None,
            scan_generation: 0,
            scan_pending: false,
            pending_directory_open: None,
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
            vram_mask_plane_clear: crate::present_mask_plane::VramMaskPlaneClear::default(),
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
            #[cfg(feature = "gpu")]
            brush_mask_plane_scope: None,
            #[cfg(feature = "gpu")]
            live_brush_plane_stale: false,
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
    pub(crate) fn flush_pending_edit(&mut self) -> bool {
        if self.pending_slider_commit.is_none() && self.pending_history_step.is_none() {
            return true;
        }
        if self.original.is_none() || self.path.trim().is_empty() {
            // Bytes-only sessions have no persistence target; their documented
            // source-switch policy drops the armed UI edit on replacement.
            return true;
        }
        // LRPAR-G01-BASIC: with "Reset Sliders Automatically" armed, an image
        // switch discards the armed-but-uncommitted edit (sliders reset)
        // instead of flushing it to the previous image's sidecar.
        // Persisted state is untouched either way.
        if self.reset_sliders_automatically {
            self.pending_slider_commit = None;
            self.pending_history_step = None;
            self.pending_mask_state_before = None;
            info!("{}", Str::ResetSlidersDropped.t());
            self.status = Str::ResetSlidersDropped.t().into();
            return true;
        }
        trace!("GUI save: flushing pending edit before source change");
        self.commit_pending_slider_save([0, 0])
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

    pub(crate) fn ensure_document_loaded(&mut self) -> Result<(), GuiError> {
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

    pub(crate) fn active_copy_mut(&mut self) -> Result<&mut lumina_sidecar::VirtualCopy, GuiError> {
        self.document
            .as_mut()
            .and_then(|d| {
                d.virtual_copies
                    .iter_mut()
                    .find(|c| c.id == self.virtual_copy_id)
            })
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))
    }
}
