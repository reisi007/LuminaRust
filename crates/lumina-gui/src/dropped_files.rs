//! Native dropped-file routing and headless persistence regressions.
//!
//! A native egui drop exposes a real filesystem path. It must therefore use
//! the asynchronous decode/sidecar lineage while deferring target-directory
//! adoption: synchronous byte loading pairs pixels with the wrong lineage, while
//! eager navigation strands A in B's folder if B cannot be decoded.
//!
//! Drop-event latest-wins: every drop handed to [`LuminaApp::accept_dropped_file`]
//! — path-bearing, bytes-bearing, or a failed pathless byte-read — is the newest
//! source request and supersedes any in-flight decode. A newer drop bumps
//! `decode_generation` and replaces (`take`s) the decode receiver, so an older
//! worker result can never land after it (the generation check in `poll_decode`
//! rejects already-staged results; a dropped receiver makes the worker's send a
//! silent no-op). This includes the pathless byte-read failure: it reports
//! loudly via `show_error` and additionally invalidates the in-flight decode
//! instead of leaving it pending behind the error.

use std::path::Path;

use log::warn;

use super::LuminaApp;

impl LuminaApp {
    /// Route one native drop without reading its bytes on the UI thread.
    ///
    /// Path-bearing drops use the shared asynchronous decode/sidecar lineage,
    /// but defer target-directory adoption until the decode succeeds. The
    /// closure is only for an integration that genuinely supplies bytes
    /// without a path; [`LuminaApp::load_bytes`] then detaches the prior file
    /// lineage before adopting the in-memory source.
    pub(super) fn accept_dropped_file(
        &mut self,
        path: &Path,
        read_bytes: impl FnOnce() -> Result<Vec<u8>, String>,
    ) {
        if !path.as_os_str().is_empty() {
            self.open_file_deferred(path.to_string_lossy().into_owned());
            return;
        }

        match read_bytes() {
            Ok(bytes) => {
                if let Err(error) = self.load_bytes(bytes, "dropped-image") {
                    self.show_error(error);
                }
            }
            Err(read_error) => {
                // Drop-event latest-wins (see module docs): this failed drop is
                // the newest source request, so it supersedes any in-flight
                // path decode exactly like a successful `load_bytes` would —
                // bump the generation and drop the receiver so the older worker
                // result can never land after this failure; pending anchors
                // are cleared while the previously loaded source remains.
                if self.decode_rx.take().is_some() {
                    self.note_decode_failed();
                }
                self.decode_generation += 1;
                self.pending_load_path = None;
                self.pending_directory_open = None;
                warn!("pathless dropped file could not be read: {read_error}");
                self.show_error(format!("dropped file unreadable: {read_error}"));
            }
        }
    }
}

#[cfg(test)]
mod decode_state_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    use lumina_core::{ImageFileFormat, ImageFrame};
    use lumina_sidecar::{EditRecipe, SidecarDocument};
    use std::path::{Path, PathBuf};

    #[derive(Debug, PartialEq, Eq)]
    struct NavigationSnapshot {
        directory: String,
        entries: Vec<(String, bool)>,
        selection: Vec<String>,
        anchor: Option<String>,
        auto_load_attempted: bool,
        scan_pending: bool,
    }

    fn navigation_snapshot(app: &LuminaApp) -> NavigationSnapshot {
        NavigationSnapshot {
            directory: app.directory.clone(),
            entries: app
                .entries
                .iter()
                .map(|entry| (entry.path.display().to_string(), entry.has_sidecar))
                .collect(),
            selection: app.filmstrip_selection(),
            anchor: app.filmstrip_anchor.clone(),
            auto_load_attempted: app.auto_load_attempted,
            scan_pending: app.scan_pending,
        }
    }

    fn png_bytes(rgb: [u8; 3]) -> Vec<u8> {
        ImageFrame::new(1, 1, vec![rgb[0], rgb[1], rgb[2], 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    fn write_png(path: &Path, rgb: [u8; 3]) -> Vec<u8> {
        let bytes = png_bytes(rgb);
        std::fs::write(path, &bytes).unwrap();
        bytes
    }

    fn new_app() -> LuminaApp {
        LuminaApp::new(egui::Context::default())
    }

    /// Pump the same non-blocking decode channel driven by `update`.
    fn settle_decode(app: &mut LuminaApp) {
        for _ in 0..120_000 {
            app.poll_decode();
            if !app.decode_pending() {
                assert!(
                    app.pending_load_path.is_none(),
                    "a settled request must clear its pending path"
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("background drop decode did not settle");
    }

    fn loaded_app_with_saved_edit(stem: &str) -> (tempfile::TempDir, PathBuf, Vec<u8>, LuminaApp) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(format!("{stem}.png"));
        let bytes = write_png(&path, [10, 20, 30]);
        let mut app = new_app();
        app.accept_dropped_file(&path, || panic!("path drop must not read bytes"));
        settle_decode(&mut app);
        assert_eq!(app.path, path.display().to_string());
        assert!(app.error().is_none());
        app.set_adjustment("exposure", 1.25);
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none(), "A setup edit must save");
        (directory, path, bytes, app)
    }

    fn seed_sidecar(path: &Path, bytes: &[u8], exposure: f64) {
        let frame = ImageFrame::decode(bytes).unwrap();
        let identity = super::super::selection_source_identity(
            path.file_name().unwrap().to_string_lossy().as_ref(),
            bytes,
            &frame,
            1,
            false,
        );
        let mut document = SidecarDocument::new(identity, "raster-mvp-1");
        document.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), exposure);
        lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(path), &document).unwrap();
    }

    struct PreviousSourceExpectation<'a> {
        path: &'a Path,
        bytes: &'a [u8],
        recipe: &'a EditRecipe,
        document_revision: &'a str,
        sidecar_bytes: &'a [u8],
        navigation: &'a NavigationSnapshot,
        error_text: &'a str,
    }

    fn assert_previous_source_preserved(app: &LuminaApp, expected: PreviousSourceExpectation<'_>) {
        let PreviousSourceExpectation {
            path,
            bytes,
            recipe,
            document_revision,
            sidecar_bytes,
            navigation,
            error_text,
        } = expected;
        assert!(app.error().is_some_and(|error| error.contains(error_text)));
        assert!(
            !app.error_dialog_open(),
            "an asynchronous decode failure stays a loud banner"
        );
        assert_eq!(app.path, path.display().to_string());
        assert_eq!(app.source_name, "a.png");
        assert_eq!(app.source_bytes.as_deref(), Some(bytes));
        assert_eq!(
            &navigation_snapshot(app),
            navigation,
            "a failed cross-directory drop must preserve A's full navigation lineage"
        );
        assert_eq!(app.recipe(), recipe);
        let revision =
            lumina_sidecar::document_revision(app.document.as_ref().expect("A sidecar document"))
                .unwrap();
        assert_eq!(revision, document_revision);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(lumina_sidecar::sidecar_path_for(path)).unwrap(),
            sidecar_bytes
        );
    }

    /// A real path bypasses synchronous `file.bytes()`, preserves all navigation
    /// while decoding, then adopts its directory/listing/selection exactly once.
    #[test]
    fn dropped_path_uses_open_file_lineage() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("dropped.png");
        let bytes = write_png(&source, [40, 80, 120]);
        let mut app = new_app();
        let initial_navigation = navigation_snapshot(&app);

        app.accept_dropped_file(&source, || panic!("path drop must not read bytes"));

        assert!(app.decode_pending(), "path decode must be asynchronous");
        assert_eq!(
            app.pending_load_path.as_deref(),
            Some(source.to_str().expect("UTF-8 fixture path"))
        );
        assert!(app.path.is_empty(), "path waits for decode success");
        assert!(
            app.source_name.is_empty(),
            "bytes are not applied synchronously"
        );
        assert_eq!(
            navigation_snapshot(&app),
            initial_navigation,
            "a deferred drop must not navigate before success"
        );
        let request_generation = app.decode_generation;

        settle_decode(&mut app);

        assert_eq!(app.path, source.display().to_string());
        assert_eq!(app.directory, directory.path().display().to_string());
        assert!(app.entries.iter().any(|entry| entry.path == source));
        assert_eq!(app.filmstrip_selection(), vec![app.path.clone()]);
        assert_eq!(app.filmstrip_anchor.as_deref(), Some(app.path.as_str()));
        assert!(app.auto_load_attempted);
        assert_eq!(
            app.decode_generation, request_generation,
            "directory adoption must not start a duplicate auto-load"
        );
        assert_eq!(app.source_name, "dropped.png");
        assert_eq!(app.source_bytes.as_deref(), Some(bytes.as_slice()));
        assert!(app.document.is_none());
        assert!(app.error().is_none());
        let frame = app.original.as_ref().expect("dropped frame");
        let identity = app.source_identity(frame);
        assert_eq!(identity.relative_name, "dropped.png");
        assert_eq!(
            identity.content_hash,
            format!("blake3:{}", blake3::hash(&bytes).to_hex())
        );
    }

    /// Drop B after A, commit an A edit while B is pending, restore B's sidecar,
    /// then prove the B edit cannot rewrite A and both originals stay intact.
    #[test]
    fn drop_after_loaded_image_never_writes_previous_sidecar() {
        let (dir_a, source_a, bytes_a, mut app) = loaded_app_with_saved_edit("a");
        let dir_b = tempfile::tempdir().unwrap();
        let source_b = dir_b.path().join("b.png");
        let bytes_b = write_png(&source_b, [200, 100, 50]);
        seed_sidecar(&source_b, &bytes_b, 0.4);
        let sidecar_a = lumina_sidecar::sidecar_path_for(&source_a);

        app.accept_dropped_file(&source_b, || panic!("path drop must not read bytes"));

        // Until the worker lands, the complete source-A lineage remains active.
        assert_eq!(app.path, source_a.display().to_string());
        assert_eq!(app.source_name, "a.png");
        assert_eq!(app.source_bytes.as_deref(), Some(bytes_a.as_slice()));
        assert_eq!(app.recipe().adjustments.get("exposure"), Some(&1.25));
        assert!(app.decode_pending());

        // An edit committed while B is pending must still be written to A
        // before the successful B decode adopts B's path and sidecar lineage.
        app.set_adjustment("exposure", 1.5);
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none());
        assert_eq!(app.path, source_a.display().to_string());
        let document_a = lumina_sidecar::load_sidecar(&sidecar_a).expect("A edit committed");
        assert_eq!(
            document_a.virtual_copies[0]
                .recipe
                .adjustments
                .get("exposure"),
            Some(&1.5)
        );
        let sidecar_a_before = std::fs::read(&sidecar_a).unwrap();

        settle_decode(&mut app);

        assert_eq!(app.path, source_b.display().to_string());
        assert_eq!(app.directory, dir_b.path().display().to_string());
        assert_eq!(
            app.filmstrip_selection(),
            vec![source_b.display().to_string()]
        );
        assert_eq!(
            app.filmstrip_anchor.as_deref(),
            Some(source_b.to_str().expect("UTF-8 fixture path"))
        );
        assert_eq!(app.source_name, "b.png");
        assert_eq!(app.source_bytes.as_deref(), Some(bytes_b.as_slice()));
        assert_eq!(app.recipe().adjustments.get("exposure"), Some(&0.4));
        let document = app.document.as_ref().expect("B sidecar restored");
        assert_eq!(document.source.relative_name, "b.png");

        app.set_adjustment("exposure", -0.75);
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none());

        assert_eq!(
            std::fs::read(&sidecar_a).unwrap(),
            sidecar_a_before,
            "editing dropped B must not rewrite A's sidecar"
        );
        let document_b = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source_b))
            .expect("B sidecar written beside B");
        assert_eq!(
            document_b.virtual_copies[0]
                .recipe
                .adjustments
                .get("exposure"),
            Some(&-0.75)
        );
        assert_eq!(std::fs::read(&source_a).unwrap(), bytes_a);
        assert_eq!(std::fs::read(&source_b).unwrap(), bytes_b);

        let mut reopened = new_app();
        reopened.accept_dropped_file(&source_b, || panic!("path drop must not read bytes"));
        settle_decode(&mut reopened);
        assert_eq!(reopened.path, source_b.display().to_string());
        assert_eq!(reopened.directory, dir_b.path().display().to_string());
        assert_eq!(reopened.recipe().adjustments.get("exposure"), Some(&-0.75));
        assert!(reopened.error().is_none());
        drop(dir_a);
    }

    /// A missing path is handled by the async path loader. Its visible failure
    /// must not replace any part of the already-loaded source A.
    #[test]
    fn unreadable_drop_preserves_previous_source() {
        let (_dir_a, source_a, bytes_a, mut app) = loaded_app_with_saved_edit("a");
        let recipe = app.recipe().clone();
        let revision =
            lumina_sidecar::document_revision(app.document.as_ref().expect("A sidecar document"))
                .unwrap();
        let sidecar_before = std::fs::read(lumina_sidecar::sidecar_path_for(&source_a)).unwrap();
        let navigation = navigation_snapshot(&app);
        let target_dir = tempfile::tempdir().unwrap();
        let missing = target_dir.path().join("missing.png");

        app.accept_dropped_file(&missing, || panic!("path drop must not read bytes"));
        settle_decode(&mut app);

        assert_previous_source_preserved(
            &app,
            PreviousSourceExpectation {
                path: &source_a,
                bytes: &bytes_a,
                recipe: &recipe,
                document_revision: &revision,
                sidecar_bytes: &sidecar_before,
                navigation: &navigation,
                error_text: "missing.png",
            },
        );
    }

    /// An existing but undecodable path is just as loud and equally isolated.
    #[test]
    fn unsupported_drop_preserves_previous_source() {
        let (_dir_a, source_a, bytes_a, mut app) = loaded_app_with_saved_edit("a");
        let recipe = app.recipe().clone();
        let revision =
            lumina_sidecar::document_revision(app.document.as_ref().expect("A sidecar document"))
                .unwrap();
        let sidecar_before = std::fs::read(lumina_sidecar::sidecar_path_for(&source_a)).unwrap();
        let navigation = navigation_snapshot(&app);
        let target_dir = tempfile::tempdir().unwrap();
        let unsupported = target_dir.path().join("unsupported.png");
        std::fs::write(&unsupported, b"not an image").unwrap();

        app.accept_dropped_file(&unsupported, || panic!("path drop must not read bytes"));
        settle_decode(&mut app);

        assert_previous_source_preserved(
            &app,
            PreviousSourceExpectation {
                path: &source_a,
                bytes: &bytes_a,
                recipe: &recipe,
                document_revision: &revision,
                sidecar_bytes: &sidecar_before,
                navigation: &navigation,
                error_text: "unsupported.png",
            },
        );
    }

    /// The bytes-only fallback supersedes an older path worker, detaches A's
    /// path before exposing B's pixels, and refuses later saves under A.
    #[test]
    fn pathless_bytes_drop_cannot_write_previous_sidecar() {
        let (dir_a, source_a, bytes_a, mut app) = loaded_app_with_saved_edit("a");
        let sidecar_a = lumina_sidecar::sidecar_path_for(&source_a);
        let sidecar_before = std::fs::read(&sidecar_a).unwrap();
        let bytes_b = png_bytes([90, 180, 240]);
        let pending_dir = tempfile::tempdir().unwrap();
        let pending = pending_dir.path().join("pending.png");
        let _ = write_png(&pending, [30, 60, 90]);
        app.accept_dropped_file(&pending, || panic!("path drop must not read bytes"));
        assert!(app.decode_pending());

        app.accept_dropped_file(Path::new(""), || Ok(bytes_b.clone()));

        assert!(!app.decode_pending(), "the older path worker is superseded");
        assert!(app.pending_load_path.is_none());
        assert!(
            app.path.is_empty(),
            "bytes-only source must not retain A's path"
        );
        assert_eq!(app.source_name, "dropped-image");
        assert_eq!(app.source_bytes.as_deref(), Some(bytes_b.as_slice()));
        app.set_adjustment("exposure", 2.0);
        app.commit_pending_slider_save([0, 0]);

        assert!(
            app.error().is_some(),
            "pathless edits must refuse sidecar save"
        );
        assert_eq!(std::fs::read(&sidecar_a).unwrap(), sidecar_before);
        assert_eq!(std::fs::read(&source_a).unwrap(), bytes_a);
        assert_eq!(std::fs::read_dir(dir_a.path()).unwrap().count(), 2);
    }

    /// Optional real-RAW path proof using the repository's licensed fixture
    /// convention. Run with `LUMINA_RAW_FIXTURE=/path/to/aircraft-*.cr3`.
    #[test]
    #[ignore = "set LUMINA_RAW_FIXTURE to a licensed RAW fixture"]
    fn dropped_raw_path_preserves_orientation_metadata_and_identity() {
        let fixture = PathBuf::from(
            std::env::var_os("LUMINA_RAW_FIXTURE")
                .expect("LUMINA_RAW_FIXTURE must point to a licensed RAW fixture"),
        );
        let fixture_bytes = std::fs::read(&fixture).unwrap();
        let metadata = lumina_raw::read_metadata(&fixture).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join(
            fixture
                .file_name()
                .expect("RAW fixture file name")
                .to_string_lossy()
                .into_owned(),
        );
        std::fs::write(&source, &fixture_bytes).unwrap();
        let mut app = new_app();

        app.accept_dropped_file(&source, || panic!("path drop must not read bytes"));
        settle_decode(&mut app);

        assert!(
            app.error().is_none(),
            "RAW drop must decode: {:?}",
            app.error()
        );
        assert!(app.source_is_raw);
        assert_eq!(app.raw_orientation, metadata.orientation);
        let frame = app.original.as_ref().expect("RAW frame");
        assert_eq!(
            (frame.width, frame.height),
            (metadata.width, metadata.height)
        );
        if let Some(make) = metadata.camera_make.as_deref() {
            assert_eq!(
                app.loaded_lens_identity
                    .as_ref()
                    .and_then(|identity| identity.camera_make.as_deref()),
                Some(make)
            );
        }
        if let Some(lens) = metadata.lens.as_deref() {
            assert_eq!(
                app.loaded_lens_identity
                    .as_ref()
                    .and_then(|identity| identity.lens.as_deref()),
                Some(lens)
            );
        }

        app.set_adjustment("exposure", 0.2);
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none());
        let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source))
            .expect("sidecar written beside RAW copy");
        assert_eq!(document.source.orientation, metadata.orientation);
        assert_eq!(
            document.source.relative_name,
            source.file_name().unwrap().to_string_lossy().as_ref()
        );
        assert_eq!(
            document.source.geometry_fingerprint.orientation,
            metadata.orientation
        );
        assert_eq!(std::fs::read(&source).unwrap(), fixture_bytes);
        assert_eq!(std::fs::read(&fixture).unwrap(), fixture_bytes);
    }
}
