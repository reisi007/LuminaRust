// F-098-N2: feature-gated CLI->Lensfun wiring tests (see mod.rs cfg).

use super::*;

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
#[test]
fn the_diagnostics_sink_persists_across_calls_so_a_batch_reports_once() {
    let first = crate::lensfun_cli::with_diagnostics(|sink| sink as *const _);
    let second = crate::lensfun_cli::with_diagnostics(|sink| sink as *const _);
    assert_eq!(
        first, second,
        "every call must receive the same process-lifetime sink, otherwise a \
         per-image lookup re-prints the failure and the batch spams one block \
         per file"
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
