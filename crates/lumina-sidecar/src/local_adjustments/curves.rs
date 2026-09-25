//! MASK-LOCAL-P1.2a accessors for the local tone-curve block.
//!
//! These live beside (not inside) the schema type so the typed object stays a
//! pure data/validation contract. Every mutation validates against the shared
//! global curve rules *before* it stores anything, so a rejected point list
//! leaves the layer byte-for-byte unchanged.

use super::LocalAdjustments;
use crate::{identity_curve_points, CurvePoints, Curves, CURVE_CHANNELS};

impl LocalAdjustments {
    /// Compact, deterministic summary of the stored local curve channels used
    /// by the CLI status line: `none` for a neutral block, otherwise
    /// `<channel>:<point-count>` for each stored channel in canonical order.
    #[must_use]
    pub fn curve_summary(&self) -> String {
        let Some(curves) = &self.curves else {
            return "none".into();
        };
        let stored: Vec<String> = CURVE_CHANNELS
            .iter()
            .filter_map(|channel| {
                curves
                    .channel(channel)
                    .map(|points| format!("{channel}:{}", points.len()))
            })
            .collect();
        if stored.is_empty() {
            return "none".into();
        }
        stored.join(",")
    }

    /// Read one local curve channel. A layer without a curve block reads
    /// `None`, which every editor resolves to the identity curve.
    #[must_use]
    pub fn local_curve_channel(&self, channel: &str) -> Option<CurvePoints> {
        self.curves
            .as_ref()
            .and_then(|curves| curves.channel(channel))
            .cloned()
    }

    /// True when the layer stores a tone curve that is not the identity. This
    /// is the compositor's "this layer needs the tone kernel" predicate: an
    /// absent block and a persisted identity both read as "no curve".
    #[must_use]
    pub fn has_local_curves(&self) -> bool {
        self.curves
            .as_ref()
            .is_some_and(|curves| !curves.is_identity())
    }

    /// Replace one local curve channel with `points`.
    pub fn set_local_curve_channel(
        &mut self,
        channel: &str,
        points: CurvePoints,
    ) -> Result<(), String> {
        let mut curves = self.curves.clone().unwrap_or_else(Curves::identity);
        let slot = curves
            .channel_mut(channel)
            .ok_or_else(|| format!("unknown local curve channel `{channel}`"))?;
        *slot = points;
        crate::validate_curves(&curves).map_err(|error| format!("local tone curve: {error}"))?;
        self.curves = Some(curves);
        Ok(())
    }

    /// Reset one local curve channel. An RGB channel is removed entirely
    /// (`None` reads as the identity); `master` returns to the two-point
    /// identity. A block that is identity everywhere afterwards is dropped
    /// entirely, so a reset is byte-identical to "never edited".
    ///
    /// The channel name is checked *before* the "no curve block yet"
    /// shortcut: a typo must be loud even on a layer that was never edited.
    pub fn reset_local_curve_channel(&mut self, channel: &str) -> Result<(), String> {
        if !CURVE_CHANNELS.contains(&channel) {
            return Err(format!("unknown local curve channel `{channel}`"));
        }
        let Some(curves) = &mut self.curves else {
            return Ok(());
        };
        match channel {
            "master" => curves.master = identity_curve_points(),
            "red" => curves.channels.red = None,
            "green" => curves.channels.green = None,
            "blue" => curves.channels.blue = None,
            _ => unreachable!("checked against CURVE_CHANNELS"),
        }
        if curves.is_identity() {
            self.curves = None;
        }
        Ok(())
    }

    /// Reset every local curve channel of this layer.
    pub fn reset_local_curves(&mut self) {
        self.curves = None;
    }
}
