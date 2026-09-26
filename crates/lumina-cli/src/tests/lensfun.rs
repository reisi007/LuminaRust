// F-098-N2: feature-gated CLI->Lensfun wiring tests (see mod.rs cfg).

use super::*;
// The merge of `origin/main` (2026-09-26) moved the lens corrector into
// `src/lensfun_cli.rs`, so `RawMetadata` is no longer in scope through the
// binary root's imports. Named explicitly, like every other payload type here:
// a test that depends on which `use` lines the root happens to carry breaks on
// an unrelated extraction.
use lumina_raw::RawMetadata;

// Build a `RawMetadata` from the minimal EXIF fields the CLI wiring
// inspects. All other fields are left at inert defaults — the wiring
// only reads make/model/focal_length/aperture/width/height.
fn make_metadata(
    make: Option<&str>,
    model: Option<&str>,
    focal_length: Option<f32>,
    aperture: Option<f32>,
) -> RawMetadata {
    RawMetadata {
        width: 1000,
        height: 750,
        orientation: 1,
        camera_make: make.map(str::to_string),
        camera_model: model.map(str::to_string),
        iso: None,
        shutter: None,
        aperture,
        lens: None,
        focal_length,
        timestamp: None,
        artist: None,
        description: None,
        camera_matrix: [[0.0; 4]; 3],
        camera_white_balance: [1.0; 4],
        pre_multipliers: [1.0; 4],
        icc_profile: None,
    }
}

// The same real camera the `lumina-lensfun` native tests use, so the
// installed profile database is guaranteed to contain a matching,
// non-identity profile (distortion + vignetting).
const MAKE: &str = "Nikon Corporation";
const MODEL: &str = "Nikon D40";

#[test]
fn real_camera_with_full_exif_yields_corrector() {
    let metadata = make_metadata(Some(MAKE), Some(MODEL), Some(18.0), Some(5.6));
    let (_db, corrector) = build_lensfun_corrector(Some(&metadata))
        .expect("a Lensfun corrector for the known {MAKE} {MODEL} profile");
    // The modifier references lens data owned by the DB; `_db` is dropped
    // after `corrector`, so the handle stays alive while the corrector is used.
    assert!(
        !corrector.is_identity(),
        "the resolved Nikon D40 profile must be a non-identity correction"
    );
}

#[test]
fn missing_make_yields_none() {
    let metadata = make_metadata(None, Some(MODEL), Some(18.0), Some(5.6));
    assert!(build_lensfun_corrector(Some(&metadata)).is_none());
}

#[test]
fn missing_model_yields_none() {
    let metadata = make_metadata(Some(MAKE), None, Some(18.0), Some(5.6));
    assert!(build_lensfun_corrector(Some(&metadata)).is_none());
}

#[test]
fn missing_focal_length_yields_none() {
    let metadata = make_metadata(Some(MAKE), Some(MODEL), None, Some(5.6));
    assert!(build_lensfun_corrector(Some(&metadata)).is_none());
}

#[test]
fn missing_aperture_yields_none() {
    let metadata = make_metadata(Some(MAKE), Some(MODEL), Some(18.0), None);
    assert!(build_lensfun_corrector(Some(&metadata)).is_none());
}

#[test]
fn no_metadata_yields_none() {
    assert!(build_lensfun_corrector(None).is_none());
}

#[test]
fn render_with_corrector_changes_pixels() {
    // Smoke test: feeding a real Lensfun corrector through
    // `RenderContext.lensfun` must actually alter the rendered pixels
    // versus the manual/identity model (`None`).
    //
    // A *uniform* frame is invariant under lensfun: distortion only remaps
    // positions (uniform → uniform) and the small vignette rounds back to
    // the same 8-bit value. So we use a spatial gradient: distortion then
    // moves different source positions under each destination pixel and the
    // vignette brightens the corners, both of which change 8-bit values.
    let metadata = make_metadata(Some(MAKE), Some(MODEL), Some(18.0), Some(5.6));
    let (_db, corrector) = build_lensfun_corrector(Some(&metadata))
        .expect("a Lensfun corrector for the known profile");
    let width: u32 = 1000;
    let height: u32 = 750;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x / 4 + y / 4) % 256) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    let recipe = lumina_sidecar::EditRecipe::default();

    let rendered_none = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .unwrap();
    let rendered_some = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            #[cfg(feature = "lensfun")]
            lensfun: Some(LensfunCorrectorRef(&corrector)),
            #[cfg(not(feature = "lensfun"))]
            lensfun: None,
            depth: None,
        },
    )
    .unwrap();
    assert_ne!(
        rendered_none.frame.pixels, rendered_some.frame.pixels,
        "a Lensfun corrector must change the rendered pixels"
    );
}

/// LENSFUN-CALLER-37 / F2: the diagnostics sink must be **process-lifetime**.
///
/// `build_lensfun_corrector` runs once per image. If each call constructed a
/// fresh `ReportOnce`, a batch over a machine without a resolvable Lensfun
/// database would print one unlevelled block per image again — the exact defect
/// this wiring removes. What makes the de-duplication work is that the *same*
/// sink instance is handed to every call, so its `seen` set accumulates.
///
/// The de-duplication logic itself is `lumina-lensfun`'s (`ReportOnce`) and is
/// tested there. What is specific to the CLI — and what this pins — is the
/// persistence across calls.
///
/// # Why this asserts on the sink's *state*, not on its address
///
/// The obvious proof — "both calls got the same `*const _`" — is **vacuous in a
/// debug build**, which is how `cargo test` runs: a fresh `ReportOnce` created
/// and dropped inside every call is re-materialised in the same stack slot, so
/// the address is identical for a per-call sink and a process-lifetime one.
/// Measured *in this crate*, with `with_diagnostics` temporarily constructing a
/// fresh `ReportOnce` per call (i.e. exactly the regression below): the
/// address-comparison assertion still **passed**. An address therefore cannot
/// fail if the sink stops being process-lifetime, which is precisely what this
/// test exists to catch.
///
/// Instead it reads the sink's own de-duplication state: `ReportOnce` derives
/// `Debug` and its only field is the `Mutex<BTreeSet<String>>` of keys it has
/// already reported, so `format!("{sink:?}")` shows which events this instance
/// has seen. A **fresh** sink can only ever show the events of its own call, so
/// "the second call's sink still knows the first call's event" is unsatisfiable
/// for a per-call sink and true for a process-lifetime one. No timing, no
/// pointer, no sleep.
///
/// The events are synthetic (`SystemDbError::NotFound` over a made-up probe
/// directory) and keyed on a unique path per test, so they never collide with a
/// real lookup's keys and never depend on whether the host has a Lensfun
/// database. Only the *marker substrings* are asserted, never the `Debug`
/// punctuation around them, so a cosmetic change to `ReportOnce`'s derived
/// formatting does not break this test — a genuinely unreachable sink state
/// does.
#[test]
fn the_diagnostics_sink_persists_across_calls_so_a_batch_reports_once() {
    use lumina_lensfun::db_path::{MissReason, ProbeMiss, Source, SystemDbError};
    use lumina_lensfun::system_load::Diagnostics;

    // Two distinct failures, so a second report proves the sink kept its state
    // while a repeat proves the de-duplication still works on it.
    let failure = |name: &str| SystemDbError::NotFound {
        misses: vec![ProbeMiss {
            dir: std::path::PathBuf::from(format!("/nonexistent/cli-sink-{name}")),
            source: Source::PlatformDefault,
            reason: MissReason::Absent,
        }],
    };
    let alpha = failure("alpha");
    let beta = failure("beta");
    // The marker is part of the error text, which is part of the sink's key.
    const ALPHA_MARKER: &str = "/nonexistent/cli-sink-alpha";
    const BETA_MARKER: &str = "/nonexistent/cli-sink-beta";

    // First call: report a failure and read the sink's state back.
    let after_alpha = crate::lensfun_cli::with_diagnostics(|sink| {
        sink.failed(&alpha);
        format!("{sink:?}")
    });
    assert!(
        after_alpha.contains(ALPHA_MARKER),
        "the failure must have reached the sink, otherwise the assertions below \
         prove nothing: {after_alpha}"
    );
    assert!(
        !after_alpha.contains(BETA_MARKER),
        "the sink must not know a failure nobody reported yet: {after_alpha}"
    );

    // **The decisive call:** a *different* failure, through a *separate*
    // `with_diagnostics` call. Its sink must still remember the first call's
    // failure — which is only possible if both calls received the same
    // instance. A per-call sink starts empty and cannot satisfy this.
    let after_beta = crate::lensfun_cli::with_diagnostics(|sink| {
        sink.failed(&beta);
        format!("{sink:?}")
    });
    assert!(
        after_beta.contains(ALPHA_MARKER),
        "the second call must see the failure the first call already reported: \
         the sink is not process-lifetime, so a per-image lookup re-prints the \
         failure and a batch spams one block per file"
    );
    assert!(
        after_beta.contains(BETA_MARKER),
        "the second call's own failure must be recorded as well: {after_beta}"
    );

    // A third image reporting the *same* failure as the first adds no key: the
    // de-duplication the process-lifetime sink exists for actually works.
    let after_repeat = crate::lensfun_cli::with_diagnostics(|sink| {
        sink.failed(&alpha);
        format!("{sink:?}")
    });
    assert_eq!(
        after_repeat.matches(ALPHA_MARKER).count(),
        1,
        "a repeated identical failure must occupy exactly one key, so the batch \
         reports it once: {after_repeat}"
    );
    assert_eq!(
        after_repeat.matches(BETA_MARKER).count(),
        1,
        "the first call's keys must survive the third call untouched: \
         {after_repeat}"
    );
}

/// LENSFUN-CALLER-37 / F2: the **production call site** must write its database
/// load into the process-lifetime sink.
///
/// The test above proves the *helper* is process-lifetime — but it proves it by
/// being the helper: it calls `with_diagnostics` itself and never goes through
/// `build_lensfun_corrector`. The production seam, `lensfun_cli.rs`
/// `let db = with_diagnostics(LensfunDb::load_system_with)?;`, was therefore
/// unanchored. Replacing exactly that line with a per-call `report_once()` sink
/// left every other test in this file green (measured on this tree: 355 passed /
/// 0 failed), because the seven `build_lensfun_corrector` calls here observe the
/// *corrector* and never the sink. A refactor re-introducing the very defect
/// `LENSFUN-CALLER-37` removed — a fresh sink per image, so a batch re-prints
/// one unlevelled block per file — would have shipped green.
///
/// # What it drives, what it observes
/// `build_lensfun_corrector` is called with a make/model no Lensfun database can
/// contain, so the production function runs its whole body: it loads the system
/// database through `with_diagnostics` (the production line) and then misses in
/// `for_camera`. The sink is afterwards read back through a **separate**
/// `with_diagnostics` call, and two things must be true there:
///
/// 1. a key this test wrote itself is still there — the production call site
///    does not swap or replace the shared sink, and
/// 2. a **real load record** is there — the production call site recorded its
///    load into that same shared instance.
///
/// A sink constructed per call is empty at every read, so both are red when the
/// production line stops using the process-lifetime sink.
///
/// # Machine independence: it asserts that *something* was recorded
/// Which branch the load takes is a property of the host, so the test never
/// claims a particular outcome. That is the trap
/// `feature/platform/capability-matrix.md` §"Pflicht des Aufrufers von
/// `load_system()`" point 2 records for the GUI half: "a failure was recorded"
/// passes vacuously on a machine with a system database and goes red on a bare
/// runner. The markers below are therefore chosen to be **unreachable for any
/// hand-fed sink** in this file:
///
/// * `resolved:` — the key prefix `ReportOnce::resolved` writes. No test in this
///   file produces it, so a sibling test cannot satisfy the assertion instead.
/// * `OverrideUnusable` — the `LUMINA_LENSFUN_DB`-pinned failure variant, which
///   only a real resolution can report.
/// * `/usr/share/lensfun` — in the candidate list of **both**
///   `linux_default_dirs` and `macos_default_dirs`, so a genuinely unresolved
///   real lookup names it. The synthetic keys above live under
///   `/nonexistent/cli-sink-*` and carry none of these.
///
/// On a host **with** a database the load takes the `resolved` path and the
/// first marker matches. On a host **without** one it takes the `failed` path
/// and the marker pair matches. Neither host can satisfy the assertion without a
/// production-path load record, and the per-call-sink mutation satisfies neither.
///
/// # The honest limit
/// The sink is a process-wide set of *host-determined* keys, so no test inside
/// one process can prove that the record it reads was written by *its own* call
/// rather than by a sibling's `build_lensfun_corrector` (all production calls on
/// one host produce the same key, so no delta is observable either). What is
/// pinned is the property that actually broke: a load record written through the
/// production call site is still readable from a later, independent
/// `with_diagnostics` call — the two share one instance. Assertion 1 above is
/// the fully attributed half; it cannot be satisfied by another test at all.
#[test]
fn the_production_call_site_records_its_load_into_the_process_lifetime_sink() {
    use lumina_lensfun::db_path::{MissReason, ProbeMiss, Source, SystemDbError};
    use lumina_lensfun::system_load::Diagnostics;

    // A make/model no profile database can contain. `for_camera` searches with
    // `sflags == 0`, i.e. without `LF_SEARCH_LOOSE` (GUI-ROUTING-N6), so this
    // cannot match a body and the `None` below proves the lookup really ran and
    // really missed instead of short-circuiting before it.
    const PROBE_MAKE: &str = "LuminaRust Regression Probe";
    const PROBE_MODEL: &str = "LuminaRust No Such Camera 9000-XZ";
    let metadata = make_metadata(Some(PROBE_MAKE), Some(PROBE_MODEL), Some(18.0), Some(5.6));

    // A key only this test writes, so assertion 1 below has an anchor no sibling
    // can provide. It is reported once here and never asserted to be absent
    // afterwards, so it cannot collide with anything.
    const OWN_KEY_MARKER: &str = "/nonexistent/cli-prodseam-own-sink-key";
    let own_failure = SystemDbError::NotFound {
        misses: vec![ProbeMiss {
            dir: std::path::PathBuf::from(OWN_KEY_MARKER),
            source: Source::PlatformDefault,
            reason: MissReason::Absent,
        }],
    };
    crate::lensfun_cli::with_diagnostics(|sink| sink.failed(&own_failure));

    // **The production call site.** No sink is passed in and none is observed
    // here — the sink is the callee's own process-lifetime state, which is the
    // entire subject of this test.
    assert!(
        build_lensfun_corrector(Some(&metadata)).is_none(),
        "the probe identity {PROBE_MAKE} / {PROBE_MODEL} must not match any \
         profile: a match would mean the test no longer drives the production \
         load-then-lookup body"
    );

    // An **independent** observer: a separate `with_diagnostics` call, which
    // sees only the keys the process-lifetime sink has accumulated.
    let observed = crate::lensfun_cli::with_diagnostics(|sink| format!("{sink:?}"));

    assert!(
        observed.contains(OWN_KEY_MARKER),
        "the production call site must not swap or replace the process-lifetime \
         sink: a key recorded before `build_lensfun_corrector` ran is gone \
         afterwards, so a per-image caller gets a sink nobody else can see. \
         Sink state: {observed}"
    );

    const RESOLVED: &str = "resolved:";
    const OVERRIDE_UNUSABLE: &str = "OverrideUnusable";
    const REAL_PROBE_DIR: &str = "/usr/share/lensfun";
    let load_recorded = observed.contains(RESOLVED)
        || observed.contains(OVERRIDE_UNUSABLE)
        || (observed.contains("failed:") && observed.contains(REAL_PROBE_DIR));

    assert!(
        load_recorded,
        "the production call site must leave its database-load record in the \
         process-lifetime sink, readable from a later, separate \
         `with_diagnostics` call: expected one of {RESOLVED:?} (host with a \
         system database), {OVERRIDE_UNUSABLE:?} (LUMINA_LENSFUN_DB pinned to \
         an unusable directory) or a real `failed:` key naming \
         {REAL_PROBE_DIR:?} (host without one). A sink constructed per call is \
         empty at every read, so this is what turns red if \
         `build_lensfun_corrector` stops using the shared sink. \
         Sink state: {observed}"
    );
}

/// The lookup itself must still run per image — only the *reporting* is
/// de-duplicated. A miss is never cached, so a Lensfun database installed while
/// a long batch is running is picked up by the next image. Pinning the negative
/// here guards against someone "optimising" the sink into a cache of results.
#[test]
fn the_sink_deduplicates_reports_but_does_not_cache_the_lookup() {
    // Two calls, two *invocations* of the loader: a caching implementation
    // would collapse these into one lookup and stop noticing later DB changes.
    let calls = std::cell::Cell::new(0u32);
    for _ in 0..2 {
        crate::lensfun_cli::with_diagnostics(|_sink| calls.set(calls.get() + 1));
    }
    assert_eq!(
        calls.get(),
        2,
        "the closure must run once per call; wrapping the loader in the \
         process-lifetime sink must not turn into a per-process result cache"
    );
}
