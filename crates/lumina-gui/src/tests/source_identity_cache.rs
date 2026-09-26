//! THUMB-HASH-PERF-35: the whole-file source identity is computed once per
//! file state, not once per visible cell per frame — and a genuinely changed
//! source still forces a new identity.
//!
//! The pre-fix UI thread read and BLAKE3-hashed every visible RAW in every
//! frame (0.105 s per 12 MB CR3, 0.79 s per frame at three visible cells
//! against 0.69 s of decode → ~1 fps for a folder of real RAWs). The memo in
//! [`crate::source_identity`] keys the expensive identity on
//! `(path, mtime, len)`; the normative contract lives in
//! `feature/platform/cli-gui-wasm.md` § *Quell-Identitäts-Cache im UI-Thread*.
//!
//! Every assertion here is a **count** or a **value**, never a sleep: the memo
//! records how many whole-file hashes it spent on a key, and the counter is
//! per key (not global), so the multi-threaded suite cannot perturb it.

use crate::source_actions::FileContentIdentity;
use crate::source_identity;
use crate::tests::source_actions::{action_fixture, ActionFixtureMode};
use crate::thumb_cache::THUMB_VIRTUAL_COPY;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A temp file with a fixed mtime, so `(path, mtime, len)` keys are
/// reproducible and a deliberate mtime bump is unambiguous.
struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl Fixture {
    fn new(name: &str, bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let fixture = Self { _dir: dir, path };
        fixture.set_mtime(Self::mtime_anchor());
        fixture
    }

    /// Pin the mtime to a fixed instant (well in the past, so no later write can
    /// accidentally produce the same value).
    fn set_mtime(&self, mtime: SystemTime) {
        let file = std::fs::File::options()
            .write(true)
            .open(&self.path)
            .expect("fixture must be writable");
        file.set_times(std::fs::FileTimes::new().set_modified(mtime))
            .expect("set_times");
    }

    fn mtime_anchor() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000)
    }

    /// Pin the mtime back to the anchor (used by the ctime guard).
    fn restore_mtime(&self) {
        self.set_mtime(Self::mtime_anchor());
    }

    fn identity(&self) -> FileContentIdentity {
        FileContentIdentity::from_path(&self.path)
    }

    /// Whole-file hashes the memo has spent on this file's *current* key.
    fn hashes(&self) -> Option<u64> {
        source_identity::hash_count(&self.path)
    }
}

/// Deterministic filler large enough to span several 64-KB hash chunks, so the
/// streaming/one-shot equivalence below is a real test and not a
/// single-chunk coincidence.
fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|index| (index as u8) ^ seed).collect()
}

fn identity_of_bytes(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

// ---- 1. No re-hash on an unchanged file ----

/// The regression guard for the reported bug: the UI thread asks for a visible
/// cell's source identity once per frame, and that must cost **one** whole-file
/// hash, not one per frame. Proven by the per-key hash counter, not by timing.
#[test]
fn unchanged_file_is_hashed_once_across_repeated_lookups() {
    let bytes = payload(300 * 1024, 0x5A);
    let fixture = Fixture::new("unchanged.arw", &bytes);

    // Ten further lookups stand in for ten frames of `ensure_thumbnail` on the
    // same visible cell.
    let first = fixture.identity();
    assert_eq!(
        first,
        FileContentIdentity::Hashed(identity_of_bytes(&bytes))
    );
    for _ in 0..10 {
        assert_eq!(
            fixture.identity(),
            first,
            "the identity value must be stable"
        );
    }
    assert_eq!(
        fixture.hashes(),
        Some(1),
        "an unchanged file must be hashed exactly once, not once per frame"
    );

    // The same holds for the second consumer of the same file: the fail-closed
    // sidecar/source fingerprint check that used to `std::fs::read` the whole
    // source per frame.
    let fingerprint = source_identity::source_fingerprint_of(&fixture.path).unwrap();
    assert_eq!(fingerprint.content_hash, identity_of_bytes(&bytes));
    assert_eq!(fingerprint.byte_length, bytes.len() as u64);
    assert_eq!(
        fixture.hashes(),
        Some(1),
        "the live source fingerprint must reuse the memoized identity"
    );
}

// ---- 2. A changed file yields a different identity ----

/// The cache must never serve a remembered identity for a file that actually
/// changed — the failure mode that would silently show a stale thumbnail next
/// to a replaced original. Both documented invalidation signals are covered:
/// a changed byte length and a changed mtime at an identical length.
#[test]
fn changed_file_forces_a_new_identity() {
    let original = payload(200 * 1024, 0x11);
    let fixture = Fixture::new("changed.arw", &original);
    assert_eq!(
        fixture.identity(),
        FileContentIdentity::Hashed(identity_of_bytes(&original))
    );
    assert_eq!(fixture.hashes(), Some(1));

    // (a) Different length — the common case (an edited export, a new take).
    let longer = payload(original.len() + 1024, 0x11);
    std::fs::write(&fixture.path, &longer).unwrap();
    let after_length = fixture.identity();
    assert_ne!(
        after_length,
        FileContentIdentity::Hashed(identity_of_bytes(&original)),
        "a longer file must not reuse the old identity"
    );
    assert_eq!(
        after_length,
        FileContentIdentity::Hashed(identity_of_bytes(&longer))
    );
    assert_eq!(
        fixture.hashes(),
        Some(1),
        "the new length is a new key, so its own first hash is spent"
    );

    // (b) Same length, different content, different mtime — the residual case
    // the SOLL names. An edit that preserves the byte count still invalidates,
    // because the write bumps the mtime.
    let same_size = payload(longer.len(), 0x22);
    assert_eq!(
        same_size.len(),
        longer.len(),
        "this case is only about a preserved length"
    );
    std::fs::write(&fixture.path, &same_size).unwrap();
    fixture.set_mtime(Fixture::mtime_anchor() + Duration::from_secs(60));
    assert_eq!(
        fixture.identity(),
        FileContentIdentity::Hashed(identity_of_bytes(&same_size)),
        "same length + new mtime must resolve to the new content's identity"
    );
    assert_ne!(
        fixture.identity(),
        after_length,
        "a same-length rewrite must not keep the previous identity"
    );
}

/// The residual case the task brief's `(path, mtime, len)` key gets **wrong**,
/// and the reason the key carries `ctime`. A rewrite that preserves the byte
/// length *and* restores the mtime still changes the kernel-maintained inode
/// change time, so the memo must miss.
///
/// This mirrors the committed guard
/// `thumbnail_source_replacement_invalidates_cached_and_pending_state`
/// ("content identity must change even with identical mtime/length"); the memo
/// must not silently weaken it.
#[cfg(unix)]
#[test]
fn same_length_rewrite_with_restored_mtime_still_forces_a_new_identity() {
    let original = payload(150 * 1024, 0x44);
    let fixture = Fixture::new("ctime-guard.arw", &original);
    let before = fixture.identity();
    assert_eq!(
        before,
        FileContentIdentity::Hashed(identity_of_bytes(&original))
    );
    assert_eq!(fixture.hashes(), Some(1));

    // Same length, different content, mtime put back to the original value.
    let mut replacement = original.clone();
    replacement[0] ^= 0x01;
    assert_eq!(replacement.len(), original.len());
    std::fs::write(&fixture.path, &replacement).unwrap();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&fixture.path)
        .unwrap();
    file.set_len(original.len() as u64).unwrap();
    file.set_modified(Fixture::mtime_anchor()).unwrap();
    drop(file);
    fixture.restore_mtime();

    let metadata = std::fs::metadata(&fixture.path).unwrap();
    assert_eq!(
        metadata.modified().unwrap(),
        Fixture::mtime_anchor(),
        "the fixture must really have the mtime restored, else it proves nothing"
    );
    assert_eq!(metadata.len(), original.len() as u64);

    assert_eq!(
        fixture.identity(),
        FileContentIdentity::Hashed(identity_of_bytes(&replacement)),
        "a same-length rewrite with a restored mtime must not serve the old hash"
    );
    assert_ne!(
        fixture.identity(),
        before,
        "content identity must change even with identical mtime/length"
    );
}

// ---- 3. Missing / unreadable stay distinct and are never memoized ----

/// A missing source is `Missing`; an unreadable one is `Unavailable` with the
/// OS error text. Neither class is ever *stored*: a failure is never memoized
/// as a successful identity, so a file that later appears — or becomes readable
/// — resolves freshly instead of inheriting a remembered failure.
///
/// Note the deliberate boundary (documented in the SOLL § 3): a path whose
/// content hash was *already* computed keeps serving that hash even if its
/// permissions are revoked afterwards, because `chmod` changes neither mtime nor
/// length and the content genuinely did not change. The `Unavailable` class is
/// about a file whose identity was never determined; a determined identity is a
/// fact about bytes that are still on disk.
#[test]
fn missing_and_unreadable_files_keep_their_distinct_uncached_behaviour() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent.arw");
    assert_eq!(
        FileContentIdentity::from_path(&absent),
        FileContentIdentity::Missing
    );
    assert_eq!(
        source_identity::hash_count(&absent),
        None,
        "a missing file must not be memoized as an identity"
    );
    assert_eq!(
        FileContentIdentity::from_path(&absent),
        FileContentIdentity::Missing,
        "and the miss must be retried, not remembered"
    );

    // It appears: the remembered `Missing` must not survive.
    let bytes = payload(64, 0x33);
    std::fs::write(&absent, &bytes).unwrap();
    assert_eq!(
        FileContentIdentity::from_path(&absent),
        FileContentIdentity::Hashed(identity_of_bytes(&bytes))
    );

    // Never-readable file: `stat` succeeds, `open` is denied. The failure class
    // is distinct from `Missing`, it repeats on every call (no memoized
    // success), and it does not poison the path once access is restored.
    let locked = dir.path().join("locked.arw");
    std::fs::write(&locked, &bytes).unwrap();
    restrict_permissions(&locked);
    for _ in 0..2 {
        let identity = FileContentIdentity::from_path(&locked);
        assert!(
            matches!(identity, FileContentIdentity::Unavailable(_)),
            "a permission error must stay Unavailable, not become a hash: {identity:?}"
        );
    }
    assert_eq!(
        source_identity::hash_count(&locked),
        None,
        "a failed read must never be memoized as a successful identity"
    );
    restore_permissions(&locked);
    assert_eq!(
        FileContentIdentity::from_path(&locked),
        FileContentIdentity::Hashed(identity_of_bytes(&bytes)),
        "a readable-again file resolves to its real identity"
    );
    assert_eq!(source_identity::hash_count(&locked), Some(1));
}

// ---- 4. The cache changes no identity value ----

/// The memo replaces a chunked whole-file read with a lookup. This pins that
/// both consumers of the value see exactly what the pre-cache code produced:
/// `FileContentIdentity::from_bytes` (one-shot) and
/// `source_fingerprint` over the same bytes. If the chunked hasher ever
/// diverged from the one-shot hash, every persisted `SourceIdentity` would
/// silently change — this is the guard against that.
#[test]
fn memoized_hash_equals_the_uncached_full_read_hash() {
    for len in [
        0_usize,
        1,
        64 * 1024 - 1,
        64 * 1024,
        64 * 1024 + 1,
        250 * 1024,
    ] {
        let bytes = payload(len, 0x7E);
        let fixture = Fixture::new("equivalence.arw", &bytes);
        let one_shot = FileContentIdentity::from_bytes(&bytes);
        assert_eq!(
            fixture.identity(),
            one_shot,
            "chunked read must hash identically at {len} bytes"
        );
        let fingerprint = source_identity::source_fingerprint_of(&fixture.path).unwrap();
        assert_eq!(
            fingerprint.content_hash,
            identity_of_bytes(&bytes),
            "content hash mismatch at {len} bytes"
        );
        assert_eq!(
            fingerprint.byte_length, len as u64,
            "byte length mismatch at {len} bytes"
        );
        assert!(fingerprint.extras.is_empty());
    }
}

// ---- 5. persisted_action_identity precedence is unchanged ----

/// The thumbnail/neighbor identity bundle still resolves in the documented
/// precedence — source image, then sidecar document, then the source-action
/// bundle — and the memo does not alter any of those values. A missing sidecar
/// still yields `Missing` for the document and *no* bundle; a valid
/// source-action sidecar still yields both.
#[test]
fn bundle_identity_precedence_and_source_action_bundle_are_unchanged() {
    // No sidecar at all: the source is hashed, the document is `Missing`, and
    // there is no bundle — and repeated resolution is free.
    let plain = Fixture::new("plain.arw", &payload(1024, 0x01));
    let first = crate::source_actions::sidecar_bundle_identity(&plain.path, THUMB_VIRTUAL_COPY);
    let second = crate::source_actions::sidecar_bundle_identity(&plain.path, THUMB_VIRTUAL_COPY);
    assert_eq!(first, second, "an unchanged bundle identity must be stable");
    assert!(!first.has_source_action_bundle());
    assert_eq!(
        plain.hashes(),
        Some(1),
        "resolving the same bundle again must not re-hash the source"
    );

    // A real source-action sidecar: the bundle component is present, and the
    // resolved identity is again stable without a second source hash.
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let with_action =
        crate::source_actions::sidecar_bundle_identity(&fixture.source, THUMB_VIRTUAL_COPY);
    assert!(
        with_action.has_source_action_bundle(),
        "a source-action sidecar must keep its bundle component"
    );
    let before = source_identity::hash_count(&fixture.source);
    let again = crate::source_actions::sidecar_bundle_identity(&fixture.source, THUMB_VIRTUAL_COPY);
    assert_eq!(
        with_action, again,
        "the bundle identity value must not drift"
    );
    assert_eq!(
        source_identity::hash_count(&fixture.source),
        before,
        "re-resolving an unchanged source-action bundle must not re-hash it"
    );

    // Changing the source must change the bundle identity, so a replaced
    // original can never keep the old cell state.
    let mut changed = std::fs::read(&fixture.source).unwrap();
    changed.extend_from_slice(b"appended");
    std::fs::write(&fixture.source, &changed).unwrap();
    let after_change =
        crate::source_actions::sidecar_bundle_identity(&fixture.source, THUMB_VIRTUAL_COPY);
    assert_ne!(
        after_change, with_action,
        "a changed source must force a new bundle identity"
    );
}

// ---- Helpers ----

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
}

#[cfg(unix)]
fn restore_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(not(unix))]
fn restore_permissions(_path: &Path) {}
