//! MASK-LOCAL-P1.2a CLI parsing and application of local tone-curve edits.
//!
//! The local curve reuses the same `CHANNEL:I,O;I,O;...` point syntax the
//! global `--curve-points` flag already uses, so a user (and a test) can move
//! between the global and the local editor without learning a second grammar.
//! It is reached through the existing generic
//! `--set-local-adjustment KEY=VALUE` / `--reset-local-adjustment KEY` flags
//! with the `curves.` key namespace, which keeps the CLI surface free of a
//! second, near-identical flag pair.
//!
//! Accepted keys:
//!
//! | spec                                   | effect                                  |
//! |----------------------------------------|-----------------------------------------|
//! | `--set-local-adjustment curves.master=0,0;0.25,0.3;1,1` | replace the master curve |
//! | `--set-local-adjustment curves.red=…`  | replace one RGB channel curve           |
//! | `--reset-local-adjustment curves.master` | reset one channel to the identity     |
//! | `--reset-local-adjustment curves`      | reset every local curve of the layer    |
//!
//! The local HSL, point-color, grading, presence and detail controls stay
//! deliberately unimplemented: they have no key here, so a request for one is
//! the generic "unknown local adjustment" error rather than a silent no-op.

use super::CliError;
use lumina_sidecar::{CurvePoints, LocalAdjustments};

/// One parsed `--set-local-adjustment` request.
pub(crate) enum LocalSetSpec {
    Scalar {
        key: String,
        value: f64,
    },
    Curve {
        channel: String,
        points: CurvePoints,
    },
}

/// One parsed `--reset-local-adjustment` request.
pub(crate) enum LocalResetSpec {
    Scalar(String),
    Channel(String),
    All,
}

/// The key prefix that routes a local-adjustment spec to the tone curve.
const CURVES_KEY: &str = "curves";

/// Split a local-adjustment key into its curve namespace, if any.
///
/// Only the exact key `curves` and the prefix `curves.` are curve keys; a key
/// such as `curves_foo` stays a (rejected) scalar key instead of silently
/// resetting the whole curve block.
fn curve_channel_of(key: &str) -> Option<&str> {
    if key == CURVES_KEY {
        return Some("");
    }
    key.strip_prefix(&format!("{CURVES_KEY}."))
}

/// Parse one `--set-local-adjustment KEY=VALUE` request.
pub(crate) fn parse_local_set_spec(spec: &str) -> Result<LocalSetSpec, CliError> {
    let (key, value) = spec.split_once('=').ok_or_else(|| {
        CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; expected KEY=VALUE"
        ))
    })?;
    if let Some(channel) = curve_channel_of(key) {
        if channel.is_empty() {
            return Err(CliError::Message(format!(
                "invalid --set-local-adjustment `{spec}`; `curves` needs a channel, use `curves.<master|red|green|blue>`"
            )));
        }
        // Reuse the global curve-channel and point-list parsers so both
        // surfaces accept exactly the same input.
        let points = super::parse_curve_points(&format!("{channel}:{value}"))?.1;
        return Ok(LocalSetSpec::Curve {
            channel: channel.into(),
            points,
        });
    }
    let value = value.parse::<f64>().map_err(|_| {
        CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be a finite number"
        ))
    })?;
    if !value.is_finite() {
        return Err(CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be finite"
        )));
    }
    Ok(LocalSetSpec::Scalar {
        key: key.into(),
        value,
    })
}

/// Parse one `--reset-local-adjustment KEY` request.
pub(crate) fn parse_local_reset_spec(key: &str) -> Result<LocalResetSpec, CliError> {
    match curve_channel_of(key) {
        None => Ok(LocalResetSpec::Scalar(key.into())),
        Some("") => Ok(LocalResetSpec::All),
        Some(channel) => {
            super::check_curve_channel(channel)?;
            Ok(LocalResetSpec::Channel(channel.into()))
        }
    }
}

/// Human-readable label for the history/log action of one applied request.
pub(crate) fn action_label(spec: &LocalSetSpec) -> String {
    match spec {
        LocalSetSpec::Scalar { key, value } => format!("local:{key}={value}"),
        LocalSetSpec::Curve { channel, points } => {
            format!("local:curves.{channel}={}pts", points.len())
        }
    }
}

/// Human-readable label for the history/log action of one applied reset.
pub(crate) fn reset_label(spec: &LocalResetSpec) -> String {
    match spec {
        LocalResetSpec::Scalar(key) => format!("local-reset:{key}"),
        LocalResetSpec::Channel(channel) => format!("local-reset:curves.{channel}"),
        LocalResetSpec::All => "local-reset:curves".into(),
    }
}

/// Apply one parsed request to a staged (not yet persisted) local recipe.
///
/// Every path validates before it mutates, so a rejected request leaves the
/// staged recipe exactly as the caller found it and the whole CLI transaction
/// stays atomic.
pub(crate) fn apply_set_spec(
    adjustments: &mut LocalAdjustments,
    spec: &LocalSetSpec,
) -> Result<(), CliError> {
    match spec {
        LocalSetSpec::Scalar { key, value } => adjustments
            .set_value(key, *value)
            .map_err(CliError::Message),
        LocalSetSpec::Curve { channel, points } => adjustments
            .set_local_curve_channel(channel, points.clone())
            .map_err(CliError::Message),
    }
}

/// Apply one parsed reset to a staged (not yet persisted) local recipe.
pub(crate) fn apply_reset_spec(
    adjustments: &mut LocalAdjustments,
    spec: &LocalResetSpec,
) -> Result<(), CliError> {
    match spec {
        LocalResetSpec::Scalar(key) => adjustments.set_value(key, 0.0).map_err(CliError::Message),
        LocalResetSpec::Channel(channel) => adjustments
            .reset_local_curve_channel(channel)
            .map_err(CliError::Message),
        LocalResetSpec::All => {
            adjustments.reset_local_curves();
            Ok(())
        }
    }
}
