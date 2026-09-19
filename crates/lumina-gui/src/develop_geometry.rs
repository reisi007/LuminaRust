//! GUI-REFACTOR-W2-20 S2.2: the Geometry Develop section, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_geometry`] paints crop/straighten/rotate/mirror/upright;
//! [`LuminaApp::aspect_name`] labels the active crop aspect. `draw_geometry` is
//! `pub(crate)` because `DEVELOP_SECTIONS` references it; `aspect_name` stays
//! private to this module.

// UX-LOOK-CROP-18 (UXG-01): the interactive on-canvas crop overlay lives in
// its own file (file-size ratchet) while staying in this Geometry module
// boundary.
pub(crate) mod crop_overlay;

use super::*;

impl LuminaApp {
    pub(crate) fn draw_geometry(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_GEOMETRY];
        let section_header =
            egui::CollapsingHeader::new(Str::Geometry.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_GEOMETRY);
            // G-06 (LRPAR-G06-GEO): crop, straighten, mirrors and manual
            // perspective are pure core-pipeline controls — always
            // available, never behind a Lensfun feature gate (F-093/F-099
            // are core-only models; the N6 finding was "not rotatable /
            // wiring missing or unfindable"). The Lensfun *auto* correction
            // keeps its own status line below.
            ui.label(Str::Crop.t());
            // GUI-SLIDER-SAVE-1: geometry edits commit through the
            // `set_*` setters (save at debounce); locals are only control
            // binding buffers.
            let geo = self.recipe.geometry.clone().unwrap_or(Geometry {
                version: 1,
                crop: None,
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            });
            // Aspect preset selector: Off (full frame), the ten F-093
            // presets, or Custom for a free rectangle (edited below).
            let current_aspect = match &geo.crop {
                None => "off",
                Some(Crop::Aspect { preset }) => Self::aspect_name(preset),
                Some(Crop::Free { .. }) => "custom",
            };
            egui::ComboBox::from_label(Str::Aspect.t())
                .selected_text(current_aspect)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current_aspect == "off", "off")
                        .clicked()
                        && geo.crop.is_some()
                    {
                        // `clear_crop` removes only the rectangle and keeps
                        // rotation/mirrors by design.
                        self.clear_crop();
                    }
                    for name in Self::ASPECT_NAMES {
                        if ui.selectable_label(current_aspect == name, name).clicked()
                            && current_aspect != name
                        {
                            if let Err(error) = self.set_crop_aspect(name) {
                                self.show_error(error);
                            }
                        }
                    }
                });
            // Free rectangle fields (normalized 0..=1). Editing any field
            // commits a free rect through `set_crop_free` (loud on invalid
            // input, rejected at save without a silent clip).
            let mut free = match &geo.crop {
                Some(Crop::Free {
                    x,
                    y,
                    width,
                    height,
                }) => [*x, *y, *width, *height],
                _ => [0.0, 0.0, 1.0, 1.0],
            };
            let mut free_changed = false;
            ui.horizontal(|ui| {
                for (i, label) in ["x", "y", "w", "h"].into_iter().enumerate() {
                    let mut v = free[i];
                    if ui
                        .add(
                            egui::DragValue::new(&mut v)
                                .speed(0.01)
                                .range(0.0..=1.0)
                                .prefix(format!("{label} ")),
                        )
                        .changed()
                    {
                        free[i] = v;
                        free_changed = true;
                    }
                }
            });
            if free_changed {
                if let Err(error) = self.set_crop_free(
                    f64::from(free[0]),
                    f64::from(free[1]),
                    f64::from(free[2]),
                    f64::from(free[3]),
                ) {
                    self.show_error(error);
                }
            }
            if geo.crop.is_some() && ui.button(Str::ClearCrop.t()).clicked() {
                // `clear_crop` removes only the rectangle and keeps
                // rotation/mirrors by design.
                self.clear_crop();
            }
            // Straighten (G-06): documented alias of the rotation field —
            // same setter path as the former Rotation slider.
            let mut straighten = geo.rotation_degrees;
            if matches!(
                lr_slider(
                    ui,
                    Str::Straighten.t(),
                    &mut straighten,
                    identity_spec(-180.0..=180.0, 0.0, 1.0)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_straighten(f64::from(straighten));
            }
            ui.horizontal(|ui| {
                if ui.button(Str::RotateLeft.t()).clicked() {
                    self.rotate_step(-90.0);
                }
                if ui.button(Str::RotateRight.t()).clicked() {
                    self.rotate_step(90.0);
                }
            });
            let mut mh = geo.mirror_horizontal;
            if ui.checkbox(&mut mh, Str::MirrorHorizontal.t()).changed() {
                self.set_geometry_mirror(mh, geo.mirror_vertical);
            }
            let mut mv = geo.mirror_vertical;
            if ui.checkbox(&mut mv, Str::MirrorVertical.t()).changed() {
                // Re-read the horizontal flag: a same-frame horizontal
                // change above already committed through the setter.
                let horizontal = self
                    .recipe
                    .geometry
                    .as_ref()
                    .map(|g| g.mirror_horizontal)
                    .unwrap_or(mh);
                self.set_geometry_mirror(horizontal, mv);
            }
            ui.label(Str::Perspective.t());
            // GUI-SLIDER-SAVE-1: perspective sliders commit through
            // `set_perspective_value` (save at debounce); `persp` is only
            // a slider binding buffer.
            let mut persp = self.recipe.perspective.unwrap_or(Perspective {
                version: 1,
                vertical: 0.0,
                horizontal: 0.0,
                rotation: 0.0,
                scale: 1.0,
                aspect_ratio: 1.0,
                shift_x: 0.0,
                shift_y: 0.0,
            });
            for (field, label, spec) in [
                (
                    &mut persp.vertical,
                    Str::Vertical,
                    percent_spec(-1.0..=1.0, 0.0),
                ),
                (
                    &mut persp.horizontal,
                    Str::Horizontal,
                    percent_spec(-1.0..=1.0, 0.0),
                ),
                (
                    &mut persp.rotation,
                    Str::Rotation,
                    percent_spec(-1.0..=1.0, 0.0),
                ),
                (
                    &mut persp.scale,
                    Str::Scale,
                    identity_spec(0.1..=10.0, 1.0, 0.01),
                ),
                (
                    &mut persp.aspect_ratio,
                    Str::AspectRatio,
                    identity_spec(0.1..=10.0, 1.0, 0.01),
                ),
                (
                    &mut persp.shift_x,
                    Str::ShiftX,
                    percent_spec(-1.0..=1.0, 0.0),
                ),
                (
                    &mut persp.shift_y,
                    Str::ShiftY,
                    percent_spec(-1.0..=1.0, 0.0),
                ),
            ] {
                let mut v = *field;
                if matches!(
                    lr_slider(ui, label.t(), &mut v, spec),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    let name = match label {
                        Str::Vertical => "vertical",
                        Str::Horizontal => "horizontal",
                        Str::Rotation => "rotation",
                        Str::Scale => "scale",
                        Str::AspectRatio => "aspect_ratio",
                        Str::ShiftX => "shift_x",
                        Str::ShiftY => "shift_y",
                        _ => continue,
                    };
                    self.set_perspective_value(name, f64::from(v));
                }
            }
            // LRPAR-G06-UPRIGHT-15: persisted automatic upright analysis. The
            // suggestion is computed on demand (classic, model-free) and stored
            // with a source fingerprint; the status line reports `fresh`/`stale`
            // visibly (never a silent recompute). "Apply analysis" toggles
            // whether it supplies the effective perspective — the manual
            // sliders above stay persisted and return when disabled.
            let status = self.upright_status();
            ui.label(Str::UprightStatusPattern.format_arg(match status {
                "fresh" => Str::UprightFresh.t(),
                "stale" => Str::UprightStale.t(),
                _ => Str::UprightNone.t(),
            }));
            if ui
                .button(Str::UprightAnalyze.t())
                .on_hover_text(Str::UprightHint.t())
                .clicked()
            {
                if let Err(error) = self.analyze_upright_now() {
                    self.show_error(error);
                }
            }
            let mut enabled = self
                .recipe
                .upright
                .as_ref()
                .is_some_and(|stage| stage.enabled);
            if ui.checkbox(&mut enabled, Str::UprightEnable.t()).changed() {
                if let Err(error) = self.set_upright_enabled(enabled) {
                    self.show_error(error);
                }
            }
            if self.recipe.upright.is_some() && ui.button(Str::UprightClear.t()).clicked() {
                self.clear_upright();
            }
            // Lensfun auto status (G-06): always visible (EXIF snapshot or
            // the missing-capability reason), never implied.
            ui.label(Str::LensfunAutoPattern.format_arg(&self.lensfun_auto_status_text()));
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_GEOMETRY, !section_was_open);
        }
    }

    /// Canonical aspect preset names in F-093 order (G-06 crop selector;
    /// same vocabulary as the CLI `--set-crop-aspect`).
    const ASPECT_NAMES: [&'static str; 10] = [
        "original", "1:1", "4:5", "5:4", "3:2", "2:3", "4:3", "3:4", "16:9", "9:16",
    ];

    /// Canonical name of one aspect preset (matches [`Self::ASPECT_NAMES`]).
    fn aspect_name(preset: &AspectPreset) -> &'static str {
        match preset {
            AspectPreset::Original => "original",
            AspectPreset::OneToOne => "1:1",
            AspectPreset::FourToFive => "4:5",
            AspectPreset::FiveToFour => "5:4",
            AspectPreset::ThreeToTwo => "3:2",
            AspectPreset::TwoToThree => "2:3",
            AspectPreset::FourToThree => "4:3",
            AspectPreset::ThreeToFour => "3:4",
            AspectPreset::SixteenToNine => "16:9",
            AspectPreset::NineToSixteen => "9:16",
        }
    }
}
