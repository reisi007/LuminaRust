//! The `color` command of the CLI: inspect and edit the color stages (tone
//! curve per channel, HSL mixer, Point Color, color grading,
//! vibrance/saturation) of one virtual copy.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet (SPLICE-CLI-MAIN-89,
//! slice 4). Only three symbols changed visibility (`color`,
//! `check_curve_channel`, `parse_curve_points` -> `pub(crate)`), because
//! callers outside this block exist; every other symbol moved unchanged.
//!
//! The contract is unchanged: unknown copies, channels, fields, ids and
//! malformed values abort loudly (exit 1) before anything is written; range
//! validation runs on save via `document.validate()`; the original image is
//! never modified. The `validate()` gate runs **before** `save_sidecar`, so a
//! range error is rejected loudly instead of being silently clipped.
//!
//! `ColorArgs` stays in the crate root (`main.rs`): its private fields are
//! visible in every descendant of the crate root, so this module reads them
//! without any visibility widening.

use crate::mask_local::{mask_copy_mut, resolve_mask_copy};
use crate::{emit, CliError, ColorArgs};
use log::info;
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, ColorGrading, ColorGradingRange, CurveChannels,
    CurvePoint, Curves, HslAdjustments, HslChannel, PointColor, PointColorEntry, SidecarDocument,
};

/// G-02 Color-Parität (LRPAR-G02-COLOR): inspect and edit the color stages
/// (tone curve per channel, HSL mixer, Point Color, color grading,
/// vibrance/saturation) of one virtual copy. Unknown copies, channels,
/// fields, ids and malformed values abort loudly (exit 1) before anything
/// is written; range validation runs on save via `document.validate()`.
/// The original image is never modified.
pub(crate) fn color(args: ColorArgs) -> Result<(), CliError> {
    let wants_mutation = !args.set_curve_param.is_empty()
        || !args.set_curve_points.is_empty()
        || args.clear_curves
        || args.clear_curve_channel.is_some()
        || !args.set_hsl.is_empty()
        || args.clear_hsl
        || args.add_point_color
        || !args.set_point_color.is_empty()
        || !args.remove_point_color.is_empty()
        || args.clear_point_color
        || !args.set_grading.is_empty()
        || args.set_grading_balance.is_some()
        || args.set_grading_blending.is_some()
        || args.clear_grading
        || args.set_vibrance.is_some()
        || args.set_saturation.is_some();
    let path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    if args.clear_curves && args.clear_curve_channel.is_some() {
        return Err(CliError::Message(
            "--clear-curves and --clear-curve-channel are mutually exclusive".into(),
        ));
    }
    if args.clear_hsl && !args.set_hsl.is_empty() {
        return Err(CliError::Message(
            "--clear-hsl and --set-hsl are mutually exclusive".into(),
        ));
    }
    if args.clear_grading
        && (!args.set_grading.is_empty()
            || args.set_grading_balance.is_some()
            || args.set_grading_blending.is_some())
    {
        return Err(CliError::Message(
            "--clear-grading and --set-grading* are mutually exclusive".into(),
        ));
    }
    if args.clear_point_color
        && (args.add_point_color
            || !args.set_point_color.is_empty()
            || !args.remove_point_color.is_empty())
    {
        return Err(CliError::Message(
            "--clear-point-color and --add/set/remove-point-color are mutually exclusive".into(),
        ));
    }
    if args.clear_curves {
        mask_copy_mut(&mut document, &copy_id)?.recipe.curves = None;
        info!("color: cleared curves on copy `{copy_id}`");
        actions.push("clear-curves".into());
    }
    if let Some(channel) = args.clear_curve_channel.as_deref() {
        check_curve_channel(channel)?;
        let curves = curves_block_mut(&mut document, &copy_id)?;
        match channel {
            "master" => {
                curves.master = vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ];
            }
            "red" => curves.channels.red = None,
            "green" => curves.channels.green = None,
            "blue" => curves.channels.blue = None,
            _ => unreachable!(),
        }
        info!("color: cleared curve channel {channel} on copy `{copy_id}`");
        actions.push(format!("clear-curve-channel:{channel}"));
    }
    for spec in &args.set_curve_param {
        let (channel, deltas) = parse_curve_param(spec)?;
        let points = curve_param_points(&deltas);
        set_curve_channel_points(curves_block_mut(&mut document, &copy_id)?, channel, points)?;
        info!("color: curve param {channel} on copy `{copy_id}`");
        actions.push(format!("curve-param:{channel}"));
    }
    for spec in &args.set_curve_points {
        let (channel, points) = parse_curve_points(spec)?;
        set_curve_channel_points(curves_block_mut(&mut document, &copy_id)?, channel, points)?;
        info!("color: curve points {channel} on copy `{copy_id}`");
        actions.push(format!("curve-points:{channel}"));
    }
    if args.clear_hsl {
        mask_copy_mut(&mut document, &copy_id)?.recipe.hsl = None;
        info!("color: cleared hsl on copy `{copy_id}`");
        actions.push("clear-hsl".into());
    }
    for spec in &args.set_hsl {
        let (channel, field, value) = parse_hsl_triple(spec)?;
        let slot = hsl_slot_mut(hsl_block_mut(&mut document, &copy_id)?, channel);
        match field {
            "hue" => slot.hue = value,
            "saturation" => slot.saturation = value,
            "luminance" => slot.luminance = value,
            _ => unreachable!(),
        }
        info!("color: hsl {channel}.{field}={value} on copy `{copy_id}`");
        actions.push(format!("hsl:{channel}.{field}"));
    }
    if args.clear_point_color {
        mask_copy_mut(&mut document, &copy_id)?.recipe.point_color = None;
        info!("color: cleared point_color on copy `{copy_id}`");
        actions.push("clear-point-color".into());
    }
    if args.add_point_color {
        let block = point_color_block_mut(&mut document, &copy_id)?;
        if block.entries.len() >= 8 {
            return Err(CliError::Message(
                "point_color entry limit (8) reached".into(),
            ));
        }
        let id = PointColorEntry::next_id(&block.entries);
        block.entries.push(PointColorEntry {
            id: id.clone(),
            hue_center: args.hue_center.unwrap_or(0.0),
            hue_range: args.hue_range.unwrap_or(30.0),
            hue_shift: args.hue_shift.unwrap_or(0.0),
            saturation_shift: args.sat_shift.unwrap_or(0.0),
            luminance_shift: args.lum_shift.unwrap_or(0.0),
        });
        info!("color: point_color add {id} on copy `{copy_id}`");
        actions.push(format!("point-color-add:{id}"));
    }
    for spec in &args.set_point_color {
        let (id, field, value) = parse_point_color_triple(spec)?;
        let block = point_color_block_mut(&mut document, &copy_id)?;
        let Some(entry) = block.entries.iter_mut().find(|e| e.id == id) else {
            return Err(CliError::Message(format!(
                "unknown point_color entry `{id}` on copy `{copy_id}`"
            )));
        };
        match field {
            "hue_center" => entry.hue_center = value,
            "hue_range" => entry.hue_range = value,
            "hue_shift" => entry.hue_shift = value,
            "saturation_shift" => entry.saturation_shift = value,
            "luminance_shift" => entry.luminance_shift = value,
            _ => unreachable!(),
        }
        info!("color: point_color {id}.{field}={value} on copy `{copy_id}`");
        actions.push(format!("point-color:{id}.{field}"));
    }
    for id in &args.remove_point_color {
        let block = point_color_block_mut(&mut document, &copy_id)?;
        let len = block.entries.len();
        block.entries.retain(|e| e.id != *id);
        if block.entries.len() == len {
            return Err(CliError::Message(format!(
                "unknown point_color entry `{id}` on copy `{copy_id}`"
            )));
        }
        if block.entries.is_empty() {
            mask_copy_mut(&mut document, &copy_id)?.recipe.point_color = None;
        }
        info!("color: point_color remove {id} on copy `{copy_id}`");
        actions.push(format!("point-color-remove:{id}"));
    }
    if args.clear_grading {
        mask_copy_mut(&mut document, &copy_id)?.recipe.color_grading = None;
        info!("color: cleared grading on copy `{copy_id}`");
        actions.push("clear-grading".into());
    }
    for spec in &args.set_grading {
        let (range, field, value) = parse_grading_triple(spec)?;
        let slot = grading_slot_mut(grading_block_mut(&mut document, &copy_id)?, range);
        match field {
            "hue_degrees" => slot.hue_degrees = value,
            "saturation" => slot.saturation = value,
            "luminance" => slot.luminance = value,
            _ => unreachable!(),
        }
        info!("color: grading {range}.{field}={value} on copy `{copy_id}`");
        actions.push(format!("grading:{range}.{field}"));
    }
    if let Some(balance) = args.set_grading_balance {
        grading_block_mut(&mut document, &copy_id)?.balance = balance;
        info!("color: grading balance {balance} on copy `{copy_id}`");
        actions.push(format!("grading-balance:{balance}"));
    }
    if let Some(blending) = args.set_grading_blending {
        grading_block_mut(&mut document, &copy_id)?.blending = blending;
        info!("color: grading blending {blending} on copy `{copy_id}`");
        actions.push(format!("grading-blending:{blending}"));
    }
    if let Some(vibrance) = args.set_vibrance {
        mask_copy_mut(&mut document, &copy_id)?
            .recipe
            .adjustments
            .insert("vibrance".into(), vibrance);
        info!("color: vibrance {vibrance} on copy `{copy_id}`");
        actions.push(format!("vibrance:{vibrance}"));
    }
    if let Some(saturation) = args.set_saturation {
        mask_copy_mut(&mut document, &copy_id)?
            .recipe
            .adjustments
            .insert("saturation".into(), saturation);
        info!("color: saturation {saturation} on copy `{copy_id}`");
        actions.push(format!("saturation:{saturation}"));
    }
    if wants_mutation {
        // Loud gate: ranges, point rules, focal order and ids are rejected
        // before anything is written (no silent clipping, no half-apply).
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        save_sidecar(&path, &document)?;
    }
    color_list(&args, &document, &copy_id, &actions)
}

/// Mutable access to one virtual copy's tone-curve stage, creating an
/// identity stage when none exists (loud on unknown ids).
fn curves_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Curves, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.curves.get_or_insert(Curves {
        version: 1,
        master: vec![
            CurvePoint {
                input: 0.0,
                output: 0.0,
            },
            CurvePoint {
                input: 1.0,
                output: 1.0,
            },
        ],
        channels: CurveChannels::default(),
    }))
}

/// Mutable access to one virtual copy's HSL mixer, creating a neutral stage
/// when none exists (loud on unknown ids).
fn hsl_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut HslAdjustments, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    let hsl = copy.recipe.hsl.get_or_insert(HslAdjustments {
        version: 1,
        ..Default::default()
    });
    hsl.version = 1;
    Ok(hsl)
}

/// Mutable access to one virtual copy's Point Color stage, creating an empty
/// stage when none exists (loud on unknown ids).
fn point_color_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut PointColor, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    let block = copy.recipe.point_color.get_or_insert(PointColor {
        version: 1,
        entries: Vec::new(),
    });
    block.version = 1;
    Ok(block)
}

/// Mutable access to one virtual copy's color-grading stage, creating a
/// neutral stage when none exists (loud on unknown ids).
fn grading_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut ColorGrading, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy
        .recipe
        .color_grading
        .get_or_insert_with(ColorGrading::neutral))
}

pub(crate) fn check_curve_channel(channel: &str) -> Result<(), CliError> {
    if matches!(channel, "master" | "red" | "green" | "blue") {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "invalid curve channel `{channel}`: expected master|red|green|blue"
        )))
    }
}

fn set_curve_channel_points(
    curves: &mut Curves,
    channel: &str,
    points: Vec<CurvePoint>,
) -> Result<(), CliError> {
    check_curve_channel(channel)?;
    curves.version = 1;
    match channel {
        "master" => curves.master = points,
        "red" => curves.channels.red = Some(points),
        "green" => curves.channels.green = Some(points),
        "blue" => curves.channels.blue = Some(points),
        _ => unreachable!(),
    }
    Ok(())
}

/// Builds the parametric 4-point list at base positions `0, 1/3, 2/3, 1`
/// (same mapping as the GUI); endpoint/range validity is enforced on save.
fn curve_param_points(deltas: &[f64; 4]) -> Vec<CurvePoint> {
    const BASE: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    BASE.iter()
        .zip(deltas.iter())
        .map(|(base, delta)| CurvePoint {
            input: *base as f32,
            output: ((base + delta).clamp(0.0, 1.0)) as f32,
        })
        .collect()
}

fn parse_f32_list(value: &str, expected: usize, what: &str) -> Result<Vec<f32>, CliError> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != expected {
        return Err(CliError::Message(format!(
            "invalid {what} `{value}`: expected {expected} comma-separated numbers"
        )));
    }
    parts
        .iter()
        .map(|part| {
            part.trim().parse::<f32>().map_err(|_| {
                CliError::Message(format!(
                    "invalid {what} `{value}`: `{part}` is not a number"
                ))
            })
        })
        .collect()
}

/// Parses `CHANNEL:S,D,L,H` (loud on malformed input; ranges on save).
fn parse_curve_param(spec: &str) -> Result<(&str, [f64; 4]), CliError> {
    let (channel, rest) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid curve param `{spec}`: expected `CHANNEL:S,D,L,H`"
        ))
    })?;
    check_curve_channel(channel)?;
    let values = parse_f32_list(rest, 4, "curve param")?;
    Ok((
        channel,
        [
            f64::from(values[0]),
            f64::from(values[1]),
            f64::from(values[2]),
            f64::from(values[3]),
        ],
    ))
}

/// Parses `CHANNEL:I,O;I,O;...` (loud on malformed input; point rules on
/// save).
pub(crate) fn parse_curve_points(spec: &str) -> Result<(&str, Vec<CurvePoint>), CliError> {
    let (channel, rest) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid curve points `{spec}`: expected `CHANNEL:I,O;I,O;...`"
        ))
    })?;
    check_curve_channel(channel)?;
    let mut points = Vec::new();
    for pair in rest.split(';') {
        let values = parse_f32_list(pair, 2, "curve point")?;
        points.push(CurvePoint {
            input: values[0],
            output: values[1],
        });
    }
    Ok((channel, points))
}

fn hsl_slot_mut<'a>(hsl: &'a mut HslAdjustments, channel: &str) -> &'a mut HslChannel {
    let slot = match channel {
        "red" => &mut hsl.red,
        "orange" => &mut hsl.orange,
        "yellow" => &mut hsl.yellow,
        "green" => &mut hsl.green,
        "cyan" => &mut hsl.cyan,
        "blue" => &mut hsl.blue,
        "violet" => &mut hsl.violet,
        _ => &mut hsl.magenta,
    };
    slot.get_or_insert_with(HslChannel::default)
}

/// Parses `CHANNEL:FIELD:VALUE` for the HSL mixer (loud on unknown
/// channels/fields; ranges on save).
fn parse_hsl_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
    let mut parts = spec.splitn(3, ':');
    let (Some(channel), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(CliError::Message(format!(
            "invalid hsl `{spec}`: expected `CHANNEL:FIELD:VALUE`"
        )));
    };
    if !matches!(
        channel,
        "red" | "orange" | "yellow" | "green" | "cyan" | "blue" | "violet" | "magenta"
    ) {
        return Err(CliError::Message(format!(
            "invalid hsl channel `{channel}`: expected red|orange|yellow|green|cyan|blue|violet|magenta"
        )));
    }
    if !matches!(field, "hue" | "saturation" | "luminance") {
        return Err(CliError::Message(format!(
            "invalid hsl field `{field}`: expected hue|saturation|luminance"
        )));
    }
    let value: f32 = value
        .trim()
        .parse()
        .map_err(|_| CliError::Message(format!("invalid hsl value in `{spec}`: not a number")))?;
    Ok((channel, field, value))
}

fn grading_slot_mut<'a>(grading: &'a mut ColorGrading, range: &str) -> &'a mut ColorGradingRange {
    match range {
        "shadows" => &mut grading.shadows,
        "midtones" => &mut grading.midtones,
        _ => &mut grading.highlights,
    }
}

/// Parses `RANGE:FIELD:VALUE` for color grading (loud on unknown
/// ranges/fields; ranges on save).
fn parse_grading_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
    let mut parts = spec.splitn(3, ':');
    let (Some(range), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(CliError::Message(format!(
            "invalid grading `{spec}`: expected `RANGE:FIELD:VALUE`"
        )));
    };
    if !matches!(range, "shadows" | "midtones" | "highlights") {
        return Err(CliError::Message(format!(
            "invalid grading range `{range}`: expected shadows|midtones|highlights"
        )));
    }
    if !matches!(field, "hue_degrees" | "saturation" | "luminance") {
        return Err(CliError::Message(format!(
            "invalid grading field `{field}`: expected hue_degrees|saturation|luminance"
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        CliError::Message(format!("invalid grading value in `{spec}`: not a number"))
    })?;
    Ok((range, field, value))
}

/// Parses `ID:FIELD:VALUE` for Point Color (loud on unknown fields or a
/// missing entry; ranges on save).
fn parse_point_color_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
    let mut parts = spec.splitn(3, ':');
    let (Some(id), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(CliError::Message(format!(
            "invalid point_color `{spec}`: expected `ID:FIELD:VALUE`"
        )));
    };
    if id.trim().is_empty() {
        return Err(CliError::Message(format!(
            "invalid point_color `{spec}`: empty entry id"
        )));
    }
    if !matches!(
        field,
        "hue_center" | "hue_range" | "hue_shift" | "saturation_shift" | "luminance_shift"
    ) {
        return Err(CliError::Message(format!(
            "invalid point_color field `{field}`: expected hue_center|hue_range|hue_shift|saturation_shift|luminance_shift"
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        CliError::Message(format!(
            "invalid point_color value in `{spec}`: not a number"
        ))
    })?;
    Ok((id, field, value))
}

fn color_list(
    args: &ColorArgs,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let recipe = &copy.recipe;
    if args.json {
        emit(
            true,
            serde_json::json!({
                "command": "color",
                "input": args.input,
                "copy": copy_id,
                "curves": recipe.curves,
                "hsl": recipe.hsl,
                "point_color": recipe.point_color,
                "color_grading": recipe.color_grading,
                "vibrance": recipe.adjustments.get("vibrance"),
                "saturation": recipe.adjustments.get("saturation"),
                "actions": actions,
            }),
            "color status listed",
        )
    } else {
        println!("copy: {} [{}]", copy.name, copy.id);
        match &recipe.curves {
            Some(curves) => {
                println!(
                    "  curves: master={}pts red={} green={} blue={}",
                    curves.master.len(),
                    curves.channels.red.as_ref().map_or(0, Vec::len),
                    curves.channels.green.as_ref().map_or(0, Vec::len),
                    curves.channels.blue.as_ref().map_or(0, Vec::len)
                );
            }
            None => println!("  curves: none"),
        }
        println!(
            "  hsl: {}",
            if recipe.hsl.is_some() { "set" } else { "none" }
        );
        match &recipe.point_color {
            Some(block) => {
                println!("  point_color: {} entries", block.entries.len());
                for entry in &block.entries {
                    println!(
                        "    {}: center={} range={} hue={} sat={} lum={}",
                        entry.id,
                        entry.hue_center,
                        entry.hue_range,
                        entry.hue_shift,
                        entry.saturation_shift,
                        entry.luminance_shift
                    );
                }
            }
            None => println!("  point_color: none"),
        }
        match &recipe.color_grading {
            Some(grading) => {
                println!(
                    "  grading: balance={} blending={}",
                    grading.balance, grading.blending
                );
                for (name, range) in [
                    ("shadows", grading.shadows),
                    ("midtones", grading.midtones),
                    ("highlights", grading.highlights),
                ] {
                    println!(
                        "    {name}: hue={} sat={} lum={}",
                        range.hue_degrees, range.saturation, range.luminance
                    );
                }
            }
            None => println!("  grading: none"),
        }
        println!(
            "  vibrance={:?} saturation={:?}",
            recipe.adjustments.get("vibrance"),
            recipe.adjustments.get("saturation")
        );
        if actions.is_empty() {
            emit(
                false,
                serde_json::json!({"command":"color","status":"ok"}),
                "color status listed",
            )
        } else {
            emit(
                false,
                serde_json::json!({"command":"color","status":"ok"}),
                &format!("color updated: {}", actions.join(", ")),
            )
        }
    }
}
