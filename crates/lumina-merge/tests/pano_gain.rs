//! Slice A (gain compensation): estimation exactness, outlier robustness,
//! loud limits and gain-corrected blending (A-T1–A-T4 plus the blend
//! validation), through the public API only.
//!
//! Extracted from `src/merge.rs` `#[cfg(test)]` (file-size ratchet); the
//! assertions are unchanged — a stub estimator or a clamping blend fails
//! them exactly as before.

use lumina_merge::{
    blend_panorama, blend_panorama_transformed, blend_panorama_transformed_with_gains,
    blend_panorama_with_gains, estimate_pano_gains, LinearImage, MergeError,
};

/// Translation matrix placing a frame's origin at `(ox, oy)` in canvas
/// coordinates (the matrix spelling of `blend_panorama` offsets).
fn translation_matrix(ox: f64, oy: f64) -> [f64; 9] {
    [1.0, 0.0, ox, 0.0, 1.0, oy, 0.0, 0.0, 1.0]
}

/// Horizontal ramp value at column `x`: `0.1 + 0.8 * x / (w - 1)`.
fn ramp_value(x: u32, w: u32) -> f32 {
    0.1 + 0.8 * x as f32 / (w - 1) as f32
}

/// Horizontal ramp fixture: `v = ramp_value(x)`, all channels.
/// Non-constant, so the median works over a real distribution, not a
/// single repeated value.
fn ramp_frame(w: u32, h: u32, scale: f32) -> LinearImage {
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..h {
        for x in 0..w {
            let v = ramp_value(x, w) * scale;
            px.extend_from_slice(&[v, v, v]);
        }
    }
    LinearImage::new(w, h, px).unwrap()
}

/// Shifted copy for translation `(ox, 0)`: `B(sx) = scale * A(sx + ox)`
/// (clamped at the right edge), i.e. B shows — scaled — what A shows
/// `ox` px to the right. In the canvas overlap both frames then sample
/// the same content, so the gain-corrected blend is seamless for *any*
/// blend weights (a convex combination of equal values).
fn ramp_frame_shifted(w: u32, h: u32, ox: u32, scale: f32) -> LinearImage {
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..h {
        for x in 0..w {
            let v = ramp_value((x + ox).min(w - 1), w) * scale;
            px.extend_from_slice(&[v, v, v]);
        }
    }
    LinearImage::new(w, h, px).unwrap()
}

/// A-T1: exact gains — frame B = frame A x 2.0 in the overlap gives
/// `gains == [1.0, 0.5]` (+-1e-6); identical frames give `[1.0, 1.0]`.
/// A stub returning `[1.0, 1.0]` fails the first assertion.
#[test]
fn pano_gain_estimate_recovers_exact_ratio() {
    // 64x32, offset (8, 0): overlap 56x32 = 1792px >= A_MIN (1024).
    let (w, h) = (64u32, 32u32);
    let matrices = [translation_matrix(0.0, 0.0), translation_matrix(8.0, 0.0)];
    // Solid pair (shift-invariant): exact ratios everywhere.
    let a = LinearImage::solid(w, h, [0.4, 0.4, 0.4]);
    let b = LinearImage::solid(w, h, [0.8, 0.8, 0.8]);
    let estimate = estimate_pano_gains(&[a.clone(), b], &matrices).unwrap();
    assert_eq!(estimate.gains.len(), 2);
    assert_eq!(estimate.gains[0], 1.0);
    assert!(
        (estimate.gains[1] - 0.5).abs() < 1e-6,
        "B = A x 2.0 must give gain 0.5, got {}",
        estimate.gains[1]
    );
    assert_eq!(estimate.min_overlap_px, 56 * 32);

    let b_same = LinearImage::solid(w, h, [0.4, 0.4, 0.4]);
    let estimate = estimate_pano_gains(&[a, b_same], &matrices).unwrap();
    assert!(
        (estimate.gains[1] - 1.0).abs() < 1e-6,
        "identical frames must give gain 1.0, got {}",
        estimate.gains[1]
    );

    // Ramp pair (real distribution, shifted copy): every overlap ratio
    // is exactly 0.5, so the median is exact too.
    let a = ramp_frame(w, h, 1.0);
    let b = ramp_frame_shifted(w, h, 8, 2.0);
    let estimate = estimate_pano_gains(&[a, b], &matrices).unwrap();
    assert!(
        (estimate.gains[1] - 0.5).abs() < 1e-6,
        "shifted ramp B = A x 2.0 must give gain 0.5, got {}",
        estimate.gains[1]
    );
}

/// A-T1 (application): the gain-corrected blend of B == A x 2.0 (shifted
/// copy) is seamless — every pixel covered by A equals the reference
/// brightness, and the B-only region (seed path) is scaled too.
#[test]
fn pano_gain_blend_is_seamless_after_compensation() {
    let (w, h) = (64u32, 32u32);
    let a = ramp_frame(w, h, 1.0);
    let b = ramp_frame_shifted(w, h, 8, 2.0);
    let gains = [1.0, 0.5];
    for blend in [4u32, 0u32] {
        // Translation path.
        let out =
            blend_panorama_with_gains(&[a.clone(), b.clone()], &[(0, 0), (8, 0)], blend, &gains)
                .unwrap();
        // Canvas x in [8, 64) is covered by A: must equal A exactly.
        for y in 0..h {
            for x in 8..w {
                let got = out.pixels()[((y * out.width() + x) * 3) as usize];
                let want = a.pixels()[((y * w + x) * 3) as usize];
                assert!(
                    (got - want).abs() < 1e-5,
                    "translation blend={blend} ({x},{y}): got {got}, want {want}"
                );
            }
        }
        // Transformed path (translation matrices): same pixels.
        let matrices = [translation_matrix(0.0, 0.0), translation_matrix(8.0, 0.0)];
        let out_t = blend_panorama_transformed_with_gains(
            &[a.clone(), b.clone()],
            &matrices,
            blend,
            &gains,
        )
        .unwrap();
        assert_eq!((out_t.width(), out_t.height()), (out.width(), out.height()));
        for (got, want) in out_t.pixels().iter().zip(out.pixels()) {
            assert!(
                (got - want).abs() < 1e-5,
                "transformed path must match the translation path, got {got}, want {want}"
            );
        }
    }
}

/// A-T2: outlier robustness — ~11 % of the overlap pixels carry a dark
/// ghost block, yet the median gain drifts < 1 % from the clean value.
/// The mean over a comparable mixture drifts an order of magnitude more
/// (negative control computed inline: a mean-based estimator fails it).
#[test]
fn pano_gain_median_resists_ghost_outliers() {
    let (w, h) = (64u32, 32u32);
    let a = ramp_frame(w, h, 1.0);
    // B = shifted A x 2.0, except a dark ghost block over B columns 0..6
    // (6 of 56 overlap columns ~ 10.7 %, full height, value 0.08).
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..h {
        for x in 0..w {
            let v = if (0..6).contains(&x) {
                0.08
            } else {
                ramp_value((x + 8).min(w - 1), w) * 2.0
            };
            px.extend_from_slice(&[v, v, v]);
        }
    }
    let b = LinearImage::new(w, h, px).unwrap();
    let matrices = [translation_matrix(0.0, 0.0), translation_matrix(8.0, 0.0)];
    let estimate = estimate_pano_gains(&[a, b], &matrices).unwrap();
    assert!(
        (estimate.gains[1] - 0.5).abs() / 0.5 < 0.01,
        "median gain must drift < 1 % from 0.5 despite the ghost, got {}",
        estimate.gains[1]
    );
    // Negative control: an arithmetic mean over the same mixture — clean
    // ratios ~0.5 on ~89 % of the overlap, ghost ratios ~2.5..3.3 on the
    // rest (representative 2.0 below) — drifts far outside the 1 % bound a
    // mean estimator would need.
    let clean = 0.5f64;
    let ghost = 2.0f64;
    let frac = 6.0f64 / 56.0f64;
    let mean = (1.0 - frac) * clean + frac * ghost;
    assert!(
        (mean - clean).abs() / clean > 0.05,
        "control: the mean must drift > 5 % (it does: {mean}), else this test proves nothing"
    );
}

/// A-T3: overlap below `A_MIN_OVERLAP_STAT_PX` is `Unsupported`, and the
/// text carries both pixel counts.
#[test]
fn pano_gain_small_overlap_is_unsupported() {
    let a = LinearImage::solid(8, 8, [0.4, 0.4, 0.4]);
    let b = LinearImage::solid(8, 8, [0.8, 0.8, 0.8]);
    let matrices = [translation_matrix(0.0, 0.0), translation_matrix(7.0, 0.0)];
    let err = estimate_pano_gains(&[a, b], &matrices).unwrap_err();
    assert!(
        matches!(err, MergeError::Unsupported(_)),
        "tiny overlap must be unsupported, got: {err}"
    );
    let text = err.to_string();
    assert!(
        text.contains("got 8 (frame #1"),
        "text must carry the measured pixels: {text}"
    );
    assert!(text.contains("1024"), "text must carry the minimum: {text}");
}

/// A-T4: a 16x brightness ratio exceeds `MAX_PANO_GAIN` — `Unsupported`,
/// never a clamped value. The test asserts the error, never pixels.
#[test]
fn pano_gain_beyond_max_is_unsupported_never_clamped() {
    let (w, h) = (64u32, 32u32);
    // Reference 16x brighter than frame 1: gain 16 > MAX_PANO_GAIN.
    let a = LinearImage::solid(w, h, [0.8, 0.8, 0.8]);
    let b = LinearImage::solid(w, h, [0.05, 0.05, 0.05]);
    let matrices = [translation_matrix(0.0, 0.0), translation_matrix(8.0, 0.0)];
    let err = estimate_pano_gains(&[a, b], &matrices).unwrap_err();
    assert!(
        matches!(err, MergeError::Unsupported(_)),
        "16x gain must be unsupported, got: {err}"
    );
    assert!(
        err.to_string().contains("pano gain 16"),
        "16x gain must be named in the error: {err}"
    );
    assert!(
        err.to_string().contains("exceeds 8"),
        "the MAX must be named in the error: {err}"
    );
}

/// Gain validation at the blend boundary: length, reference and
/// positivity are `Invalid` (never padded or clamped).
#[test]
fn pano_blend_gains_are_validated_loudly() {
    let a = LinearImage::solid(64, 32, [0.4, 0.4, 0.4]);
    let b = LinearImage::solid(64, 32, [0.8, 0.8, 0.8]);
    let offsets = [(0, 0), (8, 0)];
    let matrices = [translation_matrix(0.0, 0.0), translation_matrix(8.0, 0.0)];
    for gains in [
        vec![1.0],                // len mismatch
        vec![1.0, 0.5, 0.5],      // len mismatch
        vec![0.5, 0.5],           // gains[0] != 1.0
        vec![1.0, 0.0],           // not positive
        vec![1.0, f64::NAN],      // not finite
        vec![1.0, f64::INFINITY], // not finite
    ] {
        for err in [
            blend_panorama_with_gains(&[a.clone(), b.clone()], &offsets, 4, &gains).unwrap_err(),
            blend_panorama_transformed_with_gains(&[a.clone(), b.clone()], &matrices, 4, &gains)
                .unwrap_err(),
        ] {
            assert!(
                matches!(err, MergeError::Invalid(_)),
                "gains {gains:?} must be Invalid, got: {err}"
            );
        }
    }
    // `None` (plain blend) still works — the no-gain path is untouched.
    blend_panorama(&[a.clone(), b.clone()], &offsets, 4).unwrap();
    blend_panorama_transformed(&[a, b], &matrices, 4).unwrap();
}
