//! GUI-REFACTOR-W2-20 S2.3: the Library grid view and empty state,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_library_grid`] paints the filtered/sorted thumbnail grid
//! (thumbnail scheduling, selection, badges, sidecar/copy index) and delegates
//! to the loupe/compare/survey views; [`LuminaApp::draw_library_empty_state`]
//! is the empty-library placeholder. No behaviour changes: scheduling,
//! filter order and `trace!`s are byte-identical. The grid is `pub(crate)`
//! because the app root and headless tests call it.

use super::*;
use crate::library_sort::sort_label;
use log::trace;

impl LuminaApp {
    /// R3-LOG-1 (MITTEL-1): a Library-grid double-click opens the entry in
    /// Develop — the module switch goes through [`Self::set_module`] so the
    /// switch event/first-paint timing is recorded (the direct `active_module`
    /// assignment used to be invisible to the trace log). Behaviour unchanged:
    /// same selection/open path, same target module.
    pub(crate) fn open_grid_entry_in_develop(&mut self, path: String) {
        self.handle_filmstrip_click(path, false, false);
        self.set_module(Module::Develop);
    }

    /// UX-SLICE-2 (F3): the single Library empty state, shared by Grid, Loupe,
    /// Compare and Survey. Deterministic painted icon, honest body text and
    /// the F2 "Open Folder" CTA (native folder picker via [`Self::open_folder`]).
    fn draw_library_empty_state(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(48.0);
            paint_library_empty_icon(ui);
            ui.add_space(8.0);
            ui.label(egui::RichText::new(Str::LibraryEmptyTitle.t()).heading());
            ui.label(Str::ReadyForImage.t());
            ui.add_space(8.0);
            if ui.button(Str::OpenFolder.t()).clicked() {
                self.open_folder();
            }
        });
    }

    /// Lightroom-like Library grid view (center): RAW files of the current
    /// directory rendered through the shared ThumbnailManager pipeline (no
    /// duplicate generation). Double-click opens a file and switches to
    /// Develop (Loupe). The thumbnail cell size is user-adjustable via a
    /// toolbar slider (Lightroom "Grid" thumbnails).
    pub(crate) fn draw_library_grid(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // Toolbar: a thumbnail-size slider (Lightroom-like). Small/simple stub
        // for now — it drives the cell size of the grid below.
        ui.horizontal(|ui| {
            ui.label(Str::LibraryThumbSize.t());
            let mut size = self.library_thumb_size;
            if ui
                .add(
                    egui::Slider::new(&mut size, 72.0..=240.0)
                        .show_value(true)
                        .fixed_decimals(0),
                )
                .changed()
            {
                self.library_thumb_size = size.round();
            }
            // F-100 Klickbarkeit (GUI-CLICK-ALL-17): the `\` filter drawer had
            // no clickable access; this button toggles the same path.
            ui.separator();
            if ui
                .selectable_label(self.filter_bar_visible, Str::FilterBar.t())
                .on_hover_text(Str::ShortcutHint.format_arg("\\"))
                .clicked()
            {
                self.toggle_filter_bar();
            }
        });
        // R4-LIB-1(b): one-click Up + breadcrumb, so leaving a folder —
        // including a stray `.lumina/` — never requires editing the path text
        // field. Clicking an earlier segment navigates to that ancestor.
        ui.horizontal_wrapped(|ui| {
            let crumbs = crate::library_tree::library_breadcrumb(&self.directory);
            let mut jump: Option<String> = None;
            if crumbs.len() > 1 {
                let parent = crumbs[crumbs.len() - 2].1.clone();
                if ui.button("⬆").on_hover_text(parent.clone()).clicked() {
                    jump = Some(parent);
                }
            }
            for (index, (label, target)) in crumbs.iter().enumerate() {
                if index > 0 {
                    ui.label("›");
                }
                let current = index + 1 == crumbs.len();
                if ui.selectable_label(current, label).clicked() && !current {
                    jump = Some(target.clone());
                }
            }
            if let Some(target) = jump {
                self.set_directory(target);
            }
        });
        // G-09 + FACE-20-S5: explicit Library-view selector (Grid / Loupe /
        // Compare / Survey / People), UX-LOOK-TOOLBAR-18 iconified at the LR
        // place with the existing labels/shortcuts as tooltips. The keyboard
        // shortcuts stay the primary path; People deliberately has no new
        // global shortcut (FACE-20 §3).
        ui.horizontal_wrapped(|ui| {
            for view in [
                LibraryView::Grid,
                LibraryView::Loupe,
                LibraryView::Compare,
                LibraryView::Survey,
                LibraryView::People,
            ] {
                let active = self.library_view == view;
                if crate::icon_toolbar::icon_button(
                    ui,
                    crate::icon_toolbar::library_view_icon(view),
                    active,
                )
                .clicked()
                {
                    self.set_library_view(view);
                }
            }
        });
        ui.separator();
        // R5-SORT-1 (User-Bug, 2026-09-20): the Library sort modes (Name /
        // Capture Date / Custom) are always visible in the grid header. They
        // used to live only inside the `\` drawer and were unfindable without
        // knowing that shortcut; the drawer now holds only the text filter and
        // Quick Develop.
        ui.horizontal_wrapped(|ui| {
            ui.label(Str::LibrarySortName.t().to_string() + ":");
            for sort in [
                LibrarySort::Name,
                LibrarySort::CaptureDate,
                LibrarySort::Custom,
            ] {
                if ui
                    .selectable_label(self.library_sort == sort, sort_label(sort))
                    .clicked()
                {
                    if let Err(message) = self.set_library_sort(sort) {
                        self.show_error(message);
                    }
                }
            }
        });
        // Welle 3 (LR-13 light): `\` Library drawer — text filter over the
        // scanned entry metadata plus Quick Develop sliders. Hidden by
        // default, so the default grid layout (and its kittest goldens) are
        // pixel-identical without it.
        if self.filter_bar_visible {
            ui.horizontal(|ui| {
                ui.label(Str::FilterBar.t());
                let mut query = self.library_filter.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text(Str::FilterPlaceholder.t()),
                    )
                    .changed()
                {
                    self.set_library_filter(query);
                }
            });
            ui.collapsing(Str::QuickDevelop.t(), |ui| {
                for (key, label, range) in [
                    ("exposure", Str::Exposure, -10.0..=10.0),
                    ("contrast", Str::Contrast, -1.0..=1.0),
                    ("highlights", Str::Highlights, -1.0..=1.0),
                    ("shadows", Str::Shadows, -1.0..=1.0),
                ] {
                    let mut value = self.recipe.adjustments.get(key).copied().unwrap_or(0.0);
                    if ui
                        .add(egui::Slider::new(&mut value, range).text(label.t()))
                        .changed()
                    {
                        if let Err(error) = self.apply_quick_develop(key, value) {
                            self.show_error(error);
                        }
                    }
                }
            });
            self.draw_library_metadata(ui);
            ui.separator();
        }
        // GUI-SCROLL-200-1: index-based view over the RAW entries. Only the
        // visible rows are laid out (show_rows) and only the buffered window's
        // thumbnails are ensured per frame — never an O(n) loop over all
        // entries. GUI-FILMSTRIP-DUP-1: one shared index source.
        // G-09 (LRPAR-G09-LIB): grid, loupe, compare and survey share this
        // filtered order ([`Self::filtered_library_order`]: RAW-only display
        // order narrowed by the active collection view and the `\` query),
        // so painting and keyboard navigation always see the same list.
        let raw_indices: Vec<usize> = self.filtered_library_order();
        // LRPAR-G12-FACE-20 (S5): the People view is not tied to the RAW-only
        // grid order (a face analysis of any loaded source is shown), so it
        // branches before the shared empty state below.
        if self.library_view == LibraryView::People {
            self.draw_library_people(ctx, ui);
            return;
        }
        // UX-SLICE-2 (F3): one shared empty state for every Library view —
        // Grid, Loupe, Compare and Survey. The check runs before the view
        // branch so the non-grid views can no longer render a second,
        // divergent empty text (`Heading + ReadyForImage` vs. icon + CTA).
        if raw_indices.is_empty() {
            self.draw_library_empty_state(ui);
            return;
        }
        // G-09: non-grid views branch here; the grid body below (and its
        // kittest goldens) stays pixel-identical for `LibraryView::Grid`.
        match self.library_view {
            LibraryView::Loupe => {
                self.library_cols = 1;
                self.draw_library_loupe(ctx, ui, &raw_indices);
                return;
            }
            LibraryView::Compare => {
                self.library_cols = 1;
                self.draw_library_compare(ctx, ui, &raw_indices);
                return;
            }
            LibraryView::Survey => {
                self.draw_library_survey(ctx, ui, &raw_indices);
                return;
            }
            // Handled above (independent of the RAW-only grid order).
            LibraryView::People => return,
            LibraryView::Grid => {}
        }
        let thumb = self.library_thumb_size;
        const CELL_INNER_PAD: f32 = 8.0;
        let cell_inner = (thumb - CELL_INNER_PAD).max(32.0);
        let cols = ((ui.available_width() / thumb).floor() as usize).max(1);
        self.library_cols = cols;
        let count = raw_indices.len();
        let total_rows = count.div_ceil(cols);
        // LRPAR-G09-SORT-09: the grid drag-drop reorder is applied after the
        // paint loop so the running `raw_indices` stay valid for this frame.
        let mut pending_reorder: Option<(String, String)> = None;
        // The closure returns the laid-out row window so scheduling below runs
        // with the exact visible range.
        let visible_rows = {
            egui::ScrollArea::vertical()
                .show_rows(
                    ui,
                    cell_inner,
                    total_rows,
                    |ui, rows: std::ops::Range<usize>| {
                        for row in rows.clone() {
                            let row_start = row * cols;
                            let row_end = (row_start + cols).min(count);
                            ui.horizontal(|ui| {
                                for &entry_idx in &raw_indices[row_start..row_end] {
                                    let entry = self.entries[entry_idx].clone();
                                    // R3-GRIDSEL-1: the grid highlight follows
                                    // the shared filmstrip selection (like
                                    // Loupe/Compare/Survey), never the loaded
                                    // `self.path`. A selected-but-not-yet-loaded
                                    // cell is highlighted; the loaded cell is not
                                    // unless it is also selected. This makes the
                                    // "All views stay in sync" contract below true.
                                    let selected = self
                                        .filmstrip_selection
                                        .contains(&entry.path.display().to_string());
                                    let tex = self.thumbnail_for_entry(&entry);
                                    let placeholder_label =
                                        self.thumbnail_placeholder_label(&entry);
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(cell_inner, cell_inner),
                                        egui::Sense::hover(),
                                    );
                                    if selected {
                                        ui.painter().rect_stroke(
                                            rect.expand(2.0),
                                            3.0,
                                            egui::Stroke::new(
                                                2.0_f32,
                                                ui.visuals().selection.bg_fill,
                                            ),
                                            egui::StrokeKind::Outside,
                                        );
                                    }
                                    if let Some(texture) = tex {
                                        ui.put(
                                            rect,
                                            egui::Image::from_texture(&texture)
                                                .max_size(rect.size()),
                                        );
                                    } else {
                                        ui.painter().rect_filled(
                                            rect,
                                            2.0,
                                            egui::Color32::from_gray(40),
                                        );
                                        ui.put(rect, egui::Label::new(placeholder_label));
                                    }
                                    // LRPAR-G09-SORT-09: register the cell interaction
                                    // after the placeholder image/label so the cell is
                                    // the topmost interactive widget. A stable per-cell
                                    // id makes the drag-drop reorder testable headlessly;
                                    // click = select/open, drag = reorder.
                                    let resp = ui.interact(
                                        rect,
                                        crate::library_sort::library_cell_id(&entry.thumb_key),
                                        egui::Sense::click_and_drag(),
                                    );
                                    // The dragged cell carries its path; dropping it on
                                    // another cell records "move before that cell"
                                    // (applied after the paint loop).
                                    resp.dnd_set_drag_payload(entry.path.display().to_string());
                                    if let Some(dragged) = resp.dnd_release_payload::<String>() {
                                        pending_reorder = Some((
                                            dragged.to_string(),
                                            entry.path.display().to_string(),
                                        ));
                                    }
                                    // R5-SORT-1: a custom drag shows an
                                    // insertion line at the hovered target so
                                    // the drop position is visible before the
                                    // release. The dragged cell's own line is
                                    // skipped (it still contains the pointer).
                                    if let Some(dragged) = resp.dnd_hover_payload::<String>() {
                                        if dragged.as_str() != entry.path.display().to_string() {
                                            let x = rect.left() - 3.0;
                                            ui.painter().line_segment(
                                                [
                                                    egui::pos2(x, rect.top()),
                                                    egui::pos2(x, rect.bottom()),
                                                ],
                                                egui::Stroke::new(3.0_f32, crate::theme::ACCENT),
                                            );
                                        }
                                    }
                                    // LRPAR-G15-STACK-15: the painted stack
                                    // badge is clickable (toggles the collapse
                                    // of this stack) and takes precedence over
                                    // the plain cell click.
                                    let stack_badge_clicked = self.paint_stack_badge(
                                        ui,
                                        rect,
                                        &entry,
                                        crate::library_stacks::StackBadgeSurface::Grid,
                                    );
                                    // GUI-FILMSTRIP-DUP-1: single click selects
                                    // (shared filmstrip selection, no open);
                                    // double-click opens in Develop. All
                                    // views stay in sync through the same
                                    // selection bookkeeping.
                                    // R5-SELECT-1: the grid must read the same
                                    // Cmd/Ctrl-toggle + Shift-range modifiers as
                                    // the filmstrip (`filmstrip_frame.rs`),
                                    // otherwise multi-selection is impossible
                                    // in the grid while Compare/Survey accept it.
                                    if resp.clicked() && !stack_badge_clicked {
                                        let modifiers = ui.input(|state| state.modifiers);
                                        let toggle = modifiers.command || modifiers.ctrl;
                                        let range = modifiers.shift;
                                        trace!(
                                            "GUI interaction: library grid click {}",
                                            entry.path.display()
                                        );
                                        self.select_filmstrip_path(
                                            entry.path.display().to_string(),
                                            toggle,
                                            range,
                                        );
                                    }
                                    if stack_badge_clicked {
                                        if let Err(message) = self.toggle_stack_collapse_for_path(
                                            &entry.path.display().to_string(),
                                        ) {
                                            self.show_error(message);
                                        }
                                    }
                                    if resp.double_clicked() {
                                        trace!(
                                            "GUI interaction: library grid open {}",
                                            entry.path.display()
                                        );
                                        self.open_grid_entry_in_develop(
                                            entry.path.display().to_string(),
                                        );
                                    }
                                    // LR-01 + Welle 2: rating/flag/color-label
                                    // badge of the default copy, painted over
                                    // the cell's bottom edge (display-only;
                                    // edits go through the rating section or
                                    // the 1-5/6-9/P/X/U keys). Unrated +
                                    // unflagged + unlabeled cells stay clean.
                                    // UX-SLICE-1: shared with the filmstrip.
                                    paint_entry_badge(ui, rect, &entry);
                                    // LRPAR-G09-CULL-25: assisted-culling badge
                                    // (top-right; visually distinct from the
                                    // manual rating badge at the bottom edge).
                                    cull_gui::paint_cull_badge(
                                        ui,
                                        rect,
                                        cull_gui::entry_cull_badge(&entry),
                                    );
                                    // F-100 Library: relative-subfolder badge of
                                    // the recursive aggregation, painted over
                                    // the cell's top edge (display-only, like
                                    // the rating badge). Empty for top-level
                                    // files, so flat listings (tree click) and
                                    // the existing goldens stay pixel-identical.
                                    if !entry.folder.is_empty() {
                                        let badge_pos = rect.left_top() + egui::vec2(4.0, 2.0);
                                        ui.painter().rect_filled(
                                            egui::Rect::from_min_size(
                                                badge_pos - egui::vec2(2.0, 0.0),
                                                egui::vec2(118.0, 16.0),
                                            ),
                                            2.0,
                                            LIBRARY_BADGE_BG,
                                        );
                                        ui.painter().text(
                                            badge_pos,
                                            egui::Align2::LEFT_TOP,
                                            folder_badge_display(&entry.folder),
                                            egui::FontId::monospace(11.0),
                                            egui::Color32::WHITE,
                                        );
                                    }
                                    // Sidecar/copy status on hover (kept from the former
                                    // text file-browser). The full (untruncated)
                                    // subfolder badge is part of the tooltip so
                                    // the ellipsized display text loses nothing.
                                    let hover_folder = if entry.folder.is_empty() {
                                        String::new()
                                    } else {
                                        format!("\n{}", entry.folder)
                                    };
                                    resp.on_hover_text(format!(
                                        "{}{}\n[{}] {}:{} {}:{} {}:{} {}:{} {}:{}",
                                        entry.name,
                                        hover_folder,
                                        entry.status_label(),
                                        Str::Copies.t(),
                                        entry.virtual_copies,
                                        Str::Masking.t(),
                                        entry.missing_models,
                                        Str::Rating.t(),
                                        stars_for_rating(entry.rating),
                                        Str::FlagLabel.t(),
                                        flag_label(entry.flag),
                                        Str::ColorLabel.t(),
                                        color_label_name(entry.color_label),
                                    ));
                                }
                            });
                        }
                        rows
                    },
                )
                .inner
        };
        // GUI-SCROLL-200-1: schedule thumbnail work only for the visible
        // window (+ buffer), then a bounded nearest-first off-screen prefetch.
        let window = visible_rows.start * cols..(visible_rows.end * cols).min(count);
        self.frame_thumb_enqueued += self.ensure_thumbnail_priority(ctx, &raw_indices, window);
        // LRPAR-G09-SORT-09: apply a drop that happened this frame last, so the
        // paint loop and the thumbnail scheduling above both used the pre-drop
        // order consistently.
        if let Some((dragged, target)) = pending_reorder {
            if let Err(message) = self.reorder_library_entry(&dragged, &target) {
                self.show_error(message);
            }
        }
    }
}
