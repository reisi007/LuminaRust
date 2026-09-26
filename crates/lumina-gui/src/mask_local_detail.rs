//! MASK-LOCAL-P1.2d GUI: the mask-local detail editor and setters.
//!
//! This mirrors the global detail section — the same sharpening
//! amount/radius/detail/masking and the same noise-reduction luminance/colour,
//! in the *same* global ranges — but it writes to the **selected mask layer's**
//! typed local recipe and never to the global `EditRecipe::sharpening` or
//! `EditRecipe::noise_reduction`. A local detail edit is therefore visible in the
//! same transaction as every other local adjustment: it arms the coalesced
//! mask-state history snapshot, arms the debounced save, and re-renders the
//! mask-aware CPU frame.
//!
//! Reset works on three levels: one sub-block (`sharpening`, `noise_reduction`),
//! the whole block (`detail`), and — implicitly — returning a sub-block to its
//! neutral state, which drops just that sub-block.
//!
//! The editor deliberately has **no** scale control. The local radius follows the
//! global render scale, so a second per-mask scale slider would be a second
//! source of truth for something the global stage already owns.
//!
//! CPU-first: a local detail block is a local adjustment, so the existing
//! `local_adjustment_route_reason()` refusal keeps every GPU/stand-in route on
//! the CPU reference until hardware parity exists. The refusal predicate is
//! `is_neutral`, which now carries the detail block, so a detail-only layer is
//! refused too.
//!
//! The still-disabled local AI-denoise and optics have no setter, no field and no
//! widget here: they have no key in the CLI channel either, so a request for one
//! is a loud "unknown local adjustment" error rather than a silent no-op.
//! Optics stays disabled *permanently* — lens correction and perspective are
//! geometric stages ahead of the masks, not a per-mask per-pixel tone stage.

use super::{GuiError, LuminaApp, Str};
use crate::egui;
use log::info;
use lumina_sidecar::{LocalAdjustments, NoiseReduction, Sharpening};

impl LuminaApp {
    /// Read the selected layer's local sharpening block, or the neutral block
    /// for a sub-block that was never edited.
    pub fn selected_mask_local_sharpening(&self) -> Result<Sharpening, GuiError> {
        Ok(self
            .selected_local_recipe()?
            .detail
            .and_then(|detail| detail.sharpening)
            .unwrap_or(Sharpening {
                version: lumina_sidecar::DETAIL_BLOCK_VERSION,
                amount: 0.0,
                radius: 1.0,
                detail: 0.5,
                masking: 0.0,
            }))
    }

    /// Read the selected layer's local noise-reduction block, or the neutral
    /// block for a sub-block that was never edited.
    pub fn selected_mask_local_noise_reduction(&self) -> Result<NoiseReduction, GuiError> {
        Ok(self
            .selected_local_recipe()?
            .detail
            .and_then(|detail| detail.noise_reduction)
            .unwrap_or(NoiseReduction {
                version: lumina_sidecar::DETAIL_BLOCK_VERSION,
                luminance: 0.0,
                color: 0.0,
            }))
    }

    /// True when the selected layer stores a local sharpening sub-block that can
    /// change a pixel.
    pub fn has_mask_local_sharpening(&self) -> Result<bool, GuiError> {
        Ok(self.selected_local_recipe()?.has_local_sharpening())
    }

    /// True when the selected layer stores a local noise-reduction sub-block
    /// that can change a pixel.
    pub fn has_mask_local_noise_reduction(&self) -> Result<bool, GuiError> {
        Ok(self.selected_local_recipe()?.has_local_noise_reduction())
    }

    /// Apply one validated local-detail mutation as a single transaction.
    ///
    /// The mutation is validated on a *copy* first, so a refused value leaves
    /// both the layer and the pending history snapshot byte-for-byte unchanged.
    fn mutate_selected_local_detail(
        &mut self,
        action: &str,
        mutate: &mut dyn FnMut(&mut LocalAdjustments) -> Result<(), String>,
    ) -> Result<(), GuiError> {
        let mut probe = self.selected_local_recipe()?;
        (mutate)(&mut probe).map_err(|error| {
            self.status = format!("Local detail: {error}");
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
        // A local detail block is recipe data: it must arm the re-render *and*
        // the debounced save exactly like a local slider does.
        self.mark_recipe_dirty(action, 0.0);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    /// Set one local sharpening field on the selected mask layer.
    pub fn set_mask_local_sharpening(&mut self, field: &str, value: f64) -> Result<(), GuiError> {
        let field = field.to_string();
        let mut set =
            |recipe: &mut LocalAdjustments| recipe.set_local_sharpening_field(&field, value);
        self.mutate_selected_local_detail(
            &format!("mask.local.detail.sharpening.{field}"),
            &mut set,
        )?;
        info!("GUI interaction: local sharpening.{field} = {value}");
        Ok(())
    }

    /// Set one local noise-reduction field on the selected mask layer.
    pub fn set_mask_local_noise_reduction(
        &mut self,
        field: &str,
        value: f64,
    ) -> Result<(), GuiError> {
        let field = field.to_string();
        let mut set =
            |recipe: &mut LocalAdjustments| recipe.set_local_noise_reduction_field(&field, value);
        self.mutate_selected_local_detail(
            &format!("mask.local.detail.noise_reduction.{field}"),
            &mut set,
        )?;
        info!("GUI interaction: local noise_reduction.{field} = {value}");
        Ok(())
    }

    /// Reset one local detail sub-block (`sharpening` or `noise_reduction`).
    pub fn reset_mask_local_detail_field(&mut self, field: &str) -> Result<(), GuiError> {
        let field = field.to_string();
        let mut reset = |recipe: &mut LocalAdjustments| recipe.reset_local_detail_field(&field);
        self.mutate_selected_local_detail(&format!("mask.local.detail.{field}.reset"), &mut reset)
    }

    /// Reset the whole local detail block of the selected mask layer.
    pub fn reset_mask_local_detail(&mut self) -> Result<(), GuiError> {
        let mut reset = |recipe: &mut LocalAdjustments| {
            recipe.reset_local_detail();
            Ok(())
        };
        self.mutate_selected_local_detail("mask.local.detail.reset", &mut reset)
    }

    /// Paint the mask-local detail block of the Masking section.
    ///
    /// The draw path is a pure paint plus setter calls, exactly like the local
    /// sliders around it; every mutation goes through the setters, so no global
    /// recipe field is ever touched here.
    pub(crate) fn draw_mask_local_detail(&mut self, ui: &mut egui::Ui) {
        ui.label(Str::Detail.t());
        let sharpening = self.selected_mask_local_sharpening().unwrap_or(Sharpening {
            version: lumina_sidecar::DETAIL_BLOCK_VERSION,
            amount: 0.0,
            radius: 1.0,
            detail: 0.5,
            masking: 0.0,
        });
        // The ranges are the global F-095/F-096 ranges, read from the shared
        // sidecar table so the editor can never offer a value the validator
        // would refuse.
        let mut values = [
            (
                sharpening.amount,
                0.0_f32,
                3.0_f32,
                Str::Amount.t().to_string(),
            ),
            (
                sharpening.radius,
                0.1_f32,
                10.0_f32,
                Str::Radius.t().to_string(),
            ),
            (
                sharpening.detail,
                0.0_f32,
                1.0_f32,
                Str::Detail.t().to_string(),
            ),
            (
                sharpening.masking,
                0.0_f32,
                1.0_f32,
                Str::Masking.t().to_string(),
            ),
        ];
        for (index, field) in ["amount", "radius", "detail", "masking"].iter().enumerate() {
            let (mut value, low, high) = (values[index].0, values[index].1, values[index].2);
            let label = values[index].3.clone();
            if ui
                .add(egui::Slider::new(&mut value, low..=high).text(label))
                .changed()
            {
                values[index].0 = value;
                let stored = value as f64;
                if let Err(error) = self.set_mask_local_sharpening(field, stored) {
                    self.show_error(error);
                }
            }
        }
        let noise = self
            .selected_mask_local_noise_reduction()
            .unwrap_or(NoiseReduction {
                version: lumina_sidecar::DETAIL_BLOCK_VERSION,
                luminance: 0.0,
                color: 0.0,
            });
        ui.label(Str::NoiseReduction.t());
        let mut noise_values = [
            (noise.luminance, Str::Luminance.t().to_string()),
            (noise.color, Str::Color.t().to_string()),
        ];
        for (index, field) in ["luminance", "color"].iter().enumerate() {
            let mut value = noise_values[index].0;
            let label = noise_values[index].1.clone();
            if ui
                .add(egui::Slider::new(&mut value, 0.0_f32..=1.0_f32).text(label))
                .changed()
            {
                noise_values[index].0 = value;
                let stored = value as f64;
                if let Err(error) = self.set_mask_local_noise_reduction(field, stored) {
                    self.show_error(error);
                }
            }
        }
        for (field, label) in [
            ("sharpening", Str::Sharpening.t().to_string()),
            ("noise_reduction", Str::NoiseReduction.t().to_string()),
        ] {
            if ui
                .button(Str::SectionResetPattern.format_arg(&format!("local {label}")))
                .clicked()
            {
                if let Err(error) = self.reset_mask_local_detail_field(field) {
                    self.show_error(error);
                }
            }
        }
        if ui
            .button(Str::SectionResetPattern.format_arg("all local detail"))
            .clicked()
        {
            if let Err(error) = self.reset_mask_local_detail() {
                self.show_error(error);
            }
        }
    }
}
