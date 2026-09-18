//! PARITY-PATHS-2 cell: active Lensfun corrector → GPU route
//! (GPU-LENSFUN-PARITY-1). Extracted verbatim from `kittest_parity.rs`
//! (file-size ratchet DoD §8); included as a submodule of
//! [`crate::kittest_parity_support`] so the `kittest_parity` test target still
//! discovers it (`cargo test -p lumina-gui --test kittest_parity lensfun --
//! --ignored`).

use super::{
    assert_absolute_geometry, assert_path_parity, assert_preview_pixels_present, max_abs_diff,
    mean_signed_error, parity_harness, psnr_db, scene_source_png, SKIP_MESSAGE, SRC_H, SRC_W,
};
use lumina_gui::Module;

/// Minimal version_1 Lensfun fixture database (same shape as the GUI's
/// `LENSFUN_GATE_FIXTURE_XML` unit test and the `lumina-core` row tests): one
/// camera + one lens with **vignetting-only** calibration. Embedded so the cell
/// builds a genuinely non-identity corrector deterministically, without
/// depending on the system profile DB. Vignetting-only (no distortion) is
/// deliberate: the corrector is still `!is_identity()` — so it exercises the
/// `LensfunMap` bind on the GPU present path — but it does not create
/// transparent wedges, hence no CROP-MAXRECT default content crop, so the
/// geometry/overlay checks apply unchanged to the presented frame.
const LENSFUN_FIXTURE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Test Body</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Vignetting 50mm f/2.8</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
        </calibration>
    </lens>
</lensdatabase>
"#;

/// Build the fixture corrector for the exact `width × height` the render uses
/// (the GUI's own `ensure_lensfun_cache` builds at the base-frame dimensions,
/// so a mismatched fixture would not model the real caller contract).
fn synthetic_lensfun_corrector(
    directory: &std::path::Path,
    width: u32,
    height: u32,
) -> (lumina_lensfun::Corrector, lumina_lensfun::LensfunDb) {
    let path = directory.join("lensfun-parity-fixture.xml");
    std::fs::write(&path, LENSFUN_FIXTURE_XML).expect("write fixture database");
    let db = lumina_lensfun::LensfunDb::load_file(&path).expect("fixture database must load");
    let corrector = lumina_lensfun::Corrector::for_camera(
        &db,
        "Lumina Test Corp",
        "Lumina Test Body",
        None,
        width,
        height,
        50.0,
        2.8,
        10.0,
    )
    .expect("fixture profile must yield a corrector");
    (corrector, db)
}

/// PARITY-PATHS-2 (GPU-LENSFUN-PARITY-1): a genuinely active Lensfun corrector
/// (vignetting profile here) is applied by the CPU reference (EXIF auto-match /
/// TCA in the real app) **and** presents through the VRAM path — the GUI binds
/// the corrector's `LensfunMap` before `render_to_vram`, so the corrector is no
/// longer a documented CPU exception. The absolute geometry/overlay checks hold
/// on the presented frame, and the corrector provably changes pixels against the
/// same scene without it (non-vacuous cell). A silent GPU→CPU fallback would
/// leave `gpu_present_frame_size()` `None` (the matrix's present assertion).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
fn lensfun_corrector_cell_presents_gpu_without_badge() {
    let mut harness = parity_harness();
    if !harness.state_mut().gpu_adapter_available() {
        eprintln!("{SKIP_MESSAGE} (lensfun corrector cell not executed: no adapter to present)");
        return;
    }

    // Baseline: the same source/scene without a corrector (default recipe).
    {
        let app = harness.state_mut();
        app.set_module(Module::Develop);
        app.load_bytes(scene_source_png(), "parity_source.png")
            .expect("scene source loads");
        app.render_draft([1024, 720], None)
            .expect("corrector-free cpu draft render");
    }
    let without_corrector = harness
        .state()
        .preview()
        .expect("corrector-free cpu frame")
        .clone();

    // Bind a non-identity corrector for the exact base-frame geometry and
    // re-render the draft: the CPU reference now applies the vignetting
    // correction.
    let directory = tempfile::tempdir().expect("tempdir");
    let (corrector, db) = synthetic_lensfun_corrector(directory.path(), SRC_W, SRC_H);
    assert!(
        !corrector.is_identity(),
        "fixture profile must be a real (non-identity) correction"
    );
    {
        let app = harness.state_mut();
        app.bind_test_lensfun_corrector(corrector, db);
        app.render_draft([1024, 720], None)
            .expect("cpu draft render with corrector");
    }
    harness.run();
    harness.run();

    let cpu_frame = harness
        .state()
        .preview()
        .expect("corrected cpu frame")
        .clone();
    // Non-vacuous cell: a corrector that silently no-opped would leave the
    // frame byte-identical to the corrector-free baseline. The vignetting-only
    // profile keeps the frame geometry unchanged, so this is a pure pixel
    // difference on the CPU reference.
    assert!(
        max_abs_diff(&without_corrector.pixels, &cpu_frame.pixels) > 0,
        "the active Lensfun corrector must actually change CPU pixels"
    );
    assert_eq!(
        (cpu_frame.width, cpu_frame.height),
        (SRC_W, SRC_H),
        "a vignetting-only corrector must not change the frame geometry"
    );

    // GPU-LENSFUN-PARITY-1: the VRAM tone result now carries the corrector's
    // map and must be presented (no badge). The recipe-gate half is pinned by
    // `active_lensfun_corrector_is_not_a_recipe_gate_reason`; here the whole
    // present path runs with a real adapter.
    assert!(
        harness.state_mut().prime_gpu_present(),
        "an available adapter must render the corrector's VRAM result"
    );
    harness.run();
    harness.run();
    assert!(
        harness.state().gpu_present_frame_size().is_some(),
        "GPU-LENSFUN-PARITY-1: the bound map must present from VRAM \
         (no silent CPU fallback)"
    );
    assert!(
        harness.state().gpu_routing_fallback_badge().is_none(),
        "GPU-LENSFUN-PARITY-1: a bound Lensfun map must not carry a CPU-routing badge, got {:?}",
        harness.state().gpu_routing_fallback_badge()
    );
    assert_eq!(
        harness
            .state()
            .preview()
            .expect("corrected cpu frame")
            .pixels,
        cpu_frame.pixels,
        "the CPU reference preview must stay the corrected frame"
    );

    assert_absolute_geometry(harness.state(), "lensfun_corrector");
    assert_preview_pixels_present(&mut harness, "lensfun_corrector");
    harness.snapshot("parity_paths_lensfun_corrector_cpu");

    // GPU-side frame for the F-043 comparison. Measured byte-identical to the
    // CPU reference (`maxAbsDiff=0`/PSNR=inf) on this vignetting-only profile;
    // a *recipe-only* GPU would drop the corrector entirely, so the CPU-draft
    // snapshot above is the non-vacuous anchor (`max_abs_diff > 0` against the
    // corrector-free baseline).
    let gpu_frame = harness
        .state_mut()
        .render_gpu_readback_frame()
        .expect("gpu readback with a bound Lensfun map")
        .expect("gpu frame present with an adapter");
    assert_absolute_geometry(harness.state(), "lensfun_corrector");
    assert_preview_pixels_present(&mut harness, "lensfun_corrector");
    harness.snapshot("parity_paths_lensfun_corrector_gpu");

    let diff = max_abs_diff(&cpu_frame.pixels, &gpu_frame.pixels);
    let psnr = psnr_db(&cpu_frame.pixels, &gpu_frame.pixels);
    let bias = mean_signed_error(&cpu_frame.pixels, &gpu_frame.pixels);
    eprintln!(
        "parity[lensfun_corrector]: maxAbsDiff={diff} psnr={psnr:.2} dB meanSignedErr={bias:+.4} \
         (bound maxAbsDiff <= 0)"
    );
    assert_path_parity("lensfun_corrector", &cpu_frame, &gpu_frame, 0);
}
