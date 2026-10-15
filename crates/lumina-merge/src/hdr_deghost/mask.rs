/// Radiance floor of the relative deviation (`SOLL B.2`, suggestion `1e-3`):
/// the denominator of the deviation, so a black reference cannot divide by
/// zero. Documented, never guessed per pixel.
pub const DEGHOST_EPS: f64 = 1e-3;

/// The pure ghost decision for ONE output pixel (`SOLL B.2`, steps 2–3).
///
/// `radiances[i]` is frame `i`'s radiance **triple** at this pixel — already
/// shifted (via `shifts_px`) and normalised by its relative exposure — and
/// `radiances[0]` is the reference. A frame is a ghost here when the relative
/// deviation
///
/// ```text
/// |radiance_i[c] - radiance_0[c]| / max(radiance_0[c], DEGHOST_EPS)
/// ```
///
/// exceeds `threshold` in **at least one** channel `c`. Channels are not
/// averaged: a colour ghost (a red car in front of a grey wall) would
/// disappear in a luminance mean. The reference frame itself is never its own
/// ghost (its deviation is exactly 0), and an unmeasurable (non-finite)
/// deviation counts as a ghost — "cannot measure" is not "agrees".
///
/// Deterministic and total: no sort, no median, no randomness, one `f64`
/// comparison per channel.
#[must_use]
pub fn deghost_mask(radiances: &[[f64; 3]], threshold: f64) -> Vec<bool> {
    let reference = radiances.first().copied().unwrap_or([0.0; 3]);
    radiances
        .iter()
        .map(|radiance| {
            (0..3).any(|channel| {
                let deviation = (radiance[channel] - reference[channel]).abs()
                    / reference[channel].max(DEGHOST_EPS);
                !deviation.is_finite() || deviation > threshold
            })
        })
        .collect()
}
