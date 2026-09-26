//! MASK-LOCAL-P1.2d CLI parsing of the local detail block.
//!
//! Like the local tone curve, the local colour block and the local presence
//! block (see `mask_local_curves` / `mask_local_color` / `mask_local_presence`),
//! the local detail is reached through the existing generic
//! `--set-local-adjustment KEY=VALUE` / `--reset-local-adjustment KEY` flags
//! with its own `sharpening.` / `noise_reduction.` key namespaces. That keeps
//! the CLI surface free of a second, near-identical flag party and prevents two
//! grammars from drifting apart.
//!
//! Accepted keys:
//!
//! | spec                                              | effect                        |
//! |---------------------------------------------------|-------------------------------|
//! | `--set-local-adjustment sharpening.amount=1.0`    | local sharpening amount       |
//! | `--set-local-adjustment sharpening.radius=2.0`    | local sharpening radius       |
//! | `--set-local-adjustment sharpening.detail=0.5`    | local sharpening detail mix   |
//! | `--set-local-adjustment sharpening.masking=1.0`   | local flat-area masking       |
//! | `--set-local-adjustment noise_reduction.luminance=0.4` | local NR luminance     |
//! | `--set-local-adjustment noise_reduction.color=0.2`      | local NR colour        |
//! | `--reset-local-adjustment sharpening`             | reset the sharpening sub-block|
//! | `--reset-local-adjustment noise_reduction`        | reset the NR sub-block        |
//! | `--reset-local-adjustment detail`                 | reset the whole detail block  |
//!
//! The ranges are the *global* F-095/F-096 ranges and are enforced by the shared
//! sidecar validators, so the CLI can never write a value the global pipeline
//! would reject. The local detail block deliberately has **no** scale option:
//! the radius follows the global render scale.
//!
//! Local AI-denoise and local optics stay deliberately unimplemented: they have
//! no key here, so a request for one is the generic "unknown local adjustment"
//! error rather than a silent no-op. Optics stays disabled *permanently* — lens
//! correction and perspective are geometric stages ahead of the masks, not a
//! per-mask per-pixel tone stage. AI-denoise stays disabled until the F-078
//! model gate (weight licence, provenance, hash pin) clears.

use super::super::CliError;
use lumina_sidecar::{
    noise_reduction_field_range, sharpening_field_range, NOISE_REDUCTION_FIELDS, SHARPENING_FIELDS,
};

/// The key namespaces that route a local-adjustment spec to the detail block.
const SHARPENING_KEY: &str = "sharpening";
const NOISE_KEY: &str = "noise_reduction";

/// One parsed `--set-local-adjustment sharpening.*|noise_reduction.*` request.
pub(crate) struct LocalDetailSetSpec {
    /// Either [`SHARPENING_KEY`] or [`NOISE_KEY`].
    pub(crate) block: String,
    pub(crate) field: String,
    pub(crate) value: f64,
}

/// One parsed `--reset-local-adjustment sharpening|noise_reduction|detail`
/// request. The whole-block reset is its own variant so the per-area reset
/// cannot silently swallow it.
pub(crate) enum LocalDetailResetSpec {
    Block(&'static str),
    All,
}

/// Split a local-adjustment key into its detail namespace, if any.
///
/// Only the exact keys `sharpening` / `noise_reduction` and their `.` prefixes
/// are detail keys; a key such as `sharpeningx` stays a (rejected) scalar key
/// instead of silently addressing a sub-block.
fn detail_field_of(key: &str) -> Option<(&'static str, &str)> {
    if key == SHARPENING_KEY {
        return Some((SHARPENING_KEY, ""));
    }
    if key == NOISE_KEY {
        return Some((NOISE_KEY, ""));
    }
    if let Some(field) = key.strip_prefix("sharpening.") {
        return Some((SHARPENING_KEY, field));
    }
    key.strip_prefix("noise_reduction.")
        .map(|field| (NOISE_KEY, field))
}

/// Parse one `--set-local-adjustment KEY=VALUE` detail request, or `None` when
/// the key belongs to a different namespace.
pub(crate) fn parse_detail_set_spec(spec: &str) -> Result<Option<LocalDetailSetSpec>, CliError> {
    let Some((key, value)) = spec.split_once('=') else {
        return Ok(None);
    };
    let Some((block, field)) = detail_field_of(key) else {
        return Ok(None);
    };
    let known = if block == SHARPENING_KEY {
        sharpening_field_range(field).is_some()
    } else {
        noise_reduction_field_range(field).is_some()
    };
    if !known {
        let expected: Vec<&str> = if block == SHARPENING_KEY {
            SHARPENING_FIELDS.iter().map(|(name, ..)| *name).collect()
        } else {
            NOISE_REDUCTION_FIELDS
                .iter()
                .map(|(name, ..)| *name)
                .collect()
        };
        return Err(CliError::Message(format!(
            "unknown local {block} field `{field}` in `{spec}`; use one of {}",
            expected.join("|")
        )));
    }
    let value: f64 = value.trim().parse().map_err(|_| {
        CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be a finite number"
        ))
    })?;
    if !value.is_finite() {
        return Err(CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be finite"
        )));
    }
    Ok(Some(LocalDetailSetSpec {
        block: block.into(),
        field: field.into(),
        value,
    }))
}

/// Parse the key of a `--reset-local-adjustment sharpening|noise_reduction|detail`
/// request, or `None` when the key belongs to a different namespace.
pub(crate) fn parse_detail_reset_key(key: &str) -> Result<Option<LocalDetailResetSpec>, CliError> {
    if key == "detail" {
        return Ok(Some(LocalDetailResetSpec::All));
    }
    match detail_field_of(key) {
        None => Ok(None),
        Some((block, "")) => Ok(Some(LocalDetailResetSpec::Block(block))),
        Some((block, field)) => Err(CliError::Message(format!(
            "unknown local {block} field `{field}`; reset the sub-block with `--reset-local-adjustment {block}` or the whole block with `--reset-local-adjustment detail`"
        ))),
    }
}

/// Human-readable label for the history/log action of one applied request.
pub(crate) fn set_label(spec: &LocalDetailSetSpec) -> String {
    format!("local:{}.{}={}", spec.block, spec.field, spec.value)
}

/// Human-readable label for the history/log action of one applied reset.
pub(crate) fn reset_label(spec: &LocalDetailResetSpec) -> String {
    match spec {
        LocalDetailResetSpec::Block(block) => format!("local-reset:detail.{block}"),
        LocalDetailResetSpec::All => "local-reset:detail".into(),
    }
}
