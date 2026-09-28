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
mod raw_fixture_scope;

#[cfg(test)]
mod raw_fixture_override;

#[cfg(test)]
mod raw_fixture_consumption;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests {
    use super::raw_fixture_consumption::run_real_raw_proof;
    use super::test_support::{
        assert_previous_source_preserved, loaded_app_with_saved_edit, navigation_snapshot, new_app,
        png_bytes, seed_sidecar, settle_decode, write_png, PreviousSourceExpectation,
    };
    use std::path::Path;

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

    /// Real-RAW path proof: the committed licensed CR3 fixtures are decoded
    /// through the native drop path, so orientation, geometry, lens identity
    /// and the sidecar's persisted orientation are proven on real data.
    ///
    /// The body lives in `raw_fixture_consumption.rs`. The committed set is
    /// resolved deterministically (no environment gate), every committed
    /// fixture is decoded and checked against the geometry documented in
    /// `sample-data/raw/README.md`, a missing committed fixture fails with its
    /// path, and a missing operator operand is a reported skip
    /// (`fixtures-licensing.md` §3.2.1 Regel 1–3). The coverage of the
    /// committed set is a property readable in that code, not an enforced
    /// invariant — the named limit recorded in `raw_fixture_scope.rs`.
    ///
    /// `LUMINA_RAW_FIXTURE` **adds** one further file and never replaces the
    /// committed set; the addition is a **weaker** proof, not an equal one,
    /// because it carries no documented-geometry anchor, so its decode is only
    /// cross-checked against `lumina_raw::read_metadata`. An addition whose
    /// name a committed fixture already carries is reported as superfluous
    /// instead of being decoded again (`fixtures-licensing.md` §3.2.1 Regel 5).
    ///
    /// `#[ignore]` stays because decoding two 12 MB CR3s is real work, not
    /// because the proof needs something the machine might lack.
    #[test]
    #[ignore = "decodes the two committed 12 MB CR3 fixtures (~3 s); run: cargo test -p lumina-gui --lib -- --ignored dropped_raw_path_preserves_orientation"]
    fn dropped_raw_path_preserves_orientation_metadata_and_identity() {
        run_real_raw_proof();
    }
}
