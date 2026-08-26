//! Explicit 256-bin luminance histogram over the raster MVP measurement domain.
//!
//! Domain (identical to [`crate::tone::analyze_tone`]): sRGB-encoded RGBA8
//! channel values, luminance defined as
//! `(0.2126 * r + 0.7152 * g + 0.0722 * b) / 255` in `0..=1` with Rec.709
//! weights. Alpha is ignored, so every pixel — even fully transparent ones —
//! contributes its RGB sample.
//!
//! Bin `i` covers `[i/256, (i+1)/256)`; exactly `1.0` falls into the last bin
//! (index 255).
//!
//! # Quantile and mean accuracy
//!
//! Quantiles use linear interpolation over the cumulative distribution
//! (uniform-within-bin placement, cumulative counts at the bin edges) with the
//! same rank positions as `analyze_tone` (`position = q * (n - 1)`). For
//! densely populated histograms — where the enclosing sample pair lies inside
//! one populated bin or in immediately adjacent bins — the deviation from
//! `analyze_tone` is bounded by one bin width, `1/256`. Sparse images whose
//! enclosing samples span several empty bins can deviate further, so `1/256`
//! is the documented accuracy of the consistency tests below (and typical for
//! realistic, densely populated image histograms), not a universal worst case.
//! The bin-center [`LuminanceHistogram::mean`] deviates by at most half a bin
//! width, `1/512`.
//!
//! The type is `Serialize`/`Deserialize` and exposes a stable blake3
//! [`LuminanceHistogram::digest`] over bins and frame dimensions, making it
//! directly usable as the value stored under `CacheStage::Histogram`.
//!
//! Since R2-PERF-01 the binning loop itself lives in the shared single-pass
//! kernel [`accumulate_bins_and_luminance_sum`], which is also the foundation
//! of [`crate::tone::analyze_tone`]: histogram panel and tone panel derive
//! their numbers from the very same pass over the pixels.

use crate::{CoreError, ImageFrame};
use serde::{Deserialize, Serialize};

/// Number of luminance bins and, simultaneously, the inverse of the bin width.
pub(crate) const BIN_COUNT: usize = 256;
pub(crate) const BIN_WIDTH: f64 = 1.0 / BIN_COUNT as f64;
/// Rec.709 (BT.709) luminance weights for the sRGB-encoded MVP domain.
const REC709_WEIGHTS: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Explicit histogram of the per-pixel Rec.709 luminance of an [`ImageFrame`].
///
/// All statistics are derived from the bins alone, so the serialized form is
/// the complete representation and `PartialEq` compares exactly the persisted
/// state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuminanceHistogram {
    /// Frame width. Part of the identity so that identical pixel data in a
    /// different geometry produces a different histogram digest.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Per-bin sample counts; bin `i` covers `[i/256, (i+1)/256)`.
    ///
    /// `Vec<u64>` (always constructed with exactly [`BIN_COUNT`] entries) is
    /// used because serde's `Deserialize` only covers arrays up to length 32;
    /// the task scope explicitly permits this alternative.
    pub bins: Vec<u64>,
}

/// Shared single-pass kernel behind [`LuminanceHistogram::new`] and
/// [`crate::tone::analyze_tone`] (R2-PERF-01).
///
/// One iteration over the RGBA8 pixels computes BOTH the 256-bin luminance
/// histogram AND the exact pixel-order f64 luminance sum, so consumers that
/// need histogram and tone statistics no longer pay two passes — and the tone
/// analysis no longer allocates one `f64` per pixel nor sorts (the historical
/// implementation held ~8 bytes per pixel on the heap and ran O(n log n)).
///
/// The per-pixel expression is written exactly like
/// [`crate::tone::luminance_of`], and the sum accumulates sequentially in
/// pixel order starting from `0.0` — the very same operations as
/// [`crate::tone::mean_luminance`]. The returned sum is therefore bit-identical
/// to that measurement (guaranteed by a unit test in `tone.rs`). The binning
/// decision per pixel is identical to the pre-refactor inline loop of
/// [`LuminanceHistogram::new`], so persisted histograms and their digests are
/// byte-stable across this refactor.
pub(crate) fn accumulate_bins_and_luminance_sum(frame: &ImageFrame) -> ([u64; BIN_COUNT], f64) {
    let mut bins = [0u64; BIN_COUNT];
    let mut sum = 0.0f64;
    for pixel in frame.pixels.as_chunks::<4>().0 {
        let luminance = (REC709_WEIGHTS[0] * f64::from(pixel[0])
            + REC709_WEIGHTS[1] * f64::from(pixel[1])
            + REC709_WEIGHTS[2] * f64::from(pixel[2]))
            / 255.0;
        let bin = ((luminance * BIN_COUNT as f64) as usize).min(BIN_COUNT - 1);
        bins[bin] += 1;
        sum += luminance;
    }
    (bins, sum)
}

impl LuminanceHistogram {
    /// Builds the 256-bin histogram for `frame`. Alpha is ignored; every
    /// pixel contributes its Rec.709 luminance sample. Empty frames yield an
    /// all-zero histogram without panics or division by zero.
    pub fn new(frame: &ImageFrame) -> Self {
        let (bins, _) = accumulate_bins_and_luminance_sum(frame);
        Self {
            width: frame.width,
            height: frame.height,
            bins: bins.to_vec(),
        }
    }

    /// Number of contributing pixels (sum of all bins).
    pub fn sample_count(&self) -> usize {
        self.bins.iter().sum::<u64>() as usize
    }

    /// Mean luminance, approximated from the bin centers. Per-sample deviation
    /// is at most half a bin width (`1/512`), so `mean()` matches
    /// [`crate::tone::analyze_tone`] within `1/512 + epsilon`.
    pub fn mean(&self) -> f64 {
        let sample_count = self.sample_count();
        if sample_count == 0 {
            return 0.0;
        }
        let sum: f64 = self
            .bins
            .iter()
            .enumerate()
            .map(|(index, count)| (index as f64 + 0.5) * BIN_WIDTH * *count as f64)
            .sum();
        sum / sample_count as f64
    }

    /// 50th percentile via linear CDF interpolation; `0.0` for empty frames.
    pub fn median(&self) -> f64 {
        self.quantile(0.5)
    }

    /// 1st percentile via linear CDF interpolation; `0.0` for empty frames.
    pub fn p01(&self) -> f64 {
        self.quantile(0.01)
    }

    /// 99th percentile via linear CDF interpolation; `0.0` for empty frames.
    pub fn p99(&self) -> f64 {
        self.quantile(0.99)
    }

    /// Quantile by linear interpolation over the cumulative distribution.
    ///
    /// Uses the same rank position as `tone::analyze_tone`
    /// (`position = q * (n - 1)`) and interpolates linearly within the bin
    /// that covers `position`. Empty frames return `0.0`.
    fn quantile(&self, q: f64) -> f64 {
        let sample_count = self.sample_count();
        if sample_count == 0 {
            return 0.0;
        }
        let position = q * (sample_count - 1) as f64;
        let mut cumulative: u64 = 0;
        for (index, bin) in self.bins.iter().enumerate() {
            if *bin == 0 {
                continue;
            }
            let next = cumulative + *bin;
            if position < next as f64 {
                let fraction = ((position - cumulative as f64) / *bin as f64).clamp(0.0, 1.0);
                return (index as f64 + fraction) * BIN_WIDTH;
            }
            cumulative = next;
        }
        // Defensive fallback for `position` exactly on the last sample boundary
        // (q == 1.0): report the upper edge of the last occupied bin.
        let last = self
            .bins
            .iter()
            .rposition(|bin| *bin > 0)
            .unwrap_or(BIN_COUNT - 1);
        (last as f64 + 1.0) * BIN_WIDTH
    }

    /// Fraction of samples at or below `value` (`0..=1`; empty frames → `0.0`).
    ///
    /// The cumulative function is linearly interpolated between bin edges, so
    /// it is monotonic non-decreasing and continuous, mirroring the quantile
    /// interpolation used by [`Self::p01`], [`Self::median`] and [`Self::p99`].
    /// Finite inputs outside `0..=1` are clamped into range; non-finite inputs
    /// (NaN/±Inf) are rejected with [`CoreError::InvalidAdjustment`] instead of
    /// propagating NaN through the clamp (REVIEW-CORE-N2).
    pub fn cdf_at(&self, value: f64) -> Result<f64, CoreError> {
        if !value.is_finite() {
            return Err(CoreError::InvalidAdjustment {
                name: "histogram.cdf_at".into(),
                value,
                minimum: 0.0,
                maximum: 1.0,
            });
        }
        let sample_count = self.sample_count();
        if sample_count == 0 {
            return Ok(0.0);
        }
        let value = value.clamp(0.0, 1.0);
        let edge = value * BIN_COUNT as f64;
        let lower_bin = edge.floor() as usize;
        if lower_bin >= BIN_COUNT {
            return Ok(1.0);
        }
        let fraction = edge - lower_bin as f64;
        let below: u64 = self.bins[..lower_bin].iter().sum();
        Ok((below as f64 + self.bins[lower_bin] as f64 * fraction) / sample_count as f64)
    }

    /// Stable content digest for cache identity (blake3 over frame dimensions
    /// and the 256 bins in canonical little-endian form). Byte-stable for
    /// identical images and safe to persist under `CacheStage::Histogram`.
    ///
    /// The digest identifies the serialized histogram state (bins + geometry),
    /// not the raw pixels: any image change that moves a luminance into
    /// another bin, or any geometry change, changes the digest. Images that
    /// differ only *within* one bin quantize to the same histogram and hence
    /// the same digest — inherent to 256-bin quantization and safe here
    /// because the cached value *is* the histogram.
    pub fn digest(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.width.to_le_bytes());
        hasher.update(&self.height.to_le_bytes());
        for bin in &self.bins {
            hasher.update(&bin.to_le_bytes());
        }
        format!(
            "luminance-histogram-rec709-rgba8:v1:{}",
            hasher.finalize().to_hex()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tone::analyze_tone;

    /// Documented quantile consistency tolerance of the constructed test
    /// images: one bin width plus floating point slack. See module docs for
    /// the accuracy discussion (bound holds for dense histograms).
    const QUANTILE_TOLERANCE: f64 = 1.0 / 256.0 + 1e-9;
    /// Bin-center mean tolerance: half a bin width plus floating point slack.
    const MEAN_TOLERANCE: f64 = 0.5 / 256.0 + 1e-9;

    fn frame(width: u32, height: u32, pixels: Vec<u8>) -> ImageFrame {
        ImageFrame::new(width, height, pixels).unwrap()
    }

    fn gray(value: u8) -> [u8; 4] {
        [value, value, value, 255]
    }

    #[test]
    fn bin_sum_equals_pixel_count_with_mixed_alpha() {
        let image = frame(
            4,
            1,
            vec![
                0, 0, 0, 0, 64, 64, 64, 1, 128, 128, 128, 127, 255, 255, 255, 255,
            ],
        );
        let histogram = LuminanceHistogram::new(&image);
        assert_eq!(histogram.sample_count(), 4);
        assert_eq!(histogram.bins.iter().sum::<u64>(), 4);
        // Every pixel contributes regardless of alpha: four distinct bins.
        assert_eq!(histogram.bins[0], 1);
        assert_eq!(histogram.bins[64], 1);
        assert_eq!(histogram.bins[128], 1);
        assert_eq!(histogram.bins[BIN_COUNT - 1], 1);
    }

    #[test]
    fn identical_frames_produce_identical_histogram_and_digest() {
        let pixels: Vec<u8> = (0..8u8)
            .flat_map(|index| gray(index.wrapping_mul(31)))
            .collect();
        let first = LuminanceHistogram::new(&frame(8, 1, pixels.clone()));
        let second = LuminanceHistogram::new(&frame(8, 1, pixels));
        assert_eq!(first, second);
        assert_eq!(first.digest(), second.digest());
    }

    #[test]
    fn digest_changes_on_image_change() {
        let pixels = vec![10, 20, 30, 255, 200, 150, 100, 255];
        let base = LuminanceHistogram::new(&frame(2, 1, pixels.clone()));
        // A channel change large enough to move the luminance into another bin
        // (200 -> 230 shifts the Rec.709 result by ~0.025, several bin widths).
        let changed_pixels = vec![10, 20, 30, 255, 230, 150, 100, 255];
        let changed = LuminanceHistogram::new(&frame(2, 1, changed_pixels));
        assert_ne!(base.digest(), changed.digest());
        // Pure geometry change with identical pixel bytes also differs.
        let tall = LuminanceHistogram::new(&frame(1, 2, pixels));
        assert_ne!(base.digest(), tall.digest());
        // Sub-bin luminance changes intentionally keep the same histogram and
        // therefore the same digest: the digest identifies the serialized
        // histogram state (bins + geometry), which is the cached value.
        let sub_bin = vec![10, 20, 30, 255, 201, 150, 100, 255];
        let within_bin = LuminanceHistogram::new(&frame(2, 1, sub_bin));
        assert_eq!(base.bins, within_bin.bins);
        assert_eq!(base.digest(), within_bin.digest());
    }

    #[test]
    fn empty_frame_is_all_zero_and_safe() {
        let empty = LuminanceHistogram::new(&frame(0, 0, vec![]));
        assert_eq!(empty.sample_count(), 0);
        assert_eq!(empty.p01(), 0.0);
        assert_eq!(empty.median(), 0.0);
        assert_eq!(empty.p99(), 0.0);
        assert_eq!(empty.mean(), 0.0);
        assert_eq!(empty.cdf_at(0.5).unwrap(), 0.0);
        assert!(empty.bins.iter().all(|bin| *bin == 0));
        assert!(empty
            .digest()
            .starts_with("luminance-histogram-rec709-rgba8:v1:"));
    }

    #[test]
    fn cdf_is_monotonic_and_quantiles_are_in_range() {
        let image = frame(
            3,
            1,
            gray(64)
                .into_iter()
                .chain(gray(96))
                .chain(gray(255))
                .collect(),
        );
        let histogram = LuminanceHistogram::new(&image);
        let mut previous = histogram.cdf_at(0.0).unwrap();
        assert_eq!(previous, 0.0);
        for step in 1..=256u32 {
            let value = f64::from(step) / 256.0;
            let current = histogram.cdf_at(value).unwrap();
            assert!(current >= previous, "CDF not monotone at {value}");
            assert!((0.0..=1.0).contains(&current));
            previous = current;
        }
        assert!((histogram.cdf_at(1.0).unwrap() - 1.0).abs() < 1e-12);
        for quantile in [histogram.p01(), histogram.median(), histogram.p99()] {
            assert!(quantile.is_finite());
            assert!((0.0..=1.0).contains(&quantile));
        }
        assert!(histogram.p01() <= histogram.median());
        assert!(histogram.median() <= histogram.p99());
        // CDF evaluated at the quantile values preserves their ordering
        // (guaranteed by CDF monotonicity).
        let cdf_p01 = histogram.cdf_at(histogram.p01()).unwrap();
        let cdf_median = histogram.cdf_at(histogram.median()).unwrap();
        let cdf_p99 = histogram.cdf_at(histogram.p99()).unwrap();
        assert!(cdf_p01 <= cdf_median);
        assert!(cdf_median <= cdf_p99);
    }

    // REVIEW-CORE-N2: NaN/±Inf must be rejected instead of silently
    // propagating through `clamp` (which returns NaN for a NaN input).
    #[test]
    fn cdf_at_rejects_non_finite_values_with_an_error() {
        let image = frame(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]);
        let histogram = LuminanceHistogram::new(&image);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let error = histogram
                .cdf_at(value)
                .err()
                .unwrap_or_else(|| panic!("cdf_at({value}) must fail"));
            match error {
                CoreError::InvalidAdjustment {
                    ref name,
                    minimum,
                    maximum,
                    ..
                } => {
                    assert_eq!(name, "histogram.cdf_at");
                    assert_eq!((minimum, maximum), (0.0, 1.0));
                }
                other => panic!("unexpected error for cdf_at({value}): {other:?}"),
            }
        }
        // Finite out-of-range values stay clamped as documented.
        assert_eq!(histogram.cdf_at(-0.5).unwrap(), 0.0);
        assert_eq!(histogram.cdf_at(1.5).unwrap(), 1.0);
    }

    /// Constructed, densely populated images where the histogram quantiles are
    /// within the documented one-bin-width tolerance of `analyze_tone`, and
    /// the bin-center mean within half a bin width.
    #[test]
    fn quantiles_and_mean_match_analyze_tone_within_documented_tolerance() {
        let mut frames: Vec<ImageFrame> = Vec::new();
        // 1) Full gray ramp, four copies of every level 0..=255: every bin
        //    populated, n = 1024.
        let mut ramp = Vec::with_capacity(256 * 4 * 4);
        for _ in 0..4 {
            for value in 0..=255u8 {
                ramp.extend_from_slice(&gray(value));
            }
        }
        frames.push(frame(256, 4, ramp));
        // 2) Diagonal gray gradient: dense coverage of all bins, tilt against
        //    the row order, n = 8192.
        let mut diagonal = Vec::with_capacity(256 * 32 * 4);
        for y in 0..32u32 {
            for x in 0..256u32 {
                let value = ((x + 2 * y) % 256) as u8;
                diagonal.extend_from_slice(&gray(value));
            }
        }
        frames.push(frame(256, 32, diagonal));
        // 3) Rec.709-weighted luminance from saturated colors plus grays,
        //    repeated: full-range but sparse value set, n = 1100.
        let colors: [[u8; 3]; 11] = [
            [0, 0, 0],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 0],
            [0, 255, 255],
            [255, 0, 255],
            [255, 255, 255],
            [128, 128, 128],
            [64, 64, 64],
            [192, 192, 192],
        ];
        let mut colored = Vec::with_capacity(11 * 100 * 4);
        for _ in 0..100 {
            for color in colors {
                colored.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
        frames.push(frame(1100, 1, colored));

        for image in &frames {
            let histogram = LuminanceHistogram::new(image);
            let analysis = analyze_tone(image);
            assert_eq!(histogram.sample_count(), analysis.sample_count);
            for (name, actual, expected) in [
                ("p01", histogram.p01(), analysis.p01),
                ("median", histogram.median(), analysis.median),
                ("p99", histogram.p99(), analysis.p99),
            ] {
                assert!(
                    (actual - expected).abs() <= QUANTILE_TOLERANCE,
                    "{name}: histogram {actual} vs analyze_tone {expected}, \
                     deviation exceeds documented {QUANTILE_TOLERANCE}"
                );
            }
            assert!(
                (histogram.mean() - analysis.mean).abs() <= MEAN_TOLERANCE,
                "mean: histogram {} vs analyze_tone {}, deviation exceeds \
                 documented {MEAN_TOLERANCE}",
                histogram.mean(),
                analysis.mean
            );
        }
    }

    #[test]
    fn serde_roundtrip_preserves_state_and_digest() {
        let histogram = LuminanceHistogram::new(&frame(
            3,
            2,
            vec![
                3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220, 12,
                200, 71, 33, 88, 250,
            ],
        ));
        let json = serde_json::to_string(&histogram).unwrap();
        let restored: LuminanceHistogram = serde_json::from_str(&json).unwrap();
        assert_eq!(histogram, restored);
        assert_eq!(histogram.digest(), restored.digest());
        // JSON carries the bins, so a fresh serialize is a full copy.
        assert!(json.contains("\"width\":3"));
    }

    /// Consistency of quantile rank positions with `analyze_tone` at the
    /// single-sample boundary: a fully white pixel quantizes into the last bin
    /// and reports its lower edge (documented deviation of at most 1/256).
    #[test]
    fn quantiles_are_consistent_at_extreme_ranks() {
        let image = frame(1, 1, gray(255).to_vec());
        let single = LuminanceHistogram::new(&image);
        assert_eq!(single.median(), 255.0 * BIN_WIDTH);
        assert!((single.median() - analyze_tone(&image).median).abs() <= 1.0 / 256.0 + 1e-9);
    }

    // REVIEW-CORE-WASM-FOLLOWUP: these properties depend on `proptest`, which
    // is a non-wasm32 dev-dependency (its transitive `getrandom`/`wait-timeout`
    // do not compile for wasm32-unknown-unknown). The module is gated with the
    // exact same condition as the dependency in `Cargo.toml`, so test targets
    // compile for every platform; on wasm32 only the deterministic unit tests
    // above are built.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    mod proptests {
        use super::super::LuminanceHistogram;
        use super::frame;
        use crate::ImageFrame;
        use proptest::prelude::*;

        /// Strategy producing small random RGBA8 frames (1..=8 pixels per side)
        /// with arbitrary per-channel values — including arbitrary alphas.
        fn any_frame() -> impl Strategy<Value = ImageFrame> {
            (1usize..=8usize, 1usize..=8usize).prop_flat_map(|(width, height)| {
                prop::collection::vec(any::<u8>(), width * height * 4)
                    .prop_map(move |pixels| frame(width as u32, height as u32, pixels))
            })
        }

        proptest! {
            /// Random frames of any RGB/alpha mixture: the histogram always counts
            /// exactly width*height samples, all quantiles stay in 0..=1 (finite),
            /// and the CDF is monotone and reaches exactly 1.0 at the upper bound.
            #[test]
            fn histogram_invariants_hold_for_random_frames(frame in any_frame()) {
                let histogram = LuminanceHistogram::new(&frame);
                let expected = frame.width as usize * frame.height as usize;
                prop_assert_eq!(histogram.sample_count(), expected);
                prop_assert_eq!(histogram.bins.iter().sum::<u64>() as usize, expected);
                for quantile in [histogram.p01(), histogram.median(), histogram.p99(), histogram.mean()] {
                    prop_assert!(quantile.is_finite(), "quantile must be finite, got {quantile}");
                    prop_assert!((0.0..=1.0).contains(&quantile), "quantile in 0..=1, got {quantile}");
                }
                prop_assert!(histogram.p01() <= histogram.median() + 1e-15);
                prop_assert!(histogram.median() <= histogram.p99() + 1e-15);

                let mut previous = histogram.cdf_at(0.0).unwrap();
                prop_assert_eq!(previous, 0.0);
                for step in 1..=64u32 {
                    let value = f64::from(step) / 64.0;
                    let current = histogram.cdf_at(value).unwrap();
                    prop_assert!(
                        current >= previous - 1e-12,
                        "CDF must be monotone at {value}: {current} < {previous}"
                    );
                    prop_assert!((0.0..=1.0).contains(&current));
                    previous = current;
                }
                prop_assert!((histogram.cdf_at(1.0).unwrap() - 1.0).abs() < 1e-12);
            }

            /// Reconstructing the same image twice yields the identical histogram
            /// and the identical digest (cache-safety of `CacheStage::Histogram`).
            #[test]
            fn digest_is_deterministic_for_identical_frames(frame in any_frame()) {
                let first = LuminanceHistogram::new(&frame);
                let second = LuminanceHistogram::new(&frame);
                prop_assert_eq!(&first, &second);
                let first_digest = first.digest();
                let second_digest = second.digest();
                prop_assert!(!first_digest.is_empty());
                prop_assert_eq!(&first_digest, &second_digest);
            }
        }
    }
}
