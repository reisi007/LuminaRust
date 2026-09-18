//! Shared helpers for the `kittest_parity` integration test (extracted from
//! `kittest_parity.rs`, file-size ratchet DoD §8 — the >500-line test file must
//! not grow for a new cell).
//!
//! Contains the scene source generator, the headless GPU harness, the F-043
//! metric helpers and the absolute geometry/pixel assertions used by both the
//! `scenes × paths` matrix and the Lensfun-corrector cell
//! ([`lensfun`]). Byte-identical move of the former file-local helpers; no
//! semantic change.

use egui_kittest::Harness;
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{attach_wgpu_render_state, LuminaApp};
use lumina_sidecar::Crop;
use std::collections::HashSet;

#[cfg(feature = "lensfun")]
mod lensfun;

/// Printed when no usable adapter is bound so the run is visibly skipped,
/// never silently passed (same policy/wording as the `lumina-gpu` oracle
/// tests in `crates/lumina-gpu/tests/golden.rs`).
pub(crate) const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped parity check";

/// Structural PSNR floor from the F-043 policy (`docs/gpu-bootstrap.md`,
/// `lumina-gpu/tests/golden.rs`); `lumina-gpu/tests/parity.rs` uses a stricter
/// 48 dB for its bounded post-tone stages. The Lensfun-corroctor cell measures
/// PSNR=inf (byte-identical).
pub(crate) const MIN_PSNR_DB: f64 = 45.0;
/// Maximum absolute mean signed per-byte error (no systematic tilt).
pub(crate) const MAX_ABS_MEAN_SIGNED_ERROR: f64 = 0.05;

/// Synthetic source dimensions. Small enough for a fast headless render, large
/// enough that the Detail stages act on real texture and the 4:3 fit is exact.
pub(crate) const SRC_W: u32 = 160;
pub(crate) const SRC_H: u32 = 120;

/// Headless harness with the app's GPU context re-based onto the test
/// renderer's wgpu device/queue — the same shared-device wiring the native
/// entry point uses, so the VRAM present path is genuinely exercised.
pub(crate) fn parity_harness() -> Harness<'static, LuminaApp> {
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
pub(crate) fn scene_source_png() -> Vec<u8> {
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
pub(crate) fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len(), "parity inputs must have equal length");
    let mut max = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        max = max.max(x.abs_diff(*y));
    }
    max
}

/// Global PSNR over all RGBA8 bytes (MAX=255); `INFINITY` for identical bytes.
pub(crate) fn psnr_db(a: &[u8], b: &[u8]) -> f64 {
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
pub(crate) fn mean_signed_error(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "bias inputs must have equal length");
    let sum: i64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| i64::from(*x) - i64::from(*y))
        .sum();
    sum as f64 / a.len() as f64
}

/// CPU-draft ↔ GPU/VRAM engine parity at the declared F-043 bound.
pub(crate) fn assert_path_parity(scene: &str, cpu: &ImageFrame, gpu: &ImageFrame, bound: u8) {
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

/// Absolute geometry checks that parity alone cannot catch (both paths could be
/// wrong the same way): the preview fills its fit rect, and the overlay canvas
/// the painter maps overlays onto is the photo rect. The crop-overlay mapping
/// is then resolved against that exact canvas.
pub(crate) fn assert_absolute_geometry(app: &LuminaApp, scene: &str) {
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
pub(crate) fn assert_preview_pixels_present(harness: &mut Harness<'_, LuminaApp>, scene: &str) {
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
