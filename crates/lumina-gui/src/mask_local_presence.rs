//! MASK-LOCAL-P1.2c GUI: the mask-local presence editor and setters.
//!
//! This mirrors the global presence block — the same three amounts, the same
//! `-1..=1` range — but it writes to the **selected mask layer's** typed local
//! recipe and never to the global `EditRecipe::presence`. A local presence edit
//! is therefore visible in the same transaction as every other local
//! adjustment: it arms the coalesced mask-state history snapshot, arms the
//! debounced save, and re-renders the mask-aware CPU frame.
//!
//! CPU-first: a local presence block is a local adjustment, so the existing
//! `local_adjustment_route_reason()` refusal keeps every GPU/stand-in route on
//! the CPU reference until hardware parity exists. The refusal predicate is
//! `is_neutral`, which now carries the presence block, so a presence-only layer
//! is refused too.
//!
//! The still-disabled local stages (detail, sharpening, noise reduction,
//! AI-denoise, optics) have no setter, no field and no widget here: they have
//! no key in the CLI channel either, so a request for one is a loud "unknown
//! local adjustment" error rather than a silent no-op. Optics stays disabled
//! *permanently* — lens correction and perspective are geometric stages ahead
//! of the masks, not a per-mask per-pixel tone stage.

use super::{GuiError, LuminaApp, Str};
use crate::egui;
use log::info;
use lumina_sidecar::LocalAdjustments;

/// The three presence amounts in editor order.
const PRESENCE_FIELDS: [&str; 3] = ["texture", "clarity", "dehaze"];

impl LuminaApp {
    /// Read the selected layer's local presence amounts, or the neutral triple
    /// for a block that was never edited.
    pub fn selected_mask_local_presence(&self) -> Result<(f64, f64, f64), GuiError> {
        Ok(self.selected_local_recipe()?.local_presence())
    }

    /// True when the selected layer stores a local presence block that can
    /// change a pixel.
    pub fn has_mask_local_presence(&self) -> Result<bool, GuiError> {
        Ok(self.selected_local_recipe()?.has_local_presence())
    }

    /// Apply one validated local-presence mutation as a single transaction.
    ///
    /// The mutation is validated on a *copy* first, so a refused value leaves
    /// both the layer and the pending history snapshot byte-for-byte unchanged.
    fn mutate_selected_local_presence(
        &mut self,
        action: &str,
        mutate: &mut dyn FnMut(&mut LocalAdjustments) -> Result<(), String>,
    ) -> Result<(), GuiError> {
        let mut probe = self.selected_local_recipe()?;
        (mutate)(&mut probe).map_err(|error| {
            self.status = format!("Local presence: {error}");
            GuiError::Io(error)
        })?;
        let before = self.active_mask_layers_snapshot()?;
        let layer = self.active_layer_mut()?;
        // Normalize a legacy layer before editing it; a conflict is loud and
        // leaves both the layer and the pending snapshot untouched.
        layer
            .normalize_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut adjustments = layer.local_adjustments.take().unwrap_or_default();
        (mutate)(&mut adjustments).map_err(GuiError::Io)?;
        layer.local_adjustments = Some(adjustments);
        self.arm_mask_state_history_from(action, before);
        // A local presence block is recipe data: it must arm the re-render *and*
        // the debounced save exactly like a local slider does.
        self.mark_recipe_dirty(action, 0.0);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    /// Set one local presence amount on the selected mask layer.
    pub fn set_mask_local_presence(&mut self, field: &str, value: f64) -> Result<(), GuiError> {
        let stored = value;
        let field = field.to_string();
        let mut set =
            |recipe: &mut LocalAdjustments| recipe.set_local_presence_field(&field, value);
        self.mutate_selected_local_presence(&format!("mask.local.presence.{field}"), &mut set)?;
        info!("GUI interaction: local presence.{field} = {stored}");
        Ok(())
    }

    /// Reset the whole local presence block of the selected mask layer.
    pub fn reset_mask_local_presence(&mut self) -> Result<(), GuiError> {
        let mut reset = |recipe: &mut LocalAdjustments| {
            recipe.reset_local_presence();
            Ok(())
        };
        self.mutate_selected_local_presence("mask.local.presence.reset", &mut reset)
    }

    /// Paint the mask-local presence block of the Masking section.
    ///
    /// The draw path is a pure paint plus setter calls, exactly like the local
    /// sliders around it; every mutation goes through the setters, so no global
    /// recipe field is ever touched here.
    pub(crate) fn draw_mask_local_presence(&mut self, ui: &mut egui::Ui) {
        ui.label(Str::Presence.t());
        let (texture, clarity, dehaze) = self
            .selected_mask_local_presence()
            .unwrap_or((0.0, 0.0, 0.0));
        let mut values = [texture, clarity, dehaze];
        for (index, field) in PRESENCE_FIELDS.iter().enumerate() {
            let label = match index {
                0 => Str::Texture.t().to_string(),
                1 => Str::Clarity.t().to_string(),
                _ => Str::Dehaze.t().to_string(),
            };
            let field = field.to_string();
            if ui
                .add(egui::Slider::new(&mut values[index], -1.0..=1.0).text(label))
                .changed()
            {
                let stored = values[index];
                if let Err(error) = self.set_mask_local_presence(&field, stored) {
                    self.show_error(error);
                }
            }
        }
        if ui
            .button(Str::SectionResetPattern.format_arg("all local presence"))
            .clicked()
        {
            if let Err(error) = self.reset_mask_local_presence() {
                self.show_error(error);
            }
        }
    }
}
