//! Assisted-culling benchmarks for `lumina-cull` (F-074 / LRPAR-G09-CULL-25).
//!
//! Covers the deterministic, model-free Stage-1 heuristic (no ONNX, no
//! weights, no network):
//!
//! * `analyze__*` — the full per-image heuristic
//!   ([`lumina_cull::analyze_heuristic`]) behind the CLI `--analyze` and the
//!   GUI badge refresh: downscale, luminance/structural pass, noise estimate,
//!   exposure/clipping and similarity signature,
//! * `noise_sigma__*` — the Immerkaer noise kernel on a pre-blurred plane.
//!   This is the hotspot called out in the decision doc (§9): the transient
//!   interior sample vector (~22 MB at a 2048 px analysis width),
//! * `similarity_signature__*` — the content-only dHash/histogram signature
//!   used for duplicate/series grouping,
//! * `analyze_selection__4x512` — an explicit 4-image selection (per-image
//!   analysis plus the bounded duplicate/series pass; no folder traversal),
//! * `status_evaluate__valid` — the pure identity/status classification
//!   ([`lumina_cull::evaluate_culling`]) behind the CLI `--status` / GUI badge
//!   read path (I/O-free; the sidecar file read is not a cull kernel).
//!
//! Inputs are the deterministic synthetic frames from `bench/common/mod.rs`
//! (frozen seed `0x5EED`, sizes 512 / 1024 / 2048, no committed data).
//!
//! New class: all IDs are registered report-only (`gate: false`) until the
//! class is independently calibrated — the same F-074 rule the GPU and merge
//! classes followed. The full analysis at 2048 is the heaviest case, so the
//! group uses `sample_size = 10`.

use criterion::{criterion_group, criterion_main, Criterion};
use lumina_core::ImageFrame;
use lumina_cull::signals::{blur_3x3, luma_plane, noise_sigma};
use lumina_cull::{
    analyze_heuristic, analyze_selection, evaluate_culling, similarity_signature, CullConfig,
    CullSourceInput,
};
use std::hint::black_box;
use std::time::Duration;

mod common;
use common::{make_culling_status_fixture, make_frame, SIZES};

/// Number of explicitly selected images in the selection benchmark. The
/// selection boundary is always the caller's; a small fixed selection keeps the
/// grouping pass deterministic and bounded (decision §9).
const SELECTION_SIZE: usize = 4;
const SELECTION_FRAME: u32 = 512;

fn cull_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("cull");
    let config = CullConfig::default();

    for &size in SIZES {
        let frame = make_frame(size);
        let input = CullSourceInput {
            frame: &frame,
            iso: None,
        };

        // ---- full Stage-1 heuristic (CLI `cull --analyze`, GUI refresh) ----
        group.bench_function(format!("analyze__{size}"), |b| {
            b.iter(|| black_box(analyze_heuristic(black_box(&input), &config).unwrap()))
        });

        // ---- noise kernel on the pre-blurred plane (documented hotspot) ----
        let plane = luma_plane(&frame).expect("deterministic luma plane");
        let blurred = blur_3x3(&plane);
        group.bench_function(format!("noise_sigma__{size}"), |b| {
            b.iter(|| black_box(noise_sigma(black_box(&plane), black_box(&blurred))))
        });

        // ---- content-only similarity signature ----
        group.bench_function(format!("similarity_signature__{size}"), |b| {
            b.iter(|| black_box(similarity_signature(black_box(&frame)).unwrap()))
        });
    }

    // ---- explicit selection: per-image analysis + duplicate/series grouping ----
    let selection: Vec<ImageFrame> = (0..SELECTION_SIZE)
        .map(|_| make_frame(SELECTION_FRAME))
        .collect();
    let inputs: Vec<CullSourceInput<'_>> = selection
        .iter()
        .map(|frame| CullSourceInput { frame, iso: None })
        .collect();
    group.bench_function(
        format!("analyze_selection__{SELECTION_SIZE}x{SELECTION_FRAME}"),
        |b| b.iter(|| black_box(analyze_selection(black_box(&inputs), &config).unwrap())),
    );

    // ---- status/identity classification (CLI `cull --status`, GUI badge) ----
    let (document, identity) = make_culling_status_fixture(256);
    group.bench_function("status_evaluate__valid", |b| {
        b.iter(|| black_box(evaluate_culling(black_box(&document), black_box(&identity))))
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(10);
    targets = cull_benches
}
criterion_main!(benches);
