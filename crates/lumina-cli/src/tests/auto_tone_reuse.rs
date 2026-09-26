//! AUTO-TONE-CLI-6: reuse is all-or-nothing.
//!
//! The persisted Auto-Tone values are reused only when the analysis
//! fingerprint matches **and** all six mirrors are present; otherwise all six
//! are recomputed. A partial (e.g. 2-of-6) state can neither be created nor
//! persisted. The reuse branch reads the **mirrors** only, so a user value in
//! `recipe.adjustments` can never be adopted as an auto value.

use super::*;
use crate::auto_tone_cli::{apply_auto_tone_result, AutoToneSource, PersistedAutoTone};

/// The real auto values of the fixture at `target_luminance = 0.5` (identical
/// to the `AUTO_VALUES` golden of `auto_tone_process`, restated here so the
/// reuse test states its own expectation).
const TRUE_AUTO_VALUES: [f64; 6] = [
    0.881_058_927_276_492_6,
    0.653_326_018_378_823_3,
    0.442_382_812_5,
    -0.010_742_187_5,
    0.163_867_187_500_000_07,
    0.167_773_437_5,
];

/// Sentinel mirror values that Auto-Tone could never compute for this fixture.
/// If a run leaves them in place, the values were **reused**, not recomputed.
const SENTINEL: [f64; 6] = [0.6, -0.6, 0.6, -0.6, 0.6, -0.6];

/// Writes a complete but *foreign* Auto-Tone state: the six sentinel mirrors,
/// the matching analysis fingerprint and the six sentinel sliders.
fn write_sentinel_state(input: &Path, fingerprint_matches: bool) {
    let path = sidecar_path_for(input);
    let mut document = load_sidecar(&path).unwrap();
    let recipe = &mut document.virtual_copies[0].recipe;
    let real = auto_tone_input_fingerprint(&auto_tone_frame(), 0.5);
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        recipe.adjustments.insert((*key).into(), SENTINEL[index]);
    }
    let auto = &mut recipe.auto_features;
    auto.enable_auto_tone = true;
    auto.target_luminance = 0.5;
    auto.auto_exposure = Some(SENTINEL[0]);
    auto.auto_contrast = Some(SENTINEL[1]);
    auto.auto_whites = Some(SENTINEL[2]);
    auto.auto_blacks = Some(SENTINEL[3]);
    auto.auto_highlights = Some(SENTINEL[4]);
    auto.auto_shadows = Some(SENTINEL[5]);
    auto.analysis_fingerprint = Some(lumina_sidecar::AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: if fingerprint_matches {
            real
        } else {
            "blake3:not-this-frame".into()
        },
        extras: std::collections::BTreeMap::new(),
    });
    save_sidecar(&path, &document).unwrap();
}

/// A complete state with a matching fingerprint is reused verbatim: the
/// persisted values are not recomputed, so the sentinels survive the run.
#[test]
fn a_complete_state_with_a_matching_fingerprint_is_reused_verbatim() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "reuse.png");
    write_sentinel_state(&input, true);
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let mirrors = mirrored_values(recipe).expect("a reused state is complete");
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            SENTINEL[index].to_bits(),
            "mirror `{}` must be reused verbatim, not recomputed",
            MIRROR_FIELDS[index]
        );
    }
    let sliders = slider_values(recipe);
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        assert_eq!(
            sliders[index].to_bits(),
            SENTINEL[index].to_bits(),
            "`{key}` must take the reused auto value"
        );
    }
}

/// Missing **any one** of the six mirrors forces a complete recomputation of
/// all six — checked for each of the six positions. A mixed state is
/// impossible.
#[test]
fn one_missing_mirror_forces_a_full_recompute_of_all_six() {
    for (missing, mirror_field) in MIRROR_FIELDS.iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let input = auto_tone_input(directory.path(), &format!("partial-{missing}.png"));
        write_sentinel_state(&input, true);
        // Drop exactly one mirror; the fingerprint still matches.
        let path = sidecar_path_for(&input);
        let mut document = load_sidecar(&path).unwrap();
        let auto = &mut document.virtual_copies[0].recipe.auto_features;
        match missing {
            0 => auto.auto_exposure = None,
            1 => auto.auto_contrast = None,
            2 => auto.auto_whites = None,
            3 => auto.auto_blacks = None,
            4 => auto.auto_highlights = None,
            5 => auto.auto_shadows = None,
            _ => unreachable!(),
        }
        save_sidecar(&path, &document).unwrap();
        assert!(
            mirrored_values(&load_sidecar(&path).unwrap().virtual_copies[0].recipe).is_none(),
            "the fixture must really be incomplete for this case"
        );

        process_auto_tone(&input, &directory.path().join("out.png"), |_| {});

        let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
        let mirrors = mirrored_values(recipe)
            .expect("a missing mirror must be recomputed — a partial state may never be persisted");
        for (index, mirror) in mirrors.iter().enumerate() {
            assert_eq!(
                mirror.unwrap().to_bits(),
                TRUE_AUTO_VALUES[index].to_bits(),
                "with `{mirror_field}` missing, ALL SIX must be recomputed",
            );
        }
        let sliders = slider_values(recipe);
        for (index, key) in SLIDER_KEYS.iter().enumerate() {
            assert_eq!(
                sliders[index].to_bits(),
                TRUE_AUTO_VALUES[index].to_bits(),
                "`{key}` must carry the recomputed value, never a mixed 2-of-6 remnant"
            );
        }
    }
}

/// A non-matching fingerprint forces a complete recomputation even though all
/// six mirrors are present.
#[test]
fn a_non_matching_fingerprint_forces_a_full_recompute() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "stale-fingerprint.png");
    write_sentinel_state(&input, false);
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let mirrors = mirrored_values(recipe).unwrap();
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            TRUE_AUTO_VALUES[index].to_bits(),
            "a stale fingerprint must recompute all six, not keep the stale ones"
        );
    }
    assert_eq!(
        recipe
            .auto_features
            .analysis_fingerprint
            .as_ref()
            .unwrap()
            .input_fingerprint,
        auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
    );
}

/// The historic two-slider artifact (mirrored `exposure`/`contrast` only) is
/// completed to the full six-slider contract by the next `--auto-tone` run.
#[test]
fn the_historic_two_slider_artifact_is_completed_to_the_full_set() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "two-of-six.png");
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    let recipe = &mut document.virtual_copies[0].recipe;
    recipe.adjustments.insert("exposure".into(), 0.1);
    recipe.adjustments.insert("contrast".into(), 0.1);
    let auto = &mut recipe.auto_features;
    auto.enable_auto_tone = true;
    auto.auto_exposure = Some(0.1);
    auto.auto_contrast = Some(0.1);
    auto.analysis_fingerprint = Some(lumina_sidecar::AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: auto_tone_input_fingerprint(&auto_tone_frame(), 0.5),
        extras: std::collections::BTreeMap::new(),
    });
    save_sidecar(&path, &document).unwrap();

    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    let mirrors = mirrored_values(recipe).expect("the 2-of-6 state must be completed");
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            TRUE_AUTO_VALUES[index].to_bits(),
            "the two-slider remnant must not survive as a mixed state"
        );
    }
    assert_eq!(slider_values(recipe).len(), 6);
}

/// A changed `--target-luminance` binds the fingerprint, so the same frame is
/// not reused under a different target.
#[test]
fn a_changed_target_luminance_invalidates_the_persisted_values() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "target-change.png");
    write_sentinel_state(&input, true);
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.target_luminance = 0.8;
    });

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let mirrors = mirrored_values(recipe).unwrap();
    assert_eq!(
        recipe.auto_features.target_luminance, 0.8,
        "the new target must be persisted"
    );
    assert_eq!(
        recipe
            .auto_features
            .analysis_fingerprint
            .as_ref()
            .unwrap()
            .input_fingerprint,
        auto_tone_input_fingerprint(&auto_tone_frame(), 0.8),
        "the fingerprint is bound to the target luminance"
    );
    // The six values of the new target, pinned exactly (only `exposure` moves;
    // the median is the only target-dependent term).
    const TARGET_08_VALUES: [f64; 6] = [
        1.559_130_832_389_130_2,
        0.653_326_018_378_823_3,
        0.442_382_812_5,
        -0.010_742_187_5,
        0.163_867_187_500_000_07,
        0.167_773_437_5,
    ];
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            TARGET_08_VALUES[index].to_bits(),
            "the target luminance is part of the fingerprint: a new target must recompute all six"
        );
        assert_ne!(
            mirror.unwrap().to_bits(),
            SENTINEL[index].to_bits(),
            "the reused sentinel must be gone"
        );
    }
    assert_eq!(
        recipe.auto_features.auto_exposure.unwrap().to_bits(),
        1.559_130_832_389_130_2f64.to_bits(),
        "the new target changes the exposure term"
    );
}

/// The reuse branch never reads `recipe.adjustments`: a user value there can
/// not become an auto value even when everything else is reusable.
#[test]
fn reuse_reads_mirrors_only_so_a_user_value_cannot_become_an_auto_value() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "user-value.png");
    // Complete, matching state whose *sliders* carry a user value and whose
    // mirrors carry the true auto values.
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    let recipe = &mut document.virtual_copies[0].recipe;
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        recipe
            .adjustments
            .insert((*key).into(), 0.9 - index as f64 * 0.1);
    }
    let auto = &mut recipe.auto_features;
    auto.enable_auto_tone = true;
    for (index, value) in TRUE_AUTO_VALUES.iter().enumerate() {
        match index {
            0 => auto.auto_exposure = Some(*value),
            1 => auto.auto_contrast = Some(*value),
            2 => auto.auto_whites = Some(*value),
            3 => auto.auto_blacks = Some(*value),
            4 => auto.auto_highlights = Some(*value),
            5 => auto.auto_shadows = Some(*value),
            _ => unreachable!(),
        }
    }
    auto.analysis_fingerprint = Some(lumina_sidecar::AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: auto_tone_input_fingerprint(&auto_tone_frame(), 0.5),
        extras: std::collections::BTreeMap::new(),
    });
    save_sidecar(&path, &document).unwrap();

    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    let mirrors = mirrored_values(recipe).unwrap();
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            TRUE_AUTO_VALUES[index].to_bits(),
            "the mirror must document the auto value; the user value in `adjustments` must not \
             leak into `auto_features`"
        );
    }
}

/// The writer reports its value source, and the explicit regeneration mode
/// never reuses a persisted value.
#[test]
fn the_writer_reports_its_source_and_forced_mode_always_recomputes() {
    let mut recipe = lumina_sidecar::EditRecipe::default();
    let frame = auto_tone_frame();

    // Nothing persisted: computed.
    let outcome = apply_auto_tone_result(
        &mut recipe,
        &frame,
        0.5,
        PersistedAutoTone::ReuseIfComplete,
        None,
    )
    .unwrap();
    assert_eq!(outcome.source, AutoToneSource::Computed);
    let persisted: Vec<f64> = mirrored_values(&recipe)
        .unwrap()
        .iter()
        .map(|v| v.unwrap())
        .collect();
    for (index, value) in persisted.iter().enumerate() {
        assert_eq!(value.to_bits(), TRUE_AUTO_VALUES[index].to_bits());
    }

    // Now the complete, matching state is reusable...
    let outcome = apply_auto_tone_result(
        &mut recipe,
        &frame,
        0.5,
        PersistedAutoTone::ReuseIfComplete,
        None,
    )
    .unwrap();
    assert_eq!(outcome.source, AutoToneSource::Reused);

    // ...but an explicit regeneration recomputes. Overwrite the mirrors with a
    // sentinel first: a recomputation must overwrite them again.
    recipe.auto_features.auto_whites = Some(0.6);
    let outcome = apply_auto_tone_result(
        &mut recipe,
        &frame,
        0.5,
        PersistedAutoTone::AlwaysRecompute,
        None,
    )
    .unwrap();
    assert_eq!(outcome.source, AutoToneSource::Computed);
    assert_eq!(
        recipe.auto_features.auto_whites.unwrap().to_bits(),
        TRUE_AUTO_VALUES[2].to_bits(),
        "an explicit regeneration must not reproduce a persisted value"
    );
}
