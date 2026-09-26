//! MASK-LOCAL-P0/P1.1/P1.2a/P1.2b GUI transactions for typed local controls,
//! relative WB, the local tone curve, and the explicit global Reset to As
//! Shot action.

use super::{GuiError, LuminaApp, MaskLayer, MaskTool, SpotTool, Str};

impl LuminaApp {
    /// Whether the active copy has a visible, non-neutral local layer.
    /// Stand-in routes (draft/navigator/neighbor/thumbnail/VRAM present) use
    /// this before deciding whether they may omit the mask-aware CPU render.
    /// `LocalAdjustments::is_neutral` includes the tone curve *and* the colour
    /// block, so a curve-only or colour-only layer keeps every such route on
    /// the CPU reference.
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
            "local mask adjustments (relative WB, presence, tone curve, color and detail) require the full mask-aware CPU render; this stand-in route refused".to_string()
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

    /// Store one typed local control in the versioned layer object. The core
    /// compositor owns the exact global-kernel order; this method only
    /// validates and persists the declarative value.
    pub fn set_mask_local_adjustment(&mut self, key: &str, value: f64) -> Result<(), GuiError> {
        if !matches!(
            key,
            "exposure"
                | "contrast"
                | "highlights"
                | "shadows"
                | "temperature_delta_k"
                | "tint_delta"
        ) {
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
        let mut adjustments = layer.local_adjustments.take().unwrap_or_default();
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

    /// Reset one local control to its neutral value while retaining the rest
    /// of the layer's local recipe. This is a real persisted/history edit, not
    /// a display-only slider clear. The relative WB controls reset to `0` too.
    pub fn reset_mask_local_adjustment(&mut self, key: &str) -> Result<(), GuiError> {
        self.set_mask_local_adjustment(key, 0.0)
    }

    /// Read the selected layer's relative WB delta.
    pub fn selected_mask_local_wb_delta(&self) -> Result<(f64, f64), GuiError> {
        let Some(layer) = self.selected_mask_layer() else {
            return Err(GuiError::Io(Str::NoMaskSelected.t().to_string()));
        };
        let adjustments = layer
            .effective_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?
            .unwrap_or_default();
        Ok((adjustments.temperature_delta_k, adjustments.tint_delta))
    }

    /// Set both relative-WB controls atomically on the selected mask layer.
    /// Absolute global WB fields are never read or written by this path.
    pub fn set_mask_local_wb_delta(
        &mut self,
        temperature_delta_k: f64,
        tint_delta: f64,
    ) -> Result<(), GuiError> {
        // Probe both values before taking a snapshot or normalizing a legacy
        // layer. Invalid input is therefore byte-for-byte non-mutating.
        let mut probe = lumina_sidecar::LocalAdjustments::default();
        probe
            .set_value("temperature_delta_k", temperature_delta_k)
            .map_err(|_| GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()))?;
        probe
            .set_value("tint_delta", tint_delta)
            .map_err(|_| GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()))?;

        let before = self.active_mask_layers_snapshot()?;
        let layer = self.active_layer_mut()?;
        layer
            .normalize_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut adjustments = layer.local_adjustments.take().unwrap_or_default();
        adjustments
            .set_value("temperature_delta_k", temperature_delta_k)
            .map_err(|_| GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()))?;
        adjustments
            .set_value("tint_delta", tint_delta)
            .map_err(|_| GuiError::Io(Str::InvalidLocalAdjustment.t().to_string()))?;
        layer.local_adjustments = Some(adjustments);
        self.arm_mask_state_history_from("mask.local.white_balance", before);
        // One transaction covers *both* relative deltas, so the log/jank action
        // label names the whole local-WB step. Labelling it with a single
        // control (`temperature_delta_k`) would report a tint-only pick — or any
        // temperature-neutral commit — as a temperature change.
        self.mark_recipe_dirty("mask.local.white_balance", temperature_delta_k);
        self.status = "Local relative white balance set from selected sample".into();
        self.local_wb_pick_mode = false;
        Ok(())
    }

    /// Alias with the terminology used by the CLI and API clients.
    pub fn set_mask_local_white_balance_delta(
        &mut self,
        temperature_delta_k: f64,
        tint_delta: f64,
    ) -> Result<(), GuiError> {
        self.set_mask_local_wb_delta(temperature_delta_k, tint_delta)
    }

    /// Derive a *relative* local WB delta from a sampled RGB point. Unlike the
    /// global picker this returns no absolute temperature and never consults
    /// the global recipe.
    pub fn local_white_balance_delta_from_point(r: f64, g: f64, b: f64) -> Option<(f64, f64)> {
        if !r.is_finite() || !g.is_finite() || !b.is_finite() || r <= 0.0 || g <= 0.0 || b <= 0.0 {
            return None;
        }
        let l = (r + g + b) / 3.0;
        let gr = l / r;
        let gg = l / g;
        let gb = l / b;
        // Invert the same relative gains used by the CPU kernel:
        // R = 1 - warmth*0.35, G = 1 - tint*0.20, B = 1 + warmth*0.35.
        let warmth = (((1.0 - gr) + (gb - 1.0)) / 2.0) / 0.35;
        let temperature_delta_k = (warmth * 5500.0).clamp(
            lumina_sidecar::LOCAL_WB_TEMPERATURE_DELTA_RANGE.0,
            lumina_sidecar::LOCAL_WB_TEMPERATURE_DELTA_RANGE.1,
        );
        let tint_delta = ((1.0 - gg) / 0.20).clamp(
            lumina_sidecar::LOCAL_WB_TINT_DELTA_RANGE.0,
            lumina_sidecar::LOCAL_WB_TINT_DELTA_RANGE.1,
        );
        Some((temperature_delta_k, tint_delta))
    }

    /// Set the selected layer's local WB from an already validated sample.
    /// This is the shared GUI/CLI setter path; it intentionally has no global
    /// fallback and is not an Auto-WB operation.
    pub fn set_mask_local_white_balance_from_point(
        &mut self,
        r: f64,
        g: f64,
        b: f64,
    ) -> Result<(), GuiError> {
        let Some((temperature_delta_k, tint_delta)) =
            Self::local_white_balance_delta_from_point(r, g, b)
        else {
            return Err(GuiError::Io(
                "Cannot derive local white balance from this point".into(),
            ));
        };
        self.set_mask_local_wb_delta(temperature_delta_k, tint_delta)
    }

    /// Arm the local relative-WB picker. It is mutually exclusive with the
    /// global picker, red-eye picker and mask tools.
    pub fn arm_mask_local_wb_picker(&mut self) {
        self.local_wb_pick_mode = true;
        self.wb_pick_mode = false;
        self.red_eye_pick_mode = false;
        self.mask_tool = MaskTool::None;
        self.spot_tool = SpotTool::None;
        self.clear_mask_gesture();
        self.status = "Click the preview inside the selected mask to set local relative WB".into();
    }

    /// Alias retained for callers that use the shorter local-WB terminology.
    pub fn arm_local_wb_picker(&mut self) {
        self.arm_mask_local_wb_picker();
    }

    pub fn local_wb_pick_mode(&self) -> bool {
        self.local_wb_pick_mode
    }

    /// Return explicit provenance for the stage a local WB sample would use.
    /// A missing or stale stage is an error, never a raw-image fallback.
    pub fn local_wb_sample_provenance(&self) -> Result<String, GuiError> {
        let Some(layer) = self.selected_mask_layer() else {
            return Err(GuiError::Io(Str::NoMaskSelected.t().to_string()));
        };
        let Some(stage) = self.effective_source_stage.as_ref() else {
            return Err(GuiError::Io(
                "local WB sample is missing: render an effective source stage first".into(),
            ));
        };
        let Some(render_key) = self.render_key.as_ref() else {
            return Err(GuiError::Io(
                "local WB sample is stale: no completed render identity".into(),
            ));
        };
        let digest = render_key.digest();
        if self.effective_source_stage_digest.as_deref() != Some(digest.as_str()) {
            return Err(GuiError::Io(
                "local WB sample is stale: effective source stage does not match the render".into(),
            ));
        }
        if self.preview_is_draft {
            return Err(GuiError::Io(
                "local WB sample is stale: draft pixels are not an effective source stage".into(),
            ));
        }
        let Some(mask) = self
            .render_mask_layers
            .iter()
            .find(|result| result.layer_id == layer.id)
        else {
            return Err(GuiError::Io(format!(
                "local WB sample is stale: selected mask layer `{}` has no evaluated matte",
                layer.id
            )));
        };
        if mask.plane.width != stage.width || mask.plane.height != stage.height {
            return Err(GuiError::Io(
                "local WB sample is stale: evaluated matte and effective source stage differ"
                    .into(),
            ));
        }
        Ok(format!(
            "effective_source_stage:{}x{};selected_layer:{};render_digest:{}",
            stage.width, stage.height, layer.id, digest
        ))
    }

    /// Sample the effective source stage at normalized display coordinates,
    /// enforce the selected mask, and set the local relative WB delta.
    pub fn pick_mask_local_white_balance_at(
        &mut self,
        nx: f64,
        ny: f64,
    ) -> Result<(f64, f64), GuiError> {
        if !nx.is_finite()
            || !ny.is_finite()
            || !(0.0..=1.0).contains(&nx)
            || !(0.0..=1.0).contains(&ny)
        {
            return Err(GuiError::Io(
                "local WB sample coordinates are outside the preview".into(),
            ));
        }
        let provenance = self.local_wb_sample_provenance()?;
        let stage = self
            .effective_source_stage
            .as_ref()
            .ok_or_else(|| GuiError::Io("local WB sample is missing".into()))?;
        let layer = self
            .selected_mask_layer()
            .ok_or_else(|| GuiError::Io(Str::NoMaskSelected.t().to_string()))?;
        let mask = self
            .render_mask_layers
            .iter()
            .find(|result| result.layer_id == layer.id)
            .ok_or_else(|| GuiError::Io("local WB sample is stale: matte disappeared".into()))?;
        let x = (nx * stage.width as f64) as u32;
        let y = (ny * stage.height as f64) as u32;
        let x = x.min(stage.width.saturating_sub(1));
        let y = y.min(stage.height.saturating_sub(1));
        let alpha_index = (y as usize) * (stage.width as usize) + x as usize;
        let alpha = mask.plane.values.get(alpha_index).copied().ok_or_else(|| {
            GuiError::Io("local WB sample is stale: matte index is unavailable".into())
        })?;
        if alpha == 0 {
            return Err(GuiError::Io(format!(
                "local WB sample is outside selected mask layer `{}` ({provenance})",
                layer.id
            )));
        }
        let pixel_index = alpha_index * 4;
        let pixel = stage
            .pixels
            .get(pixel_index..pixel_index + 4)
            .ok_or_else(|| {
                GuiError::Io("local WB sample is stale: source pixel is unavailable".into())
            })?;
        let r = f64::from(pixel[0]) / 255.0;
        let g = f64::from(pixel[1]) / 255.0;
        let b = f64::from(pixel[2]) / 255.0;
        self.set_mask_local_white_balance_from_point(r, g, b)?;
        Ok((nx, ny))
    }

    /// Alias for preview/event callers.
    pub fn pick_local_wb_at(&mut self, nx: f64, ny: f64) -> Result<(), GuiError> {
        self.pick_mask_local_white_balance_at(nx, ny).map(|_| ())
    }

    /// Explicitly return global white balance to the decoder's As-Shot basis.
    /// This removes the absolute WB keys rather than fabricating a 6500 K
    /// correction; local mask WB is a separate relative delta and is untouched.
    pub fn reset_white_balance_to_as_shot(&mut self) -> Result<(), GuiError> {
        self.recipe.adjustments.remove("wb_temperature");
        self.recipe.adjustments.remove("wb_tint");
        self.local_wb_pick_mode = false;
        self.pending_history_step = Some("wb.reset_as_shot".into());
        self.mark_recipe_dirty("wb.reset_as_shot", 0.0);
        self.status = Str::ResetAsShot.t().into();
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }
}
