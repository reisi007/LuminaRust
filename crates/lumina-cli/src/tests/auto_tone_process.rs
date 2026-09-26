//! AUTO-TONE-CLI-6: the `process --auto-tone` contract.
//!
//! `process --auto-tone` must persist the **full** AUTO-TONE-2 six-slider
//! contract, render those exact bytes, and let the preset and the explicit CLI
//! values win over the auto layer — with the `auto_features` mirrors keeping
//! the auto value. Normative: `feature/architecture/pipeline.md` § Auto-Tone,
//! subsection „Ein Schreibpfad, Vorrangordnung und Wiederverwendung“.

use super::*;

/// The auto values for the [`auto_tone_frame`] fixture at the default
/// `target_luminance = 0.5`, pinned as exact `f64` values (see the fixture
/// docs). They were taken from the `regenerate` writer on HEAD a992f93, which
/// already wrote all six — so this test also pins that `process --auto-tone`
/// and `regenerate --module auto-tone` compute the *same* six values.
const AUTO_VALUES: [f64; 6] = [
    0.881_058_927_276_492_6,
    0.653_326_018_378_823_3,
    0.442_382_812_5,
    -0.010_742_187_5,
    0.163_867_187_500_000_07,
    0.167_773_437_5,
];

/// One `process --auto-tone` run writes all six sliders, all six mirrors and
/// the analysis fingerprint — never a subset.
#[test]
fn process_auto_tone_writes_all_six_sliders_mirrors_and_fingerprint() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "six-set.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |_| {});

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    let sliders = slider_values(recipe);
    for (key, value) in SLIDER_KEYS.iter().zip(sliders.iter()) {
        assert_eq!(
            value.to_bits(),
            AUTO_VALUES[SLIDER_KEYS.iter().position(|k| k == key).unwrap()].to_bits(),
            "the `{key}` slider must carry the exact auto value"
        );
    }
    let mirrors = mirrored_values(recipe).expect(
        "all six mirrors must be present after one `process --auto-tone` run — a partial \
         set is a contract violation",
    );
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            AUTO_VALUES[index].to_bits(),
            "mirror `{}` must document the auto value",
            MIRROR_FIELDS[index]
        );
    }
    let auto = &recipe.auto_features;
    assert!(auto.enable_auto_tone);
    assert_eq!(auto.target_luminance, 0.5);
    let fingerprint = auto
        .analysis_fingerprint
        .as_ref()
        .expect("the analysis fingerprint must be present");
    assert_eq!(fingerprint.algorithm, "tone-rgba8-rec709");
    assert_eq!(fingerprint.version, "1");
    assert_eq!(
        fingerprint.input_fingerprint,
        auto_tone_input_fingerprint(&auto_tone_frame(), 0.5),
        "the persisted fingerprint must be the one of the current frame"
    );
}

/// Byte-level golden of the render `process --auto-tone` produces. The exact
/// RGBA bytes of the exported image — not only the sidecar JSON. The golden is
/// discriminating: it differs from the plain render of the same fixture.
#[test]
fn process_auto_tone_render_has_an_exact_byte_golden() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "golden.png");
    let output = directory.path().join("golden-out.png");
    process_auto_tone(&input, &output, |_| {});

    assert_eq!(
        hex(&rendered_pixels(&output)),
        AUTO_TONE_RENDER_GOLDEN.trim(),
        "the `process --auto-tone` render must stay byte-exact"
    );
    assert_ne!(
        AUTO_TONE_RENDER_GOLDEN.trim(),
        PLAIN_RENDER_GOLDEN.trim(),
        "the Auto-Tone render must not coincide with the plain render — otherwise the golden \
         would prove nothing"
    );
    // Readable landmarks next to the opaque blob: the four corners and the
    // centre of the 16x16 result.
    let pixels = rendered_pixels(&output);
    for (index, expected) in LANDMARK_PIXELS.iter().enumerate() {
        let offset = LANDMARK_OFFSETS[index] * 4;
        assert_eq!(
            &pixels[offset..offset + 4],
            *expected,
            "landmark pixel {index} (pixel {:#010x})",
            LANDMARK_OFFSETS[index]
        );
    }
}

/// Byte offsets (pixel indices) of the readable landmarks in the golden: the
/// four corners, the centre and the two horizontal ends of the middle row.
const LANDMARK_OFFSETS: [usize; 7] = [0, 15, 15 * 16, 16 * 16 - 1, 8 * 16 + 8, 8 * 16, 8 * 16 + 15];

/// The exact RGBA bytes of those landmarks, taken from the golden render.
const LANDMARK_PIXELS: [[u8; 4]; 7] = [
    [0x0c, 0x0c, 0x0c, 0xff],
    [0x0c, 0xff, 0x41, 0xff],
    [0x99, 0x12, 0x17, 0xff],
    [0xd7, 0xff, 0x9c, 0xff],
    [0xff, 0x0c, 0x46, 0xff],
    [0x2f, 0xd1, 0x0c, 0xff],
    [0x5b, 0x5e, 0x86, 0xff],
];

/// The same recipe rendered twice is byte-identical (the run is
/// deterministic and the sidecar commit does not perturb the pixels).
#[test]
fn process_auto_tone_render_is_reproducible() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "repro.png");
    let first = directory.path().join("first.png");
    let second = directory.path().join("second.png");
    process_auto_tone(&input, &first, |_| {});
    process_auto_tone(&input, &second, |_| {});

    assert_eq!(
        fs::read(&first).unwrap(),
        fs::read(&second).unwrap(),
        "two `process --auto-tone` runs must produce identical output bytes"
    );
}

/// Without `--auto-tone` (`render`/`export`/`batch` pass `auto_tone: false`):
/// no `auto_features` field is written, `enable_auto_tone` stays `false` and
/// the render bytes are the unchanged HEAD result.
#[test]
fn without_auto_tone_no_auto_feature_is_written_and_the_render_is_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "plain.png");
    let output = directory.path().join("plain-out.png");
    process_auto_tone(&input, &output, |args| args.auto_tone = false);

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    let auto = &recipe.auto_features;
    assert!(
        !auto.enable_auto_tone,
        "`enable_auto_tone` must stay false without `--auto-tone`"
    );
    assert!(
        mirrored_values(recipe).is_none(),
        "no auto mirror may be written without `--auto-tone`"
    );
    assert!(
        auto.analysis_fingerprint.is_none(),
        "no analysis fingerprint may be written without `--auto-tone`"
    );
    assert!(
        SLIDER_KEYS
            .iter()
            .all(|key| !recipe.adjustments.contains_key(*key)),
        "no auto slider may appear without `--auto-tone`: {:?}",
        recipe.adjustments
    );
    assert_eq!(
        hex(&rendered_pixels(&output)),
        PLAIN_RENDER_GOLDEN.trim(),
        "the render without `--auto-tone` must stay byte-identical to HEAD a992f93"
    );
}

/// The exact RGBA bytes of the `process --auto-tone` render of the
/// [`auto_tone_frame`] fixture (16x16, 1024 bytes, lowercase hex).
const AUTO_TONE_RENDER_GOLDEN: &str = include_str!("auto_tone_render_golden.txt");

/// The exact RGBA bytes of the same fixture rendered **without** `--auto-tone`
/// (i.e. the `render` path). Taken from HEAD a992f93, where this slice changes
/// nothing — `render`/`export`/`batch` pass `auto_tone: false`.
const PLAIN_RENDER_GOLDEN: &str = include_str!("plain_render_golden.txt");

/// A preset wins over the auto layer for every key it sets explicitly, and
/// the auto value survives for every key the preset is silent about. The
/// mirrors keep the auto values.
#[test]
fn the_preset_wins_where_it_speaks_and_the_auto_value_survives_elsewhere() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "preset.png");
    let preset = directory.path().join("preset.json");
    write_preset(
        &preset,
        &[("contrast", 0.5), ("blacks", 0.25), ("shadows", -0.75)],
    );
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.preset = Some(preset);
    });

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let sliders = slider_values(recipe);
    let expected = [
        AUTO_VALUES[0],
        0.5,
        AUTO_VALUES[2],
        0.25,
        AUTO_VALUES[4],
        -0.75,
    ];
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        assert_eq!(
            sliders[index].to_bits(),
            expected[index].to_bits(),
            "`{key}`: the preset value wins where the preset speaks, the auto value stays where \
             it is silent"
        );
    }
    let mirrors = mirrored_values(recipe).unwrap();
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            AUTO_VALUES[index].to_bits(),
            "mirror `{}` documents the auto value, not the preset value",
            MIRROR_FIELDS[index]
        );
    }
}

/// Explicit CLI values win last — for **all six** sliders — while the mirrors
/// keep the auto values and the recipe's adjustment values are the effective
/// ones.
#[test]
fn explicit_cli_values_win_last_for_all_six_and_keep_the_auto_mirror() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "explicit.png");
    let explicit = [1.5_f64, -0.5, 0.75, -0.25, 0.125, -0.875];
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.exposure = Some(explicit[0]);
        args.contrast = Some(explicit[1]);
        args.whites = Some(explicit[2]);
        args.blacks = Some(explicit[3]);
        args.highlights = Some(explicit[4]);
        args.shadows = Some(explicit[5]);
    });

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let sliders = slider_values(recipe);
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        assert_eq!(
            sliders[index].to_bits(),
            explicit[index].to_bits(),
            "`--{key}` must win over both the auto value and any preset"
        );
    }
    let mirrors = mirrored_values(recipe).unwrap();
    for (index, mirror) in mirrors.iter().enumerate() {
        assert_eq!(
            mirror.unwrap().to_bits(),
            AUTO_VALUES[index].to_bits(),
            "mirror `{}` keeps the AUTO value while `adjustments` carries the effective one",
            MIRROR_FIELDS[index]
        );
    }
}

/// Each of the six explicit flags wins over Auto-Tone **individually**: given
/// alone it overrides exactly its own slider and leaves the other five at their
/// auto values (there is no "auto may not be overridden" and no silent reset).
#[test]
fn each_explicit_flag_overrides_only_its_own_slider() {
    for (index, key) in SLIDER_KEYS.iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let input = auto_tone_input(directory.path(), &format!("one-flag-{key}.png"));
        process_auto_tone(
            &input,
            &directory.path().join("out.png"),
            |args| match *key {
                "exposure" => args.exposure = Some(0.5),
                "contrast" => args.contrast = Some(0.5),
                "whites" => args.whites = Some(0.5),
                "blacks" => args.blacks = Some(0.5),
                "highlights" => args.highlights = Some(0.5),
                "shadows" => args.shadows = Some(0.5),
                _ => unreachable!(),
            },
        );

        let recipe = &load_sidecar(&sidecar_path_for(&input))
            .unwrap()
            .virtual_copies[0]
            .recipe;
        let sliders = slider_values(recipe);
        assert_eq!(
            sliders[index].to_bits(),
            0.5f64.to_bits(),
            "`--{key}` must win over the auto value {}",
            AUTO_VALUES[index]
        );
        for other in 0..6 {
            if other == index {
                continue;
            }
            assert_eq!(
                sliders[other].to_bits(),
                AUTO_VALUES[other].to_bits(),
                "`{}` was not given on the command line and must stay at its auto value",
                SLIDER_KEYS[other]
            );
        }
        // The mirror of the overridden slider still documents the auto value.
        assert_eq!(
            mirrored_values(recipe).unwrap()[index].unwrap().to_bits(),
            AUTO_VALUES[index].to_bits(),
            "mirror `{}` keeps the auto value",
            MIRROR_FIELDS[index]
        );
    }
}

/// `--match-total-exposure` adds onto the **effective** `exposure` value (auto,
/// preset and explicit CLI values already applied) and touches none of the
/// other five sliders.
#[test]
fn match_total_exposure_adds_onto_the_effective_exposure_only() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "match.png");
    process_auto_tone(&input, &directory.path().join("match-out.png"), |args| {
        args.match_total_exposure = true
    });

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let auto = &recipe.auto_features;
    let matching = auto
        .matched_exposure
        .expect("the matched exposure must be persisted");
    assert!(auto.match_total_exposure);
    let sliders = slider_values(recipe);
    assert_eq!(
        sliders[0].to_bits(),
        (AUTO_VALUES[0] + matching).clamp(-10.0, 10.0).to_bits(),
        "matching must be added onto the effective exposure"
    );
    for index in 1..6 {
        assert_eq!(
            sliders[index].to_bits(),
            AUTO_VALUES[index].to_bits(),
            "`{}` must not be touched by exposure matching",
            SLIDER_KEYS[index]
        );
    }
    assert_eq!(
        auto.auto_exposure.unwrap().to_bits(),
        AUTO_VALUES[0].to_bits(),
        "the exposure mirror keeps the auto value, not the matched one"
    );
}

/// With an explicit `--exposure` the matching adds onto **that** value, not
/// onto the auto value.
#[test]
fn match_total_exposure_uses_the_explicit_exposure_as_its_base() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "match-explicit.png");
    process_auto_tone(
        &input,
        &directory.path().join("match-explicit-out.png"),
        |args| {
            args.match_total_exposure = true;
            args.exposure = Some(1.25);
        },
    );

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let auto = &recipe.auto_features;
    let matching = auto.matched_exposure.unwrap();
    let sliders = slider_values(recipe);
    assert_eq!(
        sliders[0].to_bits(),
        (1.25 + matching).clamp(-10.0, 10.0).to_bits(),
        "the matching base must be the effective (explicit) exposure"
    );
    assert_eq!(
        auto.auto_exposure.unwrap().to_bits(),
        AUTO_VALUES[0].to_bits(),
        "the mirror must still document the auto value"
    );
    for index in 1..6 {
        assert_eq!(
            sliders[index].to_bits(),
            AUTO_VALUES[index].to_bits(),
            "`{}` must not be touched by exposure matching",
            SLIDER_KEYS[index]
        );
    }
}

/// The six values remain reconstructible after an override: the mirrors are
/// the auto values, so a later regeneration can tell an auto value from a user
/// value.
#[test]
fn the_six_auto_values_stay_reconstructible_after_an_override() {
    let directory = tempfile::tempdir().unwrap();
    let input = auto_tone_input(directory.path(), "reconstructible.png");
    process_auto_tone(&input, &directory.path().join("out.png"), |args| {
        args.whites = Some(0.9);
        args.shadows = Some(-0.9);
    });

    let recipe = &load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .virtual_copies[0]
        .recipe;
    let sliders = slider_values(recipe);
    let mirrors = mirrored_values(recipe).unwrap();
    for index in 0..6 {
        let auto = mirrors[index].unwrap();
        if index == 2 || index == 5 {
            assert_eq!(
                auto.to_bits(),
                AUTO_VALUES[index].to_bits(),
                "the mirror of an overridden slider still documents the auto value"
            );
            assert_ne!(sliders[index].to_bits(), auto.to_bits());
        } else {
            assert_eq!(
                sliders[index].to_bits(),
                auto.to_bits(),
                "an untouched slider equals its auto value"
            );
        }
    }
}
