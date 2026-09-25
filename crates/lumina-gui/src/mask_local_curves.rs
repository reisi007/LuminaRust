//! MASK-LOCAL-P1.2a GUI: the mask-local tone-curve editor and its setters.
//!
//! This mirrors the global Tone Curve editor in every user-visible way — the
//! same graph widget, the same gestures, the same mandatory `(0,0)`/`(1,1)`
//! endpoints — but it writes to the **selected mask layer's** typed local
//! recipe and never to the global `EditRecipe::curves`. A local curve edit is
//! therefore visible in the same transaction as every other local adjustment:
//! it arms the coalesced mask-state history snapshot, arms the debounced save,
//! and re-renders the mask-aware CPU frame.
//!
//! CPU-first: a local curve is a local adjustment, so the existing
//! `local_adjustment_route_reason()` refusal keeps every GPU/stand-in route on
//! the CPU reference until hardware parity exists.

use super::{GuiError, LuminaApp, Str};
use crate::develop_tone::tone_curve_graph::{
    clamped_point_move, curve_graph_gesture, local_tone_curve_graph_id, nearest_point_index,
    paint_tone_curve_graph, tone_curve_display_output, CurveGesture, TONE_CURVE_GRAPH_SIDE,
    TONE_CURVE_HIT_RADIUS,
};
use crate::{egui, theme};
use log::{info, warn};
use lumina_sidecar::{identity_curve_points, CurvePoint, CurvePoints, LocalAdjustments};

/// Resolve the control points a channel currently displays: the stored list,
/// or the two-point identity for a channel that was never edited.
fn local_curve_points(adjustments: &LocalAdjustments, channel: &str) -> CurvePoints {
    adjustments
        .local_curve_channel(channel)
        .filter(|points| points.len() >= 2)
        .unwrap_or_else(identity_curve_points)
}

impl LuminaApp {
    /// The selected layer's effective local recipe (loud on malformed state).
    ///
    /// Shared with the MASK-LOCAL-P1.2b colour editor so both surfaces read the
    /// exact same layer through the exact same loud migration view.
    pub(crate) fn selected_local_recipe(&self) -> Result<LocalAdjustments, GuiError> {
        let Some(layer) = self.selected_mask_layer() else {
            return Err(GuiError::Io(Str::NoMaskSelected.t().to_string()));
        };
        layer
            .effective_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?
            .map_or_else(
                || Ok(LocalAdjustments::default()),
                |adjustments| {
                    adjustments
                        .validate()
                        .map_err(|error| GuiError::Io(error.to_string()))?;
                    Ok(adjustments)
                },
            )
    }

    /// Read one local curve channel of the selected mask layer.
    pub fn selected_mask_local_curve(&self, channel: &str) -> Result<CurvePoints, GuiError> {
        Ok(local_curve_points(&self.selected_local_recipe()?, channel))
    }

    /// True when the selected layer stores a local tone curve.
    pub fn has_mask_local_curves(&self) -> Result<bool, GuiError> {
        Ok(self.selected_local_recipe()?.has_local_curves())
    }

    /// Apply one validated local-curve mutation as a single transaction.
    ///
    /// The point list is validated by the shared global curve rules *before*
    /// the layer is touched, so a refused edit leaves both the layer and the
    /// pending history snapshot byte-for-byte unchanged.
    fn mutate_selected_local_curve(
        &mut self,
        channel: &str,
        action: &str,
        mutate: &dyn Fn(&mut LocalAdjustments) -> Result<(), String>,
    ) -> Result<(), GuiError> {
        // Probe the mutation on a copy first: validation errors must not
        // normalize or otherwise touch the selected layer.
        let mut probe = self.selected_local_recipe()?;
        mutate(&mut probe)
            .map_err(|error| GuiError::Io(Str::ToneCurveInvalidPattern.format_arg(&error)))?;
        let before = self.active_mask_layers_snapshot()?;
        let layer = self.active_layer_mut()?;
        // Normalize a legacy layer before editing it; a conflict is loud and
        // leaves both the layer and the pending snapshot untouched.
        layer
            .normalize_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut adjustments = layer.local_adjustments.take().unwrap_or_default();
        mutate(&mut adjustments)
            .map_err(|error| GuiError::Io(Str::ToneCurveInvalidPattern.format_arg(&error)))?;
        layer.local_adjustments = Some(adjustments);
        self.arm_mask_state_history_from(action, before);
        // A local curve is recipe data: it must arm the re-render *and* the
        // debounced save exactly like a local slider does.
        self.mark_recipe_dirty(action, 0.0);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    /// Replace one local curve channel with an explicit point list.
    pub fn set_mask_local_curve_channel(
        &mut self,
        channel: &str,
        points: CurvePoints,
    ) -> Result<(), GuiError> {
        let stored = points.len();
        let set =
            |recipe: &mut LocalAdjustments| recipe.set_local_curve_channel(channel, points.clone());
        self.mutate_selected_local_curve(channel, &format!("mask.local.curves.{channel}"), &set)?;
        info!("GUI interaction: local curves.{channel} replaced ({stored} points)");
        Ok(())
    }

    /// Insert one control point into a local curve channel, sorted by input.
    pub fn add_mask_local_curve_point(
        &mut self,
        channel: &str,
        input: f64,
        output: f64,
    ) -> Result<(), GuiError> {
        let mut points = self.selected_mask_local_curve(channel)?;
        points.push(CurvePoint {
            input: input as f32,
            output: output as f32,
        });
        points.sort_by(|a, b| a.input.total_cmp(&b.input));
        self.set_mask_local_curve_channel(channel, points)
    }

    /// Move one interior control point. Endpoints are refused loudly.
    pub fn move_mask_local_curve_point(
        &mut self,
        channel: &str,
        index: usize,
        input: f64,
        output: f64,
    ) -> Result<(), GuiError> {
        let points = self.selected_mask_local_curve(channel)?;
        if index == 0 || index + 1 >= points.len() {
            let reason = "endpoints (0,0)/(1,1) are mandatory";
            self.status = Str::ToneCurveInvalidPattern.format_arg(reason);
            warn!("move_mask_local_curve_point: {channel}[{index}] is an endpoint");
            return Err(GuiError::Io(reason.into()));
        }
        let (input, output) = clamped_point_move(&points, index, input as f32, output as f32);
        let mut moved = points;
        moved[index].input = input;
        moved[index].output = output;
        self.set_mask_local_curve_channel(channel, moved)
    }

    /// Remove one control point. Endpoints and an already-minimal channel are
    /// refused loudly.
    pub fn remove_mask_local_curve_point(
        &mut self,
        channel: &str,
        index: usize,
    ) -> Result<(), GuiError> {
        let points = self.selected_mask_local_curve(channel)?;
        if points.len() <= 2 {
            let reason = "need 2..=32 points";
            self.status = Str::ToneCurveInvalidPattern.format_arg(reason);
            warn!("remove_mask_local_curve_point: {channel} already minimal");
            return Err(GuiError::Io(reason.into()));
        }
        if index == 0 || index + 1 >= points.len() {
            let reason = "endpoints (0,0)/(1,1) are mandatory";
            self.status = Str::ToneCurveInvalidPattern.format_arg(reason);
            warn!("remove_mask_local_curve_point: {channel}[{index}] is an endpoint");
            return Err(GuiError::Io(reason.into()));
        }
        let mut remaining = points;
        remaining.remove(index);
        self.set_mask_local_curve_channel(channel, remaining)
    }

    /// Reset one local curve channel back to the identity.
    pub fn reset_mask_local_curve_channel(&mut self, channel: &str) -> Result<(), GuiError> {
        let reset = |recipe: &mut LocalAdjustments| recipe.reset_local_curve_channel(channel);
        self.mutate_selected_local_curve(
            channel,
            &format!("mask.local.curves.{channel}.reset"),
            &reset,
        )
    }

    /// Reset every local curve of the selected mask layer.
    pub fn reset_mask_local_curves(&mut self) -> Result<(), GuiError> {
        let _ = self.selected_local_recipe()?;
        let before = self.active_mask_layers_snapshot()?;
        let layer = self.active_layer_mut()?;
        layer
            .normalize_local_adjustments()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut adjustments = layer.local_adjustments.take().unwrap_or_default();
        adjustments.reset_local_curves();
        layer.local_adjustments = Some(adjustments);
        self.arm_mask_state_history_from("mask.local.curves.reset", before);
        self.mark_recipe_dirty("mask.local.curves.reset", 0.0);
        self.status = Str::LocalAdjustmentSaved.t().to_string();
        Ok(())
    }

    /// Paint the mask-local tone-curve editor for `channel`.
    ///
    /// The draw path is a pure paint plus one gesture-decoder call, exactly
    /// like the global graph; every mutation goes through the setters above.
    pub(crate) fn draw_local_tone_curve_graph(&mut self, ui: &mut egui::Ui, channel: &str) {
        let points = self
            .selected_mask_local_curve(channel)
            .unwrap_or_else(|_| identity_curve_points());
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(TONE_CURVE_GRAPH_SIDE, TONE_CURVE_GRAPH_SIDE),
            egui::Sense::hover(),
        );
        let response = ui
            .interact(
                rect,
                local_tone_curve_graph_id(channel),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Crosshair);
        let memory_id = local_tone_curve_graph_id(channel).with("drag");
        let active = ui.memory(|m| m.data.get_temp::<usize>(memory_id));
        let hover = response
            .hover_pos()
            .and_then(|pos| nearest_point_index(&points, rect, pos, TONE_CURVE_HIT_RADIUS));
        paint_tone_curve_graph(ui.painter_at(rect), rect, &points, channel, active, hover);
        let gestures = curve_graph_gesture(ui, rect, &response, &points, memory_id);
        for gesture in gestures {
            let outcome = match gesture {
                CurveGesture::Move {
                    index,
                    input,
                    output,
                } => self.move_mask_local_curve_point(
                    channel,
                    index,
                    f64::from(input),
                    f64::from(output),
                ),
                CurveGesture::Add { input, output } => {
                    self.add_mask_local_curve_point(channel, f64::from(input), f64::from(output))
                }
                CurveGesture::Remove { index } => {
                    self.remove_mask_local_curve_point(channel, index)
                }
                CurveGesture::EndpointRefused => {
                    self.status = Str::ToneCurveInvalidPattern
                        .format_arg("endpoints (0,0)/(1,1) are mandatory");
                    warn!("local tone curve: endpoints (0,0)/(1,1) are fixed");
                    Ok(())
                }
            };
            if let Err(error) = outcome {
                self.show_error(error);
            }
        }
    }

    /// The whole mask-local tone-curve block: channel selector, graph, reset.
    pub(crate) fn draw_mask_local_tone_curve(&mut self, ui: &mut egui::Ui) {
        ui.label(Str::ToneCurveChannel.t());
        let mut selected = self.mask_local_curve_channel;
        ui.horizontal(|ui| {
            for (index, label) in [
                (0usize, Str::ToneCurveChannelMaster),
                (1, Str::ToneCurveChannelRed),
                (2, Str::ToneCurveChannelGreen),
                (3, Str::ToneCurveChannelBlue),
            ] {
                if ui.selectable_label(selected == index, label.t()).clicked() {
                    selected = index;
                }
            }
        });
        if selected != self.mask_local_curve_channel {
            self.mask_local_curve_channel = selected;
            info!("GUI interaction: mask_local_curve_channel {selected}");
        }
        let channel = match self.mask_local_curve_channel {
            1 => "red",
            2 => "green",
            3 => "blue",
            _ => "master",
        };
        ui.label(Str::ToneCurvePoints.t());
        self.draw_local_tone_curve_graph(ui, channel);
        if ui.button(Str::Reset.t()).clicked() {
            if let Err(error) = self.reset_mask_local_curve_channel(channel) {
                self.show_error(error);
            }
        }
        if ui
            .button(Str::SectionResetPattern.format_arg("all local curves"))
            .clicked()
        {
            if let Err(error) = self.reset_mask_local_curves() {
                self.show_error(error);
            }
        }
        // Show that the drawn spline is the curve the renderer applies.
        let points = self
            .selected_mask_local_curve(channel)
            .unwrap_or_else(|_| identity_curve_points());
        let sample = tone_curve_display_output(&points, 0.5);
        ui.colored_label(
            theme::SEPARATOR,
            format!(
                "local curves.{channel}: {} pts, mid {:.3}",
                points.len(),
                sample
            ),
        );
    }
}
