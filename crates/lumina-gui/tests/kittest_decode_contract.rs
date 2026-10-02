//! KITT-DECODE-CONTRACT-52: the decode-settled contract is **measured**, not
//! prose, and the settled exit has headless coverage.
//!
//! The shared wait (`kittest_decode_support`) can end before its bound, and the
//! soundness of that rests on one condition: the ready state must be *decidable
//! once the decode settled*, i.e. readable from what `finish_decode` writes
//! synchronously. The wait machinery now **enforces** that structurally — the
//! settled exit lives in `pump_until_ready`, which accepts only a
//! `DecodeSettled`, while a later-async state (`Ready`) can only reach the
//! bound-only `pump` (see the module docs there for the measurement that moved
//! the contract out of prose).
//!
//! A type split can only carry a claim, though: it says *that* a caller declared
//! the contract, never *that the claim is true*. These tests are the other half.
//!
//! 1. **What the contract permits is decided when.** For every read the
//!    contract names — the render generation, `preview()`, the adopted
//!    document, the error banner, `decode_pending()` — the first frame on which
//!    a settled decode decides it is the frame the settled exit could fire on,
//!    i.e. **offset 0**. A read decided a frame *later* would make the early
//!    exit unsound: the loop re-reads the ready state and only then tests the
//!    exit, so a later decision is exactly the window where a still-arriving
//!    state gets reported as unreachable. Measured here on a real decode of a
//!    sidecar-carrying fixture (log below).
//! 2. **The exit itself, without an adapter.** `kittest_decode_bound` pins both
//!    exits but is `#[ignore]`d with every other wgpu suite, so on a host
//!    without an adapter the settled exit had **no** run at all. The wait reads
//!    `Harness` state and needs no renderer (the module says so), so it is
//!    covered here.
//! 3. **The later-async side.** A state a folder scan feeds is *waited for*
//!    through the bound-only wait, in the very window in which the settled exit
//!    would have fired — the shape `folder_scan_settle` pins the premise of,
//!    here turned onto the wait this task changed.
//!
//! **No GPU required** — every claim below is about `LuminaApp` state, so this
//! target runs in the normal test run, unlike the goldens.

mod kittest_decode_support;
mod kittest_sidecar_support;

use egui_kittest::Harness;
use kittest_decode_support::{
    is_settled, pump, pump_until_ready, DecodeSettled, Ready, SETTLE_DEADLINE,
};
use kittest_sidecar_support::seed_metadata_history_sidecar;
use lumina_gui::LuminaApp;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The seeded sidecar's history length, from the 10 metadata commits the seeder
/// writes. A non-trivial value on purpose: "no history" would also be the value
/// of a decode that never adopted the document, so it could not tell the read
/// being decided from the read being absent.
const SEEDED_HISTORY: usize = 10;

/// A 4x3 synthetic PNG on disk: the fixture class the mask-local suites use,
/// decodable in milliseconds.
fn write_png(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, LuminaApp::sample_image_png()).expect("write png fixture");
    path
}

/// A headless harness **without** the wgpu renderer — every claim here is about
/// `LuminaApp` state, which the frame loop produces without a renderer.
fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// The state the contract names, sampled on one frame.
#[derive(Clone, Debug)]
struct Frame {
    /// The settled-exit condition itself, i.e. the frame the exit may fire on.
    settled: bool,
    rendered: bool,
    preview: bool,
    decoding: bool,
    banner: Option<String>,
    history: usize,
    draft: usize,
}

impl Frame {
    fn of(app: &LuminaApp) -> Self {
        Frame {
            settled: is_settled(app),
            rendered: app.preview_generation() >= 1,
            preview: app.preview().is_some(),
            decoding: app.decode_pending(),
            banner: app.error().map(str::to_owned),
            history: app.metadata_history().len(),
            draft: app.metadata_draft().len(),
        }
    }
}

/// Step frames until the decode settles, and return the trace of every frame up
/// to and including that one.
///
/// The bound is the production one and the failure is loud: a decode that never
/// settles has to say so instead of ending the trace early and letting the
/// assertions read a frame that is not the one they name.
fn trace_until_settled(harness: &mut Harness<'_, LuminaApp>) -> Vec<Frame> {
    let deadline = Instant::now() + SETTLE_DEADLINE;
    let mut trace = Vec::new();
    loop {
        harness.run_steps(1);
        trace.push(Frame::of(harness.state()));
        if trace.last().expect("one frame per step").settled {
            return trace;
        }
        assert!(
            Instant::now() < deadline,
            "the fixture decode never settled within {SETTLE_DEADLINE:?} after {} frames: \
             last state {trace:?}",
            trace.len()
        );
    }
}

/// Every read the contract permits is decided **on** the frame the settled exit
/// could fire on.
///
/// The predicate under test is the *window*: the loop steps a frame, re-reads the
/// ready state, and only then tests the settled exit, so a read that became
/// decidable one frame later is precisely a read the exit can misreport. All five
/// reads are checked on the settle frame itself, and the frame index is logged
/// rather than only asserted, because the count is the measurement (it is
/// machine- and load-dependent; the offset 0 is the claim).
#[test]
fn a_settled_decode_decides_every_read_the_contract_permits() {
    let dir = tempfile::tempdir().expect("temp dir");
    // A sidecar whose identity matches the bytes on disk, so the decode adopts
    // it: the document is the read that `finish_decode` writes **synchronously**
    // (before the render), and it is the one the contract's hardest case rests on.
    let photo = seed_metadata_history_sidecar(dir.path(), |_| {});

    let mut harness = build_harness();
    harness.state_mut().open_file(photo.display().to_string());
    let trace = trace_until_settled(&mut harness);
    let settle = trace.last().cloned().expect("a settled frame");
    let index = trace.len() - 1;
    eprintln!(
        "KITT-DECODE-CONTRACT-52 settled decode on frame {index} ({} frames traced): {settle:?}",
        trace.len()
    );

    assert!(
        settle.rendered,
        "the render generation must be decided on the settle frame, not one later: \
         the loop re-reads the ready state and only then tests the settled exit, so a \
         render that lands after this frame is a state the exit would misreport\n{settle:?}"
    );
    assert!(
        settle.preview,
        "preview() is written by the same render as the generation, so it is decided \
         on the same frame\n{settle:?}"
    );
    assert!(
        !settle.decoding,
        "decode_pending() is a premise of the settled condition itself, so it is \
         already false here\n{settle:?}"
    );
    assert!(
        settle.banner.is_none(),
        "a sidecar whose identity matches the bytes on disk must be adopted silently: \
         a banner here would mean the decode reported, which is a different (and \
         equally decided) outcome — but not this one\n{settle:?}"
    );
    assert_eq!(
        settle.history, SEEDED_HISTORY,
        "the adopted document must be readable on the settle frame: `finish_decode` \
         sets it before the render, which is why a history length is a state the \
         settled exit may decide\n{settle:?}"
    );
    assert!(
        settle.draft > 0,
        "the adopted document's draft must be readable on the same frame as its \
         history, not one later\n{settle:?}"
    );
}

/// The settled exit reports a declared state the decode cannot reach — at once,
/// and with the full state report.
///
/// This is the exit `pump_until_ready` is the only place in the tree to arm, and
/// the state here (`preview_generation() >= 2`) is declared in the
/// decode-settled form because a render generation *is* decided by the decode.
/// Without the exit the wait would spend the whole 300 s bound instead, so the
/// elapsed time is asserted — as an A/B against that bound, and logged, not as a
/// threshold tuned to this machine.
#[test]
fn a_declared_state_the_decode_cannot_reach_is_reported_at_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = write_png(dir.path(), "photo.png");

    let mut harness = build_harness();
    let started = Instant::now();
    let payload = catch_unwind(AssertUnwindSafe(|| {
        pump_until_ready(
            &mut harness,
            &photo,
            DecodeSettled::new("preview_generation() >= 2", |app: &LuminaApp| {
                app.preview_generation() >= 2
            }),
            SETTLE_DEADLINE,
        )
    }))
    .expect_err("an unfulfillable declared state must give up loudly, not return");
    let elapsed = started.elapsed();
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .expect("the wait must panic with a message");

    assert!(
        message.contains("cause:     the decode settled"),
        "the cause must name the settled exit, not the bound:\n{message}"
    );
    assert!(
        message.contains("expected:  preview_generation() >= 2"),
        "the report must name the declared state it was waiting for:\n{message}"
    );
    assert!(
        message.contains("preview_generation=1"),
        "the fixture must have rendered exactly once, so the declared state is \
         unreachable and the decode is terminal:\n{message}"
    );
    assert!(
        is_settled(harness.state()),
        "the settled exit must only fire on a terminal state"
    );
    assert!(
        elapsed < SETTLE_DEADLINE / 10,
        "a settled decode must be reported at once, took {elapsed:?} against the \
         production bound {SETTLE_DEADLINE:?}"
    );
    eprintln!(
        "KITT-DECODE-CONTRACT-52 settled exit headless: gave up after {elapsed:?} \
         (bound {SETTLE_DEADLINE:?})"
    );
}

/// A state a **later** async source feeds is waited for, in the very window in
/// which the settled exit would have fired — and it arrives.
///
/// `Ready` + `pump` is the only combination a later-async state can take, and it
/// is the combination this task guarantees by type. The premise (a settled decode
/// with a scan in flight) is the one `folder_scan_settle` pins over 25 runs; what
/// is new here is the consequence for the wait whose signature changed: with a
/// settled exit reachable from `pump`, this wait would panic in the window below
/// instead of reaching the listing.
#[test]
fn the_bound_only_wait_reaches_a_state_the_settled_exit_cannot_decide() {
    let source_dir = tempfile::tempdir().expect("temp dir");
    let photo = write_png(source_dir.path(), "photo.png");
    let library = tempfile::tempdir().expect("temp dir");
    write_png(library.path(), "one.png");
    write_png(library.path(), "two.png");

    let mut harness = build_harness();
    pump_until_ready(
        &mut harness,
        &photo,
        DecodeSettled::new("preview_generation() >= 1", |app: &LuminaApp| {
            app.preview_generation() >= 1
        }),
        SETTLE_DEADLINE,
    );
    // The decode wait above ends when the *decode* settles; the source
    // folder's scan is a later async source and may still be in flight then
    // (CI measured `entries().len() == 0` with `scan_pending()` at the premise
    // below: the 4x3 decode settling before the scan worker delivers). Wait
    // the source listing out with the bound-only wait — a `Ready`, never a
    // settled exit — so the premise holds on every machine, not just fast
    // ones. Durable because `begin_scan` never touches `entries` (only a
    // later frame's `poll_scan`/`apply_listing` can) and no frame runs between
    // this return and `set_directory` below.
    pump(
        &mut harness,
        source_dir.path(),
        Ready::new("entries().len() == 1", |app: &LuminaApp| {
            app.entries().len() == 1
        }),
        SETTLE_DEADLINE,
    );
    // A navigation with **no frame pumped yet**: the decode is terminal and the
    // scan worker is armed, so this is the window in which a settled exit would
    // fire and report the listing as unreachable.
    harness
        .state_mut()
        .set_directory(library.path().display().to_string());
    {
        let app = harness.state();
        assert!(
            is_settled(app),
            "premise: the decode is terminal before the new listing is applied"
        );
        assert!(
            app.scan_pending(),
            "premise: the folder scan is in flight while the decode is settled"
        );
        assert_eq!(
            app.entries().len(),
            1,
            "premise: the listing is still the previous folder's"
        );
    }

    pump(
        &mut harness,
        library.path(),
        Ready::new("entries().len() == 2", |app: &LuminaApp| {
            app.entries().len() == 2
        }),
        SETTLE_DEADLINE,
    );

    let app = harness.state();
    assert_eq!(
        app.entries().len(),
        2,
        "the bound-only wait must reach a state the settled exit cannot decide"
    );
    assert!(
        !app.scan_pending(),
        "and it must return with the scan drained, not merely with the listing applied"
    );
}
