//! Pure parsing helpers for the `color` command's textual values.

use super::*;

pub(crate) fn check_curve_channel(channel: &str) -> Result<(), CliError> {
    if matches!(channel, "master" | "red" | "green" | "blue") {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "invalid curve channel `{channel}`: expected master|red|green|blue"
        )))
    }
}

pub(super) fn curve_param_points(deltas: &[f64; 4]) -> Vec<CurvePoint> {
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

pub(super) fn parse_curve_param(spec: &str) -> Result<(&str, [f64; 4]), CliError> {
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

pub(super) fn parse_hsl_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
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

pub(super) fn parse_grading_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
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

pub(super) fn parse_point_color_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
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
