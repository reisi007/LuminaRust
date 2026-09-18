// LRPAR-G14-DENOISE-IMPL-20: pipeline-level integration tests for the
// optional `denoise_ai` stage (identity, ordering, fallback, full render path,
// schema connection). Kernel/status unit tests live in `denoise.rs`.

use super::*;

use lumina_sidecar::{
    DenoiseAi, DenoiseArtifactKind, DenoiseArtifactRef, DenoiseModelIdentity, Extras,
    NoiseReduction, Sharpening, DENOISE_AI_VERSION,
};

fn test_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (i * 11 % 256) as u8;
        px[1] = (i * 17 % 256) as u8;
        px[2] = (i * 23 % 256) as u8;
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

fn sha256(fill: u8) -> String {
    format!("sha256:{}", format!("{fill:02x}").repeat(32))
}

fn artifact(width: u32, height: u32, value: u8) -> DenoiseRgbArtifact {
    DenoiseRgbArtifact::new(
        width,
        height,
        vec![value; width as usize * height as usize * 3],
    )
    .unwrap()
}

fn denoise_ai(strength: f32, artifact_checksum: String) -> DenoiseAi {
    DenoiseAi {
        version: DENOISE_AI_VERSION,
        enabled: true,
        model: DenoiseModelIdentity {
            name: "fixture-srgb".into(),
            version: "1".into(),
            model_hash: sha256(0x11),
            extras: Extras::new(),
        },
        input_spec_digest: sha256(0x22),
        strength,
        preserve_detail: 0.0,
        artifact: Some(DenoiseArtifactRef {
            kind: DenoiseArtifactKind::DenoiseRgb,
            relative_path: "IMG.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: artifact_checksum,
            width: 6,
            height: 5,
            channels: "rgb8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }),
        extras: Extras::new(),
    }
}

fn context<'a>(recipe: &'a EditRecipe) -> RenderContext<'a> {
    RenderContext {
        recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    }
}

#[test]
fn identity_denoise_ai_renders_byte_identically_to_the_mvp_recipe() {
    let frame = test_frame(6, 5);
    let artifact = artifact(6, 5, 0);
    let without = EditRecipe {
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.4,
            color: 0.2,
        }),
        ..EditRecipe::default()
    };
    let mut with_zero = without.clone();
    with_zero.denoise_ai = Some(denoise_ai(0.0, artifact.checksum()));

    let plain = render_frame(&frame, &context(&without)).unwrap();
    let identity = render_frame_with_denoise(
        &frame,
        &context(&with_zero),
        &DenoiseStageInput::ready(&artifact),
    )
    .unwrap();
    assert_eq!(
        plain.frame.pixels, identity.frame.pixels,
        "strength:0 denoise_ai must be byte-identical to the MVP recipe"
    );
}

#[test]
fn denoise_runs_before_manual_noise_reduction_and_sharpening() {
    let frame = test_frame(6, 5);
    let denoised = artifact(6, 5, 200);
    let recipe = EditRecipe {
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.6,
            color: 0.3,
        }),
        sharpening: Some(Sharpening {
            version: 1,
            amount: 1.0,
            radius: 1.0,
            detail: 0.5,
            masking: 0.0,
        }),
        denoise_ai: Some(denoise_ai(1.0, denoised.checksum())),
        ..EditRecipe::default()
    };

    let mut combined = frame.clone();
    combined
        .apply_recipe_with_denoise(&recipe, &DenoiseStageInput::ready(&denoised))
        .unwrap();

    // Manual composition: denoise first, then the same recipe without the
    // KI stage. Identical output proves the stage order DenoiseAI → F-096
    // → F-095.
    let mut manual = frame.clone();
    apply_denoise_blend(&mut manual, &denoised, 1.0, 0.0).unwrap();
    let mut recipe_without = recipe.clone();
    recipe_without.denoise_ai = None;
    manual.apply_recipe(&recipe_without).unwrap();
    assert_eq!(combined.pixels, manual.pixels);
}

#[test]
fn warn_fallback_equals_the_manual_nr_recipe_and_leaves_strict_loud() {
    let frame = test_frame(6, 5);
    let denoised = artifact(6, 5, 0);
    let recipe = EditRecipe {
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.7,
            color: 0.4,
        }),
        denoise_ai: Some(denoise_ai(0.8, denoised.checksum())),
        ..EditRecipe::default()
    };

    // Reference: the same recipe with the KI stage removed (manual F-096
    // only), which is the documented visible fallback anchor.
    let mut manual_recipe = recipe.clone();
    manual_recipe.denoise_ai = None;
    let manual = render_frame(&frame, &context(&manual_recipe)).unwrap();

    // Warn: missing artifact falls through to the manual F-096 result.
    let fallback = render_frame_with_denoise(
        &frame,
        &context(&recipe),
        &DenoiseStageInput::non_ready(DenoiseStageStatus::Missing, "artifact absent")
            .with_policy(DenoisePolicy::Warn),
    )
    .unwrap();
    assert_eq!(fallback.frame.pixels, manual.frame.pixels);

    // Strict: the same missing status aborts loudly.
    let error = render_frame_with_denoise(
        &frame,
        &context(&recipe),
        &DenoiseStageInput::non_ready(DenoiseStageStatus::Missing, "artifact absent"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CoreError::Denoise { ref status, .. } if status == "missing"
    ));
}

#[test]
fn schema_loaded_denoise_ai_is_applied_by_the_render_path() {
    // A recipe as it appears nested under `adjustments` in a sidecar.
    let json = r#"{
        "adjustments": {
            "denoise_ai": {
                "version": 1,
                "enabled": true,
                "model": {"name": "fixture-srgb", "version": "1", "model_hash": "sha256:1111111111111111111111111111111111111111111111111111111111111111"},
                "input_spec_digest": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
                "strength": 1.0,
                "preserve_detail": 0.0,
                "artifact": {
                    "kind": "denoise_rgb",
                    "relative_path": "IMG.lumina.zdata",
                    "format": "lumina-zdata",
                    "checksum": "placeholder",
                    "width": 6,
                    "height": 5,
                    "channels": "rgb8",
                    "data_version": "1"
                }
            }
        }
    }"#;
    let recipe: EditRecipe = serde_json::from_str(json).unwrap();
    assert!(recipe.denoise_ai.is_some(), "schema field connects");
    let denoised = artifact(6, 5, 30);
    // Applied pixels equal an explicit full-strength blend.
    let mut expected = test_frame(6, 5);
    apply_denoise_blend(&mut expected, &denoised, 1.0, 0.0).unwrap();
    let mut actual = test_frame(6, 5);
    actual
        .apply_recipe_with_denoise(&recipe, &DenoiseStageInput::ready(&denoised))
        .unwrap();
    assert_eq!(actual.pixels, expected.pixels);
}

/// F3 (Auflage aus der Kern-Verifizierung 2026-09-16): a directly
/// constructed (not sidecar-loaded) recipe reaches the CPU render path's
/// `validate_nested_adjustments`, which maps every malformed `denoise_ai`
/// deviation onto the documented `CoreError::Denoise { status: "invalid" }`
/// — loud, before any pixel is written, never a clipped/defaulted stage.
///
/// The class is checked across 11 cases (version, model identity,
/// digest, bounded strengths and the artifact fields `relative_path`,
/// `resolution`, `checksum`); `format`/`channels`/`data_version` remain
/// open (Folgearbeit, s. Entscheid §8).
#[test]
fn malformed_denoise_ai_maps_to_invalid_denoise_error_via_validation() {
    // (expected reason fragment, mutator) — mirrors the sidecar validation
    // matrix, exercised through the render entry point instead of the
    // sidecar loader.
    type Mutator = fn(&mut DenoiseAi);
    let cases: [(&str, Mutator); 11] = [
        ("unsupported denoise_ai.version", |d| d.version = 2),
        ("denoise_ai.model.name", |d| d.model.name.clear()),
        ("denoise_ai.model.version", |d| {
            d.model.version = "  ".into()
        }),
        ("denoise_ai.model.model_hash", |d| {
            d.model.model_hash = "dummy".into()
        }),
        ("denoise_ai.input_spec_digest", |d| {
            d.input_spec_digest = "not-a-digest".into()
        }),
        ("denoise_ai.strength", |d| d.strength = 1.5),
        ("denoise_ai.strength", |d| d.strength = f32::NAN),
        ("denoise_ai.preserve_detail", |d| d.preserve_detail = -0.1),
        ("denoise_ai artifact relative_path", |d| {
            d.artifact.as_mut().unwrap().relative_path = "/abs/out.bin".into()
        }),
        ("denoise_ai artifact resolution", |d| {
            d.artifact.as_mut().unwrap().width = 0
        }),
        ("denoise_ai artifact checksum", |d| {
            d.artifact.as_mut().unwrap().checksum.clear()
        }),
    ];
    let frame = test_frame(6, 5);
    for (fragment, mutate) in cases {
        let valid = denoise_ai(1.0, artifact(6, 5, 0).checksum());
        let mut malformed = valid.clone();
        mutate(&mut malformed);
        let mut recipe = EditRecipe {
            denoise_ai: Some(malformed),
            ..EditRecipe::default()
        };
        let mut candidate = frame.clone();
        let error = candidate
            .apply_recipe(&recipe)
            .expect_err("malformed denoise_ai must be rejected by validation");
        match error {
            CoreError::Denoise { status, reason } => {
                assert_eq!(
                    status, "invalid",
                    "the validation mapping must use the `invalid` status"
                );
                assert!(
                    reason.contains(fragment),
                    "reason must name the offending field `{fragment}`, got: {reason}"
                );
            }
            other => panic!("expected CoreError::Denoise for `{fragment}`, got {other:?}"),
        }
        assert_eq!(
            candidate.pixels, frame.pixels,
            "validation must reject before any pixel is written"
        );

        // Sanity: undoing the mutation through the valid recipe renders.
        recipe.denoise_ai = Some(valid);
        let mut rendered = frame.clone();
        rendered
            .apply_recipe_with_denoise(&recipe, &DenoiseStageInput::ready(&artifact(6, 5, 0)))
            .unwrap();
    }
}
