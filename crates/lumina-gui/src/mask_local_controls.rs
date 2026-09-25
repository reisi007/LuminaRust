//! MASK-LOCAL-P0 GUI transactions for typed local controls and WB reset.

use super::{GuiError, LuminaApp, MaskLayer, Str};

impl LuminaApp {
    /// Whether the active copy has a visible, non-neutral P0 local layer.
    /// Stand-in routes (draft/navigator/neighbor/thumbnail/VRAM present) use
    /// this before deciding whether they may omit the mask-aware CPU render.
    pub(crate) fn has_visible_local_adjustments(&self) -> bool {
        let Some(document) = self.document.as_ref() else {
            return false;
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            return false;
        };
        copy.mask_layers.iter().any(|layer| {
            if !layer.visible {
                return false;
            }
            match layer.effective_local_adjustments() {
                Ok(Some(adjustments)) => !adjustments.is_neutral(),
                // Malformed visible local state must not silently take a
                // stand-in/global-only route; the full render will report the
                // precise validation error.
                Ok(None) => false,
                Err(_) => true,
            }
        })
    }

    /// Human-readable reason shown when a thumbnail/neighbor/draft route must
    /// refuse rather than silently render a global-only stand-in.
    pub(crate) fn local_adjustment_route_reason(&self) -> Option<String> {
        self.has_visible_local_adjustments().then(|| {
            "local mask adjustments require the full mask-aware CPU render; this stand-in route refused".to_string()
        })
    }

    /// Arm one coalesced history transaction with the complete layer state
    /// from before its first edit. Later slider/reset events keep this first
    /// snapshot instead of replacing it with the immediately previous value.
    pub(crate) fn active_mask_layers_snapshot(&self) -> Result<Vec<MaskLayer>, GuiError> {
        self.document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| copy.mask_layers.clone())
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))
    }

    pub(crate) fn arm_mask_state_history_from(&mut self, action: &str, before: Vec<MaskLayer>) {
        self.pending_mask_state_before.get_or_insert(before);
        self.pending_history_step
            .get_or_insert_with(|| action.to_string());
    }

    pub(crate) fn arm_mask_state_history(&mut self, action: &str) -> Result<(), GuiError> {
        let before = self.active_mask_layers_snapshot()?;
        self.arm_mask_state_history_from(action, before);
        Ok(())
    }

    /// Store one P0 local adjustment in the typed, versioned layer object.
    /// The core compositor owns the exact global-kernel order; this method only
    /// validates and persists the declarative value.
    pub fn set_mask_local_adjustment(&mut self, key: &str, value: f64) -> Result<(), GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") {
            return Err(GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()));
        }
        // Validate before touching the legacy layer so a rejected value cannot
        // normalize/mutate the selected layer as a side effect.
        lumina_sidecar::LocalAdjustments::default()
            .set_value(key, value)
            .map_err(|_| GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()))?;
        let before = self.active_mask_layers_snapshot()?;
        let layer = self.active_layer_mut()?;
        // Normalize a legacy layer before editing it; a conflict is loud and
        // leaves both the layer and the pending snapshot untouched.
        layer
            .normalize_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut adjustments = layer.local_adjustments.unwrap_or_default();
        adjustments
            .set_value(key, value)
            .map_err(|_| GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()))?;
        layer.local_adjustments = Some(adjustments);
        self.arm_mask_state_history_from(&format!("mask.local.{key}"), before);
        // GUI-SLIDER-SAVE-1: a local adjustment is recipe data — it must arm
        // the re-render AND the debounced save (previously neither happened).
        self.mark_recipe_dirty(&format!("mask.local.{key}"), value);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    /// Read one selected layer's typed local value. Legacy layers are
    /// resolved through the same loud migration view used by the renderer.
    pub fn selected_mask_local_adjustment(&self, key: &str) -> Result<Option<f64>, GuiError> {
        let Some(layer) = self.selected_mask_layer() else {
            return Ok(None);
        };
        let adjustments = layer
            .effective_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        Ok(adjustments.and_then(|value| value.value(key)))
    }

    /// Reset one local P0 control to its neutral value while retaining the
    /// rest of the layer's local recipe. This is a real persisted/history
    /// edit, not a display-only slider clear.
    pub fn reset_mask_local_adjustment(&mut self, key: &str) -> Result<(), GuiError> {
        self.set_mask_local_adjustment(key, 0.0)
    }

    /// Explicitly return global white balance to the decoder's As-Shot basis.
    /// This removes the absolute WB keys rather than fabricating a 6500 K
    /// correction; local mask adjustments never own a WB control in P0.
    pub fn reset_white_balance_to_as_shot(&mut self) -> Result<(), GuiError> {
        self.recipe.adjustments.remove("wb_temperature");
        self.recipe.adjustments.remove("wb_tint");
        self.pending_history_step = Some("wb.reset_as_shot".into());
        self.mark_recipe_dirty("wb.reset_as_shot", 0.0);
        self.status = Str::ResetAsShot.t().into();
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }
}
