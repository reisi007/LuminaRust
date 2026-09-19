//! UX-LOOK-CROP-18 (UXG-01): kittest golden for the interactive crop overlay.
//!
//! The armed crop mode paints corner handles, the thirds grid and the
//! darkening bands outside the crop onto the preview. The golden pins that
//! overlay geometry over the full-frame image (a live, uncommitted session
//! draft), not any wording (Namen-Vorbehalt).
//!
//! Requires a working GPU / headless wgpu backend, so it is `#[ignore]`d by
//! default (same policy as `kittest_snapshots`). Run locally with:
//!
//! ```text
//! UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_crop_overlay -- --ignored
//! ```
//!
//! Kept in its own integration-test file so the oversized
//! `kittest_snapshots.rs` (file-size ratchet) does not grow.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module};
use std::path::{Path, PathBuf};

/// Fixed, committed fixture directory so no random tempdir prefix renders
/// (same rationale as `kittest_snapshots::LIBRARY_FIXTURE_DIR`).
const LIBRARY_FIXTURE_DIR: &str = "tests/fixtures/library";

fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

fn photo_png_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = dir.path().join("photo.png");
    std::fs::write(&photo, LuminaApp::sample_image_png()).expect("write png fixture");
    (dir, photo)
}

/// Open a real file (async decode), wait for the render, then point the browser
/// back at the deterministic fixture directory so the tempdir prefix never
/// reaches pixels.
fn open_file_and_restore_fixture(harness: &mut Harness<'_, LuminaApp>, path: &Path) {
    harness.state_mut().open_file(path.display().to_string());
    for _ in 0..500 {
        harness.run_steps(1);
        if harness.state_mut().preview_generation() >= 1 {
            break;
        }
    }
    assert!(
        harness.state_mut().preview_generation() >= 1,
        "decode of {} never settled in headed harness",
        path.display()
    );
    harness
        .state_mut()
        .set_directory(LIBRARY_FIXTURE_DIR.to_owned());
    harness.run();
}

fn assert_preview_loaded(harness: &mut Harness<'_, LuminaApp>) {
    assert!(
        harness.state_mut().preview_generation() >= 1,
        "a real source must render at least once"
    );
    assert!(
        harness
            .query_all_by_label("Drop an image here or load a path")
            .next()
            .is_none(),
        "empty-state placeholder must be gone (source loaded)"
    );
}

fn assert_no_tmp_leak(harness: &mut Harness<'_, LuminaApp>, tmp: &Path) {
    assert_eq!(
        harness.state_mut().directory(),
        LIBRARY_FIXTURE_DIR,
        "browser must render the relative fixture dir (no tmp-prefix pixels)"
    );
    let component = tmp
        .file_name()
        .and_then(|name| name.to_str())
        .expect("tmp basename");
    assert!(
        harness
            .query_all_by_label_contains(component)
            .next()
            .is_none(),
        "tempdir prefix {component:?} leaked into accessible labels"
    );
}

/// Non-vacuous pixel guard: the darkening bands cover a large near-black area
/// outside the crop, and the four pure-white corner handles are present. This
/// fails on an empty/overlay-less preview regardless of the golden image.
fn assert_crop_overlay_visible(harness: &mut Harness<'_, LuminaApp>) {
    let rendered = harness.render().expect("kittest renders the frame");
    let (width, height) = rendered.dimensions();
    let mut dark = 0usize;
    let mut white = 0usize;
    for (_x, _y, pixel) in rendered.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        if a == 255 && r <= 8 && g <= 8 && b <= 8 {
            dark += 1;
        }
        if a == 255 && r >= 250 && g >= 250 && b >= 250 {
            white += 1;
        }
    }
    assert!(
        dark >= 5_000,
        "the crop darkening must cover a large area, got {dark} near-black pixels"
    );
    assert!(
        white >= 100,
        "the four corner handles must paint white, got {white} white pixels"
    );
    let _ = (width, height);
}

/// Armed crop mode with a live session draft over the full-frame image: the
/// overlay paints the darkening bands, thirds grid and corner handles. The
/// draft is driven through the real pointer path (no recipe crop committed).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_crop_overlay -- --ignored"]
fn develop_overlay_crop_interactive() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo);
    assert_preview_loaded(&mut harness);
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness.state_mut().toggle_crop_mode();
    harness.run();
    let canvas = harness
        .state()
        .overlay_full_rect()
        .expect("preview overlay canvas must be laid out");
    let at = |fx: f32, fy: f32| {
        canvas.min + eframe::egui::vec2(fx * canvas.width(), fy * canvas.height())
    };
    // Drag the top-left handle inward, then the bottom-right handle inward, so
    // the golden shows a centred crop with darkening on all four sides. The
    // press positions are nudged a few points inside the canvas (the handles
    // sit exactly on the full-frame corners).
    drag_handle(
        &mut harness,
        at(0.0, 0.0) + eframe::egui::vec2(3.0, 3.0),
        at(0.25, 0.25),
    );
    drag_handle(
        &mut harness,
        at(1.0, 1.0) - eframe::egui::vec2(3.0, 3.0),
        at(0.75, 0.75),
    );
    harness.run();
    assert_preview_loaded(&mut harness);
    assert_crop_overlay_visible(&mut harness);
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_crop_interactive");
}

/// Drive one real pointer drag from `from` to `to`, one event per frame.
fn drag_handle(
    harness: &mut Harness<'_, LuminaApp>,
    start: eframe::egui::Pos2,
    to: eframe::egui::Pos2,
) {
    let press = |pos: eframe::egui::Pos2, pressed: bool| eframe::egui::Event::PointerButton {
        pos,
        button: eframe::egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    harness.event(eframe::egui::Event::PointerMoved(start));
    harness.run_steps(1);
    harness.event(eframe::egui::Event::PointerMoved(start));
    harness.run_steps(1);
    harness.event(press(start, true));
    harness.run_steps(1);
    harness.event(eframe::egui::Event::PointerMoved(to));
    harness.run_steps(1);
    harness.event(press(to, false));
    harness.run_steps(1);
}
