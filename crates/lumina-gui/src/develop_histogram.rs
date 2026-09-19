//! GUI-REFACTOR-W2-20 S2.2: the Develop histogram section, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::current_analysis`]/[`LuminaApp::current_histogram`] expose the
//! displayed render's tone analysis and 256-bin luminance histogram,
//! [`LuminaApp::histogram_plot_points`] maps bins to plot coordinates and
//! [`LuminaApp::draw_histogram_section`]/[`LuminaApp::draw_histogram`] paint the
//! collapsible Develop histogram graphic. Pure display/read-only accessors — no
//! recipe/sidecar writes. No behaviour changes: every readout, colour and trace
//! is byte-identical to the inlined sequence this extraction replaces.
//!
//! All five are `pub(crate)`: `develop_scroll_content` and the headless
//! histogram tests call them.

use super::*;

impl LuminaApp {
    /// Histogram height in screen points (GUI-HISTOGRAM-1).
    const HISTOGRAM_HEIGHT: f32 = 72.0;

    /// Histogram of the *currently displayed* render state (original while
    /// Before/After is held or the G-10 "Show original" switch is armed,
    /// otherwise the last preview).
    pub(crate) fn current_analysis(&self) -> Option<lumina_core::ToneAnalysis> {
        if self.before_after || self.show_original_histogram {
            self.original.as_ref().map(analyze_tone)
        } else {
            self.tone_analysis
        }
    }

    /// 256-bin luminance histogram matching [`Self::current_analysis`]: computed
    /// on the fly from the original while Before/After is held or the G-10
    /// "Show original" switch is armed, otherwise the stored preview
    /// histogram (GUI-HISTOGRAM-1).
    pub(crate) fn current_histogram(&self) -> Option<LuminanceHistogram> {
        if self.before_after || self.show_original_histogram {
            self.original.as_ref().map(LuminanceHistogram::new)
        } else {
            self.preview_histogram.clone()
        }
    }

    /// Map 256 histogram bins onto plot points inside `rect` (GUI-HISTOGRAM-1).
    /// Pure helper so headless tests can pin the bins→plot mapping without a
    /// laid-out UI. Always returns one point per bin (baseline at the bottom
    /// edge when the histogram is empty), so callers can rely on non-emptiness
    /// whenever bins are present.
    pub(crate) fn histogram_plot_points(bins: &[u64], rect: egui::Rect) -> Vec<egui::Pos2> {
        let n = bins.len().max(1) as f32;
        let max = bins.iter().copied().max().unwrap_or(0).max(1) as f32;
        bins.iter()
            .enumerate()
            .map(|(i, &count)| {
                let x = rect.left() + rect.width() * (i as f32 + 0.5) / n;
                let y = rect.bottom() - rect.height() * (count as f32 / max);
                egui::pos2(x, y)
            })
            .collect()
    }

    /// Own collapsible histogram section (GUI-HISTOGRAM-1): default open,
    /// rendered at the top of the Develop panel instead of the module bar.
    /// Hosts the G-10 "Show original" compare switch and the `S`-softproof
    /// mouse switch — both route through the single `toggle_*` mutation
    /// paths so the `info!` log and status fire exactly once per user flip.
    pub(crate) fn draw_histogram_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(Str::Histogram.t())
            .default_open(true)
            .show(ui, |ui| {
                let mut show_original = self.show_original_histogram;
                ui.checkbox(&mut show_original, Str::HistogramShowOriginal.t());
                if show_original != self.show_original_histogram {
                    self.toggle_original_histogram();
                }
                let mut softproof = self.softproof_preview;
                ui.checkbox(&mut softproof, Str::SoftproofToggle.t());
                if softproof != self.softproof_preview {
                    self.toggle_softproof_preview();
                }
                self.draw_histogram(ui);
            });
    }

    pub(crate) fn draw_histogram(&self, ui: &mut egui::Ui) {
        // REVIEW-GUI-N5: a draft preview's histogram is measured from the
        // low-resolution drag render — it must say so instead of posing as
        // the final render state.
        if self.preview_is_draft {
            ui.colored_label(egui::Color32::YELLOW, Str::HistogramDraft.t());
        }
        let Some(analysis) = self.current_analysis() else {
            ui.label(Str::NotCurrent.t());
            return;
        };
        ui.label(format!(
            "Mean {:.3}  Median {:.3}",
            analysis.mean, analysis.median
        ));
        // GUI-DEBUG-SWEEP-1: the internal sample count is no longer painted;
        // the histogram statistics above are the end-user values.
        ui.label(format!("P01 {:.3}  P99 {:.3}", analysis.p01, analysis.p99));
        // G-10 "Original Photo" compare: while the switch is armed the numbers
        // and curve above already describe the unedited decode; the badge says
        // so and the delta line quantifies edited-vs-original drift.
        // LRPAR-G01-BASIC: the edited reference line keeps the current render
        // visible next to the original (both sides from real analysis values).
        if self.show_original_histogram {
            ui.colored_label(egui::Color32::YELLOW, Str::HistogramOriginalBadge.t());
            match self.histogram_delta() {
                Some((mean_delta, l1)) => {
                    let text = Str::HistogramDeltaPattern
                        .t()
                        .replacen("{}", &format!("{:+.3}", mean_delta), 1)
                        .replacen("{}", &format!("{:.3}", l1), 1);
                    ui.label(text);
                    if let Some(edited) = self.tone_analysis {
                        let edited_text = Str::HistogramEditedPattern
                            .t()
                            .replacen("{}", &format!("{:.3}", edited.mean), 1)
                            .replacen("{}", &format!("{:+.3}", mean_delta), 1)
                            .replacen("{}", &format!("{:.3}", l1), 1);
                        ui.label(edited_text);
                    }
                }
                None => {
                    ui.label(Str::NotCurrent.t());
                }
            }
        }
        let Some(histogram) = self.current_histogram() else {
            ui.label(Str::NotCurrent.t());
            return;
        };
        let width = ui.available_width().max(40.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width, Self::HISTOGRAM_HEIGHT),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(35));
        // Filled luminance bars in the theme accent (plain Painter rects).
        let n = histogram.bins.len().max(1) as f32;
        let max = histogram.bins.iter().copied().max().unwrap_or(0).max(1) as f32;
        for (i, &count) in histogram.bins.iter().enumerate() {
            let x0 = rect.left() + rect.width() * i as f32 / n;
            let x1 = rect.left() + rect.width() * (i + 1) as f32 / n;
            let bar_h = rect.height() * (count as f32 / max);
            if bar_h > 0.5 {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, rect.bottom() - bar_h),
                        egui::pos2(x1.max(x0 + 0.5), rect.bottom()),
                    ),
                    0.0,
                    crate::theme::ACCENT,
                );
            }
        }
        // Curve stroke over the bars for readability.
        let points = Self::histogram_plot_points(&histogram.bins, rect);
        if points.len() >= 2 {
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(1.0_f32, egui::Color32::WHITE),
            ));
        }
        // P01/P99 as slim marker lines.
        for (value, color) in [
            (analysis.p01, egui::Color32::WHITE),
            (analysis.p99, egui::Color32::YELLOW),
        ] {
            let x = rect.left() + rect.width() * value.clamp(0.0, 1.0) as f32;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0_f32, color),
            );
        }
    }
}
