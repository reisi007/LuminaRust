//! Tone-curve channel helpers and the shared point-rule validator.
//!
//! Both the global recipe (`EditRecipe::curves`) and the typed mask-local
//! recipe (MASK-LOCAL-P1.2a `local_adjustments.curves`) store the very same
//! [`Curves`] block and must accept exactly the same point lists. Keeping the
//! channel accessors and the one validator here is what makes that a
//! structural property instead of two validators that happen to agree today.

use crate::{CurveChannels, CurvePoint, CurvePoints, Curves, SidecarError};

/// The four curve channels a tone-curve editor can address, in the canonical
/// display/serialization order (master first, then the RGB channels).
pub const CURVE_CHANNELS: [&str; 4] = ["master", "red", "green", "blue"];

/// The two-point identity curve `[(0,0),(1,1)]`.
#[must_use]
pub fn identity_curve_points() -> CurvePoints {
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

/// True when a control-point list maps every input to itself. A persisted
/// identity is pixel-neutral but is *not* byte-identical in storage, so this
/// predicate is what lets a renderer treat it as "no curve" without losing
/// the caller's explicit state.
#[must_use]
pub fn curve_points_are_identity(points: &[CurvePoint]) -> bool {
    points.len() >= 2 && points.iter().all(|p| p.input == p.output)
}

impl Curves {
    /// A neutral, fully valid curve block: identity master, no channels.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            version: 1,
            master: identity_curve_points(),
            channels: CurveChannels::default(),
        }
    }

    /// Read one channel's stored points (`None` for an unset RGB channel).
    #[must_use]
    pub fn channel(&self, channel: &str) -> Option<&CurvePoints> {
        match channel {
            "master" => Some(&self.master),
            "red" => self.channels.red.as_ref(),
            "green" => self.channels.green.as_ref(),
            "blue" => self.channels.blue.as_ref(),
            _ => None,
        }
    }

    /// Mutable access to one channel, creating the slot for an RGB channel.
    pub fn channel_mut(&mut self, channel: &str) -> Option<&mut CurvePoints> {
        match channel {
            "master" => Some(&mut self.master),
            "red" => Some(self.channels.red.get_or_insert_with(identity_curve_points)),
            "green" => Some(
                self.channels
                    .green
                    .get_or_insert_with(identity_curve_points),
            ),
            "blue" => Some(self.channels.blue.get_or_insert_with(identity_curve_points)),
            _ => None,
        }
    }

    /// True when every stored point of every stored channel maps its input to
    /// itself, i.e. applying this block cannot change a pixel.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        curve_points_are_identity(&self.master)
            && [
                self.channels.red.as_ref(),
                self.channels.green.as_ref(),
                self.channels.blue.as_ref(),
            ]
            .into_iter()
            .flatten()
            .all(|points| curve_points_are_identity(points))
    }
}

/// Validate a complete [`Curves`] block: the block version plus the shared
/// point rules of every present channel (master plus the optional R/G/B
/// curves). The global recipe validator and the typed mask-local recipe both
/// go through this one function, so a local curve can never accept a point the
/// global pipeline would reject — or the other way round.
pub fn validate_curves(curves: &Curves) -> Result<(), SidecarError> {
    if curves.version != 1 {
        return Err(SidecarError::Invalid("unsupported curves version".into()));
    }
    validate_curve(&curves.master)?;
    for curve in [
        &curves.channels.red,
        &curves.channels.green,
        &curves.channels.blue,
    ]
    .into_iter()
    .flatten()
    {
        validate_curve(curve)?;
    }
    Ok(())
}

fn validate_curve(c: &[CurvePoint]) -> Result<(), SidecarError> {
    if !(2..=32).contains(&c.len()) {
        return Err(SidecarError::Invalid(
            "curve must contain 2..=32 points".into(),
        ));
    }
    let mut previous = -1.0;
    for p in c {
        if !p.input.is_finite()
            || !p.output.is_finite()
            || !(0.0..=1.0).contains(&p.input)
            || !(0.0..=1.0).contains(&p.output)
            || p.input <= previous
        {
            return Err(SidecarError::Invalid(
                "curve points must be finite, bounded and strictly increasing".into(),
            ));
        }
        previous = p.input;
    }
    let first = c.first().unwrap();
    let last = c.last().unwrap();
    if first.input != 0.0 || first.output != 0.0 || last.input != 1.0 || last.output != 1.0 {
        return Err(SidecarError::Invalid(
            "curve must have (0,0) and (1,1) endpoints".into(),
        ));
    }
    Ok(())
}
