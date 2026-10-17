//! MCP-PARITY-C: the synthetic fixtures of the matrix byte-parity suite.
//!
//! The §8 anchor, and deliberately so — every claim in this slice is
//! *transport-side*: same bytes in, same artifact out. The pixels are the
//! established anchor of `tests/matrix_e2e.rs:30-45` (64×48 gradient
//! `r=g=(x*4, y*5, (x+y)*2)`), so a digest measured here is comparable with
//! what the CLI suite already measures. The minimal set follows
//! `matrix_e2e.rs:49-62` (`schema_version 1`, `pipeline_version
//! "raster-mvp-1"`, `comparison_width 48`, one relative sample) with two
//! recipes of different tolerance classes (`exact`/`strict`, `:78-105`).
//!
//! What PNGs do NOT cover is the LibRaw decode path — that is the Real-RAW
//! gate T26, not this file (SOLL §8).

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// §8 anchor geometry: 64×48.
pub const ANCHOR_W: u32 = 64;
/// §8 anchor geometry: 64×48.
pub const ANCHOR_H: u32 = 48;

/// The §8 comparison width of the minimal set.
pub const ANCHOR_COMPARISON_WIDTH: u32 = 48;

/// Writes the anchor sample (`matrix_e2e.rs:30-45` pixels) as `sample.png`.
pub fn anchor_sample(dir: &Path) -> PathBuf {
    let mut pixels = Vec::with_capacity((ANCHOR_W * ANCHOR_H * 4) as usize);
    for y in 0..ANCHOR_H {
        for x in 0..ANCHOR_W {
            pixels.push((x * 4) as u8);
            pixels.push((y * 5) as u8);
            pixels.push(((x + y) * 2) as u8);
            pixels.push(255);
        }
    }
    write_png(dir, "sample.png", ANCHOR_W, ANCHOR_H, &pixels)
}

/// The identity recipe: renders to itself, `exact` tolerance, GPU route
/// declared (`matrix_e2e.rs:78-87`).
pub const IDENTITY_RECIPE: &str = r#"{
    "id": "identity",
    "goals": ["G-01"],
    "stages": ["decode", "output"],
    "tolerance": "exact",
    "expected_route": "gpu",
    "recipe": { "adjustments": {} }
  }"#;

/// The tone recipe: real pixel change, `strict` tolerance, GPU route declared
/// (`matrix_e2e.rs:89-105`).
pub const TONE_RECIPE: &str = r#"{
    "id": "tone",
    "goals": ["G-01"],
    "stages": ["exposure", "contrast", "highlights", "shadows"],
    "tolerance": "strict",
    "expected_route": "gpu",
    "recipe": {
      "adjustments": {
        "exposure": 0.4,
        "contrast": 0.2,
        "highlights": -0.2,
        "shadows": 0.25
      }
    }
  }"#;

/// Writes the minimal §8 recipe set (`schema_version 1`, `raster-mvp-1`,
/// `comparison_width 48`, one relative sample, identity + tone recipes) as
/// `recipe-set.json`. `recipes` is raw JSON for the `"recipes"` array, so a
/// test can vary tolerance/route content per case.
pub fn write_recipe_set(dir: &Path, recipes: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join("recipe-set.json");
    let json = format!(
        r#"{{
  "schema_version": 1,
  "pipeline_version": "raster-mvp-1",
  "comparison_width": {ANCHOR_COMPARISON_WIDTH},
  "samples": [{{ "id": "sample", "path": "sample.png" }}],
  "recipes": [{recipes}]
}}"#
    );
    fs::write(&path, json).unwrap();
    path
}

/// The default anchor set: identity + tone.
pub fn anchor_set(dir: &Path) -> PathBuf {
    write_recipe_set(dir, &format!("{IDENTITY_RECIPE},\n{TONE_RECIPE}"))
}

/// The golden path convention `<golden-dir>/sample__<recipe>.png`
/// (SOLL §3: `sample`/`recipe` agree with the golden name).
pub fn golden_path(golden_dir: &Path, recipe: &str) -> PathBuf {
    golden_dir.join(format!("sample__{recipe}.png"))
}

/// Builds one full anchor tree: sample + default set. Returns the set path.
pub fn anchor_tree(dir: &Path) -> PathBuf {
    anchor_sample(dir);
    anchor_set(dir)
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

/// `blake3:<64 hex>` of the file bytes.
pub fn blake3_file(path: &Path) -> String {
    format!("blake3:{}", blake3::hash(&bytes(path)).to_hex())
}

/// Replaces the golden with its pixel-inverted self (the
/// `matrix_e2e.rs:195-213` tolerance-violation path): the gate must fail, not
/// silently pass. Applied to the same-named file in whichever tree is
/// passed, so both transports see the identical violation.
pub fn invert_golden(golden: &Path) {
    let raw = bytes(golden);
    let mut frame = lumina_core::ImageFrame::decode(&raw).unwrap();
    for (index, byte) in frame.pixels_mut().iter_mut().enumerate() {
        if index % 4 != 3 {
            *byte = 255 - *byte;
        }
    }
    fs::write(
        golden,
        frame.encode(lumina_core::ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
}
