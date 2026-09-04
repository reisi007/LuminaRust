use crate::{CoreError, ImageFrame};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotHeuristic {
    pub id: String,
    pub version: u32,
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
    #[serde(default)]
    pub feather: f32,
    pub offset_dx: f32,
    pub offset_dy: f32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_status")]
    pub status: String,
}
fn default_opacity() -> f32 {
    1.0
}
fn default_status() -> String {
    "valid".into()
}
impl SpotHeuristic {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "spot_heal.version".into(),
                value: self.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        for (n, v) in [("center_x", self.center_x), ("center_y", self.center_y)] {
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("spot_heal.{n}"),
                    value: v as f64,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
        if !self.radius.is_finite() || !(0.0 < self.radius && self.radius <= 512.0) {
            return Err(CoreError::InvalidAdjustment {
                name: "spot_heal.radius".into(),
                value: self.radius as f64,
                minimum: 1.0,
                maximum: 512.0,
            });
        }
        if !self.feather.is_finite() || !(0.0..=1.0).contains(&self.feather) {
            return Err(CoreError::InvalidAdjustment {
                name: "spot_heal.feather".into(),
                value: self.feather as f64,
                minimum: 0.0,
                maximum: 1.0,
            });
        }
        for (n, v) in [("offset_dx", self.offset_dx), ("offset_dy", self.offset_dy)] {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("spot_heal.{n}"),
                    value: v as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(CoreError::InvalidAdjustment {
                name: "spot_heal.opacity".into(),
                value: self.opacity as f64,
                minimum: 0.0,
                maximum: 1.0,
            });
        }
        if self.id.trim().is_empty() {
            return Err(CoreError::InvalidAdjustment {
                name: "spot_heal.id".into(),
                value: 0.0,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        Ok(())
    }
}
/// Extracts applicable heuristic spots from a recipe.
///
/// Reads the legacy `extras["spot_removals"]` array tolerantly: absent or
/// non-heuristic entries yield no spots (generative entries are skipped here
/// and rejected loudly in the render path instead). Entries without a `mode`
/// key default to heuristic (legacy documents).
///
/// SPOT-TYPED-FIELD-FIX + SPOT-CORE-SHADOW-FOLLOWUP note: the typed
/// schema-v2 `recipe.spot_removals` is intentionally NOT converted here —
/// `SpotRemoval` carries only version/mode/artifact and no heal geometry
/// (center/radius/feather/offset/opacity), so a typed entry cannot yield a
/// `SpotHeuristic`. Healing always comes from the extras view. The render
/// path (`render::apply_spot_heals_from_recipe`) skips a geometry-free typed
/// heuristic mirror shadow while the extras `spot_removals` key is present
/// (healthy loaded recipe) and rejects it loudly when isolated (no geometry
/// anywhere), as well as any generative/unknown-version entry.
pub fn spots_from_recipe(recipe: &lumina_sidecar::EditRecipe) -> Vec<SpotHeuristic> {
    let Some(value) = recipe.extras.get("spot_removals") else {
        return Vec::new();
    };
    let Ok(arr) = serde_json::from_value::<Vec<serde_json::Value>>(value.clone()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in arr {
        let mode = v
            .get("mode")
            .and_then(|m| m.as_str())
            .unwrap_or("heuristic");
        if mode != "heuristic" {
            continue;
        }
        if let Ok(s) = serde_json::from_value::<SpotHeuristic>(v) {
            out.push(s);
        }
    }
    out
}
pub fn apply_spot_heals(frame: &mut ImageFrame, spots: &[SpotHeuristic]) -> Result<(), CoreError> {
    if spots.is_empty() {
        return Ok(());
    }
    if frame.width == 0 || frame.height == 0 {
        return Ok(());
    }
    for s in spots {
        s.validate()?;
    }
    let src_pixels = frame.pixels.clone();
    let w = frame.width as f32;
    let h = frame.height as f32;
    for spot in spots {
        let cx = spot.center_x * w;
        let cy = spot.center_y * h;
        let radius = spot.radius;
        let feather = spot.feather;
        let opacity = spot.opacity;
        let dx = spot.offset_dx * w;
        let dy = spot.offset_dy * h;
        let x0 = (cx - radius).floor().max(0.0) as u32;
        let y0 = (cy - radius).floor().max(0.0) as u32;
        let x1 = (cx + radius).ceil().min(w) as u32;
        let y1 = (cy + radius).ceil().min(h) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let ddx = x as f32 + 0.5 - cx;
                let ddy = y as f32 + 0.5 - cy;
                let dist = (ddx * ddx + ddy * ddy).sqrt();
                let weight = if feather == 0.0 {
                    if dist <= radius {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    let inner = radius * (1.0 - feather);
                    if dist <= inner {
                        1.0
                    } else if dist <= radius {
                        1.0 - (dist - inner) / (radius - inner)
                    } else {
                        0.0
                    }
                };
                if weight == 0.0 {
                    continue;
                }
                let alpha = weight * opacity;
                if alpha == 0.0 {
                    continue;
                }
                let sx = (x as f32 + dx).round() as i32;
                let sy = (y as f32 + dy).round() as i32;
                let sx = sx.clamp(0, frame.width as i32 - 1) as u32;
                let sy = sy.clamp(0, frame.height as i32 - 1) as u32;
                let src_idx = (sy * frame.width + sx) as usize * 4;
                let dst_idx = (y * frame.width + x) as usize * 4;
                for c in 0..3 {
                    let src_v = src_pixels[src_idx + c] as f32;
                    let dst_v = frame.pixels[dst_idx + c] as f32;
                    let out = dst_v * (1.0 - alpha) + src_v * alpha;
                    frame.pixels[dst_idx + c] = out.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    Ok(())
}
pub fn psnr(a: &ImageFrame, b: &ImageFrame) -> f64 {
    assert_eq!(a.width, b.width);
    assert_eq!(a.height, b.height);
    let mut mse = 0.0;
    let n = (a.pixels.len() / 4 * 3) as f64;
    for (i, (pa, pb)) in a.pixels.iter().zip(b.pixels.iter()).enumerate() {
        if i % 4 == 3 {
            continue;
        }
        let d = *pa as f64 - *pb as f64;
        mse += d * d;
    }
    mse /= n;
    if mse == 0.0 {
        return f64::INFINITY;
    }
    20.0 * (255.0 / mse.sqrt()).log10()
}
/// LRPAR-G04-REMOVE: deterministic spot-candidate detection, threshold
/// visualization, distraction policy and generative variant seeds.
///
/// All functions here are pure, model-free and RNG-free: identical inputs
/// yield byte-identical outputs. ONNX-backed stages stay behind the F-078
/// gate and are never silently substituted (see `lumina-onnx::inpaint`).
/// A heuristic spot candidate (G-04 Detect-Objects, stage 1, no model).
/// Coordinates are source-normalized `0..=1`, `radius` is in source pixels,
/// `confidence` is the dark-pixel fraction `0..=1` of the winning cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectedSpot {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub confidence: f32,
}

/// Distraction category (G-04 Distraction Removal). Only `Dust` is served by
/// the heuristic stage 1; the others need an F-078-gated model and report
/// [`DistractionStatus::NeedsModel`] instead of guessing silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistractionKind {
    Reflections,
    People,
    Dust,
}

/// Explicit distraction switches (G-04). All default to off; `auto_mode`
/// only lists candidates and never applies anything silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DistractionSetting {
    pub reflections: bool,
    pub people: bool,
    pub dust: bool,
    pub auto_mode: bool,
}

/// Per-kind outcome of a distraction query: either heuristic candidates or
/// an explicit missing-model status (never a silent fallback).
#[derive(Debug, Clone, PartialEq)]
pub enum DistractionStatus {
    Ready(Vec<DetectedSpot>),
    NeedsModel {
        kind: DistractionKind,
        reason: String,
    },
}

fn luminance_rec709(r: u8, g: u8, b: u8) -> f32 {
    (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0
}

fn check_threshold(threshold: f32) -> Result<(), CoreError> {
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        return Err(CoreError::InvalidAdjustment {
            name: "spot_visualize.threshold".into(),
            value: threshold as f64,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    Ok(())
}

/// Byte mask (`0`/`255`, row-major, `width × height`) of pixels whose
/// Rec.709 luminance is `<= threshold`. Deterministic, model-free.
pub fn visualize_spots_mask(frame: &ImageFrame, threshold: f32) -> Result<Vec<u8>, CoreError> {
    check_threshold(threshold)?;
    let mut out = Vec::with_capacity(frame.width as usize * frame.height as usize);
    for px in frame.pixels.as_chunks::<4>().0 {
        let lum = luminance_rec709(px[0], px[1], px[2]);
        out.push(u8::from(lum <= threshold) * 255);
    }
    Ok(out)
}

/// Deterministic red tint overlay for threshold candidates (G-04 Visualize):
/// masked pixels blend `50 %` towards pure red, all other pixels (and alpha)
/// are byte-identical.
pub fn apply_visualize_overlay(frame: &mut ImageFrame, threshold: f32) -> Result<usize, CoreError> {
    let mask = visualize_spots_mask(frame, threshold)?;
    let mut count = 0usize;
    for (px, &m) in frame
        .pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(mask.iter())
    {
        if m == 255 {
            count += 1;
            px[0] = ((u16::from(px[0]) + 255) / 2).min(255) as u8;
            px[1] = (u16::from(px[1]) / 2) as u8;
            px[2] = (u16::from(px[2]) / 2) as u8;
        }
    }
    Ok(count)
}

/// Heuristic stage-1 spot detection (G-04 Detect-Objects, no model): dark
/// 8×8 cells (dark-pixel fraction `> 50 %` at `threshold`) become candidates
/// at the cell centre, sorted by confidence (descending, then position for
/// determinism) and capped at `max_spots`. Empty for empty frames; loud on
/// bad parameters.
pub fn detect_spots_heuristic(
    frame: &ImageFrame,
    threshold: f32,
    max_spots: usize,
) -> Result<Vec<DetectedSpot>, CoreError> {
    check_threshold(threshold)?;
    if max_spots == 0 || max_spots > 4096 {
        return Err(CoreError::InvalidAdjustment {
            name: "spot_detect.max_spots".into(),
            value: max_spots as f64,
            minimum: 1.0,
            maximum: 4096.0,
        });
    }
    if frame.width == 0 || frame.height == 0 {
        return Ok(Vec::new());
    }
    const CELL: u32 = 8;
    let mut out = Vec::new();
    let nx = frame.width.div_ceil(CELL);
    let ny = frame.height.div_ceil(CELL);
    for cy in 0..ny {
        for cx in 0..nx {
            let x0 = cx * CELL;
            let y0 = cy * CELL;
            let x1 = (x0 + CELL).min(frame.width);
            let y1 = (y0 + CELL).min(frame.height);
            let mut dark = 0u32;
            let mut total = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = (y * frame.width + x) as usize * 4;
                    let px = &frame.pixels[idx..idx + 4];
                    total += 1;
                    if luminance_rec709(px[0], px[1], px[2]) <= threshold {
                        dark += 1;
                    }
                }
            }
            let fraction = dark as f32 / total as f32;
            if fraction > 0.5 {
                let w = (x1 - x0) as f32;
                let h = (y1 - y0) as f32;
                out.push(DetectedSpot {
                    x: ((x0 as f32 + w / 2.0) + 0.5) / frame.width as f32,
                    y: ((y0 as f32 + h / 2.0) + 0.5) / frame.height as f32,
                    radius: (w.min(h) / 2.0).clamp(1.0, 512.0),
                    confidence: fraction,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.y.total_cmp(&b.y))
            .then_with(|| a.x.total_cmp(&b.x))
    });
    out.truncate(max_spots);
    Ok(out)
}

/// Distraction query (G-04): `dust` reuses the heuristic detector when
/// enabled (empty when disabled); `reflections`/`people` report
/// `NeedsModel` when enabled (F-078 gate, no silent heuristic substitute)
/// and empty-`Ready` when disabled. `auto_mode` never applies anything — it
/// only travels with the setting so callers can log that listing (not
/// applying) happened.
pub fn distraction_candidates(
    frame: &ImageFrame,
    setting: DistractionSetting,
    threshold: f32,
    max_spots: usize,
) -> Result<Vec<(DistractionKind, DistractionStatus)>, CoreError> {
    check_threshold(threshold)?;
    let mut out = Vec::with_capacity(3);
    if setting.reflections {
        out.push((
            DistractionKind::Reflections,
            DistractionStatus::NeedsModel {
                kind: DistractionKind::Reflections,
                reason: "no F-078-gated reflections model configured; heuristic stage 1 covers dust only"
                    .into(),
            },
        ));
    } else {
        out.push((
            DistractionKind::Reflections,
            DistractionStatus::Ready(Vec::new()),
        ));
    }
    if setting.people {
        out.push((
            DistractionKind::People,
            DistractionStatus::NeedsModel {
                kind: DistractionKind::People,
                reason:
                    "no F-078-gated people model configured; heuristic stage 1 covers dust only"
                        .into(),
            },
        ));
    } else {
        out.push((
            DistractionKind::People,
            DistractionStatus::Ready(Vec::new()),
        ));
    }
    if setting.dust {
        out.push((
            DistractionKind::Dust,
            DistractionStatus::Ready(detect_spots_heuristic(frame, threshold, max_spots)?),
        ));
    } else {
        out.push((DistractionKind::Dust, DistractionStatus::Ready(Vec::new())));
    }
    Ok(out)
}

/// Deterministic generative variant seed (G-04): `variant == 0` keeps `base`
/// (back-compatible with pre-variant recipes); any other variant hashes
/// base + variant with SplitMix64 (same algorithm as
/// `lumina-onnx::variant_seed` — keep the twins in sync).
pub fn generative_variant_seed(base_seed: u64, variant: u64) -> u64 {
    if variant == 0 {
        return base_seed;
    }
    let mut z = base_seed
        .wrapping_add(0x9E3779B97F4A7C15)
        .wrapping_add(variant.wrapping_mul(0xBF58476D1CE4E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::histogram::LuminanceHistogram;
    fn checker(w: u32, h: u32) -> ImageFrame {
        let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }
    fn spot(cx: f32, cy: f32, r: f32, f: f32, dx: f32, dy: f32, o: f32) -> SpotHeuristic {
        SpotHeuristic {
            id: "spot-1".into(),
            version: 1,
            center_x: cx,
            center_y: cy,
            radius: r,
            feather: f,
            offset_dx: dx,
            offset_dy: dy,
            opacity: o,
            status: "valid".into(),
        }
    }
    #[test]
    fn deterministic_identical_inputs_byte_identical() {
        let mut a = checker(8, 8);
        let mut b = checker(8, 8);
        let s = spot(0.5, 0.5, 2.0, 0.0, 0.25, 0.0, 1.0);
        apply_spot_heals(&mut a, std::slice::from_ref(&s)).unwrap();
        apply_spot_heals(&mut b, &[s]).unwrap();
        assert_eq!(a.pixels, b.pixels);
    }
    #[test]
    fn outside_radius_unchanged() {
        let mut chk = checker(4, 4);
        let before = chk.clone();
        let s = spot(0.5, 0.5, 1.0, 0.0, 0.4, 0.0, 1.0);
        apply_spot_heals(&mut chk, &[s]).unwrap();
        assert_eq!(chk.pixels[0..4], before.pixels[0..4]);
    }
    #[test]
    fn feather_weighted() {
        let mut pixels = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                let v = if x < 4 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let s_no = spot(0.5, 0.5, 2.0, 0.0, 0.3, 0.0, 1.0);
        let mut a = frame.clone();
        apply_spot_heals(&mut a, &[s_no]).unwrap();
        let s_fe = spot(0.5, 0.5, 2.0, 1.0, 0.3, 0.0, 1.0);
        let mut b = frame.clone();
        apply_spot_heals(&mut b, &[s_fe]).unwrap();
        assert_ne!(a.pixels, b.pixels);
    }
    #[test]
    fn opacity_zero_identity() {
        let mut chk = checker(8, 8);
        let before = chk.clone();
        let s = spot(0.5, 0.5, 3.0, 0.0, 0.25, 0.0, 0.0);
        apply_spot_heals(&mut chk, &[s]).unwrap();
        assert_eq!(chk.pixels, before.pixels);
    }
    #[test]
    fn invalid_radius_rejected() {
        let mut frame = checker(4, 4);
        let s = spot(0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 1.0);
        assert!(apply_spot_heals(&mut frame, &[s]).is_err());
        let s2 = spot(0.5, 0.5, 600.0, 0.0, 0.0, 0.0, 1.0);
        assert!(apply_spot_heals(&mut frame, &[s2]).is_err());
    }
    #[test]
    fn alpha_unchanged() {
        let mut frame = ImageFrame::new(
            2,
            2,
            vec![
                10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
            ],
        )
        .unwrap();
        let s = spot(0.5, 0.5, 2.0, 0.0, 0.25, 0.0, 1.0);
        apply_spot_heals(&mut frame, &[s]).unwrap();
        assert_eq!(frame.pixels[3], 40);
        assert_eq!(frame.pixels[7], 80);
        assert_eq!(frame.pixels[11], 120);
        assert_eq!(frame.pixels[15], 160);
    }
    #[test]
    fn golden_8x8_checker_byte_identical_heal() {
        let mut pixels = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                let v = if x < 4 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let mut healed = frame.clone();
        let s = spot(0.25, 0.5, 2.0, 0.5, 0.5, 0.0, 1.0);
        apply_spot_heals(&mut healed, &[s]).unwrap();
        let ps = psnr(&frame, &healed);
        assert!(ps.is_finite() && ps > 10.0, "psnr {ps}");
        let mut healed2 = frame.clone();
        let s2 = spot(0.25, 0.5, 2.0, 0.5, 0.5, 0.0, 1.0);
        apply_spot_heals(&mut healed2, &[s2]).unwrap();
        assert_eq!(healed.pixels, healed2.pixels);
    }
    #[test]
    fn histogram_digest_delta_within_tolerance() {
        let mut pixels = Vec::new();
        for _y in 0..16 {
            for x in 0..16 {
                let v = if x < 8 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let frame = ImageFrame::new(16, 16, pixels).unwrap();
        let mut healed = frame.clone();
        let s = spot(0.25, 0.5, 3.0, 0.2, 0.5, 0.0, 1.0);
        apply_spot_heals(&mut healed, &[s]).unwrap();
        let h1 = LuminanceHistogram::new(&frame);
        let h2 = LuminanceHistogram::new(&healed);
        assert_ne!(h1.digest(), h2.digest());
        assert!((h1.mean() - h2.mean()).abs() < 0.5);
        assert!(h2.mean() > h1.mean());
    }
    #[test]
    fn spots_from_recipe_absent_is_empty() {
        let recipe = lumina_sidecar::EditRecipe::default();
        assert!(spots_from_recipe(&recipe).is_empty());
    }
    #[test]
    fn spots_from_recipe_roundtrip_via_extras() {
        let mut recipe = lumina_sidecar::EditRecipe::default();
        let s = spot(0.501, 0.498, 18.0, 0.5, 0.05, -0.02, 1.0);
        recipe.extras.insert("spot_removals".into(), serde_json::to_value(vec![serde_json::json!({"id":s.id,"version":s.version,"center_x":s.center_x,"center_y":s.center_y,"radius":s.radius,"feather":s.feather,"offset_dx":s.offset_dx,"offset_dy":s.offset_dy,"opacity":s.opacity,"status":s.status,"mode":"heuristic"})]).unwrap());
        let parsed = spots_from_recipe(&recipe);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "spot-1");
        // SPOT-SCHEMA-GEOMETRY / SPOT-CORE-SHADOW-FOLLOWUP contract:
        // `EditRecipe` serde mirrors the raw `spot_removals` value back into
        // `extras` on deserialize (the typed schema-v2 `SpotRemoval` holds
        // only version/mode/artifact, so the extras view remains the
        // geometry-carrying source of truth). A JSON roundtrip therefore
        // keeps the extras key AND populates the typed mirror shadow; the
        // render path heals from extras and tolerates that shadow (see
        // `render::reject_unsupported_spot_modes_typed`).
        let json = serde_json::to_string(&recipe).unwrap();
        let back: lumina_sidecar::EditRecipe = serde_json::from_str(&json).unwrap();
        assert!(
            back.extras.contains_key("spot_removals"),
            "serde mirrors raw spot_removals back into extras"
        );
        assert_eq!(
            back.extras.get("spot_removals"),
            recipe.extras.get("spot_removals"),
            "extras spot geometry survives the roundtrip"
        );
        assert_eq!(back.spot_removals.len(), 1);
        assert_eq!(
            back.spot_removals[0].mode,
            lumina_sidecar::SpotRemovalMode::Heuristic
        );
    }
    #[test]
    fn generative_mode_filtered_out() {
        let mut recipe = lumina_sidecar::EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"g1","version":1,"mode":"generative","prompt":"test"}]),
        );
        assert!(spots_from_recipe(&recipe).is_empty());
    }
    #[test]
    fn spots_from_recipe_typed_entries_carry_no_convertible_geometry() {
        // SPOT-TYPED-FIELD-FIX: schema-v2 typed entries hold only
        // version/mode/artifact — no heal geometry — so they contribute no
        // SpotHeuristic here. They are rejected loudly in the render path
        // instead of silently skipped.
        let mut recipe = lumina_sidecar::EditRecipe::default();
        recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Heuristic,
            artifact: None,
        });
        assert!(spots_from_recipe(&recipe).is_empty());
    }
    #[test]
    fn spots_from_recipe_legacy_extras_parsed_alongside_typed_entries() {
        // Legacy extras heuristic spots keep working while typed entries are
        // present (tolerant read, strict render validation elsewhere).
        let mut recipe = lumina_sidecar::EditRecipe::default();
        let s = spot(0.501, 0.498, 18.0, 0.5, 0.05, -0.02, 1.0);
        recipe.extras.insert("spot_removals".into(), serde_json::to_value(vec![serde_json::json!({"id":s.id,"version":s.version,"center_x":s.center_x,"center_y":s.center_y,"radius":s.radius,"feather":s.feather,"offset_dx":s.offset_dx,"offset_dy":s.offset_dy,"opacity":s.opacity,"status":s.status,"mode":"heuristic"})]).unwrap());
        recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Generative,
            artifact: None,
        });
        let parsed = spots_from_recipe(&recipe);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "spot-1");
    }
    // ---- LRPAR-G04-REMOVE -------------------------------------------------
    fn dark_frame(w: u32, h: u32, dark: &[(u32, u32)]) -> ImageFrame {
        let mut pixels = vec![255u8; w as usize * h as usize * 4];
        for (x, y) in dark {
            let idx = (*y * w + *x) as usize * 4;
            pixels[idx] = 0;
            pixels[idx + 1] = 0;
            pixels[idx + 2] = 0;
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }
    #[test]
    fn visualize_mask_is_deterministic_and_threshold_bounded() {
        let dark: Vec<(u32, u32)> = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).collect();
        let frame = dark_frame(16, 16, &dark);
        let a = visualize_spots_mask(&frame, 0.5).unwrap();
        let b = visualize_spots_mask(&frame, 0.5).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 256);
        assert_eq!(a.iter().filter(|&&m| m == 255).count(), 64);
        assert_eq!(a.iter().filter(|&&m| m == 0).count(), 192);
        assert!(visualize_spots_mask(&frame, f32::NAN).is_err());
        assert!(visualize_spots_mask(&frame, 1.5).is_err());
        assert!(visualize_spots_mask(&frame, -0.1).is_err());
    }
    #[test]
    fn visualize_overlay_tints_only_candidates() {
        let dark: Vec<(u32, u32)> = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).collect();
        let frame = dark_frame(16, 16, &dark);
        let mut over = frame.clone();
        let count = apply_visualize_overlay(&mut over, 0.5).unwrap();
        assert_eq!(count, 64);
        // Untinted pixels (white area) are byte-identical, alpha untouched.
        for y in 0..16 {
            for x in 8..16 {
                let i = (y * 16 + x) as usize * 4;
                assert_eq!(over.pixels[i..i + 4], frame.pixels[i..i + 4]);
            }
        }
        // Tinted pixels moved towards red.
        let i = 0;
        assert!(over.pixels[i] >= frame.pixels[i]);
        assert!(over.pixels[i + 3] == 255);
        let ps = psnr(&frame, &over);
        assert!(ps.is_finite() && ps > 5.0, "overlay PSNR gate: {ps}");
        // Deterministic: same inputs byte-identical.
        let mut over2 = frame.clone();
        apply_visualize_overlay(&mut over2, 0.5).unwrap();
        assert_eq!(over.pixels, over2.pixels);
    }
    #[test]
    fn detect_heuristic_finds_dark_cells_deterministically() {
        let dark: Vec<(u32, u32)> = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).collect();
        let frame = dark_frame(16, 16, &dark);
        let a = detect_spots_heuristic(&frame, 0.5, 16).unwrap();
        let b = detect_spots_heuristic(&frame, 0.5, 16).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 1);
        assert!((a[0].confidence - 1.0).abs() < 1e-6);
        // Bright frame: no candidates.
        let bright = ImageFrame::new(16, 16, vec![255u8; 16 * 16 * 4]).unwrap();
        assert!(detect_spots_heuristic(&bright, 0.5, 16).unwrap().is_empty());
        // max_spots caps deterministically.
        let all_dark: Vec<(u32, u32)> =
            (0..16).flat_map(|y| (0..16).map(move |x| (x, y))).collect();
        let full = dark_frame(16, 16, &all_dark);
        let capped = detect_spots_heuristic(&full, 0.5, 2).unwrap();
        assert_eq!(capped.len(), 2);
        assert!(detect_spots_heuristic(&frame, 0.5, 0).is_err());
        assert!(detect_spots_heuristic(&frame, f32::NAN, 4).is_err());
    }
    #[test]
    fn distraction_policy_never_acts_silently() {
        let dark: Vec<(u32, u32)> = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).collect();
        let frame = dark_frame(16, 16, &dark);
        // All off: everything empty Ready.
        let off = DistractionSetting::default();
        let res = distraction_candidates(&frame, off, 0.5, 8).unwrap();
        for (_, status) in &res {
            assert_eq!(*status, DistractionStatus::Ready(Vec::new()));
        }
        // Dust on: heuristic candidates, identical to detect.
        let dust = DistractionSetting {
            dust: true,
            auto_mode: true,
            ..Default::default()
        };
        let res = distraction_candidates(&frame, dust, 0.5, 8).unwrap();
        let dust_status = res
            .iter()
            .find(|(k, _)| *k == DistractionKind::Dust)
            .unwrap();
        match &dust_status.1 {
            DistractionStatus::Ready(spots) => assert_eq!(spots.len(), 1),
            other => panic!("dust must be Ready, got {other:?}"),
        }
        // Reflections/people on without a model: loud NeedsModel, never a
        // silent heuristic substitute.
        let rp = DistractionSetting {
            reflections: true,
            people: true,
            ..Default::default()
        };
        let res = distraction_candidates(&frame, rp, 0.5, 8).unwrap();
        for (kind, status) in &res {
            match kind {
                DistractionKind::Reflections | DistractionKind::People => {
                    assert!(
                        matches!(status, DistractionStatus::NeedsModel { .. }),
                        "{kind:?}"
                    );
                }
                DistractionKind::Dust => {
                    assert_eq!(*status, DistractionStatus::Ready(Vec::new()));
                }
            }
        }
    }
    #[test]
    fn variant_seed_zero_stable_others_distinct_and_deterministic() {
        assert_eq!(generative_variant_seed(7, 0), 7);
        let v1 = generative_variant_seed(7, 1);
        let v1b = generative_variant_seed(7, 1);
        assert_eq!(v1, v1b);
        assert_ne!(v1, 7);
        assert_ne!(generative_variant_seed(7, 2), v1);
        assert_ne!(generative_variant_seed(8, 1), v1);
    }
}
