//! MASK-LOCAL-P1.2d sidecar *state* tests for the local detail block: the
//! canonical digests and the history/previous snapshot.
//!
//! Split from `local_adjustments_detail.rs` (file-size ratchet): that file owns
//! the schema and the migration, this one owns the identity contracts. The
//! disabled-stage refusal pins live in `local_adjustments_color_digest.rs`
//! (`disabled_local_presence_detail_denoise_and_optics_stay_rejected`), the one
//! representative of that family.

use super::local_adjustments::layer_with_legacy;
use super::*;
use serde_json::json;

/// The canonical digests and the history/previous snapshot carry the complete
/// detail block.
#[test]
fn canonical_digests_cover_the_local_detail_block() {
    let mut sharpen_only = LocalAdjustments::default();
    sharpen_only
        .set_local_sharpening_field("amount", 1.0)
        .unwrap();
    let mut noise_only = LocalAdjustments::default();
    noise_only
        .set_local_noise_reduction_field("luminance", 0.3)
        .unwrap();
    let mut both = LocalAdjustments::default();
    both.set_local_sharpening_field("amount", 1.0).unwrap();
    both.set_local_noise_reduction_field("luminance", 0.3)
        .unwrap();
    let digests = [sharpen_only.digest(), noise_only.digest(), both.digest()];
    // All three are distinct: neither sub-block may be dropped from the identity.
    assert_ne!(digests[0], digests[1]);
    assert_ne!(digests[0], digests[2]);
    assert_ne!(digests[1], digests[2]);

    let layer_of = |local: LocalAdjustments| {
        let mut layer = layer_with_legacy();
        layer.extras.clear();
        layer.local_adjustments = Some(local);
        layer.normalize_local_adjustments().unwrap();
        layer
    };
    let single = [
        vec![layer_of(sharpen_only)],
        vec![layer_of(noise_only)],
        vec![layer_of(both.clone())],
    ];
    let digests: Vec<String> = single
        .iter()
        .map(|layers| mask_layers_digest(layers))
        .collect();
    assert_ne!(digests[0], digests[1]);
    assert_ne!(digests[0], digests[2]);
    assert_ne!(digests[1], digests[2]);
    // Reordering the persisted layer list is a change of the state identity.
    let mut reordered = single[2].clone();
    let extra = layer_of(LocalAdjustments::default());
    reordered.push(extra);
    assert_ne!(mask_layers_digest(&reordered), digests[2]);

    // A history snapshot carries the block verbatim.
    let snapshot = MaskStateSnapshot::new(vec![layer_of(both.clone())]);
    assert!(snapshot.validate().is_ok());
    let restored = snapshot.layers[0]
        .local_adjustments
        .as_ref()
        .expect("typed local recipe");
    assert_eq!(restored.detail, both.detail);
    assert!(restored.has_local_detail());
    // A legacy snapshot version migrates forward and keeps the block.
    let mut legacy: Value =
        serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).expect("serializable");
    legacy["version"] = json!(LOCAL_ADJUSTMENTS_VERSION);
    let loaded: MaskStateSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(loaded.version, LOCAL_ADJUSTMENTS_VERSION);
    assert!(loaded.layers[0]
        .local_adjustments
        .as_ref()
        .expect("typed local recipe")
        .has_local_detail());
}
