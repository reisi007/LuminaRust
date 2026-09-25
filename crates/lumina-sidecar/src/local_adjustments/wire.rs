//! Versioned wire decoding for the local mask recipe.

use super::{LocalAdjustments, LEGACY_LOCAL_ADJUSTMENTS_VERSION, LOCAL_ADJUSTMENTS_VERSION};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// One delta field exactly as the payload wrote it.
///
/// `None` means *the key was absent*, and it is reachable only through
/// `#[serde(default)]` — serde never calls this type's `Deserialize` for a
/// missing key. Everything a payload actually wrote stays a real
/// [`Value`], including an explicit JSON `null`, so no sentinel-shaped data
/// can ever masquerade as "absent" and coerce to the neutral `0.0`.
#[derive(Default)]
struct WireDelta(Option<Value>);

impl<'de> Deserialize<'de> for WireDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(|value| WireDelta(Some(value)))
    }
}

/// Wire-only shape.  Delta values are kept raw until the version is known, so
/// a v1 payload cannot smuggle a v2 field and a JSON `null` cannot be silently
/// treated as the default zero.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalAdjustmentsWire {
    version: u8,
    #[serde(default)]
    exposure: f64,
    #[serde(default)]
    contrast: f64,
    #[serde(default)]
    highlights: f64,
    #[serde(default)]
    shadows: f64,
    #[serde(default)]
    temperature_delta_k: WireDelta,
    #[serde(default)]
    tint_delta: WireDelta,
}

impl<'de> Deserialize<'de> for LocalAdjustments {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LocalAdjustmentsWire::deserialize(deserializer)?;
        if wire.version != LOCAL_ADJUSTMENTS_VERSION
            && wire.version != LEGACY_LOCAL_ADJUSTMENTS_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "unsupported local_adjustments version {} (expected {LEGACY_LOCAL_ADJUSTMENTS_VERSION} or {LOCAL_ADJUSTMENTS_VERSION})",
                wire.version
            )));
        }
        if wire.version == LEGACY_LOCAL_ADJUSTMENTS_VERSION
            && (wire.temperature_delta_k.0.is_some() || wire.tint_delta.0.is_some())
        {
            return Err(serde::de::Error::custom(
                "local_adjustments version 1 cannot contain relative white-balance delta fields",
            ));
        }
        let temperature_delta_k =
            parse_wire_number(wire.temperature_delta_k.0, "temperature_delta_k")?;
        let tint_delta = parse_wire_number(wire.tint_delta.0, "tint_delta")?;
        let value = Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            exposure: wire.exposure,
            contrast: wire.contrast,
            highlights: wire.highlights,
            shadows: wire.shadows,
            temperature_delta_k,
            tint_delta,
        };
        value
            .validate()
            .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        Ok(value)
    }
}

/// Decode one delta field.
///
/// `None` is the only "absent" case and yields the neutral `0.0`. Everything
/// a payload actually wrote must be a JSON number: an explicit `null`, a
/// string, a boolean, an array, or an object (including any object that looks
/// like an internal sentinel) is a loud error rather than a silent `0.0`.
fn parse_wire_number<E>(value: Option<Value>, name: &str) -> Result<f64, E>
where
    E: serde::de::Error,
{
    match value {
        None => Ok(0.0),
        Some(Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| E::custom(format!("local adjustment `{name}` must be a finite number"))),
        Some(_) => Err(E::custom(format!(
            "local adjustment `{name}` must be a number"
        ))),
    }
}
