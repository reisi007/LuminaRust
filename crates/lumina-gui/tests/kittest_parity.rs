//! KITTEST-PARITY-PATHS-1: CPU-draft ↔ GPU/VRAM path-parity matrix.
//!
//! The same scene (source + recipe) is rendered through **both** GUI present
//! paths and the two frames are compared against the project's F-043 tolerance
//! policy (the exact metrics the `lumina-gpu` oracle tests use, see
//! `crates/lumina-gpu/tests/golden.rs` and `crates/lumina-gpu/tests/parity.rs`
//! and `docs/gpu-bootstrap.md` §"Equivalence verification"):
//!
//! * `maxAbsDiff` per channel ≤ declared bound (0 where the paths are
//!   byte-identical — the neutral tone scene and the stacked Detail scene —
//!   and 1 for the tinted colour scene, whose Presence/HSL chain rounds
//!   differently),
//! * `PSNR` (global, MAX=255) ≥ 45 dB,
//! * `|mean signed error|` ≤ 0.05 (no systematic brightness/colour tilt — a
//!   bound the plain PSNR can hide).
//!
//! Tolerance alone is not enough: two paths can be *equally wrong*. Each path
//! therefore also gets **absolute geometry checks** on the painted frame — the
//! preview must fill its object-contain fit rect, and the overlay canvas the
//! painter maps overlays onto must coincide with that photo rect (overlays on
//! the photo, never beside it).
//!
//! The matrix is `scenes × paths`:
//! `neutral` (default recipe), `tinted` (WB tint + vibrance/saturation +
//! Presence clarity) and `detail` (sharpening + noise reduction — the GPU
//! stage-2 detail chain), each as `cpu` (interactive draft render) and `gpu`
//! (VRAM tone/detail pass, presented readback-free). Both frames are
//! snapshotted as goldens so the matrix is pixel-pinned and Vision-reviewable.
//!
//! # GPU availability
//!
//! The tests are `#[ignore]`d (headless GPU required). When the GUI binds no
//! usable adapter the matrix prints the explicit `SKIP` verdict and returns —
//! a missing adapter is never a silently green parity check. Run locally on a
//! Metal machine with:
//!
//! ```text
//! cargo test -p lumina-gui --test kittest_parity -- --ignored
//! ```
//!
//! Regenerating goldens (Metal baseline): `UPDATE_SNAPSHOTS=true cargo test -p
//! lumina-gui --test kittest_parity -- --ignored`.

#![cfg(feature = "gpu")]

use egui_kittest::Harness;
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{attach_wgpu_render_state, LuminaApp, Module};
use lumina_sidecar::Crop;
use std::collections::HashSet;

/// Printed when no usable adapter is bound so the run is visibly skipped,
/// never silently passed (same policy/wording as the `lumina-gpu` oracle
/// tests in `crates/lumina-gpu/tests/golden.rs`).
const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped parity check";

/// Structural PSNR floor from the F-043 policy (`docs/gpu-bootstrap.md`,
/// `lumina-gpu/tests/golden.rs`); `lumina-gpu/tests/parity.rs` uses a stricter
/// 48 dB for its bounded post-tone stages.
const MIN_PSNR_DB: f64 = 45.0;
/// Maximum absolute mean signed per-byte error (no systematic tilt).
const MAX_ABS_MEAN_SIGNED_ERROR: f64 = 0.05;

/// Synthetic source dimensions. Small enough for a fast headless render, large
/// enough that the Detail stages act on real texture and the 4:3 fit is exact.
const SRC_W: u32 = 160;
const SRC_H: u32 = 120;

/// One scene of the matrix: a named recipe applied through the public GUI
/// setters (the same code the panels use) plus its measured per-channel
/// `maxAbsDiff` bound.
///
/// The bound is only as tight as the F-043 measurement supports: scenes where
/// the CPU oracle and the VRAM path are byte-identical are pinned to `0` (a
/// single differing code value fails), while a scene with a measured non-zero
/// difference keeps its measured `1` — tightening it to `0` would be a false
/// promise, not a stricter check.
struct Scene {
    name: &'static str,
    apply: fn(&mut LuminaApp),
    max_abs_diff: u8,
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "neutral",
            apply: |_app| {},
            // PARITY-PATHS-2: measured byte-identical (maxAbsDiff=0) across
            // repeated Metal runs — the default recipe carries no tone stage
            // that rounds, so the bound is tightened from 1 to 0.
            max_abs_diff: 0,
        },
        Scene {
            name: "tinted",
            apply: |app| {
                app.set_adjustment("wb_tint", 0.3);
                app.set_adjustment("vibrance", 0.4);
                app.set_adjustment("saturation", 0.15);
                app.set_presence("clarity", 0.2);
            },
            // Measured maxAbsDiff=1: the Presence/Vibrance/Saturation chain has
            // a real CPU↔GPU rounding difference, so the F-043 bound stays 1.
            max_abs_diff: 1,
        },
        Scene {
            name: "detail",
            apply: |app| {
                app.set_noise_reduction_value("luminance", 0.35);
                app.set_sharpening_value("amount", 0.8);
                app.set_sharpening_value("radius", 0.8);
            },
            // PARITY-PATHS-2: the stage-2 detail chain is exact on both paths
            // (measured maxAbsDiff=0) — the bound is tightened from 2 to 0.
            max_abs_diff: 0,
        },
    ]
}

/// Headless harness with the app's GPU context re-based onto the test
/// renderer's wgpu device/queue — the same shared-device wiring the native
/// entry point uses, so the VRAM present path is genuinely exercised.
fn parity_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| {
            let mut app = LuminaApp::new(cc.egui_ctx.clone());
            attach_wgpu_render_state(&mut app, cc.wgpu_render_state.clone());
            app
        })
}

/// Deterministic 160×120 gradient with a fixed splitmix64 texture, encoded as
/// PNG. The gradient gives the tonal/colour stages a full range; the texture
/// gives the Detail stages real high-frequency content to act on.
fn scene_source_png() -> Vec<u8> {
    let mut state = 0x1234_5678_9ABC_DEF0_u64;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut pixels = Vec::with_capacity((SRC_W * SRC_H * 4) as usize);
    for y in 0..SRC_H {
        for x in 0..SRC_W {
            let rx = x as f32 / (SRC_W - 1) as f32;
            let ry = y as f32 / (SRC_H - 1) as f32;
            let noise = ((next() & 0x1F) as i32) - 16;
            let r = ((rx * 210.0) as i32 + noise).clamp(0, 255) as u8;
            let g = ((ry * 190.0) as i32 + noise).clamp(0, 255) as u8;
            let b = (((rx + ry) * 0.5 * 175.0) as i32 + noise).clamp(0, 255) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(SRC_W, SRC_H, pixels)
        .expect("scene source frame")
        .encode(ImageFileFormat::Png)
        .expect("scene source encodes")
}

// ---------------------------------------------------------------------------
// F-043 metrics (mirrors `lumina-gpu/tests/golden.rs` / `parity.rs`).
// ---------------------------------------------------------------------------

/// Per-channel maximum absolute difference (RGBA8).
fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len(), "parity inputs must have equal length");
    let mut max = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        max = max.max(x.abs_diff(*y));
    }
    max
}

/// Global PSNR over all RGBA8 bytes (MAX=255); `INFINITY` for identical bytes.
fn psnr_db(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "PSNR inputs must have equal length");
    let mut squared_error = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let e = (i64::from(*x) - i64::from(*y)) as f64;
        squared_error += e * e;
    }
    let mse = squared_error / a.len() as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0_f64 * 255.0 / mse).log10()
    }
}

/// Mean signed per-byte error (`(cpu - gpu)`): a systematic tilt of either sign
/// fails the `|bias| <= MAX_ABS_MEAN_SIGNED_ERROR` bound that PSNR alone can
/// hide (both signs would otherwise cancel).
fn mean_signed_error(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "bias inputs must have equal length");
    let sum: i64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| i64::from(*x) - i64::from(*y))
        .sum();
    sum as f64 / a.len() as f64
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// CPU-draft ↔ GPU/VRAM engine parity at the declared F-043 bound.
fn assert_path_parity(scene: &str, cpu: &ImageFrame, gpu: &ImageFrame, bound: u8) {
    assert_eq!(
        (cpu.width, cpu.height),
        (gpu.width, gpu.height),
        "parity frames must share geometry (scene {scene})"
    );
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
    let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
    eprintln!(
        "parity[{scene}]: maxAbsDiff={diff} psnr={psnr:.2} dB meanSignedErr={bias:+.4} \
         (bound maxAbsDiff <= {bound})"
    );
    assert!(
        diff <= bound,
        "scene {scene}: CPU↔GPU maxAbsDiff {diff} exceeds the declared bound {bound}"
    );
    assert!(
        psnr >= MIN_PSNR_DB,
        "scene {scene}: CPU↔GPU PSNR {psnr:.2} dB is below the {MIN_PSNR_DB} dB floor"
    );
    assert!(
        bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
        "scene {scene}: CPU↔GPU mean signed error {bias:+.4} exceeds the \
         {MAX_ABS_MEAN_SIGNED_ERROR} bias bound"
    );
}

/// Non-vacuous recipe guard: each scene must actually carry the stages its name
/// promises through the public setters (a setter that silently no-ops would
/// otherwise leave an all-neutral matrix looking green).
fn assert_scene_recipe(app: &LuminaApp, scene: &str) {
    let recipe = app.recipe();
    match scene {
        "neutral" => {
            assert!(
                recipe.adjustments.is_empty()
                    && recipe.presence.is_none()
                    && recipe.sharpening.is_none()
                    && recipe.noise_reduction.is_none(),
                "neutral scene must stay on the default recipe"
            );
        }
        "tinted" => {
            for key in ["wb_tint", "vibrance", "saturation"] {
                assert!(
                    recipe.adjustments.contains_key(key),
                    "tinted scene must carry adjustment {key:?}"
                );
            }
            assert!(
                recipe.presence.is_some(),
                "tinted scene must carry the Presence stage"
            );
        }
        "detail" => {
            assert!(
                recipe.sharpening.is_some() && recipe.noise_reduction.is_some(),
                "detail scene must carry the sharpening + noise-reduction stages"
            );
        }
        other => panic!("no recipe guard declared for scene {other:?}"),
    }
}

/// Absolute geometry checks that parity alone cannot catch (both paths could be
/// wrong the same way): the preview fills its fit rect, and the overlay canvas
/// the painter maps overlays onto is the photo rect. The crop-overlay mapping
/// is then resolved against that exact canvas.
fn assert_absolute_geometry(app: &LuminaApp, scene: &str) {
    let rect = app
        .preview_screen_rect()
        .unwrap_or_else(|| panic!("scene {scene}: preview was not painted"));
    let pane = app
        .preview_pane_rect()
        .unwrap_or_else(|| panic!("scene {scene}: preview pane was not laid out"));
    let overlay = app
        .overlay_full_rect()
        .unwrap_or_else(|| panic!("scene {scene}: overlay canvas was not laid out"));

    // 1. The painted preview preserves the source aspect ratio.
    let expected_aspect = SRC_W as f32 / SRC_H as f32;
    let aspect = rect.width() / rect.height().max(1e-6);
    assert!(
        (aspect - expected_aspect).abs() < 0.02,
        "scene {scene}: preview aspect {aspect:.4} must match the source {expected_aspect:.4} \
         (rect {rect:?})"
    );

    // 2. It fits inside the pane and fills the constraining axis (object-contain
    //    fit), i.e. it is as large as the pane allows.
    assert!(
        rect.width() <= pane.width() + 1.0 && rect.height() <= pane.height() + 1.0,
        "scene {scene}: preview {rect:?} must fit inside the pane {pane:?}"
    );
    let fills_x = (rect.width() - pane.width()).abs() < 1.0;
    let fills_y = (rect.height() - pane.height()).abs() < 1.0;
    assert!(
        fills_x || fills_y,
        "scene {scene}: preview {rect:?} must fill the constraining pane axis {pane:?}"
    );

    // 3. It is centred in the pane (Fit).
    let centre_delta = (rect.center() - pane.center()).length();
    assert!(
        centre_delta < 1.0,
        "scene {scene}: preview centre {:?} must coincide with the pane centre {:?}",
        rect.center(),
        pane.center()
    );

    // 4. The overlay canvas the painter maps overlays onto is the photo rect at
    //    Fit — overlays are anchored to the photo, not beside it.
    let canvas_min_delta = (overlay.min - rect.min).length();
    let canvas_max_delta = (overlay.max - rect.max).length();
    assert!(
        canvas_min_delta < 1.0 && canvas_max_delta < 1.0,
        "scene {scene}: overlay canvas {overlay:?} must coincide with the painted photo {rect:?}"
    );

    // 5. A free crop maps onto that canvas (absolute normalized mapping), never
    //    outside the photo.
    let crop = Crop::Free {
        x: 0.1,
        y: 0.1,
        width: 0.8,
        height: 0.7,
    };
    let crop_rect = LuminaApp::crop_overlay_rect(overlay, Some(&crop), SRC_W, SRC_H)
        .expect("free crop must map");
    assert!(
        overlay.contains(crop_rect.min) && overlay.contains(crop_rect.max),
        "scene {scene}: crop overlay {crop_rect:?} must lie inside the photo canvas {overlay:?}"
    );
    let expected_min =
        overlay.min + eframe::egui::vec2(0.1 * overlay.width(), 0.1 * overlay.height());
    let expected_max =
        overlay.min + eframe::egui::vec2(0.9 * overlay.width(), 0.8 * overlay.height());
    assert!(
        (crop_rect.min - expected_min).length() < 0.5
            && (crop_rect.max - expected_max).length() < 0.5,
        "scene {scene}: crop overlay {crop_rect:?} must map the normalized rect \
         ({expected_min:?}..{expected_max:?})"
    );
}

/// Non-vacuous pixel guard: the rendered frame must carry real image content
/// inside the painted preview rect (the gradient yields many distinct colours),
/// proving the photo actually fills the fit rect on this path.
fn assert_preview_pixels_present(harness: &mut Harness<'_, LuminaApp>, scene: &str) {
    let rect = harness
        .state()
        .preview_screen_rect()
        .expect("preview painted before pixel check");
    let rendered = harness.render().expect("kittest renders the frame");
    let (width, _height) = rendered.dimensions();
    let mut colours: HashSet<[u8; 3]> = HashSet::new();
    let min_x = rect.min.x.max(0.0).floor() as u32;
    let max_x = (rect.max.x.ceil() as u32).min(width);
    let min_y = rect.min.y.max(0.0).floor() as u32;
    let max_y = (rect.max.y.ceil() as u32).min(rendered.height());
    for y in min_y..max_y {
        for x in min_x..max_x {
            let pixel = rendered.get_pixel(x, y).0;
            colours.insert([pixel[0], pixel[1], pixel[2]]);
        }
    }
    assert!(
        colours.len() > 16,
        "scene {scene}: preview rect {rect:?} must carry real image content \
         (got {} distinct colours)",
        colours.len()
    );
}

/// Drive one scene through the CPU-draft and GPU/VRAM paths, snapshot both and
/// assert engine parity + absolute geometry.
fn run_scene(harness: &mut Harness<'_, LuminaApp>, scene: &Scene) {
    // Fresh scene state: load the deterministic source and apply the recipe
    // through the public setters, then settle a full render so no pending
    // slider commit/debounce is armed while we snapshot the draft path.
    {
        let app = harness.state_mut();
        app.set_module(Module::Develop);
        app.load_bytes(scene_source_png(), "parity_source.png")
            .expect("scene source loads");
        // Baseline draft with the default recipe: proves below that a
        // non-neutral scene really changes pixels (non-vacuous matrix cell).
        app.render_draft([1024, 720], None)
            .expect("baseline cpu draft render");
    }
    let baseline = harness
        .state()
        .preview()
        .expect("baseline cpu frame")
        .clone();
    {
        let app = harness.state_mut();
        app.load_bytes(scene_source_png(), "parity_source.png")
            .expect("scene source reloads");
        (scene.apply)(app);
        assert_scene_recipe(app, scene.name);
        app.render().expect("scene full render settles");
        app.render_draft([1024, 720], None)
            .expect("scene cpu draft render");
    }

    // ---- CPU path: draft render presented from the CPU texture ----
    harness.run();
    harness.run();
    assert!(
        harness.state().preview_is_draft(),
        "scene {}: CPU path must present the interactive draft",
        scene.name
    );
    assert!(
        harness.state().gpu_present_frame_size().is_none(),
        "scene {}: the CPU cell must not present from VRAM (silent GPU leakage)",
        scene.name
    );
    let cpu_frame = harness
        .state()
        .preview()
        .expect("cpu draft preview frame")
        .clone();
    if scene.name != "neutral" {
        assert!(
            max_abs_diff(&baseline.pixels, &cpu_frame.pixels) > 0,
            "scene {}: the recipe must actually change pixels vs. the default \
             recipe (non-vacuous matrix cell)",
            scene.name
        );
    } else {
        assert_eq!(
            baseline.pixels, cpu_frame.pixels,
            "neutral scene must be deterministic across two draft renders"
        );
    }
    assert_absolute_geometry(harness.state(), scene.name);
    assert_preview_pixels_present(harness, scene.name);
    harness.snapshot(format!("parity_paths_{}_cpu", scene.name));

    // ---- GPU path: VRAM tone/detail pass presented readback-free ----
    assert!(
        harness.state_mut().prime_gpu_present(),
        "scene {}: an available adapter must render the VRAM result",
        scene.name
    );
    harness.run();
    harness.run();
    assert!(
        harness.state().gpu_present_frame_size().is_some(),
        "scene {}: the GPU cell must present from VRAM, not silently fall back to the CPU",
        scene.name
    );
    let gpu_frame = harness
        .state_mut()
        .render_gpu_readback_frame()
        .expect("gpu readback succeeds")
        .expect("gpu frame present with an adapter");
    assert_absolute_geometry(harness.state(), scene.name);
    assert_preview_pixels_present(harness, scene.name);
    harness.snapshot(format!("parity_paths_{}_gpu", scene.name));

    // ---- CPU↔GPU parity at the scene's F-043 bound ----
    assert_path_parity(scene.name, &cpu_frame, &gpu_frame, scene.max_abs_diff);
}

// ---------------------------------------------------------------------------
// Tests
//
// The pixel/geometry matrix needs a headless GPU, so the scene runners are
// `#[ignore]`d and run with `-- --ignored`. The adapterless SKIP contract is
// additionally covered by an adapter-independent test that runs in the normal
// suite (`adapter_probe_without_gpu_context_reports_unavailable`).
// ---------------------------------------------------------------------------

/// PARITY-PATHS-2: outcome of one matrix run, so the adapterless SKIP branch is
/// directly assertable instead of only observable on stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixOutcome {
    /// An adapter was available and the scenes were actually executed.
    Ran,
    /// No adapter: the matrix printed `SKIP_MESSAGE` and ran no scene.
    Skipped(&'static str),
}

/// Run the full `scenes × paths` matrix, returning the loud SKIP verdict when
/// no usable adapter is available (never a silently green run).
fn run_cpu_gpu_path_parity_matrix(harness: &mut Harness<'_, LuminaApp>) -> MatrixOutcome {
    if !harness.state_mut().gpu_adapter_available() {
        eprintln!("{SKIP_MESSAGE} (matrix not executed: no adapter to compare against)");
        return MatrixOutcome::Skipped(SKIP_MESSAGE);
    }
    for scene in scenes() {
        run_scene(harness, &scene);
    }
    MatrixOutcome::Ran
}

/// Full `scenes × paths` parity matrix. Without a usable adapter the run prints
/// the explicit SKIP verdict and returns (never a silently green parity check).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
fn cpu_gpu_path_parity_matrix() {
    let mut harness = parity_harness();
    let _ = run_cpu_gpu_path_parity_matrix(&mut harness);
}

/// PARITY-PATHS-2: exercise the adapterless SKIP branch for real on this
/// adapter-equipped machine through the diagnostic override. The matrix runner
/// must return the documented loud SKIP verdict — with the same text it prints
/// to stderr — and must not execute a single scene (no load, no render), so a
/// missing adapter can never pass as a green matrix.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
fn parity_matrix_skips_loudly_without_adapter() {
    let mut harness = parity_harness();
    let real_available = harness.state().gpu_adapter_available();

    harness.state_mut().set_gpu_adapter_override(Some(false));
    assert!(
        !harness.state().gpu_adapter_available(),
        "the adapter override must win over the real Metal probe"
    );
    let generation_before = harness.state().preview_generation();
    let outcome = run_cpu_gpu_path_parity_matrix(&mut harness);
    assert_eq!(
        outcome,
        MatrixOutcome::Skipped(SKIP_MESSAGE),
        "a missing adapter must yield the loud SKIP verdict, never a green run"
    );
    assert_eq!(
        harness.state().preview_generation(),
        generation_before,
        "the SKIP branch must not execute any scene render"
    );

    harness.state_mut().set_gpu_adapter_override(None);
    assert_eq!(
        harness.state().gpu_adapter_available(),
        real_available,
        "clearing the override must restore the real adapter probe"
    );
}

/// PARITY-PATHS-2 adapter-independent half of the SKIP contract, runnable in
/// the normal (non-`--ignored`) suite without a GPU: an app that never attached
/// a wgpu render state binds no adapter, and the override is honoured. This is
/// the exact predicate the matrix SKIP branch keys on, so GPU-less CI still
/// covers the loud-skip policy.
#[test]
fn adapter_probe_without_gpu_context_reports_unavailable() {
    let mut app = LuminaApp::new(eframe::egui::Context::default());
    assert!(
        !app.gpu_adapter_available(),
        "a fresh app without an attached wgpu context must report no adapter"
    );
    app.set_gpu_adapter_override(Some(true));
    assert!(
        app.gpu_adapter_available(),
        "the override must be able to simulate a present adapter"
    );
    app.set_gpu_adapter_override(Some(false));
    assert!(!app.gpu_adapter_available());
    app.set_gpu_adapter_override(None);
    assert!(
        !app.gpu_adapter_available(),
        "clearing the override must restore the real (absent) adapter probe"
    );
}

/// The geometry half of the framework is assertable through the public API
/// without touching rendered-frame pixels; this is the adapter-independent
/// anchor documented in the task (Preview fills Fit-Rect, overlay canvas on
/// the photo). It stays part of the same file/matrix so the absolute checks
/// cannot silently rot when the matrix grows.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
fn path_parity_geometry_cpu_draft() {
    let mut harness = parity_harness();
    if !harness.state_mut().gpu_adapter_available() {
        eprintln!("{SKIP_MESSAGE} (geometry matrix not executed: no adapter present)");
        return;
    }
    for scene in scenes() {
        {
            let app = harness.state_mut();
            app.set_module(Module::Develop);
            app.load_bytes(scene_source_png(), "parity_source.png")
                .expect("scene source loads");
            (scene.apply)(app);
            assert_scene_recipe(app, scene.name);
            app.render().expect("scene full render settles");
            app.render_draft([1024, 720], None)
                .expect("scene cpu draft render");
        }
        harness.run();
        harness.run();
        assert_absolute_geometry(harness.state(), scene.name);
        assert_preview_pixels_present(&mut harness, scene.name);
    }
}

// ---------------------------------------------------------------------------
// PARITY-PATHS-2 cell: active Lensfun corrector → CPU route.
// ---------------------------------------------------------------------------

/// Minimal version_1 Lensfun fixture database (same shape as the GUI's
/// `LENSFUN_GATE_FIXTURE_XML` unit test and the `lumina-core` row tests): one
/// camera + one lens with **vignetting-only** calibration. Embedded so the cell
/// builds a genuinely non-identity corrector deterministically, without
/// depending on the system profile DB. Vignetting-only (no distortion) is
/// deliberate: the corrector is still `!is_identity()` — so the GPU present
/// gate refuses it — but it does not create transparent wedges, hence no
/// CROP-MAXRECT default content crop, so the geometry/overlay checks apply
/// unchanged to the CPU route.
#[cfg(feature = "lensfun")]
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
#[cfg(feature = "lensfun")]
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

/// PARITY-PATHS-2: a genuinely active Lensfun corrector (vignetting profile
/// here) is applied by the CPU reference (EXIF auto-match / TCA in the real
/// app), while the GUI-LENSFUN-GATE-1 present gate refuses the VRAM route — the
/// preview stays the exact corrected CPU frame and never shows the uncorrected
/// tone result. The absolute geometry/overlay checks still hold on that CPU
/// route (a corrector alone must not change the photo rect / overlay canvas),
/// and the corrector provably changes pixels against the same scene without it
/// (non-vacuous cell).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
#[cfg(feature = "lensfun")]
fn lensfun_corrector_cell_presents_cpu_and_refuses_vram() {
    let mut harness = parity_harness();
    if !harness.state_mut().gpu_adapter_available() {
        eprintln!("{SKIP_MESSAGE} (lensfun corrector cell not executed: no adapter to refuse)");
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
    // difference on the exact CPU route.
    assert!(
        max_abs_diff(&without_corrector.pixels, &cpu_frame.pixels) > 0,
        "the active Lensfun corrector must actually change CPU pixels"
    );
    assert_eq!(
        (cpu_frame.width, cpu_frame.height),
        (SRC_W, SRC_H),
        "a vignetting-only corrector must not change the frame geometry"
    );

    // The visible preview is the CPU draft; the VRAM tone result must be
    // refused (the corrector is a caller-owned stage the recipe-only GPU gate
    // cannot express) even after a fresh VRAM render was primed.
    assert!(
        harness.state().preview_is_draft(),
        "the corrected cell must present the interactive CPU draft"
    );
    assert!(
        harness.state_mut().prime_gpu_present(),
        "an available adapter must still render the VRAM tone result"
    );
    harness.run();
    harness.run();
    assert!(
        harness.state().gpu_present_frame_size().is_none(),
        "GUI-LENSFUN-GATE-1: the active corrector must force the CPU present route \
         (VRAM must not be presented)"
    );
    // GUI-LENSFUN-GATE-2: the silent-looking CPU route must carry the visible
    // badge naming the precise reason — here the caller-owned Lensfun corrector.
    let badge = harness
        .state()
        .gpu_routing_fallback_badge()
        .expect("GUI-LENSFUN-GATE-2: the CPU route must carry a visible badge");
    assert!(
        badge.contains("Lensfun corrector"),
        "the badge must name the precise Lensfun reason, got {badge:?}"
    );
    assert_eq!(
        harness
            .state()
            .preview()
            .expect("corrected cpu frame")
            .pixels,
        cpu_frame.pixels,
        "the presented preview must stay the corrected CPU frame"
    );

    assert_absolute_geometry(harness.state(), "lensfun_corrector");
    assert_preview_pixels_present(&mut harness, "lensfun_corrector");
    harness.snapshot("parity_paths_lensfun_corrector_cpu");
}
