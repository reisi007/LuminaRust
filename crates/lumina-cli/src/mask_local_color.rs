//! MASK-LOCAL-P1.2b CLI parsing of the local per-pixel colour block.
//!
//! Like the local tone curve (see `mask_local_curves`), the local colour block
//! is reached through the existing generic
//! `--set-local-adjustment KEY=VALUE` / `--reset-local-adjustment KEY` flags
//! with its own key namespaces. That keeps `main.rs` free of a second,
//! near-identical flag party and prevents two grammars from drifting apart.
//!
//! Accepted keys:
//!
//! | spec                                                              | effect                              |
//! |-------------------------------------------------------------------|-------------------------------------|
//! | `--set-local-adjustment hsl.red.hue=-0.25`                       | one HSL shift of one band           |
//! | `--set-local-adjustment vibrance=0.4`                            | local vibrance scalar               |
//! | `--set-local-adjustment saturation=-0.3`                          | local saturation scalar             |
//! | `--set-local-adjustment point_color.add=30,45,0.2,0.1,-0.1`       | append one point-colour entry       |
//! | `--set-local-adjustment point_color.pc-2.hue_shift=0.5`           | one shift of an existing entry      |
//! | `--set-local-adjustment color_grading.shadows.hue=30`            | one field of one grading range      |
//! | `--set-local-adjustment color_grading.balance=-0.4`               | grading balance                     |
//! | `--reset-local-adjustment hsl[.<band>]`                          | reset one band / the whole block    |
//! | `--reset-local-adjustment point_color[.<id>]`                    | reset one entry / the whole block   |
//! | `--reset-local-adjustment color_grading[.<target>]`              | reset one range / the whole block   |
//! | `--reset-local-adjustment color`                                 | reset every local colour control    |
//!
//! The local detail, AI-denoise, noise-reduction, sharpening and optics
//! controls stay deliberately unimplemented: they have no key here, so a request
//! for one is the generic "unknown local adjustment" error rather than a silent
//! no-op. The local presence block is implemented (MASK-LOCAL-P1.2c) but lives
//! in its own key namespace; see `mask_local_presence`.

use super::CliError;
use lumina_sidecar::{
    LocalAdjustments, LOCAL_GRADING_RANGES, LOCAL_HSL_FIELDS, LOCAL_POINT_COLOR_FIELDS,
};

/// One parsed `--set-local-adjustment` colour request.
pub(crate) enum LocalColorSetSpec {
    Hsl {
        band: String,
        field: String,
        value: f64,
    },
    PointColorAdd {
        values: [f64; 5],
    },
    PointColorField {
        id: String,
        field: String,
        value: f64,
    },
    Grading {
        target: String,
        field: String,
        value: f64,
    },
}

/// One parsed `--reset-local-adjustment` colour request.
pub(crate) enum LocalColorResetSpec {
    Hsl(Option<String>),
    PointColor(Option<String>),
    Grading(Option<String>),
    All,
}

const HSL_KEY: &str = "hsl";
const POINT_COLOR_KEY: &str = "point_color";
const COLOR_GRADING_KEY: &str = "color_grading";
const COLOR_KEY: &str = "color";

/// True when `band` is one of the eight HSL bands of the shared global block.
fn is_hsl_band(band: &str) -> bool {
    lumina_sidecar::HSL_BANDS
        .iter()
        .any(|(name, _)| *name == band)
}

/// Split `ns.rest` into its namespace and remainder, if `key` is a `ns` key.
///
/// Only the exact key `ns` and the prefix `ns.` are namespaced keys, so a key
/// such as `hslx` stays a (rejected) scalar key instead of silently resetting
/// a whole block.
fn namespace_of<'a>(key: &'a str, namespace: &str) -> Option<&'a str> {
    if key == namespace {
        return Some("");
    }
    key.strip_prefix(&format!("{namespace}."))
}

/// Parse a finite number, rejecting anything a payload could not mean.
fn parse_value(spec: &str, value: &str) -> Result<f64, CliError> {
    let parsed = value.parse::<f64>().map_err(|_| {
        CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be a finite number"
        ))
    })?;
    if !parsed.is_finite() {
        return Err(CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be finite"
        )));
    }
    Ok(parsed)
}

/// Parse the value part of a colour key, or `None` when the key belongs to a
/// different namespace.
pub(crate) fn parse_color_set_spec(spec: &str) -> Result<Option<LocalColorSetSpec>, CliError> {
    let Some((key, value)) = spec.split_once('=') else {
        return Ok(None);
    };
    if let Some(band) = namespace_of(key, HSL_KEY) {
        if band.is_empty() {
            return Err(CliError::Message(format!(
                "invalid --set-local-adjustment `{spec}`; `hsl` needs a band and a field, use `hsl.<band>.<field>`"
            )));
        }
        let (band, field) = band.split_once('.').ok_or_else(|| {
            CliError::Message(format!(
                "invalid --set-local-adjustment `{spec}`; `hsl` needs a band and a field, use `hsl.<band>.<field>`"
            ))
        })?;
        if !is_hsl_band(band) {
            return Err(CliError::Message(format!(
                "unknown local hsl band `{band}` in `{spec}`"
            )));
        }
        if !LOCAL_HSL_FIELDS.contains(&field) {
            return Err(CliError::Message(format!(
                "unknown local hsl field `{field}` in `{spec}`"
            )));
        }
        return Ok(Some(LocalColorSetSpec::Hsl {
            band: band.into(),
            field: field.into(),
            value: parse_value(spec, value)?,
        }));
    }
    if let Some(rest) = namespace_of(key, POINT_COLOR_KEY) {
        if rest == "add" {
            let parts: Vec<&str> = value.split(',').collect();
            if parts.len() != LOCAL_POINT_COLOR_FIELDS.len() {
                return Err(CliError::Message(format!(
                    "invalid --set-local-adjustment `{spec}`; `point_color.add` takes exactly {} comma-separated numbers ({})",
                    LOCAL_POINT_COLOR_FIELDS.len(),
                    LOCAL_POINT_COLOR_FIELDS.join(",")
                )));
            }
            let mut values = [0.0_f64; 5];
            for (slot, part) in values.iter_mut().zip(parts) {
                *slot = parse_value(spec, part.trim())?;
            }
            return Ok(Some(LocalColorSetSpec::PointColorAdd { values }));
        }
        let (id, field) = rest.split_once('.').ok_or_else(|| {
            CliError::Message(format!(
                "invalid --set-local-adjustment `{spec}`; use `point_color.add=<numbers>` or `point_color.<id>.<field>=<number>`"
            ))
        })?;
        if !LOCAL_POINT_COLOR_FIELDS.contains(&field) {
            return Err(CliError::Message(format!(
                "unknown local point color field `{field}` in `{spec}`"
            )));
        }
        return Ok(Some(LocalColorSetSpec::PointColorField {
            id: id.into(),
            field: field.into(),
            value: parse_value(spec, value)?,
        }));
    }
    if let Some(rest) = namespace_of(key, COLOR_GRADING_KEY) {
        let (target, field) = rest.split_once('.').unwrap_or((rest, "value"));
        if !LOCAL_GRADING_RANGES.contains(&target) && !matches!(target, "balance" | "blending") {
            return Err(CliError::Message(format!(
                "unknown local color grading target `{target}` in `{spec}`"
            )));
        }
        return Ok(Some(LocalColorSetSpec::Grading {
            target: target.into(),
            field: field.into(),
            value: parse_value(spec, value)?,
        }));
    }
    Ok(None)
}

/// Parse the key of a `--reset-local-adjustment` colour request, or `None` when
/// the key belongs to a different namespace.
pub(crate) fn parse_color_reset_spec(key: &str) -> Result<Option<LocalColorResetSpec>, CliError> {
    if let Some(band) = namespace_of(key, HSL_KEY) {
        if !band.is_empty() && !is_hsl_band(band) {
            return Err(CliError::Message(format!(
                "unknown local hsl band `{band}`"
            )));
        }
        return Ok(Some(LocalColorResetSpec::Hsl(
            (!band.is_empty()).then(|| band.into()),
        )));
    }
    if let Some(id) = namespace_of(key, POINT_COLOR_KEY) {
        return Ok(Some(LocalColorResetSpec::PointColor(
            (!id.is_empty()).then(|| id.into()),
        )));
    }
    if let Some(target) = namespace_of(key, COLOR_GRADING_KEY) {
        if !target.is_empty()
            && !LOCAL_GRADING_RANGES.contains(&target)
            && !matches!(target, "balance" | "blending")
        {
            return Err(CliError::Message(format!(
                "unknown local color grading target `{target}`"
            )));
        }
        return Ok(Some(LocalColorResetSpec::Grading(
            (!target.is_empty()).then(|| target.into()),
        )));
    }
    if key == COLOR_KEY {
        return Ok(Some(LocalColorResetSpec::All));
    }
    Ok(None)
}

/// Human-readable label for the history/log action of one applied request.
pub(crate) fn set_label(spec: &LocalColorSetSpec) -> String {
    match spec {
        LocalColorSetSpec::Hsl { band, field, value } => {
            format!("local:hsl.{band}.{field}={value}")
        }
        LocalColorSetSpec::PointColorAdd { values } => format!(
            "local:point_color.add={}",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<String>>()
                .join(",")
        ),
        LocalColorSetSpec::PointColorField { id, field, value } => {
            format!("local:point_color.{id}.{field}={value}")
        }
        LocalColorSetSpec::Grading {
            target,
            field,
            value,
        } => format!("local:color_grading.{target}.{field}={value}"),
    }
}

/// Human-readable label for the history/log action of one applied reset.
pub(crate) fn reset_label(spec: &LocalColorResetSpec) -> String {
    match spec {
        LocalColorResetSpec::Hsl(None) => "local-reset:hsl".into(),
        LocalColorResetSpec::Hsl(Some(band)) => format!("local-reset:hsl.{band}"),
        LocalColorResetSpec::PointColor(None) => "local-reset:point_color".into(),
        LocalColorResetSpec::PointColor(Some(id)) => format!("local-reset:point_color.{id}"),
        LocalColorResetSpec::Grading(None) => "local-reset:color_grading".into(),
        LocalColorResetSpec::Grading(Some(target)) => format!("local-reset:color_grading.{target}"),
        LocalColorResetSpec::All => "local-reset:color".into(),
    }
}

/// Apply one parsed request to a staged (not yet persisted) local recipe.
pub(crate) fn apply_set_spec(
    adjustments: &mut LocalAdjustments,
    spec: &LocalColorSetSpec,
) -> Result<(), CliError> {
    let outcome = match spec {
        LocalColorSetSpec::Hsl { band, field, value } => {
            adjustments.set_local_hsl_band(band, field, *value)
        }
        LocalColorSetSpec::PointColorAdd { values } => adjustments
            .add_local_point_color_entry(values[0], values[1], values[2], values[3], values[4])
            .map(|_| ()),
        LocalColorSetSpec::PointColorField { id, field, value } => {
            adjustments.set_local_point_color_field(id, field, *value)
        }
        LocalColorSetSpec::Grading {
            target,
            field,
            value,
        } => adjustments.set_local_color_grading_field(target, field, *value),
    };
    outcome.map_err(CliError::Message)
}

/// Apply one parsed reset to a staged (not yet persisted) local recipe.
pub(crate) fn apply_reset_spec(
    adjustments: &mut LocalAdjustments,
    spec: &LocalColorResetSpec,
) -> Result<(), CliError> {
    let outcome = match spec {
        LocalColorResetSpec::Hsl(None) => {
            adjustments.reset_local_hsl();
            Ok(())
        }
        LocalColorResetSpec::Hsl(Some(band)) => adjustments.reset_local_hsl_band(band),
        LocalColorResetSpec::PointColor(None) => {
            adjustments.reset_local_point_color();
            Ok(())
        }
        LocalColorResetSpec::PointColor(Some(id)) => adjustments.remove_local_point_color_entry(id),
        LocalColorResetSpec::Grading(None) => {
            adjustments.reset_local_color_grading();
            Ok(())
        }
        LocalColorResetSpec::Grading(Some(target)) => {
            adjustments.reset_local_color_grading_field(target)
        }
        LocalColorResetSpec::All => {
            adjustments.reset_local_color();
            Ok(())
        }
    };
    outcome.map_err(CliError::Message)
}
