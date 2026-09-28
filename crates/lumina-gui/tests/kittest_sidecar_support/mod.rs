//! KITT-IDENTITY-49: a sidecar fixture must carry the fingerprint of the bytes
//! that are really on disk.
//!
//! # Why this module exists
//!
//! `LuminaApp::finish_decode` validates a sidecar it found against the frame it
//! just decoded: `source_actions::source_fingerprint_matches` compares
//! `content_hash` **and** `byte_length` against the live fingerprint, and a
//! mismatch is reported as a loud `source identity conflict` banner with the
//! sidecar *not* adopted (`document` stays `None`). That is the required
//! product behaviour — a stale sidecar must never be silently applied — and it
//! is unchanged here.
//!
//! The `library_meta_history` golden seed used to write a **fabricated**
//! identity (`content_hash: "blake3:kittest-meta-history"`, `byte_length: 0`),
//! so its sidecar was stale by construction. The app refused it (correctly),
//! `metadata_history()` returned `unwrap_or_default()`, and the test then
//! waited out a 300 s wall-clock bound on a state that could never arrive. The
//! seed (dd73806, 2026-09-05) predates the identity validation (9a24fd2,
//! 2026-09-25, R3-OPEN-1): the validation broke the test, not the other way
//! round. This module makes the seed carry the real fingerprint, and
//! `kittest_sidecar_identity.rs` proves the refusal still happens.
//!
//! # The contract
//!
//! [`written_source_identity`] mirrors what production writes for a decoded
//! non-RAW source (`source_identity::build_source_identity`): the BLAKE3 of the
//! exact source bytes, their length, the uppercase extension as `raw_format`,
//! the `image` decoder identity with the package version, and the *decoded*
//! geometry. This is the same rule `kittest_fixtures_support::staged_source_identity`
//! states for the committed RAW fixtures — with one difference: those hash a
//! committed `sample-data/raw/` file, while the fixtures here are generated
//! JPEG bytes, so the hash is taken over the bytes that were just written (and
//! read back from disk, so a fixture cannot claim a fingerprint it never
//! stored).

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SidecarDocument, SourceIdentity};
use std::collections::BTreeMap;
use std::path::Path;

/// 2x1 JPEG bytes through the real encoder (same fixture pixels as the lib
/// `jpeg()` helper) so the embedded-IPTC and sidecar tests decode genuine JPEG
/// bytes instead of a hand-built byte string.
pub(crate) fn test_jpeg_bytes() -> Vec<u8> {
    ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .expect("fixture frame")
        .encode(ImageFileFormat::Jpeg)
        .expect("jpeg encodes")
}

/// The [`SourceIdentity`] production computes for the file at `path`.
///
/// The bytes are read back from disk rather than taken from the caller, so the
/// identity provably describes the file the app will decode. The geometry is
/// decoded (`ImageFrame::decode`) instead of declared, so a fixture cannot
/// disagree with its own pixels.
pub(crate) fn written_source_identity(path: &Path) -> SourceIdentity {
    let bytes = std::fs::read(path)
        .unwrap_or_else(|error| panic!("fixture {} must be readable: {error}", path.display()));
    let frame = ImageFrame::decode(&bytes)
        .unwrap_or_else(|error| panic!("fixture {} must decode: {error}", path.display()));
    let relative_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    SourceIdentity {
        content_hash: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
        byte_length: bytes.len() as u64,
        raw_format: Path::new(&relative_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("raster")
            .to_ascii_uppercase(),
        decode_fingerprint: DecodeFingerprint {
            // `source_identity::build_source_identity` with `source_is_raw ==
            // false`: the `image` decoder and the package version. Naming a
            // fixture decoder here would be a second, fictional identity.
            decoder: "image".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        relative_name,
        modified_at: None,
        orientation: 1,
        extras: BTreeMap::new(),
    }
}

/// Write `photo.jpg` into `dir` and seed its sidecar with 10 metadata history
/// entries carrying fixed RFC 3339 UTC timestamps (no wall-clock): the History
/// panel renders `rev | timestamp | origin | changed`, so real timestamps would
/// leak nondeterministic pixels into the golden. Seeding goes through the
/// public sidecar API into a tempdir file — the snapshot itself (like the
/// `create_mask` precedents) performs no disk write. Returns the image path.
///
/// `corrupt` receives the **real** identity ([`written_source_identity`]) and
/// may break exactly one field of it; `|_| {}` writes the identity production
/// would write. The parameter is what makes the rejection test possible
/// without a second, hand-written struct literal: the honest and the stale
/// fixture differ in one assignment, so they cannot drift apart.
pub(crate) fn seed_metadata_history_sidecar<C: FnOnce(&mut SourceIdentity)>(
    dir: &Path,
    corrupt: C,
) -> std::path::PathBuf {
    let photo = dir.join("photo.jpg");
    std::fs::write(&photo, test_jpeg_bytes()).expect("write jpeg fixture");
    let mut identity = written_source_identity(&photo);
    corrupt(&mut identity);
    let mut document = SidecarDocument::new(identity, "raster-mvp-1");
    for index in 1..=10_u32 {
        let mut fields = BTreeMap::new();
        fields.insert("title".to_owned(), format!("Titel {index}"));
        let timestamp = format!("2026-01-{index:02}T12:00:00Z");
        assert!(
            document
                .apply_metadata_draft(&fields, "gui", &timestamp)
                .expect("seed history entry"),
            "history entry {index} must change the draft"
        );
    }
    let sidecar = lumina_sidecar::sidecar_path_for(&photo);
    lumina_sidecar::save_sidecar(&sidecar, &document).expect("seed sidecar");
    photo
}
