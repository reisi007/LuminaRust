//! Unit tests for the shared stage-editor crate.
//!
//! The behaviour of the four editors is proven from the outside — byte identity
//! against the real CLI and the loud rejections in
//! `crates/lumina-cli/tests/stage_parity*.rs`. What is tested *here* is the
//! layer those tests cannot reach: the copy resolution, the decode/identity
//! helpers both transports share, and the `Persist` policy that decides who
//! writes the sidecar (the CLI immediately, the MCP server under a
//! compare-and-swap).

use super::*;
use crate::copy::{copy_mut, copy_ref, resolve_copy};
use crate::decode::{decode_input, is_raw_path, source_identity, timestamp};
use crate::report::Persist;
use crate::spot::{run as run_spot, SpotRequest};
use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for, SidecarDocument};
use std::path::PathBuf;

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    use lumina_core::{ImageFileFormat, ImageFrame};
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) as usize) * 4;
            pixels[index..index + 4].copy_from_slice(&[40, 80, 120, 255]);
        }
    }
    ImageFrame::new(width, height, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// Materialises a source image plus its standard sidecar, like `lumina import`.
fn fixture(dir: &std::path::Path) -> PathBuf {
    let input = dir.join("input.png");
    std::fs::write(&input, png_bytes(16, 16)).unwrap();
    let bytes = std::fs::read(&input).unwrap();
    let (frame, raw) = decode_input(&input, &bytes).unwrap();
    let identity = source_identity(&input, &bytes, &frame, raw.as_ref()).unwrap();
    save_sidecar(
        &sidecar_path_for(&input),
        &SidecarDocument::new(identity, "raster-mvp-1"),
    )
    .unwrap();
    input
}

// ------------------------------------------------------------ copy resolution

#[test]
fn resolve_copy_selects_the_default_and_rejects_an_unknown_id_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let bytes = std::fs::read(&input).unwrap();
    let (frame, raw) = decode_input(&input, &bytes).unwrap();
    let mut document = SidecarDocument::new(
        source_identity(&input, &bytes, &frame, raw.as_ref()).unwrap(),
        "raster-mvp-1",
    );
    // `Some` must match an id exactly: a *name* is not accepted, so an MCP call
    // and a CLI `--virtual-copy` can never select different copies.
    assert_eq!(resolve_copy(&document, None).unwrap(), "vc-original");
    assert_eq!(
        resolve_copy(&document, Some("vc-original")).unwrap(),
        "vc-original"
    );
    let error = resolve_copy(&document, Some("nope")).unwrap_err();
    assert_eq!(error.to_string(), "unknown virtual copy `nope`");
    assert!(matches!(
        copy_ref(&document, "nope"),
        Err(StageError::Message(_))
    ));
    assert!(matches!(
        copy_mut(&mut document, "nope"),
        Err(StageError::Message(_))
    ));
    // A document without copies is loud, not a silent "first = default".
    document.virtual_copies.clear();
    assert_eq!(
        resolve_copy(&document, None).unwrap_err().to_string(),
        "sidecar has no virtual copies"
    );
}

// ---------------------------------------------------------- decode / identity

#[test]
fn a_png_is_decoded_and_a_raw_extension_is_recognised() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    assert!(!is_raw_path(&input));
    assert!(is_raw_path(std::path::Path::new("frame.ARW")));
    assert!(!is_raw_path(std::path::Path::new("frame.png")));
    let bytes = std::fs::read(&input).unwrap();
    let (frame, raw) = decode_input(&input, &bytes).unwrap();
    assert_eq!((frame.width, frame.height), (16, 16));
    assert!(raw.is_none(), "a PNG has no RAW metadata");
}

#[test]
fn the_source_identity_is_deterministic_and_freezes_the_image_decoder_version() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let bytes = std::fs::read(&input).unwrap();
    let (frame, raw) = decode_input(&input, &bytes).unwrap();
    let first = source_identity(&input, &bytes, &frame, raw.as_ref()).unwrap();
    let second = source_identity(&input, &bytes, &frame, raw.as_ref()).unwrap();
    assert_eq!(first, second, "the identity must be reproducible");
    // The decoder version is persisted in every sidecar, so it is a pinned
    // literal (see `decode::IMAGE_DECODER_VERSION`), not this crate's version.
    assert_eq!(first.decode_fingerprint.decoder, "image");
    assert_eq!(first.decode_fingerprint.version, "0.1.0");
    assert_eq!(
        first.content_hash,
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    );
    assert_eq!(first.orientation, 1);
}

#[test]
fn a_missing_file_is_reported_with_the_clis_io_error_text() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone.png");
    let error = decode_input(&missing, b"").unwrap_err();
    // No frame in the bytes: the decode error surfaces unchanged.
    assert!(matches!(error, StageError::Core(_)));
    let io_error = StageError::io(&missing, std::io::Error::from(std::io::ErrorKind::NotFound));
    assert_eq!(
        io_error.to_string(),
        format!("I/O error for `{}`: entity not found", missing.display())
    );
}

#[test]
fn the_timestamp_never_silently_falls_back_to_an_empty_string() {
    // Pre-extraction CLI behaviour: an impossible clock yields the literal "0",
    // not "" (a history entry with an empty id would be ambiguous).
    let value = timestamp();
    assert!(!value.is_empty());
    assert!(value.chars().all(|c| c.is_ascii_digit()));
}

// -------------------------------------------------------------- Persist policy

#[test]
fn a_read_returns_no_document_under_both_persist_policies() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let before = std::fs::read(sidecar_path_for(&input)).unwrap();
    for persist in [Persist::Immediately, Persist::Deferred] {
        let request = SpotRequest {
            input: input.display().to_string(),
            list: true,
            ..SpotRequest::default()
        };
        let run = run_spot(&request, persist).unwrap();
        assert!(!run.report.wrote, "{persist:?}: a read never writes");
        assert!(run.report.actions.is_empty());
        assert!(run.report.payload["spots"].is_array());
        assert!(
            run.document.is_none(),
            "{persist:?}: a read has no document"
        );
        assert_eq!(run.sidecar_path, sidecar_path_for(&input));
        assert_eq!(
            std::fs::read(sidecar_path_for(&input)).unwrap(),
            before,
            "{persist:?}: a read must not touch the sidecar bytes"
        );
    }
}

#[test]
fn immediate_persists_by_itself_and_deferred_hands_the_document_back() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let add = |persist| {
        let request = SpotRequest {
            input: input.display().to_string(),
            add_heuristic: true,
            center_x: Some(0.5),
            center_y: Some(0.5),
            radius: Some(4.0),
            ..SpotRequest::default()
        };
        run_spot(&request, persist).unwrap()
    };
    let immediate = add(Persist::Immediately);
    assert!(immediate.report.wrote);
    assert!(immediate
        .report
        .actions
        .contains(&"add-heuristic".to_string()));
    assert!(
        immediate.document.is_none(),
        "the editor already wrote the sidecar itself"
    );
    let on_disk = std::fs::read(sidecar_path_for(&input)).unwrap();
    assert!(load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe
        .extras
        .contains_key("spot_removals"));

    // A second, deferred call on a clean document: the editor validates but does
    // not write, so the caller can compare-and-swap. The document it hands back
    // must serialise to the very bytes the immediate path already wrote.
    let dir2 = tempfile::tempdir().unwrap();
    let input2 = fixture(dir2.path());
    let before_deferred = std::fs::read(sidecar_path_for(&input2)).unwrap();
    let request = SpotRequest {
        input: input2.display().to_string(),
        add_heuristic: true,
        center_x: Some(0.5),
        center_y: Some(0.5),
        radius: Some(4.0),
        ..SpotRequest::default()
    };
    let deferred = run_spot(&request, Persist::Deferred).unwrap();
    assert!(deferred.report.wrote);
    let document = deferred.document.expect("deferred hands the document back");
    assert_eq!(
        std::fs::read(sidecar_path_for(&input2)).unwrap(),
        before_deferred,
        "a deferred write must not have touched the file yet"
    );
    save_sidecar(&deferred.sidecar_path, &document).unwrap();
    assert_eq!(
        std::fs::read(sidecar_path_for(&input2)).unwrap(),
        on_disk,
        "both persist policies must produce byte-identical sidecars"
    );
}

#[test]
fn a_loud_rejection_returns_no_document_under_either_policy() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let before = std::fs::read(sidecar_path_for(&input)).unwrap();
    for persist in [Persist::Immediately, Persist::Deferred] {
        // radius above the CLI's (0,512] window
        let request = SpotRequest {
            input: input.display().to_string(),
            add_heuristic: true,
            center_x: Some(0.5),
            center_y: Some(0.5),
            radius: Some(999.0),
            ..SpotRequest::default()
        };
        let error = run_spot(&request, persist).unwrap_err();
        assert!(
            error.to_string().contains("outside allowed range"),
            "{persist:?}: {error}"
        );
        assert_eq!(
            std::fs::read(sidecar_path_for(&input)).unwrap(),
            before,
            "{persist:?}: an aborted call must change no bytes"
        );
    }
}
