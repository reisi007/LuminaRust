//! LRPAR-G13-MERGE-15 / MERGE-CORE-1: HDR + panorama alignment and merge.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Ausrichtung, Folge-Tasks Nr. 2). This crate is the Mehrbild-Vorlauf
//! before the single-image pipeline: it consumes decoded linear frames
//! plus EXIF exposure values and produces one merged linear frame with a
//! deterministic digest. It performs **no** filesystem or DNG IO (that is
//! MERGE-DNG-1) and touches neither `Pipeline::default()` nor
//! `apply_recipe`.
//!
//! Scope (1.5, loudly enforced):
//!
//! - HDR alignment is translation-only (integer + subpixel-light, step 0.5).
//!   No homography, no ghost removal.
//! - Panorama alignment is a chained translation+rotation-light homography
//!   with cylindrical projection only (no spherical/fisheye), no bundle
//!   adjustment, feather blend only (no multi-band).
//! - HDR exposure compensation uses EXIF exposure exclusively
//!   (`exposure_time_s`, `iso`, `f_number`); missing EXIF exposure is
//!   [`MergeError::Unsupported`], never guessed from pixels.
//! - Non-overlapping / non-chainable sets are [`MergeError::Unsupported`].
//! - A shift above [`HDR_SHIFT_WARN_PX`] yields
//!   [`AlignStatus::AlignedWithResidual`] (pixel measure), never a silent
//!   abort or silent crop.
//!
//! Determinism: identical inputs (pixels, exposures, parameters) produce
//! bitwise-identical outputs on the same build. Floating-point arithmetic
//! (`f32`/`f64`) may differ across platforms toolchains by at most
//! [`FLOAT_TOLERANCE_DOC`] (`1e-6`); golden tests therefore compare with
//! tolerance instead of byte identity where documented. No randomness,
//! no wall-clock, no threads.
//!
//! Native dependencies: none. Classical image processing only, no ONNX,
//! no model (capability note: pure Rust, always available on native CLI
//! and desktop; no capability gate).

pub mod align;
pub mod digest;
pub mod dng;
pub mod image;
pub mod merge;

use thiserror::Error;

/// Merge-recipe version consumed by this crate (mirrors
/// `lumina_sidecar::merge_recipe::MERGE_RECIPE_VERSION`).
pub const MERGE_VERSION: u32 = 1;
/// HDR shift magnitude (px) above which alignment reports
/// [`AlignStatus::AlignedWithResidual`] instead of [`AlignStatus::Aligned`].
/// Documented warning threshold, not a silent abort: the merge still runs.
pub const HDR_SHIFT_WARN_PX: f64 = 16.0;
/// Panorama overlap below this width (px) is treated as non-overlapping
/// ([`MergeError::Unsupported`]), never as a silent partial panorama.
pub const PANO_MIN_OVERLAP_PX: u32 = 8;
/// Documented float tolerance for cross-platform output comparison.
/// Same-machine runs are bitwise deterministic; cross-toolchain `f32`
/// accumulation may differ up to this bound.
pub const FLOAT_TOLERANCE_DOC: f32 = 1e-6;

/// Loud merge failure: `Unsupported` is a scope/limit state (never a
/// silent fallback), `Invalid` is a caller contract violation.
#[derive(Debug, Error, PartialEq)]
pub enum MergeError {
    #[error("merge unsupported: {0}")]
    Unsupported(String),
    #[error("invalid merge input: {0}")]
    Invalid(String),
}

/// Alignment outcome. The residual is always a pixel measure; the caller
/// persists it as `residual_px` in the merge recipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlignStatus {
    Aligned,
    AlignedWithResidual,
}

impl AlignStatus {
    #[must_use]
    pub fn residual_flag(self) -> bool {
        matches!(self, AlignStatus::AlignedWithResidual)
    }
}

pub use align::{estimate_hdr_translation, estimate_pano_transform, HdrShift, PanoTransform};
pub use digest::merge_inputs_digest;
pub use dng::{
    encode_linear_dng, exif_timestamp_utc, linear_to_u16, merge_dng_filename,
    validate_dng_file_name, write_merge_dng, DngError, DngExif, DNG_BITS_PER_SAMPLE,
    DNG_MAX_DIMENSION_PX, DNG_MIN_DIMENSION_PX, DNG_PHOTOMETRIC_LINEAR_RAW, DNG_SOFTWARE,
    DNG_VERSION, HDR_DNG_SUFFIX, PANO_DNG_SUFFIX,
};
pub use image::LinearImage;
pub use merge::{blend_panorama, merge_hdr_weighted};
