use super::*;
use std::collections::BTreeMap;

pub(crate) fn recipe(values: &[(&str, f64)]) -> EditRecipe {
    EditRecipe {
        adjustments: BTreeMap::from_iter(values.iter().map(|(key, value)| ((*key).into(), *value))),
        ..EditRecipe::default()
    }
}

// ---- REVIEW-CORE-DECODE-1: decode memory-budget guard ----

/// Minimal table-less IEEE CRC32 (poly `0xEDB88320`) over `bytes`.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Builds a syntactically complete-but-empty PNG (signature + IHDR + an
/// empty IDAT + IEND) claiming `width × height`. The decoder can resolve
/// the geometry from the header, but there are no pixel data to decode:
/// the budget check must reject the image before any allocation happens.
pub(crate) fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&13u32.to_be_bytes());
    ihdr.extend_from_slice(b"IHDR");
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    ihdr.extend_from_slice(&crc32(&ihdr[4..]).to_be_bytes());
    bytes.extend_from_slice(&ihdr);
    // Empty IDAT: `read_info` scans up to the image data without
    // decoding any of it.
    let mut idat = Vec::new();
    idat.extend_from_slice(&0u32.to_be_bytes());
    idat.extend_from_slice(b"IDAT");
    idat.extend_from_slice(&crc32(b"IDAT").to_be_bytes());
    bytes.extend_from_slice(&idat);
    let mut iend = Vec::new();
    iend.extend_from_slice(&0u32.to_be_bytes());
    iend.extend_from_slice(b"IEND");
    iend.extend_from_slice(&crc32(b"IEND").to_be_bytes());
    bytes.extend_from_slice(&iend);
    bytes
}

pub(crate) fn hsl_recipe(channel: usize, adjustment: lumina_sidecar::HslChannel) -> EditRecipe {
    let mut channels = [None; 8];
    channels[channel] = Some(adjustment);
    EditRecipe {
        hsl: Some(lumina_sidecar::HslAdjustments {
            version: 1,
            red: channels[0],
            orange: channels[1],
            yellow: channels[2],
            green: channels[3],
            cyan: channels[4],
            blue: channels[5],
            violet: channels[6],
            magenta: channels[7],
        }),
        ..Default::default()
    }
}

pub(crate) fn test_perspective() -> lumina_sidecar::Perspective {
    lumina_sidecar::Perspective {
        version: 1,
        vertical: 0.0,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    }
}

// ---- CROP-MAXRECT-1: default maximum-content crop ----

/// Opaque source frame with a deterministic, non-constant pattern.
pub(crate) fn maxrect_source(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let v = ((x * 13 + y * 29) % 256) as u8;
            pixels.extend_from_slice(&[v, 255 - v, v / 2, 255]);
        }
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

pub(crate) fn has_transparent_alpha(frame: &ImageFrame) -> bool {
    frame.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255)
}

pub(crate) fn wedge_perspective() -> lumina_sidecar::Perspective {
    lumina_sidecar::Perspective {
        version: 1,
        // A strong vertical keystone guarantees transparent wedges at the
        // top/bottom of the projected bounding box.
        vertical: 0.6,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    }
}

// ---- LRPAR-G14-REDEYE-15: red-eye correction stage ----

pub(crate) fn red_eye_recipe(desaturate: f32, darken: f32) -> EditRecipe {
    EditRecipe {
        red_eye: Some(lumina_sidecar::RedEyeCorrection {
            version: 1,
            regions: vec![lumina_sidecar::RedEyeRegion {
                id: "re-1".into(),
                x: 0.5,
                y: 0.5,
                radius: 0.4,
                desaturate,
                darken,
            }],
        }),
        ..Default::default()
    }
}

/// 8x8 frame: red pupil block in the center, grey surround, distinct
/// alphas (alpha must survive the stage untouched).
pub(crate) fn red_eye_frame() -> (ImageFrame, usize) {
    let mut pixels = Vec::new();
    for y in 0..8usize {
        for x in 0..8usize {
            let pupil = (3..=4).contains(&x) && (3..=4).contains(&y);
            if pupil {
                pixels.extend_from_slice(&[220, 30, 40, 200]);
            } else {
                pixels.extend_from_slice(&[120, 120, 120, 77]);
            }
        }
    }
    let center = (3 * 8 + 3) * 4;
    (ImageFrame::new(8, 8, pixels).unwrap(), center)
}

/// Independent reimplementation of the original pass-by-pass channel
/// adjustments (pre-F-074-A1). Used only by the byte-identity property test
/// to prove the fused LUT kernel (`apply_channel_lut_adjustments`) produces
/// byte-identical output. Mirrors the original per-stage loops exactly
/// (same `f64` formulas, same per-channel application order).
pub(crate) fn reference_channel_lut_adjustments(pixels: &mut [u8], params: &ChannelLutParams) {
    let ChannelLutParams {
        wb_gains,
        exposure_multiplier,
        contrast_factor,
        shadows,
        highlights,
        whites,
        blacks,
    } = params;
    if let Some(gains) = wb_gains {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            for (channel, gain) in pixel[..3].iter_mut().zip(*gains) {
                *channel = ((*channel as f64 * gain).round()).clamp(0.0, 255.0) as u8;
            }
        }
    }
    if let Some(multiplier) = exposure_multiplier {
        for channel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .flat_map(|pixel| &mut pixel[..3])
        {
            *channel = ((*channel as f64 * multiplier).round()).clamp(0.0, 255.0) as u8;
        }
    }
    if let Some(factor) = contrast_factor {
        for channel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .flat_map(|pixel| &mut pixel[..3])
        {
            *channel =
                (((*channel as f64 - 128.0) * factor + 128.0).round()).clamp(0.0, 255.0) as u8;
        }
    }
    if let Some(shadows) = shadows {
        for channel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .flat_map(|pixel| &mut pixel[..3])
        {
            let x = *channel as f64 / 255.0;
            let weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
            *channel = ((x + shadows * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    if let Some(highlights) = highlights {
        for channel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .flat_map(|pixel| &mut pixel[..3])
        {
            let x = *channel as f64 / 255.0;
            let weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
            *channel = ((x + highlights * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    if let Some(whites) = whites {
        for channel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .flat_map(|pixel| &mut pixel[..3])
        {
            let x = *channel as f64 / 255.0;
            let weight = ((x - 0.5) / 0.5).max(0.0);
            *channel = ((x + whites * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    if let Some(blacks) = blacks {
        for channel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .flat_map(|pixel| &mut pixel[..3])
        {
            let x = *channel as f64 / 255.0;
            let weight = ((0.5 - x) / 0.5).max(0.0);
            *channel = ((x - blacks * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}
