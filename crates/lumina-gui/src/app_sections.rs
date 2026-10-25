//! Treatment/profile switching, the per-section Previous/Reset baselines and the
//! reset-sliders policy.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    /// LRPAR-G01-BASIC: set the Develop treatment through the shared sidecar
    /// path (same stash semantics as the `V` toggle and
    /// `lumina develop --treatment`). Only `"color"` and `"bw"` are
    /// accepted — anything else fails loudly. Commits through the normal
    /// save/render path (history, `preview_generation`-bump, `info!`-log).
    /// A no-change call succeeds without saving.
    pub fn set_treatment(&mut self, treatment: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetTreatment);
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if treatment != TREATMENT_COLOR && treatment != TREATMENT_BW {
            return Err(GuiError::Io(Str::InvalidTreatment.t().to_string()));
        }
        let changed = self
            .recipe
            .apply_treatment(treatment)
            .map_err(GuiError::from)?;
        info!("GUI interaction: set_treatment -> {treatment}");
        self.status = Str::TreatmentSetPattern.format_arg(treatment);
        if !changed {
            return Ok(());
        }
        self.mark_recipe_dirty("treatment", f64::from(treatment == TREATMENT_BW));
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// LRPAR-G01-BASIC: set the Develop profile through the shared sidecar
    /// path (same whitelist as `lumina develop --profile`; `"default"`
    /// removes the key). Unknown names fail loudly. MVP renders every known
    /// profile identically (persisted selection intent — see
    /// `feature/architecture/pipeline.md` § G-01). Commits like a slider.
    pub fn set_profile(&mut self, profile: &str) -> Result<(), GuiError> {
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if !DEVELOP_PROFILES.contains(&profile) {
            return Err(GuiError::Io(Str::InvalidProfile.t().to_string()));
        }
        let position = DEVELOP_PROFILES
            .iter()
            .position(|p| *p == profile)
            .unwrap_or(0);
        let changed = self
            .recipe
            .apply_develop_profile(profile)
            .map_err(GuiError::from)?;
        info!("GUI interaction: set_profile -> {profile}");
        self.status = Str::ProfileSetPattern.format_arg(profile);
        if !changed {
            return Ok(());
        }
        self.mark_recipe_dirty("profile", position as f64);
        self.commit_pending_slider_save([0, 0]);
        Ok(())
    }

    /// LRPAR-G01-BASIC: snapshot the current Previous baseline (recipe +
    /// active-copy mask layers). Captured at image load and after every
    /// successful save, so `Previous` always means "last saved state".
    pub(crate) fn capture_section_baselines(&mut self) {
        self.recipe_baseline = Some(self.recipe.clone());
        self.mask_baseline = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| copy.mask_layers.clone())
            .unwrap_or_default();
    }

    /// LRPAR-G01-BASIC: restore one Develop section from the Previous
    /// baseline (panel-local undo to the last saved state — explicitly NOT a
    /// cross-image Previous, which is LRPAR-G08-PREVIOUS). Out-of-range
    /// indices and a missing baseline fail loudly. Commits through the
    /// normal save/render path.
    pub fn restore_section_previous(&mut self, index: usize) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::RestoreSectionPrevious);
        let label = section_name(index)
            .ok_or_else(|| GuiError::Io(format!("unknown develop section {index}")))?;
        let baseline = self
            .recipe_baseline
            .clone()
            .ok_or_else(|| GuiError::Io(Str::SectionPreviousUnavailable.t().to_string()))?;
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if index == SECTION_MASKING {
            let id = self.virtual_copy_id.clone();
            let layers = self.mask_baseline.clone();
            self.arm_mask_state_history("masking.previous")?;
            let document = self
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            copy.mask_layers = layers;
        } else {
            Self::restore_recipe_section(&mut self.recipe, &baseline, index);
        }
        info!("GUI interaction: restore_section_previous {label}");
        self.status = Str::SectionPreviousPattern.format_arg(label);
        self.mark_recipe_dirty("section_previous", index as f64);
        let committed = self.commit_pending_slider_save([0, 0]);
        if !self.path.trim().is_empty() && !committed {
            return Err(GuiError::Io(
                self.error()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "section Previous could not be saved".into()),
            ));
        }
        Ok(())
    }

    /// LRPAR-G01-BASIC: reset one Develop section to its documented defaults
    /// (Basic tone keys to slider defaults, nested blocks to absent, profile
    /// to default, Masking layers of the active copy removed). Treatment is
    /// exited via the stash path (never stranded at `-1`); a Color reset
    /// leaves B&W-owned `-1` values untouched while the treatment is active
    /// (ownership of the toggle). Commits through the normal save/render path.
    pub fn reset_section(&mut self, index: usize) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::ResetSection);
        let label = section_name(index)
            .ok_or_else(|| GuiError::Io(format!("unknown develop section {index}")))?;
        if self.original.is_none() {
            return Err(GuiError::Io(Str::NoImageLoaded.t().to_string()));
        }
        self.ensure_document_loaded()?;
        if index == SECTION_MASKING {
            let id = self.virtual_copy_id.clone();
            self.arm_mask_state_history("masking.reset")?;
            let document = self
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            copy.mask_layers.clear();
        } else {
            Self::reset_recipe_section(&mut self.recipe, index).map_err(GuiError::from)?;
        }
        info!("GUI interaction: reset_section {label}");
        self.status = Str::SectionResetPattern.format_arg(label);
        self.mark_recipe_dirty("section_reset", index as f64);
        let committed = self.commit_pending_slider_save([0, 0]);
        if !self.path.trim().is_empty() && !committed {
            return Err(GuiError::Io(
                self.error()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "section Reset could not be saved".into()),
            ));
        }
        Ok(())
    }

    /// LRPAR-G01-BASIC: copy one section's recipe fields from `src` to `dst`
    /// (exact restore, absence included). Masking is handled by the caller
    /// (layers live on the virtual copy, not the recipe).
    fn restore_recipe_section(dst: &mut EditRecipe, src: &EditRecipe, index: usize) {
        match index {
            SECTION_BASIC => {
                for key in BASIC_TONE_KEYS {
                    match src.adjustments.get(*key) {
                        Some(value) => {
                            dst.adjustments.insert((*key).to_string(), *value);
                        }
                        None => {
                            dst.adjustments.remove(*key);
                        }
                    }
                }
                // Treatment marker + stash travel together (toggle-owned pair).
                for key in [TREATMENT_KEY, BW_STASH_KEY] {
                    match src.extras.get(key) {
                        Some(value) => {
                            dst.extras.insert(key.to_string(), value.clone());
                        }
                        None => {
                            dst.extras.remove(key);
                        }
                    }
                }
                match src.options.get(DEVELOP_PROFILE_KEY) {
                    Some(value) => {
                        dst.options
                            .insert(DEVELOP_PROFILE_KEY.into(), value.clone());
                    }
                    None => {
                        dst.options.remove(DEVELOP_PROFILE_KEY);
                    }
                }
            }
            SECTION_TONE_CURVE => dst.curves.clone_from(&src.curves),
            SECTION_COLOR => {
                dst.hsl.clone_from(&src.hsl);
                dst.point_color.clone_from(&src.point_color);
                dst.color_grading.clone_from(&src.color_grading);
                dst.presence.clone_from(&src.presence);
                for key in ["vibrance", "saturation"] {
                    match src.adjustments.get(key) {
                        Some(value) => {
                            dst.adjustments.insert(key.to_string(), *value);
                        }
                        None => {
                            dst.adjustments.remove(key);
                        }
                    }
                }
            }
            SECTION_DETAIL => {
                dst.sharpening.clone_from(&src.sharpening);
                dst.noise_reduction.clone_from(&src.noise_reduction);
                dst.red_eye.clone_from(&src.red_eye);
            }
            SECTION_EFFECTS => dst.effects.clone_from(&src.effects),
            SECTION_OPTICS => {
                dst.lens_correction.clone_from(&src.lens_correction);
                dst.lens_blur.clone_from(&src.lens_blur);
            }
            SECTION_GEOMETRY => {
                dst.geometry.clone_from(&src.geometry);
                dst.perspective.clone_from(&src.perspective);
                dst.upright.clone_from(&src.upright);
            }
            _ => {}
        }
    }

    /// LRPAR-G01-BASIC: reset one section's recipe fields to documented
    /// defaults (pure helper; Masking and I/O handled by the caller).
    fn reset_recipe_section(
        recipe: &mut EditRecipe,
        index: usize,
    ) -> Result<(), lumina_sidecar::SidecarError> {
        match index {
            SECTION_BASIC => {
                for key in BASIC_TONE_KEYS {
                    recipe
                        .adjustments
                        .insert((*key).to_string(), Self::default_for_adjustment(key));
                }
                // Exit B&W through the stash path (restores pre-B&W values,
                // never strands `-1`); a no-op when already color.
                recipe.apply_treatment(TREATMENT_COLOR)?;
                recipe.options.remove(DEVELOP_PROFILE_KEY);
            }
            SECTION_TONE_CURVE => recipe.curves = None,
            SECTION_COLOR => {
                recipe.hsl = None;
                recipe.point_color = None;
                recipe.color_grading = None;
                recipe.presence = None;
                // B&W-owned `-1` values stay while the treatment is active
                // (ownership of the toggle); otherwise back to identity.
                if recipe.treatment() != TREATMENT_BW {
                    recipe.adjustments.remove("vibrance");
                    recipe.adjustments.remove("saturation");
                }
            }
            SECTION_DETAIL => {
                recipe.sharpening = None;
                recipe.noise_reduction = None;
                // G-14 red-eye lives in this panel; a section reset clears it
                // too (visible recipe edit, no hidden state).
                recipe.red_eye = None;
            }
            SECTION_EFFECTS => recipe.effects = None,
            SECTION_OPTICS => {
                recipe.lens_correction = None;
                recipe.lens_blur = None;
            }
            SECTION_GEOMETRY => {
                recipe.geometry = None;
                recipe.perspective = None;
                // LRPAR-G06-UPRIGHT-15 lives in this panel.
                recipe.upright = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether "Reset Sliders Automatically" is armed (read-only accessor
    /// for the footer checkbox and headless tests).
    pub fn reset_sliders_automatically(&self) -> bool {
        self.reset_sliders_automatically
    }

    /// Set "Reset Sliders Automatically" (LRPAR-G01-BASIC). Persists
    /// folder-inherited in the current folder's `.lumina/settings.json`
    /// (other folder flags are preserved via an effective-settings
    /// read-modify-write); a persistence failure stays visible via
    /// `show_error` instead of silently diverging from disk.
    pub fn set_reset_sliders_automatically(&mut self, enabled: bool) {
        if self.reset_sliders_automatically == enabled {
            return;
        }
        self.reset_sliders_automatically = enabled;
        info!("GUI interaction: set_reset_sliders_automatically -> {enabled}");
        self.status = if enabled {
            Str::ResetSlidersOn.t().into()
        } else {
            Str::ResetSlidersOff.t().into()
        };
        match self.persist_reset_sliders_flag() {
            Ok(folder) => {
                // R2-MODSWITCH-1 F7: the folder settings changed — drop the
                // memoized preview index for that folder so the new effective
                // options take effect (visible invalidation, never a silent
                // stale gate).
                self.thumbnail_cache.invalidate_folder(&folder);
            }
            Err(error) => self.show_error(error),
        }
    }

    /// Write the current flag into the current folder's settings file,
    /// preserving the other inherited flags (read-modify-write over the
    /// effective settings). Returns the folder written to, so the caller can
    /// invalidate its memoized preview index visibly.
    fn persist_reset_sliders_flag(&self) -> Result<PathBuf, GuiError> {
        let folder = if self.path.trim().is_empty() {
            PathBuf::from(self.directory.trim())
        } else {
            Path::new(self.path.trim())
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(self.directory.trim()))
        };
        let cache = DiskFolderCache::in_folder(&folder)
            .map_err(|error| GuiError::Io(format!("cannot open folder settings: {error}")))?;
        let mut settings = cache
            .effective_settings()
            .map_err(|error| GuiError::Io(format!("cannot read folder settings: {error}")))?;
        settings.reset_sliders_automatically = self.reset_sliders_automatically;
        cache
            .save_settings(&settings)
            .map_err(|error| GuiError::Io(format!("cannot save folder settings: {error}")))?;
        Ok(folder)
    }

    /// Refresh the flag from the given folder's inherited settings (called
    /// on folder navigation). Folders without a settings file keep the
    /// current value; unreadable files warn loudly and keep the current
    /// value instead of silently resetting it.
    pub(crate) fn refresh_reset_sliders_flag(&mut self, folder: &Path) {
        if !folder.join(".lumina").join("settings.json").exists() {
            return;
        }
        match DiskFolderCache::in_folder(folder).map_err(|error| error.to_string()) {
            Ok(cache) => match cache.effective_settings() {
                Ok(settings) => {
                    self.reset_sliders_automatically = settings.reset_sliders_automatically;
                }
                Err(error) => {
                    warn!(
                        "cannot read folder settings for {}: {error}",
                        folder.display()
                    );
                }
            },
            Err(error) => {
                warn!(
                    "cannot open folder settings for {}: {error}",
                    folder.display()
                );
            }
        }
    }
}
