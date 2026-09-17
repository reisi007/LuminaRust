//! LRPAR-G13-MERGE-15 — GUI slice: HDR/Panorama merge actions + DNG status.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Persistenz, §Abnahmekriterien „GUI-headless") and
//! `feature/platform/cli-gui-wasm.md` § „HDR-/Panorama-Merge (G-13)".
//!
//! ## Same shared path as the CLI (F6 dedup)
//!
//! The GUI owns **no** merge image logic. The complete step sequence
//! (align → blend → recipe → digest → envelope/bundle gate → linear DNG →
//! atomic sidecar/DNG publication) lives exactly once in
//! [`lumina_merge::bundle::run_merge`], which the CLI commands call too. This
//! module only supplies the GUI decode adapter (`decode_selection_frame`) and
//! the GUI exposure policy (`Option<&[MergeExposure]>`) plus the job control
//! (`start_merge`/`poll_merge_job`) and the visible status.
//!
//! ## Envelope conflicts are loud
//!
//! A merge writes `<ref>-HDR.dng`/`<ref>-Pano.dng` next to the reference
//! source. If the target sidecar exists and is **not** a merge envelope
//! (`"type": "merge"`), if its stored recipe digest no longer matches the
//! inputs, or if it is unreadable, the merge refuses loudly — only the
//! explicit `force` re-merge replaces it. Nothing is overwritten silently.
//!
//! ## Golden gates (decision: hash/re-import anchors instead of kittest PNG)
//!
//! A merge changes a *data artifact* (linear DNG bytes), not the GUI layout;
//! a kittest PNG golden would only pin the panel chrome and could not detect a
//! merge regression. The golden gates here are therefore **data anchors**
//! (documented tolerance `lumina_merge::FLOAT_TOLERANCE_DOC` for cross-toolchain
//! floats): same inputs → byte-identical DNG (same machine), the DNG BLAKE3
//! equals the sidecar artifact checksum, and a re-import through the pinned
//! LibRaw decoder reproduces the written geometry. This is the documented
//! decision (kittest-PNG unsuitable for a DNG/algorithms change).

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use log::{error, info, warn};
use lumina_merge::bundle::{
    linear_from_rgba, merge_checksum, run_merge, MergeExif, MergeRunError, MergeRunOptions,
    MergeSourceFrame,
};
use lumina_merge::LinearImage;
use lumina_sidecar::{
    load_sidecar, sidecar_path_for, MergeDecodeContext, MergeExposure, MergeMode, MIN_MERGE_SOURCES,
};

pub use lumina_merge::bundle::{
    classify_merge_document, stored_merge_artifact, stored_merge_recipe, MergeDocumentKind,
    MergeRunOutcome as MergeOutcome, PANO_DEFAULT_EXPOSURE_S, PANO_DEFAULT_F_NUMBER,
    PANO_DEFAULT_ISO,
};

use crate::i18n::Str;
#[cfg(debug_assertions)]
use crate::GuiAction;
use crate::GuiError;

/// Visible status of a persisted merge bundle (decision §Persistenz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeArtifactStatus {
    /// No merge bundle at the target (nothing to report).
    NoBundle,
    /// Source hashes, decode contexts and the DNG checksum match.
    Ok,
    /// The bundle exists but an input changed (only `force` re-merges).
    Stale,
    /// A bundle part is gone (DNG or sidecar).
    Missing,
    /// The sidecar is not a merge envelope / unreadable / the stored recipe is
    /// invalid (a hard, loud state — never overwritten silently).
    Unsupported,
}

impl MergeArtifactStatus {
    /// Visible status text (status line + panel).
    pub fn text(self) -> &'static str {
        match self {
            MergeArtifactStatus::NoBundle => Str::MergeStatusNone.t(),
            MergeArtifactStatus::Ok => Str::MergeStatusOk.t(),
            MergeArtifactStatus::Stale => Str::MergeStatusStale.t(),
            MergeArtifactStatus::Missing => Str::MergeStatusMissing.t(),
            MergeArtifactStatus::Unsupported => Str::MergeStatusUnsupported.t(),
        }
    }
}

/// Running merge job (background thread + result channel). Polled by
/// `LuminaApp::poll_merge_job` in the frame loop — job control with a visible
/// status instead of a frozen UI.
pub struct MergeJob {
    pub mode: MergeMode,
    rx: mpsc::Receiver<Result<MergeOutcome, String>>,
}

impl std::fmt::Debug for MergeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeJob")
            .field("mode", &self.mode)
            .finish()
    }
}

/// RGBA8 (`0..=255`, alpha ignored) → linear `f32` RGB frame. Delegates to the
/// shared [`linear_from_rgba`]; pure and loud on a malformed buffer (`None`).
pub fn linear_from_frame(frame: &lumina_core::ImageFrame) -> Option<LinearImage> {
    linear_from_rgba(frame)
}

/// Maps the RAW metadata onto the neutral [`MergeExif`] subset (missing fields
/// stay `None`; validity is enforced by the accessors).
fn merge_exif_from_raw(raw: Option<&lumina_raw::RawMetadata>) -> MergeExif {
    raw.map_or_else(MergeExif::default, |meta| MergeExif {
        camera_make: meta.camera_make.clone(),
        camera_model: meta.camera_model.clone(),
        lens: meta.lens.clone(),
        exposure_time_s: meta.shutter,
        f_number: meta.aperture,
        iso: meta.iso,
        timestamp: meta.timestamp,
    })
}

/// GUI decode adapter for the shared orchestration: one source file → linear
/// frame + decode context + EXIF subset. Loud on missing/undecodable input.
fn decode_source(input: &Path) -> Result<MergeSourceFrame, MergeRunError> {
    let (bytes, frame, orientation) = crate::decode_selection_frame(input).map_err(|error| {
        if error.contains("No such file") || error.contains("missing") {
            MergeRunError::Missing(error)
        } else {
            MergeRunError::Unsupported(error)
        }
    })?;
    let content_hash = merge_checksum(&bytes);
    let raw_metadata = lumina_raw::read_metadata(input).ok();
    let decode_context = MergeDecodeContext {
        decoder: if raw_metadata.is_some() {
            "libraw".into()
        } else {
            "image".into()
        },
        decode_version: if raw_metadata.is_some() {
            lumina_raw::libraw_decode_version()
        } else {
            env!("CARGO_PKG_VERSION").into()
        },
        orientation,
    };
    let linear = linear_from_frame(&frame).ok_or_else(|| {
        MergeRunError::Unsupported(format!(
            "source `{}` has {} bytes for a {}x{} RGBA8 frame",
            input.display(),
            frame.pixels.len(),
            frame.width,
            frame.height
        ))
    })?;
    Ok(MergeSourceFrame {
        path: input.to_path_buf(),
        content_hash,
        frame: linear,
        decode_context,
        exif: merge_exif_from_raw(raw_metadata.as_ref()),
    })
}

/// GUI exposure policy: explicit per-source value wins unchanged; otherwise
/// the RAW EXIF exposure; HDR without either is `unsupported` (never guessed
/// from pixels); panorama stores EXIF or the documented neutral default as
/// provenance only.
fn resolve_exposure(
    mode: MergeMode,
    index: usize,
    explicit: Option<&[MergeExposure]>,
    source: &MergeSourceFrame,
) -> Result<MergeExposure, MergeRunError> {
    if let Some(exposure) = explicit.and_then(|values| values.get(index)) {
        return Ok(exposure.clone());
    }
    let from_exif = || source.exif.exposure();
    match mode {
        MergeMode::Hdr => from_exif().ok_or_else(|| {
            MergeRunError::Unsupported(format!(
                "HDR source #{index} has no EXIF exposure \
                 (exposure is never guessed from pixels)"
            ))
        }),
        MergeMode::Panorama => Ok(from_exif().unwrap_or(MergeExposure {
            exposure_time_s: PANO_DEFAULT_EXPOSURE_S,
            iso: PANO_DEFAULT_ISO,
            f_number: PANO_DEFAULT_F_NUMBER,
        })),
    }
}

/// Runs one merge synchronously through the shared orchestration (the job
/// thread calls this; headless tests call it directly for deterministic
/// results).
///
/// `exposures` mirrors the CLI `--exposure-times/--isos/--f-numbers` triple;
/// `None` resolves per-source EXIF (HDR: loud `unsupported` without it).
pub fn run_merge_sync(
    mode: MergeMode,
    inputs: &[PathBuf],
    exposures: Option<&[MergeExposure]>,
    force: bool,
    max_shift_px: i32,
    blend_width_px: u32,
) -> Result<MergeOutcome, String> {
    if let Some(values) = exposures {
        if values.len() != inputs.len() {
            return Err(format!(
                "invalid exposure list: expected {} values, got {}",
                inputs.len(),
                values.len()
            ));
        }
    }
    let options = MergeRunOptions {
        output: None,
        max_shift_px,
        blend_width_px,
        force,
        dng_decode_version: lumina_raw::libraw_decode_version(),
    };
    let resolve = |mode: MergeMode, index: usize, source: &MergeSourceFrame| {
        resolve_exposure(mode, index, exposures, source)
    };
    let outcome = run_merge(mode, inputs, &options, decode_source, resolve)
        .map_err(|error| error.to_string())?;
    if outcome.residual_warning {
        warn!(
            "merge: aligned with residual {:.2}px (above the documented threshold)",
            outcome.residual_px
        );
    }
    Ok(outcome)
}

/// Reads the visible status of a merge bundle at `dng_path` (decision
/// §Persistenz). Pure I/O + validation; never rewrites anything.
pub fn merge_bundle_status(dng_path: &Path) -> (MergeArtifactStatus, String) {
    let sidecar_path = sidecar_path_for(dng_path);
    if !sidecar_path.exists() {
        return if dng_path.exists() {
            (
                MergeArtifactStatus::Missing,
                format!(
                    "merge DNG `{}` exists without its sidecar",
                    dng_path.display()
                ),
            )
        } else {
            (MergeArtifactStatus::NoBundle, String::new())
        };
    }
    let document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(error) => {
            return (
                MergeArtifactStatus::Unsupported,
                format!("merge sidecar unreadable: {error}"),
            )
        }
    };
    match classify_merge_document(&document) {
        MergeDocumentKind::Merge => {}
        MergeDocumentKind::Standard => {
            return (
                MergeArtifactStatus::Unsupported,
                "sidecar is a standard sidecar (no merge envelope)".into(),
            )
        }
        MergeDocumentKind::Unknown(kind) => {
            return (
                MergeArtifactStatus::Unsupported,
                format!("unknown document type `{kind}`"),
            )
        }
    }
    let recipe = match stored_merge_recipe(&document) {
        Ok(recipe) => recipe,
        Err(reason) => return (MergeArtifactStatus::Unsupported, reason),
    };
    let artifact = match stored_merge_artifact(&document) {
        Ok(artifact) => artifact,
        Err(reason) => return (MergeArtifactStatus::Unsupported, reason),
    };
    let Ok(dng_bytes) = std::fs::read(dng_path) else {
        return (MergeArtifactStatus::Missing, "merge DNG is gone".into());
    };
    if merge_checksum(&dng_bytes) != artifact.checksum {
        return (
            MergeArtifactStatus::Stale,
            "merge DNG checksum changed on disk".into(),
        );
    }
    // Source hashes are relative to the bundle directory.
    let bundle_dir = dng_path.parent().unwrap_or_else(|| Path::new("."));
    for source in &recipe.sources {
        let source_path = bundle_dir.join(&source.path);
        let Ok(bytes) = std::fs::read(&source_path) else {
            return (
                MergeArtifactStatus::Missing,
                format!("source `{}` is missing", source.path),
            );
        };
        if merge_checksum(&bytes) != source.content_hash {
            return (
                MergeArtifactStatus::Stale,
                format!("source `{}` changed (hash mismatch)", source.path),
            );
        }
    }
    (MergeArtifactStatus::Ok, String::new())
}

impl crate::LuminaApp {
    /// Selected merge sources (filmstrip selection, else the loaded image), in
    /// the deterministic sorted path order (reference = first).
    pub fn merge_sources(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.filmstrip_selection.iter().map(PathBuf::from).collect();
        if paths.is_empty() && !self.path.is_empty() {
            paths.push(PathBuf::from(&self.path));
        }
        paths.sort();
        paths
    }

    /// Whether a merge job is currently running.
    pub fn merge_running(&self) -> bool {
        self.merge_job.is_some()
    }

    /// Last merge outcome text (status line).
    pub fn merge_status_text(&self) -> &str {
        &self.merge_status
    }

    /// Starts a merge on the current selection as a background job (job
    /// control: the UI stays responsive; `poll_merge_job` reports the result).
    pub fn start_merge(&mut self, mode: MergeMode) -> Result<(), GuiError> {
        #[cfg(debug_assertions)]
        let _gui_action_timer = self.begin_gui_action(GuiAction::StartMerge);
        if self.merge_job.is_some() {
            return Err(GuiError::Io(Str::MergeAlreadyRunning.t().to_string()));
        }
        let inputs = self.merge_sources();
        if inputs.len() < MIN_MERGE_SOURCES {
            return Err(GuiError::Io(
                Str::MergeNeedsSelection.format_arg(&MIN_MERGE_SOURCES.to_string()),
            ));
        }
        let command = match mode {
            MergeMode::Hdr => "merge-hdr",
            MergeMode::Panorama => "merge-pano",
        };
        info!("{command}: GUI job started for {} source(s)", inputs.len());
        let (tx, rx) = mpsc::channel();
        let job_inputs = inputs.clone();
        std::thread::spawn(move || {
            let result = run_merge_sync(mode, &job_inputs, None, false, 16, 64);
            let _ = tx.send(result);
        });
        self.merge_job = Some(MergeJob { mode, rx });
        self.merge_status = Str::MergeRunningPattern.format_arg(command).to_string();
        self.status = self.merge_status.clone();
        Ok(())
    }

    /// Polls the running merge job (called once per frame). Completion updates
    /// the status line, raises a loud error dialog on failure and refreshes the
    /// directory so the new DNG appears in the Library.
    pub fn poll_merge_job(&mut self, now: f64) {
        let Some(job) = &self.merge_job else {
            return;
        };
        let mode = job.mode;
        match job.rx.try_recv() {
            Ok(Ok(outcome)) => {
                self.merge_job = None;
                let message = if outcome.cached {
                    Str::MergeCurrentPattern.format_arg(&outcome.dng_path.display().to_string())
                } else {
                    Str::MergeDonePattern.format_arg(&outcome.dng_path.display().to_string())
                };
                self.merge_status = message.clone();
                self.status = message.clone();
                self.show_toast(message, now);
                // The new DNG is a supported source: a targeted single-file
                // refresh lists it without a full rescan.
                self.refresh_entry(&outcome.dng_path);
                info!(
                    "merge job finished: mode={mode:?} dng={} cached={} digest={}",
                    outcome.dng_path.display(),
                    outcome.cached,
                    outcome.digest
                );
            }
            Ok(Err(message)) => {
                self.merge_job = None;
                self.merge_status = message.clone();
                error!("merge job failed: {message}");
                self.show_error(message);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.merge_job = None;
                let message = Str::MergeFailed.t().to_string();
                self.merge_status = message.clone();
                error!("merge job thread disconnected before reporting a result");
                self.show_error(message);
            }
        }
    }

    /// Visible merge panel (Library metadata column): mode actions, job state,
    /// selection count and the merge-bundle status of the loaded image.
    pub(crate) fn draw_merge_section(&mut self, ui: &mut crate::egui::Ui) {
        // Collapsed by default like every other Library sub-section (the
        // panel's 320 px default width only carries the narrow header).
        ui.collapsing(Str::MergeSection.t(), |ui| {
            let selected = self.merge_sources().len();
            ui.add(
                crate::egui::Label::new(
                    Str::MergeSelectionPattern.format_arg(&selected.to_string()),
                )
                .wrap(),
            );
            let running = self.merge_running();
            // Stacked (not a wide row) so the panel width stays stable.
            ui.add_enabled_ui(!running, |ui| {
                if ui.button(Str::MergeHdr.t()).clicked() {
                    if let Err(error) = self.start_merge(MergeMode::Hdr) {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::MergePano.t()).clicked() {
                    if let Err(error) = self.start_merge(MergeMode::Panorama) {
                        self.show_error(error);
                    }
                }
            });
            if !self.merge_status.is_empty() {
                ui.add(
                    crate::egui::Label::new(
                        Str::MergeStatusPattern.format_arg(&self.merge_status.clone()),
                    )
                    .wrap(),
                );
            }
            // Bundle status of the loaded image (if it is a merge DNG).
            if !self.path.is_empty() {
                let (status, reason) = merge_bundle_status(Path::new(&self.path));
                if status != MergeArtifactStatus::NoBundle {
                    ui.add(
                        crate::egui::Label::new(Str::MergeBundlePattern.format_arg(status.text()))
                            .wrap(),
                    );
                    if !reason.is_empty() {
                        ui.add(crate::egui::Label::new(reason).wrap());
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LuminaApp;
    use lumina_core::{ImageFileFormat, ImageFrame};
    use lumina_sidecar::{ArtifactReference, MergeAlignmentMethod, MergeProjection};

    fn new_app() -> LuminaApp {
        LuminaApp::new(crate::egui::Context::default())
    }

    /// Synthetic 64×48 RGB frame encoded as PNG (brightness `level`). The
    /// gradient gives the alignment a non-flat signal.
    fn write_png(path: &Path, level: u8) {
        let pixels: Vec<u8> = (0..64 * 48)
            .flat_map(|i| {
                let x = (i % 64) as u8;
                let value = level.saturating_add(x % 16);
                [value, value, value, 255]
            })
            .collect();
        let png = ImageFrame::new(64, 48, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    fn exposure(t: f64) -> Vec<MergeExposure> {
        vec![
            MergeExposure {
                exposure_time_s: t,
                iso: 100,
                f_number: 8.0,
            },
            MergeExposure {
                exposure_time_s: t * 4.0,
                iso: 100,
                f_number: 8.0,
            },
        ]
    }

    /// Golden gate (HDR): hash anchor + sidecar envelope + deterministic
    /// re-run + LibRaw re-import. The recipe decode context is the shared
    /// raster contract, so the same inputs produce the same digest on the CLI
    /// path (F6 parity anchor).
    #[test]
    fn hdr_merge_writes_dng_bundle_and_reimports() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("shot_a.png");
        let b = dir.path().join("shot_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let exposures = exposure(0.004);
        let inputs = vec![a.clone(), b.clone()];
        let outcome =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        assert!(!outcome.cached);
        assert_eq!(outcome.dng_path, dir.path().join("shot_a-HDR.dng"));
        assert!(outcome.dng_path.is_file());
        assert!(outcome.sidecar_path.is_file());
        let dng_bytes = std::fs::read(&outcome.dng_path).unwrap();
        assert_eq!(merge_checksum(&dng_bytes), outcome.checksum, "hash anchor");

        let document = load_sidecar(&outcome.sidecar_path).unwrap();
        assert_eq!(classify_merge_document(&document), MergeDocumentKind::Merge);
        let artifact: ArtifactReference = serde_json::from_value(
            document.extras[lumina_merge::bundle::MERGE_ARTIFACT_KEY].clone(),
        )
        .unwrap();
        assert_eq!(
            artifact.checksum, outcome.checksum,
            "sidecar artifact checksum"
        );
        assert_eq!((artifact.width, artifact.height), (64, 48));
        assert_eq!(artifact.channels, "rgb16");
        assert_eq!(
            (document.virtual_copies[0].recipe.denoise_ai.as_ref(),),
            (None,),
            "merge DNG carries a fresh standard recipe"
        );

        // F6 parity anchor: the persisted recipe digest is the orchestrator
        // digest and every source uses the raster decode contract the CLI
        // adapter produces for the same PNG bytes.
        let recipe = stored_merge_recipe(&document).unwrap();
        assert_eq!(recipe.digest(), outcome.digest);
        for source in &recipe.sources {
            assert_eq!(source.decode_context.decoder, "image");
            assert_eq!(source.decode_context.orientation, 1);
        }

        // Re-import anchor through the pinned decoder.
        let (_, frame, _) = crate::decode_selection_frame(&outcome.dng_path).unwrap();
        assert_eq!((frame.width, frame.height), (64, 48));

        // Deterministic re-run with `force` (same machine → byte-identical).
        let again =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), true, 16, 64).unwrap();
        let again_bytes = std::fs::read(&again.dng_path).unwrap();
        assert_eq!(
            merge_checksum(&again_bytes),
            outcome.checksum,
            "same inputs must reproduce the DNG (tolerance {})",
            lumina_merge::FLOAT_TOLERANCE_DOC
        );

        // Without `force` an already-current bundle is reported cached, not
        // rewritten.
        let cached =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        assert!(cached.cached);
    }

    /// Golden gate (Panorama): overlap geometry, cylindrical projection and the
    /// deterministic digest.
    #[test]
    fn panorama_merge_writes_dng_and_persists_projection() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("pano_a.png");
        let b = dir.path().join("pano_b.png");
        write_png(&a, 90);
        write_png(&b, 90);
        let inputs = vec![a.clone(), b.clone()];
        // Identical frames overlap completely: a valid panorama with no shift.
        let outcome = run_merge_sync(MergeMode::Panorama, &inputs, None, false, 16, 64).unwrap();
        assert!(outcome.dng_path.is_file());
        let document = load_sidecar(&outcome.sidecar_path).unwrap();
        let recipe = stored_merge_recipe(&document).unwrap();
        assert_eq!(recipe.mode, MergeMode::Panorama);
        assert_eq!(recipe.alignment.projection, MergeProjection::Cylindrical);
        assert_eq!(
            recipe.alignment.method,
            MergeAlignmentMethod::PanoCylindricalHomography
        );
        assert_eq!(recipe.sources.len(), 2);
        // Panorama provenance defaults are stored for EXIF-less sources.
        assert!(
            (recipe.sources[0].exposure.exposure_time_s - PANO_DEFAULT_EXPOSURE_S).abs() < 1e-9
        );
        assert!((recipe.sources[0].exposure.f_number - PANO_DEFAULT_F_NUMBER).abs() < 1e-9);
    }

    /// HDR without EXIF and without explicit exposures is `unsupported`
    /// (never guessed from pixels).
    #[test]
    fn hdr_without_exposure_is_unsupported_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("noexif_a.png");
        let b = dir.path().join("noexif_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let error = run_merge_sync(MergeMode::Hdr, &[a.clone(), b.clone()], None, false, 16, 64)
            .unwrap_err();
        assert!(error.contains("unsupported"), "{error}");
        assert!(!dir.path().join("noexif_a-HDR.dng").exists());
        assert!(!sidecar_path_for(&dir.path().join("noexif_a-HDR.dng")).exists());
    }

    /// Envelope conflict: a standard sidecar at the target is refused loudly
    /// and left byte-identical.
    #[test]
    fn standard_sidecar_envelope_conflict_is_loud_and_non_destructive() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("conflict_a.png");
        let b = dir.path().join("conflict_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let inputs = vec![a.clone(), b.clone()];
        let exposures = exposure(0.004);
        // First merge creates the bundle, then we deface the envelope.
        run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        let sidecar = sidecar_path_for(&dir.path().join("conflict_a-HDR.dng"));
        let mut document = load_sidecar(&sidecar).unwrap();
        document.extras.remove("type");
        lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
        let before = std::fs::read(&sidecar).unwrap();
        let error =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap_err();
        assert!(
            error.contains("merge stale") || error.contains("standard sidecar"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(&sidecar).unwrap(),
            before,
            "no silent overwrite"
        );
        // The bundle status reports the conflict visibly.
        let (status, reason) = merge_bundle_status(&dir.path().join("conflict_a-HDR.dng"));
        assert_eq!(status, MergeArtifactStatus::Unsupported);
        assert!(!reason.is_empty());
    }

    /// Status matrix: ok / stale / missing.
    #[test]
    fn bundle_status_reports_ok_stale_missing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("st_a.png");
        let b = dir.path().join("st_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let inputs = vec![a.clone(), b.clone()];
        let exposures = exposure(0.004);
        let outcome =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        assert_eq!(
            merge_bundle_status(&outcome.dng_path).0,
            MergeArtifactStatus::Ok
        );

        // Change a source: stale.
        write_png(&b, 20);
        assert_eq!(
            merge_bundle_status(&outcome.dng_path).0,
            MergeArtifactStatus::Stale
        );
        // Remove the DNG: missing.
        std::fs::remove_file(&outcome.dng_path).unwrap();
        assert_eq!(
            merge_bundle_status(&outcome.dng_path).0,
            MergeArtifactStatus::Missing
        );
    }

    /// DoD §3 (class completeness): every [`MergeArtifactStatus`] variant has a
    /// distinct, non-empty visible text, and `NoBundle` is the quiet state
    /// (empty reason) at a target that carries nothing.
    #[test]
    fn merge_artifact_status_covers_every_variant() {
        let variants = [
            MergeArtifactStatus::NoBundle,
            MergeArtifactStatus::Ok,
            MergeArtifactStatus::Stale,
            MergeArtifactStatus::Missing,
            MergeArtifactStatus::Unsupported,
        ];
        let mut texts: Vec<&str> = variants.iter().map(|status| status.text()).collect();
        assert!(texts.iter().all(|text| !text.is_empty()), "{texts:?}");
        texts.sort_unstable();
        texts.dedup();
        assert_eq!(
            texts.len(),
            variants.len(),
            "status texts must be distinct: {texts:?}"
        );
        assert_eq!(
            MergeArtifactStatus::NoBundle.text(),
            Str::MergeStatusNone.t()
        );
        let dir = tempfile::tempdir().unwrap();
        let (status, reason) = merge_bundle_status(&dir.path().join("absent.dng"));
        assert_eq!(status, MergeArtifactStatus::NoBundle);
        assert!(reason.is_empty(), "`NoBundle` must stay silent: {reason:?}");
    }

    /// Job control: `start_merge` runs in the background and completion is
    /// observable through `poll_merge_job` (no frozen UI, no silent failure).
    #[test]
    fn merge_job_runs_in_background_and_reports_via_poll() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("job_a.png");
        let b = dir.path().join("job_b.png");
        write_png(&a, 90);
        write_png(&b, 90);
        let mut app = new_app();
        app.set_directory(dir.path().display().to_string());
        app.list_directory_flat();
        app.open_file(a.display().to_string());
        // The filmstrip selection is the RAW-only UI selection; this headless
        // test injects exactly the state the UI produces for a selection of
        // two sources (the merge entry point itself is source-format agnostic).
        app.filmstrip_selection =
            std::collections::BTreeSet::from([a.display().to_string(), b.display().to_string()]);
        assert_eq!(app.merge_sources().len(), 2);
        app.start_merge(MergeMode::Panorama).unwrap();
        assert!(app.merge_running());
        // A second start while running is refused loudly.
        assert!(app.start_merge(MergeMode::Panorama).is_err());
        let mut outcome = None;
        for _ in 0..2000 {
            app.poll_merge_job(0.0);
            if !app.merge_running() {
                outcome = Some(app.merge_status_text().to_string());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let message = outcome.expect("merge job must finish");
        assert!(message.contains("job_a-Pano.dng"), "{message}");
        assert!(dir.path().join("job_a-Pano.dng").is_file());
        // The new DNG is listed after completion (targeted entry refresh).
        assert!(app
            .entries()
            .iter()
            .any(|entry| entry.name == "job_a-Pano.dng"));
    }

    /// Too few sources is a loud refusal (no partial panorama/HDR).
    #[test]
    fn merge_needs_at_least_two_sources() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("solo.png");
        write_png(&a, 90);
        let mut app = new_app();
        app.open_file(a.display().to_string());
        let error = app.start_merge(MergeMode::Hdr).unwrap_err();
        assert!(error.to_string().contains("2"), "{error}");
    }

    /// Bundled path safety: sources outside the bundle directory are refused.
    #[test]
    fn outside_bundle_sources_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside/ref.png");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        write_png(&outside, 90);
        let b = dir.path().join("inside.png");
        write_png(&b, 90);
        let error =
            run_merge_sync(MergeMode::Panorama, &[outside, b], None, false, 16, 64).unwrap_err();
        assert!(error.contains("unsupported"), "{error}");
    }

    /// The RGBA8→linear conversion is pure and loud on a malformed buffer.
    #[test]
    fn linear_conversion_is_exact_and_loud() {
        let frame = ImageFrame::new(2, 1, vec![255, 0, 0, 128, 0, 128, 0, 255]).unwrap();
        let linear = linear_from_frame(&frame).unwrap();
        assert_eq!(linear.width(), 2);
        assert_eq!(&linear.pixels()[0..3], &[1.0, 0.0, 0.0]);
        let mut broken = frame.clone();
        broken.pixels.pop();
        assert!(linear_from_frame(&broken).is_none());
    }
}
