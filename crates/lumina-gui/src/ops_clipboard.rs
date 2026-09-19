//! GUI-REFACTOR-W2-20 S2.8: the settings clipboard and the B&W toggle,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::copy_settings`]/[`LuminaApp::paste_settings`] implement the
//! Lightroom copy/paste settings roundtrip against the session clipboard and
//! [`LuminaApp::toggle_black_white`] the `V` treatment with its
//! saturation/vibrance stash; [`LuminaApp::bw_active`] and
//! [`LuminaApp::bw_restore_needs_stash_warning`] are the read-only helpers.
//! Public methods keep `pub`; `bw_restore_needs_stash_warning` is `pub(crate)`
//! because the headless B&W-stash tests call it.

use super::*;
use log::{info, trace, warn};

impl LuminaApp {
    /// Copy the session recipe into the session clipboard (Welle 2, LR-09,
    /// `Cmd/Ctrl+Shift+C`). Session-only — never persisted. Fails loudly
    /// when no image is loaded so an empty copy can never silently succeed.
    pub fn copy_settings(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::CopySettings);
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        info!("GUI interaction: copy_settings");
        self.settings_clipboard = Some(self.recipe.clone());
        self.status = Str::SettingsCopied.t().into();
        Ok(())
    }

    /// Whether the session clipboard holds copied settings (read-only
    /// accessor for headless tests).
    pub fn clipboard_has_settings(&self) -> bool {
        self.settings_clipboard.is_some()
    }

    /// Paste the clipboard recipe onto the active virtual copy (Welle 2,
    /// LR-09, `Cmd/Ctrl+Shift+V`). Applies through the normal save/render
    /// path, so the preview generation bumps and the sidecar persists the
    /// result. Fails loudly on an empty clipboard or without a loaded image —
    /// never a silent no-op.
    pub fn paste_settings(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::PasteSettings);
        let Some(snapshot) = self.settings_clipboard.clone() else {
            return Err(GuiError::Io(Str::ClipboardEmpty.t().to_string()));
        };
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        info!("GUI interaction: paste_settings");
        self.recipe = snapshot;
        self.mark_dirty();
        self.save_sidecar();
        self.render()?;
        self.status = Str::SettingsPasted.t().into();
        Ok(())
    }

    /// Whether the black-&-white treatment (`V`) is active: the recipe carries
    /// `extras["treatment"] = "bw"`. Read-only accessor for badges and
    /// headless tests.
    pub fn bw_active(&self) -> bool {
        self.recipe
            .extras
            .get("treatment")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == TREATMENT_BW)
    }

    /// LRPAR-G01-BASIC (B2): whether exiting B&W needs the stash warning —
    /// true only when the stash is actually missing or unparsable. A clean
    /// stash restores silently (no log noise suggesting corruption). Pure
    /// helper so the condition is unit-testable without a logger.
    pub(crate) fn bw_restore_needs_stash_warning(recipe: &EditRecipe) -> bool {
        recipe.treatment() == TREATMENT_BW
            && recipe.extras.get(BW_STASH_KEY).is_none_or(|stash| {
                serde_json::from_value::<BTreeMap<String, Option<f64>>>(stash.clone()).is_err()
            })
    }

    /// Toggle the Lightroom-style B&W treatment (`V`, Welle 2). Enabling
    /// stashes the current `saturation`/`vibrance` (including absence) in
    /// `extras["bw_stash"]` and sets both to `-1.0` — full desaturation
    /// through the shared pipeline stage, no GUI-side pixel logic.
    /// Disabling restores the stashed values exactly (absent keys are removed
    /// again, never left at `-1`). Persists via [`Self::save_sidecar`] and
    /// re-renders, so the preview generation bumps. Fails loudly without a
    /// loaded image. LRPAR-G01-BASIC: delegates to the shared
    /// [`lumina_sidecar::EditRecipe::apply_treatment`] path (same as
    /// `lumina develop --treatment` and the Treatment selector).
    pub fn toggle_black_white(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::ToggleBlackWhite);
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        let target = if self.bw_active() {
            TREATMENT_COLOR
        } else {
            TREATMENT_BW
        };
        // B2: warn only when the stash is actually missing/corrupt — a clean
        // restore stays silent. (`apply_treatment` itself still falls back to
        // identity loudly-by-contract; this flag is just the log gate.)
        let needs_stash_warning =
            target == TREATMENT_COLOR && Self::bw_restore_needs_stash_warning(&self.recipe);
        self.recipe
            .apply_treatment(target)
            .map_err(GuiError::from)?;
        if needs_stash_warning {
            warn!("B&W stash missing or corrupt; resetting saturation/vibrance to identity");
        }
        trace!(
            "GUI interaction: toggle_black_white -> {}",
            self.bw_active()
        );
        self.mark_dirty();
        self.save_sidecar();
        self.render()?;
        // `render` overwrites the status ("Preview current"); restore the
        // treatment message so the toggle stays visible.
        self.status = if self.bw_active() {
            Str::BlackWhiteOn.t().into()
        } else {
            Str::BlackWhiteOff.t().into()
        };
        Ok(())
    }
}
