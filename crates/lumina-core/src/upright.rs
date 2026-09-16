//! LRPAR-G06-UPRIGHT-15: automatic upright analysis (Release 1.5).
//!
//! SOLL: `feature/architecture/pipeline.md` § F-099, Abschnitt
//! „LRPAR-G06-UPRIGHT-15 Auto-Upright-Analyse". This module is the classic,
//! model-free, deterministic line detector behind the persisted
//! `recipe.upright` stage. It performs **no** rendering itself: it produces a
//! [`UprightSuggestion`] (suggested F-099 `vertical`/`horizontal`/`rotation`
//! plus deterministic evidence), which the caller wraps into a
//! [`lumina_sidecar::UprightAnalysis`] with a source identity fingerprint.
//!
//! The suggested correction is applied via the existing F-099 perspective
//! homography ([`lumina_sidecar::EditRecipe::effective_perspective`]), never via
//! a second transform. Pure arithmetic: no FS/IO, no randomness, no ONNX — the
//! same frame yields byte-identical suggestions.
//!
//! Algorithm `upright-lines-v1` (projection profiles): grayscale box
//! downsample, then for each candidate angle θ in a deterministic ±30° sweep
//! the image is binned along the rotated axis and the sharpness of the
//! resulting projection profile is measured (sum of squared second
//! differences). The angle that best aligns the near-horizontal /
//! near-vertical lines with the axes is the rotation suggestion; the
//! difference of the best angle between the left/right (vertical lines) and
//! top/bottom (horizontal lines) halves is the keystone suggestion. The
//! projection-profile method is immune to the staircase aliasing that biases
//! per-pixel gradient orientation histograms on large-period gratings.

use lumina_sidecar::{AnalysisFingerprint, UprightAnalysis};

use crate::ImageFrame;

/// Algorithm name persisted in the analysis fingerprint.
pub const UPRIGHT_ALGORITHM: &str = "upright-lines-v1";
/// Algorithm version persisted in the analysis fingerprint. Bump when the
/// detector changes so stale analyses are visible.
pub const UPRIGHT_ALGORITHM_VERSION: &str = "1";

/// Longest side of the internal working grayscale image. The projection sweep
/// is `O(angles × pixels)`; 256 px keep an explicit analysis fast while staying
/// stable on real photos.
pub const UPRIGHT_WORK_MAX_SIDE: u32 = 256;
/// Search half range (degrees) of the angle sweep. A tilt larger than this
/// saturates at the domain edge (documented, deterministic).
pub const UPRIGHT_ANGLE_LIMIT_DEG: f32 = 30.0;
/// Deterministic angle sweep step (degrees).
pub const UPRIGHT_ANGLE_STEP_DEG: f32 = 0.5;

/// Deterministic suggestion of one upright analysis. The three coefficients are
/// in the F-099 perspective domain (`-1..=1`); `line_count` and `confidence`
/// are evidence, not a user-facing score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UprightSuggestion {
    pub vertical: f32,
    pub horizontal: f32,
    pub rotation: f32,
    pub line_count: u32,
    pub confidence: f32,
}

impl UprightSuggestion {
    /// Identity suggestion (no supporting lines found).
    pub const NONE: UprightSuggestion = UprightSuggestion {
        vertical: 0.0,
        horizontal: 0.0,
        rotation: 0.0,
        line_count: 0,
        confidence: 0.0,
    };
}

/// Deterministic source-identity fingerprint for an upright analysis.
///
/// The analysis sees only pixels; the *identity* of the analysed input is the
/// source content hash plus the decode geometry context and the algorithm
/// version. Persisting this string lets CLI/GUI detect a stale analysis
/// without re-running the detector (SOLL: Identität/Veraltung).
pub fn upright_input_fingerprint(
    content_hash: &str,
    width: u32,
    height: u32,
    orientation: u8,
) -> String {
    let canonical = format!(
        "upright|{UPRIGHT_ALGORITHM}|{UPRIGHT_ALGORITHM_VERSION}|{content_hash}|{width}x{height}|o{orientation}"
    );
    format!("blake3:{}", blake3::hash(canonical.as_bytes()).to_hex())
}

/// Wraps a [`UprightSuggestion`] into the persisted, validated schema object
/// with the given source-identity fingerprint.
pub fn upright_analysis(
    suggestion: UprightSuggestion,
    input_fingerprint: impl Into<String>,
) -> UprightAnalysis {
    UprightAnalysis {
        fingerprint: AnalysisFingerprint {
            algorithm: UPRIGHT_ALGORITHM.into(),
            version: UPRIGHT_ALGORITHM_VERSION.into(),
            input_fingerprint: input_fingerprint.into(),
            extras: Default::default(),
        },
        vertical: suggestion.vertical,
        horizontal: suggestion.horizontal,
        rotation: suggestion.rotation,
        line_count: suggestion.line_count,
        confidence: suggestion.confidence,
    }
}

/// Working dimensions for the downsampled analysis image (longest side
/// `UPRIGHT_WORK_MAX_SIDE`), never zero for a non-empty input.
fn work_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0);
    }
    let longest = width.max(height);
    if longest <= UPRIGHT_WORK_MAX_SIDE {
        return (width, height);
    }
    let scale = UPRIGHT_WORK_MAX_SIDE as f64 / longest as f64;
    let w = ((width as f64 * scale).round() as u32).max(1);
    let h = ((height as f64 * scale).round() as u32).max(1);
    (w, h)
}

/// Box-averaged Rec.709 luminance of the frame at the working resolution.
fn luma_downsample(frame: &ImageFrame) -> (u32, u32, Vec<f32>) {
    let (gw, gh) = work_dimensions(frame.width, frame.height);
    if gw == 0 || gh == 0 {
        return (0, 0, Vec::new());
    }
    let mut out = vec![0.0f32; (gw * gh) as usize];
    let sw = frame.width as u64;
    let sh = frame.height as u64;
    for gy in 0..gh {
        let y0 = gy as u64 * sh / gh as u64;
        let y1 = ((gy as u64 + 1) * sh / gh as u64).max(y0 + 1);
        for gx in 0..gw {
            let x0 = gx as u64 * sw / gw as u64;
            let x1 = ((gx as u64 + 1) * sw / gw as u64).max(x0 + 1);
            let mut sum = 0.0f64;
            let mut count = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * sw + x) * 4) as usize;
                    sum += 0.2126 * frame.pixels[i] as f64
                        + 0.7152 * frame.pixels[i + 1] as f64
                        + 0.0722 * frame.pixels[i + 2] as f64;
                    count += 1;
                }
            }
            out[(gy * gw + gx) as usize] = (sum / count.max(1) as f64) as f32;
        }
    }
    (gw, gh, out)
}

/// Sharpness of a projection profile: sum of squared second differences. A
/// strongly aligned line grid produces a spiky profile (high score); a
/// constant profile scores zero.
fn projection_sharpness(profile: &[f32]) -> f32 {
    if profile.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for w in profile.windows(3) {
        let second = w[2] - 2.0 * w[1] + w[0];
        sum += second * second;
    }
    sum
}

/// Best `(angle_deg, score)` of one profile family over the deterministic
/// sweep. Ties resolve to the angle closest to zero (stable, documented).
#[derive(Debug, Clone, Copy, Default)]
struct AngleScore {
    angle_deg: f32,
    score: f32,
    initialized: bool,
}

impl AngleScore {
    fn observe(&mut self, angle_deg: f32, score: f32) {
        let better = !self.initialized
            || score > self.score
            || (score == self.score && angle_deg.abs() < self.angle_deg.abs());
        if better {
            self.angle_deg = angle_deg;
            self.score = score;
            self.initialized = true;
        }
    }
}

/// Maximum number of angle iterations (deterministic, inclusive sweep).
fn angle_step_count() -> usize {
    (2.0 * UPRIGHT_ANGLE_LIMIT_DEG / UPRIGHT_ANGLE_STEP_DEG).round() as usize + 1
}

/// Maps a signed deviation (radians) to the F-099 domain (`-1..=1` over ±45°)
/// with the correction sign (`deviation → coefficient = -deviation/45°`).
fn deviation_to_coefficient(deviation_rad: f32) -> f32 {
    (-deviation_rad / std::f32::consts::FRAC_PI_4).clamp(-1.0, 1.0)
}

/// Classic, model-free, deterministic upright analysis. See the module docs
/// for the `upright-lines-v1` contract.
pub fn analyze_upright(frame: &ImageFrame) -> UprightSuggestion {
    if frame.width < 8 || frame.height < 8 {
        return UprightSuggestion::NONE;
    }
    let (gw, gh, luma) = luma_downsample(frame);
    if gw < 8 || gh < 8 {
        return UprightSuggestion::NONE;
    }

    // Gradient evidence: count pixels whose Sobel magnitude exceeds 25 % of
    // the strongest gradient (the reported `line_count`).
    let (mut magnitudes, max_magnitude) = gradient_evidence(gw, gh, &luma);
    let mut line_count = 0u32;
    if max_magnitude > 0.0 {
        let threshold = max_magnitude * 0.25;
        magnitudes.retain(|m| *m >= threshold);
        line_count = magnitudes.len() as u32;
    }
    if line_count == 0 {
        return UprightSuggestion::NONE;
    }

    let cx = (gw as f32 - 1.0) * 0.5;
    let cy = (gh as f32 - 1.0) * 0.5;
    // Rotated coordinates stay within the image diagonal; one profile per
    // family half is accumulated in the same sweep.
    let bins = (gw + gh) as usize + 2;
    let offset = (bins / 2) as isize;

    // Full-frame horizontal/vertical alignment + the four position halves.
    let mut h_full = AngleScore::default();
    let mut h_top = AngleScore::default();
    let mut h_bottom = AngleScore::default();
    let mut v_full = AngleScore::default();
    let mut v_left = AngleScore::default();
    let mut v_right = AngleScore::default();
    let mut h_score_sum = 0.0f64;
    let mut v_score_sum = 0.0f64;

    let steps = angle_step_count();
    let half_index = bins / 2;
    let mut h_profile = vec![0.0f32; bins];
    let mut v_profile = vec![0.0f32; bins];

    for step in 0..steps {
        let angle_deg = -UPRIGHT_ANGLE_LIMIT_DEG + step as f32 * UPRIGHT_ANGLE_STEP_DEG;
        let (sa, ca) = angle_deg.to_radians().sin_cos();

        // Horizontal line alignment: `h = -sinθ·(x-cx) + cosθ·(y-cy)`.
        h_profile.iter_mut().for_each(|v| *v = 0.0);
        for y in 0..gh {
            let dy = y as f32 - cy;
            for x in 0..gw {
                let dx = x as f32 - cx;
                let value = luma[(y * gw + x) as usize];
                let h = -sa * dx + ca * dy;
                let index = (h.round() as isize + offset).clamp(0, bins as isize - 1) as usize;
                h_profile[index] += value;
            }
        }
        let score = projection_sharpness(&h_profile);
        h_score_sum += score as f64;
        h_full.observe(angle_deg, score);
        score_halves(&h_profile, half_index, &mut h_top, &mut h_bottom, angle_deg);

        // Vertical line alignment: `v = cosδ·(x-cx) + sinδ·(y-cy)`.
        v_profile.iter_mut().for_each(|v| *v = 0.0);
        for y in 0..gh {
            let dy = y as f32 - cy;
            for x in 0..gw {
                let dx = x as f32 - cx;
                let value = luma[(y * gw + x) as usize];
                let v = ca * dx + sa * dy;
                let index = (v.round() as isize + offset).clamp(0, bins as isize - 1) as usize;
                v_profile[index] += value;
            }
        }
        let score = projection_sharpness(&v_profile);
        v_score_sum += score as f64;
        v_full.observe(angle_deg, score);
        score_halves(&v_profile, half_index, &mut v_left, &mut v_right, angle_deg);
    }

    // Rotation: prefer the axis with the stronger alignment peak.
    let (rotation_rad, peak, mean_score) = if h_full.score >= v_full.score {
        (
            h_full.angle_deg.to_radians(),
            h_full.score,
            h_score_sum / steps as f64,
        )
    } else {
        (
            v_full.angle_deg.to_radians(),
            v_full.score,
            v_score_sum / steps as f64,
        )
    };
    if peak <= 0.0 {
        return UprightSuggestion::NONE;
    }
    let rotation = deviation_to_coefficient(rotation_rad);

    // Keystone: the difference between the halves' best angles. A pure
    // rotation moves both halves identically; a convergence moves them apart.
    let vertical_keystone =
        deviation_to_coefficient(v_right.angle_deg.to_radians() - v_left.angle_deg.to_radians());
    let horizontal_keystone =
        deviation_to_coefficient(h_bottom.angle_deg.to_radians() - h_top.angle_deg.to_radians());

    // Confidence: how much the full-frame peak stands out from the mean sweep
    // score (`0..=1`; a flat/noise profile is near zero, a strong line grid
    // near one).
    let confidence = (1.0 - mean_score / peak as f64).clamp(0.0, 1.0) as f32;

    UprightSuggestion {
        vertical: vertical_keystone,
        horizontal: horizontal_keystone,
        rotation,
        line_count,
        confidence,
    }
}

/// Updates the `first`/`second` half scores of a profile split at `half_index`.
fn score_halves(
    profile: &[f32],
    half_index: usize,
    first: &mut AngleScore,
    second: &mut AngleScore,
    angle_deg: f32,
) {
    first.observe(angle_deg, projection_sharpness(&profile[..half_index]));
    second.observe(angle_deg, projection_sharpness(&profile[half_index..]));
}

/// Returns all Sobel magnitudes of the interior pixels plus the maximum.
fn gradient_evidence(gw: u32, gh: u32, luma: &[f32]) -> (Vec<f32>, f32) {
    let idx = |x: u32, y: u32| (y * gw + x) as usize;
    let mut magnitudes = Vec::with_capacity((gw * gh) as usize);
    let mut max = 0.0f32;
    for y in 0..gh {
        for x in 0..gw {
            if x == 0 || y == 0 || x + 1 >= gw || y + 1 >= gh {
                continue;
            }
            let p = |dx: u32, dy: u32| luma[idx(x + dx - 1, y + dy - 1)];
            let gx = (p(2, 0) + 2.0 * p(2, 1) + p(2, 2)) - (p(0, 0) + 2.0 * p(0, 1) + p(0, 2));
            let gy = (p(0, 2) + 2.0 * p(1, 2) + p(2, 2)) - (p(0, 0) + 2.0 * p(1, 0) + p(2, 0));
            let magnitude = (gx * gx + gy * gy).sqrt();
            max = max.max(magnitude);
            magnitudes.push(magnitude);
        }
    }
    (magnitudes, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{EditRecipe, Upright};

    /// Synthetic `w`×`h` frame with a bright grid (horizontal + vertical bars)
    /// rotated by `angle` radians, on a dark background.
    fn grid_frame(w: u32, h: u32, angle: f32) -> ImageFrame {
        let (ca, sa) = (angle.cos(), angle.sin());
        let period = 0.4f32;
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                let ny = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
                let rx = ca * nx - sa * ny;
                let ry = sa * nx + ca * ny;
                let dx = (rx / period - (rx / period).round()).abs() * period;
                let dy = (ry / period - (ry / period).round()).abs() * period;
                let on = dx < 0.05 || dy < 0.05;
                let value = if on { 235u8 } else { 20u8 };
                let i = ((y * w + x) as usize) * 4;
                pixels[i] = value;
                pixels[i + 1] = value;
                pixels[i + 2] = value;
                pixels[i + 3] = 255;
            }
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }

    /// Horizontal bars only (unambiguous skew signal).
    fn bars_frame(w: u32, h: u32, angle: f32) -> ImageFrame {
        let (ca, sa) = (angle.cos(), angle.sin());
        let period = 0.4f32;
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                let ny = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
                let ry = sa * nx + ca * ny;
                let dy = (ry / period - (ry / period).round()).abs() * period;
                let value = if dy < 0.05 { 235u8 } else { 20u8 };
                let i = ((y * w + x) as usize) * 4;
                pixels[i] = value;
                pixels[i + 1] = value;
                pixels[i + 2] = value;
                pixels[i + 3] = 255;
            }
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }

    /// Vertical lines that converge toward the top (positive `strength`),
    /// i.e. a classic keystone distortion: lines on the left tilt one way,
    /// lines on the right the other way.
    fn keystone_frame(w: u32, h: u32, strength: f32) -> ImageFrame {
        let period = 0.4f32;
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                let ny = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
                let scaled = nx / (1.0 + strength * ny);
                let d = (scaled / period - (scaled / period).round()).abs() * period;
                let value = if d < 0.03 { 235u8 } else { 20u8 };
                let i = ((y * w + x) as usize) * 4;
                pixels[i] = value;
                pixels[i + 1] = value;
                pixels[i + 2] = value;
                pixels[i + 3] = 255;
            }
        }
        ImageFrame::new(w, h, pixels).unwrap()
    }

    #[test]
    fn analysis_reduces_vertical_keystone() {
        for strength in [0.12f32, -0.12] {
            let frame = keystone_frame(256, 256, strength);
            let suggestion = analyze_upright(&frame);
            assert!(
                suggestion.vertical.abs() > 0.01,
                "keystone must yield a vertical suggestion: {suggestion:?}"
            );
            let recipe = EditRecipe {
                upright: Some(Upright {
                    version: 1,
                    enabled: true,
                    analysis: Some(upright_analysis(suggestion, "blake3:test")),
                }),
                ..Default::default()
            };
            let effective = recipe.effective_perspective().unwrap();
            let mut corrected = frame.clone();
            apply_perspective_only(&mut corrected, &effective);
            let residual = analyze_upright(&corrected);
            assert!(
                residual.vertical.abs() < suggestion.vertical.abs(),
                "correction must reduce the keystone (strength {strength}, suggested {}, \
                 residual {})",
                suggestion.vertical,
                residual.vertical
            );
        }
    }

    /// Feature-agnostic perspective application for the headless gates
    /// (`lensfun` changes the signature of the public stage).
    fn apply_perspective_only(frame: &mut ImageFrame, perspective: &lumina_sidecar::Perspective) {
        #[cfg(feature = "lensfun")]
        frame
            .apply_perspective_stage(None, Some(perspective), None)
            .unwrap();
        #[cfg(not(feature = "lensfun"))]
        frame
            .apply_perspective_stage(None, Some(perspective))
            .unwrap();
    }

    #[test]
    fn analysis_is_deterministic_and_identity_on_flat_input() {
        let frame = grid_frame(240, 240, 0.1);
        let first = analyze_upright(&frame);
        let second = analyze_upright(&frame);
        assert_eq!(first, second, "two runs must be byte-identical");
        assert!(first.line_count > 0);

        let flat = ImageFrame::new(64, 64, vec![128; 64 * 64 * 4]).unwrap();
        assert_eq!(analyze_upright(&flat), UprightSuggestion::NONE);
    }

    #[test]
    fn analysis_suggests_the_inverse_rotation_for_a_tilted_grid() {
        for angle_deg in [4.0f32, 8.0, -6.0, 15.0] {
            let frame = bars_frame(256, 256, angle_deg.to_radians());
            let suggestion = analyze_upright(&frame);
            assert!(suggestion.confidence > 0.0);
            assert!(
                suggestion.rotation.abs() > 0.02,
                "a {angle_deg}° tilt must yield a non-trivial rotation suggestion"
            );
            let recipe = EditRecipe {
                upright: Some(Upright {
                    version: 1,
                    enabled: true,
                    analysis: Some(upright_analysis(suggestion, "blake3:test")),
                }),
                ..Default::default()
            };
            let effective = recipe.effective_perspective().unwrap();
            let mut corrected = frame.clone();
            apply_perspective_only(&mut corrected, &effective);
            let residual = analyze_upright(&corrected);
            assert!(
                residual.rotation.abs() < suggestion.rotation.abs() * 0.6,
                "suggested rotation must reduce the residual tilt \
                 (angle {angle_deg}, suggested {}, residual {})",
                suggestion.rotation,
                residual.rotation
            );
        }
    }

    #[test]
    fn fingerprint_is_deterministic_and_source_sensitive() {
        let a = upright_input_fingerprint("blake3:src", 100, 200, 1);
        let b = upright_input_fingerprint("blake3:src", 100, 200, 1);
        assert_eq!(a, b);
        assert_ne!(a, upright_input_fingerprint("blake3:other", 100, 200, 1));
        assert_ne!(a, upright_input_fingerprint("blake3:src", 101, 200, 1));
        assert_ne!(a, upright_input_fingerprint("blake3:src", 100, 201, 1));
        assert_ne!(a, upright_input_fingerprint("blake3:src", 100, 200, 6));
    }

    #[test]
    fn persisted_analysis_wraps_suggestion_identity() {
        let suggestion = UprightSuggestion {
            vertical: 0.1,
            horizontal: -0.2,
            rotation: 0.3,
            line_count: 42,
            confidence: 0.5,
        };
        let analysis = upright_analysis(suggestion, "blake3:fp");
        assert_eq!(analysis.fingerprint.algorithm, UPRIGHT_ALGORITHM);
        assert_eq!(analysis.fingerprint.version, UPRIGHT_ALGORITHM_VERSION);
        assert_eq!(analysis.fingerprint.input_fingerprint, "blake3:fp");
        assert_eq!(analysis.line_count, 42);
        assert_eq!(analysis.confidence, 0.5);
    }

    /// The acceptance contract: a recipe with an enabled upright stage renders
    /// byte-identically to a recipe with the same manual `Perspective` values,
    /// and the manual values return unchanged when upright is disabled.
    #[test]
    fn upright_recipe_renders_byte_identical_to_manual_perspective() {
        use crate::render::{render_frame, RenderContext};

        let frame = grid_frame(160, 120, 6.0f32.to_radians());
        let suggestion = analyze_upright(&frame);
        let analysis = upright_analysis(suggestion, "blake3:fp");

        let upright_recipe = EditRecipe {
            upright: Some(Upright {
                version: 1,
                enabled: true,
                analysis: Some(analysis.clone()),
            }),
            perspective: Some(lumina_sidecar::Perspective {
                version: 1,
                vertical: 0.9,
                horizontal: 0.9,
                rotation: 0.9,
                scale: 2.0,
                aspect_ratio: 1.0,
                shift_x: 0.0,
                shift_y: 0.0,
            }),
            ..Default::default()
        };
        let manual_recipe = EditRecipe {
            perspective: Some(
                upright_recipe
                    .effective_perspective()
                    .expect("upright supplies the effective perspective"),
            ),
            ..Default::default()
        };

        let render = |recipe: &EditRecipe| {
            render_frame(
                &frame,
                &RenderContext {
                    recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    depth: None,
                    lensfun: None,
                },
            )
            .unwrap()
        };
        let from_upright = render(&upright_recipe);
        let from_manual = render(&manual_recipe);
        assert_eq!(from_upright.frame.width, from_manual.frame.width);
        assert_eq!(from_upright.frame.height, from_manual.frame.height);
        assert_eq!(
            from_upright.frame.pixels, from_manual.frame.pixels,
            "upright and the equivalent manual perspective must be byte-identical"
        );

        // The manual perspective is authoritative again when upright is off.
        let mut disabled = upright_recipe.clone();
        disabled.upright.as_mut().unwrap().enabled = false;
        assert_eq!(
            disabled.effective_perspective().unwrap().vertical,
            0.9,
            "disabled upright must restore the persisted manual perspective"
        );
    }
}
