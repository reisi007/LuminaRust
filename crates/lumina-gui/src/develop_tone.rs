//! GUI-REFACTOR-W2-20 S2.2: the Tone Curve Develop section, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_tone_curve`] paints the per-channel region sliders and the
//! interactive point-curve graph. The curve math (`tone_curve_regions`,
//! `build_tone_curve_points`, …) stays at the crate root because the headless
//! tests and recipe helpers share it. `pub(crate)` because `DEVELOP_SECTIONS`
//! references it.

// UX-LOOK-TONECURVE-18: the interactive point-curve graph (new logic in its own
// file per the file-size ratchet) while staying in this module boundary.
pub(crate) mod tone_curve_graph;

use super::*;
use log::{info, warn};

impl LuminaApp {
    pub(crate) fn draw_tone_curve(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_TONE_CURVE];
        let section_header =
            egui::CollapsingHeader::new(Str::ToneCurve.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_TONE_CURVE);
            // G-02: channel selector (Master/Red/Green/Blue). Display-only
            // session state; the parametric sliders and the point editor
            // below bind to the selected channel.
            ui.label(Str::ToneCurveChannel.t());
            let channels = [
                (0usize, Str::ToneCurveChannelMaster),
                (1, Str::ToneCurveChannelRed),
                (2, Str::ToneCurveChannelGreen),
                (3, Str::ToneCurveChannelBlue),
            ];
            let mut selected = self.tone_curve_channel;
            ui.horizontal(|ui| {
                for (index, label) in channels {
                    if ui.selectable_label(selected == index, label.t()).clicked() {
                        selected = index;
                    }
                }
            });
            if selected != self.tone_curve_channel {
                self.tone_curve_channel = selected;
                info!("GUI interaction: tone_curve_channel {selected}");
            }
            let channel = match self.tone_curve_channel {
                1 => "red",
                2 => "green",
                3 => "blue",
                _ => "master",
            };
            ui.label(Str::CurveRegions.t());
            let (mut s, mut d, mut l, mut h) = tone_curve_channel_regions(&self.recipe, channel);
            let spec = percent_spec(-1.0..=1.0, 0.0);
            // GUI-SLIDER-SAVE-1: each region slider commits through
            // `set_tone_curve_channel_region` (save at debounce); the locals
            // are only slider binding buffers.
            for (val, label) in [
                (&mut s, Str::ToneCurveShadows),
                (&mut d, Str::ToneCurveDarks),
                (&mut l, Str::ToneCurveLights),
                (&mut h, Str::ToneCurveHighlights),
            ] {
                if matches!(
                    lr_slider(ui, label.t(), val, spec),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    let region = match label {
                        Str::ToneCurveShadows => "shadows",
                        Str::ToneCurveDarks => "darks",
                        Str::ToneCurveLights => "lights",
                        Str::ToneCurveHighlights => "highlights",
                        _ => continue,
                    };
                    self.set_tone_curve_channel_region(channel, region, *val);
                }
            }
            // UX-LOOK-TONECURVE-18: interactive point-curve graph. A click on
            // the curve adds a control point, a drag moves it, a double-click
            // removes it (see `tone_curve_graph`). Edits reuse the existing
            // `curves` recipe block — no schema change, no migration.
            ui.separator();
            ui.label(Str::ToneCurvePoints.t());
            self.draw_tone_curve_graph(ui, channel);
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_TONE_CURVE, !section_was_open);
        }
    }

    /// Set one tone-curve region delta (`shadows`, `darks`, `lights`,
    /// `highlights`) and record the save commit (GUI-SLIDER-SAVE-1). Unknown
    /// region names are ignored loudly (`warn!`) — all call sites pass
    /// literals, and the headless save tests pin every valid name.
    /// Master-channel shorthand over [`Self::set_tone_curve_channel_region`]
    /// (test- and compat-owned; the panel binds the channel variant).
    #[cfg(test)]
    pub(crate) fn set_tone_curve_region(&mut self, region: &str, value: f64) {
        self.set_tone_curve_channel_region("master", region, value);
    }

    /// Set one parametric tone-curve region (`shadows`/`darks`/`lights`/
    /// `highlights`) of one channel (`master`/`red`/`green`/`blue`,
    /// G-02) and record the save commit (GUI-SLIDER-SAVE-1). The four
    /// region values persist as that channel's 4-point curve (same mapping
    /// as the master path); setting a region replaces a free point list
    /// (Last-Write-Wins je Kanal). Unknown names are ignored loudly — all
    /// call sites pass literals, and the headless save tests pin every
    /// valid name.
    ///
    /// Crash-Fix Runde 2 (F6): the built point list is validated BEFORE it is
    /// written to `self.recipe.curves`. The parametric mapping puts the shadows
    /// delta on the mandatory `(0,0)` endpoint, so a positive `shadows` value
    /// would yield a curve that every subsequent render rejects (GPU and CPU)
    /// — a per-frame error storm that the deduped error path only contains.
    /// Such a region is refused loudly (status + `warn!`), never clipped
    /// silently, and the recipe stays untouched.
    pub(crate) fn set_tone_curve_channel_region(
        &mut self,
        channel: &str,
        region: &str,
        value: f64,
    ) {
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("set_tone_curve_channel_region: unknown channel {channel}");
            return;
        }
        let (mut s, mut d, mut l, mut h) = tone_curve_channel_regions(&self.recipe, channel);
        match region {
            "shadows" => s = value,
            "darks" => d = value,
            "lights" => l = value,
            "highlights" => h = value,
            _ => {
                warn!("set_tone_curve_channel_region: unknown region {region}");
                return;
            }
        }
        let mut curves = self.recipe.curves.clone().unwrap_or(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels::default(),
        });
        curves.version = 1;
        let points = build_tone_curve_points(s, d, l, h);
        // Crash-Fix Runde 2 (F6): validate before the write. `points` is a
        // local candidate, so a refusal leaves `self.recipe.curves` untouched.
        if let Some(reason) = Self::validate_curve_points(&points) {
            self.status = Str::ToneCurveInvalidPattern.format_arg(&reason);
            warn!("set_tone_curve_channel_region: {channel}.{region}={value} refused ({reason})");
            return;
        }
        match channel {
            "master" => curves.master = points,
            "red" => curves.channels.red = Some(points),
            "green" => curves.channels.green = Some(points),
            "blue" => curves.channels.blue = Some(points),
            _ => unreachable!(),
        }
        self.recipe.curves = Some(curves);
        self.mark_recipe_dirty(&format!("curves.{channel}.{region}"), value);
        // REVIEW-GUI-CURVE-1: a clamped output absorbs part of a delta, so the
        // affected slider visibly snaps back. Surface that MVP limit explicitly
        // instead of leaving the user with a silently moving slider.
        if tone_curve_roundtrip_is_lossy(s, d, l, h) {
            self.status = "Tone curve: extreme region values are clamped to the 0..=1 output range (MVP limit) — negative Shadows beyond the base point are not representable.".into();
        }
    }
}
