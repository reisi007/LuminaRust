//! Slice B (deghosting): the alignment half of the merge-recipe validation,
//! extracted from `merge_recipe.rs` in the same move that added the
//! `deghost` field (file-size ratchet).
//!
//! What lives here: the three alignment validators — residual/blend/transform
//! ranges plus the two mode-specific blocks (the Slice-A gain compensation
//! and the Slice-B deghosting). They are grouped because they are one
//! question ("does this alignment belong to this recipe?") and because every
//! new alignment option lands in `validate_merge_alignment`, which is exactly
//! what pushed `merge_recipe.rs` over its size entry.
//!
//! Nothing here is a second implementation: the range constants, the schema
//! types and the gain/deghost values live in `merge_recipe`, `merge_gain` and
//! `merge_deghost`; this module only decides loudly what is allowed.

use crate::merge_recipe::{
    invalid, MergeAlignment, MergeTransform, MAX_MERGE_BLEND_WIDTH_PX, MAX_MERGE_RESIDUAL_PX,
};
use crate::{MergeAlignmentMethod, MergeMode, MergeProjection, SidecarError};

pub(crate) fn validate_merge_alignment(
    alignment: &MergeAlignment,
    source_count: usize,
    mode: MergeMode,
) -> Result<(), SidecarError> {
    if !alignment.residual_px.is_finite()
        || alignment.residual_px < 0.0
        || alignment.residual_px > MAX_MERGE_RESIDUAL_PX
    {
        return invalid(format!(
            "merge residual_px must be finite within 0..={MAX_MERGE_RESIDUAL_PX}, got {}",
            alignment.residual_px
        ));
    }
    if alignment.blend_width_px > MAX_MERGE_BLEND_WIDTH_PX {
        return invalid(format!(
            "merge blend_width_px must be within 0..={MAX_MERGE_BLEND_WIDTH_PX}, got {}",
            alignment.blend_width_px
        ));
    }
    for (index, transform) in alignment.transforms.iter().enumerate() {
        if transform.matrix_3x3.iter().any(|v| !v.is_finite()) {
            return invalid(format!(
                "merge transform #{index} matrix_3x3 must be finite"
            ));
        }
    }
    crate::merge_gain::validate_pano_gain_compensation(
        alignment.gain_compensation.as_ref(),
        source_count,
        mode,
    )?;
    crate::merge_deghost::validate_hdr_deghost(alignment.deghost.as_ref(), source_count, mode)?;
    Ok(())
}

/// 1.5 scope pairing: HDR is translation-only without projection, panorama
/// is the chained cylindrical homography. Anything else is rejected loudly
/// (no silent reinterpretation of a foreign pipeline).
pub(crate) fn validate_mode_alignment(
    mode: MergeMode,
    alignment: &MergeAlignment,
) -> Result<(), SidecarError> {
    let (expected_method, expected_projection) = match mode {
        MergeMode::Hdr => (MergeAlignmentMethod::HdrTranslate, MergeProjection::None),
        MergeMode::Panorama => (
            MergeAlignmentMethod::PanoCylindricalHomography,
            MergeProjection::Cylindrical,
        ),
    };
    if alignment.method != expected_method {
        return invalid(format!(
            "merge mode `{mode:?}` requires method `{expected_method:?}`, got `{:?}`",
            alignment.method
        ));
    }
    if alignment.projection != expected_projection {
        return invalid(format!(
            "merge mode `{mode:?}` requires projection `{expected_projection:?}`, got `{:?}`",
            alignment.projection
        ));
    }
    Ok(())
}

pub(crate) fn validate_merge_transforms(
    transforms: &[MergeTransform],
    source_count: usize,
) -> Result<(), SidecarError> {
    if transforms.len() > source_count {
        return invalid(format!(
            "merge has more transforms ({}) than sources ({source_count})",
            transforms.len()
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for (index, transform) in transforms.iter().enumerate() {
        if transform.source_index >= source_count {
            return invalid(format!(
                "merge transform #{index} references source {} of {source_count}",
                transform.source_index
            ));
        }
        if !seen.insert(transform.source_index) {
            return invalid(format!(
                "duplicate merge transform for source {}",
                transform.source_index
            ));
        }
    }
    Ok(())
}
