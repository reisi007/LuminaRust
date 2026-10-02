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
//! The saving of an early crop is **not** claimed and **not** measured here:
//! this file measures the *unmodified* pipeline only. Measuring the *reordered*
//! one requires the reorder to exist, which `PIPELINE-CROP-EARLY-9` makes
//! conditional on these numbers. Reading a saving off a measurement of unchanged
//! code would be the exact inversion of `DoD.md` §9.
//!
//! Rotation is deliberately `0.0` in every case: this benchmark measures the
//! **selection**, the byte-neutral half. Rotation is a resample and a separate
//! question (`AUTO-TONE-ANALYSIS-INPUT-8`).

use criterion::{criterion_group, criterion_main, Criterion};
use lumina_core::{render_frame, MaskContext, MaskPolicy, RenderContext};
use lumina_sidecar::{Crop, EditRecipe, Geometry};
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

criterion_group!(benches, cropped_render_benches);
criterion_main!(benches);
