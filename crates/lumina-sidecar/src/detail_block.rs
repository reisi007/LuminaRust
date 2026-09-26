//! MASK-LOCAL-P1.2d: the one validator for the two global detail blocks,
//! [`Sharpening`] and [`NoiseReduction`].
//!
//! The global recipe (`EditRecipe::sharpening` / `EditRecipe::noise_reduction`,
//! the `adjustments`-nested `sharpening` / `noise_reduction` objects) and the
//! typed mask-local recipe (`local_adjustments.detail`) store the *same* blocks
//! and must accept exactly the same values. Keeping the rules in one place is
//! what makes that a structural property instead of two validators that happen
//! to agree today: a local detail block can never accept a value the global
//! pipeline would reject, or the other way round. The rules — and the exact
//! error strings — are unchanged from the previous inline
//! `validate_adjustments` checks; this is an extraction, not a relaxation.

use crate::{NoiseReduction, Sharpening, SidecarError};

fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}

/// One sharpening field: its stable name plus the selector of the persisted
/// block, as a tuple so a new field cannot be added without also being
/// validated and range-checked here.
pub type SharpeningField = (&'static str, fn(&Sharpening) -> f32, f64, f64);

/// One noise-reduction field: its stable name, the selector of the persisted
/// block and its inclusive range.
pub type NoiseReductionField = (&'static str, fn(&NoiseReduction) -> f32, f64, f64);

/// The three sharpening fields in their canonical order. Kept as one list so a
/// new field cannot be added to the type without also being validated and
/// range-checked here.
///
/// The ranges are the **global** F-095/F-096 ranges, written as `f64` bounds and
/// compared against the widened `f32` field, so there is exactly one place that
/// documents them instead of a literal per validator and per setter. The radius
/// bounds `0.1` and `10.0` are inclusive and are pinned as exact values.
pub const SHARPENING_FIELDS: [SharpeningField; 4] = [
    ("amount", |s| s.amount, 0.0, 3.0),
    ("radius", |s| s.radius, 0.1, 10.0),
    ("detail", |s| s.detail, 0.0, 1.0),
    ("masking", |s| s.masking, 0.0, 1.0),
];

/// The inclusive sharpening radius bounds, read from the one field table so
/// there is no second copy of the numbers. Callers that want to document or pin
/// the two legal boundary values use this instead of re-typing `0.1` and `10.0`.
#[must_use]
pub fn sharpening_radius_range() -> (f64, f64) {
    let (_, _, low, high) = SHARPENING_FIELDS[1];
    (low, high)
}

/// The inclusive range of one named sharpening field.
#[must_use]
pub fn sharpening_field_range(name: &str) -> Option<(f64, f64)> {
    SHARPENING_FIELDS
        .iter()
        .find(|(field, _, _, _)| *field == name)
        .map(|(_, _, low, high)| (*low, *high))
}

/// The inclusive range of one named noise-reduction field.
#[must_use]
pub fn noise_reduction_field_range(name: &str) -> Option<(f64, f64)> {
    NOISE_REDUCTION_FIELDS
        .iter()
        .find(|(field, _, _, _)| *field == name)
        .map(|(_, _, low, high)| (*low, *high))
}

/// One noise-reduction field: its stable name, the selector of the persisted
/// block and its inclusive range. Both fields share the same range, so it is one
/// constant.
pub const NOISE_REDUCTION_FIELDS: [NoiseReductionField; 2] = [
    ("luminance", |n| n.luminance, 0.0, 1.0),
    ("color", |n| n.color, 0.0, 1.0),
];

/// The block version both detail blocks must carry. A different version is
/// refused loudly, never interpreted best-effort.
pub const DETAIL_BLOCK_VERSION: u8 = 1;

/// True when `value` is a legal value for **every** detail field, i.e. finite
/// and inside the narrowest detail range. Used by the block validators and by
/// every local setter, so a local field can never be written with a value the
/// validator would refuse.
#[must_use]
pub fn detail_amount_is_valid(value: f64) -> bool {
    value.is_finite()
        && (NOISE_REDUCTION_FIELDS[0].2..=NOISE_REDUCTION_FIELDS[0].3).contains(&value)
}

/// Validate a whole [`Sharpening`] block: the block version plus the four
/// amount fields. Shared verbatim by the global and the mask-local recipe — see
/// the module docs.
pub fn validate_sharpening(sharpening: &Sharpening) -> Result<(), SidecarError> {
    if sharpening.version != DETAIL_BLOCK_VERSION {
        return invalid("unsupported sharpening version");
    }
    for (name, select, low, high) in SHARPENING_FIELDS {
        let value = f64::from(select(sharpening));
        if !value.is_finite() || !(low..=high).contains(&value) {
            return invalid(format!("invalid sharpening {name}"));
        }
    }
    Ok(())
}

/// Validate a whole [`NoiseReduction`] block: the block version plus the two
/// `0..=1` strengths. Shared verbatim by the global and the mask-local recipe —
/// see the module docs.
pub fn validate_noise_reduction(noise: &NoiseReduction) -> Result<(), SidecarError> {
    if noise.version != DETAIL_BLOCK_VERSION {
        return invalid("unsupported noise_reduction version");
    }
    for (name, select, low, high) in NOISE_REDUCTION_FIELDS {
        let value = f64::from(select(noise));
        if !value.is_finite() || !(low..=high).contains(&value) {
            return invalid(format!("invalid noise_reduction {name}"));
        }
    }
    Ok(())
}

/// True when a stored sharpening block cannot change a pixel.
///
/// This is the *kernel-path* predicate, so it deliberately asks **one** question:
/// is the amount zero? That is the global F-095 stage's own `amount == 0.0`
/// early return, and it is the only way a sharpening block can be a no-op.
///
/// Two things follow from that, and both are load-bearing:
///
/// * `masking = 0` is **not** neutral. Masking is only the flat-area
///   suppression multiplier `((1−masking) + masking·edge)`; at `masking = 0` the
///   factor is `1` for *every* pixel, which is the strongest possible setting —
///   the whole frame is sharpened, flat areas included. A block with
///   `masking = 0` and a non-zero `amount` therefore must never be dropped as a
///   silent no-op.
/// * The radius never makes the block neutral. The global kernel floors the
///   Gaussian sigma at `0.5`, and even at that floor the first off-centre tap
///   has weight `exp(−1 / (2·0.5²)) = exp(−2) ≈ 0.135 ≠ 0`, so the separable
///   kernel still reaches the neighbours. Every radius in the legal
///   `0.1..=10.0` range therefore sharpens something.
#[must_use]
pub fn sharpening_is_neutral(sharpening: &Sharpening) -> bool {
    sharpening.amount == 0.0
}

/// True when a stored noise-reduction block cannot change a pixel: both
/// strengths at zero, exactly the global F-096 stage's own early return.
#[must_use]
pub fn noise_reduction_is_neutral(noise: &NoiseReduction) -> bool {
    NOISE_REDUCTION_FIELDS
        .iter()
        .all(|(_, select, _, _)| select(noise) == 0.0)
}
