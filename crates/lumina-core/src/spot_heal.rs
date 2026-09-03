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
/// SPOT-TYPED-FIELD-FIX note: the typed schema-v2 `recipe.spot_removals` is
/// intentionally NOT converted here — `SpotRemoval` carries only
/// version/mode/artifact and no heal geometry (center/radius/feather/offset/
/// opacity), so a typed entry cannot yield a `SpotHeuristic`. Typed entries
/// are validated loudly in the render path
/// (`render::apply_spot_heals_from_recipe`) instead of being silently treated
/// as healed or as absent.
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
        // GEN-ZDATA-LINK-1 contract: `EditRecipe` serde consumes the
        // top-level `spot_removals` key into the typed schema-v2 field, so a
        // JSON roundtrip moves the entry out of extras. The typed entry keeps
        // the key (no silent key loss) but drops the heuristic geometry
        // (`SpotRemoval` holds only version/mode/artifact) — the render path
        // rejects such typed entries loudly instead of healing nothing
        // (see `render::tests::typed_heuristic_spot_without_geometry_is_hard_error`).
        let json = serde_json::to_string(&recipe).unwrap();
        let back: lumina_sidecar::EditRecipe = serde_json::from_str(&json).unwrap();
        assert!(
            !back.extras.contains_key("spot_removals"),
            "serde moves spot_removals into the typed field"
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
}
