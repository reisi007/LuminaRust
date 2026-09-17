//! KI-Denoise benchmarks for `lumina-core` (F-074 / LRPAR-G14-DENOISE-IMPL-20).
//!
//! Covers the model-free, deterministic denoise surface added by the
//! KI-Denoise slice:
//!
//! * `blend__*` — the `strength`/`preserve_detail` blend kernel
//!   ([`lumina_core::apply_denoise_blend`]) that the `Adjustments` sub-stage
//!   runs per render,
//! * `assemble_tiles__*` — the canonical seam-free tile assembly
//!   ([`lumina_core::assemble_denoise_tiles`], 512 px tiles / 32 px overlap,
//!   the documented default),
//! * `render_ready__*` — the denoise-aware end-to-end render
//!   ([`lumina_core::render_frame_with_denoise`]) that the CLI `denoise
//!   --render` and the GUI preview path call,
//! * `status_resolve__ready` — the pure §6 status classification
//!   ([`lumina_core::resolve_denoise_status`]) behind the CLI `--status` /
//!   GUI badge refresh (I/O-free; the sidecar file read is not a core kernel).
//!
//! **Not a model benchmark.** The ONNX weights are `pending-integration`
//! (decision §3.1), so no real inference exists to measure. These benchmarks
//! run the deterministic fixture path (synthetic artifact + pinned filler
//! hash) exactly like the tests; they must never be read as denoiser
//! throughput numbers. Inputs are synthetic and deterministic from the frozen
//! seed regime in `bench/common/mod.rs` (no network, no weights, no fixtures).
//!
//! New class: all IDs are registered report-only (`gate: false`) until the
//! class is independently calibrated — the same F-074 rule the GPU and merge
//! classes followed. `sample_size = 20` keeps the capture tractable; the
//! `render_ready__2048` case is the heaviest.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use lumina_core::{
    apply_denoise_blend, assemble_denoise_tiles, render_frame_with_denoise, resolve_denoise_status,
    DenoiseStageInput, DenoiseTile, RenderContext,
};
use std::hint::black_box;
use std::time::Duration;

mod common;
use common::{
    make_denoise_ai, make_denoise_artifact, make_denoise_identity, make_frame, make_recipe, SIZES,
};

/// Documented default tile geometry from the denoise ONNX slice (part of the
/// `input_spec_digest`): 512×512 tiles with a 32 px overlap.
const TILE: u32 = 512;
const OVERLAP: u32 = 32;

/// One deterministic tile buffer plus its placement; `denoise` consumes
/// borrowed `DenoiseTile` views, so the pixels stay owned here.
struct TileData {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl TileData {
    fn as_tile(&self) -> DenoiseTile<'_> {
        DenoiseTile {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            pixels: &self.pixels,
        }
    }
}

/// Ascending tile origins that cover `size` completely (last origin pinned to
/// `size - TILE`). A frame not larger than one tile yields a single origin.
fn tile_origins(size: u32) -> Vec<u32> {
    if size <= TILE {
        return vec![0];
    }
    let stride = TILE - OVERLAP;
    let mut origins = Vec::new();
    let mut position = 0;
    loop {
        origins.push(position);
        if position + TILE >= size {
            break;
        }
        position = (position + stride).min(size - TILE);
    }
    origins
}

/// Deterministic tiling of a `size × size` frame with the documented tile
/// geometry; pixel values are a fixed function of the global coordinate, so
/// assembly is reproducible and the overlap actually crossfades.
fn make_tiles(size: u32) -> Vec<TileData> {
    let origins = tile_origins(size);
    let mut tiles = Vec::with_capacity(origins.len() * origins.len());
    for &y in &origins {
        for &x in &origins {
            let mut pixels = Vec::with_capacity((TILE * TILE * 3) as usize);
            for py in 0..TILE {
                for px in 0..TILE {
                    let gx = x + px;
                    let gy = y + py;
                    pixels.push(((gx.wrapping_mul(3) + gy.wrapping_mul(7)) % 256) as u8);
                    pixels.push(((gx.wrapping_mul(5) + gy.wrapping_mul(11) + 17) % 256) as u8);
                    pixels.push(((gx.wrapping_mul(13) + gy.wrapping_mul(2) + 91) % 256) as u8);
                }
            }
            tiles.push(TileData {
                x,
                y,
                width: TILE,
                height: TILE,
                pixels,
            });
        }
    }
    tiles
}

fn denoise_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("denoise");

    for &size in SIZES {
        let frame = make_frame(size);
        let artifact = make_denoise_artifact(size);
        let checksum = artifact.checksum();

        // ---- core blend kernel (mutates the frame; reset per iteration) ----
        group.bench_function(format!("blend__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |mut candidate| {
                    apply_denoise_blend(black_box(&mut candidate), black_box(&artifact), 0.5, 0.5)
                        .unwrap();
                    black_box(candidate.pixels[0])
                },
                BatchSize::SmallInput,
            )
        });

        // ---- canonical tile assembly (seam-free weighted mean) ----
        let tiles = make_tiles(size);
        let tile_refs: Vec<DenoiseTile<'_>> = tiles.iter().map(TileData::as_tile).collect();
        group.bench_function(format!("assemble_tiles__{size}"), |b| {
            b.iter(|| black_box(assemble_denoise_tiles(size, size, black_box(&tile_refs)).unwrap()))
        });

        // ---- denoise-aware render path (CLI `denoise --render`, GUI preview) ----
        let mut recipe = make_recipe();
        recipe.denoise_ai = Some(make_denoise_ai(size, 0.5, 0.5, &checksum));
        let stage = DenoiseStageInput::ready(&artifact);
        let render_ctx = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        };
        group.bench_function(format!("render_ready__{size}"), |b| {
            b.iter(|| {
                black_box(
                    render_frame_with_denoise(black_box(&frame), &render_ctx, &stage).unwrap(),
                )
            })
        });
    }

    // ---- §6 status classification (CLI `--status`, GUI badge refresh) ----
    let checksum = make_denoise_artifact(64).checksum();
    let recipe = make_denoise_ai(64, 0.5, 0.5, &checksum);
    let current = make_denoise_identity(&checksum);
    let recorded = current.clone();
    group.bench_function("status_resolve__ready", |b| {
        b.iter(|| {
            black_box(resolve_denoise_status(
                black_box(&recipe),
                black_box(&current),
                black_box(&recorded),
                true,
            ))
        })
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .sample_size(20);
    targets = denoise_benches
}
criterion_main!(benches);
