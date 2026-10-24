use super::*;
use std::collections::BTreeMap;
#[test]
fn key_changes_for_every_render_input() {
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
        ..Default::default()
    };
    let key = RenderKey::new(
        "source",
        "decode-1",
        "pipeline-1",
        "vc",
        &recipe,
        vec!["mask".into()],
        OutputSpec {
            profile: "sRGB".into(),
            width: 10,
            height: 20,
            format: "png".into(),
        },
    );
    let mut changed = key.clone();
    changed.output.width += 1;
    assert_ne!(key.digest(), changed.digest());
    changed = key.clone();
    changed.mask_artifact_hashes.push("other".into());
    assert_ne!(key.digest(), changed.digest());
    assert_ne!(
        key.recipe_hash,
        RenderKey::new(
            "source",
            "decode-1",
            "pipeline-1",
            "vc",
            &Default::default(),
            vec!["mask".into()],
            OutputSpec {
                profile: "sRGB".into(),
                width: 10,
                height: 20,
                format: "png".into(),
            },
        )
        .recipe_hash
    );
}
mod drift_order_tests;
mod local_state_tests;
#[test]
fn pipeline_order_and_formats_are_explicit() {
    let pipeline = Pipeline::default();
    let stages = pipeline.stages();
    assert_eq!(
        stages
            .iter()
            .map(|(stage, _, _)| *stage)
            .collect::<Vec<_>>(),
        vec![
            PipelineStage::Decode,
            PipelineStage::SourceActions,
            PipelineStage::SpotHeal,
            PipelineStage::AutoAnalysis,
            PipelineStage::Adjustments,
            PipelineStage::Crop,
            PipelineStage::Masks,
            PipelineStage::Output,
        ]
    );
    assert!(pipeline.validate());
}

#[test]
fn downstream_recipe_options_do_not_change_decode_or_mask_digest() {
    let mut first_recipe = EditRecipe::default();
    first_recipe
        .options
        .insert("crop".into(), "original".into());
    first_recipe
        .options
        .insert("output".into(), "100x100".into());
    let mut second_recipe = first_recipe.clone();
    second_recipe.options.insert("crop".into(), "square".into());
    second_recipe
        .options
        .insert("output".into(), "200x200".into());
    let first = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &first_recipe,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 100,
            height: 100,
            format: "png".into(),
        },
    );
    let second = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &second_recipe,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 200,
            height: 200,
            format: "png".into(),
        },
    );
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Decode),
        second.stage_digest(crate::cache::CacheStage::Decode)
    );
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Mask),
        second.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(first.digest(), second.digest());
}
#[test]
fn geometry_changes_render_but_not_source_sized_mask_digest() {
    let base = EditRecipe::default();
    let mut cropped = base.clone();
    cropped.geometry = Some(lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Aspect {
            preset: lumina_sidecar::AspectPreset::OneToOne,
        }),
        rotation_degrees: 90.0,
        mirror_horizontal: true,
        mirror_vertical: false,
    });
    let first = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &base,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 10,
            height: 20,
            format: "png".into(),
        },
    );
    let second = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &cropped,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 20,
            height: 10,
            format: "png".into(),
        },
    );
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Decode),
        second.stage_digest(crate::cache::CacheStage::Decode)
    );
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Mask),
        second.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(first.digest(), second.digest());
}
#[test]
fn sharpening_render_scale_changes_render_only() {
    let recipe = EditRecipe {
        sharpening: Some(lumina_sidecar::Sharpening {
            version: 1,
            amount: 1.0,
            radius: 2.0,
            detail: 0.5,
            masking: 0.0,
        }),
        ..Default::default()
    };
    let small = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &recipe,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 100,
            height: 100,
            format: "png".into(),
        },
    );
    let large = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &recipe,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 200,
            height: 200,
            format: "png".into(),
        },
    );
    assert_eq!(
        small.stage_digest(crate::cache::CacheStage::Decode),
        large.stage_digest(crate::cache::CacheStage::Decode)
    );
    assert_eq!(
        small.stage_digest(crate::cache::CacheStage::Mask),
        large.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(
        small.stage_digest(crate::cache::CacheStage::Preview),
        large.stage_digest(crate::cache::CacheStage::Preview)
    );
}
#[test]
fn lens_and_perspective_change_render_not_decode_or_mask_digest() {
    let base = EditRecipe::default();
    let mut changed = base.clone();
    changed.lens_correction = Some(lumina_sidecar::LensCorrection {
        version: 1,
        profile: Some("wide-light".into()),
        distortion_k1: Some(0.0),
        distortion_k2: Some(0.0),
        distortion_k3: Some(0.0),
        vignette_c0: Some(1.0),
        vignette_c1: Some(0.0),
        vignette_c2: Some(0.0),
        ca_red: Some(0.0),
        ca_blue: Some(0.0),
    });
    changed.perspective = Some(lumina_sidecar::Perspective {
        version: 1,
        vertical: 0.2,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    });
    let a = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &base,
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 10,
            height: 10,
            format: "png".into(),
        },
    );
    let b = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &changed,
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 10,
            height: 10,
            format: "png".into(),
        },
    );
    assert_eq!(
        a.stage_digest(crate::cache::CacheStage::Decode),
        b.stage_digest(crate::cache::CacheStage::Decode)
    );
    assert_eq!(
        a.stage_digest(crate::cache::CacheStage::Mask),
        b.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(a.digest(), b.digest());
}
fn base_key() -> RenderKey {
    RenderKey::new(
        "source",
        "decode-1",
        "pipeline-1",
        "vc",
        &EditRecipe::default(),
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 100,
            height: 100,
            format: "png".into(),
        },
    )
}

// REVIEW-CORE-EXPORTKEY-1
#[test]
fn export_options_change_render_but_not_upstream_stage_digests() {
    let options = crate::ExportOptions::default();
    let key = base_key().with_export_options(options);
    assert_ne!(key.digest(), base_key().digest());

    // Every individual export parameter must be distinguishable.
    let mut changed = options;
    changed.quality = 42;
    assert_ne!(
        key.digest(),
        base_key().with_export_options(changed).digest()
    );
    changed = options;
    changed.dither = !options.dither;
    assert_ne!(
        key.digest(),
        base_key().with_export_options(changed).digest()
    );
    changed = options;
    changed.seed = options.seed + 1;
    assert_ne!(
        key.digest(),
        base_key().with_export_options(changed).digest()
    );
    changed = options;
    changed.format = crate::ImageFileFormat::Jpeg;
    assert_ne!(
        key.digest(),
        base_key().with_export_options(changed).digest()
    );

    // Encoder parameters cannot affect decode, matte or histogram stages.
    assert_eq!(
        key.stage_digest(crate::cache::CacheStage::Decode),
        base_key().stage_digest(crate::cache::CacheStage::Decode)
    );
    assert_eq!(
        key.stage_digest(crate::cache::CacheStage::Mask),
        base_key().stage_digest(crate::cache::CacheStage::Mask)
    );
    // "No options attached" differs from every attached option set.
    assert_eq!(base_key().export_options, None);
}

// REVIEW-CORE-SRCACC-1
#[test]
fn source_action_artifact_hashes_change_every_downstream_digest() {
    let key = base_key().with_source_action_hashes(["blake3:repair-a".to_owned()]);
    let mut changed = key.clone();
    changed.source_action_artifact_hashes = vec!["blake3:repair-b".to_owned()];
    assert_ne!(key.digest(), changed.digest());
    assert_ne!(
        key.stage_digest(crate::cache::CacheStage::Preview),
        changed.stage_digest(crate::cache::CacheStage::Preview)
    );
    assert_ne!(
        key.stage_digest(crate::cache::CacheStage::Histogram),
        changed.stage_digest(crate::cache::CacheStage::Histogram)
    );
    // The decode stage is upstream of source actions and stays shareable.
    assert_eq!(
        key.stage_digest(crate::cache::CacheStage::Decode),
        changed.stage_digest(crate::cache::CacheStage::Decode)
    );
}

// PERF-GUI-1: the base-stage digest must be recipe-blind (any slider drag
// reuses the cached demosaiced base) while separating every input that
// changes the base pixels themselves.
#[test]
fn adjustment_only_changes_reuse_the_base_stage_digest() {
    let plain = RenderKey::new(
        "s",
        "d",
        "p",
        "vc",
        &EditRecipe::default(),
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 100,
            height: 80,
            format: "rgba8".into(),
        },
    );
    let mut exposure_drag = EditRecipe::default();
    exposure_drag.adjustments.insert("exposure".into(), 1.25);
    exposure_drag.adjustments.insert("contrast".into(), -0.4);
    let dragged = RenderKey::new(
        "s",
        "d",
        "p",
        "vc",
        &exposure_drag,
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 100,
            height: 80,
            format: "rgba8".into(),
        },
    )
    .with_base_roi(Some([2, 3, 50, 40]));
    let plain_with_same_roi = plain.clone().with_base_roi(Some([2, 3, 50, 40]));
    assert_eq!(
        plain_with_same_roi.stage_digest(crate::cache::CacheStage::Base),
        dragged.stage_digest(crate::cache::CacheStage::Base),
        "a pure exposure/color change must keep the base stage identity"
    );
    assert_ne!(
        plain_with_same_roi.digest(),
        dragged.digest(),
        "the final render digest still separates the recipes"
    );

    let other_roi = plain.with_base_roi(Some([3, 2, 50, 40]));
    assert_ne!(
        plain_with_same_roi.stage_digest(crate::cache::CacheStage::Base),
        other_roi.stage_digest(crate::cache::CacheStage::Base),
        "same window size at a different offset is a different base"
    );
}
#[test]
fn base_stage_digest_separates_source_actions_geometry_and_identity() {
    let key = RenderKey::new(
        "s",
        "d",
        "p",
        "vc",
        &EditRecipe::default(),
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 64,
            height: 32,
            format: "rgba8".into(),
        },
    )
    .with_base_roi(Some([0, 0, 32, 32]));
    // Changed repair artifacts composite into the base -> new identity.
    assert_ne!(
        key.stage_digest(crate::cache::CacheStage::Base),
        key.clone()
            .with_source_action_hashes(["blake3:repair-a".to_owned()])
            .stage_digest(crate::cache::CacheStage::Base)
    );
    // A different source/decoder/pipeline/copy invalidates the base.
    for mutate in [
        |k: &mut RenderKey| k.source_content_hash = "other".into(),
        |k: &mut RenderKey| k.decode_version = "other".into(),
        |k: &mut RenderKey| k.pipeline_version = "other".into(),
        |k: &mut RenderKey| k.virtual_copy_id = "other".into(),
    ] {
        let mut changed = key.clone();
        mutate(&mut changed);
        assert_ne!(
            key.stage_digest(crate::cache::CacheStage::Base),
            changed.stage_digest(crate::cache::CacheStage::Base)
        );
    }
    // Frame geometry of the cropped base participates ...
    let mut resized = key.clone();
    resized.output.width = 48;
    assert_ne!(
        key.stage_digest(crate::cache::CacheStage::Base),
        resized.stage_digest(crate::cache::CacheStage::Base)
    );
    // ... while output profile/format/export options stay render-only.
    let mut rec2020 = key.clone();
    rec2020.output.profile = "Rec2020".into();
    rec2020.output.format = "png".into();
    assert_eq!(
        key.stage_digest(crate::cache::CacheStage::Base),
        rec2020.stage_digest(crate::cache::CacheStage::Base)
    );
    assert_ne!(key.digest(), rec2020.digest());
    let with_export = key
        .clone()
        .with_export_options(crate::ExportOptions::default());
    assert_eq!(
        key.stage_digest(crate::cache::CacheStage::Base),
        with_export.stage_digest(crate::cache::CacheStage::Base)
    );
    // `None` ROI is reproducible and distinct from every attached ROI.
    assert_eq!(
        key.stage_digest(crate::cache::CacheStage::Base),
        key.clone().stage_digest(crate::cache::CacheStage::Base)
    );
}

// REVIEW-CORE-N1
#[test]
fn histogram_digest_distinguishes_output_geometry() {
    let small = RenderKey::new(
        "s",
        "d",
        "p",
        "vc",
        &EditRecipe::default(),
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 100,
            height: 100,
            format: "rgba8".into(),
        },
    );
    let large = RenderKey::new(
        "s",
        "d",
        "p",
        "vc",
        &EditRecipe::default(),
        vec![],
        OutputSpec {
            profile: "sRGB".into(),
            width: 200,
            height: 200,
            format: "rgba8".into(),
        },
    );
    assert_ne!(
        small.stage_digest(crate::cache::CacheStage::Histogram),
        large.stage_digest(crate::cache::CacheStage::Histogram)
    );
    // Profile/format stay render-only: they cannot change luminance bins.
    let rec2020 = RenderKey {
        output: OutputSpec {
            profile: "Rec2020".into(),
            ..small.output.clone()
        },
        ..small.clone()
    };
    assert_eq!(
        small.stage_digest(crate::cache::CacheStage::Histogram),
        rec2020.stage_digest(crate::cache::CacheStage::Histogram)
    );
    assert_ne!(small.digest(), rec2020.digest());
}

// GEN-RENDERKEY-LINK-1: the generative `artifact` link is part of the
// serialized recipe bytes, so swapping it must invalidate render + mask
// while leaving decode shareable (decode is upstream of GenerativeEdit).
#[test]
fn generative_artifact_link_changes_render_and_mask_not_decode() {
    fn link(checksum: &str) -> lumina_sidecar::GenerativeArtifactRef {
        lumina_sidecar::GenerativeArtifactRef {
            id: "gen-1".into(),
            relative_path: "IMG_0001.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: checksum.into(),
            width: 64,
            height: 64,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Default::default(),
        }
    }
    fn recipe_with_link(checksum: Option<&str>) -> EditRecipe {
        EditRecipe {
            generative_edit: Some(lumina_sidecar::GenerativeEdit {
                version: 1,
                canvas: None,
                artifact: checksum.map(link),
                keep_generative_content: None,
                auto_fill_transparent: None,
                expand_beyond_image: None,
                seed: None,
                prompt: None,
                extras: Default::default(),
            }),
            ..Default::default()
        }
    }
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 10,
        height: 10,
        format: "png".into(),
    };
    let missing = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_link(None),
        vec![],
        output.clone(),
    );
    let first = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_link(Some("blake3:aaa")),
        vec![],
        output.clone(),
    );
    let second = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_link(Some("blake3:bbb")),
        vec![],
        output.clone(),
    );
    // Attaching a link (missing -> available) changes the identity.
    assert_ne!(missing.recipe_hash, first.recipe_hash);
    assert_ne!(missing.digest(), first.digest());
    // Swapping the linked artifact (same recipe, new checksum) changes it.
    assert_ne!(first.recipe_hash, second.recipe_hash);
    assert_ne!(first.digest(), second.digest());
    assert_ne!(
        first.stage_digest(crate::cache::CacheStage::Mask),
        second.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(
        first.stage_digest(crate::cache::CacheStage::Preview),
        second.stage_digest(crate::cache::CacheStage::Preview)
    );
    // Decode stays shareable: generative work happens downstream.
    assert_eq!(
        missing.stage_digest(crate::cache::CacheStage::Decode),
        first.stage_digest(crate::cache::CacheStage::Decode)
    );
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Decode),
        second.stage_digest(crate::cache::CacheStage::Decode)
    );
}

// GEN-RENDERKEY-LINK-1: same guarantee for the typed schema-v2
// `spot_removals` (a generative spot's `artifact` link and its `mode`).
#[test]
fn spot_removal_link_changes_render_and_mask_not_decode() {
    fn generative_spot(checksum: Option<&str>) -> lumina_sidecar::SpotRemoval {
        lumina_sidecar::SpotRemoval {
            id: "spot-render-key".into(),
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Generative,
            artifact: checksum.map(|checksum| lumina_sidecar::GenerativeArtifactRef {
                id: "spot-1".into(),
                relative_path: "IMG_0001.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: checksum.into(),
                width: 32,
                height: 32,
                channels: "rgba8".into(),
                data_version: "1".into(),
                extras: Default::default(),
            }),
        }
    }
    fn recipe_with_spots(spots: Vec<lumina_sidecar::SpotRemoval>) -> EditRecipe {
        EditRecipe {
            spot_removals: spots,
            ..Default::default()
        }
    }
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 10,
        height: 10,
        format: "png".into(),
    };
    let empty = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_spots(vec![]),
        vec![],
        output.clone(),
    );
    let missing = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_spots(vec![generative_spot(None)]),
        vec![],
        output.clone(),
    );
    let first = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_spots(vec![generative_spot(Some("blake3:aaa"))]),
        vec![],
        output.clone(),
    );
    let second = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_spots(vec![generative_spot(Some("blake3:bbb"))]),
        vec![],
        output.clone(),
    );
    // Adding a spot, attaching its link, and swapping the linked artifact
    // each change the render identity (no stale preview on a new artifact).
    assert_ne!(empty.recipe_hash, missing.recipe_hash);
    assert_ne!(missing.recipe_hash, first.recipe_hash);
    assert_ne!(first.recipe_hash, second.recipe_hash);
    assert_ne!(first.digest(), second.digest());
    // SpotHeal runs upstream of masks, so the mask stage invalidates too.
    assert_ne!(
        first.stage_digest(crate::cache::CacheStage::Mask),
        second.stage_digest(crate::cache::CacheStage::Mask)
    );
    // Mode is identity as well: heuristic (no bundle record) vs generative.
    let heuristic = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_spots(vec![lumina_sidecar::SpotRemoval {
            id: "spot-heuristic-key".into(),
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Heuristic,
            artifact: None,
        }]),
        vec![],
        output.clone(),
    );
    assert_ne!(heuristic.recipe_hash, missing.recipe_hash);
    assert_ne!(heuristic.digest(), missing.digest());
    // Decode stays shareable: spot healing happens downstream of decode.
    for key in [&empty, &missing, &first, &second, &heuristic] {
        assert_eq!(
            key.stage_digest(crate::cache::CacheStage::Decode),
            empty.stage_digest(crate::cache::CacheStage::Decode)
        );
    }
}

// GEN-PIPELINE-DECOUPLE: the decoupled geometry sub-stage order is
// normative (Lens → Fill → Perspective → Expand → Crop).
#[test]
fn geometry_stage_order_is_lens_fill_perspective_expand_crop() {
    assert_eq!(
        super::GEOMETRY_STAGE_ORDER,
        [
            super::GeometryStage::LensCorrection,
            super::GeometryStage::AutoFillTransparent,
            super::GeometryStage::Perspective,
            super::GeometryStage::GenerativeExpand,
            super::GeometryStage::Crop,
        ]
    );
}
fn recipe_with_generative(seed: Option<u64>, expand: Option<bool>) -> EditRecipe {
    EditRecipe {
        generative_edit: Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: expand,
            seed,
            prompt: None,
            extras: Default::default(),
        }),
        ..Default::default()
    }
}

// GEN-PIPELINE-DECOUPLE: every generative_edit field participates in the
// render identity (recipe_hash + digest).
#[test]
fn generative_edit_fields_change_render_digest() {
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 10,
        height: 10,
        format: "png".into(),
    };
    let base = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_generative(Some(1), None),
        vec![],
        output.clone(),
    );
    for other in [
        recipe_with_generative(Some(2), None),
        recipe_with_generative(Some(1), Some(true)),
        EditRecipe::default(),
    ] {
        let key = RenderKey::new("s", "d", "p", "v", &other, vec![], output.clone());
        assert_ne!(base.recipe_hash, key.recipe_hash);
        assert_ne!(base.digest(), key.digest());
    }
}

// GEN-PIPELINE-DECOUPLE: generative_edit is intentionally kept in the
// mask identity (only geometry/lens/perspective are stripped there):
// a seed change must invalidate cached mask evaluations too.
#[test]
fn generative_edit_changes_mask_digest() {
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 10,
        height: 10,
        format: "png".into(),
    };
    let a = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_generative(Some(1), None),
        vec![],
        output.clone(),
    );
    let b = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_generative(Some(2), None),
        vec![],
        output.clone(),
    );
    assert_ne!(
        a.stage_digest(crate::cache::CacheStage::Mask),
        b.stage_digest(crate::cache::CacheStage::Mask)
    );
    // Decode stays shareable: generative work happens downstream.
    assert_eq!(
        a.stage_digest(crate::cache::CacheStage::Decode),
        b.stage_digest(crate::cache::CacheStage::Decode)
    );
}

// LRPAR-G14-DENOISE-IMPL-20: `denoise_ai` (incl. strength, model,
// input-spec digest and artifact checksum) is part of the recipe and
// therefore of `recipe_hash`/render digest. Denoise sits upstream of masks
// (inside Adjustments), so it must invalidate the mask digest too — while
// decode stays shareable.
fn recipe_with_denoise(strength: f32, checksum: &str) -> EditRecipe {
    use lumina_sidecar::{
        DenoiseAi, DenoiseArtifactKind, DenoiseArtifactRef, DenoiseModelIdentity, Extras,
        DENOISE_AI_VERSION,
    };
    EditRecipe {
        denoise_ai: Some(DenoiseAi {
            version: DENOISE_AI_VERSION,
            enabled: true,
            model: DenoiseModelIdentity {
                name: "fixture-srgb".into(),
                version: "1".into(),
                model_hash:
                    "sha256:1111111111111111111111111111111111111111111111111111111111111111".into(),
                extras: Extras::new(),
            },
            input_spec_digest:
                "sha256:2222222222222222222222222222222222222222222222222222222222222222".into(),
            strength,
            preserve_detail: 0.0,
            artifact: Some(DenoiseArtifactRef {
                kind: DenoiseArtifactKind::DenoiseRgb,
                relative_path: "IMG.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: checksum.into(),
                width: 10,
                height: 10,
                channels: "rgb8".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            }),
            extras: Extras::new(),
        }),
        ..EditRecipe::default()
    }
}
#[test]
fn denoise_ai_fields_change_render_and_mask_digests_but_not_decode() {
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 10,
        height: 10,
        format: "png".into(),
    };
    let base = RenderKey::new(
        "s",
        "d",
        "p",
        "v",
        &recipe_with_denoise(0.5, "aa"),
        vec![],
        output.clone(),
    );
    for other in [
        recipe_with_denoise(0.9, "aa"),
        recipe_with_denoise(0.5, "bb"),
        EditRecipe::default(),
    ] {
        let key = RenderKey::new("s", "d", "p", "v", &other, vec![], output.clone());
        assert_ne!(base.recipe_hash, key.recipe_hash);
        assert_ne!(base.digest(), key.digest());
        assert_ne!(
            base.stage_digest(crate::cache::CacheStage::Mask),
            key.stage_digest(crate::cache::CacheStage::Mask),
            "denoise is upstream of masks"
        );
        assert_eq!(
            base.stage_digest(crate::cache::CacheStage::Decode),
            key.stage_digest(crate::cache::CacheStage::Decode),
            "denoise is downstream of decode"
        );
    }
}
