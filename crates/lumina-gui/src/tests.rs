//! The inline `#[cfg(test)] mod tests` of `lib.rs`, moved verbatim into
//! `src/tests.rs` (FILE-SIZE-1000-ALL-EXCEPTIONS-100). The module path stays
//! `crate::tests`, so the child modules below still resolve to `src/tests/`
//! and every test name is unchanged; only the wrapper braces and the 4-space
//! body indentation of the inline module are gone.

use super::*;
// GUI-INSTRDBG-17b: the remaining section-action logging tests, extracted
// to keep this root test module within the file-size ratchet.
mod action_log_direct_triggers;
mod action_log_trigger_setup;
// GUI-INSTRDBG-17b-REST: the click tests for the Spot/Detail/Optics/
// Tone-Curve/Presets buttons (second extracted slice).
mod action_log_click_spot_detail_optics_presets;
// GUI-INSTRDBG-17c: core instrumentation tests (moved out of gui_action.rs)
// plus the last-button click tests (WB eyedropper, Point Color, Spot
// distraction, Red-Eye picker, Presets refresh, generative canvas).
mod action_log_click_basic_color_generative;
mod action_log_click_section_controls_and_filmstrip;
mod action_log_contract;
// GUI-INSTRDBG-17c-Rest: the People-view "Use as mask" click test.
mod action_log_click_people_use_as_mask;
// GUI-GPU-AUDIT-17: headless audit over every `GuiAction` (Metal, --ignored).
mod gpu_audit;
mod gpu_audit_actions;
// GUI-REFACTOR-W3-20 / UX-LOOK-TOOLBAR-18: thematic split of the former
// monolithic root test module. The shared draw/click helpers live in
// `support.rs` (see the ratchet comment there); each file pulls them in via
// `use super::*` and keeps its test bodies otherwise unchanged.
mod support;
use support::*;
mod badges;
mod basic_commit;
mod brush_gradient;
mod brush_interaction;
mod brush_lifecycle;
mod brush_management;
mod brush_management_ui;
mod button_audit_actions;
mod button_audit_click_toggles;
mod button_audit_shortcut_maps;
mod button_audit_surfaces;
mod distortion;
mod export;
mod g15_batch;
mod g15_collections;
mod g15_stacks;
// AUTO-TONE-CLI-6 clause (1): the GUI and the CLI must write the same
// auto-tone contract. Differential (GUI vs. the shared writer) plus the
// structural guard that a second hand-copied algorithm literal cannot return.
mod auto_tone_contract;
// AUTO-TONE-ANALYSIS-INPUT-8: the Auto-Tone measurement domain (post
// SourceActions + post Crop, without Adjustments) proven with numbers
// through the real `auto_tone` entry point.
mod auto_tone_analysis_domain;
// AUTO-TONE-ANALYSIS-INPUT-8: the SourceActions half of that domain (the
// retouched measurement) plus the reload behaviour when it cannot be built.
mod auto_tone_analysis_source_actions;
// AUTO-TONE-CLI-6 clause (2): split out of `g16_shortcuts` for the 500-line
// ratchet — the keyboard mappings stay there, the state the G-16 end point
// leaves in the recipe is its own concern.
mod g16_auto_endpoint;
// AUTO-TONE-ENDPOINT-MIXED-7: the order-sensitive acceptance sequences
// (frisch/voller Lauf/zweimal Endpunkt, repair, load-path stale clear).
mod g16_auto_endpoint_sequences;
mod g16_shortcuts;
mod generative_expand;
mod generative_render;
mod generative_status;
mod geometry;
mod geometry_session;
mod gpu_mask_drafts;
mod gpu_mask_gate;
mod gpu_mask_plane;
// GPU-MASKPLANE-BRUSH-50: a failed live-brush upload clears the plane.
mod gpu_mask_plane_brush;
mod gpu_routing;
// GPU-ROUTE-LOG-54: editorial present refusals explained in the trace.
// Declared here rather than beside the other `gpu_*` modules to keep
// this insertion zero-width for the `lib.rs` ratchet: the
// alphabetical group above is full and ratcheted files may not grow.
mod gpu_route_log;
mod treatment_panel_histogram;
// R3-Runde-3: routing/denoise fixes (R3-ROUTING-1/-DENOISE-1/-DENOISE-2).
mod crop_display_and_denoise_policy;
mod gpu_state;
mod histogram;
// UX-LOOK-HISTORY-18: readable/clickable history entries + presets tree.
mod history_presets_look;
mod iptc;
mod layout;
mod lens_blur;
// LENSFUN-CALLER-37: the diagnostics sink's three contract clauses. Gated on
// the same feature as the module under test — without the gate the lean
// `--no-default-features` build loses both `lensfun_diag` and `lumina_lensfun`.
#[cfg(feature = "lensfun")]
mod lensfun_diagnostics;
mod library_scan;
mod library_sort;
mod library_sync;
mod library_tree_r4;
mod library_views;
// R2-MODSWITCH-1 F7: module-switch latency (off-thread thumbnail cache,
// metadata-only probe, deferred full render).
mod mask_local;
mod mask_local_color;
mod mask_local_curve_graph;
mod mask_local_curves;
mod mask_local_detail;
mod mask_local_presence;
mod mask_local_previous;
mod mask_visibility;
mod masking_g03;
mod masking_g11;
mod masking_groups;
mod modswitch;
mod navigator;
mod navigator_r4;
mod optics;
mod panels;
mod preview_placement;
mod preview_render;
// R3-RENDER-SIZE-1: the preview viewport cap (draft + full) and its
// export / 1:1-loupe exemptions.
mod preview_viewport_cap;
// R2-MODSWITCH-1 F8: the asynchronous folder scan (worker + drain).
mod library_scan_async;
// R3-CONFLICT-1: the CAS-rebase path under a real two-writer race.
mod recipe_session;
mod red_eye;
mod render_cache;
mod render_dirty;
mod scheduling;
mod selection;
mod shortcuts;
mod sidecar;
mod sidecar_rebase_race;
mod sidecar_restore;
// GUI-SRCACC-1: strict resolver, active/export parity, cache identity,
// and navigator/neighbor/thumbnail stand-in coverage.
mod sliders_commit;
mod sliders_domain;
mod sliders_filmstrip;
mod source_actions;
// THUMB-HASH-PERF-35: the whole-file source-identity memo (hash once per
// file state, not once per frame) and its changed-file/missing-file guards.
mod source_identity_cache;
mod spot_heal;
// R5-DUST-23-FOLLOWUP: spot selection + per-spot editing (select/update/
// remove, detail-only-when-selected, no-Clone-fallback).
mod spot_followup;
mod spot_followup_ui;
// R5-DUST-23: Dust-Removal toolbar tool (arming, size, dab, cursor).
mod spot_tool;
mod spot_visualize;
mod startup;
// R3-WARMUP-1: the one-shot cold-start warmup tests.
mod startup_warmup;
mod toast;
// UX-LOOK-TONECURVE-18: interactive tone-curve graph tests.
mod tone_curve_graph;
// R3-LOG-1: wall-clock switch/decode/render instrumentation tests.
mod timing_instrumentation;
// UX-LOOK-TOOLBAR-18: icon tool strip + Library view-tab paint/click tests.
mod toolbar_icons;
mod w3_release;
mod zoom;
mod zoom_steps;
use lumina_core::ImageFileFormat;
use lumina_sidecar::{
    BokehShape, BrushMark, BrushMarkSign, CoordinateSystem, Crop, DecodeFingerprint,
    DepthArtifactRef, GenerativeCanvas, GenerativeEdit, GeometryFingerprint, LensCorrection,
    MaskDefinition, MaskOperation, MaskPrompt, MaskStatus, ModelIdentity, NormalizedRect, Point2,
    Preprocessing, PromptTransform, Resolution, SourceFingerprint, SourceStatus,
};
/// GUI-STARTUP-FOLLOWUP-1 (B4): JPEG fixture through the real encoder so
/// the startup test below decodes genuine JPEG bytes (not a renamed PNG).
fn jpeg() -> Vec<u8> {
    ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .unwrap()
        .encode(ImageFileFormat::Jpeg)
        .unwrap()
}
/// GUI-STARTUP-FOLLOWUP-1 (B4): WebP fixture through the real encoder so
/// the startup test below decodes genuine WebP bytes (not a renamed PNG).
fn webp() -> Vec<u8> {
    ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .unwrap()
        .encode(ImageFileFormat::WebP)
        .unwrap()
}
/// GUI-STARTUP-SELECTION-1: pump the background decode until the directory
/// auto-load settles (loaded frame or loud error). Mirrors
/// `open_and_decode` without opening a file first — the load was started
/// by the scan itself.
fn drain_auto_load(app: &mut LuminaApp) {
    for _ in 0..2000 {
        app.poll_decode();
        if app.original.is_some() || app.error().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
// -----------------------------------------------------------------------
// F-100 Klickbarkeit (GUI-CLICK-ALL-17): per-button click/toggle tests.
// Paint-only is not enough (DoD §3/§5): every new button must flip its
// state on a real headless click, exactly like the Crop/Lights-Out anchors.
// -----------------------------------------------------------------------

/// Load the in-memory sample used by the display-only toolbar tests.
fn toolbar_app() -> LuminaApp {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app
}

/// A real file on disk so the persisting History buttons (duplicate,
/// snapshot, stack) can save their sidecar.
fn persistent_app() -> (tempfile::TempDir, LuminaApp) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.ensure_document_loaded().unwrap();
    (directory, app)
}

/// Raw listing fixture: `scan_entry`/`list_directory` only need a
/// supported extension — no decode runs during a directory scan.
fn save_raw(path: &Path) {
    std::fs::write(path, b"lumina-raw-listing-sentinel").unwrap();
}

// ---- G-11 overlay/panel comfort (LRPAR-G11-OVERLAYS) ----

/// Create a mask and inject a box prompt so overlay/pin tests have
/// deterministic geometry without a pointer drag. Returns the mask id;
/// the mask is selected afterwards.
fn mask_with_box_prompt(app: &mut LuminaApp, name: &str, rect: (f32, f32, f32, f32)) -> String {
    let mask_id = app.create_mask(name).unwrap();
    {
        let virtual_copy_id = app.virtual_copy_id.clone();
        let document = app.document.as_mut().expect("document loaded");
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == virtual_copy_id)
            .expect("active copy");
        let mask = copy
            .mask_library
            .iter_mut()
            .find(|mask| mask.id == mask_id)
            .expect("mask");
        mask.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: rect.0,
                y: rect.1,
                width: rect.2,
                height: rect.3,
            },
            transformation: PromptTransform::default(),
        });
    }
    app.select_mask(&mask_id).unwrap();
    mask_id
}

// ---- PERF-GUI-1: staged base cache (hit/miss, stepwise invalidation,
// pixel identity) ----

/// Deterministic RGBA gradient PNG so the render stages exercise real
/// per-pixel math over more than a handful of samples.
fn gradient_png(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / width) as u8;
            let g = ((y * 255) / height) as u8;
            let b = ((x + y) % 256) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(width, height, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

fn save_png(path: &Path) {
    let png = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(path, png).unwrap();
}

/// LRPAR-G08-PREVIOUS: like [`open_and_decode`], but waits for an image
/// *switch* — `open_and_decode` returns immediately when any frame is
/// loaded, so a second open would assert against the still-loaded first
/// image. Pumps until the new path is adopted (or a loud error lands).
fn open_and_decode_switch(app: &mut LuminaApp, path: &str) {
    app.open_file(path.to_string());
    for _ in 0..2000 {
        app.poll_decode();
        if app.path == path || app.error().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Shared commit-save-assert for the struct-backed slider classes
/// (GUI-SLIDER-SAVE-1, native only): runs the debounced commit, loads the
/// written sidecar document and fails loudly when no file was written.
fn commit_and_load_doc(
    app: &mut LuminaApp,
    source: &std::path::Path,
) -> lumina_sidecar::SidecarDocument {
    app.commit_pending_slider_save([0, 0]);
    assert!(
        app.error().is_none(),
        "commit must not fail, got {:?}",
        app.error()
    );
    let sidecar = lumina_sidecar::sidecar_path_for(source);
    assert!(sidecar.is_file(), "Sidecar must be written");
    lumina_sidecar::load_sidecar(&sidecar).unwrap()
}

/// Reopen a source in a fresh app (DoD §1: values survive restarts).
fn reopen_app(source: &std::path::Path) -> LuminaApp {
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app
}

// ---- LRPAR-G06-UPRIGHT-15 / G-14 (LRPAR-G14-REDEYE-15) ----

/// A `size`×`size` grid (bright bars on dark) tilted by `angle_deg`, so the
/// deterministic upright analysis has a real line signal.
fn save_tilted_png(path: &Path, angle_deg: f32) {
    let size = 128u32;
    let (sa, ca) = angle_deg.to_radians().sin_cos();
    let period = 0.4f32;
    let mut pixels = vec![0u8; (size as usize) * (size as usize) * 4];
    for y in 0..size {
        for x in 0..size {
            let nx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let ny = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let rx = ca * nx + sa * ny;
            let dy = (rx / period - (rx / period).round()).abs() * period;
            let value = if dy < 0.05 { 235u8 } else { 20u8 };
            let i = ((y * size + x) as usize) * 4;
            pixels[i] = value;
            pixels[i + 1] = value;
            pixels[i + 2] = value;
            pixels[i + 3] = 255;
        }
    }
    let frame = ImageFrame::new(size, size, pixels).unwrap();
    std::fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
}

/// Grey RGBA PNG with solid red rectangles — the deterministic GUI fixture
/// for LRPAR-G14-REDEYE-AUTO-15.
fn save_red_pupil_png(path: &Path, width: u32, height: u32, pupils: &[(u32, u32, u32, u32)]) {
    let mut frame = ImageFrame::new(
        width,
        height,
        [120u8, 120, 120, 255]
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect(),
    )
    .unwrap();
    for &(x0, y0, x1, y1) in pupils {
        for y in y0..y1 {
            for x in x0..x1 {
                let index = ((y * width + x) as usize) * 4;
                frame.pixels_mut()[index..index + 4].copy_from_slice(&[220, 30, 40, 255]);
            }
        }
    }
    std::fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
}

// ---- GUI-SCROLL-200-1: visible-first thumbnail scheduling (headless) ----

/// Build an app browsing a folder with `count` supported images and return
/// the app plus all entry indices (the scheduling helpers are format-
/// agnostic; the RAW filter only applies to which views show entries).
fn app_with_entries(count: usize) -> (LuminaApp, tempfile::TempDir, Vec<usize>) {
    let directory = tempfile::tempdir().unwrap();
    for i in 0..count {
        save_png(&directory.path().join(format!("img{i:03}.png")));
    }
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    let indices: Vec<usize> = (0..app.entries().len()).collect();
    assert_eq!(indices.len(), count, "every image must become an entry");
    (app, directory, indices)
}

// ---- GPU-LENSFUN-PARITY-1: a corrector recipe presents on the GPU route ----

/// Minimal version_1 Lensfun fixture database (same shape as the
/// `lumina-core` row tests): one camera + one lens with distortion and
/// vignetting calibration. Embedded so the test builds a genuinely
/// non-identity corrector deterministically, without depending on the
/// system profile DB.
#[cfg(all(feature = "gpu", feature = "lensfun"))]
const LENSFUN_GATE_FIXTURE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Test Body</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
    </camera>
    <lens>
        <maker>Lumina Test Corp</maker>
        <model>Lumina Distortion+Vignetting 50mm f/2.8</model>
        <mount>LuminaTestMount</mount>
        <cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.08" b="-0.10" c="0.02"/>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
        </calibration>
    </lens>
</lensdatabase>
"#;

#[cfg(all(feature = "gpu", feature = "lensfun"))]
fn synthetic_lensfun_corrector(
    directory: &std::path::Path,
) -> (lumina_lensfun::Corrector, lumina_lensfun::LensfunDb) {
    let path = directory.join("lensfun-gate-fixture.xml");
    std::fs::write(&path, LENSFUN_GATE_FIXTURE_XML).expect("write fixture database");
    let db = lumina_lensfun::LensfunDb::load_file(&path).expect("fixture database must load");
    let corrector = lumina_lensfun::Corrector::for_camera(
        &db,
        "Lumina Test Corp",
        "Lumina Test Body",
        None,
        640,
        480,
        50.0,
        2.8,
        10.0,
    )
    .expect("fixture profile must yield a corrector");
    (corrector, db)
}

// ---- F-103-INTEGRATION-PREVIEW-SIDECAR: headless UI integration ----

fn synthetic_8x8_png() -> (Vec<u8>, ImageFrame) {
    // 8×8 checkerboard: 4×4 blocks alternating dark/mid, alpha 255.
    // Block color A = 32, B = 180 — both well inside 0..255 so exposure
    // brightening is measurable without clipping immediately.
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let is_a = ((x / 4) + (y / 4)) % 2 == 0;
            let v = if is_a { 32u8 } else { 180u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let png = frame.encode(ImageFileFormat::Png).unwrap();
    (png, frame)
}

fn avg_luminance(frame: &ImageFrame) -> f64 {
    let sum: u64 = frame
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[0] as u64 + p[1] as u64 + p[2] as u64)
        .sum();
    sum as f64 / (frame.width() as f64 * frame.height() as f64 * 3.0)
}

// ---- GEN-FILL-02 / GEN-ONNX-1 Welle 2b: Artefakt-Compositing ----

/// GEN-ONNX-1 Welle 2b: build the deterministic fixture canvas for `edit`'s
/// active role over `input` and wrap it as the core render-hook artifact.
fn fixture_canvas_artifact(
    input: &ImageFrame,
    edit: &GenerativeEdit,
    role: lumina_onnx::GenerativeRole,
) -> GenerativeCanvasArtifact {
    let output = lumina_onnx::produce_canvas(
        input,
        edit,
        &lumina_onnx::GenerativeModelSource::Fixture(role),
    )
    .unwrap();
    let core_role = match role {
        lumina_onnx::GenerativeRole::Expand => lumina_core::GenerativeRole::Expand,
        lumina_onnx::GenerativeRole::AutoFillTransparent => {
            lumina_core::GenerativeRole::AutoFillTransparent
        }
    };
    GenerativeCanvasArtifact::new(core_role, output.to_frame().unwrap())
}

/// GEN-ONNX-1 Welle 2b: give `app` a real source path (writing `png` there)
/// so the generative action can persist its `.lumina.zdata` bundle. The
/// returned tempdir must stay alive for the test.
fn app_source_path(app: &mut LuminaApp, png: &[u8], name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(name);
    std::fs::write(&source, png).unwrap();
    app.path = source.display().to_string();
    dir
}

// ---- LRPAR-G04-REMOVE (G-04 Remove-Parität) ---------------------------
/// 16×16 fixture with a dark 8×8 block (top-left): exactly one heuristic
/// 8×8 cell at threshold 0.5, deterministic across runs.
fn dark_block_png() -> Vec<u8> {
    let mut pixels = vec![255u8; 16 * 16 * 4];
    for y in 0..8 {
        for x in 0..8 {
            let idx = (y * 16 + x) as usize * 4;
            pixels[idx] = 0;
            pixels[idx + 1] = 0;
            pixels[idx + 2] = 0;
        }
    }
    ImageFrame::new(16, 16, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// 32x32 vertical-stripe fixture (G-05): even columns black, odd columns
/// white — high contrast so any bokeh blur visibly changes pixels.
fn lens_blur_striped_png() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for _ in 0..32 {
        for x in 0..32 {
            let v = if x % 2 == 0 { 0 } else { 255 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    ImageFrame::new(32, 32, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// GUI-PREVIEW-NOISE-1: a synthetic 64×40 RGB gradient (content spread
/// over the whole luminance range, like a real photo — never a flat
/// field). Pure helper, no app side effects.
fn synthetic_gradient_png() -> (Vec<u8>, ImageFrame) {
    let (w, h) = (64u32, 40u32);
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let r = (x * 255 / (w - 1)) as u8;
            let g = (y * 255 / (h - 1)) as u8;
            let b = ((x + y) * 255 / (w - 1 + h - 1)) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let frame = ImageFrame::new(w, h, pixels).unwrap();
    let png = frame.encode(ImageFileFormat::Png).unwrap();
    (png, frame)
}

/// Normalized L1 distance of two 256-bin histograms (`0` = identical
/// distributions, `2` = disjoint). Scale-free so a full-res preview and a
/// downscaled thumbnail of the same content compare directly.
fn histogram_l1(a: &[u64], b: &[u64]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum_a: u64 = a.iter().sum();
    let sum_b: u64 = b.iter().sum();
    assert!(sum_a > 0 && sum_b > 0, "histograms must be non-empty");
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| (x as f64 / sum_a as f64 - y as f64 / sum_b as f64).abs())
        .sum()
}

/// Build a synthetic RAW-only browser entry (no disk IO — the name
/// extension alone drives the RAW filter).
fn raw_entry(dir: &std::path::Path, name: &str) -> FileBrowserEntry {
    let path = dir.join(name);
    FileBrowserEntry {
        thumb_key: path.display().to_string(),
        name: name.to_string(),
        path,
        has_sidecar: false,
        source_status: SourceStatus::Missing,
        conflict: false,
        virtual_copies: 0,
        missing_models: 0,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        color_label: 0,
        keywords: Vec::new(),
        collections: Vec::new(),
        camera: None,
        iso: None,
        focal_length: None,
        capture_timestamp: None,
        folder: String::new(),
        cull_badge: cull_gui::CullBadge::None,
        face_persons: Vec::new(),
        stack: None,
    }
}

/// Read one manual optics field from a lens block (DoD-§3
/// Klassen-Vollständigkeit: all eight fields share one assertion path).
fn lens_field(lens: &LensCorrection, field: &str) -> Option<f32> {
    match field {
        "distortion_k1" => lens.distortion_k1,
        "distortion_k2" => lens.distortion_k2,
        "distortion_k3" => lens.distortion_k3,
        "vignette_c0" => lens.vignette_c0,
        "vignette_c1" => lens.vignette_c1,
        "vignette_c2" => lens.vignette_c2,
        "ca_red" => lens.ca_red,
        "ca_blue" => lens.ca_blue,
        _ => None,
    }
}

/// Test-Eintrag mit allen erweiterten Filterdaten (Keywords,
/// Sammlungen, EXIF). Die Felder sind `pub(crate)`; der Test baut den
/// Eintrag per Feldname (GUI-LIB-SPLIT-65 Slice 9).
fn filter_entry() -> FileBrowserEntry {
    FileBrowserEntry {
        path: PathBuf::from("/tmp/a.png"),
        name: "a.png".to_string(),
        thumb_key: "a".to_string(),
        has_sidecar: true,
        source_status: SourceStatus::Unchanged,
        conflict: false,
        virtual_copies: 1,
        missing_models: 0,
        rating: 4,
        flag: Flag::Pick,
        color_label: 1,
        keywords: vec!["portrait".to_string(), "Studio".to_string()],
        collections: vec![CollectionMembership {
            id: "best".to_string(),
            name: "Best Of".to_string(),
        }],
        camera: Some("Canon EOS R5".to_string()),
        iso: Some(400.0),
        focal_length: Some(50.0),
        capture_timestamp: Some(1_700_000_000),
        folder: String::new(),
        cull_badge: cull_gui::CullBadge::Review,
        face_persons: vec!["Alex".to_string()],
        stack: None,
    }
}

// ---- LRPAR-G15-IPTC-S8: Library Metadata panel -----------------------
//
// Jede Aktion folgt der Kette Edit → Commit → Datei → Reload (DoD §7);
// das Original bleibt dabei byte-identisch.

fn save_jpeg(path: &Path) {
    std::fs::write(path, jpeg()).unwrap();
}

fn write_meta_preset(
    dir: &Path,
    file: &str,
    name: &str,
    fields: &[(&str, &str)],
    placeholders: &[(&str, &str)],
) -> PathBuf {
    let preset = serde_json::json!({
        "format": "lumina-meta-preset",
        "version": 1,
        "name": name,
        "fields": fields.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect::<std::collections::BTreeMap<String, String>>(),
        "placeholders": placeholders.iter().map(|(n, d)| serde_json::json!({"name": n, "description": d})).collect::<Vec<_>>(),
    });
    let path = dir.join(file);
    std::fs::write(&path, serde_json::to_vec_pretty(&preset).unwrap()).unwrap();
    path
}

fn seed_sidecar(source: &Path) {
    let mut setup = new_app();
    open_and_decode(&mut setup, source.display().to_string());
    setup.add_keyword("seed").unwrap();
}
