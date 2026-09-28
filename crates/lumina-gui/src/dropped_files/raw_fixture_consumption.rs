//! Consumption of the committed fixture set (`DoD.md` §10).
//!
//! The proof resolves the committed set in [`raw_fixtures`] and decodes every
//! member through the native drop path, checking each against the geometry
//! `sample-data/raw/README.md` documents (`feature/quality/fixtures-licensing.md`
//! §3.2.1 **Regel 2**). The operator addition is a separate branch, never mixed
//! into that set: `LUMINA_RAW_FIXTURE` **adds** one file and never replaces the
//! committed set (§3.2.1 **Regel 5**).
//!
//! The coverage of the committed fixtures is a property **readable in**
//! [`raw_fixtures`], not an enforced invariant — the named limit recorded in
//! `raw_fixture_scope.rs`.

use std::path::Path;

use super::raw_fixture_override::{operator_addition, OperatorAddition};
use super::raw_fixture_scope::{
    accept_committed_fixture, accept_operator_fixture, raw_fixtures, DocumentedGeometry,
};
use super::test_support::{new_app, report_redundant_override, settle_decode};

/// The name of the real-RAW proof, used verbatim by every message it emits.
pub(super) const TEST: &str = "dropped_raw_path_preserves_orientation_metadata_and_identity";

/// Decode the committed set through the native drop path.
///
/// Every committed fixture is checked for presence and then decoded
/// unconditionally; the operator addition is resolved before the loop and
/// handled in its own branch afterwards, so it can neither replace nor evict a
/// committed fixture.
pub(super) fn run_real_raw_proof() {
    let committed = raw_fixtures();
    // Resolve the operator addition before the committed set is consumed; it is
    // handled in its own branch and never enters the committed set.
    let addition = operator_addition(&committed);
    for fixture in committed {
        accept_committed_fixture(&fixture);
        assert_raw_drop_preserves_orientation_and_identity(&fixture.path, Some(fixture.documented));
    }
    if let Some(addition) = addition {
        match addition {
            OperatorAddition::Append(operand) => {
                if accept_operator_fixture(&operand, TEST) {
                    assert_raw_drop_preserves_orientation_and_identity(operand.path(), None);
                }
            }
            OperatorAddition::Redundant { operand, committed } => {
                report_redundant_override(operand.path(), &committed);
            }
        }
    }
}

/// Decode one RAW fixture through the native drop path and assert its
/// orientation, geometry, lens identity and persisted sidecar orientation.
///
/// `documented` is `Some` for a committed fixture — the geometry the inventory
/// documents is the second, independent anchor — and `None` for an operator
/// addition, whose weaker run is announced by [`accept_operator_fixture`] and
/// which is only cross-checked against `lumina_raw::read_metadata`.
fn assert_raw_drop_preserves_orientation_and_identity(
    path: &Path,
    documented: Option<DocumentedGeometry>,
) {
    let fixture_bytes = std::fs::read(path).unwrap();
    let metadata = lumina_raw::read_metadata(path).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join(
        path.file_name()
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
    // Agreement with the metadata reader is one anchor …
    assert_eq!(app.raw_orientation, metadata.orientation);
    let frame = app.original.as_ref().expect("RAW frame");
    assert_eq!(
        (frame.width, frame.height),
        (metadata.width, metadata.height)
    );
    // … the inventory literals are the second, independent one: they pin the
    // values themselves instead of showing two production paths agree
    // (`fixtures-licensing.md` §3.2.1 Regel 2). A committed fixture always
    // carries them; only the operator operand does not, and its weaker run is
    // announced by `accept_operator_fixture`.
    if let Some(documented) = documented {
        assert_eq!(
            (app.raw_orientation, frame.width, frame.height),
            (documented.orientation, documented.width, documented.height),
            "{} must decode to the geometry documented in sample-data/raw/README.md",
            path.display()
        );
    }
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
    assert_eq!(std::fs::read(path).unwrap(), fixture_bytes);
}
