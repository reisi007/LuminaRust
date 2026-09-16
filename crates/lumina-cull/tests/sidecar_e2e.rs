//! End-to-end sidecar binding: analyze → section → record → atomic save →
//! load → evaluate, plus stale/unusable/missing states and the non-destructive
//! guarantee (proposal never writes rating/flag/label).

mod common;

use common::checkerboard;

use lumina_cull::{
    analyze_heuristic, clear_culling, evaluate_culling, evaluate_section, heuristic_identity,
    load_culling, record_culling, save_culling, CullConfig, CullSourceInput, CullingReadState,
    IdentityMismatch,
};
use lumina_sidecar::{
    CullingAnalyzerKind, CullingStatus, DecodeFingerprint, Flag, GeometryFingerprint,
    SidecarDocument, SourceFingerprint, SourceIdentity,
};
use std::collections::BTreeMap;

const CREATED_AT: &str = "2026-09-16T08:00:00Z";

fn decode() -> DecodeFingerprint {
    DecodeFingerprint {
        decoder: "libraw".into(),
        version: "0.22".into(),
        parameters: BTreeMap::new(),
        extras: Default::default(),
    }
}

fn geometry(width: u32, height: u32) -> GeometryFingerprint {
    GeometryFingerprint {
        width,
        height,
        orientation: 1,
        pixel_aspect_ratio: 1.0,
        extras: Default::default(),
    }
}

fn source_identity(content_hash: &str, width: u32, height: u32) -> SourceIdentity {
    SourceIdentity {
        relative_name: "IMG_0001.ARW".into(),
        content_hash: content_hash.into(),
        byte_length: 42,
        modified_at: None,
        raw_format: "ARW".into(),
        orientation: 1,
        decode_fingerprint: decode(),
        geometry_fingerprint: geometry(width, height),
        extras: Default::default(),
    }
}

fn fingerprint(source: &SourceIdentity) -> SourceFingerprint {
    SourceFingerprint {
        content_hash: source.content_hash.clone(),
        byte_length: source.byte_length,
        extras: Default::default(),
    }
}

#[test]
fn analyze_record_save_load_evaluate_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("IMG_0001.ARW.lumina.json");

    let frame = checkerboard(160, 160, 8, 90, 165);
    let config = CullConfig::default();
    let input = CullSourceInput {
        frame: &frame,
        iso: None,
    };
    let analysis = analyze_heuristic(&input, &config).expect("analysis");
    let source = source_identity("blake3:abc", frame.width, frame.height);
    let source_fingerprint = fingerprint(&source);
    let identity = analysis.core.identity(
        source_fingerprint.clone(),
        source.decode_fingerprint.clone(),
        source.geometry_fingerprint.clone(),
    );
    let section = analysis
        .core
        .to_section(
            source_fingerprint.clone(),
            source.decode_fingerprint.clone(),
            source.geometry_fingerprint.clone(),
            CREATED_AT,
        )
        .expect("section");
    assert_eq!(section.identity, identity);
    assert_eq!(
        section.identity.analyzer.kind,
        CullingAnalyzerKind::Heuristic
    );
    assert!(section.identity.analyzer.model_hash.is_none());

    let mut document = SidecarDocument::new(source.clone(), "pipeline-1");
    // Manual flags/rating must survive the proposal untouched.
    document.virtual_copies[0].rating = 3;
    document.virtual_copies[0].flag = Flag::Pick;
    let copies_before = document.virtual_copies.clone();

    record_culling(&mut document, section.clone()).expect("record");
    assert_eq!(document.culling.as_ref(), Some(&section));
    assert_eq!(
        document.virtual_copies, copies_before,
        "recording a proposal must never mutate virtual copies (rating/flag/label)"
    );
    assert_eq!(document.virtual_copies[0].rating, 3);
    assert_eq!(document.virtual_copies[0].flag, Flag::Pick);

    assert!(matches!(
        evaluate_culling(&document, &identity),
        CullingReadState::Valid(_)
    ));

    save_culling(&path, &document).expect("save");
    match load_culling(&path, &identity).expect("load") {
        CullingReadState::Valid(loaded) => assert_eq!(loaded, section),
        other => panic!("expected a valid proposal after reload, got {other:?}"),
    }

    // A different source content hash invalidates the proposal visibly.
    let changed = heuristic_identity(
        SourceFingerprint {
            content_hash: "blake3:changed".into(),
            byte_length: 42,
            extras: Default::default(),
        },
        source.decode_fingerprint.clone(),
        source.geometry_fingerprint.clone(),
        section.identity.analysis_resolution.clone(),
    );
    match evaluate_culling(&document, &changed) {
        CullingReadState::Stale { mismatches, .. } => {
            assert!(mismatches.contains(&IdentityMismatch::SourceContentHash));
        }
        other => panic!("expected stale on content-hash change, got {other:?}"),
    }

    // Absent section is the valid "no proposal" state.
    let empty = SidecarDocument::new(source.clone(), "pipeline-1");
    assert_eq!(
        evaluate_culling(&empty, &identity),
        CullingReadState::NoProposal
    );

    // Explicit clear returns to "no proposal" without inventing a replacement.
    clear_culling(&mut document);
    assert!(document.culling.is_none());
    assert_eq!(
        evaluate_culling(&document, &identity),
        CullingReadState::NoProposal
    );
    assert_eq!(
        document.virtual_copies[0].rating, 3,
        "clear is not destructive"
    );
}

#[test]
fn missing_sidecar_file_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nope.lumina.json");
    let frame = checkerboard(32, 32, 4, 90, 165);
    let config = CullConfig::default();
    let analysis = analyze_heuristic(
        &CullSourceInput {
            frame: &frame,
            iso: None,
        },
        &config,
    )
    .expect("analysis");
    let source = source_identity("blake3:abc", frame.width, frame.height);
    let identity = analysis
        .core
        .identity(fingerprint(&source), decode(), geometry(32, 32));
    assert!(
        load_culling(&path, &identity).is_err(),
        "a missing sidecar must be a loud error, not a silent no-proposal"
    );
}

#[test]
fn persisted_non_valid_status_is_unusable_and_not_writable() {
    let frame = checkerboard(64, 64, 8, 90, 165);
    let config = CullConfig::default();
    let analysis = analyze_heuristic(
        &CullSourceInput {
            frame: &frame,
            iso: None,
        },
        &config,
    )
    .expect("analysis");
    let source = source_identity("blake3:abc", frame.width, frame.height);
    let source_fingerprint = fingerprint(&source);
    let identity = analysis.core.identity(
        source_fingerprint.clone(),
        source.decode_fingerprint.clone(),
        source.geometry_fingerprint.clone(),
    );
    let mut section = analysis
        .core
        .to_section(
            source_fingerprint,
            source.decode_fingerprint.clone(),
            source.geometry_fingerprint.clone(),
            CREATED_AT,
        )
        .expect("section");
    section.status = CullingStatus::Stale;
    section.error = Some("analysis artifact no longer usable".into());

    assert!(matches!(
        evaluate_section(Some(&section), &identity),
        CullingReadState::Unusable { .. }
    ));

    let mut document = SidecarDocument::new(source, "pipeline-1");
    assert!(
        record_culling(&mut document, section).is_err(),
        "only valid proposals may be recorded; stale states are read-only"
    );
    assert!(document.culling.is_none());
}

#[test]
fn analyzer_or_preprocessing_change_marks_stale() {
    let frame = checkerboard(64, 64, 8, 90, 165);
    let config = CullConfig::default();
    let analysis = analyze_heuristic(
        &CullSourceInput {
            frame: &frame,
            iso: None,
        },
        &config,
    )
    .expect("analysis");
    let source = source_identity("blake3:abc", frame.width, frame.height);
    let source_fingerprint = fingerprint(&source);
    let section = analysis
        .core
        .to_section(
            source_fingerprint,
            source.decode_fingerprint.clone(),
            source.geometry_fingerprint.clone(),
            CREATED_AT,
        )
        .expect("section");

    // Analyzer version bump ⇒ stale.
    let mut bumped = section.identity.clone();
    bumped.analyzer.version = "2".into();
    let mismatches = lumina_cull::identity_mismatches(&section.identity, &bumped);
    assert_eq!(mismatches, vec![IdentityMismatch::Analyzer]);

    // Preprocessing version bump ⇒ stale.
    let mut preprocessed = section.identity.clone();
    preprocessed.preprocessing.version = "2".into();
    let mismatches = lumina_cull::identity_mismatches(&section.identity, &preprocessed);
    assert_eq!(mismatches, vec![IdentityMismatch::Preprocessing]);

    // Decode parameter change ⇒ stale.
    let mut decoded = section.identity.clone();
    decoded
        .decode
        .parameters
        .insert("half_size".into(), "true".into());
    let mismatches = lumina_cull::identity_mismatches(&section.identity, &decoded);
    assert_eq!(mismatches, vec![IdentityMismatch::Decode]);
}
