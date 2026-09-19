//! GUI-REFACTOR-W2-20 S2.5: the filmstrip frame and thumbnail scheduling,
//! extracted verbatim from `lib.rs` (the thumbnail `ThumbnailManager`/disk
//! cache itself lives in `filmstrip.rs`).
//!
//! [`LuminaApp::draw_filmstrip`] paints the bottom file browser,
//! [`LuminaApp::ensure_thumbnail_priority`] schedules on-screen/nearest cells
//! first, [`LuminaApp::ensure_thumbnail`] resolves one cell against the
//! manager/disk cache, and the label/texture helpers render the placeholder and
//! texture. No behaviour changes: scheduling order, retry budgets and the
//! `debug!`/`trace!`s are byte-identical. All entry points are `pub(crate)`
//! because the app root, navigator and headless tests call them.

use super::*;
use log::{debug, trace};

impl LuminaApp {
    /// Library-module sidecar / virtual-copy manager (native only).  Mask editing
    /// lives in the Develop panel's Masking section; here the user picks which
    /// source copy to work on and can duplicate it.
    pub(crate) fn draw_filmstrip(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // RAW-only: the Develop/Lightroom preview pipeline is RAW-first, so the
        // filmstrip never shows jpg/png/webp/raster entries (those remain
        // browseable in the Library file-browser via `is_supported_image`).
        // GUI-FILMSTRIP-DUP-1: one shared index source — each image once.
        let raw_indices: Vec<usize> = self.raw_entry_indices();
        let count = raw_indices.len();
        // UX-SLICE-1 (UXG-09, mapper P1): one shared strip component for
        // Library / Develop / Export with an "n of N" counter in the header. `n` is the
        // number of strip entries currently selected; thumbnails come from the
        // same `ThumbnailManager` the Library grid uses.
        ui.horizontal(|ui| {
            ui.heading(Str::Filmstrip.t());
            if count > 0 {
                let selected = raw_indices
                    .iter()
                    .filter(|&&index| {
                        self.filmstrip_selection
                            .contains(&self.entries[index].path.display().to_string())
                    })
                    .count();
                let counter = Str::FilmstripCounter
                    .t()
                    .replacen("{}", &selected.to_string(), 1)
                    .replacen("{}", &count.to_string(), 1);
                ui.label(egui::RichText::new(counter).small());
            }
        });
        // UX-SLICE-1: an empty strip keeps an honest empty text instead of the
        // misleading "click a thumbnail" hint or a fake placeholder thumb.
        ui.label(if count == 0 {
            Str::FilmstripEmpty.t()
        } else {
            Str::FilmstripHint.t()
        });
        // GUI-FILMSTRIP-SYNC-1: selection actions (Lightroom Sync Settings /
        // Match Total Exposures). They apply to the multi-selection below and
        // live here — not in the Develop footer — so they stay reachable in
        // all three modules like the filmstrip itself.
        ui.horizontal(|ui| {
            let selected = self.filmstrip_selection.len();
            let sync_label = if selected == 0 {
                Str::SyncSettings.t().to_string()
            } else {
                format!("{} ({selected})", Str::SyncSettings.t())
            };
            if ui.button(sync_label).clicked() {
                self.sync_settings_to_selection();
            }
            if ui.button(Str::MatchSelection.t()).clicked() {
                self.match_exposures_of_selection();
            }
            // LRPAR-G08-PREVIOUS: one-click takeover from the previously
            // edited image (cross-image Previous, unlike the panel-local
            // G-01 Previous/Reset rows). Same headless visibility guarantee
            // as Sync/Match (see `filmstrip_selection_actions_are_visible`).
            if ui.button(Str::PreviousImage.t()).clicked() {
                self.apply_previous_to_selection();
            }
        });
        // GUI-SCROLL-200-1: fixed-size cells let us lay out only the visible
        // window (+ a small buffer). Off-screen cells are never allocated,
        // painted or probed for thumbnails on this frame.
        // Lightroom-like filmstrip cell: the larger 140x110 cell (was 110x84)
        // keeps the strip readable on high-DPI displays ("switching too small").
        const CELL_W: f32 = 140.0;
        const CELL_H: f32 = 110.0;
        let step = CELL_W + ui.spacing().item_spacing.x;
        // The closure returns the visible window it laid out so thumbnail
        // scheduling below runs *after* drawing with no extra state.
        let visible = {
            egui::ScrollArea::horizontal()
                .show_viewport(ui, |ui, viewport| {
                    ui.set_height(CELL_H);
                    let visible = viewport::visible_cell_range(
                        viewport.left(),
                        viewport.width(),
                        step,
                        count,
                    );
                    // Leading spacer positions the first *drawn* cell at its
                    // absolute content position; `set_width` keeps the
                    // scrollbar proportional to the full strip even though
                    // only the window is laid out.
                    let buffered = viewport::buffered_range(
                        visible.clone(),
                        count,
                        viewport::VISIBLE_BUFFER_CELLS,
                    );
                    // R2-GUI-FILMSTRIP-ROW: the filmstrip must lay out as one
                    // horizontal row inside the horizontally scrolled area.
                    // `ScrollArea::horizontal()` only enables the horizontal
                    // scrollbar — it does NOT change the child UI's layout
                    // direction, which would otherwise stay top-down (vertical)
                    // and stack the cells into a column. The horizontal wrapper
                    // restores the single-row filmstrip (this was lost in the
                    // GUI-SCROLL-200-1 virtualization refactor).
                    ui.horizontal(|ui| {
                        // Leading spacer positions the first *drawn* cell at its
                        // absolute content position; `set_width` keeps the
                        // scrollbar proportional to the full strip even though
                        // only the window is laid out.
                        if buffered.start > 0 {
                            ui.add_space(buffered.start as f32 * step);
                        }
                        for i in buffered.clone() {
                            let entry = self.entries[raw_indices[i]].clone();
                            let tex = self.thumbnails.get(&entry.thumb_key).cloned();
                            let placeholder_label = self.thumbnail_placeholder_label(&entry);
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(CELL_W, CELL_H),
                                egui::Sense::click(),
                            );
                            if let Some(texture) = tex {
                                ui.put(
                                    rect,
                                    egui::Image::from_texture(&texture).max_size(rect.size()),
                                );
                            } else {
                                ui.painter()
                                    .rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                                ui.put(rect, egui::Label::new(placeholder_label));
                            }
                            // GUI-FILMSTRIP-SYNC-1: the multi-selection is
                            // always visible — never implied by soft pixels.
                            if self
                                .filmstrip_selection
                                .contains(&entry.path.display().to_string())
                            {
                                ui.painter().rect_stroke(
                                    rect.expand(2.0),
                                    3.0,
                                    egui::Stroke::new(2.0_f32, ui.visuals().selection.bg_fill),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            // UX-SLICE-1 (UXG-09): small rating/flag/color
                            // badge per cell — same `FileBrowserEntry` data and
                            // presentation as the Library grid (shared helper).
                            paint_entry_badge(ui, rect, &entry);
                            if resp.clicked() {
                                // Cmd/Ctrl-Click toggles, Shift-Click extends
                                // the range from the anchor; a plain click
                                // selects exactly this image.
                                let modifiers = ctx.input(|state| state.modifiers);
                                let toggle = modifiers.command || modifiers.ctrl;
                                let range = modifiers.shift;
                                trace!("GUI interaction: filmstrip click {}", entry.path.display());
                                self.handle_filmstrip_click(
                                    entry.path.display().to_string(),
                                    toggle,
                                    range,
                                );
                            }
                        }
                        let total_width =
                            (count as f32 * step - ui.spacing().item_spacing.x).max(0.0);
                        ui.set_width(total_width);
                    });
                    visible
                })
                .inner
        };
        // GUI-SCROLL-200-1: visible-first thumbnail scheduling (see
        // `ensure_thumbnail_priority`). No O(n) per-frame loop anymore.
        self.frame_thumb_enqueued += self.ensure_thumbnail_priority(ctx, &raw_indices, visible);
    }

    /// GUI-SCROLL-200-1: visible-first thumbnail scheduling.
    ///
    /// Enqueues thumbnail work for the entries in `visible_window` (widened by
    /// [`viewport::VISIBLE_BUFFER_CELLS`]) first and unconditionally; then
    /// touches at most [`viewport::PREFETCH_BUDGET_PER_FRAME`] off-screen
    /// entries, nearest to the window first ([`viewport::prefetch_order`]).
    /// Entries that already have a texture or an in-flight job are skipped for
    /// free (`ThumbnailManager::needs_job`) and never consume budget.
    ///
    /// This ordering *is* the job priority mechanism: the worker pool drains
    /// the unbounded FIFO channel in order, so a visible cell's job is always
    /// enqueued — and therefore started — before any prefetched off-screen
    /// job of the same frame. Off-screen work is additionally rate-limited to
    /// keep the worst-case per-frame disk-cache probes bounded.
    ///
    /// Returns how many worker jobs were enqueued / cached previews loaded
    /// this call (fed into the `LUMINA_PERF_LOG` counters).
    pub(crate) fn ensure_thumbnail_priority(
        &mut self,
        ctx: &egui::Context,
        raw_indices: &[usize],
        visible_window: std::ops::Range<usize>,
    ) -> usize {
        let count = raw_indices.len();
        let buffered =
            viewport::buffered_range(visible_window, count, viewport::VISIBLE_BUFFER_CELLS);
        let mut enqueued = 0;
        // Pass 1: the buffered visible window — always fully ensured, no cap.
        for i in buffered.clone() {
            let entry = self.entries[raw_indices[i]].clone();
            if self.ensure_thumbnail(ctx, &entry) {
                enqueued += 1;
            }
        }
        // Pass 2: bounded nearest-first off-screen prefetch.
        let mut budget = viewport::PREFETCH_BUDGET_PER_FRAME;
        for i in viewport::prefetch_order(count, buffered) {
            if budget == 0 {
                break;
            }
            let key = &self.entries[raw_indices[i]].thumb_key;
            // Free check: skips cells with a texture / in-flight job without
            // any disk IO. Only real candidates consume the per-frame budget.
            if !self.thumbnails.needs_job(key) {
                continue;
            }
            budget -= 1;
            let entry = self.entries[raw_indices[i]].clone();
            if self.ensure_thumbnail(ctx, &entry) {
                enqueued += 1;
            }
        }
        enqueued
    }

    /// Ensure a thumbnail exists for `entry`.
    ///
    /// Returns `true` when this call did potentially expensive work: enqueued a
    /// background worker job or synchronously loaded/inserted a cached preview
    /// from the disk cache. Callers feed this into the `LUMINA_PERF_LOG`
    /// frame counters (GUI-SCROLL-200-1). `false` means the call was cheap
    /// (texture already present, job in flight, retry budget exhausted).
    pub(crate) fn ensure_thumbnail(
        &mut self,
        ctx: &egui::Context,
        entry: &FileBrowserEntry,
    ) -> bool {
        // Key is the canonicalized absolute path, never the bare filename
        // (REVIEW-GUI-THUMB-1).
        let key = entry.thumb_key.clone();
        if self.thumbnails.get(&key).is_some() {
            return false;
        }
        if !self.thumbnails.needs_job(&key) {
            return false;
        }
        if let Ok(cache) = DiskFolderCache::for_image(entry.path.as_path()) {
            // Use the headless-testable cache probe; on a hit, load and display
            // the stored preview.  A miss enqueues a background thumbnail job
            // (no silent fallback to a wrong/sized-up image).
            if filmstrip::filmstrip_preview_cached(&cache, &entry.name, "vc-original") {
                if let Ok(Some(bytes)) =
                    cache.load_preview(&entry.name, "vc-original", PreviewKind::Standard)
                {
                    match ImageFrame::decode(&bytes) {
                        Ok(frame) => {
                            let tex = self.make_thumbnail_texture(ctx, &frame, &key);
                            // insert marks the key probed *after* success only
                            // (REVIEW-GUI-THUMB-2).
                            self.thumbnails.insert(&key, tex);
                            return true;
                        }
                        Err(error) => {
                            // A cached-but-corrupt preview is a visible error,
                            // not a silent miss.
                            self.thumbnails
                                .mark_failed(&key, format!("cached preview unreadable: {error}"));
                            return true;
                        }
                    }
                }
            }
        }
        // Cache miss: enqueue a background thumbnail job on the dedicated thread
        // pool rather than the bounded `IdleQueue`. The channel is unbounded, so
        // it never drops jobs under load. The key is marked in-flight (NOT
        // probed) so a worker failure can retry in a bounded way and surface a
        // visible error instead of a permanent gray cell (REVIEW-GUI-THUMB-2).
        self.thumbnails.begin_job(&key);
        match self.thumbnail_tx.send(ThumbnailJob {
            source: entry.path.clone(),
            name: entry.name.clone(),
            key,
        }) {
            Ok(()) => {
                debug!("enqueued thumbnail job for {}", entry.name);
                true
            }
            Err(_) => {
                // Channel closed: release the in-flight slot so a later frame
                // retries once the pool is back.
                self.thumbnails.job_dispatch_failed(&entry.thumb_key);
                debug!(
                    "thumbnail channel closed; will retry {} on a later frame",
                    entry.name
                );
                false
            }
        }
    }

    /// Placeholder caption for a thumbnail cell: the filename, plus the visible
    /// failure message once the retry budget is exhausted
    /// (REVIEW-GUI-THUMB-2 — never a silent gray cell).
    pub(crate) fn thumbnail_placeholder_label(&self, entry: &FileBrowserEntry) -> String {
        match self.thumbnails.failure(&entry.thumb_key) {
            Some(message) => format!("{} ⚠ {}", entry.name, message),
            None => entry.name.clone(),
        }
    }

    /// Thumbnail textures are produced by the worker pool.
    pub(crate) fn make_thumbnail_texture(
        &self,
        ctx: &egui::Context,
        frame: &ImageFrame,
        key: &str,
    ) -> egui::TextureHandle {
        let size = [frame.width as usize, frame.height as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
        ctx.load_texture(format!("thumb-{key}"), image, egui::TextureOptions::LINEAR)
    }
}
