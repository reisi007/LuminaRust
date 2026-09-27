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
    ///    end of every render and writes zeros over the active pool entry
    ///    whenever the frame carries no coverage, so a plane uploaded for a
    ///    since-deleted mask is overwritten by the very render that empties the
    ///    layer list. Without that write the relaxation below presented a
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
