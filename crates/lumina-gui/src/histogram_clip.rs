//! Shadow/highlight clipping fractions and normalized histogram L1 distance:
//! pure image-statistics math for display diagnostics (the `J` clipping badge
//! and the G-10 "Original Photo" histogram delta).
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 8, file-size ratchet).
//! Pure functions over frames/histogram bins; no GUI behaviour change.

use super::*;

/// Shadow/highlight clipping fractions (`0..=1`) of a frame: a pixel counts
/// as shadow-clipped when all channels are `0`, as highlight-clipped when
/// all are `255`. Pure display diagnostic for the `J` overlay badge — it
/// never feeds back into the recipe or render. Empty frames report `(0, 0)`.
pub fn clip_fractions(frame: &ImageFrame) -> (f64, f64) {
    let total = (frame.width as usize) * (frame.height as usize);
    if total == 0 || frame.pixels.len() < total * 4 {
        return (0.0, 0.0);
    }
    let mut shadow = 0usize;
    let mut highlight = 0usize;
    for px in frame.pixels.as_chunks::<4>().0 {
        if px[0] == 0 && px[1] == 0 && px[2] == 0 {
            shadow += 1;
        } else if px[0] == 255 && px[1] == 255 && px[2] == 255 {
            highlight += 1;
        }
    }
    (
        shadow as f64 / total as f64,
        highlight as f64 / total as f64,
    )
}

/// Normalized L1 distance of two equal-length histograms (`0` = identical
/// distributions, `2` = disjoint). Scale-free so a full-res preview and the
/// unedited decode compare directly. Returns `None` on length mismatch or
/// empty inputs — never a silent `0`. Pure function, unit-tested headless;
/// feeds the G-10 "Original Photo" histogram delta.
pub fn normalized_histogram_l1(a: &[u64], b: &[u64]) -> Option<f64> {
    if a.len() != b.len() {
        return None;
    }
    let sum_a: u64 = a.iter().sum();
    let sum_b: u64 = b.iter().sum();
    if sum_a == 0 || sum_b == 0 {
        return None;
    }
    Some(
        a.iter()
            .zip(b.iter())
            .map(|(&x, &y)| (x as f64 / sum_a as f64 - y as f64 / sum_b as f64).abs())
            .sum(),
    )
}
