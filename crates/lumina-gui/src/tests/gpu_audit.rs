//! GUI-GPU-AUDIT-17 (Release 1.0, User-Vorgabe 2026-09-17 / F-103-N6):
//! automated headless routing audit over **every** instrumented GUI action.
//!
//! For each `GuiAction` in [`ALL_GUI_ACTIONS`] the audit
//!
//! 1. loads a deterministic synthetic source into a real GPU-bound
//!    [`LuminaApp`] (standalone Metal context via
//!    [`attach_wgpu_render_state`]; on a machine without a usable adapter the
//!    audit prints the explicit SKIP verdict and returns — never a silently
//!    green run, the same policy as `kittest_parity`),
//! 2. drives the action through its real handler (`drive_action`, exhaustive
//!    match — a new `GuiAction` cannot compile without an arm),
//! 3. runs one coalesced draft tick (`render_draft_tick`: the real hot path
//!    that fills `vram_fresh` / `vram_render_refusal`), and
//! 4. recomputes the present route (`update_texture` →
//!    [`LuminaApp::gpu_routing_fallback_badge`]).
//!
//! Assertions per action:
//!
//! * no `gpu_routing_fallback_badge()` (the VRAM present route is eligible /
//!   the action only touched editorial or session state), **or**
//! * for the documented CPU-route exceptions ([`documented_cpu_exception`],
//!   mirrored in `feature/platform/cli-gui-wasm.md` § GUI-GPU-Audit and the
//!   GPU inventory in `feature/architecture/pipeline.md`) the badge is present
//!   and names the documented reason.
//!
//! The audit never gates on duration: every action's handler is timed and the
//! table is printed report-only (calibration is a follow-up). Thresholds stay
//! report-only by design — a slow action must surface in the table, not break
//! the build.
//!
//! The Metal half is `#[ignore]`d (`cargo test -p lumina-gui --lib gpu_audit --
//! --ignored`), consistent with `kittest_snapshots` / `kittest_parity`: CI has
//! no Metal adapter (documented CI gap in
//! `feature/platform/capability-matrix.md`). The adapter-independent half
//! (`gpu_audit_exception_table_is_complete_without_gpu`) runs in the normal
//! `cargo test -p lumina-gui` suite and pins the exception table to the
//! documented reason inventory so the audit cannot rot silently.

#![cfg(feature = "gpu")]

use super::*;
use std::time::Instant;

use super::gpu_audit_actions::{drive_action, AUDIT_SRC_H, AUDIT_SRC_W};

/// Printed when no usable adapter is bound (same policy/wording family as the
/// `kittest_parity` / `lumina-gpu` oracle SKIP verdicts).
const GPU_AUDIT_SKIP: &str = "GPU adapter unavailable - skipped GUI GPU audit";

// Documented CPU-route reason classes (mirrors the live inventory
// `cpu_routing_inventory_is_complete` in `crates/lumina-gpu/tests/parity.rs`).
// Kept as string constants so the exception table and the adapter-independent
// completeness test share one source of truth. GPU-LENSFUN-PARITY-1 removed the
// former GUI-only Lensfun exception: a strictly matched corrector is bound as a
// `LensfunMap` and presents through VRAM.
const REASON_DEFAULT_CONTENT_CROP: &str = "geometry (default content crop)";
const REASON_DIMENSION_CHANGING: &str = "geometry (dimension-changing output";
const REASON_DENOISE: &str = "denoise_ai (not GPU-wired)";
const REASON_GENERATIVE: &str = "generative_edit";

/// Every documented CPU-route reason the exception table may use.
const DOCUMENTED_CPU_REASONS: &[&str] = &[
    REASON_DEFAULT_CONTENT_CROP,
    REASON_DIMENSION_CHANGING,
    REASON_DENOISE,
    REASON_GENERATIVE,
];

/// The documented CPU-route exception for `action`, or `None` when the action
/// must present through the VRAM route with no badge. Exhaustive by intent:
/// every `Some` cites the reason class from the GPU inventory.
///
/// * `geometry (default content crop)` — `SetLensProfile` / `AnalyzeUpright` /
///   `SetUprightEnabled` activate a lens/perspective correction **without** an
///   explicit `geometry.crop`, so the CPU oracle's data-dependent
///   maximum-content rectangle applies (CROP-MAXRECT-1 / GPU-MAXRECT-WELLE,
///   documented in `feature/architecture/pipeline.md` § GPU-Pfad).
/// * `geometry (dimension-changing output)` — `SetCropAspect` / `RotateStep`
///   produce an output whose dimensions differ from the source; the
///   readback-free VRAM present texture is source-sized, so the GUI refuses it
///   loudly and presents the exact CPU frame (GUI-LENSFUN-GATE-3 F1,
///   pipeline.md § GPU-Pfad; the recipe itself stays GPU-renderable for
///   export/readback).
/// * `denoise_ai (not GPU-wired)` — `SetDenoiseEnabled(true)` activates the
///   additive KI-Denoise stage (Release 2.0), which has no WGSL pass yet
///   (LRPAR-G14-DENOISE-IMPL-20).
/// * `generative_edit` — the four generative canvas controls activate a
///   generative role; the GUI deliberately presents previews through the
///   artifact-aware CPU path because the readback-free VRAM present is
///   artifact-blind (GEN-ONNX-1 Welle 2b, documented in
///   `feature/platform/cli-gui-wasm.md`).
///
/// The caller-owned **Lensfun corrector** is no longer an exception
/// (GPU-LENSFUN-PARITY-1): it is exercised by
/// `gpu_audit_lensfun_corrector_presents_gpu_without_badge`, which asserts the
/// GPU present route and the absence of a badge.
fn documented_cpu_exception(action: GuiAction) -> Option<&'static str> {
    match action {
        GuiAction::SetLensProfile | GuiAction::AnalyzeUpright | GuiAction::SetUprightEnabled => {
            Some(REASON_DEFAULT_CONTENT_CROP)
        }
        GuiAction::SetCropAspect | GuiAction::RotateStep => Some(REASON_DIMENSION_CHANGING),
        GuiAction::SetDenoiseEnabled => Some(REASON_DENOISE),
        GuiAction::SetExpandCanvas
        | GuiAction::GenerateCanvas
        | GuiAction::SetExpandBeyondImage
        | GuiAction::SetAutoFillTransparent => Some(REASON_GENERATIVE),
        _ => None,
    }
}

/// Deterministic gradient source with a fixed high-frequency texture, encoded
/// as PNG (same generator shape as `kittest_parity`'s scene source). PNG
/// carries no EXIF, so no Lensfun auto-profile can be fabricated for it.
fn audit_source_png() -> Vec<u8> {
    let mut state = 0x1234_5678_9ABC_DEF0_u64;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut pixels = Vec::with_capacity((AUDIT_SRC_W * AUDIT_SRC_H * 4) as usize);
    for y in 0..AUDIT_SRC_H {
        for x in 0..AUDIT_SRC_W {
            let rx = x as f32 / (AUDIT_SRC_W - 1) as f32;
            let ry = y as f32 / (AUDIT_SRC_H - 1) as f32;
            let noise = ((next() & 0x1F) as i32) - 16;
            let r = ((rx * 210.0) as i32 + noise).clamp(0, 255) as u8;
            let g = ((ry * 190.0) as i32 + noise).clamp(0, 255) as u8;
            let b = (((rx + ry) * 0.5 * 175.0) as i32 + noise).clamp(0, 255) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(AUDIT_SRC_W, AUDIT_SRC_H, pixels)
        .expect("audit source frame")
        .encode(ImageFileFormat::Png)
        .expect("audit source encodes")
}

/// Adapter-independent completeness pin: the exception table uses only
/// documented reason classes, and every exception action exists in
/// [`ALL_GUI_ACTIONS`]. Runs in the normal suite (`cargo test -p lumina-gui`)
/// without a GPU, so the audit cannot rot into a stale table on CI.
#[test]
fn gpu_audit_exception_table_is_complete_without_gpu() {
    assert_eq!(
        ALL_GUI_ACTIONS.len(),
        102,
        "the F-100 action surface grew/shrank: update the audit (and its docs)"
    );
    let mut exceptions = 0usize;
    for action in ALL_GUI_ACTIONS {
        if let Some(reason) = documented_cpu_exception(*action) {
            assert!(
                DOCUMENTED_CPU_REASONS.contains(&reason),
                "{action:?}: reason {reason:?} is not a documented CPU-route class"
            );
            exceptions += 1;
        }
    }
    assert_eq!(
        exceptions, 10,
        "the documented CPU-exception action list changed: update the audit and \
         the exception table in feature/platform/cli-gui-wasm.md"
    );
    // Spot-pin the documented exceptions and a few must-present actions so a
    // silent classification flip fails the adapter-independent suite too.
    assert_eq!(
        documented_cpu_exception(GuiAction::SetLensProfile),
        Some(REASON_DEFAULT_CONTENT_CROP)
    );
    assert_eq!(
        documented_cpu_exception(GuiAction::AnalyzeUpright),
        Some(REASON_DEFAULT_CONTENT_CROP)
    );
    assert_eq!(
        documented_cpu_exception(GuiAction::SetUprightEnabled),
        Some(REASON_DEFAULT_CONTENT_CROP)
    );
    assert_eq!(
        documented_cpu_exception(GuiAction::SetCropAspect),
        Some(REASON_DIMENSION_CHANGING)
    );
    assert_eq!(
        documented_cpu_exception(GuiAction::RotateStep),
        Some(REASON_DIMENSION_CHANGING)
    );
    assert_eq!(
        documented_cpu_exception(GuiAction::SetDenoiseEnabled),
        Some(REASON_DENOISE)
    );
    for action in [
        GuiAction::GenerateCanvas,
        GuiAction::SetExpandCanvas,
        GuiAction::SetExpandBeyondImage,
        GuiAction::SetAutoFillTransparent,
    ] {
        assert_eq!(
            documented_cpu_exception(action),
            Some(REASON_GENERATIVE),
            "{action:?} must be a documented generative CPU route"
        );
    }
    for action in [
        GuiAction::ToggleBeforeAfter,
        GuiAction::SetGeometryMirror,
        GuiAction::AddCurvePoint,
        GuiAction::AutoTone,
        GuiAction::ClearLensProfile,
        GuiAction::SetLensBlurEnabled,
    ] {
        assert_eq!(
            documented_cpu_exception(action),
            None,
            "{action:?} must present through the VRAM route without a badge"
        );
    }
}

/// The Metal audit. With a real GPU context (standalone wgpu adapter, local
/// Metal) every action is driven and classified; without an adapter the run
/// prints the loud SKIP verdict and returns.
///
/// ```text
/// cargo test -p lumina-gui --lib gpu_audit -- --ignored
/// ```
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --lib gpu_audit -- --ignored"]
fn gpu_action_routing_audit_metal() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    // `None` builds the standalone GPU context at the single construction
    // point (headless harness path), i.e. a real Metal adapter/device.
    attach_wgpu_render_state(&mut app, None);
    if !app.gpu_adapter_available() {
        eprintln!("{GPU_AUDIT_SKIP} (no usable adapter; routing not asserted)");
        return;
    }

    let directory = tempfile::tempdir().expect("audit tempdir");
    let presets = directory.path().join("presets");
    std::fs::create_dir_all(&presets).expect("audit presets dir");
    // LRPAR-G09-SORT-09: point the app at the tempdir so `set_library_sort`
    // persists its folder file there instead of the process CWD.
    app.directory = directory.path().display().to_string();
    let source = audit_source_png();

    // (action name, handler microseconds, documented route)
    let mut rows: Vec<(&'static str, u128, Option<&'static str>)> =
        Vec::with_capacity(ALL_GUI_ACTIONS.len());

    for (index, action) in ALL_GUI_ACTIONS.iter().copied().enumerate() {
        let source_path = directory.path().join(format!("gpu_audit_{index}.png"));
        std::fs::write(&source_path, &source).expect("write audit source");
        let name = source_path
            .file_name()
            .expect("source file name")
            .to_string_lossy()
            .into_owned();
        app.load_bytes(source.clone(), name)
            .expect("audit source loads");
        // Sidecar-mutating actions need a local file path (same precondition
        // as the real GUI). The standalone context has no eframe render state,
        // so the present step itself stays on the CPU upload — the *routing
        // verdict* is identical (see `gpu_routing_fallback_badge`).
        app.path = source_path.display().to_string();
        app.presets_dir = Some(presets.clone());

        let start = Instant::now();
        drive_action(
            &mut app,
            action,
            &directory
                .path()
                .join(format!("gpu_audit_export_{index}.png")),
        );
        let elapsed = start.elapsed();

        // Real hot path: fill `vram_fresh` / `vram_render_refusal`, then let
        // the present gate recompute the badge for this frame.
        app.render_draft_tick([AUDIT_SRC_W, AUDIT_SRC_H]);
        app.update_texture(&ctx);
        let badge = app.gpu_routing_fallback_badge().map(str::to_owned);

        match (documented_cpu_exception(action), badge.as_deref()) {
            (None, None) => {}
            (None, Some(reason)) => panic!(
                "{action:?}: undocumented CPU route (badge {reason:?}) — \
                 classify it against the documented GPU inventory"
            ),
            (Some(want), Some(got)) => assert!(
                got.contains(want),
                "{action:?}: documented CPU route must name {want:?}, got {got:?}"
            ),
            (Some(want), None) => {
                panic!("{action:?}: expected the documented CPU route {want:?}, got no badge")
            }
        }
        rows.push((
            action.name(),
            elapsed.as_micros(),
            documented_cpu_exception(action),
        ));
    }

    // Report-only timing table (no threshold gate until calibration).
    eprintln!("GUI-GPU-AUDIT-17 timing table (report-only, handler wall time):");
    eprintln!("{:<34} {:>10}  route", "action", "us");
    for (name, micros, route) in &rows {
        let route = route.unwrap_or("present (no fallback badge)");
        eprintln!("{name:<34} {micros:>10}  {route}");
    }
    let total: u128 = rows.iter().map(|(_, micros, _)| *micros).sum();
    let exceptions = rows.iter().filter(|(_, _, route)| route.is_some()).count();
    eprintln!(
        "GUI-GPU-AUDIT-17: {} actions, {} documented CPU exceptions, {} us total handler time",
        rows.len(),
        exceptions,
        total
    );
}

/// GPU-LENSFUN-PARITY-1: a strictly matched Lensfun corrector is bound as a
/// `LensfunMap` and presents through the VRAM path — the badge is absent. This
/// is the headless beleg for the former GUI-GPU-AUDIT-17 CPU exception turning
/// into a GPU route. Ignored with the audit; same fixture database shape as the
/// `kittest_parity` Lensfun cell.
///
/// Negative argumentation in the same test: a **distortion** corrector without
/// an explicit crop would need the CPU content-based default crop, which the
/// `lumina-gpu` map guard refuses. That route must stay loud (badge present,
/// `vram_fresh == false`), so the removal of the blanket corrector exception
/// cannot silently present a divergent frame.
#[cfg(feature = "lensfun")]
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --lib gpu_audit -- --ignored"]
fn gpu_audit_lensfun_corrector_presents_gpu_without_badge() {
    const FIXTURE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Distortion 50mm f/2.8</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
        </calibration>
    </lens>
</lensdatabase>
"#;

    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    attach_wgpu_render_state(&mut app, None);
    if !app.gpu_adapter_available() {
        eprintln!("{GPU_AUDIT_SKIP} (no usable adapter; Lensfun GPU route not asserted)");
        return;
    }

    let directory = tempfile::tempdir().expect("lensfun tempdir");
    let source_path = directory.path().join("gpu_audit_lensfun.png");
    std::fs::write(&source_path, audit_source_png()).expect("write source");
    app.load_bytes(audit_source_png(), "gpu_audit_lensfun.png")
        .expect("source loads");
    app.path = source_path.display().to_string();

    let fixture = directory.path().join("lensfun-fixture.xml");
    std::fs::write(&fixture, FIXTURE_XML).expect("write fixture db");

    // Positive: vignetting-only corrector (no distortion, hence no default
    // content crop). The map is bound and the present route is the VRAM path —
    // no badge.
    let db = lumina_lensfun::LensfunDb::load_file(&fixture).expect("fixture db loads");
    let corrector = lumina_lensfun::Corrector::for_camera(
        &db,
        "Lumina Test Corp",
        "Lumina Test Body",
        Some("Lumina Vignetting 50mm f/2.8"),
        AUDIT_SRC_W,
        AUDIT_SRC_H,
        50.0,
        2.8,
        10.0,
    )
    .expect("fixture vignetting profile yields a corrector");
    assert!(
        !corrector.is_identity() && !corrector.has_distortion(),
        "fixture must be a real vignetting-only correction"
    );
    assert!(
        !lumina_core::LensfunMap::from_corrector(&corrector, AUDIT_SRC_W, AUDIT_SRC_H)
            .expect("fixture map builds")
            .has_distortion,
        "the positive fixture must not trigger the default-content-crop guard"
    );
    app.bind_test_lensfun_corrector(corrector, db);
    app.render_draft_tick([AUDIT_SRC_W, AUDIT_SRC_H]);
    app.update_texture(&ctx);
    assert!(
        app.vram_fresh,
        "GPU-LENSFUN-PARITY-1: the bound map must render on the VRAM path"
    );
    assert!(
        app.vram_render_refusal.is_none(),
        "a bound map must not record a present refusal, got {:?}",
        app.vram_render_refusal
    );
    assert!(
        app.gpu_routing_fallback_badge().is_none(),
        "GPU-LENSFUN-PARITY-1: a corrector recipe must present without a badge, got {:?}",
        app.gpu_routing_fallback_badge()
    );

    // Negative: a distortion corrector without an explicit crop hits the CPU
    // content-based default crop. The `lumina-gpu` map guard refuses it, so the
    // route stays loudly on the CPU with a badge naming the reason.
    let db = lumina_lensfun::LensfunDb::load_file(&fixture).expect("fixture db reloads");
    let distortion = lumina_lensfun::Corrector::for_camera(
        &db,
        "Lumina Test Corp",
        "Lumina Test Body",
        Some("Lumina Distortion 50mm f/2.8"),
        AUDIT_SRC_W,
        AUDIT_SRC_H,
        50.0,
        2.8,
        10.0,
    )
    .expect("fixture distortion profile yields a corrector");
    assert!(
        distortion.has_distortion(),
        "negative fixture must carry a distortion model"
    );
    app.bind_test_lensfun_corrector(distortion, db);
    app.render_draft_tick([AUDIT_SRC_W, AUDIT_SRC_H]);
    app.update_texture(&ctx);
    assert!(
        !app.vram_fresh,
        "a distortion corrector without an explicit crop must not present from VRAM"
    );
    let badge = app
        .gpu_routing_fallback_badge()
        .expect("the default-content-crop guard must surface a badge");
    assert!(
        badge.contains("Lensfun corrector"),
        "the badge must name the precise Lensfun reason, got {badge:?}"
    );
}
