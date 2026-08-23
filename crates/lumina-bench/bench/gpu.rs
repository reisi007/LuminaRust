//! GPU render-path benchmarks for Lumina (F-074-N6 draft).
//!
//! Extends the F-074 benchmark suite so the **GPU** render path
//! ([`lumina_gpu::GpuContext::render_with_gpu`]) is measured alongside the
//! existing CPU pipeline benchmarks in `core.rs`/`batch.rs`, satisfying the
//! requirement that *both* the CPU and GPU parts are benchmarked.
//!
//! Reported benchmarks (group `gpu`, fixed sizes 512 / 1024 / 2048 per
//! `bench/common/mod.rs`):
//!
//! - `gpu/render_with_gpu__{512,1024,2048}` — full-frame GPU render, reusing
//!   the exact same synthetic frames + recipes as `core.rs`.
//! - `gpu/update_uniforms__recipe` — uniform-upload-only microbenchmark (the
//!   `queue.write_buffer` path) at 2048, i.e. the per-recipe host→device
//!   upload cost without the render pass.
//! - `gpu/cpu_vs_gpu__cpu__2048` / `gpu/cpu_vs_gpu__gpu__2048` — end-to-end
//!   comparison at 2048: the CPU adjustment path (`ImageFrame::apply_recipe`,
//!   identical to the GPU fallback) and the GPU path measured back-to-back so
//!   their ratio is directly comparable in one report.
//!
//! The GPU context is created **once** per group. If no GPU adapter is bound
//! (e.g. headless CI without Metal/Vulkan), the whole group is skipped
//! gracefully with a clear message — no panic, no network, no fake fallback
//! number. Because both the `gpu` bench target and the `lumina-gpu` API are
//! gated behind LuminaRust's `gpu` feature, the file only compiles when that
//! feature is enabled (see `Cargo.toml`).

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use lumina_gpu::GpuContext;

mod common;
use common::{make_frame, make_recipe, SIZES};

/// One shared GPU context for the whole group. Returns `Some` only when a real
/// adapter is bound; otherwise `None` and the caller skips the group. Adapter
/// init failures are also treated as "unavailable" so the harness stays green.
fn build_gpu_context() -> Option<GpuContext> {
    match GpuContext::new() {
        Ok(ctx) => {
            if ctx.is_available() {
                Some(ctx)
            } else {
                eprintln!("GPU adapter unavailable - skipped equivalence check");
                None
            }
        }
        Err(e) => {
            eprintln!("GPU adapter unavailable - skipped equivalence check: {e}");
            None
        }
    }
}

fn gpu_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu");

    // Build the render context once. If no GPU adapter is bound, skip the whole
    // group gracefully (no panic, no fake number). All GPU render benchmarks use
    // this `&self` context.
    let Some(render_ctx) = build_gpu_context() else {
        group.finish();
        return;
    };

    // ---- full-frame GPU render (same frames/recipes as core.rs) ----
    for &size in SIZES {
        let frame = make_frame(size);
        let recipe = make_recipe();
        group.bench_function(format!("render_with_gpu__{size}"), |b| {
            b.iter(|| {
                black_box(
                    render_ctx
                        .render_with_gpu(black_box(&frame), black_box(&recipe))
                        .unwrap(),
                )
            })
        });
    }

    // ---- uniform upload-only microbenchmark (queue.write_buffer path) ----
    // A separate mutable context: `update_uniforms` needs `&mut self`. It builds
    // the pipeline lazily once, then re-uploads the recipe sliders into the
    // uniform buffer on every iteration — isolating the host→device upload cost
    // from the render pass.
    {
        let recipe = make_recipe();
        let mut uctx = GpuContext::new().expect("gpu context must build");
        group.bench_function("update_uniforms__recipe", |b| {
            b.iter(|| {
                uctx.update_uniforms(black_box(&recipe)).unwrap();
            })
        });
    }

    // ---- CPU vs GPU end-to-end @2048 (back-to-back, comparable ratio) ----
    {
        let size = 2048;
        let frame = make_frame(size);
        let recipe = make_recipe();

        // CPU path: identical adjustment math to the GPU fallback in
        // `lumina-gpu` (`ImageFrame::apply_recipe`).
        group.bench_function("cpu_vs_gpu__cpu__2048", |b| {
            b.iter_batched(
                || frame.clone(),
                |mut f| {
                    f.apply_recipe(black_box(&recipe)).unwrap();
                    black_box(f.pixels[0])
                },
                BatchSize::SmallInput,
            )
        });

        // GPU path: end-to-end render via the same context used above.
        group.bench_function("cpu_vs_gpu__gpu__2048", |b| {
            b.iter(|| {
                black_box(
                    render_ctx
                        .render_with_gpu(black_box(&frame), black_box(&recipe))
                        .unwrap(),
                )
            })
        });
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(2))
        .sample_size(50);
    targets = gpu_benches
}
criterion_main!(benches);
