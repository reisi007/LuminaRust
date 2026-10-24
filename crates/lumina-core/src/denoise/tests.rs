use super::*;
use lumina_sidecar::{
    DenoiseAi, DenoiseArtifactKind, DenoiseArtifactRef, DenoiseModelIdentity, Extras,
    DENOISE_AI_VERSION, DENOISE_PENDING_MODEL_HASH,
};

fn sha256(fill: u8) -> String {
    format!("sha256:{}", format!("{fill:02x}").repeat(32))
}

fn artifact(width: u32, height: u32, rgb: [u8; 3]) -> DenoiseRgbArtifact {
    let pixels = rgb
        .iter()
        .copied()
        .cycle()
        .take(width as usize * height as usize * 3)
        .collect();
    DenoiseRgbArtifact::new(width, height, pixels).unwrap()
}

fn denoise(strength: f32, preserve_detail: f32) -> DenoiseAi {
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
        preserve_detail,
        artifact: Some(DenoiseArtifactRef {
            kind: DenoiseArtifactKind::DenoiseRgb,
            relative_path: "IMG.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: artifact(2, 2, [0, 0, 0]).checksum(),
            width: 2,
            height: 2,
            channels: "rgb8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }),
        extras: Extras::new(),
    }
}

fn gradient_frame(w: u32, h: u32) -> ImageFrame {
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (i * 7 % 256) as u8;
        px[1] = (i * 13 % 256) as u8;
        px[2] = (i * 29 % 256) as u8;
        px[3] = 255;
    }
    ImageFrame::new(w, h, pixels).unwrap()
}

#[test]
fn absent_and_identity_recipe_stages_are_no_ops_without_artifact() {
    let mut frame = gradient_frame(4, 4);
    let original = frame.clone();
    let input = DenoiseStageInput::inactive();
    // Absent field.
    let outcome = apply_denoise_stage(&mut frame, None, &input).unwrap();
    assert!(!outcome.applied);
    assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
    assert_eq!(frame, original);
    // enabled=false.
    let mut disabled = denoise(0.5, 0.0);
    disabled.enabled = false;
    let outcome = apply_denoise_stage(&mut frame, Some(&disabled), &input).unwrap();
    assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
    // strength=0.
    let zero = denoise(0.0, 0.5);
    let outcome = apply_denoise_stage(&mut frame, Some(&zero), &input).unwrap();
    assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
    assert_eq!(frame, original, "identity must leave pixels untouched");
}

#[test]
fn strength_zero_is_strict_identity_with_a_ready_artifact() {
    let mut frame = gradient_frame(2, 2);
    let original = frame.clone();
    let zero = denoise(0.0, 1.0);
    let ready = artifact(2, 2, [0, 0, 0]);
    let outcome =
        apply_denoise_stage(&mut frame, Some(&zero), &DenoiseStageInput::ready(&ready)).unwrap();
    assert_eq!(outcome.status, DenoiseStageStatus::Inactive);
    assert!(!outcome.applied);
    assert_eq!(frame, original, "strength:0 is byte-identical identity");
}

#[test]
fn strength_one_blend_replaces_rgb_and_preserves_alpha_and_determinism() {
    let mut frame = gradient_frame(2, 2);
    let mut expected = frame.clone();
    for px in expected.pixels.as_chunks_mut::<4>().0.iter_mut() {
        px[0] = 5;
        px[1] = 6;
        px[2] = 7;
    }
    let denoised = artifact(2, 2, [5, 6, 7]);
    let recipe = denoise(1.0, 0.0);
    let outcome = apply_denoise_stage(
        &mut frame,
        Some(&recipe),
        &DenoiseStageInput::ready(&denoised),
    )
    .unwrap();
    assert!(outcome.applied);
    assert_eq!(outcome.status, DenoiseStageStatus::Ready);
    assert_eq!(frame, expected);
    // Determinism: an identical second run is byte-identical.
    let mut again = gradient_frame(2, 2);
    apply_denoise_stage(
        &mut again,
        Some(&recipe),
        &DenoiseStageInput::ready(&denoised),
    )
    .unwrap();
    assert_eq!(frame, again);
}

#[test]
fn partial_strength_moves_pixels_towards_the_artifact_monotonically() {
    let denoised = artifact(3, 3, [0, 0, 0]);
    let mut previous: u8 = gradient_frame(3, 3).pixels[0];
    for strength in [0.25_f32, 0.5, 1.0] {
        let mut candidate = gradient_frame(3, 3);
        let recipe = denoise(strength, 0.0);
        apply_denoise_stage(
            &mut candidate,
            Some(&recipe),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        assert!(
            candidate.pixels[0] <= previous,
            "higher strength must not move away from the denoised value"
        );
        previous = candidate.pixels[0];
    }
}

#[test]
fn preserve_detail_protects_high_detail_pixels() {
    // A frame with a strong luminance edge: detail weight must reduce the
    // denoise amount on the edge relative to a flat frame.
    let mut flat = ImageFrame::new(3, 3, [100, 100, 100, 255].repeat(9)).unwrap();
    let mut edge = flat.clone();
    for (i, px) in edge.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let value = if i % 2 == 0 { 10 } else { 240 };
        px[0] = value;
        px[1] = value;
        px[2] = value;
    }
    let denoised = artifact(3, 3, [0, 0, 0]);
    let recipe = denoise(1.0, 1.0);
    apply_denoise_stage(
        &mut flat,
        Some(&recipe),
        &DenoiseStageInput::ready(&denoised),
    )
    .unwrap();
    apply_denoise_stage(
        &mut edge,
        Some(&recipe),
        &DenoiseStageInput::ready(&denoised),
    )
    .unwrap();
    // Flat center pixel is fully denoised to 0; a high-detail pixel keeps
    // more of its original value (is denoised less).
    assert_eq!(flat.pixels[4 * 4], 0);
    assert!(
        edge.pixels[4 * 4] > 0,
        "detail protection must retain source value on an edge"
    );
}

fn artifact_as_frame(artifact: &DenoiseRgbArtifact) -> ImageFrame {
    let mut pixels = Vec::with_capacity(artifact.width as usize * artifact.height as usize * 4);
    for rgb in artifact.pixels.as_chunks::<3>().0 {
        pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    ImageFrame::new(artifact.width, artifact.height, pixels).unwrap()
}

#[test]
fn psnr_gate_approaches_the_artifact_with_strength_and_is_infinite_at_zero() {
    let frame = gradient_frame(8, 6);
    let denoised = artifact(8, 6, [0, 0, 0]);
    let target = artifact_as_frame(&denoised);
    let mut previous = crate::psnr(&frame, &target);
    for strength in [0.25_f32, 0.5, 0.75, 1.0] {
        let mut candidate = frame.clone();
        let recipe = denoise(strength, 0.0);
        apply_denoise_stage(
            &mut candidate,
            Some(&recipe),
            &DenoiseStageInput::ready(&denoised),
        )
        .unwrap();
        let distance = crate::psnr(&candidate, &target);
        assert!(
            distance >= previous - 1e-6,
            "higher strength must not move away from the denoised target \
             ({distance} < {previous})"
        );
        previous = distance;
    }
    assert!(
        previous.is_infinite() || previous > 40.0,
        "full strength must reach the artifact, got {previous} dB"
    );
    // strength:0 is strict identity → infinite PSNR against the source.
    let mut identity = frame.clone();
    let zero = denoise(0.0, 0.0);
    apply_denoise_stage(
        &mut identity,
        Some(&zero),
        &DenoiseStageInput::ready(&denoised),
    )
    .unwrap();
    assert_eq!(crate::psnr(&frame, &identity), f64::INFINITY);
}

#[test]
fn dimension_mismatch_is_a_loud_corrupt_error() {
    let mut frame = gradient_frame(2, 2);
    let wrong = artifact(3, 3, [0, 0, 0]);
    let recipe = denoise(0.5, 0.0);
    let error = apply_denoise_stage(&mut frame, Some(&recipe), &DenoiseStageInput::ready(&wrong))
        .unwrap_err();
    assert!(matches!(
        error,
        CoreError::Denoise { ref status, .. } if status == "corrupt"
    ));
}

#[test]
fn active_stage_without_resolution_is_loud_under_strict() {
    let mut frame = gradient_frame(2, 2);
    let recipe = denoise(0.5, 0.0);
    let error =
        apply_denoise_stage(&mut frame, Some(&recipe), &DenoiseStageInput::inactive()).unwrap_err();
    assert!(matches!(
        error,
        CoreError::Denoise { ref status, .. } if status == "unavailable"
    ));
}

#[test]
fn warn_policy_surfaces_every_non_ready_status_without_pixel_change() {
    for status in [
        DenoiseStageStatus::Unavailable,
        DenoiseStageStatus::Stale,
        DenoiseStageStatus::Missing,
        DenoiseStageStatus::Corrupt,
    ] {
        let mut frame = gradient_frame(2, 2);
        let original = frame.clone();
        let recipe = denoise(0.5, 0.0);
        let input =
            DenoiseStageInput::non_ready(status, "test reason").with_policy(DenoisePolicy::Warn);
        let outcome = apply_denoise_stage(&mut frame, Some(&recipe), &input).unwrap();
        assert_eq!(outcome.status, status);
        assert!(!outcome.applied);
        assert_eq!(frame, original, "fallback leaves the manual NR its input");
    }
}

#[test]
fn staleness_is_classified_per_identity_field() {
    let recipe = denoise(0.5, 0.0);
    let current = DenoiseIdentity {
        source_content_hash: "src".into(),
        decode_fingerprint: "decode".into(),
        model_name: "fixture-srgb".into(),
        model_version: "1".into(),
        model_hash: sha256(0x11),
        input_spec_digest: sha256(0x22),
        artifact_checksum: recipe.artifact.as_ref().unwrap().checksum.clone(),
    };
    // The persisted producer provenance matches the live context.
    let recorded = current.clone();
    assert_eq!(
        resolve_denoise_status(&recipe, &current, &recorded, true),
        DenoiseStageStatus::Ready
    );
    // Each identity field, changed independently in the live context, is
    // stale — `model_version` is mutated too (B3).
    let mutations: [fn(&mut DenoiseIdentity); 6] = [
        |c| c.source_content_hash = "other-source".into(),
        |c| c.decode_fingerprint = "other-decode".into(),
        |c| c.model_name = "other".into(),
        |c| c.model_version = "other".into(),
        |c| c.model_hash = sha256(0x33),
        |c| c.input_spec_digest = sha256(0x44),
    ];
    for mutate in mutations {
        let mut candidate = current.clone();
        mutate(&mut candidate);
        assert_eq!(
            resolve_denoise_status(&recipe, &candidate, &recorded, true),
            DenoiseStageStatus::Stale
        );
    }
    // B2: the producer `recorded` identity is read on **every** field, so a
    // drifted persisted provenance is stale even when the live context and
    // the recipe request are unchanged.
    for mutate in mutations {
        let mut drift = recorded.clone();
        mutate(&mut drift);
        assert_eq!(
            resolve_denoise_status(&recipe, &current, &drift, true),
            DenoiseStageStatus::Stale,
            "a drifted producer provenance must be stale, never silently ready"
        );
    }
    // Missing provenance (all-default `recorded`) can never be `ready`.
    assert_eq!(
        resolve_denoise_status(&recipe, &current, &DenoiseIdentity::default(), true),
        DenoiseStageStatus::Stale
    );
    // Checksum mismatch is corrupt, not stale; missing artifact is missing.
    let mut corrupt = current.clone();
    corrupt.artifact_checksum = "00".repeat(32);
    assert_eq!(
        resolve_denoise_status(&recipe, &corrupt, &recorded, true),
        DenoiseStageStatus::Corrupt
    );
    assert_eq!(
        resolve_denoise_status(&recipe, &current, &recorded, false),
        DenoiseStageStatus::Missing
    );
    // pending-integration is unavailable even with a perfect artifact.
    let mut pending = recipe.clone();
    pending.model.model_hash = DENOISE_PENDING_MODEL_HASH.into();
    assert_eq!(
        resolve_denoise_status(&pending, &current, &recorded, true),
        DenoiseStageStatus::Unavailable
    );
}

/// B2: the producer identity has a real persistence location — the
/// flattened `DenoiseAi.extras` — and survives a JSON roundtrip losslessly.
#[test]
fn producer_provenance_roundtrips_through_extras() {
    let mut recipe = denoise(0.5, 0.0);
    let identity = DenoiseIdentity {
        source_content_hash: "blake3:src".into(),
        decode_fingerprint: "libraw:1:raw".into(),
        model_name: "fixture-srgb".into(),
        model_version: "1".into(),
        model_hash: sha256(0x11),
        input_spec_digest: sha256(0x22),
        artifact_checksum: recipe.artifact.as_ref().unwrap().checksum.clone(),
    };
    assert!(
        denoise_producer_provenance(&recipe).is_none(),
        "no provenance before it was recorded"
    );
    set_denoise_producer_provenance(&mut recipe, &identity);
    assert_eq!(
        denoise_producer_provenance(&recipe).as_ref(),
        Some(&identity)
    );
    // The flattened `extras` make the new key additive: a JSON roundtrip is
    // loss-free and needs no schema change or migration.
    let json = serde_json::to_string(&recipe).unwrap();
    assert!(json.contains(DENOISE_PRODUCER_PROVENANCE_KEY), "{json}");
    let decoded: DenoiseAi = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, recipe);
    assert_eq!(
        denoise_producer_provenance(&decoded).as_ref(),
        Some(&identity)
    );
    // A malformed persisted value reads as absent (the caller then
    // classifies `stale`), never as a silently valid identity.
    let mut garbage = recipe.clone();
    garbage.extras.insert(
        DENOISE_PRODUCER_PROVENANCE_KEY.into(),
        serde_json::Value::from("not-an-identity"),
    );
    assert!(denoise_producer_provenance(&garbage).is_none());
}

#[test]
fn checksum_is_canonical_and_matches_the_sidecar_layout() {
    let artifact = artifact(2, 2, [10, 20, 30]);
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&1u32.to_le_bytes());
    canonical.extend_from_slice(&2u32.to_le_bytes());
    canonical.extend_from_slice(&2u32.to_le_bytes());
    canonical.extend_from_slice(&artifact.pixels);
    assert_eq!(canonical, artifact.encode_raw());
    assert_eq!(
        artifact.checksum(),
        blake3::hash(&canonical).to_hex().to_string()
    );
    // Stable across calls and independent of any artifact id.
    assert_eq!(artifact.checksum(), artifact.checksum());
}

#[test]
fn tile_assembly_of_constant_tiles_is_seamless_and_deterministic() {
    // Two overlapping horizontal tiles, same constant colour: the output
    // must be exactly that constant with no seam line.
    let tile_a = [40u8, 50, 60].repeat(4 * 2); // 4 wide x 2 high
    let tile_b = [40u8, 50, 60].repeat(4 * 2);
    let tiles = [
        DenoiseTile {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
            pixels: &tile_a,
        },
        DenoiseTile {
            x: 2,
            y: 0,
            width: 4,
            height: 2,
            pixels: &tile_b,
        },
    ];
    let assembled = assemble_denoise_tiles(6, 2, &tiles).unwrap();
    assert_eq!(assembled.width, 6);
    assert_eq!(assembled.height, 2);
    assert!(
        assembled
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .all(|p| *p == [40, 50, 60]),
        "constant tiles must reconstruct a constant, seam-free frame"
    );
    // Determinism.
    let again = assemble_denoise_tiles(6, 2, &tiles).unwrap();
    assert_eq!(assembled, again);
}

#[test]
fn tile_assembly_crossfades_without_discontinuity() {
    // A dark tile overlapping a bright tile: the transition must be
    // monotone between the two constants (no seam step beyond the range).
    let dark = [0u8; 3].repeat(4);
    let bright = [255u8; 3].repeat(4);
    let tiles = [
        DenoiseTile {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
            pixels: &dark,
        },
        DenoiseTile {
            x: 2,
            y: 0,
            width: 4,
            height: 1,
            pixels: &bright,
        },
    ];
    let assembled = assemble_denoise_tiles(6, 1, &tiles).unwrap();
    let row: Vec<u8> = assembled
        .pixels
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p[0])
        .collect();
    assert!(row.windows(2).all(|w| w[0] <= w[1]), "row must be monotone");
}

#[test]
fn uncovered_pixels_and_out_of_bounds_tiles_are_loud() {
    let tile = [0u8; 3].repeat(2);
    let missing_coverage = [DenoiseTile {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        pixels: &tile[..3],
    }];
    let error = assemble_denoise_tiles(2, 2, &missing_coverage).unwrap_err();
    assert!(matches!(
        error,
        CoreError::Denoise { ref status, .. } if status == "missing"
    ));
    let out_of_bounds = [DenoiseTile {
        x: 1,
        y: 0,
        width: 2,
        height: 1,
        pixels: &tile,
    }];
    let error = assemble_denoise_tiles(2, 2, &out_of_bounds).unwrap_err();
    assert!(matches!(
        error,
        CoreError::Denoise { ref status, .. } if status == "corrupt"
    ));
}
