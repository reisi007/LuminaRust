//! HDR/panorama merge benchmarks for `lumina-merge` (F-074 / MERGE-IMPL-15).
//!
//! Covers the merge class introduced with LRPAR-G13-MERGE-15:
//! weighted HDR merge, cylindrical panorama blend and the linear DNG writer,
//! plus one HDR alignment benchmark. Inputs are the deterministic synthetic
//! linear gradients from `bench/common/mod.rs` (no network, no fixtures).
//!
//! Methodology: `feature/quality/performance-benchmarks.md` (F-074) and
//! ADR 0003. New IDs start `gate: false` (report mode) until they have been
//! independently calibrated, exactly like the GPU class.
//!
//! `merge/align_hdr__512` uses a small search radius (`max_shift_px = 8`); the
//! exhaustive SAD search is quadratic in the radius, so the radius is
//! documented in the ID's store note rather than encoded in the fixture size.
//! The panorama *estimator* is deliberately not benchmarked: its joint
//! translation/rotation search is an interactive-latency path measured by the
//! CLI/GUI status, not a per-frame kernel; the blend it feeds is measured here.

use criterion::{criterion_group, criterion_main, Criterion};
use lumina_merge::{
    blend_panorama_transformed, encode_linear_dng, estimate_hdr_translation, merge_hdr_weighted,
    DngExif,
};
use lumina_sidecar::{MergeExposure, MergeMode};
use std::hint::black_box;
use std::time::Duration;

mod common;
use common::{make_linear_gradient, make_linear_gradient_shifted, scale_linear, SIZES};

const ALIGN_SIZE: u32 = 512;
const ALIGN_MAX_SHIFT_PX: i32 = 8;

fn exposure(time: f64) -> MergeExposure {
    MergeExposure {
        exposure_time_s: time,
        iso: 100,
        f_number: 8.0,
    }
}

fn merge_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("merge");

    for &size in SIZES {
        let reference = make_linear_gradient(size);
        let underexposed = scale_linear(&reference, 0.5);
        let frames = [reference.clone(), underexposed];
        let exposures = [exposure(0.01), exposure(0.02)];
        let shifts = [(0.0, 0.0), (0.0, 0.0)];

        // ---- weighted linear HDR merge ----
        group.bench_function(format!("hdr_weighted__{size}"), |b| {
            b.iter(|| {
                black_box(
                    merge_hdr_weighted(black_box(&frames), black_box(&exposures), &shifts).unwrap(),
                )
            })
        });

        // ---- panorama blend through the full 3x3 matrix ----
        let shifted = make_linear_gradient_shifted(size, 4.0);
        let pano_frames = [reference.clone(), shifted];
        let matrices = [
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 4.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        ];
        group.bench_function(format!("pano_blend__{size}"), |b| {
            b.iter(|| {
                black_box(
                    blend_panorama_transformed(black_box(&pano_frames), &matrices, 64).unwrap(),
                )
            })
        });

        // ---- linear 16-bit DNG writer ----
        group.bench_function(format!("encode_dng__{size}"), |b| {
            b.iter(|| {
                black_box(
                    encode_linear_dng(black_box(&reference), MergeMode::Hdr, &DngExif::default())
                        .unwrap(),
                )
            })
        });
    }

    // ---- HDR translation alignment (small documented search radius) ----
    let align_reference = make_linear_gradient(ALIGN_SIZE);
    let align_moving = make_linear_gradient_shifted(ALIGN_SIZE, 3.5);
    group.bench_function(format!("align_hdr__{ALIGN_SIZE}"), |b| {
        b.iter(|| {
            black_box(
                estimate_hdr_translation(
                    black_box(&align_reference),
                    black_box(&align_moving),
                    ALIGN_MAX_SHIFT_PX,
                )
                .unwrap(),
            )
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2))
        .sample_size(30);
    targets = merge_benches
}
criterion_main!(benches);
