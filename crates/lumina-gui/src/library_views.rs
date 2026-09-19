//! GUI-REFACTOR-W2-20 S2.3: the Library loupe/compare/survey views,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_library_loupe`], [`LuminaApp::draw_library_compare`] and
//! [`LuminaApp::draw_library_survey`] render the single/multi-image Library
//! views selected by `draw_library_grid`. Pure display over the existing
//! indices; all three are `pub(crate)` because the grid module calls them.

use super::*;

impl LuminaApp {
    /// G-09 (LRPAR-G09-LIB) Loupe: the active selection shown large
    /// (Lightroom `E`). Single image, same badges/hover as the grid cells,
    /// no second render path — the filmstrip thumbnail texture is reused.
    /// Display-only; edits stay on the rating keys and Quick Develop.
    pub(crate) fn draw_library_loupe(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        raw_indices: &[usize],
    ) {
        // UX-SLICE-2 (F3): the empty listing is handled once, before the view
        // branch in `draw_library_grid` — no per-view duplicate empty state.
        let active = raw_indices
            .iter()
            .find(|&&index| self.entries[index].path.display().to_string() == self.path)
            .or(raw_indices.first())
            .copied()
            .unwrap_or(0);
        let entry = self.entries[active].clone();
        ui.heading(Str::LoupeOn.t());
        ui.label(format!("{}  [{}]", entry.name, entry.status_label()));
        // B2: the image height is derived from the remaining center space
        // (minus the rating line below) instead of a fixed 420px: a fixed
        // height overflows short viewports and the rating line ends up
        // painted underneath the bottom filmstrip panel (invisible but still
        // in the accesskit tree — a vacuous guard). Clamped so absurdly
        // short windows still paint something.
        let height = (ui.available_height() - 30.0).clamp(64.0, 600.0);
        let size = egui::vec2(ui.available_width().max(64.0), height);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        if let Some(texture) = self.thumbnails.get(&entry.thumb_key).cloned() {
            ui.put(
                rect,
                egui::Image::from_texture(&texture).max_size(rect.size()),
            );
        } else {
            ui.painter()
                .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
            ui.put(
                rect,
                egui::Label::new(self.thumbnail_placeholder_label(&entry)),
            );
            self.frame_thumb_enqueued +=
                self.ensure_thumbnail_priority(ctx, raw_indices, 0..raw_indices.len().min(4));
        }
        ui.label(format!(
            "{}:{} {}:{} {}:{}",
            Str::Rating.t(),
            stars_for_rating(entry.rating),
            Str::FlagLabel.t(),
            flag_label(entry.flag),
            Str::ColorLabel.t(),
            color_label_name(entry.color_label),
        ));
    }

    /// G-09 (LRPAR-G09-LIB) Compare: Before/After of the active image
    /// (Lightroom `C`). Reuses the existing `before_after` proxy — the full
    /// Before/After toggle stays `Y` in Develop; this view only surfaces the
    /// same proxy state in the Library module. Display-only, never recipe.
    pub(crate) fn draw_library_compare(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        raw_indices: &[usize],
    ) {
        // UX-SLICE-2 (F3): the empty listing is handled once, before the view
        // branch in `draw_library_grid` — no per-view duplicate empty state.
        let active = raw_indices
            .iter()
            .find(|&&index| self.entries[index].path.display().to_string() == self.path)
            .or(raw_indices.first())
            .copied()
            .unwrap_or(0);
        let entry = self.entries[active].clone();
        ui.heading(Str::CompareModeCompare.t());
        ui.label(format!(
            "{}  ({})",
            entry.name,
            if self.before_after {
                Str::CompareOnPattern.format_arg(Str::CompareModeCompare.t())
            } else {
                Str::CompareOff.t().to_string()
            }
        ));
        ui.horizontal(|ui| {
            let left = entry.clone();
            for (title, _active_before) in [
                (Str::CompareBefore.t(), true),
                (Str::CompareAfter.t(), false),
            ] {
                ui.vertical(|ui| {
                    ui.label(title);
                    let size = egui::vec2((ui.available_width() / 2.0).max(64.0), 360.0);
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    if let Some(texture) = self.thumbnails.get(&left.thumb_key).cloned() {
                        ui.put(
                            rect,
                            egui::Image::from_texture(&texture).max_size(rect.size()),
                        );
                    } else {
                        ui.painter()
                            .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                        ui.put(
                            rect,
                            egui::Label::new(self.thumbnail_placeholder_label(&left)),
                        );
                    }
                });
            }
        });
        self.frame_thumb_enqueued +=
            self.ensure_thumbnail_priority(ctx, raw_indices, 0..raw_indices.len().min(4));
    }

    /// G-09 (LRPAR-G09-LIB) Survey: the multi-selection side by side
    /// (Lightroom `N`). Below two selected images the view falls back to the
    /// filtered raster so the pane never goes empty while images exist.
    /// Click selects, double-click opens in Loupe (same bookkeeping as the
    /// grid). Display-only, never recipe.
    pub(crate) fn draw_library_survey(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        raw_indices: &[usize],
    ) {
        // UX-SLICE-2 (F3): the empty listing is handled once, before the view
        // branch in `draw_library_grid` — no per-view duplicate empty state.
        let selected: Vec<usize> = raw_indices
            .iter()
            .copied()
            .filter(|&index| {
                self.filmstrip_selection
                    .contains(&self.entries[index].path.display().to_string())
            })
            .collect();
        let show: Vec<usize> = if selected.len() >= 2 {
            selected
        } else {
            raw_indices.to_vec()
        };
        ui.heading(Str::CompareModeSurvey.t());
        ui.label(
            Str::SelectionCountPattern.format_arg(&self.filmstrip_selection.len().to_string()),
        );
        let thumb = (self.library_thumb_size * 1.5).clamp(108.0, 360.0);
        let cols = ((ui.available_width() / thumb).floor() as usize).max(1);
        self.library_cols = cols;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for chunk in show.chunks(cols) {
                ui.horizontal(|ui| {
                    for &entry_idx in chunk {
                        let entry = self.entries[entry_idx].clone();
                        let selected = self
                            .filmstrip_selection
                            .contains(&entry.path.display().to_string());
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(thumb, thumb), egui::Sense::click());
                        if selected {
                            ui.painter().rect_stroke(
                                rect.expand(2.0),
                                3.0,
                                egui::Stroke::new(2.0_f32, ui.visuals().selection.bg_fill),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if let Some(texture) = self.thumbnails.get(&entry.thumb_key).cloned() {
                            ui.put(
                                rect,
                                egui::Image::from_texture(&texture).max_size(rect.size()),
                            );
                        } else {
                            ui.painter()
                                .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                            ui.put(
                                rect,
                                egui::Label::new(self.thumbnail_placeholder_label(&entry)),
                            );
                        }
                        ui.vertical(|ui| {
                            ui.label(entry.name.clone());
                        });
                        if resp.clicked() {
                            self.select_filmstrip_path(
                                entry.path.display().to_string(),
                                false,
                                false,
                            );
                        }
                        if resp.double_clicked() {
                            self.handle_filmstrip_click(
                                entry.path.display().to_string(),
                                false,
                                false,
                            );
                            self.set_library_view(LibraryView::Loupe);
                        }
                    }
                });
            }
        });
        self.frame_thumb_enqueued +=
            self.ensure_thumbnail_priority(ctx, &show, 0..show.len().min(8));
    }
}
