//! `lumina_analyze` — structured, vision-free image analysis of the current
//! edit state (expanded MCP scope). Returns histogram, per-channel statistics,
//! dominant colors and an exposure estimate so an agent without vision can
//! judge the result.

use crate::error::McpError;
use crate::util::{get_str, render_copy};
use crate::Server;
use lumina_core::{analyze_tone, ImageFrame, LuminanceHistogram};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const NAME: &str = "lumina_analyze";
pub const DESCRIPTION: &str = "Analyze the currently rendered edit state and return structured \
JSON: a luminance/per-channel histogram, per-channel mean/stddev/min/max, dominant colors and an \
exposure estimate. Useful for vision-less agents to judge brightness, contrast and color balance.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy name or id (default: the standard copy)."
            }
        },
        "required": ["image_id"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let image_id = get_str(args, "image_id")?;
    let state = server.session.require_id(image_id)?;

    let requested = args.get("virtual_copy").and_then(|value| value.as_str());
    let copy = state.find_copy(requested)?;
    let white_balance = state
        .raw_metadata
        .as_ref()
        .map(|meta| meta.camera_white_balance);
    let rendered = render_copy(state, copy, white_balance)?;

    let tone = analyze_tone(&rendered);
    let histogram = LuminanceHistogram::new(&rendered);
    let (red, green, blue) = channel_histograms(&rendered);
    let channel = channel_stats(&rendered);
    let dominant = dominant_colors(&rendered, 5);

    // Exposure estimate: EV to reach a mid-gray (0.5) target from the median
    // luminance, clamped to the pipeline's -10..=10 EV range.
    let exposure_ev = if tone.median > 1e-6 {
        (0.5 / tone.median).log2().clamp(-10.0, 10.0)
    } else {
        10.0
    };

    Ok(json!({
        "width": rendered.width,
        "height": rendered.height,
        "exposure_estimate": {
            "ev": exposure_ev,
            "median_luminance": tone.median,
            "mean_luminance": tone.mean,
        },
        "luminance": {
            "mean": tone.mean,
            "median": tone.median,
            "p01": tone.p01,
            "p99": tone.p99,
            "stddev": stddev_luminance(&rendered, tone.mean),
        },
        "per_channel": channel,
        "histogram": {
            "luminance": histogram.bins,
            "channels": { "r": red, "g": green, "b": blue },
        },
        "dominant_colors": dominant,
    }))
}

/// Per-channel 256-bin histograms.
fn channel_histograms(frame: &ImageFrame) -> (Vec<u64>, Vec<u64>, Vec<u64>) {
    let mut red = vec![0u64; 256];
    let mut green = vec![0u64; 256];
    let mut blue = vec![0u64; 256];
    for pixel in frame.pixels.as_chunks::<4>().0 {
        red[pixel[0] as usize] += 1;
        green[pixel[1] as usize] += 1;
        blue[pixel[2] as usize] += 1;
    }
    (red, green, blue)
}

/// Per-channel mean, stddev, min and max.
fn channel_stats(frame: &ImageFrame) -> Value {
    let total = (frame.width as usize * frame.height as usize).max(1) as f64;
    let mut sums = [0f64; 3];
    let mut sum_squares = [0f64; 3];
    let mut minimum = [255u8; 3];
    let mut maximum = [0u8; 3];
    for pixel in frame.pixels.as_chunks::<4>().0 {
        for channel in 0..3 {
            let value = pixel[channel] as f64;
            sums[channel] += value;
            sum_squares[channel] += value * value;
            minimum[channel] = minimum[channel].min(pixel[channel]);
            maximum[channel] = maximum[channel].max(pixel[channel]);
        }
    }
    let mut means = [0f64; 3];
    let mut stddevs = [0f64; 3];
    for channel in 0..3 {
        let mean = sums[channel] / total;
        let variance = (sum_squares[channel] / total) - mean * mean;
        means[channel] = mean;
        stddevs[channel] = variance.max(0.0).sqrt();
    }
    json!({
        "r": {
            "mean": means[0], "stddev": stddevs[0],
            "min": minimum[0], "max": maximum[0],
        },
        "g": {
            "mean": means[1], "stddev": stddevs[1],
            "min": minimum[1], "max": maximum[1],
        },
        "b": {
            "mean": means[2], "stddev": stddevs[2],
            "min": minimum[2], "max": maximum[2],
        },
    })
}

/// Standard deviation of Rec.709 luminance over the frame.
fn stddev_luminance(frame: &ImageFrame, mean: f64) -> f64 {
    let total = (frame.width as usize * frame.height as usize).max(1) as f64;
    let mut sum_squares = 0.0;
    for pixel in frame.pixels.as_chunks::<4>().0 {
        let luminance =
            (0.2126 * pixel[0] as f64 + 0.7152 * pixel[1] as f64 + 0.0722 * pixel[2] as f64)
                / 255.0;
        sum_squares += (luminance - mean) * (luminance - mean);
    }
    (sum_squares / total).max(0.0).sqrt()
}

/// Most frequent quantized colors, sorted by frequency (then key for stability).
fn dominant_colors(frame: &ImageFrame, count: usize) -> Vec<Value> {
    let mut frequencies: HashMap<u32, u64> = HashMap::new();
    for pixel in frame.pixels.as_chunks::<4>().0 {
        // Quantize each channel to its top 4 bits for a stable, coarse palette.
        let key = ((pixel[0] as u32 & 0xF0) << 16)
            | ((pixel[1] as u32 & 0xF0) << 8)
            | (pixel[2] as u32 & 0xF0);
        *frequencies.entry(key).or_insert(0) += 1;
    }
    let total = frame.pixels.len() as u64 / 4;
    let mut entries: Vec<(u32, u64)> = frequencies.into_iter().collect();
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    entries
        .into_iter()
        .take(count)
        .map(|(key, frequency)| {
            let red = ((key >> 16) & 0xFF) as u8;
            let green = ((key >> 8) & 0xFF) as u8;
            let blue = (key & 0xFF) as u8;
            json!({
                "rgb": [red, green, blue],
                "frequency": frequency as f64 / total as f64,
            })
        })
        .collect()
}
