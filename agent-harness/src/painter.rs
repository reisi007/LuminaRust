//! Painter-home helpers (AGENT-HARNESS-2).
//!
//! Analysis result: three GUI contents are Painter-composited and therefore
//! invisible to AccessKit, so `ui_tree_json()` can never expose them without
//! a production change:
//!
//! 1. **Library path badges** — `ui.painter().text()` over a `rect_filled`
//!    chip (`crates/lumina-gui/src/lib.rs`, `LIBRARY_BADGE_BG`). Immediate-mode
//!    painter drawing creates no AccessKit node (only widgets do); the only
//!    tree trace is the hover tooltip (`on_hover_text`), which kittest only
//!    materialises on hover. Verdict home: composited PNG pixels.
//! 2. **Navigator viewport rect** — `ui.painter().rect_stroke()` with
//!    `crate::theme::ACCENT` (`draw_navigator_viewport`). Same reason:
//!    stroke-only, no widget, no node. Verdict home: composited PNG pixels
//!    for presence; exact rect-vs-ROI geometry additionally needs production
//!    getters (see Folgeaufgaben in `README.md`).
//! 3. **Preview photo content** — a GPU/native texture (`painter().image`).
//!    The texture paints black in kittest readback (no GPU composition
//!    headless), so composited pixels cannot prove content either. Verdict
//!    home: the in-app `preview()` RGBA frame (pre-texture pipeline output).
//!
//! All helpers here are harness-side only: they query the composited PNG or
//! the public `preview()` frame. No production behaviour changes.

use image::RgbaImage;

use crate::TreeNode;

/// Fill colour of the Library badge chips, mirroring prod
/// `LIBRARY_BADGE_BG` (`crates/lumina-gui/src/lib.rs`).
pub const BADGE_CHIP_RGB: [u8; 3] = [0x42, 0x42, 0x42];

/// Stroke colour of the navigator viewport rect, mirroring prod
/// `crate::theme::ACCENT` (`crates/lumina-gui/src/theme.rs`).
pub const NAV_RECT_ACCENT_RGB: [u8; 3] = [0x4a, 0x90, 0xd9];

/// Tolerance for chip-fill pixels: the fill is flat, so a tight bound keeps
/// AA text edges and neighbouring surfaces out of the count.
pub const BADGE_CHIP_TOL: u8 = 4;

/// Tolerance for the 2px accent stroke: slightly wider than the chip bound
/// because thin strokes anti-alias against varying backgrounds.
pub const NAV_ACCENT_TOL: u8 = 8;

/// Minimum chip-coloured pixels for a PASS: one painted badge box is
/// 118x16 = 1888 fill pixels (minus the white glyph area); 800 stays green
/// even if only one of two subfolder cells is fully visible.
pub const MIN_BADGE_CHIP_PIXELS: u64 = 800;

/// Minimum accent pixels for a PASS: a 2px rect perimeter around a ~200px
/// overview is well over 1000 stroke pixels; 200 stays green for small
/// navigators while ruling out stray accent dots.
pub const MIN_NAV_ACCENT_PIXELS: u64 = 200;

/// Count pixels whose RGB is within `tol` (per-channel abs diff) of `rgb`.
/// Alpha is ignored: chips and strokes are opaque, and the readback is too.
pub fn count_near_color(img: &RgbaImage, rgb: [u8; 3], tol: u8) -> u64 {
    let tol = u16::from(tol);
    img.pixels()
        .filter(|px| {
            u16::from(px[0].abs_diff(rgb[0])) <= tol
                && u16::from(px[1].abs_diff(rgb[1])) <= tol
                && u16::from(px[2].abs_diff(rgb[2])) <= tol
        })
        .count() as u64
}

/// Badge-chip pixels in a composited shot (path-badge evidence).
pub fn badge_chip_pixels(img: &RgbaImage) -> u64 {
    count_near_color(img, BADGE_CHIP_RGB, BADGE_CHIP_TOL)
}

/// Accent-stroke pixels in a composited shot (navigator-rect evidence).
pub fn nav_accent_pixels(img: &RgbaImage) -> u64 {
    count_near_color(img, NAV_RECT_ACCENT_RGB, NAV_ACCENT_TOL)
}

/// Statistics of an in-app RGBA8 frame (`preview().pixels`, row-major).
#[derive(Debug, Clone)]
pub struct AppFrameStats {
    pub width: u32,
    pub height: u32,
    pub mean_luma: f64,
    pub std_luma: f64,
    pub gray_fraction: f64,
    /// Fraction of pixels with alpha == 255 (opaque-composition contract).
    pub opaque_fraction: f64,
}

/// Statistics over raw RGBA8 bytes. `None` when `pixels.len()` does not match
/// `width * height * 4` or the frame is empty.
pub fn app_frame_stats(width: u32, height: u32, pixels: &[u8]) -> Option<AppFrameStats> {
    if width == 0 || height == 0 {
        return None;
    }
    let expect = width as usize * height as usize * 4;
    if pixels.len() != expect {
        return None;
    }
    stats_over_rgba(width, height, pixels, 0, 0, width, height)
}

/// Statistics restricted to the central `frac` (0..1, exclusive) of the frame.
/// `None` on dimension mismatch or degenerate crop.
pub fn app_frame_center_stats(
    width: u32,
    height: u32,
    pixels: &[u8],
    frac: f32,
) -> Option<AppFrameStats> {
    if !(0.0 < frac && frac < 1.0) {
        return None;
    }
    let expect = width as usize * height as usize * 4;
    if pixels.len() != expect || width == 0 || height == 0 {
        return None;
    }
    let mx = (width as f32 * (1.0 - frac) / 2.0) as u32;
    let my = (height as f32 * (1.0 - frac) / 2.0) as u32;
    let cw = (width as f32 * frac) as u32;
    let ch = (height as f32 * frac) as u32;
    if cw == 0 || ch == 0 || mx + cw > width || my + ch > height {
        return None;
    }
    stats_over_rgba(width, height, pixels, mx, my, cw, ch)
}

fn stats_over_rgba(
    width: u32,
    height: u32,
    pixels: &[u8],
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
) -> Option<AppFrameStats> {
    let stride = width as usize * 4;
    let mut sum = 0.0;
    let mut sum2 = 0.0;
    let mut gray = 0u64;
    let mut opaque = 0u64;
    let mut n = 0u64;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = y as usize * stride + x as usize * 4;
            let p = &pixels[i..i + 4];
            let luma =
                (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                    / 255.0;
            sum += luma;
            sum2 += luma * luma;
            if p[0]
                .abs_diff(p[1])
                .max(p[1].abs_diff(p[2]))
                .max(p[0].abs_diff(p[2]))
                <= 6
            {
                gray += 1;
            }
            if p[3] == 255 {
                opaque += 1;
            }
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let n = n as f64;
    let mean = sum / n;
    let var = (sum2 / n - mean * mean).max(0.0);
    Some(AppFrameStats {
        width,
        height,
        mean_luma: mean,
        std_luma: var.sqrt(),
        gray_fraction: gray as f64 / n,
        opaque_fraction: opaque as f64 / n,
    })
}

/// PSNR (dB) between two same-length byte buffers (all bytes incl. alpha).
/// `None` on length mismatch or empty input; `INFINITY` when identical
/// (determinism proof for consecutive preview frames).
pub fn frame_psnr(a: &[u8], b: &[u8]) -> Option<f64> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let mut se = 0u64;
    for (x, y) in a.iter().zip(b.iter()) {
        let d = u64::from(x.abs_diff(*y));
        se += d * d;
    }
    if se == 0 {
        return Some(f64::INFINITY);
    }
    let mse = se as f64 / a.len() as f64;
    Some(10.0 * (255.0 * 255.0 / mse).log10())
}

/// Combined display text of a tree node: static text lives in `value`,
/// widget names in `label` — callers must match both.
pub fn combined_text(node: &TreeNode) -> String {
    if node.label.is_empty() {
        node.value.clone()
    } else if node.value.is_empty() {
        node.label.clone()
    } else {
        format!("{} | {}", node.label, node.value)
    }
}

/// Combined texts of all nodes containing `needle` (label or value).
pub fn texts_containing(nodes: &[TreeNode], needle: &str) -> Vec<String> {
    nodes
        .iter()
        .filter(|n| n.text_contains(needle))
        .map(combined_text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: image::Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(w, h, px)
    }

    #[test]
    fn near_color_counts_exact_and_ignores_alpha() {
        let mut img = solid(8, 8, image::Rgba([10, 10, 10, 255]));
        img.put_pixel(0, 0, image::Rgba([0x42, 0x42, 0x42, 255]));
        img.put_pixel(1, 0, image::Rgba([0x42, 0x42, 0x42, 0]));
        img.put_pixel(2, 0, image::Rgba([0x47, 0x42, 0x42, 255]));
        assert_eq!(badge_chip_pixels(&img), 2);
        assert_eq!(count_near_color(&img, BADGE_CHIP_RGB, 4), 2);
        assert_eq!(count_near_color(&img, BADGE_CHIP_RGB, 5), 3);
        assert_eq!(nav_accent_pixels(&img), 0);
    }

    #[test]
    fn chip_box_yields_expected_fill_count() {
        let mut img = solid(200, 60, image::Rgba([24, 24, 24, 255]));
        for y in 2..18 {
            for x in 4..122 {
                img.put_pixel(x, y, image::Rgba([0x42, 0x42, 0x42, 255]));
            }
        }
        assert_eq!(badge_chip_pixels(&img), 118 * 16);
        assert!(badge_chip_pixels(&img) >= MIN_BADGE_CHIP_PIXELS);
    }

    #[test]
    fn accent_stroke_detected_above_threshold() {
        let mut img = solid(300, 200, image::Rgba([30, 30, 30, 255]));
        for x in 40..240 {
            for dy in 0..2 {
                img.put_pixel(x, 30 + dy, image::Rgba([0x4a, 0x90, 0xd9, 255]));
                img.put_pixel(x, 170 + dy, image::Rgba([0x4a, 0x90, 0xd9, 255]));
            }
        }
        for y in 30..172 {
            for dx in 0..2 {
                img.put_pixel(40 + dx, y, image::Rgba([0x4a, 0x90, 0xd9, 255]));
                img.put_pixel(240 + dx, y, image::Rgba([0x4a, 0x90, 0xd9, 255]));
            }
        }
        assert!(nav_accent_pixels(&img) >= MIN_NAV_ACCENT_PIXELS);
        assert_eq!(badge_chip_pixels(&img), 0);
    }

    #[test]
    fn app_stats_reject_bad_shapes() {
        assert!(app_frame_stats(0, 4, &[]).is_none());
        assert!(app_frame_stats(2, 2, &[0u8; 15]).is_none());
        assert!(app_frame_stats(2, 2, &[]).is_none());
        assert!(app_frame_center_stats(4, 4, &[0u8; 64], 0.0).is_none());
        assert!(app_frame_center_stats(4, 4, &[0u8; 64], 1.0).is_none());
        assert!(app_frame_center_stats(4, 4, &[0u8; 63], 0.5).is_none());
    }

    #[test]
    fn app_stats_opaque_flat_frame() {
        let mut px = vec![0u8; 8 * 8 * 4];
        for chunk in px.as_chunks_mut::<4>().0.iter_mut() {
            chunk.copy_from_slice(&[128, 128, 128, 255]);
        }
        let s = app_frame_stats(8, 8, &px).expect("valid");
        assert!((s.mean_luma - 128.0 / 255.0).abs() < 1e-9);
        assert!(s.std_luma < 1e-6);
        assert!((s.opaque_fraction - 1.0).abs() < 1e-12);
        assert!((s.gray_fraction - 1.0).abs() < 1e-12);
    }

    #[test]
    fn app_stats_see_transparency_and_content() {
        let mut px = vec![0u8; 4 * 4 * 4];
        for (i, chunk) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if i < 8 {
                chunk.copy_from_slice(&[240, 235, 220, 255]);
            } else {
                chunk.copy_from_slice(&[20, 25, 90, 128]);
            }
        }
        let s = app_frame_stats(4, 4, &px).expect("valid");
        assert!(s.std_luma > 0.1, "content must vary: {s:?}");
        assert!((s.opaque_fraction - 0.5).abs() < 1e-12, "{s:?}");
        assert!(s.gray_fraction < 0.6, "{s:?}");
    }

    #[test]
    fn center_stats_isolate_middle_content() {
        // Dark border, bright center: full-frame mean must sit below the
        // center-crop mean.
        let w = 10u32;
        let h = 10u32;
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize * 4;
                let bright = (3..7).contains(&x) && (3..7).contains(&y);
                let v = if bright { 220 } else { 20 };
                px[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let full = app_frame_stats(w, h, &px).expect("valid");
        let center = app_frame_center_stats(w, h, &px, 0.4).expect("valid");
        assert!(
            center.mean_luma > full.mean_luma + 0.3,
            "{full:?} {center:?}"
        );
    }

    #[test]
    fn psnr_identical_is_infinite_and_mismatch_is_none() {
        let a = vec![1u8, 2, 3, 255];
        assert_eq!(frame_psnr(&a, &a), Some(f64::INFINITY));
        assert!(frame_psnr(&a, &[1u8, 2, 3]).is_none());
        assert!(frame_psnr(&[], &[]).is_none());
    }

    #[test]
    fn psnr_single_byte_diff_is_finite() {
        let a = vec![100u8; 64];
        let mut b = a.clone();
        b[0] = 101;
        let psnr = frame_psnr(&a, &b).expect("finite");
        assert!(psnr.is_finite() && psnr > 30.0, "psnr={psnr}");
    }

    #[test]
    fn combined_text_covers_label_and_value() {
        let nodes = vec![
            TreeNode {
                role: "Label".into(),
                label: String::new(),
                value: "Zoom: Custom".into(),
                x0: 0.0,
                y0: 0.0,
                x1: 1.0,
                y1: 1.0,
            },
            TreeNode {
                role: "Button".into(),
                label: "Dismiss".into(),
                value: String::new(),
                x0: 0.0,
                y0: 0.0,
                x1: 1.0,
                y1: 1.0,
            },
        ];
        assert_eq!(texts_containing(&nodes, "Custom"), vec!["Zoom: Custom"]);
        assert_eq!(texts_containing(&nodes, "Dismiss"), vec!["Dismiss"]);
        assert!(texts_containing(&nodes, "missing").is_empty());
    }
}
