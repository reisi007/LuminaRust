//! GUI-LIB-SPLIT-65 Slice 10: the parametric tone-curve region math —
//! Lightroom's four regions (Shadows, Darks, Lights, Highlights) converted to
//! and from the persisted `CurvePoint` list — plus the identity-curve and
//! roundtrip-loss helpers.
//!
//! Extracted from `lib.rs` (file-size ratchet). One responsibility — the
//! conversion between the GUI's four region deltas and the normative `[0,1]`
//! `Curves` point list — with no GUI behaviour change. The items are
//! re-exported at the crate root so the Tone Curve panel, the recipe helpers
//! and the headless tests keep calling them unqualified.

use lumina_sidecar::{CurveChannels, CurvePoint, Curves, EditRecipe};

/// The two-point identity curve `[(0,0),(1,1)]`.
pub(crate) fn identity_curve_points() -> Vec<CurvePoint> {
    vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ]
}

/// The four Lightroom parametric tone-curve regions (Shadows, Darks, Lights,
/// Highlights) of one curve channel as the GUI's source of truth. They are
/// persisted as that channel's point list via [`build_tone_curve_points`];
/// the read-back keeps the slider values stable for typical (unclamped)
/// adjustments. A channel without an explicit list reads as all-zero deltas.
pub(crate) fn tone_curve_channel_regions(
    recipe: &EditRecipe,
    channel: &str,
) -> (f64, f64, f64, f64) {
    let points: &[CurvePoint] = match channel {
        "red" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.red.as_deref())
            .unwrap_or(&[]),
        "green" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.green.as_deref())
            .unwrap_or(&[]),
        "blue" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.blue.as_deref())
            .unwrap_or(&[]),
        _ => recipe
            .curves
            .as_ref()
            .map(|c| c.master.as_slice())
            .unwrap_or(&[]),
    };
    tone_curve_regions_from_points(points)
}

/// Master-channel regions (backwards-compatible wrapper over
/// [`tone_curve_channel_regions`]; test-owned, the panel binds channels).
#[cfg(test)]
pub(crate) fn tone_curve_regions(recipe: &EditRecipe) -> (f64, f64, f64, f64) {
    tone_curve_channel_regions(recipe, "master")
}

/// Read-back of the four region deltas from stored curve points
/// (REVIEW-GUI-CURVE-1): kept separate so the roundtrip-loss detection can
/// evaluate the exact same math without constructing an [`EditRecipe`].
fn tone_curve_regions_from_points(points: &[CurvePoint]) -> (f64, f64, f64, f64) {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let mut out = (0.0f64, 0.0, 0.0, 0.0);
    let vals: [f64; 4] = std::array::from_fn(|i| {
        let bx = base[i];
        let out_v = points.get(i).map(|p| p.output as f64).unwrap_or(bx);
        (out_v - bx).clamp(-1.0, 1.0)
    });
    out.0 = vals[0];
    out.1 = vals[1];
    out.2 = vals[2];
    out.3 = vals[3];
    out
}

/// Persist the four region values as a [`Curves`] point list.  Outputs
/// stay in `[0,1]` so the render pipeline never sees an out-of-range control
/// point; extreme region values are clamped (a documented MVP simplification).
pub(crate) fn build_tone_curve_points(
    shadows: f64,
    darks: f64,
    lights: f64,
    highlights: f64,
) -> Vec<CurvePoint> {
    let base: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let deltas = [shadows, darks, lights, highlights];
    base.iter()
        .zip(deltas.iter())
        .map(|(bx, d)| CurvePoint {
            input: *bx as f32,
            output: ((bx + d).clamp(0.0, 1.0)) as f32,
        })
        .collect()
}

/// Persist the four region values as a master [`Curves`] point list (see
/// [`build_tone_curve_points`]).
fn build_tone_curve(shadows: f64, darks: f64, lights: f64, highlights: f64) -> Curves {
    Curves {
        version: 1,
        master: build_tone_curve_points(shadows, darks, lights, highlights),
        channels: CurveChannels::default(),
    }
}

/// REVIEW-GUI-CURVE-1: true when building the master curve from the four
/// region deltas loses part of a delta because the clamped `[0,1]` output
/// absorbs it — most visibly Shadows, whose base point is `0.0`, so any
/// negative delta clamps to "no change" and the slider would snap back to 0.
/// Storing raw deltas would be a recipe-schema change (`Curves` outputs are
/// normatively `[0,1]`), so the GUI instead surfaces this limit explicitly in
/// the UI instead of letting the slider move silently.
///
/// The comparison carries an epsilon because outputs are stored as `f32`:
/// read-back noise (~1e-7) must not be reported as clamp loss; real losses
/// are multiples of the slider step (≥1e-2).
pub(crate) fn tone_curve_roundtrip_is_lossy(
    shadows: f64,
    darks: f64,
    lights: f64,
    highlights: f64,
) -> bool {
    const EPSILON: f64 = 1e-3;
    let curve = build_tone_curve(shadows, darks, lights, highlights);
    let (rs, rd, rl, rh) = tone_curve_regions_from_points(&curve.master);
    (rs - shadows).abs() > EPSILON
        || (rd - darks).abs() > EPSILON
        || (rl - lights).abs() > EPSILON
        || (rh - highlights).abs() > EPSILON
}
