//! `impl eframe::App for LuminaApp` — the per-frame `ui` entry point, moved
//! verbatim out of `lib.rs` (FILE-SIZE-1000-ALL-EXCEPTIONS-100, the last
//! 1000-line Ratchet exception). A trait impl may live in any module of the
//! defining crate, so this is a pure structural move: the body is
//! byte-identical to its `lib.rs` text; only this header and the
//! `use super::*;` glob are new.

use super::*;

impl eframe::App for LuminaApp {
    // eframe 0.36: `update(&mut self, ctx, frame)` was replaced by
    // `ui(&mut self, ui, frame)`; the context is cloned off the root `Ui`.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let perf_t0 = if std::env::var("LUMINA_PERF_LOG").as_deref() == Ok("1") {
            Some(std::time::Instant::now())
        } else {
            None
        };
        // Apply the Lumina dark theme once per frame. `egui` only re-applies the
        // fields that changed, so this is cheap and keeps the Lightroom feeling
        // consistent across modules.
        apply_lightroom_dark(&ctx);

        // Keyboard: `Y` toggles Before/After (which never mutates the recipe);
        // `Shift+Y` toggles the split Before/After marker (Welle 3, same
        // recipe-free guarantee). `Esc` cancels an armed white-balance
        // eyedropper; the interactive crop tool handles its own Enter/Esc.
        let shift_held = ctx.input(|i| i.modifiers.shift);
        if ctx.input(|i| i.key_pressed(egui::Key::Y)) {
            if shift_held {
                self.toggle_split_view();
            } else {
                self.toggle_before_after();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Q)) && !ctx.egui_wants_keyboard_input() {
            // UX-LOOK-TOOLBAR-18: shared with the icon toolbar (same status).
            self.toggle_spot_heal_tool();
        }
        // R5-BRUSH-24 / R5-DUST-23: `[`/`]` resize the exclusively armed mask
        // brush or spot tool. Each handler is widget-gated and inert while the
        // other tool owns the preview.
        self.handle_brush_size_shortcuts(&ctx);
        self.handle_spot_size_shortcuts(&ctx);
        self.handle_crop_shortcuts(&ctx);

        // Module-switch shortcuts (`G` Library grid, `D` Develop, `E`
        // Library loupe). They are ignored while a widget wants keyboard
        // input — e.g. a focused text field for mask/preset names — so they
        // cannot hijack typing. Switching modules never mutates the recipe
        // or sidecar. `G`/`E` route through the G-09 Library view so the
        // grid and the loupe are distinct, headless-testable states.
        if !ctx.egui_wants_keyboard_input() {
            if let Some(module) = ctx.input(|i| {
                for key in [egui::Key::G, egui::Key::D, egui::Key::E] {
                    if i.key_pressed(key) {
                        if let Some(target) = module_for_key(key) {
                            return Some(target);
                        }
                    }
                }
                None
            }) {
                if ctx.input(|i| i.key_pressed(egui::Key::E)) {
                    self.set_library_view(LibraryView::Loupe);
                } else if ctx.input(|i| i.key_pressed(egui::Key::G)) {
                    self.set_library_view(LibraryView::Grid);
                } else {
                    self.set_module(module);
                }
            }
        }

        // Consume idle tasks only while there is no interactive pointer input.
        // Only mask inference remains here; filmstrip thumbnails are produced by
        // the dedicated background thread pool (handled just below, without a
        // pointer gate, so switching the filmstrip never freezes).
        if !ctx.input(|input| input.pointer.any_down()) {
            if let Some((_id, task)) = self.idle_queue.pop_next() {
                match task {
                    IdleTask::MaskInference { mask_id } => {
                        self.status = Str::InferenceWaiting.format_arg(&mask_id);
                    }
                    IdleTask::Thumbnail { .. } => {
                        // Thumbnails are no longer enqueued on the idle queue
                        // (the thread pool owns their generation); kept only for
                        // an exhaustive match.
                    }
                }
            }
        }

        // Zoom shortcuts (Lightroom-like). Ignored while a widget wants keyboard
        // input so they never hijack typing. These set the zoom mode / a custom
        // multiplier; the actual `preview_zoom` is derived per-frame in
        // `sync_zoom()` so the ROI crop matches the on-screen view.
        // LR-01: `Num1`/`Num2` no longer zoom — they set the star rating (1:1
        // and 2:1 stay reachable through the preview toolbar buttons).
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
                self.zoom_step(1.2);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
                self.zoom_step(1.0 / 1.2);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F)) {
                // Welle 3 (LR-09 light): `F` is the fullscreen preview (hides
                // the lights-out chrome and settles the zoom on Fit when
                // enabling, so the previous zoom-to-fit role is preserved).
                self.toggle_fullscreen();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                // `Num0` clears the rating only when a document is loaded;
                // without an image it keeps its historical zoom-to-fit role.
                if self.document.is_some() {
                    if let Err(error) = self.set_rating(0) {
                        self.show_error(error);
                    }
                } else {
                    self.set_zoom_mode(ZoomMode::Fit);
                }
            }
        }

        // LR-01 / LR-09 / LR-10 rating, flag, mask-tool and duplicate shortcuts.
        // Ignored while a widget wants keyboard input so typing (mask names,
        // preset names, paths) is never hijacked. Failures surface via
        // `show_error`, never silently.
        if !ctx.egui_wants_keyboard_input() {
            for key in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ] {
                if ctx.input(|i| i.key_pressed(key)) {
                    if let Some(rating) = rating_for_key(key) {
                        if let Err(error) = self.set_rating(rating) {
                            self.show_error(error);
                        }
                    }
                }
            }
            for key in [egui::Key::P, egui::Key::X, egui::Key::U] {
                if ctx.input(|i| i.key_pressed(key)) {
                    if let Some(flag) = flag_for_key(key) {
                        if let Err(error) = self.set_flag(flag) {
                            self.show_error(error);
                        }
                    }
                }
            }
            let shift = ctx.input(|i| i.modifiers.shift);
            for key in [egui::Key::K, egui::Key::M] {
                // Cmd/Ctrl+M is the G-13 panorama-merge chord below; the mask
                // tool is modifier-free (see the F-100 shortcut table).
                if ctx.input(|i| i.key_pressed(key) && !i.modifiers.ctrl && !i.modifiers.command) {
                    if let Some(tool) = mask_tool_for_key(key, shift) {
                        // R5-TOOLFLOW-1: commit the active tool before arming.
                        self.commit_outgoing_tool_for_switch(&ctx);
                        self.set_mask_tool(tool);
                    }
                }
            }
            // LRPAR-G13-MERGE-15: `Cmd/Ctrl+H` / `Cmd/Ctrl+M` start the HDR /
            // panorama merge on the filmstrip selection (job control; the
            // shared `lumina-merge` entry points, no GUI image logic).
            for (key, mode) in [
                (egui::Key::H, lumina_sidecar::MergeMode::Hdr),
                (egui::Key::M, lumina_sidecar::MergeMode::Panorama),
            ] {
                if ctx.input(|i| i.key_pressed(key) && (i.modifiers.ctrl || i.modifiers.command)) {
                    if let Err(error) = self.start_merge(mode) {
                        self.show_error(error);
                    }
                }
            }
            if ctx.input(|i| {
                i.key_pressed(egui::Key::Quote) && (i.modifiers.ctrl || i.modifiers.command)
            }) {
                if let Err(error) = self.duplicate_active_copy() {
                    self.show_error(error);
                }
            }
            // Welle 2: color labels `6`–`9` (extras, no schema change).
            for key in [
                egui::Key::Num6,
                egui::Key::Num7,
                egui::Key::Num8,
                egui::Key::Num9,
            ] {
                if ctx.input(|i| i.key_pressed(key)) {
                    if let Some(label) = color_label_for_key(key) {
                        if let Err(error) = self.set_color_label(label) {
                            self.show_error(error);
                        }
                    }
                }
            }
            // Welle 2 (LR-09): copy/paste settings `Cmd/Ctrl+Shift+C/V` for
            // the active virtual copy.
            for key in [egui::Key::C, egui::Key::V] {
                if ctx.input(|i| {
                    i.key_pressed(key)
                        && (i.modifiers.ctrl || i.modifiers.command)
                        && i.modifiers.shift
                }) {
                    match clipboard_action_for_key(key, true, true) {
                        Some(ClipboardAction::Copy) => {
                            if let Err(error) = self.copy_settings() {
                                self.show_error(error);
                            }
                        }
                        Some(ClipboardAction::Paste) => {
                            if let Err(error) = self.paste_settings() {
                                self.show_error(error);
                            }
                        }
                        None => {}
                    }
                }
            }
            // Welle 2: B&W treatment `V` (recipe-backed, restores on repeat).
            if ctx.input(|i| i.key_pressed(egui::Key::V)) {
                if let Err(error) = self.toggle_black_white() {
                    self.show_error(error);
                }
            }
            // G-16: plain `S` toggles the display-only softproof preview
            // (G-10 binding reservation; full simulation is G-10 follow-up).
            // Strictly modifier-free so the `Cmd/Ctrl+Alt+S` snapshot chord
            // above never double-fires.
            if ctx.input(|i| {
                i.key_pressed(egui::Key::S)
                    && !i.modifiers.ctrl
                    && !i.modifiers.command
                    && !i.modifiers.alt
                    && !i.modifiers.shift
            }) {
                self.toggle_softproof_preview();
            }
            // Welle 3 (LR-17 light): stack-group proxy `Cmd/Ctrl+G` for the
            // active virtual copy. Failures surface via `show_error`, never
            // silently.
            if ctx
                .input(|i| i.key_pressed(egui::Key::G) && (i.modifiers.ctrl || i.modifiers.command))
            {
                match self.toggle_stack_group() {
                    Ok(_) => {}
                    Err(error) => self.show_error(error),
                }
            }
            // Welle 3 (LR-12 light): snapshot `Cmd/Ctrl+Alt+S` freezes the
            // session recipe under an auto name (`Snapshot <n>`).
            if ctx.input(|i| {
                i.key_pressed(egui::Key::S)
                    && (i.modifiers.ctrl || i.modifiers.command)
                    && i.modifiers.alt
            }) {
                self.create_snapshot_auto();
            }
        }

        // Welle 2 display-only view toggles (`J` clipping, `L` lights-out,
        // `R` crop mode, `Tab` side panels). Recipe-free by construction, so
        // they stay available globally.
        if !ctx.egui_wants_keyboard_input() {
            for key in [egui::Key::J, egui::Key::L] {
                if ctx.input(|i| i.key_pressed(key)) {
                    match view_toggle_for_key(key) {
                        Some(ViewToggle::Clipping) => self.toggle_clipping_overlay(),
                        Some(ViewToggle::LightsOut) => self.toggle_lights_out(),
                        Some(ViewToggle::BlackWhite) | None => {}
                    }
                }
            }
            for key in [egui::Key::R, egui::Key::Tab] {
                if ctx.input(|i| i.key_pressed(key)) {
                    // G-11: `Shift+Tab` hides all panels (incl. filmstrip);
                    // plain `Tab` keeps the filmstrip. Disambiguated here so
                    // the shift variant never falls into the plain branch.
                    if all_panels_toggle_for_key(key, ctx.input(|i| i.modifiers.shift)) {
                        self.toggle_all_panels_hidden();
                    } else {
                        match panel_toggle_for_key(key) {
                            Some(PanelToggle::CropMode) => self.toggle_crop_mode(),
                            Some(PanelToggle::PanelsHidden) => self.toggle_panels_hidden(),
                            None => {}
                        }
                    }
                }
            }
            // Welle 3 (LR-13 light): `\` toggles the Library filter drawer
            // (text filter + Quick Develop). Recipe-free like the other view
            // toggles, so it stays available on every platform.
            if ctx.input(|i| i.key_pressed(egui::Key::Backslash)) {
                self.toggle_filter_bar();
            }
            // Welle 3 (LR-20 light) + G-09: `C` compare reuses Before/After,
            // `N` survey jumps to the Library grid. Plain presses only —
            // `Cmd/Ctrl+Shift+C` stays copy-settings (native block below).
            // The G-09 Library view follows the compare proxy so both stay
            // in sync (repeat press leaves the view back to Grid).
            for key in [egui::Key::C, egui::Key::N] {
                if ctx.input(|i| i.key_pressed(key) && !i.modifiers.ctrl && !i.modifiers.command) {
                    if let Some(mode) = compare_mode_for_key(key) {
                        self.toggle_compare_mode(mode);
                        self.library_view = match self.compare_mode {
                            Some(CompareMode::Compare) => LibraryView::Compare,
                            Some(CompareMode::Survey) => LibraryView::Survey,
                            None => LibraryView::Grid,
                        };
                    }
                }
            }
            // G-09 (LRPAR-G09-LIB) Library keyboard navigation: arrows move
            // the selection over the filtered raster (no open), `Home`/`End`
            // jump to the ends, `Enter` opens the active image in Loupe,
            // `Esc` returns to Grid. Ignored outside the Library module and
            // while a widget wants keyboard input, like every F-100 shortcut.
            if self.active_module == Module::Library {
                let cols = self.library_cols.max(1) as isize;
                let mut delta: Option<isize> = None;
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    delta = Some(1);
                } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    delta = Some(-1);
                } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    delta = Some(cols);
                } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    delta = Some(-cols);
                } else if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                    delta = Some(isize::MIN / 2);
                } else if ctx.input(|i| i.key_pressed(egui::Key::End)) {
                    delta = Some(isize::MAX / 2);
                }
                if let Some(step) = delta {
                    self.move_library_selection(step);
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.open_library_selection();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    && self.library_view != LibraryView::Grid
                {
                    self.set_library_view(LibraryView::Grid);
                }
            }
            // Welle 3 (LR-13 light): `Cmd/Ctrl+Shift+I` jumps to Library
            // (import lives there), `Cmd/Ctrl+Shift+E` jumps to Export. Both
            // only switch the module and announce it — dialogs stay manual.
            for key in [egui::Key::I, egui::Key::E] {
                if ctx.input(|i| {
                    i.key_pressed(key)
                        && (i.modifiers.ctrl || i.modifiers.command)
                        && i.modifiers.shift
                }) {
                    if let Some(action) = import_export_for_key(key, true, true) {
                        self.apply_import_export_action(action);
                    }
                }
            }
        }

        // PERF-FILMSTRIP: drain completed thumbnails from the background pool and
        // build their textures on the main thread. This runs every frame
        // *regardless of pointer state* — thumbnails stream in while the user
        // scrolls/clicks the filmstrip, so switching directories no longer blocks
        // on a synchronous decode+render on the UI thread.
        self.poll_thumbnails(&ctx);

        // PREVIEW-CACHE-FEATURE: per-frame LUMINA_PERF_LOG counters reset before any
        // neighbor work of this frame (a schedule inside `poll_decode` counts).
        {
            self.frame_previews_enqueued = 0;
            self.frame_previews_ready = 0;
        }

        // PERF-GUI-7: drain any completed background RAW/raster decode without
        // blocking the UI (non-blocking `try_recv`). The decoded frame is applied
        // on the main thread here, so a slow decode never freezes interaction.
        self.poll_decode();

        // R2-MODSWITCH-1 F8: drain a completed background folder scan and apply
        // it atomically (visible loading status until it lands).
        self.poll_scan();

        // PREVIEW-CACHE-FEATURE: drain neighbor-preview worker results (RAM LRU
        // insert + visible failure states) on the main thread; the prefetch
        // itself runs on dedicated background workers, never the IdleQueue.
        self.poll_neighbor_previews(&ctx);

        // LRPAR-G13-MERGE-15: job control for the HDR/panorama merge — the
        // worker thread result is applied on the main thread (status/toast/
        // error + targeted entry refresh), never silently.
        self.poll_merge_job(ctx.input(|i| i.time));

        // Derive `preview_zoom` from the active mode using the geometry cached by
        // the previous frame's `draw_preview`, so the render's ROI crop matches
        // the on-screen zoom (even on the frame a mode button/shortcut fires).
        self.sync_zoom();

        // R3-RENDER-SIZE-1: refresh dpr + rebuild the draft source if the
        // viewport cap changed (resize / display move), before the scheduler.
        self.refresh_preview_cap(ctx.pixels_per_point());

        // PERF-GUI-3/4 + R2-JANK-1 F1/F4 + R2-MODSWITCH-1 F7: the per-frame
        // render scheduling (draft tick, debounce, module-switch deferral) lives
        // in `render_schedule` (file-size ratchet).
        // R3-WARMUP-1: the one-shot cold-start warmup runs on the existing
        // background paths while the UI is idle (armed by the native entry
        // point). It runs before the scheduler so a render it arms is committed
        // by the existing debounce path in the same frame.
        self.maybe_run_startup_warmup(&ctx);
        self.schedule_render(&ctx);

        // F-103-N6-DND-PERSIST-01: native path drops share asynchronous
        // decode/sidecar handling but defer navigation. Bytes are requested only
        // when an integration genuinely supplies no filesystem path.
        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            self.accept_dropped_file(file.path(), || file.bytes());
        }

        // Top: brand + status/error.
        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lumina");
                ui.separator();
                // GUI-DEBUG-SWEEP-1: the render hash is an internal key and is
                // no longer painted as header text. It stays available as a
                // tooltip on the status line (UX-SLICE-1/2's "hash in the
                // status line" is narrowed to "hash in the status tooltip").
                let status_response = ui.label(&self.status);
                if let Some(hash) = self.render_hash_tooltip() {
                    status_response.on_hover_text(hash);
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });

        // Top: module bar (Library / Develop / Export) + the Before/After toggle.
        // The histogram lives in its own collapsible Develop-panel section
        // (GUI-HISTOGRAM-1), not in the module bar. The module
        // labels advertise their Lightroom keyboard shortcuts (`G`, `D`).
        egui::Panel::top("modules").show(ui, |ui| self.draw_module_bar(ui));

        // Left: Lightroom-like Library folder tree. Develop/Export leave the
        // left edge to the navigator/preview working area. Hidden under `Tab`
        // panels-hide, `Shift+Tab` all-panels-hide, `L` lights-out and `F`
        // fullscreen (Welle 2/3, G-11); the
        // header/module bar stay so status and errors remain visible.
        if self.active_module == Module::Library && !self.side_chrome_hidden() {
            egui::Panel::left("folders")
                .resizable(true)
                .default_size(220.0)
                .show(ui, |ui| self.draw_folder_tree(ui));
        }

        // Left: Develop left rail (Navigator + Presets + Snapshots + History)
        // or the plain Navigator panel (Export). The Library module keeps its
        // text file-browser on the left instead (R4-UX-1: no duplicate rail;
        // the bottom filmstrip is the single selection surface everywhere).
        if self.navigator_open
            && !matches!(self.active_module, Module::Library)
            && !self.side_chrome_hidden()
        {
            self.draw_left_rail_panel(&ctx, ui);
        }

        // Right: Develop controls (eight sections), the Library Metadata
        // panel (LRPAR-G15-IPTC-S8, SOLL §10), or nothing extra for
        // Export (placeholder shown centrally). Hidden under
        // `Tab`/`Shift+Tab`/`L`/`F` like the left panels.
        if !self.side_chrome_hidden() {
            egui::Panel::right("controls")
                .resizable(true)
                .default_size(320.0)
                .show(ui, |ui| match self.active_module {
                    Module::Develop => self.draw_develop_panel(ui),
                    Module::Library => {
                        // UX-SLICE-1 (Layout-Bruch, mapper P4): a vertical `ScrollArea`
                        // shrinks horizontally to its content by default, so on
                        // the first frame the resizable panel frame anchored to
                        // the wrong edge and left a transparent/white strip to
                        // its right. `auto_shrink([false, _])` makes the scroll
                        // area claim the panel width, so the panel fills its
                        // `default_size` on every frame.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                self.draw_library_metadata_panel(ui);
                            });
                    }
                    Module::Export => {
                        self.draw_export_panel(ui);
                    }
                });
        }

        // Bottom: filmstrip in all three modules (F-100, see
        // `shows_filmstrip`). Generated thumbnails are produced by the
        // background worker pool (miss -> background job).
        // `L` lights-out and `F` fullscreen hide it (Welle 2/3); `Tab`
        // panels-hide keeps it.
        if self.shows_filmstrip() {
            egui::Panel::bottom("filmstrip").show(ui, |ui| self.draw_filmstrip(&ctx, ui));
        }

        // Central: the large preview/navigator. The Export module shows the
        // current render (what will be exported); the controls live in the
        // right-side Export panel.
        egui::CentralPanel::default().show(ui, |ui| match self.active_module {
            Module::Export => {
                self.draw_preview_area(&ctx, ui);
            }
            // Library: Lightroom-like grid view (folders tree left, RAW
            // thumbnail grid center); Develop/Export keep the large preview.
            Module::Library => {
                self.draw_library_grid(&ctx, ui);
            }
            _ => self.draw_preview_area(&ctx, ui),
        });
        // GUI-TOAST-OVERLAP-1: transient overlay toast (own Area, auto-dismiss
        // + manual ✕) — drawn last so it floats above the panels without
        // taking layout width.
        self.update_toast(&ctx);
        self.draw_toast(&ctx);
        // KITTEST-COVERAGE-STATES-1: error popup dialog (own floating window,
        // Close dismisses). Drawn with the other floating overlays.
        self.draw_error_dialog(&ctx);
        // LRPAR-G15-IPTC-S8: dynamic-preset prompt dialog (floating window,
        // Cancel discards without touching any sidecar).
        self.draw_meta_preset_dialog(&ctx);
        self.note_first_paint_after_switch();
        if let Some(t0) = perf_t0 {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            // GUI-SCROLL-200-1: `slow_frame` flags every frame over the 16.7 ms
            // (60 Hz) budget so scrolling spikes are greppable while thumbnail
            // jobs run; `thumb_jobs_enqueued`/`thumbs_ready` correlate a spike
            // with same-frame thumbnail work.
            let slow_frame = ms > 16.7;
            let counters = (
                self.frame_thumb_enqueued,
                self.frame_thumbs_ready,
                self.frame_previews_enqueued,
                self.frame_previews_ready,
            );
            log::info!(
                "LUMINA_PERF frame={:.2}ms pointer_down={} thumb_jobs_enqueued={} thumbs_ready={} neighbor_previews_enqueued={} neighbor_previews_ready={} slow_frame={}",
                ms,
                ctx.input(|i| i.pointer.any_down()),
                counters.0,
                counters.1,
                counters.2,
                counters.3,
                slow_frame
            );
            eprintln!(
                "LUMINA_PERF frame={:.2}ms thumb_jobs_enqueued={} thumbs_ready={} neighbor_previews_enqueued={} neighbor_previews_ready={} slow_frame={}",
                ms, counters.0, counters.1, counters.2, counters.3, slow_frame
            );
        }
    }
}
