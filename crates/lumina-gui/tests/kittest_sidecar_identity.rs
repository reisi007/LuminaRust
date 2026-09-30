//! KITT-IDENTITY-49: a sidecar whose `SourceIdentity` does not match the bytes
//! that were decoded is **refused**, loudly, and the document stays unloaded.
//!
//! `LuminaApp::finish_decode` validates a sidecar it found against the frame it
//! just decoded — `source_actions::source_fingerprint_matches` compares
//! `content_hash` **and** `byte_length` (R3-OPEN-1) — and a mismatch is reported
//! as a `source identity conflict` banner with the sidecar not adopted. That is
//! the required product behaviour and it is unchanged.
//!
//! These two tests are the counterpart of the repaired `library_meta_history`
//! seed. That golden now writes the *real* fingerprint of the bytes it wrote, so
//! its sidecar is adopted; this file proves the refusal still happens, which is
//! what keeps the repair a correction instead of an exception.
//!
//! `sidecar_with_real_identity_is_adopted` is the non-vacuity anchor: it runs
//! the identical fixture through the identical harness and asserts the document
//! *is* loaded. Without it, the empty state asserted in
//! `stale_sidecar_identity_is_rejected` could come from anything at all (a
//! sidecar that was never written, a fixture the app cannot find) and would
//! prove nothing about the rejection.
//!
//! Requires a working GPU / headless wgpu backend, so both tests are
//! `#[ignore]`d by the same policy as the goldens. Run locally with:
//!
//! ```text
//! cargo test -p lumina-gui --test kittest_sidecar_identity -- --ignored --test-threads=1
//! ```

mod kittest_decode_support;
mod kittest_sidecar_support;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use kittest_decode_support::{is_settled, pump_until_ready, DecodeSettled, SETTLE_DEADLINE};
use kittest_sidecar_support::{seed_metadata_history_sidecar, written_source_identity};
use lumina_gui::LuminaApp;
use lumina_sidecar::SourceIdentity;

/// Ready state: the background decode settled and reported its outcome — the
/// state a caller that waits for the *decode result* itself needs. The
/// condition is the shared [`is_settled`], so there is no second copy of it
/// here; only the description lives with this file's two call sites.
///
/// `DecodeSettled` and not `Ready` is the declared form of the contract
/// (KITT-DECODE-CONTRACT-52): the decode itself decides this state, so it may
/// be paired with the settled exit. A later-async state — a folder listing, a
/// thumbnail — is a `Ready` and cannot reach this wait at all.
fn ready_settled() -> DecodeSettled<impl FnMut(&LuminaApp) -> bool> {
    DecodeSettled::new(
        "the decode settled (is_settled: !decode_pending() && reported)",
        is_settled,
    )
}

/// The production refusal wording (`LuminaApp::finish_decode`); asserted
/// verbatim so a silent adoption cannot pass as a differently-worded message.
const CONFLICT: &str = "source identity conflict";

/// The title the seeded sidecar ends with, i.e. what an *adopted* document
/// would expose through `metadata_draft`.
const SEEDED_TITLE: &str = "Titel 10";

/// Breaks exactly one field of a truthful [`SourceIdentity`].
type Corrupt = fn(&mut SourceIdentity);

fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Seed the fixture with a **truthful** identity and prove the app adopts it:
/// the document is loaded, so the empty state the rejection test asserts can
/// only come from the rejection.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_sidecar_identity -- --ignored"]
fn sidecar_with_real_identity_is_adopted() {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = seed_metadata_history_sidecar(dir.path(), |_| {});
    // The fixture really carries the fingerprint of the bytes on disk — a
    // fabricated identity is not a way to make this test pass.
    assert_eq!(
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&photo))
            .expect("seeded sidecar loads")
            .source,
        written_source_identity(&photo),
        "the seeded identity must be the fingerprint of the written bytes"
    );
    let mut harness = build_harness();
    pump_until_ready(&mut harness, &photo, ready_settled(), SETTLE_DEADLINE);
    let app = harness.state();
    assert!(
        app.error().is_none(),
        "a sidecar whose identity matches the decoded bytes must be adopted: {:?}",
        app.error()
    );
    assert_eq!(
        app.metadata_draft().get("title").map(String::as_str),
        Some(SEEDED_TITLE),
        "the adopted document must expose the seeded draft"
    );
    assert_eq!(
        app.metadata_history().len(),
        10,
        "the adopted document must expose the seeded history"
    );
}

/// A sidecar whose identity lies is refused: the banner is raised, rendered and
/// names the conflict, and the document is never loaded.
///
/// Both halves of `source_fingerprint_matches` are broken independently, so
/// neither can regress unnoticed. `byte_length: 0` is the exact defect the
/// `library_meta_history` seed carried.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_sidecar_identity -- --ignored"]
fn stale_sidecar_identity_is_rejected() {
    let cases: [(&'static str, Corrupt); 2] = [
        ("content_hash", |identity: &mut SourceIdentity| {
            identity.content_hash = format!("blake3:{}", "0".repeat(64));
        }),
        ("byte_length", |identity: &mut SourceIdentity| {
            identity.byte_length = 0
        }),
    ];
    for (field, corrupt) in cases {
        assert_stale_sidecar_rejected(field, corrupt);
    }
}

/// Open a sidecar whose `field` lies and assert the refusal is loud and total.
fn assert_stale_sidecar_rejected(field: &str, corrupt: Corrupt) {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = seed_metadata_history_sidecar(dir.path(), corrupt);
    // Sanity: the sidecar on disk really is stale for this one field, so a
    // green test cannot come from the corruption not having been written.
    let stored = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&photo))
        .expect("seeded sidecar loads")
        .source;
    let honest = written_source_identity(&photo);
    assert_ne!(
        stored, honest,
        "the {field} corruption must actually reach the sidecar on disk"
    );

    let mut harness = build_harness();
    pump_until_ready(&mut harness, &photo, ready_settled(), SETTLE_DEADLINE);

    // 1. The refusal is loud: the production message, naming the conflict.
    let error = harness.state().error().unwrap_or_else(|| {
        panic!(
            "a sidecar with a broken {field} must be refused with an error banner, \
             not adopted silently (KITT-IDENTITY-49)"
        )
    });
    assert!(
        error.contains(CONFLICT),
        "the refusal must name the source identity conflict, got {error:?}"
    );
    // 2. It is *visible*, not only state: the header paints the message in red.
    assert!(
        harness
            .query_all_by_label_contains(CONFLICT)
            .next()
            .is_some(),
        "the {CONFLICT:?} banner must be rendered in the header, but no accessible \
         node carries it"
    );
    // 3. A background decode failure is a header banner, never a modal dialog
    //    (KITTEST-COVERAGE-STATES-2a: browsing a folder must not stack dialogs).
    assert!(
        !harness.state().error_dialog_open(),
        "a refused sidecar is a background failure and must not open the error dialog"
    );
    // 4. The document was never loaded: the seeded draft and history stay
    //    invisible, which is what `document: None` looks like from outside.
    assert!(
        harness.state().metadata_draft().is_empty(),
        "a refused sidecar must not be adopted (draft would be {SEEDED_TITLE:?})"
    );
    assert!(
        harness.state().metadata_history().is_empty(),
        "a refused sidecar must not be adopted (history would carry 10 entries)"
    );
}
