//! G-11 overlay and edit-pin visibility (GUI slice, extracted from `lib.rs`).
//!
//! The runtime half of the G-11 visibility modes whose *types* live in
//! `overlay_modes.rs` (`OverlayMode`, `PinVisibility`): which overlay mode is
//! armed, whether the mask matte may paint right now, whether the numbered
//! edit pins paint, and which pins those are.
//! One responsibility: what the preview's overlay chrome shows at this
//! moment. The mask-view gate (R5-MASKVIS-25) stays in `mask_visibility.rs`,
//! the painters in `preview_masks.rs`, the pin record type in `edit_pins.rs` —
//! none of them is re-implemented here.
//!
//! Extracted verbatim for the `GUI-LIB-SPLIT-65` file-size ratchet; the only
//! change to the moved code is the visibility of the private
//! `effective_overlay_prompt` helper (`pub(crate)`, called from the preview
//! painter and the headless tests).

use super::*;

impl LuminaApp {
    /// Current tool-overlay mode (G-11, read-only accessor for headless tests).
    pub fn overlay_mode(&self) -> OverlayMode {
        self.overlay_mode
    }

    /// Set the tool-overlay mode (G-11). Display-only session state: never
    /// touches the recipe or the sidecar.
    pub fn set_overlay_mode(&mut self, mode: OverlayMode) {
        instrument_gui_action!(self, GuiAction::SetOverlayMode);
        if self.overlay_mode == mode {
            return;
        }
        self.overlay_mode = mode;
        log::info!("GUI interaction: set_overlay_mode -> {mode:?}");
        self.status = Str::OverlayModeSetPattern.format_arg(overlay_mode_name(mode));
    }

    /// Whether the mask-matte overlay paints right now (G-11): `Always` shows
    /// whenever a prompt exists, `Never` hides, `Auto` shows only while a
    /// masking/retouch tool is armed or a drag is in progress. Consumed by
    /// [`Self::effective_overlay_prompt`] (the single draw-path gate), so the
    /// default `Always` preserves the historical behaviour exactly.
    pub fn overlay_visible(&self) -> bool {
        match self.overlay_mode {
            OverlayMode::Always => true,
            OverlayMode::Never => false,
            OverlayMode::Auto => {
                self.mask_tool != MaskTool::None || self.spot_tool != SpotTool::None || self.drawing
            }
        }
    }

    /// The overlay prompt gated by the single mask visibility predicate: `None`
    /// when the current view, display mode, Show switch, global G-11 mode, or
    /// selected-mask eye hides the overlay; otherwise the live drag or saved
    /// prompt is returned. Kept headless-testable without pixels.
    pub(crate) fn effective_overlay_prompt(&self) -> Option<MaskPrompt> {
        if !self.mask_overlay_allowed() {
            return None;
        }
        self.current_overlay_prompt()
    }

    /// Current edit-pin visibility mode (G-11, read-only accessor).
    pub fn pin_visibility(&self) -> PinVisibility {
        self.pin_visibility
    }

    /// Set the edit-pin visibility (G-11). Display-only session state: never
    /// touches the recipe or the sidecar.
    pub fn set_pin_visibility(&mut self, visibility: PinVisibility) {
        instrument_gui_action!(self, GuiAction::SetPinVisibility);
        if self.pin_visibility == visibility {
            return;
        }
        self.pin_visibility = visibility;
        log::info!("GUI interaction: set_pin_visibility -> {visibility:?}");
        self.status = Str::PinVisibilitySetPattern.format_arg(pin_visibility_name(visibility));
    }

    /// Whether edit pins paint right now (G-11): `Always` shows, `Never`
    /// hides, `Auto` shows only while a masking/retouch tool is armed.
    pub fn pins_visible(&self) -> bool {
        match self.pin_visibility {
            PinVisibility::Always => true,
            PinVisibility::Never => false,
            PinVisibility::Auto => {
                self.mask_tool != MaskTool::None || self.spot_tool != SpotTool::None
            }
        }
    }

    /// The edit pins to paint (G-11/R5-MASKVIS-25): one pin per mask of the
    /// active copy with a derivable anchor while the Masking view is open,
    /// plus one pin per stored spot heal with finite `0..=1` centre coordinates.
    /// Empty unless [`Self::pins_visible`]. Pins are Painter-content
    /// (invisible to AccessKit per HARNESS-2), so this getter is the testable
    /// model state; the painter loop paints exactly this list in order (labels
    /// `1..=n`).
    pub fn visible_edit_pins(&self) -> Vec<EditPin> {
        if !self.pins_visible() {
            return Vec::new();
        }
        let mut pins = Vec::new();
        let document = match self.document.as_ref() {
            Some(document) => document,
            None => return pins,
        };
        let copy = match document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
        {
            Some(copy) => copy,
            None => return pins,
        };
        for mask in &copy.mask_library {
            // R5-MASKVIS-25: mask pins belong to the open Masking view. Spot
            // pins remain global retouch anchors and are appended below even
            // when the mask view is closed.
            if !self.mask_view_open() || !self.mask_visible(&mask.id) {
                continue;
            }
            let Some(prompt) = mask.prompt.as_ref() else {
                continue;
            };
            let Some((x, y)) = pin_anchor_for_prompt(prompt) else {
                continue;
            };
            pins.push(EditPin {
                id: format!("mask:{}", mask.id),
                label: (pins.len() + 1).to_string(),
                pos: (x, y),
                selected: self.selected_mask_id.as_deref() == Some(mask.id.as_str()),
                kind: EditPinKind::Mask,
            });
        }
        let spots = self.spot_entries();
        for spot in &spots {
            let centre = spot
                .get("center_x")
                .and_then(serde_json::Value::as_f64)
                .zip(spot.get("center_y").and_then(serde_json::Value::as_f64));
            let Some((x, y)) = centre else {
                continue;
            };
            if !x.is_finite()
                || !y.is_finite()
                || !(0.0..=1.0).contains(&x)
                || !(0.0..=1.0).contains(&y)
            {
                continue;
            }
            let Some(id) = spot.get("id").and_then(|value| value.as_str()) else {
                continue;
            };
            pins.push(EditPin {
                id: format!("spot:{id}"),
                label: (pins.len() + 1).to_string(),
                pos: (x as f32, y as f32),
                // R5-DUST-23-FOLLOWUP: the selected spot pin paints
                // accent-filled, like the selected mask pin.
                selected: self.selected_spot_id.as_deref() == Some(id),
                kind: EditPinKind::Spot,
            });
        }
        pins
    }
}
