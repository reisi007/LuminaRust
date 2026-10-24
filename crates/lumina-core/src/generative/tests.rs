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
        artifact: None,
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
        artifact: None,
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
        artifact: None,
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
        crate::ImageFrame::new(2, 2, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
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
        artifact: None,
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

// ---- GEN-EXPAND-CACHE-1: BFS result cache (hit / miss / identity) ----

/// Opaque 4x4 frame with distinct pixels so the BFS has real neighbours.
fn expand_frame() -> crate::ImageFrame {
    let mut pixels = Vec::with_capacity(4 * 4 * 4);
    for i in 0..16u8 {
        pixels.extend_from_slice(&[i.wrapping_mul(7), i.wrapping_mul(3), i, 255]);
    }
    crate::ImageFrame::new(4, 4, pixels).unwrap()
}

/// Transparent 3x3 frame with one opaque centre pixel.
fn auto_fill_frame() -> crate::ImageFrame {
    let mut pixels = vec![0u8; 3 * 3 * 4];
    let c = 4 * 4;
    pixels[c] = 100;
    pixels[c + 1] = 150;
    pixels[c + 2] = 200;
    pixels[c + 3] = 255;
    crate::ImageFrame::new(3, 3, pixels).unwrap()
}

fn expand_recipe(seed: u64, canvas: GenerativeCanvas) -> lumina_sidecar::EditRecipe {
    let mut recipe = lumina_sidecar::EditRecipe::default();
    recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
        version: 1,
        canvas: Some(canvas),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: Some(seed),
        prompt: None,
        extras: Default::default(),
    });
    recipe
}

#[test]
fn expand_cache_serves_second_render_without_bfs() {
    let mut cache = GenerativeCache::new(10_000_000);
    let frame = expand_frame();
    let recipe = expand_recipe(0, canvas(6, 6, 1, 1));

    let first = apply_generative_expand_cached(&mut cache, &frame, &recipe).unwrap();
    assert_eq!(cache.stats().bfs_runs, 1);
    assert_eq!(cache.stats().misses, 1);
    assert_eq!(cache.stats().hits, 0);

    let second = apply_generative_expand_cached(&mut cache, &frame, &recipe).unwrap();
    assert_eq!(
        first.pixels, second.pixels,
        "cached result is byte-identical"
    );
    assert_eq!((second.width, second.height), (6, 6));
    assert_eq!(cache.stats().bfs_runs, 1, "second render must not run BFS");
    assert_eq!(cache.stats().hits, 1);
}

#[test]
fn expand_cache_misses_on_seed_change() {
    let mut cache = GenerativeCache::new(10_000_000);
    let frame = expand_frame();
    apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(0, canvas(6, 6, 1, 1)))
        .unwrap();
    apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(7, canvas(6, 6, 1, 1)))
        .unwrap();
    assert_eq!(cache.stats().bfs_runs, 2, "a seed change is a loud miss");
    assert_eq!(cache.stats().hits, 0);
}

#[test]
fn expand_cache_misses_on_canvas_change() {
    let mut cache = GenerativeCache::new(10_000_000);
    let frame = expand_frame();
    apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(0, canvas(6, 6, 1, 1)))
        .unwrap();
    apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(0, canvas(6, 6, 0, 0)))
        .unwrap();
    assert_eq!(cache.stats().bfs_runs, 2, "a canvas change is a loud miss");
}

#[test]
fn expand_cache_misses_on_source_change() {
    let mut cache = GenerativeCache::new(10_000_000);
    let recipe = expand_recipe(0, canvas(6, 6, 1, 1));
    let mut other = expand_frame();
    other.pixels[0] ^= 0xFF;
    apply_generative_expand_cached(&mut cache, &expand_frame(), &recipe).unwrap();
    apply_generative_expand_cached(&mut cache, &other, &recipe).unwrap();
    assert_eq!(
        cache.stats().bfs_runs,
        2,
        "changed source pixels are a loud miss"
    );
}

#[test]
fn auto_fill_cache_serves_second_render_without_bfs() {
    let mut cache = GenerativeCache::new(10_000_000);
    let mut first = auto_fill_frame();
    let outcome = fill_transparent_cached(&mut cache, &mut first, 5);
    assert!(outcome.filled);
    assert!(!outcome.cache_hit);
    assert_eq!(cache.stats().bfs_runs, 1);
    assert!(!has_transparent_pixels(&first));

    let mut second = auto_fill_frame();
    let outcome = fill_transparent_cached(&mut cache, &mut second, 5);
    assert!(outcome.filled);
    assert!(outcome.cache_hit, "second render is a cache hit");
    assert_eq!(first.pixels, second.pixels);
    assert_eq!(cache.stats().bfs_runs, 1, "second render must not run BFS");
}

#[test]
fn auto_fill_cache_misses_on_seed_change_without_serving_stale() {
    let mut cache = GenerativeCache::new(10_000_000);
    let mut a = auto_fill_frame();
    fill_transparent_cached(&mut cache, &mut a, 1);
    let mut b = auto_fill_frame();
    let outcome = fill_transparent_cached(&mut cache, &mut b, 2);
    assert!(
        !outcome.cache_hit,
        "seed change never serves the stale result"
    );
    assert_eq!(cache.stats().bfs_runs, 2);
}

#[test]
fn cache_key_digest_separates_role_seed_and_canvas() {
    let frame = expand_frame();
    let identity = GenerativeIdentity::heuristic();
    let base = GenerativeCacheKey::expand(&frame, &canvas(6, 6, 1, 1), 0, &identity);
    let seed = GenerativeCacheKey::expand(&frame, &canvas(6, 6, 1, 1), 1, &identity);
    let canvas_changed = GenerativeCacheKey::expand(&frame, &canvas(6, 6, 0, 0), 0, &identity);
    let auto = GenerativeCacheKey::auto_fill(&frame, 0, &identity);
    assert_ne!(base.digest(), seed.digest());
    assert_ne!(base.digest(), canvas_changed.digest());
    assert_ne!(base.digest(), auto.digest(), "roles never alias");
    assert_eq!(base.digest(), base.clone().digest(), "stable digest");
}

// GEN-ONNX-1 BLOCKER fix: the durable identity must participate in the
// prompt, negative prompt and model hash — a change to any of them is a
// loud miss, never a silently served stale canvas.
#[test]
fn cache_key_digest_separates_prompt_negative_prompt_and_model() {
    let frame = expand_frame();
    let canvas = canvas(6, 6, 1, 1);
    let base_identity = GenerativeIdentity::new("sha256:model-a", "extend the sky");
    let base = GenerativeCacheKey::expand(&frame, &canvas, 0, &base_identity);

    let prompt_changed = GenerativeCacheKey::expand(
        &frame,
        &canvas,
        0,
        &base_identity.clone().with_prompt("extend the sea"),
    );
    assert_ne!(
        base.digest(),
        prompt_changed.digest(),
        "a prompt change MUST flip the generative identity"
    );

    let negative_changed = GenerativeCacheKey::expand(
        &frame,
        &canvas,
        0,
        &base_identity.clone().with_negative_prompt(Some("blurry")),
    );
    assert_ne!(
        base.digest(),
        negative_changed.digest(),
        "a negative-prompt change MUST flip the generative identity"
    );
    // `None` vs `Some("")` are distinct (not implicitly equal).
    let empty_negative = GenerativeCacheKey::expand(
        &frame,
        &canvas,
        0,
        &base_identity.clone().with_negative_prompt(Some("")),
    );
    assert_ne!(base.digest(), empty_negative.digest());

    let model_changed = GenerativeCacheKey::expand(
        &frame,
        &canvas,
        0,
        &GenerativeIdentity::new("sha256:model-b", "extend the sky"),
    );
    assert_ne!(
        base.digest(),
        model_changed.digest(),
        "a model-hash change MUST flip the generative identity"
    );

    // No concatenation aliasing: moving a character between fields differs.
    let a = GenerativeCacheKey::expand(&frame, &canvas, 0, &GenerativeIdentity::new("ab", "c"));
    let b = GenerativeCacheKey::expand(&frame, &canvas, 0, &GenerativeIdentity::new("a", "bc"));
    assert_ne!(a.digest(), b.digest());
}

#[test]
fn expand_cache_is_bounded_and_clearable() {
    let mut cache = GenerativeCache::new(10_000_000);
    let recipe = expand_recipe(0, canvas(6, 6, 1, 1));
    apply_generative_expand_cached(&mut cache, &expand_frame(), &recipe).unwrap();
    assert!(!cache.is_empty());
    assert!(cache.used_bytes() > 0);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.used_bytes(), 0);
}

#[test]
fn global_render_paths_reuse_the_thread_cache() {
    // Auto-fill via the shared pipeline method.
    clear_generative_cache();
    let before = generative_cache_stats().bfs_runs;
    let mut a = auto_fill_frame();
    assert!(a.apply_auto_fill_transparent(true, 9));
    let after_first = generative_cache_stats().bfs_runs;
    assert_eq!(after_first, before + 1);
    let mut b = auto_fill_frame();
    assert!(b.apply_auto_fill_transparent(true, 9));
    assert_eq!(
        generative_cache_stats().bfs_runs,
        after_first,
        "second identical auto-fill must not run BFS"
    );
    assert_eq!(a.pixels, b.pixels);

    // Expand via the public entry point.
    clear_generative_cache();
    let before = generative_cache_stats().bfs_runs;
    let recipe = expand_recipe(0, canvas(6, 6, 1, 1));
    let first = apply_generative_expand(&expand_frame(), &recipe).unwrap();
    let after_first = generative_cache_stats().bfs_runs;
    assert_eq!(after_first, before + 1);
    let second = apply_generative_expand(&expand_frame(), &recipe).unwrap();
    assert_eq!(
        generative_cache_stats().bfs_runs,
        after_first,
        "second identical expand must not run BFS"
    );
    assert_eq!(first.pixels, second.pixels);
}

// ---- GEN-ONNX-1: artefact compositing replaces the BFS render path ----

fn composite_recipe(canvas: GenerativeCanvas) -> lumina_sidecar::EditRecipe {
    let mut recipe = lumina_sidecar::EditRecipe::default();
    recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
        version: 1,
        canvas: Some(canvas),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: Some(0),
        prompt: None,
        extras: Default::default(),
    });
    recipe
}

fn solid(width: u32, height: u32, rgba: [u8; 4]) -> crate::ImageFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&rgba);
    }
    crate::ImageFrame::new(width, height, pixels).unwrap()
}

#[test]
fn composite_expand_adopts_the_artifact_bytes() {
    let recipe = composite_recipe(canvas(6, 6, 1, 1));
    let frame = solid(4, 4, [10, 20, 30, 255]);
    let artifact =
        GenerativeCanvasArtifact::new(GenerativeRole::Expand, solid(6, 6, [1, 2, 3, 255]));
    let out = composite_expand(frame, &recipe, Some(&artifact)).unwrap();
    assert_eq!(
        out.pixels, artifact.frame.pixels,
        "the composited canvas is adopted byte-for-byte (no arithmetic)"
    );
    assert_eq!((out.width, out.height), (6, 6));
}

#[test]
fn composite_expand_missing_artifact_is_loud() {
    let recipe = composite_recipe(canvas(6, 6, 1, 1));
    let err = composite_expand(solid(4, 4, [1, 2, 3, 255]), &recipe, None).unwrap_err();
    assert!(matches!(err, CoreError::InvalidAdjustment { .. }));
}

#[test]
fn composite_expand_wrong_role_is_loud() {
    let recipe = composite_recipe(canvas(6, 6, 1, 1));
    let artifact = GenerativeCanvasArtifact::new(
        GenerativeRole::AutoFillTransparent,
        solid(6, 6, [1, 2, 3, 255]),
    );
    assert!(matches!(
        composite_expand(solid(4, 4, [1, 2, 3, 255]), &recipe, Some(&artifact)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}

#[test]
fn composite_expand_wrong_dimensions_is_loud() {
    let recipe = composite_recipe(canvas(6, 6, 1, 1));
    let artifact =
        GenerativeCanvasArtifact::new(GenerativeRole::Expand, solid(5, 5, [1, 2, 3, 255]));
    assert!(matches!(
        composite_expand(solid(4, 4, [1, 2, 3, 255]), &recipe, Some(&artifact)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}

#[test]
fn composite_auto_fill_without_transparency_needs_no_artifact() {
    let frame = solid(4, 4, [10, 20, 30, 255]);
    let out = composite_auto_fill(frame.clone(), None).unwrap();
    assert_eq!(out.pixels, frame.pixels, "identity: no artifact required");
}

#[test]
fn composite_auto_fill_with_transparency_requires_matching_artifact() {
    let frame = solid(2, 2, [10, 20, 30, 0]);
    assert!(matches!(
        composite_auto_fill(frame.clone(), None),
        Err(CoreError::InvalidAdjustment { .. })
    ));
    let wrong_dims =
        GenerativeCanvasArtifact::new(GenerativeRole::AutoFillTransparent, solid(3, 3, [9; 4]));
    assert!(matches!(
        composite_auto_fill(frame.clone(), Some(&wrong_dims)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
    let good = GenerativeCanvasArtifact::new(
        GenerativeRole::AutoFillTransparent,
        solid(2, 2, [9, 8, 7, 255]),
    );
    assert_eq!(
        composite_auto_fill(frame, Some(&good)).unwrap().pixels,
        good.frame.pixels
    );
}

#[test]
fn render_never_runs_the_bfs_and_adopts_the_artifact() {
    // GEN-ONNX-1: the render path composites the caller artifact and never
    // invokes the heuristic BFS. Proven via the cache's BFS counter.
    clear_generative_cache();
    let before = generative_cache_stats().bfs_runs;
    let recipe = composite_recipe(canvas(6, 6, 1, 1));
    let frame = solid(4, 4, [10, 20, 30, 255]);
    let artifact =
        GenerativeCanvasArtifact::new(GenerativeRole::Expand, solid(6, 6, [1, 2, 3, 255]));
    let context = crate::RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let out = crate::render_frame_with_generative(
        &frame,
        &context,
        GenerativeCanvasInput {
            auto_fill: None,
            expand: Some(&artifact),
        },
    )
    .unwrap();
    assert_eq!(out.frame.pixels, artifact.frame.pixels);
    assert_eq!(
        generative_cache_stats().bfs_runs,
        before,
        "artifact compositing must not run the heuristic BFS"
    );
}
