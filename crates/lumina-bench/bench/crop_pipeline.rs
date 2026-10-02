//! PIPELINE-ANNASSUNGEN-10: the benchmark `perf/baseline.json` does **not**
//! have.
//!
//! # Why this file exists
//!
//! The question in `PIPELINE-CROP-EARLY-9` is whether pulling `crop` earlier in
//! the render pipeline is worth the reorder at all. **A3 measured the answer to
//! the prerequisite: no benchmark with a crop exists.** The 60 benchmark ids in
//! `perf/baseline.json` contain zero with `crop`, `perspective` or `lens` in the
//! name, so the saving cannot be read off the existing baseline — it has to be
//! produced.
//!
//! # What is measured, and what is not
//!
//! **(b) the crop itself, and the saving it produces today**
//!
//! * `render_frame_cropped__{size}__{rest}pct` — a **complete** `render_frame`
//!   with a `Crop::Free` rectangle leaving `rest` percent of the area, for
//!   50 / 25 / 10 %. This is the *current* pipeline, where crop is applied near
//!   the end and the expensive stages still run on the full frame. The number is
//!   the **cost an early crop would remove**.
//! * `render_frame_cropped_full__{size}` — a crop rect covering the **whole**
//!   frame. This is the control: it exercises the crop stage without removing a
//!   pixel, so the delta against `render_frame__{size}` is the crop stage's own
//!   cost. Without it, "the crop is cheap" and "the early reorder would be cheap"
//!   cannot be told apart.
//!
//! **(a) the individual geometry-stage cost**
//!
//! * `lens_stage__{size}`, `perspective_stage__{size}`, `autofill_stage__{size}`,
//!   `expand_stage__{size}` — each render-path geometry stage **called
//!   directly**, so the cost is measured instead of inferred by subtracting two
//!   end-to-end numbers (the subtraction the previous pass explicitly refused as
//!   not isolable). AutoFill/Expand measure the artifact *clone* the render path
//!   performs; the retired heuristic BFS is not on that path and is not measured.
//!
//! **(c) the cache**
//!
//! * `prepare_source_base__{size}` — the base build/cache-**miss** cost
//!   (`source.clone()` plus source actions).
//! * `stage_cache_hit__{size}` — a keyed clone out of `StageFrameCache`, the
//!   **hit** cost. Both are independent of any crop rectangle: the cache key is
//!   recipe-blind, so a crop placed *behind* the cache cannot change the hit
//!   rate. That is the structural answer to (c), and these two numbers bound its
//!   price.
//!
//! The saving of an early crop is **not** claimed and **not** measured here:
//! this file measures the *unmodified* pipeline only. Measuring the *reordered*
//! one requires the reorder to exist, which `PIPELINE-CROP-EARLY-9` makes
//! conditional on these numbers. Reading a saving off a measurement of unchanged
//! code would be the exact inversion of `DoD.md` §9.
//!
//! Rotation is deliberately `0.0` in every case: this benchmark measures the
//! **selection**, the byte-neutral half. Rotation is a resample and a separate
//! question (`AUTO-TONE-ANALYSIS-INPUT-8`).
//!
//! Lensfun-dependent stages are measured through the `#[cfg(feature =
//! "lensfun")]` helpers; the manual lens model (`lens_stage__*`) is the same
//! path with and without the feature, so the number stays comparable across
//! builds.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use lumina_core::{
    composite_auto_fill, composite_expand, prepare_source_base, render_frame,
    GenerativeCanvasArtifact, GenerativeRole, ImageFrame, MaskContext, MaskPolicy, RenderContext,
    StageFrameCache, StageWork,
};
use lumina_sidecar::{
    Crop, EditRecipe, Extras, GenerativeCanvas, GenerativeEdit, Geometry, LensCorrection,
    Perspective,
};
use std::hint::black_box;

mod common;
use common::{make_frame, make_mask_fixture, make_recipe, SIZES};

/// A `Crop::Free` rectangle leaving `rest_percent` of the area, centred.
///
/// `side = sqrt(rest/100)`, so the remaining area is exactly `rest` percent.
fn recipe_with_crop(rest_percent: u32) -> EditRecipe {
    let mut recipe = make_recipe();
    let side = (rest_percent as f32 / 100.0).sqrt();
    let offset = (1.0 - side) / 2.0;
    recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: offset,
            y: offset,
            width: side,
            height: side,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    recipe
}

/// A crop rect covering the whole frame — the control measurement.
fn recipe_with_full_crop() -> EditRecipe {
    recipe_with_crop(100)
}

fn cropped_render_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("core");

    for &size in SIZES {
        let frame = make_frame(size);
        let fixture = make_mask_fixture(size);

        // Control: full-frame rect. `apply_crop_stage` short-circuits on
        // `(0, 0, width, height)` (lib.rs:641), so this is the uncropped cost
        // plus the crop stage's own bookkeeping.
        let full_recipe = recipe_with_full_crop();
        let full_ctx = RenderContext {
            recipe: &full_recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: Some(MaskContext {
                copies: &fixture.copies,
                active_copy_id: "vc-original",
                planes: fixture.planes.clone(),
                policy: MaskPolicy::Warn,
                source_roi: None,
            }),
            lensfun: None,
            depth: None,
        };
        group.bench_function(format!("render_frame_cropped_full__{size}"), |b| {
            b.iter(|| black_box(render_frame(black_box(&frame), &full_ctx).unwrap()))
        });

        // The real cases: 50 %, 25 %, 10 % remaining area.
        for rest in [50u32, 25, 10] {
            let recipe = recipe_with_crop(rest);
            let ctx = RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(MaskContext {
                    copies: &fixture.copies,
                    active_copy_id: "vc-original",
                    planes: fixture.planes.clone(),
                    policy: MaskPolicy::Warn,
                    source_roi: None,
                }),
                lensfun: None,
                depth: None,
            };
            group.bench_function(format!("render_frame_cropped__{size}__{rest}pct"), |b| {
                b.iter(|| black_box(render_frame(black_box(&frame), &ctx).unwrap()))
            });
        }
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// PIPELINE-ANNASSUNGEN-10 (a): individual geometry-stage cost.
//
// `render_frame` runs the geometry chain `Lens -> AutoFill -> Perspective ->
// Expand -> Crop` on the **full** frame today. The hard lower bound keeps an
// early crop *after* `Expand`; only moving it before `Lens`/`AutoFill` is even
// conceivable without breaking the coordinate contract. The individual stage
// costs decide how much is at stake, so each stage is called **directly** here
// rather than inferred by subtracting two end-to-end numbers (the subtraction
// the previous pass explicitly refused as not isolable).
//
// On the render path (GEN-ONNX-1) AutoFill and Expand are *artifact
// compositing*: `artifact.frame.clone()`. The micro-benchmarks below measure
// exactly that clone — never the retired heuristic BFS, which is not on the
// render path.
// ---------------------------------------------------------------------------

fn lens_fixture() -> LensCorrection {
    LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(0.15),
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: Some(0.2),
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    }
}

fn perspective_fixture() -> Perspective {
    Perspective {
        version: 1,
        vertical: 0.1,
        horizontal: 0.0,
        rotation: 0.05,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    }
}

// `apply_lens_stage`/`apply_perspective_stage` carry a `lensfun` parameter only
// when `lumina-core`'s feature is on — which an outside caller cannot observe
// (in a unified workspace build `lumina-gui`'s default `lensfun` turns it on
// while this crate's own `lensfun` feature stays off). The core-side wrappers
// are cfg-free for the caller and use the manual model, which is what an
// isolated stage cost must measure.
fn run_lens(frame: &mut ImageFrame, lens: &LensCorrection) {
    lumina_core::geometry_stage_api::apply_lens_stage_standalone(frame, Some(lens)).unwrap();
}

fn run_perspective(frame: &mut ImageFrame, perspective: &Perspective) {
    lumina_core::geometry_stage_api::apply_perspective_stage_standalone(
        frame,
        None,
        Some(perspective),
    )
    .unwrap();
}

/// A frame with a transparent border (alpha 0), so `composite_auto_fill`
/// exercises its real clone path instead of the identity short-circuit.
fn frame_with_transparent_border(size: u32) -> ImageFrame {
    let frame = make_frame(size);
    let mut pixels = frame.pixels.clone();
    let border = (size / 8).max(1);
    for y in 0..size {
        for x in 0..size {
            if x < border || y < border || x + border >= size || y + border >= size {
                let i = (y as usize * size as usize + x as usize) * 4;
                pixels[i + 3] = 0;
            }
        }
    }
    ImageFrame::new(size, size, pixels).expect("transparent-border frame keeps geometry")
}

/// Recipe with an active `expand_beyond_image` canvas (source inset by
/// `border`). The canvas only defines the target geometry; the render stage
/// adopts the supplied artifact.
fn expand_recipe(size: u32, border: u32) -> EditRecipe {
    let mut recipe = make_recipe();
    recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(GenerativeCanvas {
            output_width: size + 2 * border,
            output_height: size + 2 * border,
            source_offset_x: border as i32,
            source_offset_y: border as i32,
            extras: Extras::default(),
        }),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: Some(0),
        prompt: None,
        extras: Extras::default(),
    });
    recipe
}

fn geometry_stage_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("core");

    for &size in SIZES {
        let frame = make_frame(size);
        let lens = lens_fixture();
        let perspective = perspective_fixture();

        group.bench_function(format!("lens_stage__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |mut f| {
                    run_lens(&mut f, &lens);
                    black_box(f.pixels[0])
                },
                BatchSize::SmallInput,
            )
        });

        group.bench_function(format!("perspective_stage__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |mut f| {
                    run_perspective(&mut f, &perspective);
                    black_box(f.pixels[0])
                },
                BatchSize::SmallInput,
            )
        });

        let transparent = frame_with_transparent_border(size);
        let autofill_artifact =
            GenerativeCanvasArtifact::new(GenerativeRole::AutoFillTransparent, frame.clone());
        group.bench_function(format!("autofill_stage__{size}"), |b| {
            b.iter_batched(
                || transparent.clone(),
                |f| black_box(composite_auto_fill(f, Some(&autofill_artifact)).unwrap()),
                BatchSize::SmallInput,
            )
        });

        let border = 64u32;
        let expand_recipe = expand_recipe(size, border);
        let expand_artifact = GenerativeCanvasArtifact::new(
            GenerativeRole::Expand,
            ImageFrame::new(
                size + 2 * border,
                size + 2 * border,
                vec![0; ((size + 2 * border) as usize).pow(2) * 4],
            )
            .expect("expand canvas keeps geometry"),
        );
        group.bench_function(format!("expand_stage__{size}"), |b| {
            b.iter_batched(
                || frame.clone(),
                |f| black_box(composite_expand(f, &expand_recipe, Some(&expand_artifact)).unwrap()),
                BatchSize::SmallInput,
            )
        });

        // (c) The cache producer: `prepare_source_base` is `source.clone()` plus
        // source actions — exactly what `StageFrameCache` stores. Its key is
        // recipe-blind, so a crop placed *behind* the cache cannot change the
        // hit rate. This is the miss/build cost.
        group.bench_function(format!("prepare_source_base__{size}"), |b| {
            b.iter(|| {
                let mut work = StageWork::default();
                black_box(prepare_source_base(black_box(&frame), &[], &mut work).unwrap())
            })
        });

        // (c) The cache hit itself: a keyed clone out of `StageFrameCache`,
        // independent of any crop rectangle.
        let mut cache = StageFrameCache::new(1_000_000_000);
        let _ = cache.insert("base", frame.clone());
        group.bench_function(format!("stage_cache_hit__{size}"), |b| {
            b.iter(|| black_box(cache.get("base").unwrap()))
        });
    }

    group.finish();
}

criterion_group!(benches, cropped_render_benches, geometry_stage_benches);
criterion_main!(benches);
