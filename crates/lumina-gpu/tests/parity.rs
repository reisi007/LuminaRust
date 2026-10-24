//! GPU-RENDER-PARITY-1 parity harness: the post-tone GPU stages (per-pixel
//! color + Presence) against the CPU oracle (`lumina_core::render_frame`).
//!
//! Each implemented stage gets a recipe that drives it at a non-neutral value;
//! the test asserts
//!
//! 1. [`unsupported_gpu_stages`] is **empty** (the stage is genuinely GPU-
//!    renderable, not silently CPU-routed), and
//! 2. the GPU output matches the CPU oracle at that stage's **strongest
//!    measured property** ([`Equivalence`]): stages measured byte-identical on
//!    both frames (Point Color, Color Grading, neutral vibrance/saturation,
//!    Presence Clarity and positive Dehaze) assert `maxAbsDiff == 0`; the rest
//!    assert their measured bound (≤ 1, or ≤ 2 for the fully stacked recipe)
//!    plus a structural PSNR floor and a **mean-signed-error bias bound** so the
//!    tolerance cannot hide a systematic tilt. The bound is not loosened to fit
//!    the data — the prose in `lib.rs` and here states exactly what is asserted.
//!
//! The residuals come from the oracle evaluating the curves ratio in `f64`
//! (the GPU is `f32`) and from the Metal backend rounding one ulp differently
//! at a `round()` tie.
//!
//! Without a bound adapter the hardware checks are skipped loudly, never
//! failed (matching `golden.rs`); the validator assertions still run.
#![cfg(feature = "gpu")]
mod support;

use lumina_core::{
    render_frame, CoreError, DepthPlane, ImageFrame, MaskPlane, RenderContext, SourceActionArtifact,
};
use lumina_gpu::{
    unsupported_gpu_stages, unsupported_gpu_stages_for, unsupported_gpu_stages_with_context,
    GpuContext, GpuError, MAX_SOURCE_ACTIONS,
};
use lumina_sidecar::{
    AnalysisFingerprint, AspectPreset, BokehShape, ColorGrading, ColorGradingRange, Crop,
    CurveChannels, Curves, DenoiseAi, DenoiseModelIdentity, EditRecipe, Effects, FocusRect,
    GenerativeCanvas, GenerativeEdit, Geometry, Grain, HslAdjustments, LensBlur, LensCorrection,
    NoiseReduction, Perspective, PointColor, PointColorEntry, Presence, RedEyeCorrection,
    RedEyeRegion, Sharpening, SourceActionArtifactRef, SourceActionKind, SourceActionSpec,
    SpotRemoval, SpotRemovalMode, Upright, UprightAnalysis, Vignette, DENOISE_AI_VERSION,
    SOURCE_ACTION_VERSION, ZDATA_FILE_SUFFIX,
};
use std::collections::BTreeMap;

mod parity_depth;
mod parity_generative;
mod parity_source_action;
// SPLICE-GPU-PARITY-92 (Slice 6): der Lensfun-Paritaetsblock (GPU-LENSFUN-PARITY-1)
// ist als reiner Move in `parity_lensfun/` ausgelagert; das `lensfun`-Feature
// steuert die Modul-Deklaration, ohne das Feature existiert das Modul nicht.
#[cfg(feature = "lensfun")]
mod parity_lensfun;
mod parity_support;
use parity_support::*;

fn entry(id: &str, center: f32, range: f32, hue: f32, sat: f32, lum: f32) -> PointColorEntry {
    PointColorEntry {
        id: id.into(),
        hue_center: center,
        hue_range: range,
        hue_shift: hue,
        saturation_shift: sat,
        luminance_shift: lum,
    }
}

fn range(hue: f32, saturation: f32, luminance: f32) -> ColorGradingRange {
    ColorGradingRange {
        hue_degrees: hue,
        saturation,
        luminance,
    }
}

mod parity_recipes;
use parity_recipes::*;
mod parity_fixtures;
use parity_fixtures::*;
mod parity_tone_tests;
mod parity_vram_tests;
