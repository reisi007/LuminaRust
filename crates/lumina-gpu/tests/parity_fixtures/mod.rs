use super::*;

/// A manual lens-correction recipe (distortion + vignette + CA) driving all
/// three sub-stages with non-neutral coefficients.
pub(super) fn manual_lens_recipe() -> EditRecipe {
    EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.12),
            distortion_k2: Some(-0.04),
            distortion_k3: None,
            vignette_c0: Some(0.9),
            vignette_c1: Some(-0.2),
            vignette_c2: None,
            ca_red: Some(0.006),
            ca_blue: Some(-0.006),
        }),
        ..Default::default()
    }
}

/// A manual lens correction whose radial map pushes the sampled source
/// coordinate *outward* (`k1 < 0`, pincushion), so the resampled frame keeps
/// transparent edges — the canonical "lens wedge" that makes the CPU oracle's
/// content-based default crop non-identity (CROP-MAXRECT-1).
fn lens_wedge_recipe() -> EditRecipe {
    EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(-0.08),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }),
        ..Default::default()
    }
}

/// GPU-MAXRECT-WELLE: recipes that activate the CPU oracle's content-based
/// default crop (a lens/perspective correction without an explicit
/// `geometry.crop`). The resulting rectangle is derived from the resampled
/// alpha channel, so the recipe-only GPU geometry plan cannot predict its
/// (possibly smaller) output dimensions. These recipes are therefore routed to
/// the CPU loudly (gate reason `geometry (default content crop)`) instead of
/// rendering an uncropped, differently-sized frame.
///
/// Note the two non-wedge lens recipes: their default crop is the identity, but
/// the recipe-only gate cannot prove that without the resampled alpha, so it
/// conservatively routes them too. `lens_wedge`/`perspective_*` make the
/// divergence real and keep this test non-vacuous.
pub(super) fn default_content_crop_recipes() -> Vec<(&'static str, EditRecipe)> {
    vec![
        ("lens_correction_manual", manual_lens_recipe()),
        (
            "lens_correction_wide_light",
            EditRecipe {
                lens_correction: Some(LensCorrection {
                    version: 1,
                    profile: Some("wide-light".into()),
                    distortion_k1: None,
                    distortion_k2: None,
                    distortion_k3: None,
                    vignette_c0: None,
                    vignette_c1: None,
                    vignette_c2: None,
                    ca_red: None,
                    ca_blue: None,
                }),
                ..Default::default()
            },
        ),
        ("lens_wedge", lens_wedge_recipe()),
        (
            "perspective_vertical",
            EditRecipe {
                perspective: Some(Perspective {
                    version: 1,
                    vertical: 0.25,
                    horizontal: 0.0,
                    rotation: 0.0,
                    scale: 1.0,
                    aspect_ratio: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "perspective_full",
            EditRecipe {
                perspective: Some(Perspective {
                    version: 1,
                    vertical: 0.2,
                    horizontal: -0.15,
                    rotation: 0.1,
                    scale: 1.15,
                    aspect_ratio: 1.1,
                    shift_x: 0.05,
                    shift_y: -0.03,
                }),
                ..Default::default()
            },
        ),
    ]
}

/// GPU-RENDER-PARITY-1 follow-up end-state probe: a single recipe that sets
/// every GPU-eligible stage class. Used to assert the routing gate is empty and
/// that the accumulated parity stays within the documented tolerance.
pub(super) fn fully_stacked_eligible_recipe() -> EditRecipe {
    let mut recipe = EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 0.3),
            ("contrast".into(), 0.1),
            ("highlights".into(), -0.2),
            ("shadows".into(), 0.15),
            ("whites".into(), 0.1),
            ("blacks".into(), -0.1),
            ("wb_temperature".into(), 5600.0),
            ("wb_tint".into(), 0.05),
            ("vibrance".into(), 0.3),
            ("saturation".into(), -0.1),
        ]),
        curves: Some(Curves {
            version: 1,
            master: curve(&[(0.0, 0.0), (0.4, 0.35), (0.7, 0.78), (1.0, 1.0)]),
            channels: CurveChannels {
                red: Some(curve(&[(0.0, 0.0), (0.5, 0.55), (1.0, 1.0)])),
                green: None,
                blue: Some(curve(&[(0.0, 0.0), (0.5, 0.48), (1.0, 1.0)])),
            },
        }),
        hsl: Some(HslAdjustments {
            version: 1,
            blue: Some(hsl(0.15, 0.1, 0.0)),
            orange: Some(hsl(-0.1, 0.05, 0.0)),
            ..Default::default()
        }),
        point_color: Some(PointColor {
            version: 1,
            entries: vec![entry("pc-1", 200.0, 70.0, 0.2, -0.1, 0.05)],
        }),
        color_grading: Some(ColorGrading {
            version: 1,
            shadows: range(210.0, 0.2, -0.05),
            midtones: ColorGradingRange::neutral(),
            highlights: range(50.0, 0.15, 0.05),
            balance: 0.1,
            blending: 0.55,
        }),
        presence: Some(Presence {
            version: 1,
            texture: 0.3,
            clarity: 0.2,
            dehaze: 0.2,
        }),
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.3,
            color: 0.2,
        }),
        sharpening: Some(Sharpening {
            version: 1,
            amount: 0.8,
            radius: 1.2,
            detail: 0.5,
            masking: 0.3,
        }),
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.45,
                y: 0.5,
                radius: 0.2,
                desaturate: 0.5,
                darken: 0.2,
            }],
        }),
        effects: Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: 0.25,
                midpoint: 0.5,
                roundness: 1.0,
                feather: 0.5,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.2,
                size: 0.4,
                roughness: 0.5,
                seed: 2026,
            }),
        }),
        ..Default::default()
    };
    recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{
            "id": "spot-1",
            "version": 1,
            "mode": "heuristic",
            "center_x": 0.4,
            "center_y": 0.45,
            "radius": 7.0,
            "feather": 0.5,
            "offset_dx": 0.25,
            "offset_dy": 0.1,
            "opacity": 0.9,
            "status": "valid"
        }]),
    );
    recipe
}

/// The strongest CPU↔GPU equivalence **measured** for each recipe on this
/// backend (MAX-frames run: gradient 64² + noise 64²).
///
/// Every recipe must declare a bound — there is no catch-all, so adding a
/// recipe forces an explicit, reviewed decision instead of silently inheriting
/// a loose tolerance. The byte-identical group is asserted at `diff == 0`; the
/// bounded groups additionally require the PSNR floor and the mean-signed-error
/// bias bound.
pub(super) fn equivalence_for(name: &str) -> Equivalence {
    match name {
        // Measured 0 on both frames.
        "point_color_two_entries"
        | "vibrance_present_zero"
        | "color_grading_full"
        | "presence_clarity"
        | "presence_dehaze_positive"
        | "presence_texture_clarity" => Equivalence::ByteIdentical,
        // Measured ≤ 1 on both frames (one rounding-tie code).
        "curves_s_curve"
        | "hsl_all_channels"
        | "vibrance_saturation"
        | "presence_texture"
        | "presence_dehaze_negative"
        | "presence_full"
        | "presence_texture_clarity_curves"
        | "tone_curves" => Equivalence::Bounded(1),
        // Stacks tone + Presence + every color stage; two ±1 codes coincide on
        // one pixel of the gradient frame.
        "combined_tone_color_presence" => Equivalence::Bounded(2),
        // --- GPU-RENDER-PARITY-1 stage 2: detail stages ---
        // Vignette and grain are `f32`-exact ports of the oracle and asserted
        // byte-identical on their own; stacking both can flip one `round()` tie
        // in the grain delta (GPU FMA vs. the oracle's separate multiply/add),
        // so the combined recipe is bounded at one code. Noise Reduction /
        // Sharpening carry the same `exp`/FMA residual.
        "effects_vignette" | "effects_grain" => Equivalence::ByteIdentical,
        "effects_vignette_grain" => Equivalence::Bounded(1),
        "noise_reduction_luminance"
        | "noise_reduction_color"
        | "noise_reduction_full"
        | "sharpening_unmasked"
        | "sharpening_masked" => Equivalence::Bounded(1),
        "detail_stage_stack" => Equivalence::Bounded(2),
        // GPU-RENDER-PARITY-1 stage 3 (G-14): the red-eye pass is a direct port
        // (per-pixel + region-local); `hypot`-vs-`sqrt` rounding at the disc
        // edge can flip a single code. The stacked recipe adds the detail
        // residual.
        "red_eye_single_region" => Equivalence::Bounded(1),
        "red_eye_after_detail" => Equivalence::Bounded(2),
        // Stage-2 follow-up: maximum-radius sharpening is the same kernel as
        // `sharpening_masked`, just wider (still one rounding-tie code).
        "sharpening_radius_max_masked" => Equivalence::Bounded(1),
        // GPU-RENDER-PARITY-1 follow-up end-state: every eligible stage class
        // stacked; the per-stage FMA/rounding residuals accumulate on the noise
        // frame. Bound set from the measured MAX frames run (see test output).
        "fully_stacked_eligible" => Equivalence::Bounded(4),
        // --- GPU-RENDER-PARITY-1 geometry wave ---
        // Crop and mirror are exact integer sub-rect copies / flips; a
        // quarter-turn rotation is an exact integer remap. Asserted
        // byte-identical. Arbitrary rotation, lens and perspective resample in
        // the same `f32` 0..=255 domain as the oracle and carry at most one
        // rounding-tie code; the full chain accumulates that per stage.
        "geometry_crop_free"
        | "geometry_crop_aspect"
        | "geometry_rotate_90"
        | "geometry_mirror_both" => Equivalence::ByteIdentical,
        "geometry_rotate_arbitrary" | "geometry_full_stack" | "geometry_after_post_stages" => {
            Equivalence::Bounded(1)
        }
        // LRPAR-G06-UPRIGHT-15: an enabled upright analysis is the same F-099
        // perspective homography (explicit crop) — one rounding-tie code max.
        "upright_perspective_cropped" => Equivalence::Bounded(1),
        // --- GPU-RENDER-PARITY-1 lens-blur wave ---
        // The convolution itself is exact integer accumulation; only the final
        // `f32` lerp differs from the oracle's `f64` (the heuristic weight math
        // matches operation-for-operation in `f32`), so each recipe carries at
        // most one rounding-tie code. `geometry` before it is an exact integer
        // sub-rect copy.
        "lens_blur_round"
        | "lens_blur_elliptical"
        | "lens_blur_hexagonal"
        | "lens_blur_focus_far"
        | "lens_blur_max_radius"
        | "lens_blur_after_post"
        | "lens_blur_after_geometry" => Equivalence::Bounded(1),
        // Disabled blur leaves the source bytes untouched.
        "lens_blur_disabled" => Equivalence::ByteIdentical,
        other => panic!("no measured equivalence bound declared for recipe `{other}`"),
    }
}

/// A Red-Eye recipe with a real, pixel-effective region on a red frame.
pub(super) fn red_eye_recipe() -> EditRecipe {
    EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.5,
                y: 0.5,
                radius: 0.3,
                desaturate: 0.8,
                darken: 0.3,
            }],
        }),
        ..Default::default()
    }
}

/// LRPAR-G14-DENOISE-IMPL-20: an **active** additive KI-Denoise stage. The
/// stage has no WGSL pass yet, so the GPU gate must CPU-route it; the CPU oracle
/// then aborts loudly (`CoreError::Denoise`) under the default `Strict` policy.
pub(super) fn denoise_ai_recipe(strength: f32) -> EditRecipe {
    EditRecipe {
        denoise_ai: Some(DenoiseAi {
            version: DENOISE_AI_VERSION,
            enabled: true,
            model: DenoiseModelIdentity {
                name: "fixture-srgb".into(),
                version: "1".into(),
                model_hash: format!("sha256:{}", "11".repeat(32)),
                extras: Default::default(),
            },
            input_spec_digest: format!("sha256:{}", "22".repeat(32)),
            strength,
            preserve_detail: 0.5,
            artifact: None,
            extras: Default::default(),
        }),
        ..Default::default()
    }
}

/// A typed `spot_removals` entry with no `extras` geometry: the CPU reference
/// rejects it loudly (`reject_unsupported_spot_modes_typed`), never heals it.
pub(super) fn typed_spot_recipe() -> EditRecipe {
    EditRecipe {
        spot_removals: vec![SpotRemoval {
            id: "spot-parity-heuristic".into(),
            version: 1,
            mode: SpotRemovalMode::Heuristic,
            artifact: None,
        }],
        ..Default::default()
    }
}
/// A legacy `extras["spot_removals"]` entry with valid, pixel-effective heal
/// geometry (`apply_spot_heals` copies an offset source patch).
pub(super) fn legacy_spot_recipe() -> EditRecipe {
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
        "spot_removals".into(),
        // Valid heuristic geometry (id + center + radius + offset), matching
        // `validate_spot_removal_extra_entry`.
        serde_json::json!([{
            "id": "spot-1",
            "version": 1,
            "mode": "heuristic",
            "center_x": 0.5,
            "center_y": 0.5,
            "radius": 8.0,
            "feather": 0.5,
            "offset_dx": 0.25,
            "offset_dy": 0.0,
            "opacity": 1.0,
            "status": "valid"
        }]),
    );
    recipe
}

/// A generative edit with a valid, larger canvas and `expand_beyond_image` on:
/// the CPU reference expands the frame to the canvas size.
pub(super) fn generative_expand_recipe() -> EditRecipe {
    EditRecipe {
        generative_edit: Some(GenerativeEdit {
            version: 1,
            canvas: Some(GenerativeCanvas {
                output_width: 128,
                output_height: 96,
                source_offset_x: 0,
                source_offset_y: 0,
                extras: BTreeMap::new(),
            }),
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(false),
            expand_beyond_image: Some(true),
            seed: Some(7),
            prompt: None,
            extras: BTreeMap::new(),
        }),
        ..Default::default()
    }
}

/// GEN-ONNX-1: a generative edit with **no active role** is the identity and
/// needs no artifact, while still being CPU-only-gated as a recipe class.
pub(super) fn generative_identity_recipe() -> EditRecipe {
    let mut recipe = generative_expand_recipe();
    if let Some(edit) = recipe.generative_edit.as_mut() {
        edit.canvas = None;
        edit.expand_beyond_image = None;
        edit.auto_fill_transparent = None;
    }
    recipe
}

/// An invalid red-eye correction (out-of-range desaturation) that the CPU
/// oracle rejects loudly: the GPU gate must keep it CPU-routed.
pub(super) fn invalid_red_eye_recipe() -> EditRecipe {
    EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.5,
                y: 0.5,
                radius: 0.3,
                desaturate: 1.5,
                darken: 0.3,
            }],
        }),
        ..Default::default()
    }
}

/// A solid red frame so the Red-Eye correction is unambiguously pixel-effective.
pub(super) fn red_frame(width: u32, height: u32) -> ImageFrame {
    ImageFrame::new(
        width,
        height,
        [200u8, 40, 40, 255].repeat((width * height) as usize),
    )
    .expect("red frame")
}
