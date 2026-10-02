//! G-16 auto tone end points (`Shift`+double-click on `whites`/`blacks`).
//!
//! AUTO-TONE-CLI-6 clause (2) and AUTO-TONE-ENDPOINT-MIXED-7: the end point is
//! a **user override**, never a partial Auto-Tone state. On an existing
//! Auto-Tone state it is completed to the full six-mirror contract through the
//! shared writer ([`apply_auto_tone_result`]), with itself (and every other
//! overriding slider) carried through the shared override helper
//! [`auto_tone_overrides`]. On a fresh recipe it stays a pure override and
//! creates no Auto-Tone state at all.
//!
//! Extracted from `lib.rs` (RATCHET: `lib.rs` is a baselined 12.5k-line file
//! and must not grow). The public method is unchanged; only its home moved.

use super::*;

impl LuminaApp {
    /// The Auto-Tone analysis input for the **active** recipe (post SourceActions
    /// + post Crop, without Adjustments).
    ///
    /// AUTO-TONE-ANALYSIS-INPUT-8: built from the shared production seam
    /// (`lumina_stages::auto_tone::auto_analysis_frame`) that the CLI and
    /// `regenerate` use, so the GUI and the CLI cannot measure different
    /// domains. Loud without a loaded image.
    pub(crate) fn auto_analysis_frame(&self) -> Result<ImageFrame, GuiError> {
        self.auto_analysis_frame_for(&self.recipe)
    }

    /// [`Self::auto_analysis_frame`] for an **explicit** recipe — the reload
    /// path validates a *candidate* recipe that is not adopted yet, so binding to
    /// `self.recipe` would measure the wrong domain.
    ///
    /// The source actions come from [`Self::resolve_current_source_actions`],
    /// which takes the recipe as a parameter. There is deliberately **no**
    /// second resolver here: a copy of its four guards would be a second
    /// implementation whose bundle-loading branch no test reaches, while the
    /// original's is covered (`src/tests/source_actions.rs`).
    pub(crate) fn auto_analysis_frame_for(
        &self,
        recipe: &EditRecipe,
    ) -> Result<ImageFrame, GuiError> {
        let Some(source) = &self.original else {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        };
        let resolved = self
            .resolve_current_source_actions(recipe, source)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        Ok(lumina_stages::auto_tone::auto_analysis_frame(
            source,
            recipe,
            resolved.artifacts(),
        )?)
    }

    /// Shared `suggest_auto_tone` evaluation over the Auto-Tone analysis frame
    /// (G-16), used by [`Self::apply_auto_endpoint`] (one end point).
    /// [`Self::auto_tone`] no longer comes here — it goes through the shared
    /// writer, which evaluates and persists in one step. Loud without a loaded
    /// image — never a silent no-op.
    fn compute_auto_tone(&self) -> Result<AutoToneResult, GuiError> {
        let frame = self.auto_analysis_frame()?;
        let config = AutoToneConfig {
            target_luminance: self.recipe.auto_features.target_luminance,
            ..Default::default()
        };
        Ok(suggest_auto_tone(&frame, config)?)
    }

    /// Apply one auto end point (G-16, `Shift`+double-click on the
    /// `whites`/`blacks` label): evaluates the shared auto-tone path and
    /// persists exactly that one field as a **user override** through the
    /// normal save/render commit. Loud without a loaded image.
    ///
    /// AUTO-TONE-ENDPOINT-MIXED-7: on an **existing** Auto-Tone state the end
    /// point is completed to the full six-mirror contract through the shared
    /// writer, with itself carried as an override (its effective value is the
    /// end point, its mirror documents the recomputed auto value). No mixed
    /// state remains. On a **fresh** recipe no Auto-Tone state is created (a
    /// pure override, 0 of 6) — that is "all six or none" too, and it is not a
    /// mixed state. The override set comes from one shared helper
    /// ([`auto_tone_overrides`]), the same one the collective repair run uses;
    /// there is no second algorithm.
    pub fn apply_auto_endpoint(&mut self, endpoint: AutoEndpoint) -> Result<(), GuiError> {
        let result = self.compute_auto_tone()?;
        let (key, value) = match endpoint {
            AutoEndpoint::White => ("whites", result.whites),
            AutoEndpoint::Black => ("blacks", result.blacks),
        };
        // The end point claims this one slider for the user: its effective value
        // is the end point, never the auto value. On a fresh recipe this is the
        // whole effect (no Auto-Tone state exists yet).
        self.recipe.adjustments.insert(key.into(), value);
        if self.recipe.auto_features.enable_auto_tone {
            self.complete_auto_tone_after_override()?;
        }
        info!("GUI interaction: apply_auto_endpoint {key}={value}");
        self.status = Str::AutoEndpointAppliedPattern.format_arg(key);
        // Same commit discipline as `auto_tone` (GUI-AUTOTONE-SAVE-1 /
        // GUI-SIDECAR-READ-1): record + synchronously persist (CAS, loud
        // conflicts) instead of stranding the save on a later edit.
        self.mark_recipe_dirty("auto_endpoint", value);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// Completes an existing Auto-Tone state to the full six-mirror contract
    /// after a slider was claimed for the user. The end point and every other
    /// key whose mirror does not document its effective value (a hand value set
    /// after a previous end point or after the Auto-Tone run) is preserved
    /// through the shared override helper; the writer fills all six mirrors with
    /// the recomputed auto values, so the state is 6 of 6 and stays fresh — no
    /// mixed state. Caller guarantees `enable_auto_tone` and a decoded frame.
    fn complete_auto_tone_after_override(&mut self) -> Result<(), GuiError> {
        let frame = self.auto_analysis_frame()?;
        let target_luminance = self.recipe.auto_features.target_luminance;
        let overrides = lumina_stages::auto_tone::auto_tone_overrides(&self.recipe);
        lumina_stages::auto_tone::apply_auto_tone_result(
            &mut self.recipe,
            &frame,
            target_luminance,
            lumina_stages::auto_tone::PersistedAutoTone::AlwaysRecompute,
            Some(&overrides),
        )?;
        Ok(())
    }
}
