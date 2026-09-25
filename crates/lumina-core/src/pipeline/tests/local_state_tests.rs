//! MASK-LOCAL-P0 render-key identity coverage.

use super::*;

#[test]
fn local_mask_state_digest_invalidates_mask_and_render_but_not_decode() {
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 2,
        height: 2,
        format: "rgba8".into(),
    };
    let mut layer = lumina_sidecar::MaskLayer {
        id: "layer".into(),
        mask: lumina_sidecar::MaskReference {
            copy_id: "vc".into(),
            mask_id: "mask".into(),
            extras: Default::default(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: Some(lumina_sidecar::LocalAdjustments::default()),
        extras: Default::default(),
    };
    let recipe = EditRecipe::default();
    let first = RenderKey::new("s", "d", "p", "vc", &recipe, vec![], output.clone())
        .with_mask_local_state_digest(lumina_sidecar::mask_layers_digest(&[layer.clone()]));
    layer
        .local_adjustments
        .as_mut()
        .unwrap()
        .temperature_delta_k = 500.0;
    let second = RenderKey::new("s", "d", "p", "vc", &recipe, vec![], output)
        .with_mask_local_state_digest(lumina_sidecar::mask_layers_digest(&[layer]));
    assert_ne!(
        first.stage_digest(crate::cache::CacheStage::Mask),
        second.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(first.digest(), second.digest());
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Decode),
        second.stage_digest(crate::cache::CacheStage::Decode)
    );
}

/// MASK-LOCAL-P1.2a: a local tone-curve edit must invalidate the mask and
/// render identities exactly like a scalar local edit, so a cached frame can
/// never survive a curve change.
#[test]
fn local_curve_state_digest_invalidates_mask_and_render_but_not_decode() {
    let output = OutputSpec {
        profile: "sRGB".into(),
        width: 2,
        height: 2,
        format: "rgba8".into(),
    };
    let mut layer = lumina_sidecar::MaskLayer {
        id: "layer".into(),
        mask: lumina_sidecar::MaskReference {
            copy_id: "vc".into(),
            mask_id: "mask".into(),
            extras: Default::default(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: Some(lumina_sidecar::LocalAdjustments::default()),
        extras: Default::default(),
    };
    let recipe = EditRecipe::default();
    let first = RenderKey::new("s", "d", "p", "vc", &recipe, vec![], output.clone())
        .with_mask_local_state_digest(lumina_sidecar::mask_layers_digest(&[layer.clone()]));
    let mut curves = lumina_sidecar::Curves::identity();
    curves.master = vec![
        lumina_sidecar::CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        lumina_sidecar::CurvePoint {
            input: 0.5,
            output: 0.7,
        },
        lumina_sidecar::CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ];
    layer
        .local_adjustments
        .as_mut()
        .unwrap()
        .set_local_curve_channel("master", curves.master.clone())
        .unwrap();
    let second = RenderKey::new("s", "d", "p", "vc", &recipe, vec![], output)
        .with_mask_local_state_digest(lumina_sidecar::mask_layers_digest(&[layer]));
    assert_ne!(
        first.stage_digest(crate::cache::CacheStage::Mask),
        second.stage_digest(crate::cache::CacheStage::Mask)
    );
    assert_ne!(first.digest(), second.digest());
    assert_eq!(
        first.stage_digest(crate::cache::CacheStage::Decode),
        second.stage_digest(crate::cache::CacheStage::Decode)
    );
}
