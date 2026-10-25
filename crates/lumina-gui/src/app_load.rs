//! Source open/decode lifecycle: decode start, decode drain, neighbor previews,
//! toasts, the deferred slider commit and the save/render actions.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    // LRPAR-G08-PREVIOUS: the filmstrip selection actions (Sync/Match/Previous)
    // and their sidecar helpers moved to `crate::selection_actions`.
    pub fn load_bytes(&mut self, bytes: Vec<u8>, name: impl Into<String>) -> Result<(), GuiError> {
        let name = name.into();
        // GUI-SIDECAR-READ-1: same flush as `open_file` — a dropped file
        // replaces the source through `apply_decoded_frame`, which drops an
        // armed commit (no-op without a file-backed image loaded).
        self.flush_pending_edit();
        // A synchronous bytes-only load is the newest source request. Cancel an
        // older worker so it cannot overwrite these pixels after they land.
        let superseded_decode = self.decode_rx.take().is_some();
        self.decode_generation += 1;
        self.pending_load_path = None;
        self.pending_directory_open = None;
        if superseded_decode {
            self.note_decode_failed();
        }
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
        // Bytes have no adjacent sidecar path. Detach only after a successful
        // decode so a failed bytes-only load still preserves the current source.
        self.path.clear();
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
    pub(crate) fn apply_decoded_frame(
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
            self.mask_rename_input.clear();
            self.mask_rename_inputs.clear();
            self.selected_spot_id = None;
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
        self.effective_source_stage = None;
        self.effective_source_stage_digest = None;
        self.tone_analysis = None;
        self.preview_histogram = None;
        self.pending_slider_commit = None;
        self.pending_history_step = None;
        self.pending_mask_state_before = None;
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
            self.brush_mask_plane_scope = None;
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
        if !self.scan_pending {
            self.status = Str::Loaded.format_arg(&self.source_name);
        }
        info!(
            "loaded image {} (raw={}, camera_white_balance={:?})",
            self.source_name, source_is_raw, self.camera_white_balance
        );
    }

    /// Surface a user-visible failure loudly (DoD §4): the message goes to the
    /// log at `error!` level, the status line switches to "Error" and the
    /// message is shown both in the header and in the error popup dialog
    /// (KITTEST-COVERAGE-STATES-1). Explicit user-action failures pop the
    /// dialog; background failures use [`Self::show_error_banner`].
    pub(crate) fn show_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        error!("{message}");
        self.status = Str::Error.t().into();
        self.error = Some(message);
        self.error_dialog = true;
    }

    /// Background/automatic failures (decode, listing) stay a header banner +
    /// log without stealing focus with a dialog; the loud signal is not lost
    /// (status + `self.error` + `error!`), only the modal surface is skipped.
    pub(crate) fn show_error_banner(&mut self, error: impl ToString) {
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
    pub(crate) fn draw_error_dialog(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn close_error_dialog(&mut self) {
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
    pub(crate) fn begin_load_path(&mut self, path: String) {
        self.begin_load_path_with_policy(path, DirectoryOpenPolicy::None);
    }

    /// Start a native path-drop decode whose directory navigation is deferred
    /// until `finish_decode` confirms that the image decoded successfully.
    pub(crate) fn begin_load_path_deferred(&mut self, path: String) {
        self.begin_load_path_with_policy(path, DirectoryOpenPolicy::Deferred);
    }

    fn begin_load_path_with_policy(&mut self, path: String, directory_policy: DirectoryOpenPolicy) {
        if path.trim().is_empty() {
            return;
        }
        // A newer source request invalidates any listing that was waiting to
        // schedule neighbors for the previous target.
        self.pending_directory_open = None;
        // Latest-wins decode identity. Dropping the old receiver prevents its
        // handoff; the generation check also rejects an already-staged stale
        // result before it can adopt deferred directory state.
        if self.decode_rx.take().is_some() {
            self.note_decode_failed();
        }
        self.decode_generation += 1;
        let request_generation = self.decode_generation;
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
        // PREVIEW-CACHE-FEATURE (A1/A4): ordinary opens may paint a cached
        // neighbor immediately. A deferred drop must preserve A's complete
        // preview/source lineage until B actually decodes, so it skips this
        // transient adoption and paints only through `finish_decode` on success.
        if directory_policy == DirectoryOpenPolicy::None {
            self.paint_cached_neighbor_preview(&path);
        }
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
            let _ = tx.send((request_generation, directory_policy, result));
        });
    }

    /// Apply a completed background decode: set the source, then restore the
    /// sidecar recipe for that path (mirroring the old synchronous `load_path`).
    fn finish_decode(&mut self, result: DecodeResult, directory_policy: DirectoryOpenPolicy) {
        // R3-OPEN-1: the in-flight decode is settled either way.
        self.pending_load_path = None;
        match result {
            Ok(frame) => {
                self.note_decode_finish(frame.frame.width(), frame.frame.height());
                // Validate the sidecar against the exact bytes just decoded
                // before changing the active source. A stale sidecar must not
                // be adopted merely because its recipe happens to be valid.
                let path = PathBuf::from(&frame.path);
                let sidecar_path = sidecar_path_for(&path);
                let sidecar_exists = sidecar_path.exists();
                let document = if sidecar_exists {
                    match load_sidecar(&sidecar_path) {
                        Ok(document) => {
                            let live = crate::source_actions::source_fingerprint(&frame.bytes);
                            if !crate::source_actions::source_fingerprint_matches(
                                &document.source,
                                &live,
                            ) {
                                let message = format!(
                                    "source identity conflict: source content conflict between decoded `{}` (hash {}, byte length {}) and sidecar `{}` (hash {}, byte length {}); sidecar not adopted and repair actions were not applied",
                                    path.display(),
                                    live.content_hash,
                                    live.byte_length,
                                    sidecar_path.display(),
                                    document.source.content_hash,
                                    document.source.byte_length,
                                );
                                self.pending_directory_open = None;
                                if self.original.is_none() {
                                    self.path = frame.path.clone();
                                }
                                self.invalidate_source_action_preview();
                                self.show_error_banner(message);
                                return;
                            }
                            Some(document)
                        }
                        Err(error) => {
                            let message = format!(
                                "could not load sidecar `{}`: {error}",
                                sidecar_path.display()
                            );
                            self.pending_directory_open = None;
                            if self.original.is_none() {
                                self.path = frame.path.clone();
                            }
                            self.invalidate_source_action_preview();
                            self.show_error_banner(message);
                            return;
                        }
                    }
                } else {
                    None
                };
                // GUI-SIDECAR-READ-1: only a validated target may displace the
                // still-loaded image; flush its armed edit before adoption.
                if !self.flush_pending_edit() {
                    self.pending_directory_open = None;
                    return;
                }
                // LRPAR-G08-PREVIOUS: a successful switch to a different
                // image displaces the current one — stash it (path + recipe
                // snapshot) as the cross-image Previous reference before the
                // new path is adopted below.
                let displaced = (!self.path.trim().is_empty()
                    && self.path != frame.path
                    && self.original.is_some())
                .then(|| self.previous_reference_for(self.path.clone()));
                self.path = frame.path.clone();
                if let Some(reference) = displaced {
                    info!(
                        "previous reference: {} (displaced by {})",
                        reference.path, self.path
                    );
                    self.previous_reference = Some(reference);
                }
                self.apply_decoded_frame(
                    &frame.frame,
                    frame.orientation,
                    frame.camera_white_balance,
                    &frame.name,
                    &frame.bytes,
                    frame.source_is_raw,
                    frame.lens_identity,
                );
                // Do not render once with the reset default recipe: a valid
                // sidecar may contain persisted SourceActions, and a corrupt
                // sidecar must not silently degrade to the original image.
                // Adopt the sidecar first, then perform the single authoritative
                // render below (or render the clean default when no sidecar
                // exists).
                if let Some(document) = document {
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
                        self.selected_mask_id = Self::first_local_mask_id(&copy);
                        self.mask_rename_input = self
                            .selected_mask_id
                            .as_deref()
                            .and_then(|id| copy.mask_library.iter().find(|mask| mask.id == id))
                            .map(|mask| mask.name.clone())
                            .unwrap_or_default();
                        self.mask_rename_inputs.clear();
                        // R5-DUST-23-FOLLOWUP: a reloaded sidecar restores the
                        // removals, never the (session-only) spot selection.
                        self.selected_spot_id = None;
                        self.document = Some(document);
                        let config = AutoToneConfig {
                            target_luminance: candidate.auto_features.target_luminance,
                            ..Default::default()
                        };
                        // AUTO-TONE-ANALYSIS-INPUT-8: the freshness check must
                        // measure the SAME analysis domain the writer did (post
                        // SourceActions + post Crop), against the CANDIDATE
                        // recipe — otherwise a freshly auto-toned sidecar would
                        // look stale on every reload. An analysis frame that
                        // cannot be built proves nothing, so the state counts as
                        // not-fresh (the conservative direction: the values are
                        // recomputed) and the authoritative render below still
                        // reports the underlying bundle problem loudly.
                        let fingerprint = self
                            .auto_analysis_frame_for(&candidate)
                            .ok()
                            .map(|frame| tone_fingerprint(&frame, config));
                        let valid = fingerprint.is_some_and(|fingerprint| {
                            candidate
                                .auto_features
                                .analysis_fingerprint
                                .as_ref()
                                .is_some_and(|stored| {
                                    is_current_tone_analysis(stored, &fingerprint)
                                })
                        });
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
                } else if !sidecar_exists {
                    if let Err(e) = self.render() {
                        error!("render after load failed for {}: {e}", self.source_name);
                        self.show_error(e);
                    }
                }
                // Navigation is adopted only after the decoded image and its
                // sidecar are fully applied. This keeps the scan status (and
                // therefore the target listing) authoritative for neighbors.
                self.adopt_directory_after_decode(&frame.path, directory_policy);
                self.schedule_neighbor_previews_after_open(&frame.path);
            }
            Err((path, message)) => {
                self.pending_directory_open = None;
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
    pub(crate) fn poll_decode(&mut self) {
        let Some(rx) = &self.decode_rx else {
            return;
        };
        match rx.try_recv() {
            Ok((request_generation, directory_policy, result)) => {
                self.decode_rx = None;
                if request_generation != self.decode_generation {
                    trace!(
                        "GUI decode worker: dropping superseded result (gen {} != {})",
                        request_generation,
                        self.decode_generation
                    );
                    return;
                }
                self.finish_decode(result, directory_policy);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.decode_rx = None;
                // R3-OPEN-1: a dropped decode sender must not leave the
                // in-flight anchor behind (it would suppress a later open).
                let path = self
                    .pending_load_path
                    .take()
                    .unwrap_or_else(|| "unknown image".into());
                self.pending_directory_open = None;
                self.note_decode_failed();
                self.show_error_banner(format!("background decode worker disconnected for {path}"));
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
    pub(crate) fn poll_neighbor_previews(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn adopt_neighbor_preview_frame(&mut self, frame: ImageFrame) {
        let (width, height) = (frame.width(), frame.height());
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
    pub(crate) fn toast_anchor(viewport: egui::Vec2) -> egui::Pos2 {
        egui::pos2((viewport.x * 0.5 - 150.0).max(0.0), 100.0)
    }

    /// Expire the toast past its deadline and keep a visible one alive across
    /// frames by scheduling the repaint exactly at its deadline
    /// (GUI-TOAST-OVERLAP-1): without the timed repaint egui would sleep and
    /// the toast would linger until the next unrelated input.
    pub(crate) fn update_toast(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn draw_toast(&mut self, ctx: &egui::Context) {
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
    pub(crate) fn neighbor_preview_badge(&self, probe_id: &str) -> Option<(String, egui::Color32)> {
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
    pub(crate) fn commit_pending_slider_save(&mut self, viewport: [u32; 2]) -> bool {
        if let Err(error) = self.render_full(viewport, None) {
            self.show_error(error);
            return false;
        }
        let pending = self.pending_slider_commit.clone();
        if pending.is_none() && self.pending_history_step.is_none() {
            return true;
        }
        match self.save_sidecar_result() {
            Ok(()) => {
                if let Some((key, value)) = pending {
                    info!("{key}={value} saved");
                }
                true
            }
            Err(error) => {
                // The candidate save is retryable. Restore the debounce token
                // as well as the history/mask fields retained by the checked
                // save path, so a transient I/O/CAS failure cannot drop the
                // edit or turn the next commit into a different history step.
                if let Some(pending) = pending {
                    self.pending_slider_commit = Some(pending);
                }
                self.show_error(error);
                false
            }
        }
    }

    /// GUI-CLICK-ALL-17 + GUI-INSTRDBG-17: the Save Recipe footer button's
    /// action. Instrumented separately from the shared [`Self::save_sidecar`]
    /// helper because slider-debounce commits call that helper too, and a
    /// slider commit is not a button action.
    pub(crate) fn save_recipe_action(&mut self) {
        instrument_gui_action!(self, GuiAction::SaveRecipe);
        self.save_sidecar();
    }

    /// GUI-CLICK-ALL-17 + GUI-INSTRDBG-17: the Render/Apply footer button's
    /// action. Instrumented separately from [`Self::render`] because many
    /// section actions call `render` as a sub-step (they are instrumented
    /// themselves; nested suppression keeps exactly one line per action).
    pub(crate) fn render_action(&mut self) {
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
    pub(crate) fn save_sidecar(&mut self) {
        if let Err(error) = self.save_sidecar_result() {
            self.show_error(error);
        }
    }
}
