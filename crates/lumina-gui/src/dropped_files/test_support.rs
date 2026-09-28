//! Shared headless fixture kit for the dropped-file test subtree.
//!
//! `dropped_files.rs` sits on its 500-line ratchet entry, and
//! `decode_state_tests.rs` carried its own byte-identical `new_app`, `write_png`
//! and `settle_decode` copies. One kit serves both, so a change to the decode
//! pump or the PNG fixture cannot drift between the two test modules.
//!
//! Only generic helpers live here: the app/PNG/settle kit, the navigation
//! snapshot, and the two report channels. The routing behaviour and its
//! assertions stay in `dropped_files.rs`, the decode-policy state machine in
//! `decode_state_tests.rs`, and the real-RAW fixture set in
//! `raw_fixture_scope.rs`.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use eframe::egui;
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{EditRecipe, SidecarDocument};

use super::super::{selection_source_identity, LuminaApp};

/// A navigation-lineage snapshot, used to prove that a failed or deferred drop
/// changes *no* part of the already-loaded source's directory, entries,
/// selection, anchor and scan state.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NavigationSnapshot {
    pub(super) directory: String,
    pub(super) entries: Vec<(String, bool)>,
    pub(super) selection: Vec<String>,
    pub(super) anchor: Option<String>,
    pub(super) auto_load_attempted: bool,
    pub(super) scan_pending: bool,
}

pub(super) fn new_app() -> LuminaApp {
    LuminaApp::new(egui::Context::default())
}

/// Pump the same non-blocking decode channel driven by `update`.
pub(super) fn settle_decode(app: &mut LuminaApp) {
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

pub(super) fn png_bytes(rgb: [u8; 3]) -> Vec<u8> {
    ImageFrame::new(1, 1, vec![rgb[0], rgb[1], rgb[2], 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

pub(super) fn write_png(path: &Path, rgb: [u8; 3]) -> Vec<u8> {
    let bytes = png_bytes(rgb);
    std::fs::write(path, &bytes).unwrap();
    bytes
}

pub(super) fn navigation_snapshot(app: &LuminaApp) -> NavigationSnapshot {
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

pub(super) fn loaded_app_with_saved_edit(
    stem: &str,
) -> (tempfile::TempDir, PathBuf, Vec<u8>, LuminaApp) {
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

pub(super) fn seed_sidecar(path: &Path, bytes: &[u8], exposure: f64) {
    let frame = ImageFrame::decode(bytes).unwrap();
    let identity = selection_source_identity(
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

pub(super) struct PreviousSourceExpectation<'a> {
    pub(super) path: &'a Path,
    pub(super) bytes: &'a [u8],
    pub(super) recipe: &'a EditRecipe,
    pub(super) document_revision: &'a str,
    pub(super) sidecar_bytes: &'a [u8],
    pub(super) navigation: &'a NavigationSnapshot,
    pub(super) error_text: &'a str,
}

pub(super) fn assert_previous_source_preserved(
    app: &LuminaApp,
    expected: PreviousSourceExpectation<'_>,
) {
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

/// Report a **visible, explained** skip for an optional-fixture proof.
///
/// Rust has no runtime `skip`, and the two obvious channels are both wrong —
/// measured, not assumed:
///
/// 1. A panic is not one failed assertion but an aborted `--ignored`
///    invocation, and `--ignored` is the documented way to run the local
///    proofs (`feature/quality/golden-references.md`): one absent optional
///    fixture would turn the whole run red and hide every test behind it.
/// 2. `eprintln!` alone is invisible: libtest discards the captured output of
///    a *passing* test, so a skip reason printed that way never reaches the
///    developer — the pre-existing gap in `matrix::tests::real_matrix_headless`.
///    Writing to the real stderr bypasses the capture.
///
/// `reason` must be independent evidence (a concrete path on disk), so a skip
/// can be told apart from a proof that quietly stopped running.
pub(super) fn report_fixture_skip(test: &str, reason: &str) {
    report_to_real_stderr(&format!("SKIPPED {test}: {reason}\n"));
}

/// Report a proof that **ran** but checks less than the default run — a
/// weaker result is announced, never substituted silently.
pub(super) fn report_weakened_proof(test: &str, reason: &str) {
    report_to_real_stderr(&format!("WEAKENED {test}: {reason}\n"));
}

/// Report an operator addition that a committed fixture already covers.
///
/// `LUMINA_RAW_FIXTURE` adds to the proof, never substitutes
/// (`fixtures-licensing.md` §3.2.1 **Regel 5**). When the addition carries the
/// file name of a committed fixture, that committed file is decoded
/// unconditionally and the addition is therefore **superfluous**: it is not
/// decoded a second time, and the reason is written to the real stderr rather
/// than swallowed as a silent exception. The wording names the supplied
/// addition and the committed fixture it collides with, not an environment
/// variable — a message must not claim a provenance the code cannot see
/// (`DoD.md` §10).
pub(super) fn report_redundant_override(addition: &Path, committed: &Path) {
    report_to_real_stderr(&format!(
        "REDUNDANT {}: the committed fixture {} carries the same file name and is decoded \
         unconditionally, so the operator-supplied addition is superfluous and is not decoded \
         again\n",
        addition.display(),
        committed.display()
    ));
}

/// Write one line to the **real** stderr, bypassing libtest's capture.
///
/// **Named limit, not a covered claim:** that the channel bypasses the capture
/// is prose here, not an assertion. It was measured by hand on
/// `matrix::tests::real_matrix_headless`, but a test inside this process
/// cannot observe which handle a line went to, and pinning it with a child
/// process would cost far more than the line is worth. The same sentence is
/// carried as an open item under `FIXTURE-SKIP-VISIBLE-2` in
/// `Agents.todo.md`: a silent return to `eprintln!` would not be caught here.
fn report_to_real_stderr(line: &str) {
    let mut stderr = std::io::stderr();
    // Best effort: an unwritable stderr must not turn a caveat into a failure.
    let _ = stderr.write_all(line.as_bytes());
    let _ = stderr.flush();
}
