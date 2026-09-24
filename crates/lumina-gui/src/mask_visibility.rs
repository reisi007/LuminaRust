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

    /// GPU present may composite the evaluated mask only when that mask is
    /// exactly the selected mask. The CPU path can rasterize a single prompt
    /// for several masks, but the historical VRAM plane combines every layer;
    /// falling back to the CPU texture is therefore required for correctness
    /// whenever the two sets differ. Live brush stamps have no evaluated-layer
    /// flag and remain on the normal GPU path.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_mask_overlay_is_selected(&self) -> bool {
        // Keep every editorial visibility gate off the VRAM composite, even if
        // a stale mask texture is still resident. This makes the GPU route
        // obey the same multiplicative contract as the CPU painter.
        if !self.mask_overlay_allowed() {
            return false;
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
