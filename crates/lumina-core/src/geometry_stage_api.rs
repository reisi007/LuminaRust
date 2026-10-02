//! Cfg-free entry points for the isolated geometry stages.
//!
//! [`ImageFrame::apply_lens_stage`] and [`ImageFrame::apply_perspective_stage`]
//! carry an extra `Option<&Corrector>` argument **only** when this crate's
//! `lensfun` feature is on. An outside caller (a benchmark crate) cannot
//! observe that feature, so branching on the **caller's own** `lensfun` feature
//! is wrong: in a unified workspace build `lumina-gui`'s default
//! `default = ["lensfun", "gpu"]` switches the argument on while the caller's
//! feature stays off, and `cargo check --workspace --all-targets` (the CI gate)
//! then fails to compile the bench with `this method takes 2 arguments but 1
//! argument was supplied`.
//!
//! The wrappers below have a **stable signature in both feature configurations**
//! and always use the **manual** model (no Lensfun corrector) — which is exactly
//! what an isolated stage-cost measurement must measure.

use crate::{CoreError, ImageFrame};

/// Manual-model lens stage (distortion + vignette), no Lensfun corrector.
pub fn apply_lens_stage_standalone(
    frame: &mut ImageFrame,
    lens: Option<&lumina_sidecar::LensCorrection>,
) -> Result<(), CoreError> {
    frame.apply_lens_stage(
        lens,
        #[cfg(feature = "lensfun")]
        None,
    )
}

/// Manual-model perspective stage (perspective + CA), no Lensfun corrector.
///
/// The `lens` argument carries the channel-aberration model of the lens stage,
/// mirroring [`apply_perspective_stage`](ImageFrame::apply_perspective_stage).
/// Pass `None` to measure the perspective stage alone.
pub fn apply_perspective_stage_standalone(
    frame: &mut ImageFrame,
    lens: Option<&lumina_sidecar::LensCorrection>,
    perspective: Option<&lumina_sidecar::Perspective>,
) -> Result<(), CoreError> {
    frame.apply_perspective_stage(
        lens,
        perspective,
        #[cfg(feature = "lensfun")]
        None,
    )
}

// ---------------------------------------------------------------------------
// Stage validation (moved verbatim out of `lib.rs`; used only by this crate).
// ---------------------------------------------------------------------------

pub(crate) fn validate_lens(l: &lumina_sidecar::LensCorrection) -> Result<(), CoreError> {
    if l.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "lens_correction.version".into(),
            value: l.version as f64,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    if let Some(profile) = l.profile.as_deref() {
        if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
            return Err(CoreError::UnsupportedAdjustment {
                key: format!("lens profile `{profile}`"),
            });
        }
    }
    for (name, v, lo, hi) in [
        ("distortion_k1", l.distortion_k1, -1., 1.),
        ("distortion_k2", l.distortion_k2, -1., 1.),
        ("distortion_k3", l.distortion_k3, -1., 1.),
        ("vignette_c0", l.vignette_c0, -1., 1.),
        ("vignette_c1", l.vignette_c1, -1., 1.),
        ("vignette_c2", l.vignette_c2, -1., 1.),
        ("ca_red", l.ca_red, -0.05, 0.05),
        ("ca_blue", l.ca_blue, -0.05, 0.05),
    ]
    .into_iter()
    .filter_map(|(name, value, lo, hi)| value.map(|v| (name, v, lo, hi)))
    {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(CoreError::InvalidAdjustment {
                name: name.into(),
                value: v as f64,
                minimum: lo as f64,
                maximum: hi as f64,
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_perspective(p: &lumina_sidecar::Perspective) -> Result<(), CoreError> {
    if p.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective.version".into(),
            value: p.version as f64,
            minimum: 1.,
            maximum: 1.,
        });
    }
    for (name, v, lo, hi) in [
        ("vertical", p.vertical, -1., 1.),
        ("horizontal", p.horizontal, -1., 1.),
        ("rotation", p.rotation, -1., 1.),
        ("shift_x", p.shift_x, -1., 1.),
        ("shift_y", p.shift_y, -1., 1.),
        ("scale", p.scale, 0.1, 10.),
        ("aspect_ratio", p.aspect_ratio, 0.1, 10.),
    ] {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(CoreError::InvalidAdjustment {
                name: name.into(),
                value: v as f64,
                minimum: lo as f64,
                maximum: hi as f64,
            });
        }
    }
    Ok(())
}
