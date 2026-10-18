//! F-103-N5: Export module — target resolution, write guards, `export_to`,
//! and the Export panel.
//!
//! The export path is byte-identical to the CLI: it renders the current
//! recipe through the *same* `lumina_core::export_image` function (render +
//! encode) and writes the artifact through the *same* `lumina_sidecar::
//! write_atomically` helper. No encode logic is duplicated in the GUI.

use super::*;

impl LuminaApp {
    /// Resolve the effective export target and enforce the non-destructive
    /// write guards (REVIEW-GUI-EXPORT-1). The format extension is applied
    /// **first**, then the final path is checked against the loaded source and
    /// its persistent artefacts (`<source>.lumina.json` sidecar and
    /// `<source>.lumina.zdata` mask bundle) so an export can never overwrite
    /// the original or its sidecar data — e.g. target `/d/photo` with format
    /// PNG must be refused when `/d/photo.png` is the loaded source, which a
    /// pre-extension check would miss. Pure helper, unit-tested headless.
    pub(crate) fn resolve_export_target(
        source: &str,
        output: PathBuf,
        extension: &str,
    ) -> Result<PathBuf, GuiError> {
        let output = output.with_extension(extension);
        if source.trim().is_empty() {
            return Ok(output);
        }
        let source = Path::new(source);
        let sidecar = lumina_sidecar::sidecar_path_for(source);
        let zdata = lumina_sidecar::zdata_path_for(source);
        // `zdata_path_for` requires the sidecar crate's `zdata` feature, which
        // the GUI enables.
        let protected: Vec<(&Path, &str)> = vec![
            (source, "the original image"),
            (sidecar.as_path(), "its sidecar"),
            (zdata.as_path(), "its mask bundle"),
        ];
        for (protected_path, kind) in &protected {
            if Self::paths_resolve_equal_symmetric(protected_path, &output)
                .map_err(|error| GuiError::Io(error.to_string()))?
            {
                return Err(GuiError::Io(format!(
                    "export target {} resolves to {}, {}; refusing to overwrite it",
                    output.display(),
                    kind,
                    protected_path.display()
                )));
            }
        }
        Ok(output)
    }

    /// Same-path check that tolerates either side not existing yet
    /// (`lumina_sidecar::paths_resolve_equal` canonicalizes its first argument,
    /// which fails ENOENT for a sidecar/zdata artefact that was never written).
    /// Existing paths are canonicalized directly; a missing path is resolved
    /// against its parent directory so name collisions are still caught.
    fn paths_resolve_equal_symmetric(a: &Path, b: &Path) -> std::io::Result<bool> {
        let resolve = |path: &Path| -> std::io::Result<std::path::PathBuf> {
            if path.exists() {
                std::fs::canonicalize(path)
            } else {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                Ok(std::fs::canonicalize(parent)?.join(path.file_name().unwrap_or_default()))
            }
        };
        Ok(resolve(a)? == resolve(b)?)
    }

    /// Export the currently loaded image to `output` using the shared render +
    /// encode chain. The output file extension is forced to the format's
    /// canonical extension so the chosen format (not a typed extension) is
    /// authoritative — mirroring the CLI's `output.with_extension(...)`.
    ///
    /// The original source is never overwritten: if `self.path` is set and
    /// resolves equal to `output`, the export is rejected as a [`GuiError`]
    /// (no silent fallback). The artifact is written atomically; the declarative
    /// recipe/sidecar is left untouched by the export itself (the user saves
    /// the recipe explicitly via "Save Recipe / Sidecar").
    pub fn export_to(&mut self, output: PathBuf) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::Export);
        // G-06: read the source dimensions BEFORE borrowing `original` —
        // the Lensfun cache refresh below needs `&mut`.
        let (export_w, export_h) = self
            .original
            .as_ref()
            .map(|frame| (frame.width(), frame.height()))
            .unwrap_or((0, 0));
        // LRPAR-G14-DENOISE-IMPL-20: loud export gate before any work. The
        // shared export entry point has no denoise-stage input yet, so a
        // `ready` stage or a `Strict` non-ready stage refuses visibly instead
        // of exporting a frame that diverges from the preview; a `Warn`
        // fallback is surfaced (log + status below), never silent.
        if let Err(error) = self.guard_denoise_export() {
            let message = error.to_string();
            error!("export refused: {message}");
            self.show_error(message);
            return Err(error);
        }
        // GUI-SRCACC-1: resolve strict source actions before generative inputs,
        // target encoding, or writing. Generative identities then observe the
        // same post-action pixels as preview and the shared export pipeline.
        let source_actions = {
            let original = self
                .original
                .as_ref()
                .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
            match self.resolve_current_source_actions(&self.recipe, original) {
                Ok(actions) => actions,
                Err(error) => {
                    self.invalidate_source_action_preview();
                    return Err(GuiError::Io(error.to_string()));
                }
            }
        };
        // GEN-ONNX-1 Welle 2b: resolve the generative canvases before any
        // shared borrow of `self` (the resolver needs `&mut self` for the
        // corrector cache). Only done when a generative role is active, so a
        // plain export never pays for the clone.
        let generative = if self.generative_stage_active() {
            let original = self
                .original
                .clone()
                .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
            self.resolve_generative_artifacts(&original, &source_actions)?
        } else {
            GenerativeArtifacts::default()
        };
        // G-06: Lensfun auto-corrector for the exported source (same
        // cached lookup as the preview render — export and preview share
        // the correction, no second pipeline).
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(export_w, export_h);
        #[cfg(feature = "lensfun")]
        let export_lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let export_lensfun = None;
        let Some(original) = self.original.as_ref() else {
            return Err(GuiError::Io(
                "No image loaded; open or drop an image first".into(),
            ));
        };
        let format = self.export_format;
        let quality = self.export_quality;
        // Apply the format extension FIRST, then enforce the non-destructive
        // write guards against the resolved target (REVIEW-GUI-EXPORT-1).
        let output = Self::resolve_export_target(
            &self.path,
            output,
            self.export_format.default_extension(),
        )?;
        let options = ExportOptions {
            format,
            quality,
            dither: false,
            ..Default::default()
        };
        options.validate().map_err(GuiError::Core)?;
        // Build the identical render context used by `render()` (what the user
        // currently sees) so the export matches the preview.
        let masks_context = {
            let planes = self.load_mask_planes();
            self.document.as_ref().and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                    .map(|_| MaskContext {
                        copies: &document.virtual_copies,
                        active_copy_id: &self.virtual_copy_id,
                        planes,
                        policy: MaskPolicy::Warn,
                        source_roi: None,
                    })
            })
        };
        // G-06: Lensfun auto-corrector for the exported source (same
        // cached lookup as the preview render — export and preview share
        // the correction, no second pipeline).
        let context = RenderContext {
            recipe: &self.recipe,
            camera_white_balance: self.camera_white_balance,
            source_actions: source_actions.artifacts(),
            masks: masks_context,
            lensfun: export_lensfun,
            depth: None,
        };
        // GEN-ONNX-1 Welle 2b: `export_image_with_generative` renders via the
        // shared artifact-aware path (`Lens → [auto-fill] → Perspective →
        // [expand] → Crop`). No post-render expand runs here — a second expand
        // would fail `validate_with_source` (canvas no longer larger) and abort
        // the export.
        let encoded = export_image_with_generative(original, &context, options, generative.input())
            .map_err(GuiError::Core)?;
        lumina_sidecar::write_atomically(&output, &encoded).map_err(GuiError::Sidecar)?;
        self.error = None;
        self.status = format!(
            "Exported {} ({:?} @ q{})",
            output.display(),
            format,
            quality
        );
        Ok(())
    }

    /// Suggested export file name derived from the source name and the selected
    /// format (e.g. `photo.jpg` from `photo.png`). Used to prefill the save
    /// dialog and the path field.
    fn suggested_export_name(&self) -> String {
        let base = if self.source_name.is_empty() {
            "export".to_string()
        } else {
            self.source_name.clone()
        };
        let stem = Path::new(&base)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "export".to_string());
        format!("{}.{}", stem, self.export_format.default_extension())
    }

    /// Draw the Export module controls.
    pub(crate) fn draw_export_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Export.t());
        ui.label(Str::ExportTarget.t());
        // GUI-VISION-1: button-first (right-to-left) row so the Choose button
        // keeps its natural width at the panel edge and the field takes the
        // rest. An unbounded edit claimed the full row and pushed the button
        // past the panel edge (kittest `export_module` golden).
        //
        // KITTEST-COVERAGE-STATES-1: the `right_to_left(Align::Center)` layout
        // must live inside a `ui.horizontal` row. Without it the layout used
        // the whole remaining panel height as its cross axis and vertically
        // centred the row, pushing Format / Quality / Export below the fold
        // (confirmed: the quality label laid out at y=722..784 of a 720px
        // viewport). The `horizontal` wrapper constrains the cross axis to the
        // row, so the destination field sits directly under its label and all
        // controls stay pixel-visible.
        let mut choose_clicked = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                choose_clicked = ui.button(Str::ExportChoose.t()).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.text_edit_singleline(&mut self.export_path);
                });
            });
        });
        if choose_clicked {
            let suggested = self.suggested_export_name();
            if let Some(path) = rfd::FileDialog::new().set_file_name(&suggested).save_file() {
                self.export_path = path.display().to_string();
            }
        }
        ui.horizontal(|ui| {
            ui.label(Str::ExportFormatLabel.t());
            egui::ComboBox::from_label("")
                .selected_text(format_label(self.export_format))
                .show_ui(ui, |ui| {
                    for (candidate, label) in [
                        (ImageFileFormat::Png, "PNG"),
                        (ImageFileFormat::Jpeg, "JPEG"),
                        (ImageFileFormat::WebP, "WebP"),
                    ] {
                        if ui
                            .selectable_label(self.export_format == candidate, label)
                            .clicked()
                        {
                            self.export_format = candidate;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(Str::ExportQualityLabel.t());
            let mut quality = self.export_quality as f64;
            if ui
                .add(egui::Slider::new(&mut quality, 1.0..=100.0).show_value(true))
                .changed()
            {
                self.export_quality = quality as u8;
            }
        });
        if self.export_format == ImageFileFormat::Png {
            ui.label(Str::ExportQualityUnused.t());
        }
        ui.horizontal(|ui| {
            if ui.button(Str::ExportUseSuggested.t()).clicked() {
                self.export_path = self.suggested_export_name();
            }
            if ui.button(Str::ExportRun.t()).clicked() {
                let path = PathBuf::from(self.export_path.trim());
                if path.as_os_str().is_empty() {
                    self.show_error(GuiError::Io("Choose an export target first".into()));
                } else if let Err(error) = self.export_to(path) {
                    error!("export failed to {}: {error}", self.export_path.trim());
                    self.show_error(error);
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        ui.label(&self.status);
    }
}
