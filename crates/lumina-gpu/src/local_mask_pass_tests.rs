use super::*;
use lumina_sidecar::CurvePoint;

fn layer_with(adjustments: LocalAdjustments) -> LocalMaskLayer {
    LocalMaskLayer {
        layer_id: "layer-1".into(),
        plane: MaskPlane::new(2, 2, vec![u16::MAX; 4]).unwrap(),
        adjustments,
    }
}

/// A neutral recipe is never a route reason — plane-free or bound.
#[test]
fn neutral_adjustments_have_no_plane_free_route_reason() {
    let adjustments = LocalAdjustments::default();
    assert_eq!(
        unsupported_local_mask_adjustments("layer-1", &adjustments),
        None
    );
    assert!(unsupported_local_mask_stages(&[layer_with(adjustments)]).is_empty());
}

/// Every P1.2 block reports through the plane-free predicate, and the bound
/// helper is exactly that predicate plus the neutrality skip — same message.
#[test]
fn each_p12_block_is_the_same_plane_free_and_bound_reason() {
    let curve = LocalAdjustments {
        curves: Some(lumina_sidecar::Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
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
    };
    let color = LocalAdjustments {
        vibrance: 0.5,
        ..LocalAdjustments::default()
    };
    let mut presence = LocalAdjustments::default();
    presence
        .set_local_presence_field("texture", 0.5)
        .expect("valid local presence");
    let mut detail = LocalAdjustments::default();
    detail
        .set_local_sharpening_field("amount", 0.5)
        .expect("valid local sharpening");

    for adjustments in [curve, color, presence, detail] {
        let plane_free = unsupported_local_mask_adjustments("layer-1", &adjustments)
            .expect("a P1.2 block must be a plane-free route reason");
        assert_eq!(
            plane_free,
            "local mask layer `layer-1` uses a P1.2 block \
                 (tone curve/color/presence/detail) not GPU-wired"
        );
        let bound = unsupported_local_mask_stages(&[layer_with(adjustments)]);
        assert_eq!(bound, vec![plane_free]);
    }
}

#[test]
fn vram_pass_refuses_unsupported_local_stage_before_requiring_vram() {
    let context = GpuContext::new().expect("GPU context falls back without an adapter");
    let layer = layer_with(LocalAdjustments {
        vibrance: 0.5,
        ..LocalAdjustments::default()
    });

    let error = context
        .apply_gpu_local_masks_to_vram(&[layer])
        .expect_err("unsupported P1.2 layers must be refused before any VRAM write");
    assert!(error.to_string().contains("P1.2 block"));
    assert!(!error.to_string().contains("active VRAM render"));
}

#[test]
fn vram_pass_treats_neutral_layers_as_identity_without_adapter() {
    let context = GpuContext::new().expect("GPU context falls back without an adapter");
    let layer = layer_with(LocalAdjustments::default());

    context
        .apply_gpu_local_masks_to_vram(&[layer])
        .expect("neutral layers are a no-op and require no active VRAM or adapter");
}

#[test]
fn vram_preflight_rejects_mismatched_dimensions() {
    let layer = layer_with(LocalAdjustments {
        exposure: 1.0,
        ..Default::default()
    });
    let error = preflight_vram_local_mask_request(&[layer], Some((3, 2))).unwrap_err();
    assert!(error.to_string().contains("does not match the output 3x2"));
}

#[test]
fn vram_entry_rejects_malformed_neutral_arguments_before_identity_return() {
    let context = GpuContext::new().expect("GPU context falls back without an adapter");
    let mut layer = layer_with(LocalAdjustments::default());
    layer.plane = MaskPlane {
        width: 2,
        height: 2,
        values: vec![u16::MAX],
    };
    let error = context.apply_gpu_local_masks_to_vram(&[layer]).unwrap_err();
    assert!(error.to_string().contains("malformed plane"));
}
