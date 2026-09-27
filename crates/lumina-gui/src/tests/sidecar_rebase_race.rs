//! R3-CONFLICT-1: the CAS-rebase path under a real two-writer race.
//!
//! Background: the R3 measurement flagged `R3-CONFLICT-1` as "ungeprüft" — the
//! single-instance GUI run produced 6 sidecar saves but 0 rebases, so the
//! rebase logic was never observed live (only unit-tested with the
//! per-thread `set_conflict_hook` seam).
//!
//! These headless tests close that gap with **two real threads** racing one
//! sidecar path through the production [`crate::sidecar_rebase::save_rebased`]
//! CAS writer. A `Barrier` forces the collision deterministically enough to
//! exercise the rebase branch while the loop repeats the race so an unlucky
//! interleaving cannot hide a regression.
//!
//! Pinned per the task acceptance:
//! 1. **Rebase fires** — at least one writer observed `rebased == true`.
//! 2. **No data loss** — both writers' concurrent edits are present in the
//!    final file (field-selective merge, never last-writer-wins-wholesale).
//! 3. **Deterministic** — the final merged document is identical across
//!    repeated runs (same inputs → same bytes), independent of who wins.
//!
//! ## What is *not* automatable here (documented, not silently skipped)
//!
//! A genuine second **process/instance** (two `lumina-gui` binaries on one
//! sidecar) is not reachable from `cargo test -p lumina-gui`: it needs the
//! native app, a real window and a live sidecar directory. Its manual
//! reproduction is documented in `feature/platform/cli-gui-wasm.md`
//! (R3-CONFLICT-1) with the exact commands, and the code path it would exercise
//! is the *same* `save_rebased` these thread-level tests drive. The thread race
//! therefore covers the logic; only the OS-level process scheduling differs.

use super::*;
use lumina_sidecar::{save_sidecar_if_unchanged, SidecarDocument, SidecarError};
use std::sync::{Arc, Barrier};
use std::thread;

fn sidecar_document() -> SidecarDocument {
    SidecarDocument::new(
        lumina_sidecar::SourceIdentity {
            relative_name: "race.png".into(),
            content_hash: "blake3:race".into(),
            byte_length: 4,
            modified_at: None,
            raw_format: "PNG".into(),
            orientation: 1,
            decode_fingerprint: lumina_sidecar::DecodeFingerprint {
                decoder: "image".into(),
                version: "1".into(),
                parameters: Default::default(),
                extras: Default::default(),
            },
            geometry_fingerprint: lumina_sidecar::GeometryFingerprint {
                width: 2,
                height: 2,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Default::default(),
            },
            extras: Default::default(),
        },
        "raster-mvp-1",
    )
}

/// One raced save: both writers start from the same revision and each applies a
/// distinct recipe adjustment. Returns `(writer_a_rebased, writer_b_rebased,
/// final_adjustments)`.
fn race_once() -> (bool, bool, BTreeMap<String, f64>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("race.png.lumina.json");
    let base = sidecar_document();
    // Materialize the file so both writers share a real expected revision.
    let base_revision = save_sidecar_if_unchanged(&path, &base, None).unwrap();

    let mut local_a = base.clone();
    local_a.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 0.25);
    let mut local_b = base.clone();
    local_b.virtual_copies[0]
        .recipe
        .adjustments
        .insert("contrast".into(), -0.4);

    let barrier = Arc::new(Barrier::new(2));
    let spawn = |local: SidecarDocument, barrier: Arc<Barrier>| {
        let path = path.clone();
        let base = base.clone();
        let revision = base_revision.clone();
        thread::spawn(move || {
            // Both writers reach the CAS at the same time; one loses and rebases.
            barrier.wait();
            crate::sidecar_rebase::save_rebased(
                &path,
                &base,
                &local,
                Some(&revision),
                crate::sidecar_rebase::MAX_REBASE_ATTEMPTS,
            )
        })
    };
    let a = spawn(local_a, Arc::clone(&barrier));
    let b = spawn(local_b, Arc::clone(&barrier));
    let ra = a.join().expect("writer A must not panic");
    let rb = b.join().expect("writer B must not panic");

    let (a_rebased, b_rebased) = match (&ra, &rb) {
        (Ok(out_a), Ok(out_b)) => (out_a.rebased, out_b.rebased),
        (Err(e), _) | (_, Err(e)) => panic!("a raced save failed loudly: {e}"),
    };

    let final_doc = lumina_sidecar::load_sidecar(&path).unwrap();
    let adjustments = final_doc.virtual_copies[0].recipe.adjustments.clone();
    (a_rebased, b_rebased, adjustments)
}

/// The two real-thread race: the rebase branch fires without losing either
/// writer's concurrent edit. Repeated so an unlucky schedule cannot mask a
/// regression; the assertion is on the *invariant*, not on who wins.
#[test]
fn two_real_writers_rebase_without_data_loss() {
    let mut rebase_observed = false;
    for iteration in 0..32 {
        let (a_rebased, b_rebased, adjustments) = race_once();
        rebase_observed |= a_rebased || b_rebased;
        assert_eq!(
            adjustments.get("exposure"),
            Some(&0.25),
            "iteration {iteration}: writer A's edit must survive the race"
        );
        assert_eq!(
            adjustments.get("contrast"),
            Some(&-0.4),
            "iteration {iteration}: writer B's edit must survive the race"
        );
    }
    assert!(
        rebase_observed,
        "a real two-writer race must exercise the rebase branch at least once"
    );
}

/// Determinism: the same two concurrent inputs produce the same final document
/// bytes across repeated races — the merge is order-independent for distinct
/// fields, so the outcome does not depend on who wins the CAS.
#[test]
fn race_outcome_is_deterministic_across_runs() {
    let mut revisions = std::collections::BTreeSet::new();
    for _ in 0..24 {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("race.png.lumina.json");
        let base = sidecar_document();
        let revision = save_sidecar_if_unchanged(&path, &base, None).unwrap();
        let mut local_a = base.clone();
        local_a.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 0.25);
        let mut local_b = base.clone();
        local_b.virtual_copies[0]
            .recipe
            .adjustments
            .insert("contrast".into(), -0.4);
        let barrier = Arc::new(Barrier::new(2));
        let spawn = |local: SidecarDocument, barrier: Arc<Barrier>| {
            let path = path.clone();
            let base = base.clone();
            let revision = revision.clone();
            thread::spawn(move || {
                barrier.wait();
                crate::sidecar_rebase::save_rebased(
                    &path,
                    &base,
                    &local,
                    Some(&revision),
                    crate::sidecar_rebase::MAX_REBASE_ATTEMPTS,
                )
            })
        };
        let a = spawn(local_a, Arc::clone(&barrier));
        let b = spawn(local_b, Arc::clone(&barrier));
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
        // Revision is a BLAKE3 over the JSON: identical documents → identical
        // revision bytes, so the set must collapse to one value.
        revisions.insert(
            lumina_sidecar::document_revision(&lumina_sidecar::load_sidecar(&path).unwrap())
                .unwrap(),
        );
    }
    assert_eq!(
        revisions.len(),
        1,
        "the merged result must be byte-deterministic across races, got {revisions:?}"
    );
}

/// A writer that never rebases (a fresh document, `expected == None`) refuses a
/// concurrently appearing file loudly — the documented "no common ancestor"
/// rule, proving the race proof above is not the only conflict branch.
#[test]
fn fresh_document_never_rebases_over_a_concurrent_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("race.png.lumina.json");
    let base = sidecar_document();
    // A file appears while a fresh writer (expected == None) is saving.
    save_sidecar_if_unchanged(&path, &base, None).unwrap();
    let fresh = sidecar_document();
    let result = crate::sidecar_rebase::save_rebased(
        &path,
        &base,
        &fresh,
        None,
        crate::sidecar_rebase::MAX_REBASE_ATTEMPTS,
    );
    assert!(
        matches!(result, Err(SidecarError::Conflict(_))),
        "a fresh document must refuse a concurrent file, got {result:?}"
    );
}
