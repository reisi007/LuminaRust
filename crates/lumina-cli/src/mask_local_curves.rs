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
//! MASK-LOCAL-P1.2b extends the same generic channel with the colour key
//! namespaces (`hsl.`, `point_color.`, `color_grading.`, plus the `vibrance` /
//! `saturation` scalars and the `color` reset); see `mask_local_color`.
//! MASK-LOCAL-P1.2c extends it once more with the `presence.` key namespace
//! and the `presence` reset; see the [`presence`] child module.
//! MASK-LOCAL-P1.2d extends it once more with the `sharpening.` and
//! `noise_reduction.` key namespaces and the `sharpening` / `noise_reduction` /
//! `detail` resets; see the [`detail`] child module. The local AI-denoise and
//! optics controls stay deliberately unimplemented: they have no key here, so a
//! request for one is the generic "unknown local adjustment" error rather than
//! a silent no-op.
//!
//! The presence and detail namespaces are declared as *children* of this module
//! rather than as further siblings in `main.rs`, so the oversized CLI entrypoint
//! stays exactly at its 11398-line ratchet baseline: adding a key namespace to
//! the existing generic channel must not cost the entrypoint a single line.

use super::mask_local_color::{self, LocalColorResetSpec, LocalColorSetSpec};
use super::CliError;
use lumina_sidecar::{CurvePoints, LocalAdjustments};

#[path = "mask_local_detail.rs"]
mod detail;
use detail::{LocalDetailResetSpec, LocalDetailSetSpec};

#[path = "mask_local_presence.rs"]
mod presence;
use presence::LocalPresenceSetSpec;

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
    Color(LocalColorSetSpec),
    Presence(LocalPresenceSetSpec),
    Detail(LocalDetailSetSpec),
}

/// One parsed `--reset-local-adjustment` request.
pub(crate) enum LocalResetSpec {
    Scalar(String),
    Channel(String),
    All,
    Color(LocalColorResetSpec),
    Presence,
    Detail(LocalDetailResetSpec),
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
    // The presence, colour and detail namespaces are checked before the curve
    // namespace so a `presence.`/`hsl.`/`point_color.`/`color_grading.`/
    // `sharpening.`/`noise_reduction.` key never reaches the scalar parser, and
    // vice versa.
    if let Some(presence) = presence::parse_presence_set_spec(spec)? {
        return Ok(LocalSetSpec::Presence(presence));
    }
    if let Some(local_detail) = detail::parse_detail_set_spec(spec)? {
        return Ok(LocalSetSpec::Detail(local_detail));
    }
    if let Some(color) = mask_local_color::parse_color_set_spec(spec)? {
        return Ok(LocalSetSpec::Color(color));
    }
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
    if presence::parse_presence_reset_key(key)?.is_some() {
        return Ok(LocalResetSpec::Presence);
    }
    if let Some(local_detail) = detail::parse_detail_reset_key(key)? {
        return Ok(LocalResetSpec::Detail(local_detail));
    }
    if let Some(color) = mask_local_color::parse_color_reset_spec(key)? {
        return Ok(LocalResetSpec::Color(color));
    }
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
        LocalSetSpec::Color(color) => mask_local_color::set_label(color),
        LocalSetSpec::Presence(presence) => presence::set_label(presence),
        LocalSetSpec::Detail(local_detail) => detail::set_label(local_detail),
    }
}

/// Human-readable label for the history/log action of one applied reset.
pub(crate) fn reset_label(spec: &LocalResetSpec) -> String {
    match spec {
        LocalResetSpec::Scalar(key) => format!("local-reset:{key}"),
        LocalResetSpec::Channel(channel) => format!("local-reset:curves.{channel}"),
        LocalResetSpec::All => "local-reset:curves".into(),
        LocalResetSpec::Color(color) => mask_local_color::reset_label(color),
        LocalResetSpec::Presence => presence::reset_label(),
        LocalResetSpec::Detail(local_detail) => detail::reset_label(local_detail),
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
        LocalSetSpec::Color(color) => mask_local_color::apply_set_spec(adjustments, color),
        LocalSetSpec::Presence(presence) => adjustments
            .set_local_presence_field(&presence.field, presence.value)
            .map_err(CliError::Message),
        LocalSetSpec::Detail(local_detail) => if local_detail.block == detail_block_name() {
            adjustments.set_local_sharpening_field(&local_detail.field, local_detail.value)
        } else {
            adjustments.set_local_noise_reduction_field(&local_detail.field, local_detail.value)
        }
        .map_err(CliError::Message),
    }
}

/// The canonical name of the sharpening sub-block, so the apply path and the
/// parser cannot disagree about which setter a `sharpening.*` key reaches.
fn detail_block_name() -> &'static str {
    "sharpening"
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
        LocalResetSpec::Color(color) => mask_local_color::apply_reset_spec(adjustments, color),
        LocalResetSpec::Presence => {
            adjustments.reset_local_presence();
            Ok(())
        }
        LocalResetSpec::Detail(local_detail) => match local_detail {
            detail::LocalDetailResetSpec::Block(block) => adjustments
                .reset_local_detail_field(block)
                .map_err(CliError::Message),
            detail::LocalDetailResetSpec::All => {
                adjustments.reset_local_detail();
                Ok(())
            }
        },
    }
}
