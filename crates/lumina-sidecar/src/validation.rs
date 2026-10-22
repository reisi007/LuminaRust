//! SPLICE-SIDECAR-LIB-90 (Slice 5): the recipe validation block of the
//! sidecar root file, moved verbatim into its own module.
//!
//! One responsibility: the loud (never silent) validation of a persisted
//! recipe — adjustments, spot removals, generative edits, lens blur, the
//! G-01 treatment profile and the G-04 spot extras. Every deviation fails
//! loudly; nothing is clamped, coerced or reinterpreted.
//!
//! The public entry point [`validate_smart_collection_def`] stays reachable
//! under the same path (`lumina_sidecar::validate_smart_collection_def`); the
//! crate-internal validators are re-exported by the root module under their
//! unchanged names, so every existing `crate::validate_*` call site keeps
//! working.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::brush_prompt;
use crate::spot;
use crate::{
    invalid, validate_color_grading, validate_curves, validate_denoise_ai, validate_hsl,
    validate_noise_reduction, validate_point_color, validate_presence, validate_sharpening,
    AiSelect, ArtifactReference, Crop, EditRecipe, GenerativeArtifactRef, MaskPrompt, SidecarError,
    SmartCollectionDef, SmartRule, SourceActionArtifactRef, SpotDistraction, SpotRemoval,
    SpotRemovalMode, BW_STASH_KEY, DEVELOP_PROFILES, DEVELOP_PROFILE_KEY, MAX_COLLECTION_ID_CHARS,
    MAX_COLLECTION_NAME_CHARS, MAX_KEYWORD_CHARS, MAX_SMART_RULE_DEPTH, RED_EYE_MAX_REGIONS,
    SMART_COLLECTION_VERSION, SOURCE_ACTION_VERSION, SPOT_DISTRACTION_KEY, SPOT_REMOVAL_VERSION,
    SPOT_VISUALIZE_KEY, TREATMENT_BW, TREATMENT_COLOR, TREATMENT_KEY, UPRIGHT_MAX_LINE_COUNT,
};

pub(crate) fn validate_name(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.trim().is_empty() {
        invalid(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_unique_ids<T>(
    kind: &str,
    values: &[T],
    id: impl Fn(&T) -> &String,
) -> Result<(), SidecarError> {
    let mut ids = BTreeSet::new();
    for value in values {
        let value_id = id(value);
        if !ids.insert(value_id) {
            return invalid(format!("duplicate {kind} id `{value_id}`"));
        }
    }
    Ok(())
}

pub(crate) fn validate_relative_path(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || value.starts_with("//")
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return invalid(format!("{field} must be a safe portable relative path"));
    }
    Ok(())
}
pub(crate) fn validate_artifact(a: &ArtifactReference) -> Result<(), SidecarError> {
    validate_relative_path("artifact relative_path", &a.relative_path)?;
    validate_name("artifact format", &a.format)?;
    validate_name("artifact checksum", &a.checksum)?;
    validate_name("artifact channels", &a.channels)?;
    validate_name("artifact data_version", &a.data_version)
}

/// G-15 META-MVP (Slice 1): a keyword must be stored trimmed and printable:
/// non-empty, without leading/trailing whitespace or control characters, and
/// within the character limit. Violations fail loudly — the loader never
/// trims, coerces or deduplicates silently.
pub(crate) fn validate_keyword(keyword: &str) -> Result<(), SidecarError> {
    if keyword.is_empty() || keyword.trim() != keyword {
        return invalid("keyword must be non-empty and without leading/trailing whitespace");
    }
    if keyword.chars().any(char::is_control) {
        return invalid("keyword must not contain control characters");
    }
    if keyword.chars().count() > MAX_KEYWORD_CHARS {
        return invalid(format!(
            "keyword exceeds limit of {MAX_KEYWORD_CHARS} characters"
        ));
    }
    Ok(())
}

/// G-15 META-MVP (Slice 1): collection ids are stable, portable identities —
/// never paths. Besides the non-empty/trimmed contract they forbid `/`, `\`
/// and `:` so a membership can never be mistaken for (or turned into) a path.
pub(crate) fn validate_collection_id(id: &str) -> Result<(), SidecarError> {
    if id.is_empty() || id.trim() != id {
        return invalid("collection id must be non-empty and without leading/trailing whitespace");
    }
    if id.contains('/') || id.contains('\\') || id.contains(':') {
        return invalid("collection id must not contain `/`, `\\` or `:`");
    }
    if id.chars().count() > MAX_COLLECTION_ID_CHARS {
        return invalid(format!(
            "collection id exceeds limit of {MAX_COLLECTION_ID_CHARS} characters"
        ));
    }
    Ok(())
}

pub(crate) fn validate_collection_name(name: &str) -> Result<(), SidecarError> {
    if name.is_empty() || name.trim() != name {
        return invalid(
            "collection name must be non-empty and without leading/trailing whitespace",
        );
    }
    if name.chars().count() > MAX_COLLECTION_NAME_CHARS {
        return invalid(format!(
            "collection name exceeds limit of {MAX_COLLECTION_NAME_CHARS} characters"
        ));
    }
    Ok(())
}

/// G-15 META-MVP (Slice 1): validates a smart-collection definition as data —
/// version pin plus a depth-bounded walk of the rule tree. Every unknown or
/// out-of-range part fails loudly; an empty `And`/`Or` is not a vacuous truth
/// but a schema violation. Public so catalogue-level persistence in follow-up
/// slices (CLI/GUI) validates definitions with the same rules.
pub fn validate_smart_collection_def(def: &SmartCollectionDef) -> Result<(), SidecarError> {
    if def.version != SMART_COLLECTION_VERSION {
        return invalid("unsupported smart_collection version");
    }
    validate_collection_id(&def.id)?;
    validate_collection_name(&def.name)?;
    validate_smart_rule(&def.rule, 0)
}

fn validate_smart_rule(rule: &SmartRule, depth: usize) -> Result<(), SidecarError> {
    if depth > MAX_SMART_RULE_DEPTH {
        return invalid(format!(
            "smart rule exceeds maximum nesting depth of {MAX_SMART_RULE_DEPTH}"
        ));
    }
    match rule {
        SmartRule::All | SmartRule::None => Ok(()),
        SmartRule::Keyword { keyword } => validate_keyword(keyword),
        SmartRule::RatingAtLeast { rating } | SmartRule::RatingEquals { rating } => {
            if *rating > 5 {
                return invalid(format!("smart rule rating must be 0..=5, got {rating}"));
            }
            Ok(())
        }
        SmartRule::Flag { .. } => Ok(()),
        SmartRule::And { rules } | SmartRule::Or { rules } => {
            if rules.is_empty() {
                return invalid("smart rule `and`/`or` requires at least one sub-rule");
            }
            for sub in rules {
                validate_smart_rule(sub, depth + 1)?;
            }
            Ok(())
        }
        SmartRule::Not { rule } => validate_smart_rule(rule, depth + 1),
    }
}

/// F-079: reject malformed prompt sources. Normalized coordinates must be
/// finite and within `0..=1`; `width`/`height` and brush `radius` must be
/// strictly positive; required point/mark lists must be non-empty.
pub(crate) fn validate_prompt(prompt: &Option<MaskPrompt>) -> Result<(), SidecarError> {
    let Some(prompt) = prompt else {
        return Ok(());
    };
    let in_unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    match prompt {
        MaskPrompt::Box { rect, .. } => {
            if !in_unit(rect.x)
                || !in_unit(rect.y)
                || !in_unit(rect.width)
                || !in_unit(rect.height)
                || rect.width <= 0.0
                || rect.height <= 0.0
            {
                return invalid("prompt box must have finite normalized coordinates within 0..=1 with positive width/height");
            }
        }
        MaskPrompt::Brush {
            marks, resolution, ..
        } => {
            if resolution.0 == 0 || resolution.1 == 0 {
                return invalid("prompt brush resolution width/height must be strictly positive");
            }
            brush_prompt::validate_brush_marks(marks)?;
        }
        MaskPrompt::Polygon { points, .. } => {
            if points.is_empty() {
                return invalid("prompt polygon must contain at least one point");
            }
            for point in points {
                if !in_unit(point.x) || !in_unit(point.y) {
                    return invalid(
                        "prompt polygon points must have finite normalized coordinates within 0..=1",
                    );
                }
            }
        }
        MaskPrompt::Ellipse { center, radii, .. } => {
            if !in_unit(center.x)
                || !in_unit(center.y)
                || !in_unit(radii.x)
                || !in_unit(radii.y)
                || radii.x <= 0.0
                || radii.y <= 0.0
            {
                return invalid(
                    "prompt ellipse must have finite normalized coordinates within 0..=1 with positive radii",
                );
            }
        }
        MaskPrompt::Gradient {
            angle_deg,
            start,
            end,
            ..
        } => {
            if !angle_deg.is_finite() || !in_unit(*start) || !in_unit(*end) {
                return invalid(
                    "prompt gradient must have a finite angle and finite normalized start/end within 0..=1",
                );
            }
        }
        MaskPrompt::ColorRange {
            hue_center,
            hue_width,
            sat_min,
            sat_max,
            lum_min,
            lum_max,
            feather,
            ..
        } => {
            if !hue_center.is_finite() || !(0.0..=360.0).contains(hue_center) {
                return invalid("prompt color_range hue_center must be finite within 0..=360");
            }
            if !hue_width.is_finite() || !(0.0..=360.0).contains(hue_width) {
                return invalid("prompt color_range hue_width must be finite within 0..=360");
            }
            if !in_unit(*sat_min) || !in_unit(*sat_max) || sat_min > sat_max {
                return invalid(
                    "prompt color_range sat_min/sat_max must be within 0..=1 with sat_min <= sat_max",
                );
            }
            if !in_unit(*lum_min) || !in_unit(*lum_max) || lum_min > lum_max {
                return invalid(
                    "prompt color_range lum_min/lum_max must be within 0..=1 with lum_min <= lum_max",
                );
            }
            if !in_unit(*feather) {
                return invalid("prompt color_range feather must be finite within 0..=1");
            }
        }
        MaskPrompt::LuminanceRange {
            min, max, feather, ..
        } => {
            if !in_unit(*min) || !in_unit(*max) || min > max {
                return invalid(
                    "prompt luminance_range min/max must be within 0..=1 with min <= max",
                );
            }
            if !in_unit(*feather) {
                return invalid("prompt luminance_range feather must be finite within 0..=1");
            }
        }
    }
    Ok(())
}

/// Validates a declarative AI selection (G-03). `detail` accepts the
/// documented part names and any other trimmed, non-empty string without
/// control characters up to 64 chars — unknown parts stay readable and are
/// never silently remapped.
pub(crate) fn validate_ai_select(select: &Option<AiSelect>) -> Result<(), SidecarError> {
    let Some(select) = select else {
        return Ok(());
    };
    if let Some(detail) = &select.detail {
        if detail.len() > 64
            || detail.trim().is_empty()
            || detail != detail.trim()
            || detail.chars().any(|c| c.is_control())
        {
            return invalid(
                "ai_select detail must be trimmed, non-empty, free of control characters and at most 64 chars",
            );
        }
    }
    Ok(())
}

fn validate_source_action_ref(a: &SourceActionArtifactRef) -> Result<(), SidecarError> {
    validate_name("source_action id", &a.id)?;
    validate_relative_path("source_action relative_path", &a.relative_path)?;
    validate_name("source_action checksum", &a.checksum)
}

/// GEN-ZDATA-LINK-1: validates a generative bundle link (zdata `kind = 2/3`).
/// The id selects the record inside the bundle; the remaining fields are the
/// portable `ArtifactReference` payload. `format` must declare a zdata bundle
/// (so `artifact_status` deep-verifies instead of passing an opaque file),
/// `channels`/`data_version` pin the RGBA8 encoding (`rgba8`/`1`), and the
/// declared resolution must be non-zero. Anything else is rejected loudly.
pub(crate) fn validate_generative_ref(a: &GenerativeArtifactRef) -> Result<(), SidecarError> {
    validate_name("generative artifact id", &a.id)?;
    validate_relative_path("generative artifact relative_path", &a.relative_path)?;
    if !a.format.contains("zdata") {
        return invalid("generative artifact format must declare a zdata bundle");
    }
    validate_name("generative artifact checksum", &a.checksum)?;
    if a.width == 0 || a.height == 0 {
        return invalid("generative artifact width/height must be non-zero");
    }
    if a.channels != "rgba8" {
        return invalid("generative artifact channels must be `rgba8`");
    }
    if a.data_version != "1" {
        return invalid("generative artifact data_version must be `1`");
    }
    Ok(())
}

/// GEN-ZDATA-LINK-1: validates one spot removal, including the per-mode
/// exclusion rules — a heuristic spot has no bundle record and MUST NOT
/// carry an artifact link; a generative spot MAY carry the `kind = 3` link
/// (`None` = not generated yet, i.e. `missing` downstream).
fn validate_spot_removal(spot: &SpotRemoval) -> Result<(), SidecarError> {
    if spot.version != SPOT_REMOVAL_VERSION {
        return invalid("unsupported spot_removal version");
    }
    validate_name("spot_removal id", &spot.id)?;
    match spot.mode {
        SpotRemovalMode::Heuristic => {
            if spot.artifact.is_some() {
                return invalid("heuristic spot_removal must not carry an artifact");
            }
        }
        SpotRemovalMode::Generative => {
            if let Some(link) = &spot.artifact {
                validate_generative_ref(link)?;
            }
        }
    }
    Ok(())
}

/// SPOT-SCHEMA-GEOMETRY: validates the geometry-carrying
/// `extras["spot_removals"]` view of a recipe (heal parameters live here; the
/// typed `EditRecipe::spot_removals` holds only id/version/mode/artifact and is
/// checked by `validate_spot_removal`). Runs alongside the typed check from
/// `validate_adjustments`, so both views stay consistent.
///
/// Every rule fails loudly — never a silent fallback or silent reinterpretation:
/// - a present key must hold an array of objects (absent stays identity);
/// - `version` must be present and equal `SPOT_REMOVAL_VERSION`;
/// - `mode`, when absent, defaults to `heuristic` (legacy documents, mirroring
///   the tolerant core reader); any other value than
///   `heuristic`/`generative` is rejected;
/// - a `heuristic` entry MUST carry the full heal geometry (`id` non-empty;
///   `center_x`/`center_y` finite in `0..=1`; `radius` finite in `(0, 512]`;
///   `offset_dx`/`offset_dy` finite in `-1..=1`; `feather`/`opacity` are
///   optional with the core defaults `0.0`/`1.0` but must be finite in
///   `0..=1` when present) and MUST NOT carry `artifact` (no bundle record);
/// - a `generative` entry needs no geometry; an `artifact` link, when present
///   and non-null, must parse as `GenerativeArtifactRef` and pass
///   `validate_generative_ref` (absent/`None` = not generated yet, i.e.
///   `missing` downstream).
fn validate_spot_removal_extras(recipe: &EditRecipe) -> Result<(), SidecarError> {
    let Some(value) = recipe.extras.get("spot_removals") else {
        return Ok(());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| SidecarError::Invalid("extras `spot_removals` must be an array".into()))?;
    for entry in entries {
        validate_spot_removal_extra_entry(entry)?;
    }
    if let Some(id) = spot::duplicate_spot_id(entries) {
        return invalid(format!("duplicate spot_removal id `{id}`"));
    }
    Ok(())
}

fn extra_finite(object: &serde_json::Map<String, Value>, name: &str) -> Result<f64, SidecarError> {
    object
        .get(name)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            SidecarError::Invalid(format!(
                "heuristic spot_removal `{name}` must be present and finite"
            ))
        })
}

fn extra_optional_finite(
    object: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<f64>, SidecarError> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| {
                SidecarError::Invalid(format!("heuristic spot_removal `{name}` must be finite"))
            }),
    }
}

fn validate_spot_removal_extra_entry(entry: &Value) -> Result<(), SidecarError> {
    let object = entry
        .as_object()
        .ok_or_else(|| SidecarError::Invalid("spot_removal entry must be an object".into()))?;
    let version = object
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| SidecarError::Invalid("unsupported spot_removal version".into()))?;
    if version != u64::from(SPOT_REMOVAL_VERSION) {
        return invalid("unsupported spot_removal version");
    }
    let mode = object
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("heuristic");
    match mode {
        "heuristic" => {
            if object.contains_key("artifact") {
                return invalid("heuristic spot_removal must not carry an artifact");
            }
            let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
            if id.trim().is_empty() {
                return invalid("heuristic spot_removal requires a non-empty `id`");
            }
            for name in ["center_x", "center_y"] {
                let v = extra_finite(object, name)?;
                if !(0.0..=1.0).contains(&v) {
                    return invalid(format!(
                        "heuristic spot_removal `{name}` must be within 0..=1"
                    ));
                }
            }
            let radius = extra_finite(object, "radius")?;
            if !(radius > 0.0 && radius <= 512.0) {
                return invalid("heuristic spot_removal `radius` must be within (0, 512]");
            }
            for name in ["offset_dx", "offset_dy"] {
                let v = extra_finite(object, name)?;
                if !(-1.0..=1.0).contains(&v) {
                    return invalid(format!(
                        "heuristic spot_removal `{name}` must be within -1..=1"
                    ));
                }
            }
            // `feather`/`opacity` fall back to the core defaults when absent.
            let feather = extra_optional_finite(object, "feather")?.unwrap_or(0.0);
            if !(0.0..=1.0).contains(&feather) {
                return invalid("heuristic spot_removal `feather` must be within 0..=1");
            }
            let opacity = extra_optional_finite(object, "opacity")?.unwrap_or(1.0);
            if !(0.0..=1.0).contains(&opacity) {
                return invalid("heuristic spot_removal `opacity` must be within 0..=1");
            }
        }
        "generative" => {
            let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
            if id.trim().is_empty() {
                return invalid("generative spot_removal requires a non-empty `id`");
            }
            if let Some(link_value) = object.get("artifact") {
                if !link_value.is_null() {
                    let link: GenerativeArtifactRef = serde_json::from_value(link_value.clone())
                        .map_err(|_| {
                            SidecarError::Invalid("invalid generative spot_removal artifact".into())
                        })?;
                    validate_generative_ref(&link)?;
                }
            }
            // LRPAR-G04-REMOVE: optional deterministic variant controls.
            // Present values are type-checked loudly; absent stays identity.
            if let Some(seed) = object.get("seed") {
                if seed.as_u64().is_none() {
                    return invalid("generative spot_removal `seed` must be a u64");
                }
            }
            if let Some(variant) = object.get("variant") {
                if variant.as_u64().is_none() {
                    return invalid("generative spot_removal `variant` must be a u64");
                }
            }
            if let Some(base_seed) = object.get("base_seed") {
                if base_seed.as_u64().is_none() {
                    return invalid("generative spot_removal `base_seed` must be a u64");
                }
            }
            if let Some(prompt) = object.get("prompt") {
                if prompt.as_str().is_none() {
                    return invalid("generative spot_removal `prompt` must be a string");
                }
            }
        }
        _ => return invalid(format!("unsupported spot_removal mode `{mode}`")),
    }
    Ok(())
}

pub(crate) fn validate_adjustments(a: &EditRecipe) -> Result<(), SidecarError> {
    // REVIEW-SIDECAR-N3: auto-features persist analysis results; a non-finite
    // target luminance would poison every downstream tone computation.
    if !a.auto_features.target_luminance.is_finite() {
        return invalid("invalid auto_features target_luminance");
    }
    // AUTO-TONE-2 Spiegelfelder: Domäne je `-1..=1` wie die gleichnamigen
    // Recipe-Adjustments (`whites`/`blacks`/`highlights`/`shadows`, s.
    // Core-`AutoToneResult`); `None` = kein Auto-Wert persistiert.
    for (name, value) in [
        ("auto_whites", a.auto_features.auto_whites),
        ("auto_blacks", a.auto_features.auto_blacks),
        ("auto_highlights", a.auto_features.auto_highlights),
        ("auto_shadows", a.auto_features.auto_shadows),
    ] {
        if let Some(v) = value {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid(format!("invalid auto_features {name}"));
            }
        }
    }
    for (name, value) in &a.adjustments {
        let (lo, hi) = match name.as_str() {
            "exposure" => (-10.0, 10.0),
            "wb_temperature" => (1500.0, 12000.0),
            "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
            | "vibrance" | "saturation" => (-1.0, 1.0),
            // REVIEW-SIDECAR-N3: unknown adjustment keys stay accepted for
            // forward compatibility (they ride the recipe like extras), but a
            // NaN/∞ value is never a meaningful slider state and would
            // otherwise bypass validation entirely.
            _ => {
                if !value.is_finite() {
                    return invalid(format!("adjustment `{name}` must be finite"));
                }
                continue;
            }
        };
        if !value.is_finite() || !(*value >= lo && *value <= hi) {
            return invalid(format!("invalid adjustment `{name}`"));
        }
    }
    if let Some(c) = &a.curves {
        validate_curves(c)?;
    }
    if let Some(h) = &a.hsl {
        validate_hsl(h)?;
    }
    if let Some(c) = &a.color_grading {
        validate_color_grading(c)?;
    }
    if let Some(p) = &a.point_color {
        validate_point_color(p)?;
    }
    if let Some(p) = &a.presence {
        // The very same validator the mask-local P1.2c block uses, so a local
        // presence can never accept a value the global recipe rejects.
        validate_presence(p)?;
    }
    if let Some(n) = &a.noise_reduction {
        // The very same validator the mask-local P1.2d block uses, so a local
        // noise reduction can never accept a value the global recipe rejects.
        validate_noise_reduction(n)?;
    }
    // LRPAR-G14-DENOISE-20: the optional AI-denoise stage validates its own
    // contract (version, model identity, digest, strengths, artifact reference).
    if let Some(d) = &a.denoise_ai {
        validate_denoise_ai(d)?;
    }
    if let Some(s) = &a.sharpening {
        // The very same validator the mask-local P1.2d block uses, so a local
        // sharpening can never accept a value the global recipe rejects.
        validate_sharpening(s)?;
    }
    // LRPAR-G14-REDEYE-15: out-of-range or non-finite values are rejected
    // loudly, never clipped; regions are identified by stable unique ids.
    if let Some(r) = &a.red_eye {
        if r.version != 1 {
            return invalid("unsupported red_eye version");
        }
        if r.regions.len() > RED_EYE_MAX_REGIONS {
            return invalid("too many red_eye regions (max 32)");
        }
        let mut seen = std::collections::HashSet::new();
        for region in &r.regions {
            if region.id.is_empty() || !seen.insert(region.id.clone()) {
                return invalid("invalid red_eye region id (empty or duplicate)");
            }
            for (field, v) in [("x", region.x), ("y", region.y)] {
                if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                    return invalid(format!("invalid red_eye {} {field}", region.id));
                }
            }
            if !region.radius.is_finite() || region.radius <= 0.0 || region.radius > 1.0 {
                return invalid(format!("invalid red_eye {} radius", region.id));
            }
            for (field, v) in [("desaturate", region.desaturate), ("darken", region.darken)] {
                if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                    return invalid(format!("invalid red_eye {} {field}", region.id));
                }
            }
        }
    }
    if let Some(g) = &a.geometry {
        if g.version != 1
            || !g.rotation_degrees.is_finite()
            || !(-180.0..=180.0).contains(&g.rotation_degrees)
        {
            return invalid("invalid geometry version or rotation");
        }
        if let Some(Crop::Free {
            x,
            y,
            width,
            height,
        }) = &g.crop
        {
            if ![x, y, width, height].iter().all(|v| v.is_finite())
                || *width <= 0.0
                || *height <= 0.0
                || *x < 0.0
                || *y < 0.0
                || *x + *width > 1.0
                || *y + *height > 1.0
            {
                return invalid("invalid geometry free crop");
            }
        }
    }
    if let Some(l) = &a.lens_correction {
        if l.version != 1 || l.profile.as_deref().is_some_and(|p| p.is_empty()) {
            return invalid("invalid lens_correction version or profile");
        }
        if let Some(profile) = l.profile.as_deref() {
            if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
                return invalid("unknown lens correction profile");
            }
        }
        for v in [
            l.distortion_k1,
            l.distortion_k2,
            l.distortion_k3,
            l.vignette_c0,
            l.vignette_c1,
            l.vignette_c2,
        ]
        .into_iter()
        .flatten()
        {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid("invalid lens correction coefficient");
            }
        }
        for v in [l.ca_red, l.ca_blue].into_iter().flatten() {
            if !v.is_finite() || !(-0.05..=0.05).contains(&v) {
                return invalid("invalid chromatic aberration coefficient");
            }
        }
    }
    if let Some(p) = &a.perspective {
        if p.version != 1 {
            return invalid("unsupported perspective version");
        }
        for v in [p.vertical, p.horizontal, p.rotation, p.shift_x, p.shift_y] {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid("invalid perspective coefficient");
            }
        }
        for (v, lo, hi) in [(p.scale, 0.1, 10.0), (p.aspect_ratio, 0.1, 10.0)] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return invalid("invalid perspective scale");
            }
        }
    }
    // LRPAR-G06-UPRIGHT-15: additive upright stage. `enabled` requires a
    // persisted analysis (loud, never a silent identity render); the
    // suggestion stays in the F-099 domain.
    if let Some(u) = &a.upright {
        if u.version != 1 {
            return invalid("unsupported upright version");
        }
        if u.enabled && u.analysis.is_none() {
            return invalid("upright enabled requires a persisted analysis");
        }
        if let Some(analysis) = &u.analysis {
            validate_name("upright algorithm", &analysis.fingerprint.algorithm)?;
            validate_name("upright algorithm version", &analysis.fingerprint.version)?;
            validate_name(
                "upright input fingerprint",
                &analysis.fingerprint.input_fingerprint,
            )?;
            for v in [analysis.vertical, analysis.horizontal, analysis.rotation] {
                if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                    return invalid("invalid upright suggestion coefficient");
                }
            }
            if !analysis.confidence.is_finite() || !(0.0..=1.0).contains(&analysis.confidence) {
                return invalid("invalid upright confidence");
            }
            if analysis.line_count > UPRIGHT_MAX_LINE_COUNT {
                return invalid("invalid upright line_count");
            }
        }
    }
    for action in &a.source_actions {
        if action.version != SOURCE_ACTION_VERSION {
            return invalid("unsupported source_action version");
        }
        validate_source_action_ref(&action.artifact)?;
    }
    let mut spot_ids = BTreeSet::new();
    for spot in &a.spot_removals {
        validate_spot_removal(spot)?;
        if !spot_ids.insert(spot.id.as_str()) {
            return invalid(format!("duplicate spot_removal id `{}`", spot.id));
        }
    }
    validate_spot_removal_extras(a)?;
    validate_spot_g04_extras(a)?;
    validate_treatment_profile(a)?;
    if let Some(g) = &a.generative_edit {
        if g.version != 1 {
            return invalid("unsupported generative_edit version");
        }
        if let Some(canvas) = &g.canvas {
            canvas.validate()?;
        }
        if let Some(link) = &g.artifact {
            validate_generative_ref(link)?;
        }
        let expand = g.expand_beyond_image.unwrap_or(false);
        if expand {
            if g.canvas.is_none() {
                return invalid("expand_beyond_image true requires a canvas");
            }
        } else if g.canvas.is_some() {
            return invalid("canvas present without expand_beyond_image true");
        }
    }
    if let Some(e) = &a.effects {
        if let Some(v) = &e.vignette {
            if v.version != 1 {
                return invalid("unsupported vignette version");
            }
            for (name, val, lo, hi) in [
                ("amount", v.amount, -1.0, 1.0),
                ("midpoint", v.midpoint, 0.0, 1.0),
                ("roundness", v.roundness, -1.0, 1.0),
                ("feather", v.feather, 0.0, 1.0),
            ] {
                if !val.is_finite() || !(lo..=hi).contains(&val) {
                    return invalid(format!("invalid vignette {name}"));
                }
            }
        }
        if let Some(g) = &e.grain {
            if g.version != 1 {
                return invalid("unsupported grain version");
            }
            for (name, val, lo, hi) in [
                ("amount", g.amount, 0.0, 1.0),
                ("size", g.size, 0.0, 1.0),
                ("roughness", g.roughness, 0.0, 1.0),
            ] {
                if !val.is_finite() || !(lo..=hi).contains(&val) {
                    return invalid(format!("invalid grain {name}"));
                }
            }
        }
    }
    if let Some(b) = &a.lens_blur {
        if b.version != 1 {
            return invalid("unsupported lens_blur version");
        }
        let r = &b.focus_rect;
        for (name, v) in [
            ("x", r.x),
            ("y", r.y),
            ("width", r.width),
            ("height", r.height),
        ] {
            if !v.is_finite() {
                return invalid(format!("invalid lens_blur focus_rect.{name}"));
            }
        }
        if !(0.0..=1.0).contains(&r.x)
            || !(0.0..=1.0).contains(&r.y)
            || r.width <= 0.0
            || r.height <= 0.0
            || r.x + r.width > 1.0
            || r.y + r.height > 1.0
        {
            return invalid("invalid lens_blur focus_rect (must be a positive area inside 0..=1)");
        }
        for (name, v, lo, hi) in [
            ("focal_near", b.focal_near, 0.0, 1.0),
            ("focal_far", b.focal_far, 0.0, 1.0),
            ("blur_amount", b.blur_amount, 0.0, 1.0),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return invalid(format!("invalid lens_blur {name}"));
            }
        }
        if b.focal_near > b.focal_far {
            return invalid("invalid lens_blur focal range (focal_near must be <= focal_far)");
        }
        if let Some(d) = &b.depth_artifact {
            validate_relative_path("lens_blur depth_artifact relative_path", &d.relative_path)?;
            validate_name("lens_blur depth_artifact sha256", &d.sha256)?;
        }
    }
    Ok(())
}
/// LRPAR-G01-BASIC: validates the additive G-01 recipe fields —
/// `extras["treatment"]` (`"color"`|`"bw"` when present), `extras["bw_stash"]`
/// (object with optional finite `-1..=1` `saturation`/`vibrance` when
/// present) and `options["profile"]` (whitelisted name when present).
/// Every deviation fails loudly, never a silent reinterpretation.
fn validate_treatment_profile(recipe: &EditRecipe) -> Result<(), SidecarError> {
    if let Some(value) = recipe.extras.get(TREATMENT_KEY) {
        let ok = value
            .as_str()
            .is_some_and(|t| t == TREATMENT_COLOR || t == TREATMENT_BW);
        if !ok {
            return invalid(format!("extras `{TREATMENT_KEY}` must be `color` or `bw`"));
        }
    }
    if let Some(value) = recipe.extras.get(BW_STASH_KEY) {
        let map = serde_json::from_value::<BTreeMap<String, Option<f64>>>(value.clone()).map_err(
            |_| SidecarError::Invalid(format!("extras `{BW_STASH_KEY}` must be an object")),
        )?;
        for key in ["saturation", "vibrance"] {
            if let Some(Some(v)) = map.get(key) {
                if !v.is_finite() || !(-1.0..=1.0).contains(v) {
                    return invalid(format!(
                        "extras `{BW_STASH_KEY}.{key}` must be finite within -1..=1"
                    ));
                }
            }
        }
    }
    if let Some(profile) = recipe.options.get(DEVELOP_PROFILE_KEY) {
        if !DEVELOP_PROFILES.contains(&profile.as_str()) {
            return invalid(format!(
                "options `{DEVELOP_PROFILE_KEY}` must be one of {}",
                DEVELOP_PROFILES.join("|")
            ));
        }
    }
    Ok(())
}

/// LRPAR-G04-REMOVE: validates the additive G-04 recipe extras —
/// `spot_visualize_threshold` (finite `0..=1` when present, absent = off) and
/// `spot_distraction` (an object of bools when present, absent = all off).
/// Every deviation fails loudly, never a silent reinterpretation.
fn validate_spot_g04_extras(recipe: &EditRecipe) -> Result<(), SidecarError> {
    if let Some(value) = recipe.extras.get(SPOT_VISUALIZE_KEY) {
        let v = value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
            SidecarError::Invalid(format!("extras `{SPOT_VISUALIZE_KEY}` must be finite"))
        })?;
        if !(0.0..=1.0).contains(&v) {
            return invalid(format!(
                "extras `{SPOT_VISUALIZE_KEY}` must be within 0..=1"
            ));
        }
    }
    if let Some(value) = recipe.extras.get(SPOT_DISTRACTION_KEY) {
        serde_json::from_value::<SpotDistraction>(value.clone()).map_err(|_| {
            SidecarError::Invalid(format!(
                "extras `{SPOT_DISTRACTION_KEY}` must be an object of bools"
            ))
        })?;
    }
    Ok(())
}
