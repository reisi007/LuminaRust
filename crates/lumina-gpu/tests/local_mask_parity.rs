//! GPU-RENDER-MASK-19 parity harness: the GPU local-mask pixel pass against the
//! CPU oracle (`lumina_core::render_frame` with a real `MaskContext`).
//!
//! Each case drives the pass at a non-neutral value and asserts
//!
//! 1. `unsupported_local_mask_stages` is empty (the layer is genuinely
//!    GPU-renderable, not silently routed),
//! 2. outside the mask (`alpha == 0`) the GPU frame is **byte-identical** to the
//!    oracle, and inside the mask the case's declared equivalence holds: the
//!    P0-only and multi-layer cases are byte-identical (the global tone shader
//!    is byte-identical to `apply_recipe` and the blend is exact integer math),
//!    the relative-WB case is bounded at `maxAbsDiff ≤ 1` (the `f32` shader vs
//!    the oracle's `f64` chain),
//! 3. the pass is **non-vacuous**: the oracle actually differs from the
//!    un-modulated global frame (a no-op pass cannot pass this test).
//!
//! Without a bound adapter the hardware cases are skipped loudly
//! (`support::require_adapter` panics under the opt-in `gpu-adapter-tests`
//! feature); the pure validation assertions still run.
#![cfg(feature = "gpu")]
mod support;

use lumina_core::{render_frame, ImageFrame, MaskContext, MaskPlane, MaskPolicy, RenderContext};
use lumina_gpu::local_mask_pass::{
    unsupported_local_mask_stages, validate_local_mask_layers, LocalMaskLayer,
};
use lumina_gpu::{Frame, GpuContext};
use lumina_sidecar::{
    CoordinateSystem, DecodeFingerprint, EditRecipe, Extras, Flag, GeometryFingerprint,
    LocalAdjustments, MaskDefinition, MaskLayer, MaskOperation, MaskReference, MaskStatus,
    ModelIdentity, Preprocessing, Resolution, SourceFingerprint, VirtualCopy,
};
use std::collections::BTreeMap;

const W: u32 = 16;
const H: u32 = 16;

fn gradient_frame() -> ImageFrame {
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let rx = x as f64 / (W as f64 - 1.0);
            let ry = y as f64 / (H as f64 - 1.0);
            pixels.extend_from_slice(&[
                (rx * 255.0).round() as u8,
                (ry * 255.0).round() as u8,
                (((rx + ry) * 0.5) * 255.0).round() as u8,
                255,
            ]);
        }
    }
    ImageFrame::new(W, H, pixels).expect("synthetic gradient frame")
}

/// A varied alpha plane with both endpoints (`0` and `u16::MAX`) and fractional
/// values present, so the endpoint and the fractional branch are both exercised.
fn alpha_plane() -> MaskPlane {
    let values = (0..(W * H) as usize)
        .map(|i| match i % 5 {
            0 => 0,
            1 => 32_768,
            2 => u16::MAX,
            3 => 12_345,
            _ => 60_000,
        })
        .collect();
    MaskPlane::new(W, H, values).expect("alpha plane")
}

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
            width: W,
            height: H,
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
            width: W,
            height: H,
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

fn reference(copy_id: &str, mask_id: &str) -> MaskReference {
    MaskReference {
        copy_id: copy_id.into(),
        mask_id: mask_id.into(),
        extras: Extras::new(),
    }
}

fn layer(id: &str, mask_id: &str, adjustments: LocalAdjustments) -> MaskLayer {
    MaskLayer {
        id: id.into(),
        mask: reference("vc", mask_id),
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: Some(adjustments),
    }
}

/// The CPU oracle: `render_frame` with a `MaskContext` whose definition geometry
/// matches the frame, so mask alignment is the identity transform and the
/// evaluated plane is exactly `plane`.
fn oracle(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    entries: &[(String, MaskPlane, LocalAdjustments)],
) -> ImageFrame {
    let definitions = entries
        .iter()
        .map(|(id, _, _)| mask_definition(id))
        .collect();
    let layers = entries
        .iter()
        .map(|(id, _, adjustments)| layer(&format!("{id}-layer"), id, adjustments.clone()))
        .collect();
    let copies = vec![VirtualCopy {
        id: "vc".into(),
        name: "vc".into(),
        is_default: false,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: definitions,
        mask_layers: layers,
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }];
    let planes = entries
        .iter()
        .map(|(id, plane, _)| (("vc".to_string(), id.clone()), plane.clone()))
        .collect();
    let context = MaskContext {
        copies: &copies,
        active_copy_id: "vc",
        planes,
        policy: MaskPolicy::Strict,
        source_roi: None,
    };
    render_frame(
        frame,
        &RenderContext {
            recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(context),
        },
    )
    .expect("CPU oracle render")
    .frame
}

fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// The strongest CPU↔GPU equivalence a case is asserted at, mirroring the
/// existing `parity.rs` convention.
#[derive(Clone, Copy)]
enum Equivalence {
    /// Measured byte-identical (`maxAbsDiff == 0`): the whole chain is exact
    /// (the tone shader is byte-identical to `apply_recipe` and the mask blend
    /// is exact integer math).
    ByteIdentical,
    /// Measured within this many 8-bit codes (the relative-WB `f32` chain vs the
    /// oracle's `f64`).
    Bounded(u8),
}

/// Assert the parity contract for one case and that it is non-vacuous.
fn assert_parity(
    global: &Frame,
    gpu: &Frame,
    cpu: &ImageFrame,
    plane: &MaskPlane,
    equivalence: Equivalence,
) {
    assert_eq!(gpu.width, cpu.width);
    assert_eq!(gpu.height, cpu.height);
    let diff = max_abs_diff(&gpu.pixels, cpu.pixels());
    match equivalence {
        Equivalence::ByteIdentical => assert_eq!(
            diff, 0,
            "expected byte-identical output, measured maxAbsDiff {diff}"
        ),
        Equivalence::Bounded(bound) => assert!(
            diff <= bound,
            "maxAbsDiff {diff} > {bound} against the CPU oracle"
        ),
    }
    for (index, &alpha) in plane.values.iter().enumerate() {
        if alpha == 0 {
            let range = index * 4..index * 4 + 4;
            assert_eq!(
                &gpu.pixels[range.clone()],
                &cpu.pixels()[range],
                "outside the mask (alpha == 0) pixel {index} must be byte-identical"
            );
        }
    }
    // Non-vacuous: the local pass must actually change RGB pixels relative to
    // the un-modulated global frame, otherwise the parity assertion above would
    // hold for a pass that does nothing.
    let changed = (0..plane.values.len()).any(|index| {
        let range = index * 4..index * 4 + 3;
        gpu.pixels[range.clone()] != global.pixels[range]
    });
    assert!(
        changed,
        "the local mask pass changed no pixel — the parity assertion is vacuous"
    );
}

fn local_layers(entries: &[(String, MaskPlane, LocalAdjustments)]) -> Vec<LocalMaskLayer> {
    entries
        .iter()
        .map(|(id, plane, adjustments)| LocalMaskLayer {
            layer_id: id.clone(),
            plane: plane.clone(),
            adjustments: adjustments.clone(),
        })
        .collect()
}

fn require_gpu() -> Option<GpuContext> {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped local-mask parity");
            return None;
        }
    };
    if support::require_adapter(&ctx) {
        Some(ctx)
    } else {
        None
    }
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn local_mask_p0_pass_matches_cpu_oracle() {
    let Some(ctx) = require_gpu() else { return };
    let frame = gradient_frame();
    let plane = alpha_plane();
    let adjustments = LocalAdjustments {
        exposure: 1.0,
        contrast: -0.3,
        ..LocalAdjustments::default()
    };
    let recipe = EditRecipe::default();
    let entries = vec![("subject".to_string(), plane.clone(), adjustments)];
    let layers = local_layers(&entries);
    assert!(
        unsupported_local_mask_stages(&layers).is_empty(),
        "a P0-only layer must be GPU-renderable"
    );
    let global = ctx.render_with_gpu(&frame, &recipe).expect("global render");
    let gpu = ctx
        .render_with_gpu_local_masks(&frame, &recipe, &layers)
        .expect("GPU local-mask render");
    let cpu = oracle(&frame, &recipe, &entries);
    assert_parity(&global, &gpu, &cpu, &plane, Equivalence::ByteIdentical);
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn local_mask_wb_delta_pass_matches_cpu_oracle() {
    let Some(ctx) = require_gpu() else { return };
    let frame = gradient_frame();
    let plane = alpha_plane();
    let adjustments = LocalAdjustments {
        temperature_delta_k: 1200.0,
        tint_delta: -0.4,
        exposure: 0.5,
        shadows: 0.4,
        ..LocalAdjustments::default()
    };
    let recipe = EditRecipe::default();
    let entries = vec![("subject".to_string(), plane.clone(), adjustments)];
    let layers = local_layers(&entries);
    assert!(unsupported_local_mask_stages(&layers).is_empty());
    let global = ctx.render_with_gpu(&frame, &recipe).expect("global render");
    let gpu = ctx
        .render_with_gpu_local_masks(&frame, &recipe, &layers)
        .expect("GPU local-mask render");
    let cpu = oracle(&frame, &recipe, &entries);
    assert_parity(&global, &gpu, &cpu, &plane, Equivalence::Bounded(1));
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn local_mask_layers_composite_in_order_and_after_global() {
    let Some(ctx) = require_gpu() else { return };
    let frame = gradient_frame();
    let first_plane = alpha_plane();
    let second_plane = MaskPlane::new(W, H, vec![u16::MAX; (W * H) as usize]).unwrap();
    let first = LocalAdjustments {
        exposure: 0.8,
        ..LocalAdjustments::default()
    };
    let second = LocalAdjustments {
        contrast: -0.5,
        ..LocalAdjustments::default()
    };
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".to_string(), 0.5)]),
        ..EditRecipe::default()
    };
    let entries = vec![
        ("first".to_string(), first_plane.clone(), first),
        ("second".to_string(), second_plane.clone(), second),
    ];
    let layers = local_layers(&entries);
    let global = ctx.render_with_gpu(&frame, &recipe).expect("global render");
    let gpu = ctx
        .render_with_gpu_local_masks(&frame, &recipe, &layers)
        .expect("GPU local-mask render");
    let cpu = oracle(&frame, &recipe, &entries);
    assert_parity(
        &global,
        &gpu,
        &cpu,
        &first_plane,
        Equivalence::ByteIdentical,
    );
}

/// Pins the **delegation contract** of the second step, not the chain's
/// effectiveness: `apply_gpu_local_masks` on the already-rendered global frame
/// and the one-call entry `render_with_gpu_local_masks` (`render_with_gpu` plus
/// that same step) agree byte-for-byte, so the split cannot have grown a
/// divergent second code path.
///
/// It cannot prove the chain itself: both sides share the one chain, so a
/// disabled chain would stay green here. The chain is pinned by the oracle tests
/// above (`local_mask_p0_pass_matches_cpu_oracle`,
/// `local_mask_wb_delta_pass_matches_cpu_oracle`,
/// `local_mask_layers_composite_in_order_and_after_global`), which a `take(0)`
/// mutation on the layer loop turns red (measured).
#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn apply_gpu_local_masks_on_a_global_frame_is_byte_identical_to_the_one_call_entry() {
    let Some(ctx) = require_gpu() else { return };
    let frame = gradient_frame();
    let plane = alpha_plane();
    let adjustments = LocalAdjustments {
        exposure: 1.0,
        contrast: -0.3,
        ..LocalAdjustments::default()
    };
    let recipe = EditRecipe::default();
    let entries = vec![("subject".to_string(), plane, adjustments)];
    let layers = local_layers(&entries);
    let global = ctx.render_with_gpu(&frame, &recipe).expect("global render");
    let isolated = ctx
        .apply_gpu_local_masks(&global, &layers)
        .expect("isolated mask pass");
    let one_call = ctx
        .render_with_gpu_local_masks(&frame, &recipe, &layers)
        .expect("one-call local-mask render");
    assert_eq!(isolated, one_call);
}

#[test]
fn local_mask_p12_blocks_are_a_loud_route_reason() {
    // No adapter needed: the gate and the refusal are pure Rust.
    let plane = alpha_plane();
    let with_curve = LocalMaskLayer {
        layer_id: "curve-layer".into(),
        plane: plane.clone(),
        adjustments: LocalAdjustments {
            curves: Some(lumina_sidecar::Curves {
                version: 1,
                master: vec![
                    lumina_sidecar::CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    lumina_sidecar::CurvePoint {
                        input: 1.0,
                        output: 0.5,
                    },
                ],
                channels: lumina_sidecar::CurveChannels {
                    red: None,
                    green: None,
                    blue: None,
                },
            }),
            ..LocalAdjustments::default()
        },
    };
    let reasons = unsupported_local_mask_stages(std::slice::from_ref(&with_curve));
    assert_eq!(reasons.len(), 1, "expected one named reason: {reasons:?}");
    assert!(reasons[0].contains("curve-layer"));

    // A neutral layer is never a reason, even with a block present.
    let neutral = LocalMaskLayer {
        layer_id: "neutral".into(),
        plane,
        adjustments: LocalAdjustments::default(),
    };
    assert!(unsupported_local_mask_stages(std::slice::from_ref(&neutral)).is_empty());
}

#[test]
fn local_mask_validation_rejects_mismatched_planes() {
    let frame = gradient_frame();
    let layer = LocalMaskLayer {
        layer_id: "layer-1".into(),
        plane: MaskPlane::new(W / 2, H, vec![0; (W / 2 * H) as usize]).unwrap(),
        adjustments: LocalAdjustments {
            exposure: 1.0,
            ..LocalAdjustments::default()
        },
    };
    let error = validate_local_mask_layers(frame.width, frame.height, std::slice::from_ref(&layer))
        .expect_err("a mismatched plane must be refused loudly");
    assert!(error.to_string().contains("does not match the output"));
}
