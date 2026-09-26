//! Hard-error and shadow-tolerance contracts for generative / heuristic
//! spot removals in the render pipeline (SPOT-REMOVE-1, SPOT-TYPED-FIELD-FIX,
//! SPOT-CORE-SHADOW-FOLLOWUP): a spot without model/artifact or heal geometry
//! must fail loudly instead of rendering as absent (a silent no-heal fallback).
//!
//! Extracted from `render.rs` (file-size ratchet) alongside the MASK-LOCAL-P1.2b
//! local colour test wiring, so the ratchet baseline for `render.rs` can drop
//! instead of being raised.

use super::*;

#[test]
fn generative_spot_mode_is_hard_error_not_silent_skip() {
    // SPOT-REMOVE-1: a generative spot needs model + artifact. Rendering
    // it as healed (or as absent) would be a silent fallback.
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id":"g1","version":1,"mode":"generative","prompt":"x"}]),
    );
    let error = render_frame(&frame, &default_context(&recipe, None)).unwrap_err();
    assert!(matches!(error, CoreError::InvalidAdjustment { .. }));
}

#[test]
fn malformed_heuristic_spot_entry_is_hard_error() {
    // A corrupt heuristic entry (radius 0) must not be silently dropped.
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s1","version":1,"mode":"heuristic","center_x":0.5,"center_y":0.5,"radius":0.0,"feather":0.0,"offset_dx":0.0,"offset_dy":0.0,"opacity":1.0,"status":"valid"}]),
        );
    assert!(matches!(
        render_frame(&frame, &default_context(&recipe, None)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}

#[test]
fn expand_without_canvas_is_hard_error() {
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    assert!(matches!(
        render_frame(&frame, &default_context(&recipe, None)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}

#[test]
fn canvas_without_expand_is_hard_error() {
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
        version: 1,
        canvas: Some(lumina_sidecar::GenerativeCanvas {
            output_width: 40,
            output_height: 40,
            source_offset_x: 4,
            source_offset_y: 4,
            extras: Default::default(),
        }),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(false),
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    assert!(matches!(
        render_frame(&frame, &default_context(&recipe, None)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}

#[test]
fn unknown_spot_mode_is_hard_error() {
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id":"s9","version":1,"mode":"clone-magic"}]),
    );
    assert!(matches!(
        render_frame(&frame, &default_context(&recipe, None)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}
#[test]
fn typed_generative_spot_is_hard_error_not_silent_skip() {
    // SPOT-TYPED-FIELD-FIX: a typed generative entry (schema-v2) needs
    // model + artifact like its legacy extras counterpart — rendering it
    // as absent would be a silent fallback.
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
        id: "spot-render-generative".into(),
        version: lumina_sidecar::SPOT_REMOVAL_VERSION,
        mode: lumina_sidecar::SpotRemovalMode::Generative,
        artifact: None,
    });
    let error = render_frame(&frame, &default_context(&recipe, None)).unwrap_err();
    assert!(matches!(error, CoreError::InvalidAdjustment { .. }));
}
#[test]
fn typed_heuristic_spot_without_geometry_is_hard_error() {
    // SPOT-CORE-SHADOW-FOLLOWUP: an ISOLATED geometry-free typed
    // heuristic shadow (no `extras["spot_removals"]` key anywhere) has
    // no heal geometry to render from — rendering it as absent would be
    // a silent no-heal, so it fails loudly. Contrast with
    // `typed_heuristic_mirror_shadow_with_extras_is_tolerated`: on a
    // healthy loaded recipe the extras view carries the geometry and
    // the same shadow is skipped.
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
        id: "spot-render-heuristic".into(),
        version: lumina_sidecar::SPOT_REMOVAL_VERSION,
        mode: lumina_sidecar::SpotRemovalMode::Heuristic,
        artifact: None,
    });
    assert!(
        !recipe.extras.contains_key("spot_removals"),
        "isolated shadow fixture must carry no extras geometry"
    );
    let error = render_frame(&frame, &default_context(&recipe, None)).unwrap_err();
    assert!(matches!(error, CoreError::InvalidAdjustment { .. }));
}
#[test]
fn typed_heuristic_mirror_shadow_with_extras_is_tolerated() {
    // SPOT-CORE-SHADOW-FOLLOWUP: a healthy loaded recipe carries the
    // heal geometry in `extras["spot_removals"]` plus the geometry-free
    // typed mirror shadow (sidecar c000c6f). The shadow is skipped and
    // healing comes from extras — no false alarm, visibly healed pixels.
    let mut pixels = Vec::new();
    for _y in 0..8 {
        for x in 0..8 {
            let v = if x < 4 { 0 } else { 255 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s1","version":1,"mode":"heuristic","center_x":0.25,"center_y":0.5,"radius":2.0,"feather":0.5,"offset_dx":0.5,"offset_dy":0.0,"opacity":1.0,"status":"valid"}]),
        );
    recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
        id: "spot-render-mirror".into(),
        version: lumina_sidecar::SPOT_REMOVAL_VERSION,
        mode: lumina_sidecar::SpotRemovalMode::Heuristic,
        artifact: None,
    });
    let output = render_frame(&frame, &default_context(&recipe, None)).unwrap();
    assert_ne!(
        output.frame.pixels, frame.pixels,
        "mirror-shadow recipe must visibly heal from extras"
    );
}
#[test]
fn typed_spot_unknown_version_is_hard_error() {
    // Unknown typed spot versions are rejected, never silently migrated.
    let frame = checker_8x8();
    let mut recipe = EditRecipe::default();
    recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
        id: "spot-render-unknown".into(),
        version: 99,
        mode: lumina_sidecar::SpotRemovalMode::Heuristic,
        artifact: None,
    });
    assert!(matches!(
        render_frame(&frame, &default_context(&recipe, None)),
        Err(CoreError::InvalidAdjustment { .. })
    ));
}

#[test]
fn legacy_heuristic_extras_still_heal_when_typed_empty() {
    // SPOT-TYPED-FIELD-FIX: the tolerant legacy path keeps healing while
    // no typed entries exist (in-memory GUI recipes pre-roundtrip).
    // Halves frame (left black, right white): a spot on black cloning
    // from white must visibly change pixels (a checker with an even
    // offset would clone identical values and prove nothing).
    let mut pixels = Vec::new();
    for _y in 0..8 {
        for x in 0..8 {
            let v = if x < 4 { 0 } else { 255 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s1","version":1,"mode":"heuristic","center_x":0.25,"center_y":0.5,"radius":2.0,"feather":0.5,"offset_dx":0.5,"offset_dy":0.0,"opacity":1.0,"status":"valid"}]),
        );
    let output = render_frame(&frame, &default_context(&recipe, None)).unwrap();
    assert_ne!(
        output.frame.pixels, frame.pixels,
        "legacy heuristic spot must visibly heal"
    );
}
