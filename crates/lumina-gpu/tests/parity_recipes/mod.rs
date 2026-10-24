use super::*;

/// The implemented stages, each represented by a non-neutral recipe.
pub(super) fn supported_recipes() -> Vec<(&'static str, EditRecipe)> {
    vec![
        (
            "curves_s_curve",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.25, 0.18), (0.75, 0.85), (1.0, 1.0)]),
                    channels: CurveChannels {
                        red: Some(curve(&[(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)])),
                        green: None,
                        blue: Some(curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)])),
                    },
                }),
                ..Default::default()
            },
        ),
        (
            "hsl_all_channels",
            EditRecipe {
                hsl: Some(HslAdjustments {
                    version: 1,
                    red: Some(hsl(0.2, 0.1, 0.05)),
                    orange: Some(hsl(-0.1, -0.2, 0.0)),
                    yellow: Some(hsl(0.0, 0.3, -0.1)),
                    green: Some(hsl(0.15, 0.0, 0.1)),
                    cyan: Some(hsl(-0.3, 0.2, 0.0)),
                    blue: Some(hsl(0.25, -0.15, 0.05)),
                    violet: Some(hsl(0.0, 0.1, -0.05)),
                    magenta: Some(hsl(-0.2, 0.25, 0.0)),
                }),
                ..Default::default()
            },
        ),
        (
            "point_color_two_entries",
            EditRecipe {
                point_color: Some(PointColor {
                    version: 1,
                    entries: vec![
                        entry("pc-1", 30.0, 40.0, 0.5, -0.3, 0.1),
                        entry("pc-2", 210.0, 90.0, -0.4, 0.2, -0.2),
                    ],
                }),
                ..Default::default()
            },
        ),
        (
            "vibrance_saturation",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("vibrance".into(), 0.45),
                    ("saturation".into(), -0.25),
                ]),
                ..Default::default()
            },
        ),
        (
            "vibrance_present_zero",
            EditRecipe {
                adjustments: BTreeMap::from([("vibrance".into(), 0.0), ("saturation".into(), 0.0)]),
                ..Default::default()
            },
        ),
        (
            "color_grading_full",
            EditRecipe {
                color_grading: Some(ColorGrading {
                    version: 1,
                    shadows: range(220.0, 0.4, -0.1),
                    midtones: range(40.0, 0.2, 0.05),
                    highlights: range(120.0, 0.3, 0.1),
                    balance: 0.2,
                    blending: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_texture",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.7,
                    clarity: 0.0,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_clarity",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.4,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_dehaze_positive",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.0,
                    dehaze: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_dehaze_negative",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.0,
                    dehaze: -0.4,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_full",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.5,
                    clarity: 0.3,
                    dehaze: 0.5,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_texture_clarity",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.4,
                    clarity: 0.2,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_texture_clarity_curves",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.4,
                    clarity: 0.2,
                    dehaze: 0.0,
                }),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..Default::default()
            },
        ),
        (
            "tone_curves",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("exposure".into(), 0.35),
                    ("contrast".into(), 0.15),
                    ("wb_temperature".into(), 5800.0),
                ]),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..Default::default()
            },
        ),
        (
            "combined_tone_color_presence",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("exposure".into(), 0.35),
                    ("contrast".into(), 0.15),
                    ("wb_temperature".into(), 5800.0),
                    ("vibrance".into(), 0.2),
                ]),
                presence: Some(Presence {
                    version: 1,
                    texture: 0.4,
                    clarity: 0.2,
                    dehaze: 0.0,
                }),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                hsl: Some(HslAdjustments {
                    version: 1,
                    blue: Some(hsl(0.2, 0.1, 0.0)),
                    ..Default::default()
                }),
                point_color: Some(PointColor {
                    version: 1,
                    entries: vec![entry("pc-1", 120.0, 60.0, 0.3, -0.1, 0.0)],
                }),
                color_grading: Some(ColorGrading {
                    version: 1,
                    shadows: range(200.0, 0.2, 0.0),
                    midtones: ColorGradingRange::neutral(),
                    highlights: range(60.0, 0.2, 0.05),
                    balance: 0.0,
                    blending: 0.5,
                }),
                ..Default::default()
            },
        ),
        // --- GPU-RENDER-PARITY-1 stage 2: detail stages ---
        (
            "effects_vignette",
            EditRecipe {
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: -0.5,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                ..Default::default()
            },
        ),
        (
            "effects_grain",
            EditRecipe {
                effects: Some(Effects {
                    vignette: None,
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.5,
                        size: 0.3,
                        roughness: 0.5,
                        seed: 0x0BAD_F00D_1234,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            "effects_vignette_grain",
            EditRecipe {
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.4,
                        midpoint: 0.4,
                        roundness: 0.5,
                        feather: 0.7,
                    }),
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.35,
                        size: 0.6,
                        roughness: 0.8,
                        seed: 7,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            "noise_reduction_luminance",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.6,
                    color: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "noise_reduction_color",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.0,
                    color: 0.7,
                }),
                ..Default::default()
            },
        ),
        (
            "noise_reduction_full",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.5,
                    color: 0.5,
                }),
                ..Default::default()
            },
        ),
        (
            "sharpening_unmasked",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.2,
                    radius: 1.0,
                    detail: 0.5,
                    masking: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "sharpening_masked",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.5,
                    radius: 1.5,
                    detail: 0.7,
                    masking: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            "detail_stage_stack",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.2)]),
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.4,
                    color: 0.3,
                }),
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 1.2,
                    detail: 0.6,
                    masking: 0.4,
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.3,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.25,
                        size: 0.4,
                        roughness: 0.5,
                        seed: 42,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            // GPU-RENDER-PARITY-1 stage 3 (G-14): a red-eye correction rendered
            // by the dedicated pass (after Sharpening, before Effects).
            "red_eye_single_region",
            red_eye_recipe(),
        ),
        (
            // Red-eye stacked after Sharpening and before Effects — the exact
            // oracle slot.
            "red_eye_after_detail",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 1.2,
                    detail: 0.5,
                    masking: 0.3,
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.2,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                ..red_eye_recipe()
            },
        ),
        (
            // GPU-RENDER-PARITY-1 stage-2 follow-up: the schema maximum radius
            // (10.0) with an active edge mask (`masking > 0`) exercises the
            // full 91-tap kernel and the readback gradient pass.
            "sharpening_radius_max_masked",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 10.0,
                    detail: 0.5,
                    masking: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            // GPU-RENDER-PARITY-1 follow-up end-state probe: every GPU-eligible
            // stage class at once — tone + curves/HSL/Point Color/Color Grading
            // + Presence + Noise Reduction/Sharpening + Red-Eye + Effects +
            // legacy spot-heal geometry. The gate must stay empty; the stacked
            // pixel parity is gated below.
            "fully_stacked_eligible",
            fully_stacked_eligible_recipe(),
        ),
        // --- GPU-RENDER-PARITY-1 geometry wave: lens/perspective/crop ---
        // (A lens/perspective correction **without** an explicit crop is
        // CPU-routed by GPU-MAXRECT-WELLE; see `default_content_crop_recipes`.)
        (
            "geometry_crop_free",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.15,
                        width: 0.7,
                        height: 0.6,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_crop_aspect",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Aspect {
                        preset: AspectPreset::OneToOne,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        // LRPAR-G06-UPRIGHT-15: an enabled upright analysis resolves to the
        // effective F-099 perspective (with an explicit crop it is
        // GPU-eligible). The GPU must render the same homography as the CPU
        // oracle, so this recipe is a plain perspective-parity case.
        (
            "upright_perspective_cropped",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.1,
                        width: 0.8,
                        height: 0.8,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                upright: Some(Upright {
                    version: 1,
                    enabled: true,
                    analysis: Some(UprightAnalysis {
                        fingerprint: AnalysisFingerprint {
                            algorithm: "upright-lines-v1".into(),
                            version: "1".into(),
                            input_fingerprint: "blake3:parity".into(),
                            extras: Default::default(),
                        },
                        vertical: 0.15,
                        horizontal: -0.1,
                        rotation: 0.05,
                        line_count: 2048,
                        confidence: 0.6,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_rotate_90",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 90.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_rotate_arbitrary",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 17.5,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_mirror_both",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.05,
                        y: 0.1,
                        width: 0.8,
                        height: 0.75,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: true,
                    mirror_vertical: true,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_full_stack",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.2)]),
                lens_correction: Some(LensCorrection {
                    version: 1,
                    profile: None,
                    distortion_k1: Some(0.08),
                    distortion_k2: None,
                    distortion_k3: None,
                    vignette_c0: Some(0.95),
                    vignette_c1: None,
                    vignette_c2: None,
                    ca_red: Some(0.004),
                    ca_blue: Some(-0.004),
                }),
                perspective: Some(Perspective {
                    version: 1,
                    vertical: 0.15,
                    horizontal: 0.0,
                    rotation: 0.0,
                    scale: 1.0,
                    aspect_ratio: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0,
                }),
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.1,
                        width: 0.7,
                        height: 0.7,
                    }),
                    rotation_degrees: 8.0,
                    mirror_horizontal: true,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            // Geometry must run *after* the post-tone adjustment chain (the
            // oracle order is adjustments → lens/perspective/crop). This recipe
            // exercises the `needs_post && geometry` wiring in both render
            // paths with a non-neutral curve + effects stack.
            "geometry_after_post_stages",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.15)]),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.42), (1.0, 1.0)]),
                    channels: CurveChannels {
                        red: Some(curve(&[(0.0, 0.0), (0.5, 0.55), (1.0, 1.0)])),
                        green: None,
                        blue: None,
                    },
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.3,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.15,
                        y: 0.1,
                        width: 0.6,
                        height: 0.7,
                    }),
                    rotation_degrees: 11.0,
                    mirror_horizontal: true,
                    mirror_vertical: true,
                }),
                ..Default::default()
            },
        ),
        // --- GPU-RENDER-PARITY-1 lens-blur wave: G-05 depth bokeh ---
        (
            "lens_blur_round",
            lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round),
        ),
        (
            "lens_blur_elliptical",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Elliptical),
        ),
        (
            "lens_blur_hexagonal",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Hexagonal),
        ),
        (
            // A far focus band: the near/far ramps exercise both sides of
            // `focal_weight` (depth 0 and depth 1 both blur).
            "lens_blur_focus_far",
            lens_blur_recipe(0.5, 0.3, 0.6, BokehShape::Round),
        ),
        (
            // Schema-maximum radius (16): the widest tap set the oracle runs.
            "lens_blur_max_radius",
            lens_blur_recipe(1.0, 0.0, 0.05, BokehShape::Round),
        ),
        (
            // Identity short circuit (disabled): the gate must stay empty and
            // the pixels must be byte-identical to the source.
            "lens_blur_disabled",
            EditRecipe {
                lens_blur: Some(LensBlur {
                    enabled: false,
                    ..lens_blur_spec(0.5, 0.0, 0.05, BokehShape::Round, None)
                }),
                ..Default::default()
            },
        ),
        (
            // Lens blur runs *after* the post-tone color chain (oracle order).
            "lens_blur_after_post",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round)
            },
        ),
        (
            // Lens blur is a sub-stage of Crop: it runs after the geometry
            // chain, so the output dimensions are the cropped ones and the
            // blur's focus rect is normalized to that canvas.
            "lens_blur_after_geometry",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.1,
                        width: 0.7,
                        height: 0.7,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round)
            },
        ),
    ]
}
