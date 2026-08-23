//! Core/Pipeline benchmarks for `lumina-core` (F-074-N3).
//!
//! Covers the benchmark classes from `feature/quality/performance-benchmarks.md`:
//! complete `render_frame` (incl. mask resample), `apply_recipe_with_white_balance`,
//! `MaskGraph` evaluation, `analyze_tone`, `suggest_auto_tone`,
//! `match_total_exposure`, `LuminanceHistogram` build, and `FolderCache`
//! hit/miss. Every operation is measured at the fixed sizes 512 / 1024 / 2048
//! (see `bench/common/mod.rs`).
//!
//! Runtime budget per benchmark is modest (1s warm-up, 2s measurement, 50
//! samples) so a full baseline capture stays tractable (see header docs in the
//! bench file). Override via Criterion CLI flags if needed.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use lumina_core::{
    analyze_tone, match_total_exposure, render_frame, suggest_auto_tone, AutoToneConfig,
    LuminanceHistogram, MaskContext, MaskGraph, MaskPolicy, RenderContext,
};
use lumina_sidecar::{Extras, MaskReference};
use std::time::Duration;

mod common;
use common::{make_cache_fixture, make_frame, make_mask_fixture, make_recipe, SIZES};

fn core_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("core");

    for &size in SIZES {
        let frame = make_frame(size);
        let recipe = make_recipe();

        // ---- complete render_frame (Source-Actions → Adjustments+WB → Masks) ----
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
            lensfun: None,
        };
        group.bench_function(format!("render_frame__{size}"), |b| {
            b.iter(|| black_box(render_frame(black_box(&frame), &render_ctx).unwrap()))
        });

        // ---- apply_recipe_with_white_balance (adjustments incl. WB) ----
        group.bench_function(format!("apply_recipe_with_white_balance__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |mut f| {
                    // Mutates `f`; read a pixel afterwards so the optimizer
                    // cannot elide the adjustment pass.
                    f.apply_recipe_with_white_balance(black_box(&recipe), None)
                        .unwrap();
                    black_box(f.pixels[0])
                },
                BatchSize::SmallInput,
            )
        });

        // ---- MaskGraph evaluation (+ plane resampling is exercised via render_frame) ----
        let mfix = make_mask_fixture(size);
        let graph = MaskGraph::new(&mfix.copies, mfix.planes.clone());
        let subject_ref = MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "subject".into(),
            extras: Extras::new(),
        };
        group.bench_function(format!("mask_graph_eval__{size}"), |b| {
            b.iter(|| black_box(black_box(&graph).evaluate(&subject_ref).unwrap()))
        });

        // ---- analyze_tone ----
        group.bench_function(format!("analyze_tone__{size}"), |b| {
            b.iter(|| black_box(analyze_tone(black_box(&frame))))
        });

        // ---- suggest_auto_tone ----
        let auto_config = AutoToneConfig::default();
        group.bench_function(format!("suggest_auto_tone__{size}"), |b| {
            b.iter(|| black_box(suggest_auto_tone(black_box(&frame), auto_config).unwrap()))
        });

        // ---- match_total_exposure ----
        group.bench_function(format!("match_total_exposure__{size}"), |b| {
            b.iter(|| black_box(match_total_exposure(black_box(&frame), 0.5).unwrap()))
        });

        // ---- LuminanceHistogram build + aggregation ----
        group.bench_function(format!("histogram__{size}"), |b| {
            b.iter(|| {
                let h = LuminanceHistogram::new(black_box(&frame));
                black_box(h.median())
            })
        });

        // ---- FolderCache hit / miss (separate benchmarks) ----
        let (cache, hit_key, miss_key) = make_cache_fixture(size);
        group.bench_function(format!("cache_hit__{size}"), |b| {
            b.iter(|| black_box(black_box(&cache).get(black_box(&hit_key))))
        });
        group.bench_function(format!("cache_miss__{size}"), |b| {
            b.iter(|| black_box(black_box(&cache).get(black_box(&miss_key))))
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2))
        .sample_size(50);
    targets = core_benches
}
criterion_main!(benches);
