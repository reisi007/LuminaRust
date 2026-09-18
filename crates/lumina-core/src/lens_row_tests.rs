//! R2-LENS-01 row-path contract tests for the Lensfun branch of
//! [`crate::apply_lens`].
//!
//! These were extracted from `lib.rs` (file-size ratchet, DoD §8) and pin the
//! observable contract of the batch row wrappers (`geometry_row` /
//! `subpixel_row` / `apply_vignetting_row`, one FFI transition per row instead
//! of two per pixel):
//!   - the rendered FIRST COLUMN is bit-identical to the previous per-pixel
//!     model (`geometry` + `color_gain` + bilinear `sample`);
//!   - the rest of the frame differs only by the documented sub-pixel
//!     geometry/vignette drift — bounded in coordinate/gain space, not a silent
//!     behaviour change (Golden rebaseline territory, F-043);
//!   - the row path still performs a real correction (no silent no-op).
#![cfg(feature = "lensfun")]

use crate::{apply_lens, sample, ImageFrame, EMPTY_LENS};
use lumina_lensfun::{Corrector, LensfunDb};

const FIXTURE_CAM_MAKE: &str = "Lumina Test Corp";
const FIXTURE_CAM_MODEL: &str = "Lumina Test Body";

fn write_combined_fixture(tag: &str) -> std::path::PathBuf {
    // Same minimal version_1 database as the lumina-lensfun tests: one
    // camera + one lens with BOTH distortion (PTLens) and vignetting
    // (PA) calibration, so the row path exercises both batch callbacks.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
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
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "lumina-core-lensfun-{tag}-{}-{seq}.xml",
        std::process::id()
    ));
    std::fs::write(&path, xml).expect("write fixture database");
    path
}

fn build_row_corrector(width: u32, height: u32) -> Corrector {
    let path = write_combined_fixture("row");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    Corrector::for_camera(
        &db,
        FIXTURE_CAM_MAKE,
        FIXTURE_CAM_MODEL,
        None,
        width,
        height,
        50.0,
        2.8,
        10.0,
    )
    .expect("combined-profile corrector must be built")
}

/// Deterministic gradient + horizontal texture so the bilinear
/// resample and the vignette polynomial are actually exercised.
fn gradient_frame(w: u32, h: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let r = (x * 255 / w.max(1)) as u8;
            let g = (y * 255 / h.max(1)) as u8;
            let b = ((x ^ y) & 0xff) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(w, h, pixels).unwrap()
}

/// The pre-R2-LENS-01 per-pixel render (exactly the old `apply_lens`
/// loop) as the oracle for the row switch.
fn apply_lens_per_pixel(frame: &mut ImageFrame, corrector: &Corrector) {
    let src = frame.clone();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let (sx, sy) = corrector.geometry(x as f64, y as f64);
            let i = (y * frame.width + x) as usize * 4;
            let r = sample(&src, sx as f32, sy as f32, 0);
            let g = sample(&src, sx as f32, sy as f32, 1);
            let b = sample(&src, sx as f32, sy as f32, 2);
            let (cr, cg, cb) = corrector.color_gain(r, g, b, x as f64, y as f64);
            frame.pixels[i] = cr.round().clamp(0.0, 255.0) as u8;
            frame.pixels[i + 1] = cg.round().clamp(0.0, 255.0) as u8;
            frame.pixels[i + 2] = cb.round().clamp(0.0, 255.0) as u8;
            frame.pixels[i + 3] = sample(&src, sx as f32, sy as f32, 3)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
}

#[test]
fn lensfun_row_first_column_is_bit_identical_to_per_pixel_rendered_reference() {
    let corrector = build_row_corrector(257, 200);
    let src = gradient_frame(257, 200);
    let mut row_path = src.clone();
    let mut per_pixel = src;
    // Public pipeline entry: explicit full-frame crop (neutralizes the
    // CROP-MAXRECT-1 content default so this test isolates the lens
    // row-path bit-identity) + empty manual lens + row-path corrector
    // (same route the pipeline takes, F-098-N1).
    let geometry = Some(&lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    row_path
        .apply_geometry(geometry, Some(&EMPTY_LENS), None, Some(&corrector))
        .unwrap();
    apply_lens_per_pixel(&mut per_pixel, &corrector);

    let width = row_path.width as usize;
    for y in 0..row_path.height as usize {
        let base = y * width * 4;
        assert_eq!(
            &row_path.pixels[base..base + 4],
            &per_pixel.pixels[base..base + 4],
            "first-column pixel at y={y} must stay bit-identical"
        );
    }
}

#[test]
fn lensfun_row_full_frame_drift_stays_bounded() {
    let corrector = build_row_corrector(257, 200);
    // The drift contract is asserted in COORDINATE space and GAIN
    // space — not in byte space. Byte diffs equal drift × texture
    // gradient, and this fixture deliberately uses a high-frequency
    // `(x ^ y)` texture whose neighbour bytes differ by up to 255:
    // a sub-pixel coordinate difference then yields triple-digit
    // byte diffs depending on platform float rounding (x86_64 vs
    // ARM), which no byte bound can pin without masking real
    // regressions. Coordinates and gains are smooth, so their
    // bounds are meaningful on every platform.
    let w = 257usize;
    let h = 200usize;
    let mut max_coord_drift = 0.0f64;
    let mut max_coord_at = (0usize, 0usize);
    let mut coords = vec![(0.0f64, 0.0f64); w];
    for y in 0..h {
        corrector.geometry_row(0.0, y as f64, &mut coords);
        for (x, coord) in coords.iter().enumerate().take(w) {
            let (sx, sy) = corrector.geometry(x as f64, y as f64);
            let dx = (coord.0 - sx).abs();
            let dy = (coord.1 - sy).abs();
            if dx > max_coord_drift {
                max_coord_drift = dx;
                max_coord_at = (x, y);
            }
            if dy > max_coord_drift {
                max_coord_drift = dy;
                max_coord_at = (x, y);
            }
        }
    }
    assert!(
        max_coord_drift <= 1e-3,
        "batch geometry_row must match per-pixel geometry within 1e-3 px, got {max_coord_drift} at {:?}",
        max_coord_at,
    );
    // Vignetting gains: batch row call vs per-pixel oracle on the
    // same input triples.
    let src = gradient_frame(w as u32, h as u32);
    let mut max_gain_drift = 0.0f32;
    for y in [0, h / 2, h - 1] {
        let base = y * w * 4;
        let mut row: Vec<f32> = (0..w)
            .flat_map(|x| {
                [
                    src.pixels[base + x * 4] as f32,
                    src.pixels[base + x * 4 + 1] as f32,
                    src.pixels[base + x * 4 + 2] as f32,
                ]
            })
            .collect();
        let expected = row.clone();
        corrector.apply_vignetting_row(&mut row, 0.0, y as f64);
        for x in 0..w {
            let (er, eg, eb) = corrector.color_gain(
                expected[x * 3],
                expected[x * 3 + 1],
                expected[x * 3 + 2],
                x as f64,
                y as f64,
            );
            for (ch, e) in [er, eg, eb].into_iter().enumerate() {
                let d = (row[x * 3 + ch] - e).abs();
                if d > max_gain_drift {
                    max_gain_drift = d;
                }
            }
        }
    }
    assert!(
        max_gain_drift <= 1e-3,
        "batch apply_vignetting_row must match per-pixel color_gain within 1e-3, got {max_gain_drift}"
    );
}

#[test]
fn lensfun_row_path_applies_a_real_correction() {
    // A flat frame highlights both effects: the distortion maps the
    // corner destinations to source pixels outside the image (→ sampled
    // as black, `sample` clamps), and the vignette flattens the
    // falloff. An all-passthrough/identity row path would leave the
    // frame untouched, so this guards against a silent no-op.
    let corrector = build_row_corrector(400, 300);
    let mut frame = ImageFrame::new(400, 300, vec![100u8; 400 * 300 * 4]).unwrap();
    apply_lens(&mut frame, &EMPTY_LENS, Some(&corrector));

    // Centre pixel stays close to the flat input (identity geometry +
    // centre vignette ≈ 1.0)…
    let centre = {
        let i = (150 * 400 + 200) as usize * 4;
        frame.pixels[i]
    };
    assert!(
        (centre as i16 - 100).abs() <= 2,
        "centre should stay ~flat, got {centre}"
    );
    // …but the corners must move (distortion pushes the source sample
    // out of range and/or the vignette scales the falloff) — the row
    // path is a real correction, not a pass-through.
    let corner = frame.pixels[0];
    assert!(
        (corner as i16 - 100).abs() > 2,
        "corner should deviate from the flat input, got {corner}"
    );
}
