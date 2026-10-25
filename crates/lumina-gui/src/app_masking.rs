//! Mask state (select/create/rename/combine/regenerate), the mask preview key,
//! the brush/spot pen setters and the gradient/radial drag prompts.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
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

    /// Select a mask from the active copy's library and make it the active layer.
    /// The matte is only referenced; no payload is copied or modified.
    pub fn select_mask(&mut self, mask_id: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SelectMask);
        self.ensure_document_loaded()?;
        let needs_persistence = {
            let copy_id = self.virtual_copy_id.clone();
            let copy = self
                .document
                .as_ref()
                .and_then(|document| {
                    document
                        .virtual_copies
                        .iter()
                        .find(|copy| copy.id == copy_id)
                })
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            !copy
                .mask_layers
                .iter()
                .any(|layer| layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id)
        };
        let mut materialized = false;
        self.transact_mask_mutation(needs_persistence, |app| {
            materialized = app.select_mask_in_memory(mask_id)?;
            Ok(())
        })?;
        info!("GUI interaction: select_mask {mask_id} materialized={materialized}");
        self.status = Str::MaskSelected.format_arg(mask_id);
        Ok(())
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
        let definition = {
            let document = self.document.as_mut().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == self.virtual_copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            if copy.mask_library.iter().any(|mask| mask.id == id) {
                return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
            }
            MaskDefinition {
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
                    width: frame.width(),
                    height: frame.height(),
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
                    width: frame.width(),
                    height: frame.height(),
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
            }
        };
        self.push_mask_definition(definition)?;
        info!("GUI interaction: create_mask -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    pub fn rename_mask(&mut self, mask_id: &str, name: impl Into<String>) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::RenameMask);
        self.ensure_document_loaded()?;
        let name = name.into().trim().to_owned();
        if name.is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        self.transact_mask_mutation(true, |app| {
            let copy_id = app.virtual_copy_id.clone();
            let document = app.document.as_mut().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
                return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
            }
            if copy
                .mask_library
                .iter()
                .any(|mask| mask.id != mask_id && mask.name == name)
            {
                return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
            }
            copy.mask_library
                .iter_mut()
                .find(|mask| mask.id == mask_id)
                .expect("mask existence checked")
                .name = name.clone();
            app.mask_rename_input = name.clone();
            app.mask_rename_inputs.insert(mask_id.into(), name.clone());
            Ok(())
        })?;
        info!("GUI interaction: rename_mask {mask_id} -> `{name}`");
        self.status = Str::MaskRenamed.t().into();
        Ok(())
    }

    // ---- G-03 Maskierungs-Parität: AI-select, Range-Stufen, Kombinatorik ----

    /// Build a fresh source [`MaskDefinition`] for the active copy from the
    /// loaded frame (dimensions + source identity). Shared by all G-03 mask
    /// constructors so geometry/model placeholders stay identical.
    pub(crate) fn new_source_mask_template(
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
                width: frame.width(),
                height: frame.height(),
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
                width: frame.width(),
                height: frame.height(),
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

    /// Push, select, materialize, validate, and persist a new definition as one
    /// transaction.  A rejected graph or failed CAS/IO write restores the full
    /// previous document and selection state; callers must not perform a second
    /// selection save after this returns.
    pub(crate) fn push_mask_definition(
        &mut self,
        definition: MaskDefinition,
    ) -> Result<String, GuiError> {
        self.ensure_document_loaded()?;
        let id = definition.id.clone();
        self.transact_mask_mutation(true, |app| {
            let copy_id = app.virtual_copy_id.clone();
            let document = app.document.as_mut().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            if copy.mask_library.iter().any(|mask| mask.id == id) {
                return Err(GuiError::Io(Str::MaskNameExists.t().to_string()));
            }
            copy.mask_library.push(definition);
            app.select_mask_in_memory(&id)?;
            Ok(id.clone())
        })
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
        info!("GUI interaction: combine_masks {operation:?} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    pub fn set_mask_inverted(&mut self, inverted: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetMaskInverted);
        let layer = self.active_layer_mut()?;
        layer.inverted = inverted;
        // REVIEW-GUI-MASKRENDER-1: layer edits change the evaluated matte, so
        // the preview must actually re-render. GUI-SLIDER-SAVE-1: the layer
        // value is persistent data too, so arm the same debounced CAS save
        // path as the other mask controls instead of merely invalidating.
        self.mark_recipe_dirty("mask.inverted", f64::from(u8::from(inverted)));
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
        // Module `auto-tone`: stale = enabled and the shared freshness predicate
        // (`auto_tone_is_fresh`) refuses the recipe. That predicate is exactly
        // the one `lumina regenerate --module auto-tone` uses, so a GUI recipe
        // the CLI would call fresh is not touched here, and a recipe with a
        // non-matching fingerprint (e.g. a changed target luminance) IS
        // repaired — the same trigger the CLI has. A historic two-slider
        // artifact and a half-mirrored state are stale by the same rule.
        let auto_tone_stale = self.recipe.auto_features.enable_auto_tone
            && self.original.as_ref().is_none_or(|frame| {
                let target = self.recipe.auto_features.target_luminance;
                let fingerprint =
                    lumina_stages::auto_tone::auto_tone_input_fingerprint(frame, target);
                !lumina_stages::auto_tone::auto_tone_is_fresh(&self.recipe, &fingerprint)
            });
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
        // R5-DUST-23 / R5-BRUSH-24: arming a source tool disarms every
        // competing preview picker/tool, so a stale click handler cannot
        // intercept the first gesture of the newly armed tool.
        if tool != MaskTool::None {
            self.spot_tool = SpotTool::None;
            self.wb_pick_mode = false;
            self.local_wb_pick_mode = false;
            self.red_eye_pick_mode = false;
        }
        self.clear_mask_gesture();
    }

    /// Set the normalized brush radius. Rejected (no state change) if not finite
    /// or outside the open-closed `(0, 1]` range.
    pub fn set_brush_radius(&mut self, radius: f32) -> Result<(), GuiError> {
        if !radius.is_finite() || !(0.0..=1.0).contains(&radius) || radius <= 0.0 {
            return Err(GuiError::Io(Str::BrushRadiusInvalid.t().to_string()));
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
}
