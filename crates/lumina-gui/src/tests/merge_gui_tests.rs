//! Slice B (deghosting): the merge fixtures of the GUI headless suite,
//! extracted from `merge_gui.rs` (file-size ratchet).
//!
//! Reached through `#[cfg(test)] #[path = "tests/merge_gui_tests.rs"] mod
//! merge_gui_tests;` in `merge_gui.rs`, so `check_test_orphans.sh` sees it in
//! the crate's dep-info like every other file under `src/tests/`. The move is
//! verbatim: nothing here changed when the `deghost` parameter arrived on
//! `run_merge_sync`, and the extraction is what keeps `merge_gui.rs` under its
//! size entry despite the extra call-site argument.

#![allow(dead_code)]
// Every helper is used by at least one test in `merge_gui::tests`; the file
// itself carries no test, so a helper nobody calls yet would be dead code.

use crate::LuminaApp;
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::MergeExposure;
use std::path::Path;

/// A fresh app (the headless harness context).
pub fn new_app() -> LuminaApp {
    LuminaApp::new(crate::egui::Context::default())
}

/// Synthetic 64×48 RGB frame encoded as PNG (brightness `level`). The
/// gradient gives the alignment a non-flat signal.
pub fn write_png(path: &Path, level: u8) {
    let pixels: Vec<u8> = (0..64 * 48)
        .flat_map(|i| {
            let x = (i % 64) as u8;
            let value = level.saturating_add(x % 16);
            [value, value, value, 255]
        })
        .collect();
    let png = ImageFrame::new(64, 48, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(path, png).unwrap();
}

/// The 1x/4x bracket pair the HDR fixtures merge with.
pub fn exposure(t: f64) -> Vec<MergeExposure> {
    vec![
        MergeExposure {
            exposure_time_s: t,
            iso: 100,
            f_number: 8.0,
        },
        MergeExposure {
            exposure_time_s: t * 4.0,
            iso: 100,
            f_number: 8.0,
        },
    ]
}
