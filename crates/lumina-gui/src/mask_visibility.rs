//! R5-MASKVIS-25: mask-view visibility, overlay display modes, and focus state.
//!
//! The mask view is a real session gate, not a recipe flag. Keeping the small
//! state machine here makes it possible for the CPU painter, the GPU-present
//! routing gate, the panel painter, and headless tests to consume the same
//! predicates without teaching any of them about the other paths.

use super::*;

/// What the mask view paints over the preview.
///
/// `SelectedFull` is the historical Lightroom-style selected-mask matte and is
/// the default. `PinsOnly` deliberately keeps every visible mask pin while
/// suppressing the matte, so a user can navigate a multi-mask set without
/// losing the selection anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskOverlayMode {
    PinsOnly,
    #[default]
    SelectedFull,
}

impl LuminaApp {
    /// Whether the Develop `Masking` section is currently open.
    pub fn mask_view_open(&self) -> bool {
        self.is_section_open(SECTION_MASKING)
    }

    /// Current mask overlay display mode (R5-MASKVIS-25).
    pub fn mask_overlay_mode(&self) -> MaskOverlayMode {
        self.mask_overlay_mode
    }

    /// Set the mask overlay display mode. This is session display state: it
    /// never changes the recipe, sidecar, zoom, or armed tool.
    pub fn set_mask_overlay_mode(&mut self, mode: MaskOverlayMode) {
        instrument_gui_action!(self, GuiAction::SetMaskOverlayMode);
        if self.mask_overlay_mode == mode {
            return;
        }
        self.mask_overlay_mode = mode;
        info!("GUI interaction: set_mask_overlay_mode -> {mode:?}");
        self.status = Str::OverlayModeSetPattern.format_arg(match mode {
            MaskOverlayMode::PinsOnly => Str::MaskPin.t(),
            MaskOverlayMode::SelectedFull => Str::ShowOverlay.t(),
        });
    }

    /// Flip between pins-only and selected-mask-full display.
    pub fn toggle_mask_overlay_mode(&mut self) {
        let next = match self.mask_overlay_mode {
            MaskOverlayMode::PinsOnly => MaskOverlayMode::SelectedFull,
            MaskOverlayMode::SelectedFull => MaskOverlayMode::PinsOnly,
        };
        self.set_mask_overlay_mode(next);
    }

    /// Whether the selected-mask matte may be painted right now.
    ///
    /// This is the single visibility predicate for the mask matte. The mask
    /// view, the two-state R5-MASKVIS mode, the legacy G-11 overlay mode, the
    /// Show switch, and the selected mask's eye are all multiplicative gates.
    /// A live gesture follows the same rule; closing the view hides its live
    /// preview too, while leaving the gesture armed and its eventual stroke
    /// persistence untouched.
    pub fn mask_overlay_allowed(&self) -> bool {
        if !self.mask_view_open()
            || self.mask_overlay_mode != MaskOverlayMode::SelectedFull
            || !self.show_mask_overlay
            || !self.overlay_visible()
        {
            return false;
        }
        if self.drawing && self.mask_tool != MaskTool::None {
            // Gradient/radial gestures can begin before a default mask exists.
            // If a selection does exist, its eye is still a multiplicative
            // gate; without a selection there is no eye to evaluate yet.
            return self
                .selected_mask_id
                .as_deref()
                .is_none_or(|id| self.mask_visible(id));
        }
        let Some(id) = self.selected_mask_id.as_deref() else {
            return false;
        };
        self.mask_visible(id)
    }

    /// Whether the CPU painter must draw a mask matte that the VRAM present
    /// composite cannot contain. `false` means the readback-free VRAM path is
    /// pixel-equal to the CPU upload for this frame.
    ///
    /// GPU-PARITY-MASKGATE-1: this gate answers a **pixel** question — *does the
    /// frame to be presented carry an evaluated mask layer that the CPU painter
    /// would not draw?* — and the answer is read from the **layers**
    /// ([`Self::render_mask_layers`]), never from the selection. Two states
    /// used to be conflated here:
    ///
    /// 1. **The frame carries no evaluated mask layer** — there is nothing
    ///    composited that the CPU painter would miss, so the VRAM path is
    ///    pixel-equal and *must* be reachable. This is the shipped default
    ///    Develop state (Masking section closed, no mask); before the split it
    ///    was unreachable, which silently demoted every default-state frame to
    ///    the CPU upload. Pixel equality rests on a third condition — *no stale
    ///    mask plane may be resident in the VRAM pool for the current
    ///    dimensions* — and that one is held by construction, not by a flag:
    ///    `present_mask_plane::LuminaApp::sync_mask_plane_to_vram` runs at the
    ///    end of every render and writes zeros over the active pool entry when
    ///    the frame carries no coverage **and** that entry still holds coverage
    ///    from an earlier frame (`GPU-CLEAR-TRANSITION`: the transition, not
    ///    every maskless frame — an entry that already reads zero is left
    ///    alone), so a plane uploaded for a since-deleted mask is overwritten
    ///    by the very render that empties the layer list. Without that write the
    ///    relaxation below presented a
    ///    deleted mask's coverage (measured on a real adapter: `maxAbsDiff=67`
    ///    over 127 707 presented photo bytes), and `vram_mask_is_evaluated`
    ///    could not have detected it — every `mark_dirty` resets that flag. The
    ///    one coverage the clear preserves on purpose is a live brush plane the
    ///    present path is allowed to composite (the `drawing` branch below).
    /// 2. **The frame carries an evaluated layer the CPU painter would not
    ///    draw** — a layer is present, but an editorial gate is closed
    ///    (section, display mode, Show switch, mask eye) or the layer is not the
    ///    selected one. The CPU painter must show the required matte, so CPU
    ///    present stays mandatory (`R5-MASKVIS-25`).
    ///
    /// The selection is deliberately **not** the signal: after deleting the
    /// selected mask a prompted mask can remain in the document with its layer
    /// evaluated into the frame, while `selected_mask_id` is `None` and the CPU
    /// painter has no prompt to draw. Routing that frame from VRAM presented a
    /// mask tint no CPU path can reproduce (measured on a real adapter:
    /// `maxAbsDiff=67` over 127 707 presented photo bytes), and the user
    /// cannot switch it off because `mask_overlay_allowed` demands the
    /// selection that no longer exists.
    ///
    /// With a selection the further equality conditions still apply: the CPU
    /// path can rasterize a single prompt for several masks, but the historical
    /// VRAM plane combines every layer, so falling back to the CPU texture is
    /// required whenever the two sets differ. Live brush stamps have no
    /// evaluated-layer flag and remain on the normal GPU path.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_mask_overlay_is_selected(&self) -> bool {
        // Keep every editorial visibility gate off the VRAM composite. This
        // makes the GPU route obey the same multiplicative contract as the CPU
        // painter. The one relaxation is the layerless frame: with no evaluated
        // layer in the frame there is no matte to gate, so the VRAM route is
        // allowed — and it is pixel-equal because
        // `present_mask_plane::LuminaApp::sync_mask_plane_to_vram` zeroed the
        // active entry's plane on that same render. A plane left over from a
        // deleted mask is *not* something this gate can see; that invariant is
        // maintained where the plane is written, not here.
        if !self.mask_overlay_allowed() {
            return self.render_mask_layers.is_empty();
        }
        if self.drawing && self.mask_tool != MaskTool::None {
            // Gradient/radial live prompts have no VRAM representation. A
            // brush stamp is uploaded incrementally and can stay on the GPU.
            return self.mask_tool == MaskTool::Brush;
        }
        if !self.vram_mask_is_evaluated {
            return false;
        }
        // Avoid cloning a potentially large brush prompt on every GPU frame;
        // the evaluated-plane check below is sufficient for normal renders,
        // while this cheap existence check rejects a stale resident plane
        // after a prompt was removed.
        let has_selected_prompt = self.selected_mask_id.as_deref().is_some_and(|id| {
            self.document.as_ref().is_some_and(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                    .is_some_and(|copy| {
                        copy.mask_library
                            .iter()
                            .any(|mask| mask.id == id && mask.prompt.is_some())
                    })
            })
        });
        if !has_selected_prompt {
            return false;
        }
        let Some(selected_layer) = self.selected_mask_layer() else {
            return false;
        };
        self.render_mask_layers.len() == 1
            && self.render_mask_layers[0].layer_id == selected_layer.id
    }

    /// Visibility eye of the mask list (G-03). Sets `visible` on every layer
    /// of the active copy that references `mask_id`; when no layer references
    /// it yet, a layer is created (never an invented matte — only the
    /// reference). Persisted per virtual copy, loud on unknown masks.
    pub fn set_mask_visible(&mut self, mask_id: &str, visible: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetMaskVisible);
        self.ensure_document_loaded()?;
        let copy_id = self.virtual_copy_id.clone();
        let copy_snapshot = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == copy_id)
            })
            .cloned()
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        {
            let document = self.document.as_mut().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
                return Err(GuiError::Io(Str::MaskNotFound.t().to_string()));
            }
            // Snapshot for rollback: the eye must never leave a half-written
            // layer list behind when validation rejects the result.
            let layer_snapshot: Vec<(String, bool)> = copy
                .mask_layers
                .iter()
                .map(|layer| (layer.id.clone(), layer.visible))
                .collect();
            let layer_count = copy.mask_layers.len();
            let mut touched = false;
            for layer in copy
                .mask_layers
                .iter_mut()
                .filter(|layer| layer.mask.copy_id == copy_id && layer.mask.mask_id == mask_id)
            {
                layer.visible = visible;
                touched = true;
            }
            if !touched {
                let mut layer_id = format!("layer-{mask_id}");
                let mut suffix = 2;
                while copy.mask_layers.iter().any(|layer| layer.id == layer_id) {
                    layer_id = format!("layer-{mask_id}-{suffix}");
                    suffix += 1;
                }
                copy.mask_layers.push(MaskLayer {
                    id: layer_id,
                    mask: MaskReference {
                        copy_id: copy_id.clone(),
                        mask_id: mask_id.into(),
                        extras: BTreeMap::new(),
                    },
                    inverted: false,
                    feather: 0.0,
                    blur: 0.0,
                    density: 1.0,
                    visible,
                    local_adjustments: None,
                    extras: BTreeMap::new(),
                });
            }
            if let Err(error) = document.validate() {
                let copy = document
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == copy_id)
                    .expect("copy was found above");
                copy.mask_layers.truncate(layer_count);
                for layer in copy.mask_layers.iter_mut() {
                    if let Some((_, was)) = layer_snapshot.iter().find(|(id, _)| id == &layer.id) {
                        layer.visible = *was;
                    }
                }
                return Err(GuiError::Io(error.to_string()));
            }
        }
        if let Err(error) = self.save_sidecar_checked() {
            if let Some(copy) = self.document.as_mut().and_then(|document| {
                document
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == copy_id)
            }) {
                *copy = copy_snapshot;
            }
            return Err(error);
        }
        self.mark_dirty();
        info!("GUI interaction: set_mask_visible {mask_id} -> {visible}");
        Ok(())
    }

    /// Eye state of the mask list (G-03): true when no layer references the
    /// mask yet (vacuous) or at least one referencing layer is visible.
    pub fn mask_visible(&self, mask_id: &str) -> bool {
        let Some(document) = self.document.as_ref() else {
            return true;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            return true;
        };
        let mut any = false;
        let mut visible = false;
        for layer in &copy.mask_layers {
            if layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id {
                any = true;
                visible = visible || layer.visible;
            }
        }
        !any || visible
    }

    /// Selected mask status for the panel status line (G-03): status plus the
    /// persisted error text, if any. `None` without a selection.
    pub fn selected_mask_status(&self) -> Option<(MaskStatus, Option<String>)> {
        let id = self.selected_mask_id.as_deref()?;
        let document = self.document.as_ref()?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)?;
        let mask = copy.mask_library.iter().find(|mask| mask.id == id)?;
        Some((mask.status.clone(), mask.error_text.clone()))
    }

    /// Master Show switch of the mask overlay (G-03). Display-only session
    /// state (never recipe/sidecar), ANDed with the G-11 overlay mode.
    pub fn show_mask_overlay(&self) -> bool {
        self.show_mask_overlay
    }

    pub fn set_show_mask_overlay(&mut self, shown: bool) {
        instrument_gui_action!(self, GuiAction::SetShowMaskOverlay);
        if self.show_mask_overlay == shown {
            return;
        }
        self.show_mask_overlay = shown;
        info!("GUI interaction: set_show_mask_overlay -> {shown}");
    }

    /// Matte tint of the mask overlay (G-03). Display-only session state.
    pub fn overlay_color(&self) -> [u8; 3] {
        self.overlay_color
    }

    pub fn set_overlay_color(&mut self, color: [u8; 3]) {
        instrument_gui_action!(self, GuiAction::SetOverlayColor);
        if self.overlay_color == color {
            return;
        }
        self.overlay_color = color;
        info!(
            "GUI interaction: set_overlay_color -> #{:02X}{:02X}{:02X}",
            color[0], color[1], color[2]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_is_session_only_and_defaults_to_selected_full() {
        let mut app = LuminaApp::new(egui::Context::default());
        assert_eq!(app.mask_overlay_mode(), MaskOverlayMode::SelectedFull);
        let recipe = app.recipe().clone();
        app.set_mask_overlay_mode(MaskOverlayMode::PinsOnly);
        assert_eq!(app.mask_overlay_mode(), MaskOverlayMode::PinsOnly);
        assert_eq!(*app.recipe(), recipe);
        app.toggle_mask_overlay_mode();
        assert_eq!(app.mask_overlay_mode(), MaskOverlayMode::SelectedFull);
    }
}
