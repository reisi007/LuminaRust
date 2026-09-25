//! base-stage preview render cache tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::source_actions::{action_fixture, replace_fixture_artifact, ActionFixtureMode};
use super::*;

#[test]
fn alt_reset_path_restores_single_adjustment_default() {
    // Welle 2 Alt-Regler-Reset (`label_reset_requested` in `slider.rs`
    // wires Alt+click to the same path): resetting one control restores
    // its documented default and leaves every other key alone.
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_adjustment("exposure", 3.0);
    app.set_adjustment("contrast", 0.5);
    app.reset_single_adjustment("exposure");
    assert_eq!(app.recipe().adjustments["exposure"], 0.0);
    assert_eq!(app.recipe().adjustments["contrast"], 0.5);
}

#[test]
fn render_key_is_invalidated_until_render() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    assert!(app.render_key().is_some());
    app.set_adjustment("exposure", 1.0);
    assert!(app.render_key().is_none());
    app.render().unwrap();
    assert!(app.render_key().is_some());
    assert_eq!(app.tone_analysis().unwrap().sample_count, 2);
}

/// UX-SLICE-2 (F1): the header hash gate. With a current render it is
/// hidden only in the Library module while the visible raster is empty; a
/// non-empty raster, any other module or an absent render fails the gate
/// the other way. UX-SLICE-3 (F1 follow-up): "empty" must be the empty
/// state's own predicate ([`LuminaApp::filtered_library_order`]) — a
/// zero-match `\` filter hides the hash even though the RAW listing is
/// non-empty. `render_key` itself is never cleared by the gate.
#[test]
fn render_hash_gate_hides_hash_in_empty_library_only() {
    let mut app = new_app();
    assert!(!app.render_hash_visible(), "no render implies no hash");
    app.load_bytes(png(), "test.png").unwrap();
    assert!(app.render_key().is_some(), "load renders synchronously");
    assert!(
        app.render_hash_visible(),
        "Develop (default) keeps the hash"
    );
    // Unfiltered empty: Library with no listing entries at all.
    let empty = tempfile::tempdir().unwrap();
    app.active_module = Module::Library;
    app.set_directory(empty.path().display().to_string());
    assert!(app.entries().is_empty());
    assert!(
        !app.render_hash_visible(),
        "an empty RAW listing must hide the hash"
    );
    // Listing a RAW entry makes the (unfiltered) raster non-empty: the
    // hash returns.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("img.arw"), b"lumina-raw-fixture").unwrap();
    app.set_directory(dir.path().display().to_string());
    assert!(!app.entries().is_empty());
    assert!(
        app.render_hash_visible(),
        "a listed RAW entry keeps the hash visible"
    );
    // Filtered empty (UX-SLICE-3): entries exist but the `\` query matches
    // nothing, so the shared empty state shows and the gate must agree —
    // no hash above "No images".
    app.set_library_filter("definitely-no-match");
    assert!(
        app.filtered_library_order().is_empty(),
        "the filter must produce a zero-match raster"
    );
    assert!(
        !app.render_hash_visible(),
        "a zero-match filter must hide the hash despite listed entries"
    );
    // A matching filter restores the visible raster and the hash.
    app.set_library_filter("img");
    assert!(!app.filtered_library_order().is_empty());
    assert!(
        app.render_hash_visible(),
        "a matching filter keeps the hash visible"
    );
    // Clearing the filter keeps it.
    app.set_library_filter("");
    assert!(
        app.render_hash_visible(),
        "clearing the filter keeps the hash visible"
    );
    assert!(
        app.render_key().is_some(),
        "the gate must never clear the underlying render_key"
    );
}

#[test]
fn exposure_change_hits_base_cache_and_stays_pixel_identical_to_cold_render() {
    let mut app = new_app();
    app.load_bytes(gradient_png(16, 12), "grad.png").unwrap();

    // `load_bytes` renders once internally, so the base stage is warm:
    // every recipe-only change below must be a pure downstream re-render.
    assert_eq!(app.base_stage_cache_len(), 1);
    app.set_adjustment("exposure", 1.0);
    app.render().unwrap();
    let first = app.last_stage_work().expect("stage work recorded");
    assert!(
        first.base_cache_hit,
        "a pure exposure change must reuse the base built at load time"
    );
    assert_eq!(first.adjustments_passes, 1);

    // A further drag tick keeps hitting without growing the cache.
    app.set_adjustment("exposure", -0.5);
    app.render().unwrap();
    let warm = app.last_stage_work().expect("stage work recorded");
    assert!(warm.base_cache_hit);
    assert_eq!(warm.adjustments_passes, 1);
    assert_eq!(
        app.base_stage_cache_len(),
        1,
        "recipe-only changes must not create new base entries"
    );
    let warm_pixels = app.preview().unwrap().pixels.clone();

    // Pixel identity proof: forcing the cold path (cache cleared) for the
    // same recipe reproduces the warm output byte-for-byte.
    app.clear_preview_stage_cache();
    app.render().unwrap();
    let forced_cold = app.last_stage_work().unwrap();
    assert!(!forced_cold.base_cache_hit);
    assert_eq!(
        warm_pixels,
        app.preview().unwrap().pixels,
        "the base-stage shortcut must not change a single pixel"
    );
}

#[test]
fn color_change_reuses_base_while_geometry_change_keeps_it_too() {
    let mut app = new_app();
    app.load_bytes(gradient_png(16, 12), "grad.png").unwrap();
    app.set_adjustment("wb_temperature", 7000.0);
    app.set_adjustment("saturation", 0.3);
    app.render().unwrap();
    assert!(app.last_stage_work().unwrap().base_cache_hit);

    // A further WB/color tick is an adjustment like exposure: base stays.
    app.set_adjustment("wb_tint", -0.2);
    app.render().unwrap();
    let color_tick = app.last_stage_work().unwrap();
    assert!(color_tick.base_cache_hit);
    assert_eq!(color_tick.adjustments_passes, 1);

    // Geometry runs downstream of the base in the documented order, so it
    // also reuses the cached base while invalidating the final render.
    app.recipe.geometry = Some(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 90.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    app.mark_dirty();
    app.render().unwrap();
    let geometry_tick = app.last_stage_work().unwrap();
    assert!(geometry_tick.base_cache_hit);
    assert_eq!(app.base_stage_cache_len(), 1);
}

#[test]
fn roi_changes_get_separate_base_entries() {
    let mut app = new_app();
    app.load_bytes(gradient_png(16, 12), "grad.png").unwrap();
    // The load-time render already populated the full-frame entry.
    assert_eq!(app.base_stage_cache_len(), 1);

    // A zoom ROI is part of the base identity → its own entry.
    app.render_full([800, 600], Some([2, 2, 8, 6])).unwrap();
    assert_eq!(app.base_stage_cache_len(), 2);
    assert!(!app.last_stage_work().unwrap().base_cache_hit);

    // The same window hits its entry without growing the cache.
    app.render_full([800, 600], Some([2, 2, 8, 6])).unwrap();
    assert_eq!(app.base_stage_cache_len(), 2);
    assert!(app.last_stage_work().unwrap().base_cache_hit);

    // A different offset with equal size is a different base window.
    app.render_full([800, 600], Some([3, 2, 8, 6])).unwrap();
    assert_eq!(app.base_stage_cache_len(), 3);
}

#[test]
fn draft_drag_ticks_share_the_base_stage_with_full_renders() {
    let mut app = new_app();
    app.load_bytes(gradient_png(64, 48), "grad.png").unwrap();
    assert_eq!(app.base_stage_cache_len(), 1);

    // Slider drag: draft renders use the same (sub-draft-cap) source, so
    // the prepared base is identical to the full one and must be shared —
    // the first tick already hits the entry built by the load render.
    app.set_adjustment("exposure", 0.25);
    app.render_draft([800, 600], None).unwrap();
    let first_tick = app.last_stage_work().unwrap();
    assert!(first_tick.base_cache_hit);
    assert_eq!(app.base_stage_cache_len(), 1);

    app.set_adjustment("exposure", 0.75);
    app.render_draft([800, 600], None).unwrap();
    assert!(app.last_stage_work().unwrap().base_cache_hit);

    // Committing the drag (full-quality render) keeps hitting the same
    // base instead of rebuilding it.
    app.render().unwrap();
    assert!(app.last_stage_work().unwrap().base_cache_hit);
    assert_eq!(app.base_stage_cache_len(), 1);
}

#[test]
fn loading_a_new_source_clears_the_base_stage_cache() {
    let mut app = new_app();
    app.load_bytes(gradient_png(16, 12), "a.png").unwrap();
    app.render_full([800, 600], Some([2, 2, 8, 6])).unwrap();
    assert_eq!(app.base_stage_cache_len(), 2);

    // A new source identity invalidates every cached stage at once.
    app.load_bytes(gradient_png(12, 16), "b.png").unwrap();
    assert_eq!(
        app.base_stage_cache_len(),
        1,
        "the old source entries are gone; only b's own base remains"
    );
}

/// REVIEW-CORE-DIGEST-WIRING: pins the contract of the single RenderKey
/// construction site (the preview render). Identical inputs must produce
/// an identical digest (cache-hit contract), and the digest must separate
/// the deliberately plain in-memory preview from every attached export-
/// option set or source-action artifact hash list.
#[test]
fn render_key_digest_separates_export_options_and_source_action_hashes() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.render().unwrap();
    let key = app.render_key().expect("render key after render").clone();

    // Same inputs -> same digest: an unchanged state keeps cache identity.
    app.render().unwrap();
    assert_eq!(
        key.digest(),
        app.render_key().expect("re-rendered").digest()
    );

    // The preview is a plain in-memory frame render: no encoder options,
    // no applied source-action artifacts.
    assert!(key.export_options.is_none());
    assert!(key.source_action_artifact_hashes.is_empty());

    // Varying export options change the digest ...
    let quality_90 = key.clone().with_export_options(ExportOptions {
        quality: 90,
        ..Default::default()
    });
    let quality_60 = key.clone().with_export_options(ExportOptions {
        quality: 60,
        ..Default::default()
    });
    assert_ne!(key.digest(), quality_90.digest());
    assert_ne!(quality_90.digest(), quality_60.digest());
    // ... and equal options reproduce it exactly.
    let quality_90_again = key.clone().with_export_options(ExportOptions {
        quality: 90,
        ..Default::default()
    });
    assert_eq!(quality_90.digest(), quality_90_again.digest());

    // Varying source-action artifact hashes change the digest ...
    let repaired = key
        .clone()
        .with_source_action_hashes(["blake3:repair-artifact".to_owned()]);
    assert_ne!(key.digest(), repaired.digest());
    assert_ne!(
        repaired.digest(),
        key.clone()
            .with_source_action_hashes(["blake3:other-artifact".to_owned()])
            .digest()
    );
    // ... and equal hashes reproduce it exactly.
    assert_eq!(
        repaired.digest(),
        key.with_source_action_hashes(["blake3:repair-artifact".to_owned()])
            .digest()
    );
}

/// GUI-SRCACC-1: a changed runtime artifact must miss the prepared base and
/// change preview/export identity, while a recipe-only source-action change
/// invalidates the final preview/export identity but may reuse the same pixels.
#[test]
fn source_action_artifact_and_action_changes_invalidate_gui_cache_keys() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());
    assert!(app.error().is_none());
    let initial_cache_len = app.base_stage_cache_len();

    // A normal downstream edit still reuses the action-aware base.
    app.set_adjustment("exposure", 0.5);
    app.render().unwrap();
    assert!(app.last_stage_work().unwrap().base_cache_hit);
    // Return to a neutral adjustment before comparing raw repair pixels. The
    // first render above still proves that a downstream edit reuses the
    // action-aware base; the artifact revision below is then the only input
    // changing between the two identities.
    app.set_adjustment("exposure", 0.0);
    app.render().unwrap();
    let initial_key = app.render_key().unwrap().clone();

    // Revise both the binary artifact and the in-memory reference checksum.
    // The new checksum is a different base identity, so stale repaired pixels
    // cannot be served.
    let changed = replace_fixture_artifact(&fixture, [7, 8, 9, 255, 1, 1, 1, 1]);
    app.recipe.source_actions[0].artifact.checksum = changed.checksum();
    app.mark_dirty();
    app.render().unwrap();
    let changed_key = app.render_key().unwrap().clone();
    assert_ne!(initial_key.digest(), changed_key.digest());
    assert_ne!(
        initial_key.stage_digest(CacheStage::Export),
        changed_key.stage_digest(CacheStage::Export)
    );
    assert!(!app.last_stage_work().unwrap().base_cache_hit);
    assert_eq!(app.base_stage_cache_len(), initial_cache_len + 1);
    assert_eq!(&app.preview().unwrap().pixels[..4], &[7, 8, 9, 255]);

    // The operation kind is part of recipe identity but not the composited
    // bytes. Preview/export must still miss; the artifact-aware base may hit.
    app.recipe.source_actions[0].kind = lumina_sidecar::SourceActionKind::AiReplacement;
    app.mark_dirty();
    app.render().unwrap();
    let changed_action_key = app.render_key().unwrap().clone();
    assert_ne!(changed_key.digest(), changed_action_key.digest());
    assert_ne!(
        changed_key.stage_digest(CacheStage::Export),
        changed_action_key.stage_digest(CacheStage::Export)
    );
    assert!(app.last_stage_work().unwrap().base_cache_hit);
}
