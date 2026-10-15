use super::merge_hdr_weighted_deghost;
use super::HdrDeghostMode;
use crate::bundle::{map_merge_error, MergePass, MergeRunError};
use crate::{estimate_hdr_translation, LinearImage};
use lumina_sidecar::{MergeExposure, MergeTransform};

/// The HDR merge pass: align (translation-only shifts), then merge — with the
/// Slice-B mask when a threshold is set, exactly as before otherwise. Moved
/// from `bundle.rs` (`align_and_merge`, HDR arm) unchanged apart from the
/// deghosting parameter.
pub(crate) fn align_and_merge_hdr(
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    max_shift_px: i32,
    deghost: HdrDeghostMode,
) -> Result<MergePass, MergeRunError> {
    let mut shifts = Vec::with_capacity(frames.len());
    let mut transforms = Vec::new();
    let mut residual_px: f64 = 0.0;
    let mut residual_warning = false;
    shifts.push((0.0, 0.0));
    for (index, frame) in frames.iter().enumerate().skip(1) {
        let shift =
            estimate_hdr_translation(&frames[0], frame, max_shift_px).map_err(map_merge_error)?;
        shifts.push((shift.dx, shift.dy));
        transforms.push(MergeTransform {
            source_index: index,
            matrix_3x3: [1.0, 0.0, shift.dx, 0.0, 1.0, shift.dy, 0.0, 0.0, 1.0],
        });
        residual_px = residual_px.max(shift.residual_px);
        residual_warning |= shift.status.residual_flag();
    }
    let merged =
        merge_hdr_weighted_deghost(frames, exposures, &shifts, deghost.recipe_value().as_ref())
            .map_err(map_merge_error)?;
    Ok(MergePass {
        merged,
        transforms,
        residual_px,
        residual_warning,
        gain_compensation: None,
    })
}
