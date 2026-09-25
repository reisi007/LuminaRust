//! MASK-LOCAL-P1.2b GUI: the mask-local per-pixel colour editor and setters.
//!
//! This mirrors the global Color section — the same HSL band names, the same
//! Point Color entry semantics, the same grading ranges — but it writes to the
//! **selected mask layer's** typed local recipe and never to the global
//! `EditRecipe::hsl`/`point_color`/`color_grading` or
//! `EditRecipe::adjustments["vibrance"|"saturation"]`. A local colour edit is
//! therefore visible in the same transaction as every other local adjustment:
//! it arms the coalesced mask-state history snapshot, arms the debounced save,
//! and re-renders the mask-aware CPU frame.
//!
//! CPU-first: a local colour block is a local adjustment, so the existing
//! `local_adjustment_route_reason()` refusal keeps every GPU/stand-in route on
//! the CPU reference until hardware parity exists.

use super::{GuiError, LuminaApp, Str};
use crate::{egui, theme};
use log::info;
use lumina_sidecar::{ColorGradingRange, LocalAdjustments, PointColorEntry};

/// The eight HSL band names in the canonical order the editor cycles through.
const HSL_BANDS: [&str; 8] = [
    "red", "orange", "yellow", "green", "cyan", "blue", "violet", "magenta",
];

/// The three HSL shift fields, in editor order.
const HSL_FIELDS: [&str; 3] = ["hue", "saturation", "luminance"];

/// The three grading ranges, in editor order.
const GRADING_RANGES: [&str; 3] = ["shadows", "midtones", "highlights"];

impl LuminaApp {
    /// Read the selected layer's local HSL band, or the neutral triple for a
    /// band that was never edited.
    pub fn selected_mask_local_hsl_band(&self, band: &str) -> Result<(f64, f64, f64), GuiError> {
        let channel = self
            .selected_local_recipe()?
            .local_hsl_band(band)
            .unwrap_or_default();
        Ok((
            f64::from(channel.hue),
            f64::from(channel.saturation),
            f64::from(channel.luminance),
        ))
    }

    /// Read the selected layer's local grading range, or the neutral range.
    pub fn selected_mask_local_grading_range(
        &self,
        range: &str,
    ) -> Result<(f64, f64, f64), GuiError> {
        let range = self
            .selected_local_recipe()?
            .local_color_grading_range(range)
            .unwrap_or_else(ColorGradingRange::neutral);
        Ok((
            f64::from(range.hue_degrees),
            f64::from(range.saturation),
            f64::from(range.luminance),
        ))
    }

    /// Read the selected layer's local grading `balance`/`blending` pair.
    pub fn selected_mask_local_grading_balance(&self) -> Result<(f64, f64), GuiError> {
        let grading = self.selected_local_recipe()?.local_color_grading();
        Ok(grading.map_or((0.0, 0.5), |grading| {
            (f64::from(grading.balance), f64::from(grading.blending))
        }))
    }

    /// Read the selected layer's local vibrance/saturation pair.
    pub fn selected_mask_local_vibrance(&self) -> Result<(f64, f64), GuiError> {
        let adjustments = self.selected_local_recipe()?;
        Ok((adjustments.vibrance, adjustments.saturation))
    }

    /// Read the selected layer's local Point Color entries in persisted order.
    pub fn selected_mask_local_point_color(&self) -> Result<Vec<PointColorEntry>, GuiError> {
        Ok(self
            .selected_local_recipe()?
            .local_point_color_entries()
            .to_vec())
    }

    /// True when the selected layer stores a local colour block that can
    /// change a pixel.
    pub fn has_mask_local_color(&self) -> Result<bool, GuiError> {
        Ok(self.selected_local_recipe()?.has_local_color())
    }

    /// Apply one validated local-colour mutation as a single transaction.
    ///
    /// The mutation is validated on a *copy* first, so a refused value leaves
    /// both the layer and the pending history snapshot byte-for-byte unchanged.
    fn mutate_selected_local_color(
        &mut self,
        action: &str,
        mutate: &mut dyn FnMut(&mut LocalAdjustments) -> Result<(), String>,
    ) -> Result<(), GuiError> {
        let mut probe = self.selected_local_recipe()?;
        (mutate)(&mut probe).map_err(|error| {
            self.status = format!("Local color: {error}");
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
        // A local colour block is recipe data: it must arm the re-render *and*
        // the debounced save exactly like a local slider does.
        self.mark_recipe_dirty(action, 0.0);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    /// Set one HSL shift of one band on the selected mask layer.
    pub fn set_mask_local_hsl_band(
        &mut self,
        band: &str,
        field: &str,
        value: f64,
    ) -> Result<(), GuiError> {
        let stored = value;
        let mut set = |recipe: &mut LocalAdjustments| recipe.set_local_hsl_band(band, field, value);
        self.mutate_selected_local_color(&format!("mask.local.hsl.{band}.{field}"), &mut set)?;
        info!("GUI interaction: local hsl.{band}.{field} = {stored}");
        Ok(())
    }

    /// Reset one HSL band, or the whole HSL block when `band` is empty.
    pub fn reset_mask_local_hsl_band(&mut self, band: &str) -> Result<(), GuiError> {
        if band.is_empty() {
            let mut reset = |recipe: &mut LocalAdjustments| {
                recipe.reset_local_hsl();
                Ok(())
            };
            return self.mutate_selected_local_color("mask.local.hsl.reset", &mut reset);
        }
        let mut reset = |recipe: &mut LocalAdjustments| recipe.reset_local_hsl_band(band);
        self.mutate_selected_local_color(&format!("mask.local.hsl.{band}.reset"), &mut reset)
    }

    /// Set the local vibrance or saturation scalar on the selected mask layer.
    pub fn set_mask_local_vibrance_saturation(
        &mut self,
        key: &str,
        value: f64,
    ) -> Result<(), GuiError> {
        let stored = value;
        let mut set = |recipe: &mut LocalAdjustments| recipe.set_value(key, value);
        self.mutate_selected_local_color(&format!("mask.local.{key}"), &mut set)?;
        info!("GUI interaction: local {key} = {stored}");
        Ok(())
    }

    /// Append one Point Color entry to the selected mask layer and return its
    /// new stable id.
    pub fn add_mask_local_point_color(
        &mut self,
        hue_center: f64,
        hue_range: f64,
        hue_shift: f64,
        saturation_shift: f64,
        luminance_shift: f64,
    ) -> Result<String, GuiError> {
        let mut id = String::new();
        {
            let mut add = |recipe: &mut LocalAdjustments| {
                id = recipe.add_local_point_color_entry(
                    hue_center,
                    hue_range,
                    hue_shift,
                    saturation_shift,
                    luminance_shift,
                )?;
                Ok(())
            };
            self.mutate_selected_local_color("mask.local.point_color.add", &mut add)?;
        }
        info!("GUI interaction: local point_color entry `{id}` added");
        Ok(id)
    }

    /// Set one field of an existing local Point Color entry.
    pub fn set_mask_local_point_color_field(
        &mut self,
        id: &str,
        field: &str,
        value: f64,
    ) -> Result<(), GuiError> {
        let mut set =
            |recipe: &mut LocalAdjustments| recipe.set_local_point_color_field(id, field, value);
        self.mutate_selected_local_color(&format!("mask.local.point_color.{id}.{field}"), &mut set)
    }

    /// Remove one local Point Color entry, or the whole block when `id` is
    /// empty.
    pub fn remove_mask_local_point_color(&mut self, id: &str) -> Result<(), GuiError> {
        if id.is_empty() {
            let mut reset = |recipe: &mut LocalAdjustments| {
                recipe.reset_local_point_color();
                Ok(())
            };
            return self.mutate_selected_local_color("mask.local.point_color.reset", &mut reset);
        }
        let mut reset = |recipe: &mut LocalAdjustments| recipe.remove_local_point_color_entry(id);
        self.mutate_selected_local_color(&format!("mask.local.point_color.{id}.remove"), &mut reset)
    }

    /// Set one field of the local grading block.
    pub fn set_mask_local_grading_field(
        &mut self,
        target: &str,
        field: &str,
        value: f64,
    ) -> Result<(), GuiError> {
        let mut set = |recipe: &mut LocalAdjustments| {
            recipe.set_local_color_grading_field(target, field, value)
        };
        self.mutate_selected_local_color(
            &format!("mask.local.color_grading.{target}.{field}"),
            &mut set,
        )
    }

    /// Reset one grading range, one of `balance`/`blending`, or the whole
    /// block when `target` is empty.
    pub fn reset_mask_local_grading(&mut self, target: &str) -> Result<(), GuiError> {
        if target.is_empty() {
            let mut reset = |recipe: &mut LocalAdjustments| {
                recipe.reset_local_color_grading();
                Ok(())
            };
            return self.mutate_selected_local_color("mask.local.color_grading.reset", &mut reset);
        }
        let mut reset =
            |recipe: &mut LocalAdjustments| recipe.reset_local_color_grading_field(target);
        self.mutate_selected_local_color(
            &format!("mask.local.color_grading.{target}.reset"),
            &mut reset,
        )
    }

    /// Reset every local colour control of the selected mask layer.
    pub fn reset_mask_local_color(&mut self) -> Result<(), GuiError> {
        let mut reset = |recipe: &mut LocalAdjustments| {
            recipe.reset_local_color();
            Ok(())
        };
        self.mutate_selected_local_color("mask.local.color.reset", &mut reset)
    }

    /// One `-1..=1` slider that writes straight into the local recipe.
    fn local_color_slider(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        value: &mut f64,
        setter: &dyn Fn(&mut Self, f64) -> Result<(), GuiError>,
    ) {
        if ui
            .add(egui::Slider::new(value, -1.0..=1.0).text(label))
            .changed()
        {
            let stored = *value;
            if let Err(error) = setter(self, stored) {
                self.show_error(error);
            }
        }
    }

    /// Paint the mask-local colour block of the Masking section.
    ///
    /// The draw path is a pure paint plus setter calls, exactly like the local
    /// sliders above it; every mutation goes through the setters, so no global
    /// recipe field is ever touched here.
    pub(crate) fn draw_mask_local_color(&mut self, ui: &mut egui::Ui) {
        ui.label(Str::HslMixer.t());
        let mut band = self.mask_local_hsl_band;
        ui.horizontal(|ui| {
            for (index, label) in [
                (0usize, Str::HslRed),
                (1, Str::HslOrange),
                (2, Str::HslYellow),
                (3, Str::HslGreen),
                (4, Str::HslCyan),
                (5, Str::HslBlue),
                (6, Str::HslViolet),
                (7, Str::HslMagenta),
            ] {
                if ui.selectable_label(band == index, label.t()).clicked() {
                    band = index;
                }
            }
        });
        if band != self.mask_local_hsl_band {
            self.mask_local_hsl_band = band;
            info!("GUI interaction: mask_local_hsl_band {band}");
        }
        let name = HSL_BANDS[band.min(HSL_BANDS.len() - 1)];
        let (hue, saturation, luminance) = self
            .selected_mask_local_hsl_band(name)
            .unwrap_or((0.0, 0.0, 0.0));
        let mut values = [hue, saturation, luminance];
        for (index, field) in HSL_FIELDS.iter().enumerate() {
            let band = name.to_string();
            let field = field.to_string();
            self.local_color_slider(
                ui,
                match index {
                    0 => Str::Hue.t().to_string(),
                    1 => Str::Saturation.t().to_string(),
                    _ => Str::Luminance.t().to_string(),
                }
                .as_str(),
                &mut values[index],
                &move |app, value| app.set_mask_local_hsl_band(&band, &field, value),
            );
        }
        if ui.button(Str::Reset.t()).clicked() {
            if let Err(error) = self.reset_mask_local_hsl_band(name) {
                self.show_error(error);
            }
        }

        ui.separator();
        let (mut vibrance, mut saturation) =
            self.selected_mask_local_vibrance().unwrap_or((0.0, 0.0));
        self.local_color_slider(ui, Str::Vibrance.t(), &mut vibrance, &|app, value| {
            app.set_mask_local_vibrance_saturation("vibrance", value)
        });
        self.local_color_slider(ui, Str::Saturation.t(), &mut saturation, &|app, value| {
            app.set_mask_local_vibrance_saturation("saturation", value)
        });

        ui.separator();
        ui.label(Str::PointColor.t());
        let entries = self.selected_mask_local_point_color().unwrap_or_default();
        for entry in &entries {
            ui.colored_label(
                theme::SEPARATOR,
                format!(
                    "{}: hue {}° ± {}°",
                    entry.id, entry.hue_center, entry.hue_range
                ),
            );
            if ui.button(Str::PointColorRemove.t()).clicked() {
                let id = entry.id.clone();
                if let Err(error) = self.remove_mask_local_point_color(&id) {
                    self.show_error(error);
                }
            }
        }
        if ui.button(Str::PointColorAdd.t()).clicked() {
            if let Err(error) = self.add_mask_local_point_color(30.0, 45.0, 0.0, 0.0, 0.0) {
                self.show_error(error);
            }
        }
        if !entries.is_empty() && ui.button(Str::Reset.t()).clicked() {
            if let Err(error) = self.remove_mask_local_point_color("") {
                self.show_error(error);
            }
        }

        ui.separator();
        ui.label(Str::ColorGrading.t());
        let mut range = self.mask_local_grading_range;
        ui.horizontal(|ui| {
            for (index, label) in [
                (0usize, Str::GradingShadows),
                (1, Str::GradingMidtones),
                (2, Str::GradingHighlights),
            ] {
                if ui.selectable_label(range == index, label.t()).clicked() {
                    range = index;
                }
            }
        });
        if range != self.mask_local_grading_range {
            self.mask_local_grading_range = range;
            info!("GUI interaction: mask_local_grading_range {range}");
        }
        let range_name = GRADING_RANGES[range.min(GRADING_RANGES.len() - 1)];
        let (hue, saturation, luminance) = self
            .selected_mask_local_grading_range(range_name)
            .unwrap_or((0.0, 0.0, 0.0));
        let mut hue_degrees = hue;
        if ui
            .add(egui::Slider::new(&mut hue_degrees, 0.0..=360.0).text(Str::Hue.t()))
            .changed()
        {
            if let Err(error) = self.set_mask_local_grading_field(range_name, "hue", hue_degrees) {
                self.show_error(error);
            }
        }
        let mut tint = saturation;
        if ui
            .add(egui::Slider::new(&mut tint, 0.0..=1.0).text(Str::Saturation.t()))
            .changed()
        {
            if let Err(error) = self.set_mask_local_grading_field(range_name, "saturation", tint) {
                self.show_error(error);
            }
        }
        let mut lift = luminance;
        if ui
            .add(egui::Slider::new(&mut lift, -1.0..=1.0).text(Str::Luminance.t()))
            .changed()
        {
            if let Err(error) = self.set_mask_local_grading_field(range_name, "luminance", lift) {
                self.show_error(error);
            }
        }
        let (mut balance, mut blending) = self
            .selected_mask_local_grading_balance()
            .unwrap_or((0.0, 0.5));
        if ui
            .add(egui::Slider::new(&mut balance, -1.0..=1.0).text(Str::GradingBalance.t()))
            .changed()
        {
            if let Err(error) = self.set_mask_local_grading_field("balance", "value", balance) {
                self.show_error(error);
            }
        }
        if ui
            .add(egui::Slider::new(&mut blending, 0.0..=1.0).text(Str::GradingBlending.t()))
            .changed()
        {
            if let Err(error) = self.set_mask_local_grading_field("blending", "value", blending) {
                self.show_error(error);
            }
        }
        if ui.button(Str::Reset.t()).clicked() {
            if let Err(error) = self.reset_mask_local_grading(range_name) {
                self.show_error(error);
            }
        }

        if ui
            .button(Str::SectionResetPattern.format_arg("all local color"))
            .clicked()
        {
            if let Err(error) = self.reset_mask_local_color() {
                self.show_error(error);
            }
        }
    }
}
