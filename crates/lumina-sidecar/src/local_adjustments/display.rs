//! The stable human-readable status line of a typed local recipe.
//!
//! Split out of `local_adjustments.rs` (file-size ratchet): the status line is a
//! *presentation* contract, not a schema contract, so it lives beside the type
//! instead of inside it.
//!
//! The documented grammar is
//!
//! ```text
//! v6 exposure=<number> contrast=<number> highlights=<number> shadows=<number>
//!   temperature_delta_k=<number> tint_delta=<number> curves=<summary>
//!   hsl=<summary> point_color=<summary> color_grading=<summary>
//!   vibrance=<number> saturation=<number> presence=<summary> detail=<summary>
//! ```
//!
//! always in exactly that field order. The `curves` summary is `curves=none` for
//! a neutral block, otherwise `curves=<channel>:<point-count>[,...]` in the
//! canonical master/red/green/blue order, listing only stored channels. The colour
//! summaries use the same `none` convention, and so does `presence`. The `detail`
//! summary is `none` when the block cannot change a pixel and otherwise the
//! non-neutral sub-block names joined with `+` in the canonical
//! `sharpening`/`noise_reduction` order. It is a presentation contract
//! independent of the derived `Debug` layout; JSON consumers should continue to
//! use the structured object instead.

use super::LocalAdjustments;
use std::fmt;

impl fmt::Display for LocalAdjustments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v{} exposure={} contrast={} highlights={} shadows={} temperature_delta_k={} tint_delta={} curves={} hsl={} point_color={} color_grading={} vibrance={} saturation={} presence={} detail={}",
            self.version,
            self.exposure,
            self.contrast,
            self.highlights,
            self.shadows,
            self.temperature_delta_k,
            self.tint_delta,
            self.curve_summary(),
            self.hsl_summary(),
            self.point_color_summary(),
            self.color_grading_summary(),
            self.vibrance,
            self.saturation,
            self.presence_summary(),
            self.detail_summary(),
        )
    }
}
