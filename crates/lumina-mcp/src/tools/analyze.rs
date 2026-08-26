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
    // R2-MCP-08: the four per-channel/colour analyses below used to be four
    // separate full-frame iterations; they are fused into a single pass in
    // `frame_analysis` so `lumina_analyze` stays cheap in the agent feedback
    // loop. `analyze_tone` and `LuminanceHistogram` remain separate core passes
    // (their output is exact/independent and lives in `lumina-core`).
    let analysis = frame_analysis(&rendered, tone.mean);
    let (red, green, blue) = analysis.channel_histograms;
    let channel = analysis.channel_stats;
    let dominant = analysis.dominant_colors;

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
            "stddev": analysis.luminance_stddev,
        },
        "per_channel": channel,
        "histogram": {
            "luminance": histogram.bins,
            "channels": { "r": red, "g": green, "b": blue },
        },
        "dominant_colors": dominant,
    }))
}

/// Aggregated single-pass statistics over an RGBA8 frame. Produced by
/// [`frame_analysis`], which replaces the four separate full-frame iterations
/// that previously computed these values (R2-MCP-08).
struct FrameAnalysis {
    /// Per-channel 256-bin histograms `(red, green, blue)`.
    channel_histograms: (Vec<u64>, Vec<u64>, Vec<u64>),
    /// Per-channel mean/stddev/min/max as a JSON object (mirrors the previous
    /// `channel_stats` shape).
    channel_stats: Value,
    /// Standard deviation of Rec.709 luminance over the frame.
    luminance_stddev: f64,
    /// Most frequent quantized colours, sorted by frequency (then key).
    dominant_colors: Vec<Value>,
}

/// Single-pass frame analysis (R2-MCP-08): computes the per-channel 256-bin
/// histograms, per-channel mean/stddev/min/max, the Rec.709 luminance standard
/// deviation and the dominant quantized colours in **one** iteration over the
/// pixel buffer instead of four. The math is byte-identical to the previous
/// `channel_histograms`/`channel_stats`/`stddev_luminance`/`dominant_colors`
/// helpers — only the number of passes changes, so the emitted JSON is
/// unchanged.
fn frame_analysis(frame: &ImageFrame, mean_luminance: f64) -> FrameAnalysis {
    let total = (frame.width as usize * frame.height as usize).max(1) as f64;
    let mut red = vec![0u64; 256];
    let mut green = vec![0u64; 256];
    let mut blue = vec![0u64; 256];
    let mut sums = [0f64; 3];
    let mut sum_squares = [0f64; 3];
    let mut minimum = [255u8; 3];
    let mut maximum = [0u8; 3];
    let mut sum_luminance_squares = 0.0;
    let mut frequencies: HashMap<u32, u64> = HashMap::new();
    for pixel in frame.pixels.as_chunks::<4>().0 {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        red[r as usize] += 1;
        green[g as usize] += 1;
        blue[b as usize] += 1;
        let channels = [r, g, b];
        for channel in 0..3 {
            let value = channels[channel] as f64;
            sums[channel] += value;
            sum_squares[channel] += value * value;
            minimum[channel] = minimum[channel].min(channels[channel]);
            maximum[channel] = maximum[channel].max(channels[channel]);
        }
        let luminance = (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0;
        sum_luminance_squares += (luminance - mean_luminance) * (luminance - mean_luminance);
        // Quantize each channel to its top 4 bits for a stable, coarse palette.
        let key = ((r as u32 & 0xF0) << 16) | ((g as u32 & 0xF0) << 8) | (b as u32 & 0xF0);
        *frequencies.entry(key).or_insert(0) += 1;
    }

    let mut means = [0f64; 3];
    let mut stddevs = [0f64; 3];
    for channel in 0..3 {
        let mean = sums[channel] / total;
        let variance = (sum_squares[channel] / total) - mean * mean;
        means[channel] = mean;
        stddevs[channel] = variance.max(0.0).sqrt();
    }
    let channel_stats = json!({
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
    });
    let luminance_stddev = (sum_luminance_squares / total).max(0.0).sqrt();

    let total_pixels = frame.pixels.len() as u64 / 4;
    let mut entries: Vec<(u32, u64)> = frequencies.into_iter().collect();
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let dominant_colors: Vec<Value> = entries
        .into_iter()
        .take(5)
        .map(|(key, frequency)| {
            let red = ((key >> 16) & 0xFF) as u8;
            let green = ((key >> 8) & 0xFF) as u8;
            let blue = (key & 0xFF) as u8;
            json!({
                "rgb": [red, green, blue],
                "frequency": frequency as f64 / total_pixels as f64,
            })
        })
        .collect();

    FrameAnalysis {
        channel_histograms: (red, green, blue),
        channel_stats,
        luminance_stddev,
        dominant_colors,
    }
}
