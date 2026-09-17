//! LRPAR-G14-REDEYE-AUTO-15: deterministic, model-free automatic red-pupil
//! detection (Release 2.0).
//!
//! SOLL: `feature/architecture/pipeline.md` § G-14, Abschnitt „Automatische
//! Pupillen-Erkennung". This module is **not** a render stage: it analyses the
//! decoded RGBA8 source pixels and proposes [`lumina_sidecar::RedEyeRegion`]s
//! that the caller persists explicitly into `recipe.adjustments.red_eye`
//! (CLI `red-eye --detect`/`--detect-apply`, GUI „Detect pupils"). Detection is
//! never implicit and never a silent prefill.
//!
//! Pure arithmetic: no FS/IO, no randomness, no ONNX. The same frame yields
//! byte-identical regions. The only coupling to the correction stage is the
//! shared "redness" definition
//! `redness = clamp((R - max(G, B)) / max(R, ε), 0, 1)` used by
//! `apply_red_eye`; the correction itself is untouched and stays GPU-parity
//! capable.

use crate::ImageFrame;
use lumina_sidecar::{RedEyeCorrection, RedEyeRegion};
use std::collections::BTreeSet;

/// Minimum per-pixel red dominance (`0..=1`) for a pixel to count as red.
/// Pixels below this threshold are never part of a candidate component.
pub const RED_EYE_DETECT_REDNESS_THRESHOLD: f32 = 0.5;

/// Minimum number of connected red pixels for a candidate. Smaller blobs are
/// treated as pixel noise and dropped.
pub const RED_EYE_DETECT_MIN_PIXELS: usize = 4;

/// Upper bound on the enclosing component radius (normalized to
/// `min(width, height)`) that still counts as a pupil. Larger red areas
/// (lips, clothing, red props) are rejected instead of being reported as eyes.
pub const RED_EYE_DETECT_MAX_RADIUS: f32 = 0.1;

/// Margin applied to the enclosing component radius for the persisted
/// `radius`: the correction feathers out to the region edge, so a pupil is
/// covered with a small documented margin.
pub const RED_EYE_DETECT_RADIUS_MARGIN: f32 = 1.25;

/// Default `desaturate` for automatically detected regions (same as the
/// GUI picker default).
pub const RED_EYE_DETECT_DEFAULT_DESATURATE: f32 = 0.8;

/// Default `darken` for automatically detected regions (same as the GUI
/// picker default).
pub const RED_EYE_DETECT_DEFAULT_DARKEN: f32 = 0.4;

/// Upper bound on the persisted region count, re-exported from the sidecar
/// schema so detection and validation cannot drift apart.
pub const RED_EYE_DETECT_MAX_REGIONS: usize = lumina_sidecar::RED_EYE_MAX_REGIONS;

/// Stable id prefix for automatically detected regions. `--detect-apply`
/// replaces exactly these and leaves manually marked regions untouched.
pub const RED_EYE_DETECT_ID_PREFIX: &str = "auto-re-";

/// One deterministic red-pupil candidate on the decoded source frame.
///
/// `x`/`y`/`radius` are source-normalized like the persisted
/// [`lumina_sidecar::RedEyeRegion`]; `confidence` is the mean red dominance of
/// the connected component in `0..=1` and is diagnostic only (it is not part
/// of the recipe schema).
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedRedEye {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub confidence: f32,
}

impl DetectedRedEye {
    /// Converts the candidate into a persistable region with the documented
    /// default strengths.
    #[must_use]
    pub fn to_region(&self) -> RedEyeRegion {
        RedEyeRegion {
            id: self.id.clone(),
            x: self.x,
            y: self.y,
            radius: self.radius,
            desaturate: RED_EYE_DETECT_DEFAULT_DESATURATE,
            darken: RED_EYE_DETECT_DEFAULT_DARKEN,
        }
    }
}

/// Result of one detection pass. `candidates` is sorted by descending
/// confidence (then `y`, then `x`) and already capped to
/// [`RED_EYE_DETECT_MAX_REGIONS`]; `dropped` reports how many findings the cap
/// removed so callers can surface it loudly instead of silently truncating.
#[derive(Debug, Clone, PartialEq)]
pub struct RedEyeDetection {
    pub candidates: Vec<DetectedRedEye>,
    pub dropped: usize,
}

impl RedEyeDetection {
    /// Wraps the candidates into a `red_eye` recipe stage (version 1).
    #[must_use]
    pub fn correction(&self) -> RedEyeCorrection {
        RedEyeCorrection {
            version: 1,
            regions: self
                .candidates
                .iter()
                .map(DetectedRedEye::to_region)
                .collect(),
        }
    }
}

struct RawCandidate {
    x: f32,
    y: f32,
    radius: f32,
    confidence: f32,
}

/// Deterministic red-pupil detection over decoded RGBA8 pixels.
///
/// See the module docs / `pipeline.md` § G-14 for the normative algorithm and
/// thresholds. Empty frames and frames without red pixels return an empty
/// candidate list.
#[must_use]
pub fn detect_red_eyes(frame: &ImageFrame) -> RedEyeDetection {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 || frame.pixels.len() < w * h * 4 {
        return RedEyeDetection {
            candidates: Vec::new(),
            dropped: 0,
        };
    }
    let min_dim = w.min(h) as f64;
    // Redness is recomputed on demand instead of materializing a full-frame
    // `f32` buffer: the pass stays O(1) in extra memory beyond the visited
    // bitmap, which matters for full-resolution RAW sources.
    let redness_at = |index: usize| -> f32 {
        let px = &frame.pixels[index * 4..index * 4 + 4];
        let r = f32::from(px[0]) / 255.0;
        let g = f32::from(px[1]) / 255.0;
        let b = f32::from(px[2]) / 255.0;
        ((r - g.max(b)) / r.max(1e-3)).clamp(0.0, 1.0)
    };
    let is_red = |index: usize| redness_at(index) >= RED_EYE_DETECT_REDNESS_THRESHOLD;

    let mut visited = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let mut raw: Vec<RawCandidate> = Vec::new();
    for start in 0..w * h {
        if visited[start] || !is_red(start) {
            continue;
        }
        visited[start] = true;
        stack.clear();
        stack.push(start);
        let mut members: Vec<usize> = Vec::new();
        let mut sum_x = 0f64;
        let mut sum_y = 0f64;
        let mut sum_red = 0f64;
        while let Some(index) = stack.pop() {
            let x = index % w;
            let y = index / w;
            members.push(index);
            sum_x += x as f64 + 0.5;
            sum_y += y as f64 + 0.5;
            sum_red += f64::from(redness_at(index));
            if x > 0 && !visited[index - 1] && is_red(index - 1) {
                visited[index - 1] = true;
                stack.push(index - 1);
            }
            if x + 1 < w && !visited[index + 1] && is_red(index + 1) {
                visited[index + 1] = true;
                stack.push(index + 1);
            }
            if y > 0 && !visited[index - w] && is_red(index - w) {
                visited[index - w] = true;
                stack.push(index - w);
            }
            if y + 1 < h && !visited[index + w] && is_red(index + w) {
                visited[index + w] = true;
                stack.push(index + w);
            }
        }
        if members.len() < RED_EYE_DETECT_MIN_PIXELS {
            continue;
        }
        let count = members.len() as f64;
        let cx = sum_x / count;
        let cy = sum_y / count;
        let mut enclosing = 0f64;
        for &index in &members {
            let dx = (index % w) as f64 + 0.5 - cx;
            let dy = (index / w) as f64 + 0.5 - cy;
            enclosing = enclosing.max(dx.hypot(dy));
        }
        let enclosing_norm = (enclosing / min_dim) as f32;
        if !enclosing_norm.is_finite() || enclosing_norm <= 0.0 {
            continue;
        }
        if enclosing_norm > RED_EYE_DETECT_MAX_RADIUS {
            continue;
        }
        raw.push(RawCandidate {
            x: (cx / w as f64) as f32,
            y: (cy / h as f64) as f32,
            radius: (enclosing_norm * RED_EYE_DETECT_RADIUS_MARGIN).clamp(1e-4, 1.0),
            confidence: (sum_red / count) as f32,
        });
    }

    // Deterministic ranking: strongest red dominance first, ties by position.
    raw.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.y.total_cmp(&b.y))
            .then_with(|| a.x.total_cmp(&b.x))
    });
    let total = raw.len();
    raw.truncate(RED_EYE_DETECT_MAX_REGIONS);
    let dropped = total - raw.len();

    let mut used: BTreeSet<String> = BTreeSet::new();
    let candidates = raw
        .into_iter()
        .map(|candidate| {
            let id = stable_id(candidate.x, candidate.y, candidate.radius, &mut used);
            DetectedRedEye {
                id,
                x: candidate.x,
                y: candidate.y,
                radius: candidate.radius,
                confidence: candidate.confidence,
            }
        })
        .collect();
    RedEyeDetection {
        candidates,
        dropped,
    }
}

/// Content-stable id for a candidate, derived from the quantized geometry so
/// the same source always yields the same ids. Collisions (practically
/// impossible) get a deterministic numeric suffix instead of a random value.
fn stable_id(x: f32, y: f32, radius: f32, used: &mut BTreeSet<String>) -> String {
    let seed = format!("{x:.6},{y:.6},{radius:.6}");
    let digest = blake3::hash(seed.as_bytes()).to_hex();
    let base = format!("{RED_EYE_DETECT_ID_PREFIX}{}", &digest[..8]);
    if used.insert(base.clone()) {
        return base;
    }
    let mut suffix = 1u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Builds `width × height` RGBA8 with a solid `[r,g,b,a]` background.
    fn frame(width: u32, height: u32, background: [u8; 4]) -> ImageFrame {
        let pixels = background
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect();
        ImageFrame::new(width, height, pixels).unwrap()
    }

    fn set_pixel(frame: &mut ImageFrame, x: u32, y: u32, rgba: [u8; 4]) {
        let index = (y * frame.width + x) as usize * 4;
        frame.pixels[index..index + 4].copy_from_slice(&rgba);
    }

    fn fill_rect(frame: &mut ImageFrame, x0: u32, y0: u32, x1: u32, y1: u32, rgba: [u8; 4]) {
        for y in y0..y1 {
            for x in x0..x1 {
                set_pixel(frame, x, y, rgba);
            }
        }
    }

    /// 24×16 grey frame with one red 3×3 pupil at (10..13, 6..9).
    fn pupil_frame() -> ImageFrame {
        let mut frame = frame(24, 16, [120, 120, 120, 255]);
        fill_rect(&mut frame, 10, 6, 13, 9, [220, 30, 40, 200]);
        frame
    }

    #[test]
    fn detects_one_pupil_deterministically_and_covers_it() {
        let frame = pupil_frame();
        let first = detect_red_eyes(&frame);
        let second = detect_red_eyes(&frame);
        assert_eq!(first, second, "same frame must be byte-identical");
        assert_eq!(first.candidates.len(), 1, "{first:?}");
        assert_eq!(first.dropped, 0);
        let candidate = &first.candidates[0];
        // Centroid of the 3×3 block is the center pixel (11.5, 7.5) in
        // continuous coordinates → normalized (11.5/24, 7.5/16).
        assert!((candidate.x - 11.5 / 24.0).abs() < 1e-5, "{candidate:?}");
        assert!((candidate.y - 7.5 / 16.0).abs() < 1e-5, "{candidate:?}");
        assert!(candidate.radius > 0.0 && candidate.radius <= 1.0);
        assert!(candidate.confidence > 0.7, "{candidate:?}");
        assert!(candidate.id.starts_with(RED_EYE_DETECT_ID_PREFIX));
    }

    #[test]
    fn detection_correction_visibly_corrects_pupil_and_spares_grey() {
        // Golden gate at core level: detection → persisted region →
        // deterministic correction. The red pupil changes, grey is untouched.
        let frame = pupil_frame();
        let detection = detect_red_eyes(&frame);
        assert_eq!(detection.candidates.len(), 1);
        let correction = detection.correction();
        assert_eq!(correction.version, 1);
        assert_eq!(correction.regions.len(), 1);
        assert_eq!(
            correction.regions[0].desaturate,
            RED_EYE_DETECT_DEFAULT_DESATURATE
        );
        assert_eq!(correction.regions[0].darken, RED_EYE_DETECT_DEFAULT_DARKEN);

        let mut corrected = frame.clone();
        corrected
            .apply_recipe(&lumina_sidecar::EditRecipe {
                red_eye: Some(correction),
                ..Default::default()
            })
            .unwrap();
        let pupil = ((7 * 24 + 11) * 4) as usize;
        assert_ne!(
            &corrected.pixels[pupil..pupil + 4],
            &frame.pixels[pupil..pupil + 4],
            "detected pupil must be corrected"
        );
        let grey = 0usize; // top-left pixel (0, 0), outside the region
        assert_eq!(
            &corrected.pixels[grey..grey + 4],
            &frame.pixels[grey..grey + 4],
            "grey pixels outside the region stay untouched"
        );
    }

    #[test]
    fn no_red_pixels_and_empty_frames_yield_no_candidates() {
        for background in [[120, 120, 120, 255], [10, 60, 200, 255], [0, 0, 0, 255]] {
            let detection = detect_red_eyes(&frame(16, 16, background));
            assert!(detection.candidates.is_empty(), "{background:?}");
            assert_eq!(detection.dropped, 0);
        }
        let empty = ImageFrame::new(0, 0, Vec::new()).unwrap();
        assert!(detect_red_eyes(&empty).candidates.is_empty());
    }

    #[test]
    fn below_threshold_is_rejected_and_above_is_detected() {
        // R=100: g=60 → redness 0.4 (below), g=40 → redness 0.6 (above).
        // An exact 0.5 boundary is deliberately not asserted: the shared
        // floating-point redness formula rounds, so the `>=` contract is
        // exercised through clearly separated values instead of a brittle
        // equality. The 64×64 frame keeps the 6×6 blob inside the pupil
        // radius cap.
        let mut below = frame(64, 64, [120, 120, 120, 255]);
        fill_rect(&mut below, 20, 20, 26, 26, [100, 60, 60, 255]);
        assert!(
            detect_red_eyes(&below).candidates.is_empty(),
            "redness below threshold must not produce a candidate"
        );

        let mut above = frame(64, 64, [120, 120, 120, 255]);
        fill_rect(&mut above, 20, 20, 26, 26, [100, 40, 40, 255]);
        assert_eq!(
            detect_red_eyes(&above).candidates.len(),
            1,
            "redness above threshold must be detected"
        );
    }

    #[test]
    fn oversized_red_area_is_rejected() {
        // The whole frame is red: the enclosing radius exceeds the pupil cap.
        let detection = detect_red_eyes(&frame(32, 32, [220, 30, 40, 255]));
        assert!(detection.candidates.is_empty(), "{detection:?}");
    }

    #[test]
    fn small_components_below_min_pixels_are_rejected() {
        // Three red pixels in an L shape (< 4) stay undetected.
        let mut frame = frame(8, 8, [120, 120, 120, 255]);
        set_pixel(&mut frame, 2, 2, [220, 30, 40, 255]);
        set_pixel(&mut frame, 3, 2, [220, 30, 40, 255]);
        set_pixel(&mut frame, 2, 3, [220, 30, 40, 255]);
        assert!(detect_red_eyes(&frame).candidates.is_empty());
    }

    #[test]
    fn more_than_max_candidates_is_capped_and_reported() {
        // 40 separated 2×2 red blobs, pitch 6 px. All have the same redness,
        // so the deterministic tie-break (y, then x) decides which 32 stay.
        let mut frame = frame(64, 48, [120, 120, 120, 255]);
        let mut expected_order: Vec<(u32, u32)> = Vec::new();
        for row in 0..5u32 {
            for col in 0..8u32 {
                let x = col * 8;
                let y = row * 9;
                fill_rect(&mut frame, x, y, x + 2, y + 2, [220, 30, 40, 255]);
                expected_order.push((x, y));
            }
        }
        let detection = detect_red_eyes(&frame);
        assert_eq!(detection.candidates.len(), RED_EYE_DETECT_MAX_REGIONS);
        assert_eq!(detection.dropped, 40 - RED_EYE_DETECT_MAX_REGIONS);
        // Tie-break order is row-major, so the first 32 blobs are kept.
        for (candidate, (x, y)) in detection.candidates.iter().zip(expected_order.iter()) {
            assert!((candidate.x - (*x as f32 + 1.0) / 64.0).abs() < 1e-4);
            assert!((candidate.y - (*y as f32 + 1.0) / 48.0).abs() < 1e-4);
        }
        // Determinism survives the cap.
        assert_eq!(detection, detect_red_eyes(&frame));
    }

    #[test]
    fn candidates_are_sorted_by_descending_confidence() {
        let mut frame = frame(24, 8, [120, 120, 120, 255]);
        // Two blobs with different red dominance: (200,95,40) < (200,20,40).
        fill_rect(&mut frame, 2, 2, 4, 4, [200, 95, 40, 255]);
        fill_rect(&mut frame, 12, 2, 14, 4, [200, 20, 40, 255]);
        let detection = detect_red_eyes(&frame);
        assert_eq!(detection.candidates.len(), 2);
        assert!(detection.candidates[0].confidence > detection.candidates[1].confidence);
        assert!(detection.candidates[0].x > detection.candidates[1].x);
    }

    proptest! {
        /// Property: stronger red dominance (same geometry, lower green) never
        /// yields a lower confidence.
        #[test]
        fn stronger_red_dominance_never_lowers_confidence(
            grey_a in 0u8..=100,
            delta in 1u8..=20,
        ) {
            // R=255 and grey <= 120 keeps both blobs above the 0.5 threshold;
            // the 64×64 frame keeps the 6×6 blob inside the pupil radius cap.
            let grey_b = grey_a.saturating_add(delta).min(120);
            prop_assume!(grey_b > grey_a);
            let mut strong = frame(64, 64, [120, 120, 120, 255]);
            fill_rect(&mut strong, 20, 20, 26, 26, [255, grey_a, 40, 255]);
            let mut weak = frame(64, 64, [120, 120, 120, 255]);
            fill_rect(&mut weak, 20, 20, 26, 26, [255, grey_b, 40, 255]);
            let strong = detect_red_eyes(&strong);
            let weak = detect_red_eyes(&weak);
            prop_assert_eq!(strong.candidates.len(), 1);
            prop_assert_eq!(weak.candidates.len(), 1);
            prop_assert!(
                strong.candidates[0].confidence >= weak.candidates[0].confidence
            );
        }
    }
}
