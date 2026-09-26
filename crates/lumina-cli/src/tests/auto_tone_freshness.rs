//! AUTO-TONE-CLI-6: the `regenerate` freshness predicate becomes honest.
//!
//! A recipe written by `process --auto-tone` now carries the full AUTO-TONE-2
//! contract, so `lumina regenerate --module auto-tone` **without** `--force`
//! reports `skipped`/`fresh` instead of overwriting it (the report shape is
//! pinned end-to-end in `tests/auto_tone_e2e.rs`). Both directions are pinned
//! here: fresh → skipped, a deliberately broken contract (a removed slider or
//! mirror) or a non-matching fingerprint → regenerated. A regeneration never
//! adopts a user value as the auto value.

use super::*;

/// The six auto values of the [`auto_tone_frame`] fixture at the default
/// `target_luminance = 0.5` (same golden as in `auto_tone_process`).
const REGENERATED_AUTO_VALUES: [f64; 6] = [
    0.881_058_927_276_492_6,
    0.653_326_018_378_823_3,
    0.442_382_812_5,
    -0.010_742_187_5,
    0.163_867_187_500_000_07,
    0.167_773_437_5,
];

/// The central pin of AUTO-TONE-CLI-6: after one `process --auto-tone` the
/// shared freshness predicate reports the recipe fresh, so the collective
/// `regenerate` skips it and the sidecar stays byte-identical. A second,
/// divergent writer would fail this test — it could not satisfy all six
/// sliders, all six mirrors and the fingerprint.
#[test]
fn a_process_auto_tone_recipe_is_fresh_for_the_collective_regenerate() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "fresh.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});
    let path = sidecar_path_for(&input);
    let before = fs::read(&path).unwrap();

    let recipe = load_sidecar(&path).unwrap().virtual_copies[0]
        .recipe
        .clone();
    assert!(
        auto_tone_is_fresh(
            &recipe,
            &auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
        ),
        "the shared predicate must report a `process --auto-tone` recipe fresh"
    );

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a fresh recipe must not be rewritten by the collective regenerate"
    );
}

/// The freshness predicate itself, in every branch, on the real recipe.
#[test]
fn freshness_is_exactly_the_documented_predicate() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "predicate.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.exposure = Some(3.0);
    });
    let path = sidecar_path_for(&input);
    let fingerprint = auto_tone_input_fingerprint(&auto_tone_frame(), 0.5);

    // Fresh even though `exposure` was user-overridden: the predicate asks for
    // presence, not for value equality — regenerating would destroy the
    // override.
    let recipe = load_sidecar(&path).unwrap().virtual_copies[0]
        .recipe
        .clone();
    assert_eq!(recipe.adjustments.get("exposure").copied(), Some(3.0));
    assert!(auto_tone_is_fresh(&recipe, &fingerprint));

    // A removed slider is a deliberately changed contract.
    for key in SLIDER_KEYS {
        let mut broken = recipe.clone();
        broken.adjustments.remove(key);
        assert!(
            !auto_tone_is_fresh(&broken, &fingerprint),
            "a recipe without the `{key}` slider is stale"
        );
    }

    // A removed mirror likewise, for each of the six.
    for (index, name) in MIRROR_FIELDS.iter().enumerate() {
        let mut broken = recipe.clone();
        match index {
            0 => broken.auto_features.auto_exposure = None,
            1 => broken.auto_features.auto_contrast = None,
            2 => broken.auto_features.auto_whites = None,
            3 => broken.auto_features.auto_blacks = None,
            4 => broken.auto_features.auto_highlights = None,
            5 => broken.auto_features.auto_shadows = None,
            _ => unreachable!(),
        }
        assert!(
            !auto_tone_is_fresh(&broken, &fingerprint),
            "a recipe without the `{name}` mirror is stale"
        );
    }

    // A non-matching fingerprint and a disabled module are stale as well.
    assert!(!auto_tone_is_fresh(&recipe, "blake3:some-other-frame"));
    let mut disabled = recipe.clone();
    disabled.auto_features.enable_auto_tone = false;
    assert!(!auto_tone_is_fresh(&disabled, &fingerprint));
}

/// A deliberately removed slider makes the recipe stale and the collective
/// `regenerate` completes it again.
#[test]
fn a_removed_slider_makes_the_recipe_stale_and_is_regenerated() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "removed-slider.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0]
        .recipe
        .adjustments
        .remove("blacks");
    save_sidecar(&path, &document).unwrap();
    assert!(!auto_tone_is_fresh(
        &load_sidecar(&path).unwrap().virtual_copies[0].recipe,
        &auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
    ));

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    assert!(
        recipe.adjustments.contains_key("blacks"),
        "the collective regenerate must restore the missing slider"
    );
    let mirrors = mirrored_values(recipe).expect("all six mirrors stay complete");
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            REGENERATED_AUTO_VALUES[index].to_bits(),
            "mirror `{}` must be recomputed",
            MIRROR_FIELDS[index]
        );
    }
    assert!(auto_tone_is_fresh(
        recipe,
        &auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
    ));
}

/// A removed mirror makes the recipe stale and the collective `regenerate`
/// completes it again.
#[test]
fn a_removed_mirror_makes_the_recipe_stale_and_is_regenerated() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "removed-mirror.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0].recipe.auto_features.auto_shadows = None;
    save_sidecar(&path, &document).unwrap();

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    let mirrors = mirrored_values(recipe).expect("the mirror must be restored");
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            REGENERATED_AUTO_VALUES[index].to_bits(),
            "mirror `{}` must be recomputed, not invented",
            MIRROR_FIELDS[index]
        );
    }
}

/// A non-matching fingerprint makes the recipe stale and the collective
/// `regenerate` regenerates it.
#[test]
fn a_non_matching_fingerprint_makes_the_recipe_stale_and_is_regenerated() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "stale-fp.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});
    let path = sidecar_path_for(&input);
    let mut document = load_sidecar(&path).unwrap();
    document.virtual_copies[0]
        .recipe
        .auto_features
        .analysis_fingerprint
        .as_mut()
        .unwrap()
        .input_fingerprint = "blake3:not-this-frame".into();
    save_sidecar(&path, &document).unwrap();

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    assert!(auto_tone_is_fresh(
        recipe,
        &auto_tone_input_fingerprint(&auto_tone_frame(), 0.5)
    ));
    let mirrors = mirrored_values(recipe).unwrap();
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            REGENERATED_AUTO_VALUES[index].to_bits(),
            "mirror `{}` must be recomputed for the current frame",
            MIRROR_FIELDS[index]
        );
    }
}

/// A user override survives the collective regenerate, and the mirrors keep
/// the auto values — the regenerate never adopts the user value.
#[test]
fn a_user_override_survives_the_collective_regenerate() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "override.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.exposure = Some(2.5);
    });
    let path = sidecar_path_for(&input);

    regenerate(regenerate_args(&input, Vec::new())).unwrap();

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    assert_eq!(
        recipe.adjustments.get("exposure").copied(),
        Some(2.5),
        "a fresh recipe with a user override must not be silently regenerated"
    );
    assert_eq!(
        mirrored_values(recipe).unwrap()[0].unwrap().to_bits(),
        REGENERATED_AUTO_VALUES[0].to_bits(),
        "the mirror keeps the auto value, not the user value"
    );
}

/// An **explicit** `--module auto-tone` regenerates even a fresh recipe and
/// derives the values from the frame again — the user values are replaced, and
/// the new auto value is the recomputed one, never the user's.
#[test]
fn an_explicit_module_run_recomputes_and_never_adopts_the_user_value() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "forced.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.exposure = Some(2.5);
        args.whites = Some(0.9);
    });
    let path = sidecar_path_for(&input);

    regenerate(regenerate_args(&input, vec![RegenerateModule::AutoTone])).unwrap();

    let recipe = &load_sidecar(&path).unwrap().virtual_copies[0].recipe;
    let sliders = slider_values(recipe);
    let mirrors = mirrored_values(recipe).unwrap();
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        assert_eq!(
            sliders[index].to_bits(),
            REGENERATED_AUTO_VALUES[index].to_bits(),
            "`{key}` must hold the recomputed auto value after an explicit regeneration"
        );
        assert_eq!(
            mirrors[index].unwrap().to_bits(),
            REGENERATED_AUTO_VALUES[index].to_bits(),
            "mirror `{}` must document the recomputed auto value",
            MIRROR_FIELDS[index]
        );
    }
    assert!(
        !sliders
            .iter()
            .any(|value| value.to_bits() == 2.5f64.to_bits()),
        "the explicit exposure value must be gone from the regenerated recipe"
    );
    assert!(
        !sliders
            .iter()
            .any(|value| value.to_bits() == 0.9f64.to_bits()),
        "the explicit whites value must be gone from the regenerated recipe"
    );
}
