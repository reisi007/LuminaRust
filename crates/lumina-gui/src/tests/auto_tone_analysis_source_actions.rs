//! AUTO-TONE-ANALYSIS-INPUT-8: the **SourceActions half** of the Auto-Tone
//! analysis domain, and what a reload does when that half cannot be built.
//!
//! The owner decision `AUTO-DOMAIN` (2026-10-02) puts SourceActions *before*
//! the Auto-Tone measurement: Auto evaluates the picture the user is editing,
//! retouch included. `auto_tone_analysis_domain.rs` proves the Crop half; this
//! module proves the retouch half, through the real production entry points
//! (`LuminaApp::auto_tone` and the reload freshness check) and against real
//! files — source, sidecar and `.lumina.zdata` bundle, never injected runtime
//! state.
//!
//! The two halves of the resolver are pinned together on purpose. "Not fresh"
//! without a loud render would be a silent stale state; a loud render without
//! "not fresh" would leave persisted auto values alive on a domain that cannot
//! be validated.

use super::auto_tone_analysis_domain::{six_values, split_luminance_png, SIX_KEYS};
use super::*;

/// Writes `split_luminance_png()` as a real file, lets the production app create
/// a source-valid sidecar, and — when `retouch` is set — attaches one **real**
/// persisted repair region that overwrites the **right half** (the bright bands
/// `140|235`) with a uniform dark patch.
///
/// Returns the source path. Every artifact is resolved from the file system on
/// the next open, so the caller drives the production resolution path.
fn retouched_split_fixture(directory: &std::path::Path, retouch: bool) -> std::path::PathBuf {
    use lumina_sidecar::{
        append_repair_region, load_sidecar, save_sidecar, sidecar_path_for, RepairRegionArtifact,
        SourceActionArtifactRef, SourceActionKind, SourceActionSpec, SOURCE_ACTION_VERSION,
    };

    let source = directory.join("split.png");
    std::fs::write(&source, split_luminance_png()).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.error().is_none(), "the fixture source must decode");
    app.save_sidecar();
    assert!(app.error().is_none(), "the fixture sidecar must save");
    if !retouch {
        return source;
    }

    let (width, height) = (32u32, 16u32);
    let region: Vec<u16> = (0..height)
        .flat_map(|_| (0..width).map(move |x| if x >= width / 2 { u16::MAX } else { 0 }))
        .collect();
    let replacement: Vec<u8> = (0..height)
        .flat_map(|_| (0..width).flat_map(|_| [20u8, 20, 20, 255]))
        .collect();
    let artifact = RepairRegionArtifact {
        id: "repair-1".into(),
        width,
        height,
        region,
        replacement,
    };
    append_repair_region(&lumina_sidecar::zdata_path_for(&source), artifact.clone()).unwrap();

    let sidecar_path = sidecar_path_for(&source);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: SourceActionArtifactRef {
            id: "repair-1".into(),
            relative_path: lumina_sidecar::zdata_path_for(&source)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            checksum: artifact.checksum(),
        },
    }];
    save_sidecar(&sidecar_path, &document).unwrap();
    source
}

/// **The retouch is part of the measured domain.** The same four-band source,
/// once with a repair region over its bright right half and once without, must
/// produce two different Auto value sets — and the direction is fixed by the
/// fixture: the patched half is now dark, so Auto needs **more** exposure.
///
/// This is the GUI half of the SourceActions → AutoAnalysis order. It fails if
/// `LuminaApp::auto_analysis_frame_for` hands `auto_analysis_frame` an empty
/// artifact list instead of the resolved ones — the exact regression a second,
/// untested copy of the resolver could introduce.
///
/// MUTATION: passing `&[]` instead of `resolved.artifacts()` in
/// `auto_analysis_frame_for` turns this red (both runs become equal).
#[test]
fn a_persisted_source_action_is_part_of_the_gui_analysis_domain() {
    let clean_dir = tempfile::tempdir().unwrap();
    let retouched_dir = tempfile::tempdir().unwrap();

    let mut clean = new_app();
    let clean_source = retouched_split_fixture(clean_dir.path(), false);
    open_and_decode(&mut clean, clean_source.display().to_string());
    assert!(
        clean.error().is_none(),
        "{}",
        clean.error().unwrap_or_default()
    );
    clean.auto_tone().unwrap();
    let clean_values = six_values(&clean);

    let mut retouched = new_app();
    let retouched_source = retouched_split_fixture(retouched_dir.path(), true);
    open_and_decode(&mut retouched, retouched_source.display().to_string());
    assert!(
        retouched.error().is_none(),
        "{}",
        retouched.error().unwrap_or_default()
    );
    assert_eq!(
        retouched.recipe().source_actions.len(),
        1,
        "premise: the reloaded recipe really carries the repair region"
    );
    retouched.auto_tone().unwrap();
    let retouched_values = six_values(&retouched);

    // MEASURED (target luminance 0.5) and pinned exactly, so a domain that
    // drifts elsewhere fails on a number:
    //
    //   clean     (bands 10|60|140|235, un-retouched)
    //     [0.34894830882107136, -0.08977777777777773, 0.030078124999999956,
    //      -0.008984375000000003, -0.12734374999999998, 0.04843750000000002]
    //   retouched (right half -> uniform 20,20,20)
    //     [2.642447995381916, 0.8499332443257677, 0.6370464672183322,
    //      -0.008984375000000003, 0.24375000000000002, 0.3609375]
    //
    // The `clean` row is byte-identical to the un-cropped row of
    // `a_crop_changes_the_auto_tone_values_and_the_direction_follows_the_brightness`,
    // which is the cross-check that the file-backed open path and the
    // in-memory path measure the same domain.
    assert_eq!(
        clean_values,
        vec![
            0.34894830882107136,
            -0.08977777777777773,
            0.030078124999999956,
            -0.008984375000000003,
            -0.12734374999999998,
            0.04843750000000002,
        ],
        "the un-retouched Auto values are pinned"
    );
    assert_eq!(
        retouched_values,
        vec![
            2.642447995381916,
            0.8499332443257677,
            0.6370464672183322,
            -0.008984375000000003,
            0.24375000000000002,
            0.3609375,
        ],
        "the retouched Auto values are pinned"
    );
    assert_ne!(
        clean_values, retouched_values,
        "AUTO-TONE-ANALYSIS-INPUT-8: a persisted retouch must be part of the Auto-Tone domain, so \
         repairing the bright half has to change the six Auto values"
    );
    assert!(
        retouched_values[0] > clean_values[0],
        "the repaired half is dark, so Auto must ask for more exposure: {} vs {}",
        retouched_values[0],
        clean_values[0]
    );
}

/// **The `.ok()` at the reload freshness check is not a silent pass.** A
/// candidate recipe whose source actions cannot be resolved makes the analysis
/// frame unbuildable, and the reload path must then (a) treat the Auto-Tone
/// state as **not fresh** — the conservative direction, the values are
/// recomputed — and (b) still report the underlying bundle problem loudly
/// through the authoritative render.
///
/// Both halves are asserted here because neither alone is enough: without (a) a
/// stale auto state would survive a load it cannot be validated against, and
/// without (b) the `.ok()` would be exactly the silent drop it claims not to be.
/// `gui_open_failure_clears_preview_instead_of_showing_recipe_only_pixels`
/// proves (b) for a recipe **without** auto state, so it cannot see the `.ok()`.
///
/// MUTATION: letting a failed analysis frame count as valid (`map_or(true, …)`
/// instead of `is_some_and(…)`) turns this red and nothing else.
#[test]
fn a_reload_whose_analysis_frame_cannot_be_built_is_not_fresh_and_still_reports_the_bundle() {
    use super::source_actions::{action_fixture, ActionFixtureMode};
    use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for};

    let fixture = action_fixture(ActionFixtureMode::MissingBundle);
    // A complete Auto-Tone state on top of the broken source action: six
    // sliders, six mirrors, a fingerprint and the enable flag. Without this the
    // "not fresh" half would hold vacuously.
    let sidecar_path = sidecar_path_for(&fixture.source);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    {
        let recipe = &mut document.virtual_copies[0].recipe;
        recipe.auto_features.enable_auto_tone = true;
        for key in SIX_KEYS {
            recipe.adjustments.insert(key.into(), 0.125);
        }
        for mirror in [
            &mut recipe.auto_features.auto_exposure,
            &mut recipe.auto_features.auto_contrast,
            &mut recipe.auto_features.auto_whites,
            &mut recipe.auto_features.auto_blacks,
            &mut recipe.auto_features.auto_highlights,
            &mut recipe.auto_features.auto_shadows,
        ] {
            *mirror = Some(0.125);
        }
        recipe.auto_features.analysis_fingerprint = Some(lumina_sidecar::AnalysisFingerprint {
            algorithm: lumina_stages::auto_tone::FINGERPRINT_ALGORITHM.to_string(),
            version: "1".into(),
            input_fingerprint: "blake3:premise".into(),
            extras: BTreeMap::new(),
        });
    }
    save_sidecar(&sidecar_path, &document).unwrap();

    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());

    // (a) not fresh: the mirrors are cleared, so `auto_tone_is_fresh` is false
    // for ANY input fingerprint — an unbuildable domain may never keep the
    // persisted auto values alive.
    assert!(
        app.recipe().auto_features.auto_exposure.is_none(),
        "an analysis frame that cannot be built must not leave the auto state fresh"
    );
    assert!(
        !lumina_stages::auto_tone::auto_tone_is_fresh(app.recipe(), "blake3:premise"),
        "the reloaded auto state must not count as fresh"
    );
    // (b) loud: the authoritative render names the missing bundle.
    let error = app
        .error()
        .expect("the bundle problem must be reported, not swallowed by `.ok()`");
    assert!(
        error.contains("source-action bundle") || error.contains("source action"),
        "the render must name the underlying bundle problem: {error}"
    );
}
