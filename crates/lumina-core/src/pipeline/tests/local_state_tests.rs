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
