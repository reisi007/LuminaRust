//! R5-BRUSH-24 brush controls, lifecycle, and per-dab prompt snapshots.
//!
//! This module owns the real key events, selected-mask materialization, prompt
//! merge/commit lifecycle, and the pure gating used by preview cursor painters.
//! Pointer sampling and GPU tile upload live in `preview_masks` /
//! `brush_plane`; every dab still snapshots the live controls into the prompt.

use super::*;
use log::info;

/// Lightroom-like `[` / `]` mapping for the armed mask brush. Kept separate
/// from the Spot mapping so each exclusive tool can be tested independently.
pub(crate) fn brush_size_factor_for_key(key: egui::Key) -> Option<f32> {
    match key {
        egui::Key::OpenBracket => Some(1.0 / 1.1),
        egui::Key::CloseBracket => Some(1.1),
        _ => None,
    }
}

fn valid_brush_mark(mark: &BrushMark) -> bool {
    mark.x.is_finite()
        && mark.y.is_finite()
        && mark.radius.is_finite()
        && (0.0..=1.0).contains(&mark.x)
        && (0.0..=1.0).contains(&mark.y)
        && (0.0..=1.0).contains(&mark.radius)
        && mark.radius > 0.0
        && mark.softness.is_finite()
        && (0.0..=1.0).contains(&mark.softness)
        && mark.flow.is_finite()
        && (0.0..=1.0).contains(&mark.flow)
}

/// Append pending marks to an existing Brush prompt, or replace a different
/// prompt kind. Shared by gesture commit and the CPU live overlay so preview
/// and persistence cannot drift.
pub(crate) fn merge_brush_prompt(
    existing: Option<MaskPrompt>,
    marks: Vec<BrushMark>,
    resolution: (u32, u32),
) -> Result<MaskPrompt, GuiError> {
    Ok(match existing {
        Some(MaskPrompt::Brush {
            marks: mut previous,
            resolution: stored_resolution,
            transformation,
        }) => {
            if stored_resolution != resolution {
                let (width, height) = resolution;
                return Err(GuiError::Io(
                    Str::MaskResolutionMismatchPattern
                        .t()
                        .replacen("{}", &format!("{stored_resolution:?}"), 1)
                        .replacen("{}", &format!("{width}x{height}"), 1),
                ));
            }
            previous.extend(marks);
            MaskPrompt::Brush {
                marks: previous,
                resolution,
                transformation,
            }
        }
        _ => MaskPrompt::Brush {
            marks,
            resolution,
            transformation: PromptTransform::default(),
        },
    })
}

impl LuminaApp {
    /// Current normalized brush radius `(0, 1]`.
    pub fn brush_size(&self) -> f32 {
        self.brush_radius
    }

    /// Current edge softness in `0..=1`.
    pub fn brush_softness(&self) -> f32 {
        self.brush_softness
    }

    /// Current per-dab flow in `0..=1`.
    pub fn brush_flow(&self) -> f32 {
        self.brush_flow
    }

    /// Stable selected-mask id for this copy's session state.
    pub fn selected_mask_id(&self) -> Option<&str> {
        self.selected_mask_id.as_deref()
    }

    /// The selected mask's own persisted layer. R5-BRUSH-24 keeps one layer
    /// per mask, so local adjustments and visibility never leak to a sibling.
    pub fn selected_mask_layer(&self) -> Option<&MaskLayer> {
        let selected = self.selected_mask_id.as_deref()?;
        let document = self.document.as_ref()?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)?;
        copy.mask_layers
            .iter()
            .find(|layer| layer.mask.copy_id == copy.id && layer.mask.mask_id == selected)
    }

    /// Set brush edge softness. Invalid values are loud and leave state intact.
    pub fn set_brush_softness(&mut self, softness: f32) -> Result<(), GuiError> {
        if !softness.is_finite() || !(0.0..=1.0).contains(&softness) {
            return Err(GuiError::Io(Str::BrushSoftnessInvalid.t().to_string()));
        }
        self.brush_softness = softness;
        info!("GUI interaction: brush softness -> {softness}");
        Ok(())
    }

    /// Set per-dab flow. Invalid values are loud and leave state intact.
    pub fn set_brush_flow(&mut self, flow: f32) -> Result<(), GuiError> {
        if !flow.is_finite() || !(0.0..=1.0).contains(&flow) {
            return Err(GuiError::Io(Str::BrushFlowInvalid.t().to_string()));
        }
        self.brush_flow = flow;
        info!("GUI interaction: brush flow -> {flow}");
        Ok(())
    }

    /// Handle the real key event only for an armed Brush. Text fields retain
    /// keyboard priority. Spot-Heal uses its own established handler.
    pub(crate) fn handle_brush_size_shortcuts(&mut self, ctx: &egui::Context) {
        if self.mask_tool != MaskTool::Brush || ctx.egui_wants_keyboard_input() {
            return;
        }
        for key in [egui::Key::OpenBracket, egui::Key::CloseBracket] {
            if ctx.input(|input| input.key_pressed(key)) {
                if let Some(factor) = brush_size_factor_for_key(key) {
                    self.nudge_brush_radius(factor);
                }
            }
        }
    }

    /// Scale the normalized size and clamp to the validated `(0, 1]` range.
    pub(crate) fn nudge_brush_radius(&mut self, factor: f32) {
        let next = (self.brush_radius * factor).clamp(0.005, 1.0);
        if (next - self.brush_radius).abs() <= f32::EPSILON {
            return;
        }
        let _ = self.set_brush_radius(next);
        info!("GUI interaction: brush size -> {:.1}%", next * 100.0);
    }

    /// Snapshot the live controls into one persisted dab.
    pub(crate) fn brush_mark_at(&self, x: f32, y: f32) -> BrushMark {
        BrushMark {
            x,
            y,
            radius: self.brush_radius,
            sign: if self.brush_eraser {
                BrushMarkSign::Negative
            } else {
                BrushMarkSign::Positive
            },
            softness: self.brush_softness,
            flow: self.brush_flow,
        }
    }

    /// The selected mask's persisted prompt, if the session selection still
    /// resolves on the active copy.
    pub(crate) fn selected_mask_prompt(&self) -> Option<MaskPrompt> {
        let id = self.selected_mask_id.as_ref()?;
        let document = self.document.as_ref()?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)?;
        let mask = copy.mask_library.iter().find(|mask| mask.id == *id)?;
        mask.prompt.clone()
    }

    /// Prompt shown for an in-progress CPU brush gesture. Pending dabs are
    /// cumulative with the selected mask's persisted Brush prompt.
    pub(crate) fn pending_brush_overlay_prompt(&self) -> Option<MaskPrompt> {
        if self.pending_brush_marks.is_empty() {
            return None;
        }
        let (width, height) = self.image_dims().unwrap_or((1, 1));
        merge_brush_prompt(
            self.selected_mask_prompt(),
            self.pending_brush_marks.clone(),
            (width, height),
        )
        .ok()
    }

    /// Active source dimensions used by prompts and source-space tools.
    pub(crate) fn image_dims(&self) -> Result<(u32, u32), GuiError> {
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        Ok((frame.width, frame.height))
    }

    /// Clear only the transient mask gesture. The arming setters use this
    /// before handing the preview to a competing picker/tool, so a stale
    /// in-progress brush can never intercept the next click.
    pub(crate) fn clear_mask_gesture(&mut self) {
        self.pending_brush_marks.clear();
        self.drag_start = None;
        self.drag_current = None;
        self.drawing = false;
        self.reset_brush_mask_plane();
    }

    /// The first layer that belongs to the copy that owns it. Cross-copy mask
    /// references are valid sidecar graph edges, but they are never a valid
    /// session selection for the active virtual copy.
    pub(crate) fn first_local_mask_id(copy: &VirtualCopy) -> Option<String> {
        copy.mask_layers
            .iter()
            .find(|layer| {
                layer.mask.copy_id == copy.id
                    && copy
                        .mask_library
                        .iter()
                        .any(|mask| mask.id == layer.mask.mask_id)
            })
            .map(|layer| layer.mask.mask_id.clone())
    }

    /// Ensure the selected id still names a mask in the active copy. A missing
    /// selection is materialized once by `create_mask`/`select_mask`.
    pub(crate) fn ensure_selected_mask(&mut self) -> Result<String, GuiError> {
        if let Some(id) = self.selected_mask_id.clone() {
            let exists = self.document.as_ref().is_some_and(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                    .is_some_and(|copy| copy.mask_library.iter().any(|mask| mask.id == id))
            });
            if exists {
                return Ok(id);
            }
            self.selected_mask_id = None;
        }
        let count = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map_or(0, |copy| copy.mask_library.len());
        self.create_mask(format!("Mask {}", count + 1))
    }

    /// Return the selected id only when it still names a definition on the
    /// active copy.  This keeps prompt commits from creating a definition in
    /// one save and then persisting its prompt in a second save.
    fn selected_valid_mask_id(&self) -> Option<String> {
        let selected = self.selected_mask_id.as_deref()?;
        self.document.as_ref().and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
                .is_some_and(|copy| copy.mask_library.iter().any(|mask| mask.id == selected))
                .then(|| selected.to_owned())
        })
    }

    /// Persist a finished prompt on the selected mask, invalidate both CPU and
    /// GPU preview state, and use one checked sidecar operation.  If there is
    /// no selected definition, the default definition, layer, selection, and
    /// prompt are built in that same transaction.
    pub(crate) fn apply_mask_prompt(&mut self, prompt: MaskPrompt) -> Result<(), GuiError> {
        if let Some(mask_id) = self.selected_valid_mask_id() {
            let select_required = self.selected_mask_id.as_deref() != Some(mask_id.as_str())
                || !self.document.as_ref().is_some_and(|document| {
                    document
                        .virtual_copies
                        .iter()
                        .find(|copy| copy.id == self.virtual_copy_id)
                        .is_some_and(|copy| {
                            copy.mask_layers.iter().any(|layer| {
                                layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id
                            })
                        })
                });
            self.transact_mask_mutation(true, |app| {
                if select_required {
                    app.select_mask_in_memory(&mask_id)?;
                }
                let copy_id = app.virtual_copy_id.clone();
                let mask = app
                    .document
                    .as_mut()
                    .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == copy_id)
                    .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?
                    .mask_library
                    .iter_mut()
                    .find(|mask| mask.id == mask_id)
                    .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
                mask.prompt = Some(prompt.clone());
                mask.status = MaskStatus::Valid;
                mask.error_text = None;
                Ok(())
            })?;
            self.reset_brush_mask_plane();
            info!("GUI interaction: apply_mask_prompt {mask_id}");
            self.status = Str::MaskPromptSaved.format_arg(&mask_id);
            return Ok(());
        }

        // No valid selection: construct the default mask with the prompt before
        // the transaction, so definition + layer + selection + prompt all land
        // in one checked save rather than a create-save followed by a prompt-save.
        let snapshot = self.mask_mutation_snapshot();
        self.selected_mask_id = None;
        let count = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map_or(0, |copy| copy.mask_library.len());
        let name = format!("Mask {}", count + 1);
        let id = format!("mask-{}", blake3::hash(name.as_bytes()).to_hex());
        let mut definition = match self.new_source_mask_template(&id, &name, MaskStatus::Valid) {
            Ok(definition) => definition,
            Err(error) => {
                self.restore_mask_mutation(snapshot);
                return Err(error);
            }
        };
        definition.prompt = Some(prompt);
        let id = match self.push_mask_definition(definition) {
            Ok(id) => id,
            Err(error) => {
                self.restore_mask_mutation(snapshot);
                return Err(error);
            }
        };
        self.reset_brush_mask_plane();
        info!("GUI interaction: apply_mask_prompt {id}");
        self.status = Str::MaskPromptSaved.format_arg(&id);
        Ok(())
    }

    /// Commit one completed stroke. A second stroke appends to the selected
    /// mask's existing Brush prompt; a different prompt kind is replaced only
    /// by the explicit new Brush gesture.
    pub fn commit_brush_stroke(&mut self, marks: Vec<BrushMark>) -> Result<(), GuiError> {
        if marks.is_empty() {
            return Err(GuiError::Io(Str::BrushStrokeEmpty.t().to_string()));
        }
        if !marks.iter().all(valid_brush_mark) {
            return Err(GuiError::Io(Str::BrushMarkInvalid.t().to_string()));
        }
        let (width, height) = self.image_dims()?;
        let existing = self.selected_valid_mask_id().and_then(|mask_id| {
            self.document
                .as_ref()
                .and_then(|document| {
                    document
                        .virtual_copies
                        .iter()
                        .find(|copy| copy.id == self.virtual_copy_id)
                })
                .and_then(|copy| copy.mask_library.iter().find(|mask| mask.id == mask_id))
                .and_then(|mask| mask.prompt.clone())
        });
        let prompt = merge_brush_prompt(existing, marks, (width, height))?;
        self.apply_mask_prompt(prompt)
    }

    /// Finish the in-progress mask-tool drag and dispatch its tool-specific
    /// commit. Errors are surfaced through the normal GUI error banner.
    pub(crate) fn finish_drawing(&mut self) {
        let tool = self.mask_tool;
        let start = self.drag_start;
        let end = self.drag_current;
        let marks = std::mem::take(&mut self.pending_brush_marks);
        self.drawing = false;
        self.drag_start = None;
        self.drag_current = None;
        let result = match tool {
            MaskTool::None => return,
            MaskTool::Brush => self.commit_brush_stroke(marks),
            MaskTool::LinearGradient => match (start, end) {
                (Some(a), Some(b)) => self.commit_gradient(a, b),
                _ => Ok(()),
            },
            MaskTool::Radial => match (start, end) {
                (Some(a), Some(b)) => self.commit_radial(a, b),
                _ => Ok(()),
            },
        };
        if let Err(error) = result {
            self.show_error(error);
        }
    }

    /// Resolve only the selected mask's own layer. Missing selected state is a
    /// hard error; a sibling layer is never a fallback.
    pub(crate) fn active_layer_mut(&mut self) -> Result<&mut MaskLayer, GuiError> {
        let selected = self
            .selected_mask_id
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let copy_id = self.virtual_copy_id.clone();
        self.active_copy_mut()?
            .mask_layers
            .iter_mut()
            .find(|layer| layer.mask.copy_id == copy_id && layer.mask.mask_id == selected)
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))
    }

    /// Common cursor gate: text focus and every competing preview picker/tool
    /// suppress source-tool circles without changing the arming state.
    pub(crate) fn interactive_cursor_allowed(&self, ui: &egui::Ui) -> bool {
        !ui.ctx().egui_wants_keyboard_input()
            && !self.wb_pick_mode
            && !self.red_eye_pick_mode
            && !self.crop_mode
    }

    pub(crate) fn brush_cursor_allowed(&self, ui: &egui::Ui) -> bool {
        self.mask_tool == MaskTool::Brush
            && self.spot_tool == SpotTool::None
            && self.interactive_cursor_allowed(ui)
    }

    pub(crate) fn spot_cursor_allowed(&self, ui: &egui::Ui) -> bool {
        self.spot_tool != SpotTool::None
            && self.mask_tool == MaskTool::None
            && self.interactive_cursor_allowed(ui)
    }
}
