//! UX-LOOK-CROP-18b (UXG-01 Runde 2): crop-tool display recipe.
//!
//! The interactive crop session authors its rectangle in **full-frame** source
//! coordinates. When the recipe already carries a committed crop, rendering it
//! verbatim would bake that crop into the preview texture — the reference image
//! shrinks, so a later re-edit could only shrink further and never grow back to
//! the full frame (the Runde-2 finding: "nach Commit kann nicht mehr
//! vergrößern").
//!
//! While crop mode is armed the preview must therefore show the full frame.
//! [`LuminaApp::crop_mode_display_recipe`] returns a display-only copy of the
//! recipe with the whole `geometry` stage removed (crop, rotation, mirror). The
//! committed recipe itself is never touched — `Enter` still writes through the
//! regular setters, `Esc` is a no-op.
//!
//! Why the whole geometry stage and not just the crop rectangle: the core
//! pipeline applies `crop → rotate → mirror` (`lumina_core::apply_crop_stage`),
//! so the stored crop rectangle lives in the *unrotated, unmirrored* source
//! frame. Leaving rotation/mirror in the crop-mode preview would rotate/flip the
//! rendered frame while the overlay still maps the rectangle into the unrotated
//! frame — the handles and the darkening would drift off the content. Authoring
//! against the neutral full frame keeps the rectangle exact; the rotation angle
//! is shown on the crop bar and lands in the preview once the tool is left.
//!
//! This is a documented display decision, never a silent fallback: the recipe,
//! the sidecar and the render-key identity of a committed edit are unchanged.

use super::*;

impl LuminaApp {
    /// Display-only recipe for the armed crop tool: the committed `geometry`
    /// stage (crop rectangle, rotation, mirror) is removed so the preview shows
    /// the full corrected frame and a committed crop stays re-editable
    /// (shrink **and** grow back to full frame).
    ///
    /// Lens/perspective corrections are kept — the core crop stage runs after
    /// them, so the full frame the user authors against is exactly the frame the
    /// stored normalized rectangle refers to. The returned recipe is never
    /// written back; it only feeds the crop-mode preview render.
    pub(crate) fn crop_mode_display_recipe(&self) -> EditRecipe {
        let mut recipe = self.recipe.clone();
        recipe.geometry = None;
        recipe
    }
}
