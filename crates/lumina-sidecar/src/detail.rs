//! MASK-LOCAL-P1.2d: the mask-local **detail** block type.
//!
//! This module owns all three detail types: the two **global** stages
//! ([`Sharpening`] / [`NoiseReduction`]) and the mask-local [`Detail`] container.
//! The container is not a dialect: both sub-blocks are literally the global
//! types, validated by the shared detail validators
//! ([`crate::validate_sharpening`] / [`crate::validate_noise_reduction`]) with the
//! unchanged global ranges.
//!
//! The sub-blocks are independent, and both are optional, so a layer can carry
//! only one of the two detail stages. The canonical order inside a layer is
//! noise reduction **before** sharpening, exactly as the global kernel runs them.

use crate::SidecarError;
use serde::{Deserialize, Serialize};

/// F-096: the global noise-reduction stage. `luminance`/`color` are the two
/// `0..=1` strengths of the 5x5 bilateral kernel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoiseReduction {
    pub version: u8,
    pub luminance: f32,
    pub color: f32,
}

/// F-095: the global sharpening stage. `amount` is `0..=3`, `radius` is
/// `0.1..=10.0` (scaled by the global render scale), `detail` is the fine/coarse
/// mix `0..=1` and `masking` is the flat-area suppression `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sharpening {
    pub version: u8,
    pub amount: f32,
    pub radius: f32,
    pub detail: f32,
    pub masking: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Detail {
    /// The global F-095/F-096 sharpening stage, applied after noise reduction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpening: Option<Sharpening>,
    /// The global F-096 noise-reduction stage, applied before sharpening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_reduction: Option<NoiseReduction>,
}

impl Detail {
    /// Validate both sub-blocks with exactly the validators the global recipe
    /// uses. A neutral value is *not* refused: a sub-block whose stored values
    /// cannot change a pixel is a legal, pixel-neutral state that a reset
    /// removes.
    pub fn validate(&self) -> Result<(), SidecarError> {
        if let Some(sharpening) = &self.sharpening {
            crate::validate_sharpening(sharpening)?;
        }
        if let Some(noise) = &self.noise_reduction {
            crate::validate_noise_reduction(noise)?;
        }
        Ok(())
    }

    /// True when neither sub-block can change a pixel. This is the routing and
    /// kernel-path predicate, and it is a pure function of the persisted block —
    /// never of the mask and never of the render scale.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.sharpening
            .as_ref()
            .is_none_or(crate::sharpening_is_neutral)
            && self
                .noise_reduction
                .as_ref()
                .is_none_or(crate::noise_reduction_is_neutral)
    }
}
