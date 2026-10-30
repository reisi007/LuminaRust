//! Generative expand/canvas: the artifact resolution, the canvas request and the
//! persisted canvas record.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
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
                .map(|f| (f.width(), f.height()))
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
                        .validate_with_source(frame.width(), frame.height())
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
                .validate_with_source(frame.width(), frame.height())
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
    // reference (AGENTS.md: GPU is acceleration only, never a requirement).

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
    pub(crate) fn generative_stage_active(&self) -> bool {
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
    pub(crate) fn generative_status_text(&self) -> String {
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
    fn generative_memo_key(&mut self, source_actions: &ResolvedSourceActions) -> String {
        let source_hash = self.resolved_source_hash();
        let recipe = serde_json::to_vec(&self.recipe).unwrap_or_default();
        let mut key = format!("{source_hash}|blake3:{}", blake3::hash(&recipe).to_hex());
        for identity in source_actions.identities() {
            key.push('|');
            key.push_str(&identity.id);
            key.push('@');
            key.push_str(&identity.checksum);
        }
        key
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
        // GUI-SRCACC-1: generative inputs are downstream of SourceActions and
        // must observe the same repaired pixels as preview/export.
        let source_actions = match self.resolve_current_source_actions(&self.recipe, &frame) {
            Ok(actions) => actions,
            Err(error) => {
                self.invalidate_source_action_preview();
                return Err(GuiError::Io(error.to_string()));
            }
        };
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
        // (same corrector, white balance, and resolved source actions).
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(frame.width(), frame.height());
        #[cfg(feature = "lensfun")]
        let lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let lensfun = None;
        let (after_lens, after_perspective) = generative_input_frames(
            &frame,
            &self.recipe,
            self.camera_white_balance,
            source_actions.artifacts(),
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
    pub(crate) fn resolve_generative_artifacts(
        &mut self,
        full_source: &ImageFrame,
        source_actions: &ResolvedSourceActions,
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
        let memo_key = self.generative_memo_key(source_actions);
        if self.generative_memo.as_deref() == Some(memo_key.as_str()) {
            return Ok(self.generative_artifacts.clone());
        }
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(full_source.width(), full_source.height());
        #[cfg(feature = "lensfun")]
        let lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let lensfun = None;
        let (after_lens, after_perspective) = generative_input_frames(
            full_source,
            &self.recipe,
            self.camera_white_balance,
            source_actions.artifacts(),
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
}
