//! Full schema validation at the GPU entry (GPU-RENDER-PARITY-1 follow-up).
//!
//! The GPU pipeline renders a growing set of stages in WGSL. Those shaders
//! quantize and clamp in the `0..=255` byte domain and precompute fixed-capacity
//! parameter blocks, so a schema-invalid recipe (out-of-range/NaN value, unknown
//! key, duplicate id, malformed curve, …) would be **silently clamped** where the
//! CPU oracle rejects it with [`lumina_core::CoreError::InvalidAdjustment`].
//!
//! [`validate_gpu_recipe`] closes that gap: it mirrors the platform-neutral
//! validation the CPU reference runs before touching a pixel
//! (`ImageFrame::apply_recipe_with_scale_and_white_balance` →
//! `validate_nested_adjustments`, plus the spot-mode and generative/lens-blur
//! checks in `lumina-core::render` / `lens_blur`). The errors it returns are the
//! **same** [`lumina_core::CoreError`] values the CPU oracle produces, so a
//! schema-invalid recipe fails identically on both backends instead of being
//! clamped on the GPU or routed to the CPU to hide the difference.
//!
//! Range/semantics are intentionally a verbatim port; when `lumina-core` gains a
//! validated field, this module must be extended in the same change.
//!
//! One documented exception: `validate_curve` collapses the endpoint errors to
//! a single `"{name}.points"` payload (`-1.0`/`0.0`/`1.0`) and clamps a
//! non-monotone input to `f64::MIN_POSITIVE`, where the Core validator reports
//! `"{name}.points[0]"`/`"{name}.points[len-1]"` and clamps to the previous
//! input. Accept/reject behavior is identical (no parity break) — only the
//! error payload differs.

use lumina_core::CoreError;
use lumina_sidecar::EditRecipe;

use crate::GpuError;

fn invalid(name: impl Into<String>, value: f64, minimum: f64, maximum: f64) -> GpuError {
    GpuError::Core(CoreError::InvalidAdjustment {
        name: name.into(),
        value,
        minimum,
        maximum,
    })
}

fn unsupported(key: impl Into<String>) -> GpuError {
    GpuError::Core(CoreError::UnsupportedAdjustment { key: key.into() })
}

/// Validate a recipe against the exact schema ranges the CPU reference enforces,
/// returning the same [`CoreError`] the CPU oracle would.
///
/// Callers on the GPU path must run this **before** building any GPU
/// parameters, so no invalid value can ever reach a shader's fixed-capacity
/// encoding.
pub fn validate_gpu_recipe(recipe: &EditRecipe) -> Result<(), GpuError> {
    // Order mirrors the CPU reference: spot modes first (`apply_spot_heals_from_
    // recipe` in `render_frame_from_base`), then the top-level adjustment map and
    // the nested validators (`apply_recipe_with_white_balance`).
    validate_spot_modes(recipe)?;
    validate_adjustment_map(recipe)?;
    validate_nested(recipe)?;
    Ok(())
}

/// Top-level `recipe.adjustments` keys/value ranges (core `apply_recipe`).
fn validate_adjustment_map(recipe: &EditRecipe) -> Result<(), GpuError> {
    for (key, value) in &recipe.adjustments {
        let (minimum, maximum) = match key.as_str() {
            "exposure" => (-10.0, 10.0),
            "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
            | "vibrance" | "saturation" => (-1.0, 1.0),
            "wb_temperature" => (1500.0, 12000.0),
            _ => return Err(unsupported(key.clone())),
        };
        if !value.is_finite() || !(minimum..=maximum).contains(value) {
            return Err(invalid(key.clone(), *value, minimum, maximum));
        }
    }
    Ok(())
}

/// Verbatim port of `lumina_core::validate_nested_adjustments` plus the
/// stage-specific validators it delegates to (lens, lens blur, perspective,
/// generative edit, geometry). Every arm returns the CPU's [`CoreError`].
fn validate_nested(recipe: &EditRecipe) -> Result<(), GpuError> {
    if let Some(l) = &recipe.lens_correction {
        validate_lens(l)?;
    }
    if let Some(b) = &recipe.lens_blur {
        lumina_core::validate_lens_blur(b).map_err(GpuError::Core)?;
    }
    if let Some(p) = &recipe.perspective {
        validate_perspective(p)?;
    }
    if let Some(u) = &recipe.upright {
        // LRPAR-G06-UPRIGHT-15: the core validator is the single source of
        // truth for the additive upright stage, so the GPU entry returns the
        // exact CPU `CoreError` (including `enabled` without `analysis`).
        lumina_core::validate_upright(u).map_err(GpuError::Core)?;
    }
    if let Some(g) = &recipe.generative_edit {
        validate_generative_edit(g)?;
    }
    if let Some(g) = &recipe.geometry {
        if g.version != 1
            || !g.rotation_degrees.is_finite()
            || !(-180.0..=180.0).contains(&g.rotation_degrees)
        {
            return Err(invalid(
                "geometry.version/rotation",
                g.rotation_degrees as f64,
                -180.0,
                180.0,
            ));
        }
        if let Some(lumina_sidecar::Crop::Free {
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
                return Err(invalid("geometry.crop", -1.0, 0.0, 1.0));
            }
        }
    }
    if let Some(curves) = &recipe.curves {
        if curves.version != 1 {
            return Err(invalid("curves.version", curves.version as f64, 1.0, 1.0));
        }
        validate_curve("curves.master", &curves.master)?;
        for (name, curve) in [
            ("curves.channels.red", &curves.channels.red),
            ("curves.channels.green", &curves.channels.green),
            ("curves.channels.blue", &curves.channels.blue),
        ] {
            if let Some(curve) = curve {
                validate_curve(name, curve)?;
            }
        }
    }
    if let Some(hsl) = &recipe.hsl {
        if hsl.version != 1 {
            return Err(invalid("hsl.version", hsl.version as f64, 1.0, 1.0));
        }
        for (name, channel) in [
            ("hsl.red", &hsl.red),
            ("hsl.orange", &hsl.orange),
            ("hsl.yellow", &hsl.yellow),
            ("hsl.green", &hsl.green),
            ("hsl.cyan", &hsl.cyan),
            ("hsl.blue", &hsl.blue),
            ("hsl.violet", &hsl.violet),
            ("hsl.magenta", &hsl.magenta),
        ] {
            if let Some(channel) = channel {
                for (field, value) in [
                    ("hue", channel.hue),
                    ("saturation", channel.saturation),
                    ("luminance", channel.luminance),
                ] {
                    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                        return Err(invalid(format!("{name}.{field}"), value as f64, -1.0, 1.0));
                    }
                }
            }
        }
    }
    if let Some(p) = &recipe.presence {
        if p.version != 1 {
            return Err(invalid("presence.version", p.version as f64, 1.0, 1.0));
        }
        for (name, value) in [
            ("texture", p.texture),
            ("clarity", p.clarity),
            ("dehaze", p.dehaze),
        ] {
            if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                return Err(invalid(format!("presence.{name}"), value as f64, -1.0, 1.0));
            }
        }
    }
    if let Some(c) = &recipe.color_grading {
        if c.version != 1 {
            return Err(invalid("color_grading.version", c.version as f64, 1.0, 1.0));
        }
        if !c.balance.is_finite() || !(-1.0..=1.0).contains(&c.balance) {
            return Err(invalid(
                "color_grading.balance",
                c.balance as f64,
                -1.0,
                1.0,
            ));
        }
        if !c.blending.is_finite() || !(0.0..=1.0).contains(&c.blending) {
            return Err(invalid(
                "color_grading.blending",
                c.blending as f64,
                0.0,
                1.0,
            ));
        }
        for (name, range) in [
            ("shadows", c.shadows),
            ("midtones", c.midtones),
            ("highlights", c.highlights),
        ] {
            if !range.hue_degrees.is_finite() || !(0.0..=360.0).contains(&range.hue_degrees) {
                return Err(invalid(
                    format!("color_grading.{name}.hue_degrees"),
                    range.hue_degrees as f64,
                    0.0,
                    360.0,
                ));
            }
            if !range.saturation.is_finite() || !(0.0..=1.0).contains(&range.saturation) {
                return Err(invalid(
                    format!("color_grading.{name}.saturation"),
                    range.saturation as f64,
                    0.0,
                    1.0,
                ));
            }
            if !range.luminance.is_finite() || !(-1.0..=1.0).contains(&range.luminance) {
                return Err(invalid(
                    format!("color_grading.{name}.luminance"),
                    range.luminance as f64,
                    -1.0,
                    1.0,
                ));
            }
        }
    }
    if let Some(p) = &recipe.point_color {
        if p.version != 1 {
            return Err(invalid("point_color.version", p.version as f64, 1.0, 1.0));
        }
        if p.entries.len() > 8 {
            return Err(invalid(
                "point_color.entries",
                p.entries.len() as f64,
                0.0,
                8.0,
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for entry in &p.entries {
            if entry.id.is_empty() || !seen.insert(entry.id.clone()) {
                return Err(unsupported(format!(
                    "point_color entry id `{}` (empty or duplicate)",
                    entry.id
                )));
            }
            for (field, value, lo, hi) in [
                ("hue_center", entry.hue_center, 0.0_f32, 360.0_f32),
                ("hue_range", entry.hue_range, 0.0_f32, 180.0_f32),
                ("hue_shift", entry.hue_shift, -1.0_f32, 1.0_f32),
                (
                    "saturation_shift",
                    entry.saturation_shift,
                    -1.0_f32,
                    1.0_f32,
                ),
                ("luminance_shift", entry.luminance_shift, -1.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(invalid(
                        format!("point_color.{}.{}", entry.id, field),
                        value as f64,
                        lo as f64,
                        hi as f64,
                    ));
                }
            }
        }
    }
    if let Some(n) = &recipe.noise_reduction {
        if n.version != 1 {
            return Err(invalid(
                "noise_reduction.version",
                n.version as f64,
                1.0,
                1.0,
            ));
        }
        for (name, value) in [("luminance", n.luminance), ("color", n.color)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(invalid(
                    format!("noise_reduction.{name}"),
                    value as f64,
                    0.0,
                    1.0,
                ));
            }
        }
    }
    // LRPAR-G14-DENOISE-IMPL-20: mirror the CPU oracle's `denoise_ai`
    // validation (`lumina_core::validate_nested_adjustments`) so a
    // directly-constructed (not sidecar-loaded) invalid recipe fails with the
    // **same** `CoreError::Denoise { status: "invalid" }` on the GPU entry as on
    // the CPU path — never a clamped/ignored stage. The stage itself is CPU-
    // routed (routing gate) until a GPU pass exists.
    if let Some(d) = &recipe.denoise_ai {
        d.validate().map_err(|error| {
            GpuError::Core(CoreError::Denoise {
                status: "invalid".into(),
                reason: error.to_string(),
            })
        })?;
    }
    if let Some(s) = &recipe.sharpening {
        if s.version != 1 {
            return Err(invalid("sharpening.version", s.version as f64, 1.0, 1.0));
        }
        for (name, value, lo, hi) in [
            ("amount", s.amount, 0.0, 3.0),
            ("radius", s.radius, 0.1, 10.0),
            ("detail", s.detail, 0.0, 1.0),
            ("masking", s.masking, 0.0, 1.0),
        ] {
            if !value.is_finite() || !(lo..=hi).contains(&value) {
                return Err(invalid(
                    format!("sharpening.{name}"),
                    value as f64,
                    lo as f64,
                    hi as f64,
                ));
            }
        }
    }
    if let Some(r) = &recipe.red_eye {
        if r.version != 1 {
            return Err(invalid("red_eye.version", r.version as f64, 1.0, 1.0));
        }
        if r.regions.len() > lumina_sidecar::RED_EYE_MAX_REGIONS {
            return Err(invalid(
                "red_eye.regions",
                r.regions.len() as f64,
                0.0,
                lumina_sidecar::RED_EYE_MAX_REGIONS as f64,
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for region in &r.regions {
            if region.id.is_empty() || !seen.insert(region.id.clone()) {
                return Err(unsupported(format!(
                    "red_eye region id `{}` (empty or duplicate)",
                    region.id
                )));
            }
            for (field, value, lo, hi) in [
                ("x", region.x, 0.0_f32, 1.0_f32),
                ("y", region.y, 0.0_f32, 1.0_f32),
                ("desaturate", region.desaturate, 0.0_f32, 1.0_f32),
                ("darken", region.darken, 0.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(invalid(
                        format!("red_eye.{}.{}", region.id, field),
                        value as f64,
                        lo as f64,
                        hi as f64,
                    ));
                }
            }
            if !region.radius.is_finite() || region.radius <= 0.0 || region.radius > 1.0 {
                return Err(invalid(
                    format!("red_eye.{}.radius", region.id),
                    region.radius as f64,
                    f32::MIN_POSITIVE as f64,
                    1.0,
                ));
            }
        }
    }
    if let Some(e) = &recipe.effects {
        if let Some(v) = &e.vignette {
            if v.version != 1 {
                return Err(invalid(
                    "effects.vignette.version",
                    v.version as f64,
                    1.0,
                    1.0,
                ));
            }
            for (name, value, lo, hi) in [
                ("effects.vignette.amount", v.amount, -1.0_f32, 1.0_f32),
                ("effects.vignette.midpoint", v.midpoint, 0.0_f32, 1.0_f32),
                ("effects.vignette.roundness", v.roundness, -1.0_f32, 1.0_f32),
                ("effects.vignette.feather", v.feather, 0.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(invalid(name, value as f64, lo as f64, hi as f64));
                }
            }
        }
        if let Some(g) = &e.grain {
            if g.version != 1 {
                return Err(invalid("effects.grain.version", g.version as f64, 1.0, 1.0));
            }
            for (name, value, lo, hi) in [
                ("effects.grain.amount", g.amount, 0.0_f32, 1.0_f32),
                ("effects.grain.size", g.size, 0.0_f32, 1.0_f32),
                ("effects.grain.roughness", g.roughness, 0.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(invalid(name, value as f64, lo as f64, hi as f64));
                }
            }
        }
    }
    Ok(())
}

fn validate_curve(name: &str, curve: &[lumina_sidecar::CurvePoint]) -> Result<(), GpuError> {
    if !(2..=32).contains(&curve.len()) {
        return Err(invalid(
            format!("{name}.points"),
            curve.len() as f64,
            2.0,
            32.0,
        ));
    }
    for (index, point) in curve.iter().enumerate() {
        for (field, value) in [("input", point.input), ("output", point.output)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(invalid(
                    format!("{name}.points[{index}].{field}"),
                    value as f64,
                    0.0,
                    1.0,
                ));
            }
        }
        if index > 0 && point.input <= curve[index - 1].input {
            return Err(invalid(
                format!("{name}.points[{index}].input"),
                point.input as f64,
                f64::MIN_POSITIVE,
                1.0,
            ));
        }
    }
    if curve.first().map(|p| (p.input, p.output)) != Some((0.0, 0.0))
        || curve.last().map(|p| (p.input, p.output)) != Some((1.0, 1.0))
    {
        return Err(invalid(format!("{name}.points"), -1.0, 0.0, 1.0));
    }
    Ok(())
}

fn validate_lens(l: &lumina_sidecar::LensCorrection) -> Result<(), GpuError> {
    if l.version != 1 {
        return Err(invalid(
            "lens_correction.version",
            l.version as f64,
            1.0,
            1.0,
        ));
    }
    if let Some(profile) = l.profile.as_deref() {
        if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
            return Err(unsupported(format!("lens profile `{profile}`")));
        }
    }
    for (name, v, lo, hi) in [
        ("distortion_k1", l.distortion_k1, -1., 1.),
        ("distortion_k2", l.distortion_k2, -1., 1.),
        ("distortion_k3", l.distortion_k3, -1., 1.),
        ("vignette_c0", l.vignette_c0, -1., 1.),
        ("vignette_c1", l.vignette_c1, -1., 1.),
        ("vignette_c2", l.vignette_c2, -1., 1.),
        ("ca_red", l.ca_red, -0.05, 0.05),
        ("ca_blue", l.ca_blue, -0.05, 0.05),
    ]
    .into_iter()
    .filter_map(|(name, value, lo, hi)| value.map(|v| (name, v, lo, hi)))
    {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(invalid(name, v as f64, lo as f64, hi as f64));
        }
    }
    Ok(())
}

fn validate_perspective(p: &lumina_sidecar::Perspective) -> Result<(), GpuError> {
    if p.version != 1 {
        return Err(invalid("perspective.version", p.version as f64, 1.0, 1.0));
    }
    for (name, v, lo, hi) in [
        ("vertical", p.vertical, -1., 1.),
        ("horizontal", p.horizontal, -1., 1.),
        ("rotation", p.rotation, -1., 1.),
        ("shift_x", p.shift_x, -1., 1.),
        ("shift_y", p.shift_y, -1., 1.),
        ("scale", p.scale, 0.1, 10.),
        ("aspect_ratio", p.aspect_ratio, 0.1, 10.),
    ] {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(invalid(name, v as f64, lo as f64, hi as f64));
        }
    }
    Ok(())
}

fn validate_generative_edit(g: &lumina_sidecar::GenerativeEdit) -> Result<(), GpuError> {
    if g.version != 1 {
        return Err(invalid(
            "generative_edit.version",
            g.version as f64,
            1.0,
            1.0,
        ));
    }
    let expand = g.expand_beyond_image.unwrap_or(false);
    if expand && g.canvas.is_none() {
        return Err(invalid("generative_edit.canvas", 0.0, 1.0, 1.0));
    }
    if !expand && g.canvas.is_some() {
        return Err(invalid("generative_edit.canvas", 1.0, 0.0, 0.0));
    }
    if let Some(canvas) = &g.canvas {
        if canvas.output_width == 0 || canvas.output_height == 0 {
            return Err(invalid(
                "generative_edit.canvas.output",
                0.0,
                1.0,
                f64::from(u32::MAX),
            ));
        }
    }
    Ok(())
}

/// Spot-mode validation (verbatim port of
/// `lumina_core::render::reject_unsupported_spot_modes`).
///
/// The GPU spot stage renders the legacy `extras["spot_removals"]` geometry
/// exactly like the CPU oracle. A typed schema-v2 `spot_removals` entry is a
/// geometry-free mirror shadow and is tolerated only when the extras view
/// carries the heal geometry; an isolated typed heuristic (no geometry anywhere)
/// or a generative/unknown-version typed entry is a hard error on **both**
/// backends, never a silent no-heal.
fn validate_spot_modes(recipe: &EditRecipe) -> Result<(), GpuError> {
    if let Some(value) = recipe.extras.get("spot_removals") {
        let arr = value
            .as_array()
            .ok_or_else(|| invalid("spot_removals", -1.0, 0.0, 0.0))?;
        for entry in arr {
            let mode = entry
                .get("mode")
                .and_then(|m| m.as_str())
                .unwrap_or("heuristic");
            if mode != "heuristic" {
                return Err(invalid("spot_heal.mode", -1.0, 0.0, 0.0));
            }
            let spot: lumina_core::SpotHeuristic = serde_json::from_value(entry.clone())
                .map_err(|_| invalid("spot_heal.entry", -1.0, 0.0, 0.0))?;
            spot.validate().map_err(GpuError::Core)?;
        }
    }
    let has_extras_geometry = recipe.extras.contains_key("spot_removals");
    for entry in &recipe.spot_removals {
        if entry.version != lumina_sidecar::SPOT_REMOVAL_VERSION {
            return Err(invalid("spot_heal.version", entry.version as f64, 1.0, 1.0));
        }
        match entry.mode {
            lumina_sidecar::SpotRemovalMode::Generative => {
                return Err(invalid("spot_heal.mode", -1.0, 0.0, 0.0));
            }
            lumina_sidecar::SpotRemovalMode::Heuristic => {
                let has_geometry = serde_json::to_value(entry)
                    .ok()
                    .and_then(|v| v.as_object().cloned())
                    .is_some_and(|object| {
                        [
                            "center",
                            "center_x",
                            "center_y",
                            "radius",
                            "feather",
                            "offset_dx",
                            "offset_dy",
                            "source_offset",
                            "opacity",
                            "status",
                        ]
                        .iter()
                        .any(|key| object.contains_key(*key))
                    });
                if has_geometry || !has_extras_geometry {
                    return Err(invalid("spot_heal.entry", -1.0, 0.0, 0.0));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn valid_default_recipe_passes() {
        validate_gpu_recipe(&EditRecipe::default()).expect("empty recipe is valid");
    }

    #[test]
    fn unknown_adjustment_key_is_rejected_like_cpu() {
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("definitely_not_a_key".into(), 0.5)]),
            ..Default::default()
        };
        let err = validate_gpu_recipe(&recipe).expect_err("unknown key must be rejected");
        assert!(
            matches!(err, GpuError::Core(CoreError::UnsupportedAdjustment { .. })),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn out_of_range_sharpening_radius_is_rejected() {
        let recipe = EditRecipe {
            sharpening: Some(lumina_sidecar::Sharpening {
                version: 1,
                amount: 1.0,
                radius: 12.0,
                detail: 0.5,
                masking: 0.0,
            }),
            ..Default::default()
        };
        let err = validate_gpu_recipe(&recipe).expect_err("radius 12 must be rejected");
        assert!(
            format!("{err}").contains("sharpening.radius"),
            "unexpected error: {err}"
        );
    }

    fn denoise_ai(model_hash: &str, strength: f32) -> lumina_sidecar::DenoiseAi {
        lumina_sidecar::DenoiseAi {
            version: lumina_sidecar::DENOISE_AI_VERSION,
            enabled: true,
            model: lumina_sidecar::DenoiseModelIdentity {
                name: "fixture-srgb".into(),
                version: "1".into(),
                model_hash: model_hash.into(),
                extras: Default::default(),
            },
            input_spec_digest: format!("sha256:{}", "22".repeat(32)),
            strength,
            preserve_detail: 0.5,
            artifact: None,
            extras: Default::default(),
        }
    }

    /// LRPAR-G14-DENOISE-IMPL-20: the GPU entry validates `denoise_ai` with the
    /// **same** `CoreError::Denoise` the CPU oracle emits, so a
    /// directly-constructed invalid stage is rejected on both backends instead
    /// of reaching a shader (or being silently ignored).
    #[test]
    fn denoise_ai_is_validated_like_cpu() {
        let valid = denoise_ai(&format!("sha256:{}", "11".repeat(32)), 0.5);
        validate_gpu_recipe(&EditRecipe {
            denoise_ai: Some(valid),
            ..Default::default()
        })
        .expect("a schema-valid denoise_ai passes the GPU entry validation");

        let invalid = denoise_ai("dummy", 0.5);
        let err = validate_gpu_recipe(&EditRecipe {
            denoise_ai: Some(invalid),
            ..Default::default()
        })
        .expect_err("a malformed denoise_ai model hash must be rejected");
        assert!(
            matches!(
                err,
                GpuError::Core(CoreError::Denoise { ref status, .. }) if status == "invalid"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn invalid_curve_is_rejected() {
        let recipe = EditRecipe {
            curves: Some(lumina_sidecar::Curves {
                version: 1,
                master: vec![
                    lumina_sidecar::CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    lumina_sidecar::CurvePoint {
                        input: 0.5,
                        output: 2.0,
                    },
                    lumina_sidecar::CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ],
                channels: Default::default(),
            }),
            ..Default::default()
        };
        assert!(validate_gpu_recipe(&recipe).is_err());
    }

    #[test]
    fn typed_spot_without_extras_is_rejected() {
        let recipe = EditRecipe {
            spot_removals: vec![lumina_sidecar::SpotRemoval {
                id: "spot-gpu-heuristic".into(),
                version: 1,
                mode: lumina_sidecar::SpotRemovalMode::Heuristic,
                artifact: None,
            }],
            ..Default::default()
        };
        assert!(validate_gpu_recipe(&recipe).is_err());
    }
    #[test]
    fn generative_mode_spot_is_rejected() {
        let recipe = EditRecipe {
            spot_removals: vec![lumina_sidecar::SpotRemoval {
                id: "spot-gpu-generative".into(),
                version: 1,
                mode: lumina_sidecar::SpotRemovalMode::Generative,
                artifact: None,
            }],
            ..Default::default()
        };
        assert!(validate_gpu_recipe(&recipe).is_err());
    }

    #[test]
    fn valid_legacy_spot_geometry_passes() {
        let mut recipe = EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{
                "id": "spot-1",
                "version": 1,
                "mode": "heuristic",
                "center_x": 0.5,
                "center_y": 0.5,
                "radius": 8.0,
                "feather": 0.5,
                "offset_dx": 0.25,
                "offset_dy": 0.0,
                "opacity": 1.0,
                "status": "valid"
            }]),
        );
        validate_gpu_recipe(&recipe).expect("valid legacy spot geometry");
    }

    // ---- GPU-RENDER-PARITY-1 geometry wave: the lens/perspective/geometry
    // ranges are validated at the GPU entry with the CPU oracle's own errors
    // (the stages are GPU-rendered now, so no invalid value may reach a shader).

    fn manual_lens(k1: f32) -> lumina_sidecar::LensCorrection {
        lumina_sidecar::LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(k1),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }
    }

    #[test]
    fn lens_ranges_are_validated() {
        validate_gpu_recipe(&EditRecipe {
            lens_correction: Some(manual_lens(0.5)),
            ..Default::default()
        })
        .expect("in-range distortion is valid");
        let err = validate_gpu_recipe(&EditRecipe {
            lens_correction: Some(manual_lens(1.5)),
            ..Default::default()
        })
        .expect_err("distortion_k1 > 1 must be rejected");
        assert!(format!("{err}").contains("distortion_k1"), "{err}");

        let mut ca = manual_lens(0.0);
        ca.ca_red = Some(0.2);
        let err = validate_gpu_recipe(&EditRecipe {
            lens_correction: Some(ca),
            ..Default::default()
        })
        .expect_err("ca_red > 0.05 must be rejected");
        assert!(format!("{err}").contains("ca_red"), "{err}");
    }

    #[test]
    fn perspective_ranges_are_validated() {
        let valid = lumina_sidecar::Perspective {
            version: 1,
            vertical: 0.2,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        };
        validate_gpu_recipe(&EditRecipe {
            perspective: Some(valid),
            ..Default::default()
        })
        .expect("in-range perspective is valid");

        let invalid = lumina_sidecar::Perspective {
            scale: 0.05,
            ..valid
        };
        let err = validate_gpu_recipe(&EditRecipe {
            perspective: Some(invalid),
            ..Default::default()
        })
        .expect_err("scale < 0.1 must be rejected");
        assert!(format!("{err}").contains("scale"), "{err}");
    }

    #[test]
    fn geometry_ranges_are_validated() {
        let valid = lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 45.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        };
        validate_gpu_recipe(&EditRecipe {
            geometry: Some(valid.clone()),
            ..Default::default()
        })
        .expect("in-range geometry is valid");

        let bad_rotation = lumina_sidecar::Geometry {
            rotation_degrees: 181.0,
            ..valid.clone()
        };
        let err = validate_gpu_recipe(&EditRecipe {
            geometry: Some(bad_rotation),
            ..Default::default()
        })
        .expect_err("rotation > 180 must be rejected");
        assert!(format!("{err}").contains("geometry"), "{err}");

        let bad_crop = lumina_sidecar::Geometry {
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.9,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            }),
            ..valid
        };
        let err = validate_gpu_recipe(&EditRecipe {
            geometry: Some(bad_crop),
            ..Default::default()
        })
        .expect_err("a free crop crossing the frame edge must be rejected");
        assert!(format!("{err}").contains("geometry.crop"), "{err}");
    }
}
