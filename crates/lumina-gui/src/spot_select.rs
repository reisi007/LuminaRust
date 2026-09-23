//! R5-DUST-23-FOLLOWUP: spot selection + per-spot editing (update/remove).
//!
//! A spot removal is selected like a mask (`selected_spot_id`, display-only
//! session state, never recipe/sidecar): the pin/list click selects, the
//! type/status/parameter detail renders only for the selected removal, and a
//! fresh dab selects itself. [`LuminaApp::commit_spot_heal`] and
//! [`LuminaApp::clear_spot_heals`] moved here verbatim from `lib.rs`
//! (file-size ratchet); the edit paths ([`LuminaApp::update_spot_heal`],
//! [`LuminaApp::remove_spot`]) validate like commit, preserve id/mode/status
//! and go through the same dirty → sidecar → render path. Nothing here heals
//! generatively: a generative entry carries no heuristic geometry, so
//! geometry edits on it (and variant regeneration on heuristics) fail loudly
//! — Clone is never used as a fallback (SOLL § R5-DUST-23-FOLLOWUP).

use super::*;
use log::info;

/// Screen-space hit tolerance for selecting an existing spot pin instead of
/// dabbing a new spot (screen points, Lightroom-like pin grab radius).
pub(crate) const SPOT_PIN_HIT_TOLERANCE_PX: f32 = 12.0;

/// Short display id for spot rows (the full `spot-<64 hex>` id would
/// overflow the narrow center strip and clip the row buttons — R5-DUST-23
/// B1 lesson; selection still keys on the full id).
pub(crate) fn short_spot_id(id: &str) -> String {
    const KEEP: usize = 12;
    if id.len() <= KEEP {
        return id.to_string();
    }
    format!("{}…", &id[..KEEP])
}

/// Display type label for a spot `mode` wire value (SOLL § FOLLOWUP: labels
/// only — the recipe wire values `heuristic`/`generative` are unchanged, and
/// no `Clone` mode is ever offered).
pub(crate) fn spot_type_label(mode: &str) -> &'static str {
    match mode {
        "generative" => "Generate (AI)",
        _ => "Heal",
    }
}

/// Display status for one extras spot entry: a generative entry without an
/// artifact reads `missing` (visible, never rendered) even when its stored
/// status still says `valid`; everything else shows the stored status.
#[cfg(test)]
pub(crate) fn spot_display_status(entry: &serde_json::Value) -> String {
    let mode = entry
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("heuristic");
    let stored = entry
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("valid");
    if mode == "generative"
        && entry
            .get("artifact")
            .is_none_or(|artifact| artifact.is_null())
    {
        return "missing".into();
    }
    stored.into()
}

impl LuminaApp {
    /// All spot-removal extras entries of the active recipe (geometry-carrying
    /// view; generative entries included — callers decide what applies).
    pub(crate) fn spot_entries(&self) -> Vec<serde_json::Value> {
        lumina_sidecar::spot_removal_entries(&self.recipe)
    }

    /// Effective status for the active source bundle. The CLI uses the same
    /// sidecar decision layer, so a missing/null/corrupt generative reference
    /// cannot be shown as raw `valid` in one surface and healthy in the other.
    pub(crate) fn spot_status_text(&self, entry: &serde_json::Value) -> String {
        let root = Path::new(&self.path)
            .parent()
            .unwrap_or_else(|| Path::new("."));
        lumina_sidecar::spot_removal_status(entry, root)
            .as_str()
            .to_owned()
    }

    /// Write back the whole extras spot array (single choke point for the
    /// edit paths below so add/update/remove share one serialization).
    /// Dropping the last entry also clears the typed `spot_removals` mirror:
    /// a stale mirror would serialize as lossy id-less shadows with no
    /// extras view to shadow them, and the reload would fail validation
    /// (same reason the CLI `--clear` clears both).
    pub(crate) fn store_spot_entries(
        &mut self,
        spots: &[serde_json::Value],
    ) -> Result<(), GuiError> {
        lumina_sidecar::set_spot_removal_entries(&mut self.recipe, spots)
            .map_err(|error| GuiError::Io(format!("spot recipe rejected: {error}")))
    }

    /// Select one spot removal by id (display-only session state, like
    /// [`Self::select_mask`]). Unknown ids fail loudly without touching the
    /// current selection.
    pub fn select_spot(&mut self, spot_id: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SelectSpot);
        let matches = self
            .spot_entries()
            .into_iter()
            .filter(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(spot_id))
            .count();
        if matches != 1 {
            let detail = if matches > 1 { " (duplicate id)" } else { "" };
            return Err(GuiError::Io(format!("Unknown spot `{spot_id}`{detail}")));
        }
        self.selected_spot_id = Some(spot_id.into());
        info!("GUI interaction: select_spot -> {spot_id}");
        Ok(())
    }

    /// Currently selected spot id, if any (session state, never persisted).
    pub fn selected_spot_id(&self) -> Option<&str> {
        self.selected_spot_id.as_deref()
    }

    /// The selected spot's extras entry, if the selection still resolves
    /// (a stale selection after external edits reads as no selection).
    pub fn selected_spot_entry(&self) -> Option<serde_json::Value> {
        let id = self.selected_spot_id.as_deref()?;
        let mut matches = self
            .spot_entries()
            .into_iter()
            .filter(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(id));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    /// Drop the selection when it no longer resolves (call after edits that
    /// may have removed the selected entry).
    pub(crate) fn prune_selected_spot(&mut self) {
        let stale = match self.selected_spot_id.as_deref() {
            Some(id) => !self
                .spot_entries()
                .iter()
                .any(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(id)),
            None => false,
        };
        if stale {
            self.selected_spot_id = None;
        }
    }

    /// Hit-test a normalized click against existing spot centres: the nearest
    /// entry within [`SPOT_PIN_HIT_TOLERANCE_PX`] screen points wins, so a
    /// click on a pin selects instead of dabbing. Pure (no state change).
    pub(crate) fn spot_hit_at(&self, nx: f32, ny: f32, scale: f32) -> Option<String> {
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        let (width, height) = self.image_dims().unwrap_or((1, 1));
        let tolerance_px = SPOT_PIN_HIT_TOLERANCE_PX / scale;
        let mut best: Option<(f32, String)> = None;
        for entry in self.spot_entries() {
            let centre = entry
                .get("center_x")
                .and_then(serde_json::Value::as_f64)
                .zip(entry.get("center_y").and_then(serde_json::Value::as_f64));
            let Some((x, y)) = centre else {
                continue;
            };
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let dx = (f64::from(nx) - x) * f64::from(width);
            let dy = (f64::from(ny) - y) * f64::from(height);
            let dist_px = (dx * dx + dy * dy).sqrt() as f32;
            if dist_px <= tolerance_px {
                let Some(id) = entry.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let id = id.to_string();
                if best.as_ref().is_none_or(|(d, _)| dist_px < *d) {
                    best = Some((dist_px, id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    pub fn commit_spot_heal(
        &mut self,
        center: lumina_sidecar::Point2,
        radius: f32,
        feather: f32,
        offset: lumina_sidecar::Point2,
        opacity: f32,
    ) -> Result<(), GuiError> {
        if !center.x.is_finite()
            || !center.y.is_finite()
            || !(0.0..=1.0).contains(&center.x)
            || !(0.0..=1.0).contains(&center.y)
        {
            return Err(GuiError::Io("Spot center must be 0..=1".into()));
        }
        if !radius.is_finite() || !(1.0..=512.0).contains(&radius) {
            return Err(GuiError::Io("Spot radius must be 1..=512".into()));
        }
        if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
            return Err(GuiError::Io("Spot feather must be 0..=1".into()));
        }
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(GuiError::Io("Spot opacity must be 0..=1".into()));
        }
        let id = format!(
            "spot-{}",
            blake3::hash(format!("{:.6},{:.6},{:.2}", center.x, center.y, radius).as_bytes())
                .to_hex()
        );
        let spot = serde_json::json!({"id": id, "version": lumina_sidecar::SPOT_REMOVAL_VERSION, "mode": "heuristic", "center_x": center.x, "center_y": center.y, "radius": radius, "feather": feather, "offset_dx": offset.x, "offset_dy": offset.y, "opacity": opacity, "status": "valid"});
        let mut spots = self.spot_entries();
        if spots
            .iter()
            .any(|entry| entry.get("id").and_then(|value| value.as_str()) == Some(id.as_str()))
        {
            return Err(GuiError::Io(format!(
                "Spot `{id}` already exists; IDs must be unique"
            )));
        }
        spots.push(spot);
        self.store_spot_entries(&spots)?;
        // R5-DUST-23-FOLLOWUP: a fresh dab selects itself (detail shows only
        // for the selection, so the new spot's params are visible at once).
        self.selected_spot_id = Some(id.clone());
        self.mark_dirty();
        self.save_sidecar();
        // GEN-ONNX-1 Welle 2b (F4/F7): never swallow a render error — surface
        // it loudly via the visible error dialog (the spot itself is already
        // persisted; the render failure must not be discarded).
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: commit_spot_heal -> {id}");
        Ok(())
    }

    /// Update one heuristic spot's heal parameters (validated like commit;
    /// id/mode/status preserved). Generative entries carry no heuristic
    /// geometry, so editing them fails loudly instead of silently rewriting
    /// them into a Clone — Clone is never used as a fallback.
    pub fn update_spot_heal(
        &mut self,
        spot_id: &str,
        radius: f32,
        feather: f32,
        opacity: f32,
        offset: lumina_sidecar::Point2,
    ) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::UpdateSpot);
        if !radius.is_finite() || !(1.0..=512.0).contains(&radius) {
            return Err(GuiError::Io("Spot radius must be 1..=512".into()));
        }
        if !feather.is_finite() || !(0.0..=1.0).contains(&feather) {
            return Err(GuiError::Io("Spot feather must be 0..=1".into()));
        }
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(GuiError::Io("Spot opacity must be 0..=1".into()));
        }
        if !offset.x.is_finite()
            || !offset.y.is_finite()
            || !(-1.0..=1.0).contains(&offset.x)
            || !(-1.0..=1.0).contains(&offset.y)
        {
            return Err(GuiError::Io("Spot offset must be -1..=1".into()));
        }
        let mut spots = self.spot_entries();
        let matches: Vec<usize> = spots
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (entry.get("id").and_then(|v| v.as_str()) == Some(spot_id)).then_some(index)
            })
            .collect();
        let index = match matches.as_slice() {
            [] => return Err(GuiError::Io(format!("Unknown spot `{spot_id}`"))),
            [index] => *index,
            _ => {
                return Err(GuiError::Io(format!(
                    "Spot `{spot_id}` is ambiguous: duplicate ids are not valid targets"
                )))
            }
        };
        let mode = spots[index]
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("heuristic");
        if mode != "heuristic" {
            return Err(GuiError::Io(format!(
                "Spot `{spot_id}` is {mode}, not heuristic: geometry edits apply to Heal spots only (use Regenerate for Generate spots)"
            )));
        }
        spots[index]["radius"] = serde_json::json!(radius);
        spots[index]["feather"] = serde_json::json!(feather);
        spots[index]["opacity"] = serde_json::json!(opacity);
        spots[index]["offset_dx"] = serde_json::json!(offset.x);
        spots[index]["offset_dy"] = serde_json::json!(offset.y);
        self.store_spot_entries(&spots)?;
        self.mark_dirty();
        self.save_sidecar();
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: update_spot -> {spot_id}");
        Ok(())
    }

    /// Delete exactly one spot removal by id (the selection follows when it
    /// pointed at the removed entry). Unknown ids fail loudly.
    pub fn remove_spot(&mut self, spot_id: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::RemoveSpot);
        let mut spots = self.spot_entries();
        let matches: Vec<usize> = spots
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (entry.get("id").and_then(|v| v.as_str()) == Some(spot_id)).then_some(index)
            })
            .collect();
        let index = match matches.as_slice() {
            [] => return Err(GuiError::Io(format!("Unknown spot `{spot_id}`"))),
            [index] => *index,
            _ => {
                return Err(GuiError::Io(format!(
                    "Spot `{spot_id}` is ambiguous: duplicate ids are not valid targets"
                )))
            }
        };
        spots.remove(index);
        self.store_spot_entries(&spots)?;
        self.prune_selected_spot();
        self.mark_dirty();
        self.save_sidecar();
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: remove_spot -> {spot_id}");
        Ok(())
    }

    pub fn clear_spot_heals(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearSpotHeals);
        self.recipe.extras.remove("spot_removals");
        // R5-DUST-23-FOLLOWUP: clear the typed mirror too — a doc loaded
        // from file carries id-less shadows, and saving them without the
        // extras view breaks the reload (see `store_spot_entries`).
        self.recipe.spot_removals.clear();
        self.selected_spot_id = None;
        self.mark_dirty();
        self.save_sidecar();
        // F4/F7: surface the render result instead of discarding it.
        if let Err(error) = self.render() {
            self.show_error(error);
        }
    }
}
