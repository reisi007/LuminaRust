//! GPU-RENDER-MASK-19 (Slice B2): routing and wiring tests for the CLI
//! local-mask GPU integration (`crate::local_mask_gpu`).
//!
//! The routing cells and the neutral-pass cell run in every `--features gpu`
//! test build **without an adapter** (the routing predicate is pure recipe
//! state, and `apply_gpu_local_masks` short-circuits a neutral layer set before
//! it ever touches the adapter). The end-to-end CPU↔GPU parity cell needs a
//! Metal adapter; without one it prints an explicit SKIP verdict and returns —
//! never a silently green run (the same policy as `tests/lensfun_gpu.rs`).

#![cfg(feature = "gpu")]

use super::*;
use lumina_gpu::{Frame, GpuContext};
use lumina_sidecar::{
    CoordinateSystem, DecodeFingerprint, Extras, Flag, GeometryFingerprint, LocalAdjustments,
    MaskDefinition, MaskReference, ModelIdentity, Preprocessing, Resolution, SourceFingerprint,
    VirtualCopy,
};

const SIZE: u32 = 16;

/// A gradient that stays clear of the 255 rail, so a +1 EV local exposure
/// visibly changes the masked pixels.
pub(super) fn gradient_frame() -> ImageFrame {
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            pixels.extend_from_slice(&[(x * 12) as u8, (y * 12) as u8, 128, 255]);
        }
    }
    ImageFrame::new(SIZE, SIZE, pixels).expect("synthetic gradient frame")
}

/// Left half inside the mask (`u16::MAX`), right half outside (`0`) — so a
/// masked render changes exactly the left half and the parity assertion is
/// non-vacuous.
fn alpha_plane() -> MaskPlane {
    let values = (0..(SIZE * SIZE))
        .map(|i| if (i % SIZE) < SIZE / 2 { u16::MAX } else { 0 })
        .collect();
    MaskPlane::new(SIZE, SIZE, values).expect("alpha plane")
}

/// The persisted mask definition, geometry identical to the frame, so the
/// alignment is the identity transform (mirrors `lumina-gpu/tests/local_mask_parity.rs`).
fn mask_definition(id: &str) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "h".into(),
            byte_length: 1,
            extras: Extras::new(),
        },
        decode_context: DecodeFingerprint {
            decoder: "d".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        geometry_context: GeometryFingerprint {
            width: SIZE,
            height: SIZE,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: ModelIdentity {
            name: "m".into(),
            version: "1".into(),
            hash: "h".into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width: SIZE,
            height: SIZE,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status: MaskStatus::Valid,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: None,
        operation: MaskOperation::Source,
        references: vec![],
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    }
}

/// One mask layer with the given typed recipe and visibility eye.
pub(super) fn layer(id: &str, adjustments: Option<LocalAdjustments>, visible: bool) -> MaskLayer {
    MaskLayer {
        id: id.into(),
        mask: MaskReference {
            copy_id: "vc".into(),
            mask_id: "subject".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible,
        local_adjustments: adjustments,
    }
}

/// One active copy (`vc`) carrying `layers` over the `subject` definition plus
/// the matching `subject` plane. Returned as a ready `MaskContext`.
pub(super) fn context<'a>(
    copies: &'a [VirtualCopy],
    planes: &BTreeMap<(String, String), MaskPlane>,
) -> MaskContext<'a> {
    MaskContext {
        copies,
        active_copy_id: "vc",
        planes: planes.clone(),
        policy: MaskPolicy::Strict,
        source_roi: None,
    }
}

/// A `vc` copy with the `subject` definition and the given layers.
pub(super) fn copy_with(layers: Vec<MaskLayer>) -> VirtualCopy {
    VirtualCopy {
        id: "vc".into(),
        name: "vc".into(),
        is_default: true,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: vec![mask_definition("subject")],
        mask_layers: layers,
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }
}

pub(super) fn planes() -> BTreeMap<(String, String), MaskPlane> {
    BTreeMap::from([(("vc".into(), "subject".into()), alpha_plane())])
}

/// (a) A visible, non-neutral **P0-only** layer is GPU-eligible: no mask reason
/// at all (the former blanket `masks (active copy has layers)` route is gone).
#[test]
fn p0_only_layer_is_not_a_cpu_route_reason() {
    let copies = vec![copy_with(vec![layer(
        "layer-1",
        Some(LocalAdjustments {
            exposure: 1.0,
            ..LocalAdjustments::default()
        }),
        true,
    )])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };
    let reasons = gpu_routing_reasons(&recipe, &render_ctx);
    assert!(
        reasons.is_empty(),
        "P0-only mask must stay GPU-eligible: {reasons:?}"
    );
    assert!(
        !reasons.iter().any(|r| r.contains("active copy has layers")),
        "the blanket mask route must be gone: {reasons:?}"
    );
}

/// (b) A visible, non-neutral layer with a **P1.2** block (local tone curve) is
/// named by the routing predicate — the plane-free half of the pass's own gate.
#[test]
fn p12_layer_is_named_as_a_cpu_route_reason() {
    let curve = LocalAdjustments {
        curves: Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 0.5,
                    output: 0.8,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels {
                red: None,
                green: None,
                blue: None,
            },
        }),
        ..LocalAdjustments::default()
    };
    let copies = vec![copy_with(vec![layer("curve-layer", Some(curve), true)])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };
    let reasons = gpu_routing_reasons(&recipe, &render_ctx);
    assert_eq!(
        reasons.len(),
        1,
        "expected exactly one P1.2 reason: {reasons:?}"
    );
    assert!(
        reasons[0].contains("curve-layer") && reasons[0].contains("P1.2 block"),
        "the reason must name the layer and the block class: {reasons:?}"
    );
}

/// (c) An **invisible** layer is skipped (G-03 eye): no reason — even when its
/// typed recipe carries a **P1.2** block. The P1.2 block is what makes the
/// visibility skip load-bearing: without the `if !layer.visible` guard the
/// predicate would fall through to `unsupported_local_mask_adjustments` and name
/// this layer, so removing the skip turns this cell red. `saturation` is the
/// P1.2 colour block (`has_local_color`), and it also makes the recipe
/// non-neutral — the skip, not neutrality, is what suppresses the reason here.
#[test]
fn invisible_p12_layer_is_not_a_cpu_route_reason() {
    let copies = vec![copy_with(vec![layer(
        "hidden",
        Some(LocalAdjustments {
            saturation: 0.5,
            ..LocalAdjustments::default()
        }),
        false,
    )])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };
    assert!(
        gpu_routing_reasons(&recipe, &render_ctx).is_empty(),
        "an invisible layer changes no pixel and must not route CPU"
    );
}

/// (d) A **neutral** visible layer is skipped: no reason.
///
/// The neutral arm of the predicate (`Ok(Some(a)) if a.is_neutral() => {}`) is
/// **observationally equivalent** to the fall-through: `is_neutral()` requires
/// an identity/absent curve *and* `!has_local_color/presence/detail`, so a
/// neutral recipe carries no P1.2 block and `unsupported_local_mask_adjustments`
/// returns `None` for it anyway. The two arms are therefore indistinguishable
/// from the outside — "neutral with a P1.2 block" is impossible by definition —
/// so this cell pins the intended behaviour (neutral → no reason) but cannot
/// make the skip load-bearing. The **visibility** skip is pinned by (c).
#[test]
fn neutral_layer_is_not_a_cpu_route_reason() {
    let copies = vec![copy_with(vec![layer(
        "neutral",
        Some(LocalAdjustments::default()),
        true,
    )])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };
    assert!(
        gpu_routing_reasons(&recipe, &render_ctx).is_empty(),
        "a neutral recipe changes no pixel and must not route CPU"
    );
}

/// Neutral-case of the pass (the low finding carried from Slice B1): an empty or
/// all-neutral layer set returns the global frame **unchanged** via the
/// `Ok(global.clone())` short-circuit. Measured: `GpuContext::new()` succeeds
/// headless (it degrades to `resources: None`), and the short-circuit runs
/// before the adapter is consulted — so this cell needs **no** adapter.
#[test]
fn apply_gpu_local_masks_is_a_no_op_for_an_empty_or_neutral_set() {
    let Ok(ctx) = GpuContext::new() else {
        eprintln!("GPU context init failed - skipped neutral local-mask no-op test");
        return;
    };
    let global = Frame::from_image_frame(gradient_frame());

    let empty = ctx
        .apply_gpu_local_masks(&global, &[])
        .expect("an empty layer set is a pure no-op");
    assert_eq!(
        empty, global,
        "an empty layer set must return the frame unchanged"
    );

    let neutral = vec![lumina_gpu::local_mask_pass::LocalMaskLayer {
        layer_id: "neutral".into(),
        plane: alpha_plane(),
        adjustments: LocalAdjustments::default(),
    }];
    let unchanged = ctx
        .apply_gpu_local_masks(&global, &neutral)
        .expect("an all-neutral layer set is a pure no-op");
    assert_eq!(
        unchanged, global,
        "an all-neutral layer set must return the frame unchanged"
    );
}

/// End-to-end CPU↔GPU parity for a mask-carrying recipe through the CLI render
/// entry (`render_best_effort`): the GPU route must composite the local mask and
/// match the CPU oracle byte-for-byte, with the same `mask_layers`. Needs a
/// Metal adapter (SKIP otherwise) — the CLI has no adapter-required pattern in
/// `crates/lumina-cli/tests/`, so this mirrors `tests/lensfun_gpu.rs`.
#[test]
fn mask_recipe_renders_byte_identically_on_gpu_and_cpu() {
    let Ok(mut gpu) = GpuContext::new() else {
        eprintln!("GPU adapter unavailable - skipped CLI local-mask parity test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU adapter unavailable - skipped CLI local-mask parity test");
        return;
    }
    let frame = gradient_frame();
    let copies = vec![copy_with(vec![layer(
        "layer-1",
        Some(LocalAdjustments {
            exposure: 1.0,
            contrast: -0.3,
            ..LocalAdjustments::default()
        }),
        true,
    )])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };

    let (gpu_output, route) = render_best_effort(
        Some(&mut gpu),
        &frame,
        &recipe,
        &render_ctx,
        GenerativeCanvasInput::default(),
    )
    .expect("mask recipe renders on the GPU");
    assert!(
        route.is_gpu(),
        "a P0-only mask must take the GPU route, reasons={:?}",
        route.reasons()
    );

    let cpu_output =
        render_frame_with_generative(&frame, &render_ctx, GenerativeCanvasInput::default())
            .expect("CPU oracle render");
    assert_eq!(
        gpu_output.frame.pixels(),
        cpu_output.frame.pixels(),
        "the GPU local-mask pass must be byte-identical to the CPU oracle"
    );
    assert_eq!(
        gpu_output.mask_layers, cpu_output.mask_layers,
        "the GPU path must report the same evaluated mask layers as the CPU oracle"
    );

    // Non-vacuous: the local pass must actually change pixels relative to the
    // un-modulated global render, otherwise the parity assertion above would
    // hold for a wiring that never composites anything.
    let global = gpu
        .render_with_gpu_and_generative(&frame, &recipe, &GenerativeCanvasInput::default())
        .expect("global GPU render")
        .to_image_frame();
    assert_ne!(
        gpu_output.frame.pixels(),
        global.pixels(),
        "the local mask pass changed no pixel - the parity assertion is vacuous"
    );
}

/// The wiring refuses loudly (CPU route) rather than returning the
/// un-modulated global frame: a recipe whose visible local layer carries a
/// **P1.2** block must never be silently composited. Pure recipe state, no
/// adapter.
#[test]
fn p12_layer_keeps_the_loud_cpu_route() {
    let curve = LocalAdjustments {
        curves: Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 0.5,
                    output: 0.8,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels {
                red: None,
                green: None,
                blue: None,
            },
        }),
        ..LocalAdjustments::default()
    };
    let copies = vec![copy_with(vec![layer("curve-layer", Some(curve), true)])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };
    // No adapter bound: the routing reason is computed before any GPU work, so
    // the P1.2 predicate alone decides the route.
    let Ok(mut gpu) = GpuContext::new() else {
        return;
    };
    let (_, route) = render_best_effort(
        Some(&mut gpu),
        &gradient_frame(),
        &recipe,
        &render_ctx,
        GenerativeCanvasInput::default(),
    )
    .expect("a P1.2 mask renders through the CPU reference");
    assert!(!route.is_gpu(), "a P1.2 block must stay on the CPU");
    assert!(
        route.reasons().iter().any(|r| r.contains("P1.2 block")),
        "the CPU route must be loud and name the block: {:?}",
        route.reasons()
    );
}
