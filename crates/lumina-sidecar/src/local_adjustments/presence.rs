//! MASK-LOCAL-P1.2c accessors for the local presence block.
//!
//! These live beside (not inside) the schema type so the typed object stays a
//! pure data/validation contract. Every mutation validates against the shared
//! global presence rules *before* it stores anything, so a rejected value
//! leaves the layer byte-for-byte unchanged, and a value that returns to its
//! neutral removes the whole block, so a reset is byte-identical to a layer
//! that was never edited.
//!
//! The block is deliberately the *global* `lumina_sidecar::Presence`: a local
//! presence is the global presence stage at a different place in the chain, so
//! a second local type would be a dialect that could drift.

use super::LocalAdjustments;
use crate::presence_block::{presence_amount_is_valid, presence_is_neutral, PRESENCE_FIELDS};

/// A neutral, fully valid presence block (all three amounts at zero).
#[must_use]
pub fn neutral_local_presence() -> crate::Presence {
    crate::Presence {
        version: 1,
        texture: 0.0,
        clarity: 0.0,
        dehaze: 0.0,
    }
}

impl LocalAdjustments {
    /// True when the layer stores a presence amount that can change a pixel.
    ///
    /// This is the routing predicate *and* the kernel-path predicate: an
    /// absent block and a persisted all-zero block both read `false`, so a
    /// neutral block keeps every previously pinned P0/P1.1/P1.2a/P1.2b byte
    /// exactly. That is why the check is on the *content*, not on
    /// `self.presence.is_some()`.
    #[must_use]
    pub fn has_local_presence(&self) -> bool {
        self.presence
            .as_ref()
            .is_some_and(|presence| !presence_is_neutral(presence))
    }

    /// Read the stored local presence amounts, or the neutral triple for a
    /// block that was never edited.
    #[must_use]
    pub fn local_presence(&self) -> (f64, f64, f64) {
        self.presence.as_ref().map_or((0.0, 0.0, 0.0), |presence| {
            (
                f64::from(presence.texture),
                f64::from(presence.clarity),
                f64::from(presence.dehaze),
            )
        })
    }

    /// Compact, deterministic summary of the stored local presence for the CLI
    /// status line: `none` for an absent or all-zero block, otherwise the
    /// non-neutral field names in canonical order.
    #[must_use]
    pub fn presence_summary(&self) -> String {
        let Some(presence) = &self.presence else {
            return "none".into();
        };
        if presence_is_neutral(presence) {
            return "none".into();
        }
        let stored: Vec<&str> = PRESENCE_FIELDS
            .iter()
            .filter(|(_, select)| select(presence) != 0.0)
            .map(|(name, _)| *name)
            .collect();
        stored.join("+")
    }

    /// Set one local presence amount, creating the block on demand.
    ///
    /// The field name is checked *before* the block is created, so a typo is
    /// loud even on a layer that was never edited, and the value is validated
    /// with the shared global `-1..=1` rule before it is stored. Writing the
    /// last non-neutral amount back to zero drops the whole block, so the layer
    /// is byte-identical to a layer that was never edited.
    pub fn set_local_presence_field(&mut self, field: &str, value: f64) -> Result<(), String> {
        if !PRESENCE_FIELDS.iter().any(|(name, _)| *name == field) {
            return Err(format!("unknown local presence field `{field}`"));
        }
        if !presence_amount_is_valid(value) {
            return Err(format!(
                "local presence {field} must be finite and in -1..=1, got {value}"
            ));
        }
        let value = value as f32;
        // Mutate a copy and commit only after the shared validator accepted it,
        // so a refused value leaves the layer byte-for-byte unchanged.
        let mut presence = self.presence.unwrap_or_else(neutral_local_presence);
        match field {
            "texture" => presence.texture = value,
            "clarity" => presence.clarity = value,
            "dehaze" => presence.dehaze = value,
            _ => unreachable!("validated local presence field"),
        }
        crate::validate_presence(&presence).map_err(|error| format!("local presence: {error}"))?;
        self.presence = (!presence_is_neutral(&presence)).then_some(presence);
        Ok(())
    }

    /// Reset the whole local presence block. The block disappears entirely, so
    /// a reset is byte-identical to a layer that was never edited.
    pub fn reset_local_presence(&mut self) {
        self.presence = None;
    }
}
