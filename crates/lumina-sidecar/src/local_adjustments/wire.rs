//! Versioned wire decoding for the local mask recipe.

use super::{
    LocalAdjustments, COLOR_LOCAL_ADJUSTMENTS_VERSION, CURVE_LOCAL_ADJUSTMENTS_VERSION,
    DETAIL_LOCAL_ADJUSTMENTS_VERSION, LEGACY_LOCAL_ADJUSTMENTS_VERSION,
    LEGACY_LOCAL_ADJUSTMENTS_VERSIONS, LOCAL_ADJUSTMENTS_VERSION,
    PRESENCE_LOCAL_ADJUSTMENTS_VERSION,
};
use crate::Curves;
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

/// The MASK-LOCAL-P1.2a tone-curve field, kept raw until the version is
/// known.  Presence is tracked exactly like a delta: a v1/v2 payload that
/// writes the key is a loud error instead of a silently dropped curve, and an
/// explicit JSON `null` is a loud error instead of "no curve".
#[derive(Default)]
struct WireCurves(Option<Value>);

impl<'de> Deserialize<'de> for WireCurves {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(|value| WireCurves(Some(value)))
    }
}

/// One raw JSON value of a *versioned block* field.
///
/// This is the same "presence is tracked exactly like a delta" rule the
/// relative-WB fields use: `None` means *the key was absent* and is reachable
/// only through `#[serde(default)]`, while everything a payload actually wrote
/// stays a real [`Value`]. So a v1/v2/v3 payload that writes `hsl`, an explicit
/// `null`, or a non-object is a loud error instead of a silently dropped or
/// silently coerced colour block.
#[derive(Default)]
struct WireBlock(Option<Value>);

impl<'de> Deserialize<'de> for WireBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(|value| WireBlock(Some(value)))
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
    #[serde(default)]
    curves: WireCurves,
    #[serde(default)]
    hsl: WireBlock,
    #[serde(default)]
    point_color: WireBlock,
    #[serde(default)]
    color_grading: WireBlock,
    #[serde(default)]
    vibrance: WireDelta,
    #[serde(default)]
    saturation: WireDelta,
    /// The MASK-LOCAL-P1.2c presence block, kept raw for the same reason as the
    /// colour blocks: a v1..v4 payload that writes `presence` is a loud error
    /// instead of a silently dropped or silently coerced block.
    #[serde(default)]
    presence: WireBlock,
    /// The MASK-LOCAL-P1.2d detail block, kept raw for the same reason as every
    /// other versioned block: a v1..v5 payload that writes `detail` — an
    /// explicit `null`, a number, a string or a partial object included — is a
    /// loud error instead of a silently dropped detail edit.
    #[serde(default)]
    detail: WireBlock,
}

impl<'de> Deserialize<'de> for LocalAdjustments {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LocalAdjustmentsWire::deserialize(deserializer)?;
        if wire.version != LOCAL_ADJUSTMENTS_VERSION
            && !LEGACY_LOCAL_ADJUSTMENTS_VERSIONS.contains(&wire.version)
        {
            return Err(serde::de::Error::custom(format!(
                "unsupported local_adjustments version {} (expected one of {:?} or {LOCAL_ADJUSTMENTS_VERSION})",
                wire.version, LEGACY_LOCAL_ADJUSTMENTS_VERSIONS
            )));
        }
        if wire.version == LEGACY_LOCAL_ADJUSTMENTS_VERSION
            && (wire.temperature_delta_k.0.is_some() || wire.tint_delta.0.is_some())
        {
            return Err(serde::de::Error::custom(
                "local_adjustments version 1 cannot contain relative white-balance delta fields",
            ));
        }
        // A payload that cannot express a curve must not carry one: dropping
        // it silently would lose an edit the user can see in the file.
        if wire.version < CURVE_LOCAL_ADJUSTMENTS_VERSION && wire.curves.0.is_some() {
            return Err(serde::de::Error::custom(format!(
                "local_adjustments version {} cannot contain a tone curve",
                wire.version
            )));
        }
        // The same rule for the P1.2b color block: an older version must not be
        // able to smuggle a later field, not even a neutral one. The gate is
        // anchored at the *introducing* version, not at the current one, so
        // raising `LOCAL_ADJUSTMENTS_VERSION` for a later block cannot make a
        // v4 document lose its own colour block.
        for (present, label) in [
            (wire.hsl.0.is_some(), "a local HSL block"),
            (wire.point_color.0.is_some(), "a local point color block"),
            (
                wire.color_grading.0.is_some(),
                "a local color grading block",
            ),
        ] {
            if wire.version < COLOR_LOCAL_ADJUSTMENTS_VERSION && present {
                return Err(serde::de::Error::custom(format!(
                    "local_adjustments version {} cannot contain {label}",
                    wire.version
                )));
            }
        }
        if wire.version < COLOR_LOCAL_ADJUSTMENTS_VERSION
            && (wire.vibrance.0.is_some() || wire.saturation.0.is_some())
        {
            return Err(serde::de::Error::custom(format!(
                "local_adjustments version {} cannot contain local vibrance or saturation",
                wire.version
            )));
        }
        // And the same rule for the P1.2c presence block: absent is the only
        // "no presence" case on the wire. An explicit `null`, a number, a
        // string or an object-shaped sentinel in a v1..v4 payload is loud.
        if wire.version < PRESENCE_LOCAL_ADJUSTMENTS_VERSION && wire.presence.0.is_some() {
            return Err(serde::de::Error::custom(format!(
                "local_adjustments version {} cannot contain a local presence block",
                wire.version
            )));
        }
        // And the same rule for the P1.2d detail block, anchored at the version
        // that introduced it so a v5 document keeps its own presence block.
        if wire.version < DETAIL_LOCAL_ADJUSTMENTS_VERSION && wire.detail.0.is_some() {
            return Err(serde::de::Error::custom(format!(
                "local_adjustments version {} cannot contain a local detail block",
                wire.version
            )));
        }
        let temperature_delta_k =
            parse_wire_number(wire.temperature_delta_k.0, "temperature_delta_k")?;
        let tint_delta = parse_wire_number(wire.tint_delta.0, "tint_delta")?;
        let curves = parse_wire_curves(wire.curves.0)?;
        let hsl = parse_wire_block(wire.hsl.0, "hsl", "local hsl")?;
        let point_color = parse_wire_block(wire.point_color.0, "point color", "local point color")?;
        let color_grading =
            parse_wire_block(wire.color_grading.0, "color grading", "local color grading")?;
        let vibrance = parse_wire_number(wire.vibrance.0, "vibrance")?;
        let saturation = parse_wire_number(wire.saturation.0, "saturation")?;
        let presence = parse_wire_block(wire.presence.0, "presence", "local presence")?;
        let detail = parse_wire_block(wire.detail.0, "detail", "local detail")?;
        let value = Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            exposure: wire.exposure,
            contrast: wire.contrast,
            highlights: wire.highlights,
            shadows: wire.shadows,
            temperature_delta_k,
            tint_delta,
            curves,
            hsl,
            point_color,
            color_grading,
            vibrance,
            saturation,
            presence,
            detail,
        };
        value
            .validate()
            .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        Ok(value)
    }
}

/// Decode the optional tone-curve block.  Only an absent key yields `None`;
/// everything a payload actually wrote must be a curve object that satisfies
/// the shared global point rules.
fn parse_wire_curves<E>(value: Option<Value>) -> Result<Option<Curves>, E>
where
    E: serde::de::Error,
{
    match value {
        None => Ok(None),
        Some(raw) => serde_json::from_value::<Curves>(raw)
            .map(Some)
            .map_err(|error| {
                E::custom(format!(
                    "local tone curve must be a valid curve object: {error}"
                ))
            }),
    }
}

/// Decode one optional versioned color block.
///
/// Only an absent key yields `None`; everything a payload actually wrote must
/// be an object of the *global* type, so a local block can never be a looser
/// dialect of the global one.
fn parse_wire_block<E, T>(value: Option<Value>, label: &str, context: &str) -> Result<Option<T>, E>
where
    E: serde::de::Error,
    T: serde::de::DeserializeOwned,
{
    match value {
        None => Ok(None),
        Some(raw) => serde_json::from_value::<T>(raw).map(Some).map_err(|error| {
            E::custom(format!("{context} must be a valid {label} object: {error}"))
        }),
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
