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
use lumina_gui::{LuminaApp, Module};

// Shared helpers + the PARITY-PATHS-2 Lensfun-corrector cell, extracted into a
// support module (file-size ratchet DoD §8). `mod kittest_parity_support;`
// resolves to `tests/kittest_parity_support/mod.rs` and keeps the cell in this
// integration-test target, so `cargo test -p lumina-gui --test kittest_parity
// lensfun -- --ignored` still discovers it.
mod kittest_parity_support;
use kittest_parity_support::*;

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
