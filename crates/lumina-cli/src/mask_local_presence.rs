//! MASK-LOCAL-P1.2c CLI parsing of the local presence block.
//!
//! Like the local tone curve and the local colour block (see
//! `mask_local_curves` / `mask_local_color`), the local presence is reached
//! through the existing generic `--set-local-adjustment KEY=VALUE` /
//! `--reset-local-adjustment KEY` flags with its own `presence.` key
//! namespace. That keeps the CLI surface free of a second, near-identical flag
//! party and prevents two grammars from drifting apart.
//!
//! Accepted keys:
//!
//! | spec                                    | effect                          |
//! |-----------------------------------------|---------------------------------|
//! | `--set-local-adjustment presence.texture=0.5`  | local texture amount     |
//! | `--set-local-adjustment presence.clarity=-0.25`| local clarity amount     |
//! | `--set-local-adjustment presence.dehaze=0.4`   | local dehaze amount      |
//! | `--reset-local-adjustment presence`     | reset the whole presence block   |
//!
//! The local detail, sharpening, noise-reduction, AI-denoise and optics
//! controls stay deliberately unimplemented: they have no key here, so a
//! request for one is the generic "unknown local adjustment" error rather than
//! a silent no-op. Optics stays disabled *permanently* — lens correction and
//! perspective are geometric stages ahead of the masks, not a per-mask
//! per-pixel tone stage. Noise reduction and AI-denoise stay disabled until the
//! F-078 model gate (weight licence, provenance, hash pin) clears.

use super::super::CliError;
use lumina_sidecar::PRESENCE_FIELDS;

/// The key namespace that routes a local-adjustment spec to the presence block.
const PRESENCE_KEY: &str = "presence";

/// One parsed `--set-local-adjustment presence.*` request.
pub(crate) struct LocalPresenceSetSpec {
    pub(crate) field: String,
    pub(crate) value: f64,
}

/// Split a local-adjustment key into its presence namespace, if any.
///
/// Only the exact key `presence` and the prefix `presence.` are presence keys;
/// a key such as `presencex` stays a (rejected) scalar key instead of silently
/// addressing the whole block.
fn presence_field_of(key: &str) -> Option<&str> {
    if key == PRESENCE_KEY {
        return Some("");
    }
    key.strip_prefix(&format!("{PRESENCE_KEY}."))
}

/// Parse one `--set-local-adjustment KEY=VALUE` presence request, or `None` when
/// the key belongs to a different namespace.
pub(crate) fn parse_presence_set_spec(
    spec: &str,
) -> Result<Option<LocalPresenceSetSpec>, CliError> {
    let Some((key, value)) = spec.split_once('=') else {
        return Ok(None);
    };
    let Some(field) = presence_field_of(key) else {
        return Ok(None);
    };
    if field.is_empty() {
        return Err(CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; `presence` needs a field, use `presence.<texture|clarity|dehaze>`"
        )));
    }
    if !PRESENCE_FIELDS.iter().any(|(name, _)| *name == field) {
        return Err(CliError::Message(format!(
            "unknown local presence field `{field}` in `{spec}`"
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
    Ok(Some(LocalPresenceSetSpec {
        field: field.into(),
        value,
    }))
}

/// Parse the key of a `--reset-local-adjustment presence` request, or `None`
/// when the key belongs to a different namespace.
pub(crate) fn parse_presence_reset_key(key: &str) -> Result<Option<()>, CliError> {
    match presence_field_of(key) {
        None => Ok(None),
        Some("") => Ok(Some(())),
        Some(field) => Err(CliError::Message(format!(
            "unknown local presence field `{field}`; reset the whole block with `--reset-local-adjustment presence`"
        ))),
    }
}

/// Human-readable label for the history/log action of one applied request.
pub(crate) fn set_label(spec: &LocalPresenceSetSpec) -> String {
    format!("local:presence.{}={}", spec.field, spec.value)
}

/// Human-readable label for the history/log action of one applied reset.
pub(crate) fn reset_label() -> String {
    "local-reset:presence".into()
}
