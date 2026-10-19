//! G-14 red-eye removal (GUI slice, extracted from `lib.rs`).
//!
//! The persisted red-eye stage and its GUI verbs: mark a region from the
//! preview picker ([`LuminaApp::add_red_eye_region`]), edit/remove/clear
//! persisted regions, and the deterministic, model-free pupil detection with
//! its explicit apply
//! ([`LuminaApp::detect_red_eye_candidates`]/[`LuminaApp::apply_detected_red_eyes`]).
//! One responsibility: how the GUI turns a red-eye pick or detection into the
//! `recipe.red_eye` stage and back. Panel painting lives in `develop_detail.rs`,
//! the render stage in the shared core pipeline — neither is re-implemented
//! here.
//!
//! Extracted verbatim for the `GUI-LIB-SPLIT-65` file-size ratchet; the only
//! change to the moved code is the visibility of the private
//! `apply_detected_red_eye_objects` helper (`pub(crate)`, called from the Detail
//! panel and the headless tests).

use super::*;

impl LuminaApp {
    /// G-14: mark one red-eye region at normalized source coordinates (the
    /// preview picker path). The next free `re-N` id is used; the new region
    /// starts with the panel's default strengths and the persisted default
    /// radius. Loud when the 32-region cap is reached.
    ///
    /// L1: the coordinates are validated loudly against `0..=1` (finite) and
    /// never silently clipped — consistent with the pipeline's no-clipping
    /// rule. The preview picker already maps/clamps through
    /// [`Self::to_normalized`], so its clicks stay valid.
    pub fn add_red_eye_region(&mut self, x: f32, y: f32) -> Result<(), GuiError> {
        if !x.is_finite() || !(0.0..=1.0).contains(&x) {
            return Err(GuiError::Io(format!("red-eye x `{x}` out of 0..=1")));
        }
        if !y.is_finite() || !(0.0..=1.0).contains(&y) {
            return Err(GuiError::Io(format!("red-eye y `{y}` out of 0..=1")));
        }
        let mut correction = self.recipe.red_eye.clone().unwrap_or(RedEyeCorrection {
            version: 1,
            regions: Vec::new(),
        });
        if correction.regions.len() >= RED_EYE_MAX_REGIONS {
            return Err(GuiError::Io(format!(
                "red-eye region limit of {RED_EYE_MAX_REGIONS} reached"
            )));
        }
        let mut index = 1u32;
        let id = loop {
            let candidate = format!("re-{index}");
            if correction
                .regions
                .iter()
                .all(|region| region.id != candidate)
            {
                break candidate;
            }
            index += 1;
        };
        correction.regions.push(RedEyeRegion {
            id: id.clone(),
            x,
            y,
            radius: 0.05,
            desaturate: 0.8,
            darken: 0.4,
        });
        self.recipe.red_eye = Some(correction);
        self.mark_recipe_dirty("red_eye.add", f64::from(x) + f64::from(y));
        self.pending_history_step = Some("red_eye.add".into());
        info!("GUI interaction: add_red_eye_region {id} at ({x:.3},{y:.3})");
        Ok(())
    }

    /// G-14: set one field (`radius`/`desaturate`/`darken`) of one persisted
    /// region by its stable id. Unknown ids/fields are ignored loudly.
    pub fn set_red_eye_region_value(&mut self, id: &str, field: &str, value: f64) {
        let Some(correction) = self.recipe.red_eye.as_mut() else {
            warn!("set_red_eye_region_value: no red-eye stage");
            return;
        };
        let Some(region) = correction.regions.iter_mut().find(|region| region.id == id) else {
            warn!("set_red_eye_region_value: unknown region {id}");
            return;
        };
        match field {
            "radius" => region.radius = value as f32,
            "desaturate" => region.desaturate = value as f32,
            "darken" => region.darken = value as f32,
            _ => {
                warn!("set_red_eye_region_value: unknown field {field}");
                return;
            }
        }
        self.mark_recipe_dirty(&format!("red_eye.{id}.{field}"), value);
        self.pending_history_step = Some(format!("red_eye.{id}.{field}"));
        info!("GUI interaction: set_red_eye_region_value {id}.{field} -> {value}");
    }

    /// G-14: remove one persisted region by id. Unknown ids are ignored loudly.
    pub fn remove_red_eye_region(&mut self, id: &str) {
        instrument_gui_action!(self, GuiAction::RemoveRedEyeRegion);
        let Some(correction) = self.recipe.red_eye.as_mut() else {
            return;
        };
        let before = correction.regions.len();
        correction.regions.retain(|region| region.id != id);
        if correction.regions.len() == before {
            warn!("remove_red_eye_region: unknown region {id}");
            return;
        }
        self.mark_recipe_dirty("red_eye.remove", 0.0);
        self.pending_history_step = Some("red_eye.remove".into());
        info!("GUI interaction: remove_red_eye_region {id}");
    }

    /// G-14: remove the whole red-eye stage (identity).
    pub fn clear_red_eye(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearRedEye);
        self.recipe.red_eye = None;
        self.mark_recipe_dirty("red_eye.clear", 0.0);
        self.pending_history_step = Some("red_eye.clear".into());
        info!("GUI interaction: clear_red_eye");
    }

    /// LRPAR-G14-REDEYE-AUTO-15: run the deterministic, model-free pupil
    /// detection on the loaded source frame and list the candidates. This is
    /// display-only and never persists anything; the outcome text lands in
    /// `red_eye_detect_status` (visible, never silent). Applying stays explicit
    /// via [`Self::apply_detected_red_eyes`].
    pub fn detect_red_eye_candidates(&mut self) -> Result<Vec<DetectedRedEye>, GuiError> {
        instrument_gui_action!(self, GuiAction::DetectRedEye);
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        let detection = detect_red_eyes(&frame);
        let count = detection.candidates.len();
        let dropped = detection.dropped;
        info!(
            "GUI interaction: detect_red_eye_candidates -> {count} candidate(s), {dropped} dropped"
        );
        self.red_eye_detect_status = if count == 0 {
            "No red pupils detected".into()
        } else if dropped > 0 {
            format!(
                "Detected {count} pupil candidate(s); {dropped} dropped above the \
                 {RED_EYE_MAX_REGIONS}-region cap (not applied — use Apply)"
            )
        } else {
            format!("Detected {count} pupil candidate(s) (not applied — use Apply)")
        };
        Ok(detection.candidates)
    }

    /// LRPAR-G14-REDEYE-AUTO-15: persist detected pupils (explicit only). The
    /// automatic `auto-re-` regions are replaced by the fresh detection;
    /// manually marked regions are never touched. Loud when the 32-region cap
    /// would be exceeded (no silent truncation). One recipe edit + the shared
    /// debounced save/history path, exactly like [`Self::add_red_eye_region`].
    pub fn apply_detected_red_eyes(
        &mut self,
        candidates: &[DetectedRedEye],
    ) -> Result<usize, GuiError> {
        let before = self.recipe.red_eye.clone();
        let mut correction = before.clone().unwrap_or(RedEyeCorrection {
            version: 1,
            regions: Vec::new(),
        });
        correction
            .regions
            .retain(|region| !region.id.starts_with(RED_EYE_DETECT_ID_PREFIX));
        if correction.regions.len() + candidates.len() > RED_EYE_MAX_REGIONS {
            return Err(GuiError::Io(format!(
                "red-eye detection would exceed the {RED_EYE_MAX_REGIONS}-region cap: \
                 {} manual region(s) + {} detected",
                correction.regions.len(),
                candidates.len()
            )));
        }
        for candidate in candidates {
            let region = candidate.to_region();
            match correction
                .regions
                .iter_mut()
                .find(|existing| existing.id == region.id)
            {
                Some(existing) => *existing = region,
                None => correction.regions.push(region),
            }
        }
        let after = if correction.regions.is_empty() {
            None
        } else {
            Some(correction)
        };
        if before == after {
            self.red_eye_detect_status = format!(
                "No change: {} auto region(s) already applied",
                candidates.len()
            );
            return Ok(0);
        }
        self.recipe.red_eye = after;
        self.mark_recipe_dirty("red_eye.detect_apply", candidates.len() as f64);
        self.pending_history_step = Some("red_eye.detect_apply".into());
        self.red_eye_detect_status = format!("Applied {} detected pupil(s)", candidates.len());
        info!(
            "GUI interaction: apply_detected_red_eyes -> {} region(s)",
            candidates.len()
        );
        Ok(candidates.len())
    }

    /// GUI-INSTRDBG-17b-REST: the red-eye "Apply detected" button command. One
    /// outer instrumented action around detection + persistence; the nested
    /// `detect_red_eye_candidates` must not add a second log line (depth guard).
    pub(crate) fn apply_detected_red_eye_objects(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyDetectedRedEyes);
        let candidates = self.detect_red_eye_candidates()?;
        self.apply_detected_red_eyes(&candidates)
    }
}
