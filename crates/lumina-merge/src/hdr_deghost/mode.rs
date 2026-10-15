use lumina_sidecar::{HdrDeghost, MergeMode, DEGHOST_MAX_THRESHOLD, DEGHOST_MIN_THRESHOLD};

/// Slice B: HDR deghosting mode (one enum, two spellings). `Off` is the
/// default on every frontend (no behaviour change); the only variant in 1.0
/// is a threshold in `(DEGHOST_MIN_THRESHOLD, DEGHOST_MAX_THRESHOLD]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum HdrDeghostMode {
    #[default]
    Off,
    Threshold(f64),
}

impl HdrDeghostMode {
    /// Parses the shared CLI spelling (`off` | `<threshold-float>`).
    /// Unknown values are a loud `Err` with the accepted values and the
    /// documented range — the CLI maps it onto its usage error, the GUI onto
    /// its own string. One message for both spellings of the field
    /// (`--deghost` flag / MCP `deghost` key).
    ///
    /// The range check is deliberately here *and* in `recipe.validate()`: a
    /// frontend refuses with its own class (CLI usage / MCP `InvalidParams`),
    /// and a persisted recipe is still refused loudly by the schema
    /// (`Invalid` → `merge failed:`). Neither clamps nor defaults.
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed == "off" {
            return Ok(HdrDeghostMode::Off);
        }
        let threshold: f64 = trimmed.parse().map_err(|_| {
            format!(
                "invalid `deghost` value `{value}`: expected `off` or a threshold in \
                 ({DEGHOST_MIN_THRESHOLD:.2}, {DEGHOST_MAX_THRESHOLD:.1}]"
            )
        })?;
        Self::threshold(threshold)
            .map_err(|reason| format!("invalid `deghost` value `{value}`: {reason}"))
    }

    /// The same rule for a frontend that already holds a number (the MCP wire
    /// form carries a JSON number, the CLI a text flag). Loud, never clamped.
    pub fn threshold(threshold: f64) -> Result<Self, String> {
        if !threshold.is_finite()
            || threshold <= DEGHOST_MIN_THRESHOLD
            || threshold > DEGHOST_MAX_THRESHOLD
        {
            return Err(format!(
                "threshold must be finite within ({DEGHOST_MIN_THRESHOLD:.2}, {DEGHOST_MAX_THRESHOLD:.1}], got {threshold} (never clamped)"
            ));
        }
        Ok(HdrDeghostMode::Threshold(threshold))
    }

    /// The persisted recipe value (`None` = off). The fallback policy is fixed
    /// in 1.0 but persisted explicitly — see [`HdrDeghost`].
    pub fn recipe_value(self) -> Option<HdrDeghost> {
        match self {
            HdrDeghostMode::Off => None,
            HdrDeghostMode::Threshold(threshold) => Some(HdrDeghost {
                threshold,
                reference_fallback: true,
            }),
        }
    }
}

/// Slice B: the HDR-only options gate (runs in `run_merge` before any decode
/// or file write, exactly like the `max_shift_px < 0` and the Slice-A gain
/// gates). Deghosting needs a bracket; a panorama set is stitched, not
/// bracketed, so asking for it there is a loud `merge failed:`.
pub(crate) fn check_deghost_mode(mode: MergeMode, deghost: HdrDeghostMode) -> Result<(), String> {
    if mode == MergeMode::Panorama && deghost != HdrDeghostMode::Off {
        return Err(
            "invalid --deghost: deghosting is HDR only (panorama frames are stitched, not bracketed)"
                .into(),
        );
    }
    Ok(())
}
