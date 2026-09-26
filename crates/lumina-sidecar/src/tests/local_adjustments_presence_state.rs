//! MASK-LOCAL-P1.2c sidecar tests, part 2: the local presence block inside
//! history snapshots, reloads and the canonical digests.
//!
//! Split from `local_adjustments_presence.rs` (file-size ratchet): that file
//! owns the round trip, the migrations, the shared-range refusals and the
//! disabled stages; this one owns the additive state contracts.

use super::local_adjustments_color::document_with_layer;
use super::local_adjustments_presence::{presence_layer, textured_presence};
use super::*;
use serde_json::json;

/// The presence block is part of both canonical digests, so a presence edit
/// can never reuse stale pixels, stale caches or stale history.
#[test]
fn canonical_digests_cover_the_local_presence_block() {
    let base = presence_layer();
    let base_digest = mask_layers_digest(std::slice::from_ref(&base));
    // Every single field moves the mask-state digest, and so does its sign.
    for (field, value) in [
        ("texture", 0.5),
        ("clarity", 0.5),
        ("dehaze", 0.5),
        ("texture", 0.35),
    ] {
        let mut moved = presence_layer();
        let mut local = moved.local_adjustments.clone().unwrap();
        local.set_local_presence_field(field, value).expect(field);
        assert_ne!(
            local.presence,
            base.local_adjustments.clone().unwrap().presence,
            "the `{field}` variant must really differ"
        );
        moved.local_adjustments = Some(local);
        assert_ne!(
            base_digest,
            mask_layers_digest(std::slice::from_ref(&moved)),
            "`{field}={value}` must be part of the mask-state digest"
        );
        assert_ne!(
            base.local_adjustments.as_ref().unwrap().digest(),
            moved.local_adjustments.as_ref().unwrap().digest(),
            "`{field}={value}` must be part of the per-layer digest"
        );
    }
    // Reordering two overlapping layers is a digest change, never an
    // equivalent state.
    let mut a = presence_layer();
    a.id = "layer-a".into();
    let mut b = presence_layer();
    b.id = "layer-b".into();
    let mut local = b.local_adjustments.clone().unwrap();
    local.set_local_presence_field("dehaze", 0.5).unwrap();
    b.local_adjustments = Some(local);
    assert_ne!(
        mask_layers_digest(&[a.clone(), b.clone()]),
        mask_layers_digest(&[b, a])
    );
}

/// The presence block survives a history snapshot and a reload verbatim, and a
/// legacy snapshot version migrates without losing it.
#[test]
fn local_presence_survives_history_snapshot_and_reload() {
    let document = document_with_layer(presence_layer());
    let copy = &document.virtual_copies[0];
    let snapshot = MaskStateSnapshot::new(copy.mask_layers.clone());
    assert_eq!(snapshot.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(snapshot.layers.len(), 1);
    assert_eq!(
        snapshot.layers[0]
            .local_adjustments
            .as_ref()
            .unwrap()
            .presence,
        Some(textured_presence())
    );
    // A legacy snapshot version is normalized forward and keeps its block.
    let mut legacy = snapshot.clone();
    legacy.version = COLOR_LOCAL_ADJUSTMENTS_VERSION;
    let json = serde_json::to_string(&legacy).unwrap();
    let parsed: MaskStateSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(
        parsed.layers[0]
            .local_adjustments
            .as_ref()
            .unwrap()
            .presence,
        Some(textured_presence())
    );
    // But a v4 snapshot that *carries* a presence block is refused loudly.
    let mut value: Value = serde_json::from_str(&json).unwrap();
    value["layers"][0]["local_adjustments"]["version"] = json!(COLOR_LOCAL_ADJUSTMENTS_VERSION);
    value["layers"][0]["local_adjustments"]["presence"] =
        json!({"version": 1, "texture": 0.5, "clarity": 0.0, "dehaze": 0.0});
    assert!(
        serde_json::from_str::<MaskStateSnapshot>(&serde_json::to_string(&value).unwrap()).is_err(),
        "a v4 snapshot must not be able to smuggle a presence block"
    );
    assert_eq!(
        snapshot.digest(),
        MaskStateSnapshot::new(copy.mask_layers.clone()).digest()
    );
}
