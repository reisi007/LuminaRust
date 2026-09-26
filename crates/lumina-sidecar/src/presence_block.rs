//! MASK-LOCAL-P1.2c: the one validator for the [`Presence`] block.
//!
//! The global recipe (`EditRecipe::presence`, the `adjustments`-nested
//! `presence` object) and the typed mask-local recipe
//! (`local_adjustments.presence`) store the *same* `Presence` block and must
//! accept exactly the same values. Keeping the rules in one place is what makes
//! that a structural property instead of two validators that happen to agree
//! today: a local block can never accept a value the global pipeline would
//! reject, or the other way round. The rules are unchanged from the previous
//! inline `validate_nested_adjustments` checks — this is an extraction, not a
//! relaxation.

use crate::{Presence, SidecarError};

fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}

/// One presence field: its stable name plus the selector of the persisted
/// block, so a new field cannot be added to the type without also being
/// validated and range-checked here.
pub type PresenceField = (&'static str, fn(&Presence) -> f32);

/// The three presence fields in their canonical order. Kept as one list so a
/// new field cannot be added to the type without also being validated and
/// range-checked here.
pub const PRESENCE_FIELDS: [PresenceField; 3] = [
    ("texture", |p| p.texture),
    ("clarity", |p| p.clarity),
    ("dehaze", |p| p.dehaze),
];

/// The inclusive range every presence field must satisfy. Written as `f64`
/// bounds and widened to the `f32` field type, so there is exactly one place
/// that documents the range instead of a literal per validator and setter.
pub const PRESENCE_RANGE: (f64, f64) = (-1.0, 1.0);

/// True when `value` is a legal presence amount: finite and inside
/// [`PRESENCE_RANGE`]. Used by the block validator and by every local setter,
/// so a local field can never be written with a value the validator would
/// refuse.
#[must_use]
pub fn presence_amount_is_valid(value: f64) -> bool {
    value.is_finite() && (PRESENCE_RANGE.0..=PRESENCE_RANGE.1).contains(&value)
}

/// Validate a whole [`Presence`] block: the block version plus the three
/// `-1..=1` amount fields. Shared verbatim by the global and the mask-local
/// recipe — see the module docs.
pub fn validate_presence(presence: &Presence) -> Result<(), SidecarError> {
    if presence.version != 1 {
        return invalid("unsupported presence version");
    }
    for (name, select) in PRESENCE_FIELDS {
        if !presence_amount_is_valid(f64::from(select(presence))) {
            return invalid(format!("invalid presence {name}"));
        }
    }
    Ok(())
}

/// True when no stored amount can change a pixel: all three fields at zero. A
/// missing block and an all-zero block read the same, which is what lets a
/// renderer drop a neutral block and what makes the "all-zero persisted block
/// keeps the old byte path" rule a plain content decision.
#[must_use]
pub fn presence_is_neutral(presence: &Presence) -> bool {
    PRESENCE_FIELDS
        .iter()
        .all(|(_, select)| select(presence) == 0.0)
}
