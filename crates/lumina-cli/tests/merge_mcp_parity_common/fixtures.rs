//! MCP-PARITY-C: the synthetic fixtures of the byte-parity suite.
//!
//! PNG mocks, and deliberately so — every claim in this slice is
//! *transport-side*: same bytes in, same artifact out. The pixels are the
//! established anchors of the merge suites, byte for byte, so a digest measured
//! here is comparable with the one `tests/merge_e2e.rs` and
//! `tests/merge_parity.rs` already measure:
//!
//! * the §9.1 acceptance anchor, 64×48 grayscale `level + x % 16` at levels 40
//!   and 160 with `--exposure-times 0.004,0.016 --isos 100,100 --f-numbers 8,8`
//!   (`tests/merge_parity.rs:34-49`). Its alignment residual is above the
//!   documented threshold, so this fixture also exercises the `warning` field.
//! * the smallest HDR-viable pair, 32×24 non-periodic gradient (`tests/
//!   merge_e2e.rs:20-21,26-57`), which aligns cleanly (no warning).
//! * the panorama pair: the same gradient, second frame shifted 4 px, so the
//!   estimator has a real offset to find (`tests/merge_e2e.rs:286-321`).
//!
//! What PNGs do NOT cover is the LibRaw decode path, RAW EXIF exposure and RAW
//! orientation — that is the Real-RAW gate T21, not this file (SOLL §7, §8).

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// §9.1 acceptance anchor geometry: 64×48 grayscale gradient.
pub const ANCHOR_W: u32 = 64;
pub const ANCHOR_H: u32 = 48;

/// The smallest HDR-viable fixture geometry of `tests/merge_e2e.rs:20-21`.
pub const GRADIENT_W: u32 = 32;
pub const GRADIENT_H: u32 = 24;

/// §9.1 acceptance anchor: two 64×48 grayscale gradients at levels 40 and 160
/// with explicit exposition, in merge order (reference first). Byte-identical to
/// the fixture `tests/merge_parity.rs:74-81` runs through the CLI binary.
pub fn anchor_pair(dir: &Path) -> Vec<PathBuf> {
    vec![anchor_png(dir, "a.png", 40), anchor_png(dir, "b.png", 160)]
}

/// The anchor frame itself: `level + x % 16`, 8-bit exact so both transports
/// decode exactly these pixels (`tests/merge_parity.rs:34-49`).
pub fn anchor_png(dir: &Path, name: &str, level: u8) -> PathBuf {
    let pixels: Vec<u8> = (0..ANCHOR_W * ANCHOR_H)
        .flat_map(|i| {
            let x = (i % ANCHOR_W) as u8;
            let value = level.saturating_add(x % 16);
            [value, value, value, 255]
        })
        .collect();
    write_png(dir, name, ANCHOR_W, ANCHOR_H, &pixels)
}

/// The smallest HDR-viable pair: the 32×24 gradient scaled 1.0 and 0.5
/// (`tests/merge_e2e.rs:204-205`). Its alignment is clean, so a run on this pair
/// carries no `warning` field.
pub fn hdr_pair(dir: &Path) -> Vec<PathBuf> {
    vec![
        gradient_png(dir, "a.png", 1.0, 0),
        gradient_png(dir, "b.png", 0.5, 0),
    ]
}

/// The panorama pair: the same gradient with the second frame shifted 4 px
/// right (`tests/merge_e2e.rs:289-290`), so the canvas grows by the estimated
/// offset and the side-by-side blend is visible in the artifact bytes.
pub fn pano_pair(dir: &Path) -> Vec<PathBuf> {
    vec![
        gradient_png(dir, "a.png", 1.0, 0),
        gradient_png(dir, "b.png", 1.0, 4),
    ]
}

/// Non-periodic gradient fixture `v = (x * 0.7 + y * 3.1) / 128` scaled by
/// `scale` and shifted right by `shift_x` (vacated columns stay black). The
/// non-periodicity is what gives alignment a unique minimum
/// (`tests/merge_e2e.rs:26-57`).
pub fn gradient_png(dir: &Path, name: &str, scale: f32, shift_x: u32) -> PathBuf {
    let (w, h) = (GRADIENT_W, GRADIENT_H);
    let mut pixels = Vec::with_capacity((w * h) as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let sx = x.saturating_sub(shift_x);
            let v = if x < shift_x && shift_x > 0 {
                0.0
            } else {
                (sx as f32 * 0.7 + y as f32 * 3.1) / 128.0 * scale
            };
            let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.extend_from_slice(&[b, b, b, 255]);
        }
    }
    write_png(dir, name, w, h, &pixels)
}

/// A flat frame of the given geometry: the dimension mismatch and the
/// undersized-writer-limit fixtures (`tests/merge_e2e.rs:477-527`).
pub fn solid_png(dir: &Path, name: &str, w: u32, h: u32, value: u8) -> PathBuf {
    write_png(
        dir,
        name,
        w,
        h,
        &[value, value, value, 255].repeat((w * h) as usize),
    )
}

fn write_png(dir: &Path, name: &str, w: u32, h: u32, pixels: &[u8]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let png = lumina_core::ImageFrame::new(w, h, pixels.to_vec())
        .unwrap()
        .encode(lumina_core::ImageFileFormat::Png)
        .unwrap();
    fs::write(&path, png).unwrap();
    path
}

// ------------------------------------------------------------- byte helpers

pub fn bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("cannot read `{}`: {error}", path.display()))
}

/// `blake3:<64 hex>` of the file bytes — the checksum contract of §5
/// (`tests/merge_e2e.rs:152-154`).
pub fn blake3_file(path: &Path) -> String {
    format!("blake3:{}", blake3::hash(&bytes(path)).to_hex())
}
