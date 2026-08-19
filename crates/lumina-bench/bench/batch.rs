//! Batch / End-to-End benchmarks for Lumina (F-074-N3).
//!
//! Simulates the batch-export hot loop: `render_frame` followed by
//! `encode_with_options` (PNG) on the same deterministic input, repeated. This
//! models `N` identical exports of one source. Measured at 512 / 1024 / 2048
//! (see `bench/common/mod.rs`).

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use lumina_core::{render_frame, ExportOptions, MaskContext, MaskPolicy, RenderContext};
use std::time::Duration;

mod common;
use common::{make_frame, make_mask_fixture, make_recipe, SIZES};

fn batch_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch");

    for &size in SIZES {
        let frame = make_frame(size);
        let recipe = make_recipe();

        let fixture = make_mask_fixture(size);
        let mask_ctx = MaskContext {
            copies: &fixture.copies,
            active_copy_id: "vc-original",
            planes: fixture.planes.clone(),
            policy: MaskPolicy::Warn,
        };
        let render_ctx = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: Some(mask_ctx),
        };

        group.bench_function(format!("render_export_png__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |f| {
                    let output = render_frame(&f, &render_ctx).unwrap();
                    black_box(
                        output
                            .frame
                            .encode_with_options(ExportOptions::default())
                            .unwrap(),
                    )
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2))
        .sample_size(30);
    targets = batch_benches
}
criterion_main!(benches);
