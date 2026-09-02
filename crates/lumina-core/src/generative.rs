#![allow(clippy::identity_op)]
#![allow(clippy::field_reassign_with_default)]
//! Generative canvas + keep_generative_content logic (GEN-FILL-03).
//! Plus GEN-FILL-01 heuristic auto-fill for transparent pixels after lens correction.

use lumina_sidecar::{Crop, GenerativeCanvas, GenerativeEdit};

use crate::{CoreError, ImageFrame};

pub fn has_transparent_pixels(frame: &ImageFrame) -> bool {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0))
}

/// Heuristic fill: transparent pixels (`alpha < 255`) are replaced by the
/// nearest opaque pixel's RGB (Manhattan BFS). `seed` shuffles the BFS tie-break
/// deterministically. Returns `true` iff any pixel was filled. Alpha of filled
/// pixels becomes `255`. Deterministic for identical frame+seed.
pub fn fill_transparent_heuristic(frame: &mut ImageFrame, seed: u64) -> bool {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 || frame.pixels.len() != w * h * 4 {
        return false;
    }
    let mut opaque: Vec<(usize, usize)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            if frame.pixels[idx + 3] == 255
                && !(frame.pixels[idx] == 0
                    && frame.pixels[idx + 1] == 0
                    && frame.pixels[idx + 2] == 0)
            {
                opaque.push((x, y));
            }
        }
    }
    if opaque.is_empty() {
        return false;
    }
    if !frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0))
    {
        return false;
    }
    opaque.sort_by_key(|(x, y)| {
        let mut k = (*x as u64).wrapping_mul(73856093) ^ (*y as u64).wrapping_mul(19349663) ^ seed;
        k = k.wrapping_add(0x9e3779b97f4a7c15);
        k = (k ^ (k >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        k = (k ^ (k >> 27)).wrapping_mul(0x94d049bb133111eb);
        k ^ (k >> 31)
    });
    let mut visited = vec![false; w * h];
    let mut queue: std::collections::VecDeque<(usize, usize)> = std::collections::VecDeque::new();
    for (x, y) in opaque {
        let idx = y * w + x;
        visited[idx] = true;
        queue.push_back((x, y));
    }
    let mut filled = false;
    while let Some((x, y)) = queue.pop_front() {
        let src_idx = (y * w + x) * 4;
        let src_rgb = [
            frame.pixels[src_idx],
            frame.pixels[src_idx + 1],
            frame.pixels[src_idx + 2],
        ];
        let neighbors = [
            (x.wrapping_sub(1), y, x > 0),
            (x + 1, y, x + 1 < w),
            (x, y.wrapping_sub(1), y > 0),
            (x, y + 1, y + 1 < h),
        ];
        for (nx, ny, valid) in neighbors {
            if !valid {
                continue;
            }
            let nidx = ny * w + nx;
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            let dst = nidx * 4;
            if frame.pixels[dst + 3] < 255
                || (frame.pixels[dst] == 0
                    && frame.pixels[dst + 1] == 0
                    && frame.pixels[dst + 2] == 0)
            {
                frame.pixels[dst] = src_rgb[0];
                frame.pixels[dst + 1] = src_rgb[1];
                frame.pixels[dst + 2] = src_rgb[2];
                frame.pixels[dst + 3] = 255;
                filled = true;
            }
            queue.push_back((nx, ny));
        }
    }
    filled
}

pub fn effective_keep(recipe: &lumina_sidecar::EditRecipe) -> bool {
    recipe
        .generative_edit
        .as_ref()
        .map(|g| g.effective_keep())
        .unwrap_or(true)
}

pub fn generative_edit(recipe: &lumina_sidecar::EditRecipe) -> Option<&GenerativeEdit> {
    recipe.generative_edit.as_ref()
}

pub fn generative_canvas(recipe: &lumina_sidecar::EditRecipe) -> Option<&GenerativeCanvas> {
    recipe
        .generative_edit
        .as_ref()
        .and_then(|g| g.canvas.as_ref())
}

pub fn materialize_canvas_for_crop(
    canvas: &GenerativeCanvas,
    crop: Option<&Crop>,
) -> Result<GenerativeCanvas, CoreError> {
    if canvas.output_width == 0 || canvas.output_height == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_canvas.output".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: f64::from(u32::MAX),
        });
    }
    let (cx, cy, cw, ch) = crop_rect_on_canvas(canvas.output_width, canvas.output_height, crop)?;
    let new_offset_x = canvas.source_offset_x - cx as i32;
    let new_offset_y = canvas.source_offset_y - cy as i32;
    let out = GenerativeCanvas {
        output_width: cw,
        output_height: ch,
        source_offset_x: new_offset_x,
        source_offset_y: new_offset_y,
        extras: Default::default(),
    };
    out.validate().map_err(|_| CoreError::InvalidAdjustment {
        name: "generative_canvas.materialized".into(),
        value: 0.0,
        minimum: 1.0,
        maximum: f64::from(u32::MAX),
    })?;
    Ok(out)
}

pub fn materialize_canvas_for_crop_with_source(
    canvas: &GenerativeCanvas,
    crop: Option<&Crop>,
    source_width: u32,
    source_height: u32,
) -> Result<GenerativeCanvas, CoreError> {
    let out = materialize_canvas_for_crop(canvas, crop)?;
    let right = out.source_offset_x as i64 + source_width as i64;
    let bottom = out.source_offset_y as i64 + source_height as i64;
    if right > out.output_width as i64
        || bottom > out.output_height as i64
        || right < 0
        || bottom < 0
    {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_canvas.source_bounds".into(),
            value: right as f64,
            minimum: 0.0,
            maximum: out.output_width as f64,
        });
    }
    Ok(out)
}

pub fn resolve_canvas_for_recipe(
    recipe: &lumina_sidecar::EditRecipe,
) -> Result<Option<GenerativeCanvas>, CoreError> {
    let Some(ge) = &recipe.generative_edit else {
        return Ok(None);
    };
    let Some(canvas) = &ge.canvas else {
        return Ok(None);
    };
    if ge.effective_keep() {
        Ok(Some(canvas.clone()))
    } else {
        let crop = recipe.geometry.as_ref().and_then(|g| g.crop.as_ref());
        Ok(Some(materialize_canvas_for_crop(canvas, crop)?))
    }
}

/// GEN-FILL-02 stub: expand canvas heuristically (no model). Validates canvas bounds.
pub fn apply_generative_expand(
    frame: &ImageFrame,
    recipe: &lumina_sidecar::EditRecipe,
) -> Result<ImageFrame, CoreError> {
    let Some(ge) = recipe.generative_edit.as_ref() else {
        return Ok(frame.clone());
    };
    if !ge.effective_expand() {
        return Ok(frame.clone());
    }
    let Some(canvas) = ge.canvas.as_ref() else {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_expand.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        });
    };
    canvas
        .validate()
        .map_err(|_| CoreError::InvalidAdjustment {
            name: "generative_expand.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        })?;
    if canvas.output_width <= frame.width && canvas.output_height <= frame.height {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_expand.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    // Bounds: source must fit inside canvas
    if canvas.source_offset_x < 0
        || canvas.source_offset_y < 0
        || (canvas.source_offset_x as u32 + frame.width) > canvas.output_width
        || (canvas.source_offset_y as u32 + frame.height) > canvas.output_height
    {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_expand.bounds".into(),
            value: 0.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let mut out = ImageFrame::new(
        canvas.output_width,
        canvas.output_height,
        vec![0; canvas.output_width as usize * canvas.output_height as usize * 4],
    )
    .unwrap();
    // Copy source at offset
    for y in 0..frame.height {
        for x in 0..frame.width {
            let src_idx = (y * frame.width + x) as usize * 4;
            let dst_x = (canvas.source_offset_x + x as i32) as u32;
            let dst_y = (canvas.source_offset_y + y as i32) as u32;
            let dst_idx = (dst_y * canvas.output_width + dst_x) as usize * 4;
            out.pixels[dst_idx..dst_idx + 4].copy_from_slice(&frame.pixels[src_idx..src_idx + 4]);
        }
    }
    // Fill remaining (expanded) area with heuristic (nearest neighbor via fill_transparent)
    // Mark expanded area as transparent then fill
    for y in 0..out.height {
        for x in 0..out.width {
            let dst_idx = (y * out.width + x) as usize * 4;
            let inside_source = x >= canvas.source_offset_x as u32
                && x < canvas.source_offset_x as u32 + frame.width
                && y >= canvas.source_offset_y as u32
                && y < canvas.source_offset_y as u32 + frame.height;
            if !inside_source {
                out.pixels[dst_idx + 3] = 0; // transparent
            }
        }
    }
    let seed = ge.seed.unwrap_or(0);
    fill_transparent_heuristic(&mut out, seed);
    Ok(out)
}

fn crop_rect_on_canvas(
    width: u32,
    height: u32,
    crop: Option<&Crop>,
) -> Result<(u32, u32, u32, u32), CoreError> {
    let (x, y, w, h) = match crop {
        None => (0.0, 0.0, 1.0, 1.0),
        Some(Crop::Free {
            x,
            y,
            width,
            height,
        }) => (*x as f64, *y as f64, *width as f64, *height as f64),
        Some(Crop::Aspect { preset }) => {
            let ratio = match preset {
                lumina_sidecar::AspectPreset::Original => width as f64 / height as f64,
                lumina_sidecar::AspectPreset::OneToOne => 1.0,
                lumina_sidecar::AspectPreset::FourToFive => 4.0 / 5.0,
                lumina_sidecar::AspectPreset::FiveToFour => 5.0 / 4.0,
                lumina_sidecar::AspectPreset::ThreeToTwo => 3.0 / 2.0,
                lumina_sidecar::AspectPreset::TwoToThree => 2.0 / 3.0,
                lumina_sidecar::AspectPreset::FourToThree => 4.0 / 3.0,
                lumina_sidecar::AspectPreset::ThreeToFour => 3.0 / 4.0,
                lumina_sidecar::AspectPreset::SixteenToNine => 16.0 / 9.0,
                lumina_sidecar::AspectPreset::NineToSixteen => 9.0 / 16.0,
            };
            let source_ratio = width as f64 / height as f64;
            if source_ratio > ratio {
                (
                    (1.0 - ratio / source_ratio) / 2.0,
                    0.0,
                    ratio / source_ratio,
                    1.0,
                )
            } else {
                (
                    0.0,
                    (1.0 - source_ratio / ratio) / 2.0,
                    1.0,
                    source_ratio / ratio,
                )
            }
        }
    };
    if ![x, y, w, h].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || x < 0.0
        || y < 0.0
        || x > 1.0
        || y > 1.0
        || x + w > 1.0 + 1e-6
        || y + h > 1.0 + 1e-6
    {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    if width == 0 || height == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty frame)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let px = (((x * width as f64).round() as i64).clamp(0, i64::from(width) - 1)) as u32;
    let py = (((y * height as f64).round() as i64).clamp(0, i64::from(height) - 1)) as u32;
    let pw = ((w * width as f64).round() as u32).max(1).min(width - px);
    let ph = ((h * height as f64).round() as u32).max(1).min(height - py);
    if pw == 0 || ph == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty crop rectangle)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    Ok((px, py, pw, ph))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{Crop, GenerativeCanvas};

    fn canvas(w: u32, h: u32, ox: i32, oy: i32) -> GenerativeCanvas {
        GenerativeCanvas {
            output_width: w,
            output_height: h,
            source_offset_x: ox,
            source_offset_y: oy,
            extras: Default::default(),
        }
    }

    #[test]
    fn effective_keep_defaults_to_true() {
        let mut recipe = lumina_sidecar::EditRecipe::default();
        assert!(effective_keep(&recipe));
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        assert!(effective_keep(&recipe));
        recipe
            .generative_edit
            .as_mut()
            .unwrap()
            .keep_generative_content = Some(false);
        assert!(!effective_keep(&recipe));
        recipe
            .generative_edit
            .as_mut()
            .unwrap()
            .keep_generative_content = Some(true);
        assert!(effective_keep(&recipe));
    }

    #[test]
    fn keep_true_leaves_canvas_unchanged() {
        let c = canvas(6000, 4000, 500, 0);
        let crop = Crop::Free {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        };
        let mut recipe = lumina_sidecar::EditRecipe::default();
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(c.clone()),
            keep_generative_content: Some(true),
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        recipe.geometry = Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(crop),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        let resolved = resolve_canvas_for_recipe(&recipe).unwrap().unwrap();
        assert_eq!(resolved, c);
    }

    #[test]
    fn keep_false_materializes_canvas_translation() {
        let c = canvas(6000, 4000, 500, 0);
        let crop = Crop::Free {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 4800);
        assert_eq!(out.output_height, 2400);
        assert_eq!(out.source_offset_x, 500 - 600);
        assert_eq!(out.source_offset_y, 0 - 800);
    }

    #[test]
    fn keep_false_full_crop_is_identity_translation() {
        let c = canvas(800, 600, 10, 20);
        let out = materialize_canvas_for_crop(&c, None).unwrap();
        assert_eq!(out.output_width, 800);
        assert_eq!(out.output_height, 600);
        assert_eq!(out.source_offset_x, 10);
        assert_eq!(out.source_offset_y, 20);
    }

    #[test]
    fn keep_false_half_crop_translates_correctly() {
        let c = canvas(100, 100, 0, 0);
        let crop = Crop::Free {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 50);
        assert_eq!(out.output_height, 100);
        assert_eq!(out.source_offset_x, -50);
        assert_eq!(out.source_offset_y, 0);
    }

    #[test]
    fn materialize_with_aspect_preset() {
        let c = canvas(400, 200, 0, 0);
        let crop = Crop::Aspect {
            preset: lumina_sidecar::AspectPreset::OneToOne,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 200);
        assert_eq!(out.output_height, 200);
        assert_eq!(out.source_offset_x, -100);
    }

    #[test]
    fn negative_offset_allowed_until_output_shrinks() {
        let c = canvas(200, 200, -50, -50);
        let crop = Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 100);
        assert_eq!(out.output_height, 100);
        assert_eq!(out.source_offset_x, -50);
        assert_eq!(out.source_offset_y, -50);
    }

    #[test]
    fn bounds_check_with_source() {
        let c = canvas(6000, 4000, 500, 0);
        let crop = Crop::Free {
            x: 0.9,
            y: 0.9,
            width: 0.1,
            height: 0.1,
        };
        let err = materialize_canvas_for_crop_with_source(&c, Some(&crop), 4000, 3000).unwrap_err();
        assert!(matches!(err, CoreError::InvalidAdjustment { .. }));
    }

    #[test]
    fn recipe_hash_changes_with_keep_flag() {
        let mut a = lumina_sidecar::EditRecipe::default();
        a.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(canvas(100, 100, 0, 0)),
            keep_generative_content: Some(true),
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        let mut b = a.clone();
        b.generative_edit.as_mut().unwrap().keep_generative_content = Some(false);
        let ha = blake3::hash(&serde_json::to_vec(&a).unwrap())
            .to_hex()
            .to_string();
        let hb = blake3::hash(&serde_json::to_vec(&b).unwrap())
            .to_hex()
            .to_string();
        assert_ne!(ha, hb);
        let mut c = a.clone();
        c.generative_edit.as_mut().unwrap().keep_generative_content = None;
        assert!(effective_keep(&c));
    }

    #[test]
    fn fill_transparent_no_opaque_no_fill() {
        let mut frame =
            crate::ImageFrame::new(2, 2, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
        let filled = crate::generative::fill_transparent_heuristic(&mut frame, 0);
        assert!(!filled);
    }

    #[test]
    fn fill_transparent_no_transparent_no_fill() {
        let mut frame = crate::ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(!crate::generative::fill_transparent_heuristic(
            &mut frame, 42
        ));
        assert_eq!(frame.pixels, vec![10, 20, 30, 255]);
    }

    #[test]
    fn fill_transparent_fills_border() {
        let mut pixels = vec![0u8; 3 * 3 * 4];
        for i in 0..9 {
            pixels[i * 4 + 3] = 0;
        }
        pixels[4 * 4] = 100;
        pixels[4 * 4 + 1] = 150;
        pixels[4 * 4 + 2] = 200;
        pixels[4 * 4 + 3] = 255;
        let mut frame = crate::ImageFrame::new(3, 3, pixels).unwrap();
        assert!(crate::generative::has_transparent_pixels(&frame));
        let filled = crate::generative::fill_transparent_heuristic(&mut frame, 0);
        assert!(filled);
        assert!(!crate::generative::has_transparent_pixels(&frame));
        for y in 0..3 {
            for x in 0..3 {
                let idx = (y * 3 + x) * 4;
                assert_eq!(&frame.pixels[idx..idx + 3], &[100, 150, 200]);
                assert_eq!(frame.pixels[idx + 3], 255);
            }
        }
    }

    #[test]
    fn fill_transparent_deterministic_seed() {
        let make = || {
            crate::ImageFrame::new(
                2,
                2,
                vec![10, 10, 10, 255, 0, 0, 0, 0, 0, 0, 0, 0, 20, 20, 20, 255],
            )
            .unwrap()
        };
        let mut a = make();
        let mut b = make();
        crate::generative::fill_transparent_heuristic(&mut a, 123);
        crate::generative::fill_transparent_heuristic(&mut b, 123);
        assert_eq!(a.pixels, b.pixels);
        let mut c = make();
        crate::generative::fill_transparent_heuristic(&mut c, 999);
        assert!(!crate::generative::has_transparent_pixels(&c));
    }

    #[test]
    fn recipe_hash_changes_with_auto_fill_flag() {
        let mut a = lumina_sidecar::EditRecipe::default();
        a.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        let mut b = a.clone();
        b.generative_edit.as_mut().unwrap().auto_fill_transparent = Some(false);
        let ha = blake3::hash(&serde_json::to_vec(&a).unwrap())
            .to_hex()
            .to_string();
        let hb = blake3::hash(&serde_json::to_vec(&b).unwrap())
            .to_hex()
            .to_string();
        assert_ne!(ha, hb);
    }
}
