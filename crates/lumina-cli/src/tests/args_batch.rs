use super::*;
use lumina_sidecar::Preset;

#[test]
fn parses_process_arguments() {
    let cli = Cli::try_parse_from([
        "lumina",
        "process",
        "--input",
        "a.png",
        "--output",
        "b.webp",
        "--exposure",
        "1",
        "--highlights=-0.25",
        "--shadows",
        "0.4",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Command::Process(ProcessArgs {
            exposure: Some(1.0),
            highlights: Some(-0.25),
            shadows: Some(0.4),
            ..
        })
    ));
}

/// REVIEW-CLI-N6: an uncommitted stage must vanish completely on drop and
/// a committed stage must publish exactly the staged bytes.
#[test]
fn staged_artifact_cleans_up_without_commit_and_commits_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("out.png");

    // Uncommitted stages disappear on drop — nothing partial remains.
    {
        let staged = StagedArtifact::stage(&target, b"first").unwrap();
        assert!(!target.exists(), "staging must not take the target name");
        assert!(staged.temporary.path().is_file());
    }
    assert_eq!(
        fs::read_dir(directory.path()).unwrap().count(),
        0,
        "dropped stage must leave no temporary residue"
    );

    // Commit publishes exactly the staged bytes under the target name.
    let staged = StagedArtifact::stage(&target, b"payload").unwrap();
    staged.commit().unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"payload");
    assert_eq!(
        fs::read_dir(directory.path()).unwrap().count(),
        1,
        "commit must rename, not copy: only the target remains"
    );
}

/// REVIEW-CLI-N6: staging into a nonexistent parent fails at stage time —
/// i.e. before the caller could mutate any sidecar in its sequence.
#[test]
fn staged_artifact_fails_cleanly_for_missing_target_directory() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("missing").join("out.png");
    let error = StagedArtifact::stage(&target, b"x").unwrap_err();
    assert!(
        matches!(error, CliError::Io { .. }),
        "unexpected error shape: {error:?}"
    );
}

#[test]
fn export_accepts_update_masks_before_export() {
    let cli = Cli::try_parse_from([
        "lumina",
        "export",
        "--input",
        "a.png",
        "--output",
        "b.png",
        "--update-masks",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Command::Export(ExportArgs {
            update_masks: true,
            ..
        })
    ));
}

#[test]
fn recognizes_supported_raw_extensions() {
    for extension in ["ARW", "crw", "pef", "3fr", "x3f"] {
        assert!(is_raw_path(Path::new(&format!("photo.{extension}"))));
    }
}

/// R2-CLI-01 drift guard: BOTH predicates must accept every RAW extension
/// exported by `lumina_raw` — the batch collector and the decode router
/// previously disagreed (batch silently skipped 9 of 18 formats).
#[test]
fn batch_collection_and_decode_routing_agree_on_every_raw_extension() {
    for extension in lumina_raw::RAW_EXTENSIONS {
        let path_string = format!("photo.{extension}");
        let path = Path::new(&path_string);
        assert!(
            is_raw_path(path),
            "`is_raw_path` must accept RAW extension `{extension}`"
        );
        assert!(
            has_image_extension(path),
            "`has_image_extension` (batch collection) must accept RAW extension `{extension}`"
        );
    }
    // Non-image names stay out of the batch.
    for foreign in ["notes.txt", "archive.zip", "x.lumina.json", "noext"] {
        assert!(!has_image_extension(Path::new(foreign)), "{foreign}");
    }
}

#[test]
fn rejects_identical_and_alias_paths_before_processing() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    fs::write(&input, [1, 2, 3]).unwrap();
    assert!(reject_same_path(&input, &input).is_err());
    assert!(reject_same_path(&input, &directory.path().join("./input.png")).is_err());
    assert!(reject_same_path(&input, &directory.path().join("input.png")).is_err());
}

#[test]
fn changed_source_is_rejected_without_overwriting_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    process(ProcessArgs {
        input: input.clone(),
        output: output.clone(),
        preset: None,
        exposure: None,
        contrast: None,
        whites: None,
        blacks: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap();
    let changed = ImageFrame::new(1, 1, vec![21, 30, 40, 255]).unwrap();
    fs::write(&input, changed.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let sentinel = fs::read(&output).unwrap();
    let error = process(ProcessArgs {
        input,
        output: output.clone(),
        preset: None,
        exposure: None,
        contrast: None,
        whites: None,
        blacks: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap_err();
    assert!(error.to_string().contains("source changed"));
    assert_eq!(fs::read(output).unwrap(), sentinel);
}

#[test]
fn invalid_adjustment_and_unknown_key_are_cli_errors_without_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let invalid = process(ProcessArgs {
        input: input.clone(),
        output: output.clone(),
        preset: None,
        exposure: Some(f64::INFINITY),
        contrast: None,
        whites: None,
        blacks: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap_err();
    assert!(invalid.to_string().contains("invalid exposure"));
    assert!(!output.exists());

    let preset_path = directory.path().join("unknown.json");
    let preset = Preset {
        id: "unknown".into(),
        name: "Unknown".into(),
        recipe: lumina_sidecar::EditRecipe {
            adjustments: BTreeMap::from([("clarity".into(), 0.5)]),
            ..Default::default()
        },
        extras: BTreeMap::new(),
    };
    fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
    let unknown = process(ProcessArgs {
        input,
        output: output.clone(),
        preset: Some(preset_path),
        exposure: None,
        contrast: None,
        whites: None,
        blacks: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap_err();
    assert!(unknown
        .to_string()
        .contains("unsupported adjustment `clarity`"));
    assert!(!output.exists());
}

#[test]
fn cli_rejects_non_finite_and_out_of_range_adjustments() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    for (name, values) in [
        (
            "exposure",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -10.1, 10.1],
        ),
        (
            "contrast",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
        (
            "whites",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
        (
            "blacks",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
        (
            "highlights",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
        (
            "shadows",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
    ] {
        for value in values {
            let output = directory.path().join(format!("{name}-{value:?}.png"));
            let error = process(ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: (name == "exposure").then_some(value),
                contrast: (name == "contrast").then_some(value),
                whites: (name == "whites").then_some(value),
                blacks: (name == "blacks").then_some(value),
                highlights: (name == "highlights").then_some(value),
                shadows: (name == "shadows").then_some(value),
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            })
            .unwrap_err();
            assert!(error.to_string().contains(&format!("invalid {name}")));
            assert!(!output.exists());
        }
    }
}

#[test]
fn cli_accepts_both_adjustment_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    for (name, values) in [
        ("exposure", [-10.0, 10.0]),
        ("contrast", [-1.0, 1.0]),
        ("whites", [-1.0, 1.0]),
        ("blacks", [-1.0, 1.0]),
        ("highlights", [-1.0, 1.0]),
        ("shadows", [-1.0, 1.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            process(ProcessArgs {
                input: input.clone(),
                output: directory.path().join(format!("{name}-{index}.png")),
                preset: None,
                exposure: (name == "exposure").then_some(value),
                contrast: (name == "contrast").then_some(value),
                whites: (name == "whites").then_some(value),
                blacks: (name == "blacks").then_some(value),
                highlights: (name == "highlights").then_some(value),
                shadows: (name == "shadows").then_some(value),
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            })
            .unwrap();
        }
    }
}

#[test]
fn preset_process_and_inspect_use_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.webp");
    let preset_path = directory.path().join("preset.json");
    let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let preset = Preset {
        id: "test".into(),
        name: "Bright".into(),
        recipe: lumina_sidecar::EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
            ..lumina_sidecar::EditRecipe::default()
        },
        extras: BTreeMap::new(),
    };
    fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
    process(ProcessArgs {
        input: input.clone(),
        output: output.clone(),
        preset: Some(preset_path),
        exposure: Some(0.0),
        contrast: None,
        whites: None,
        blacks: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap();
    assert!(output.exists());
    let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        sidecar.virtual_copies[0].recipe.adjustments["exposure"],
        0.0
    );
    assert_eq!(sidecar.virtual_copies[0].history.len(), 1);
    inspect(InspectArgs { input, json: false }).unwrap();
}

/// R2-CLI-03: `inspect --json` reports the machine-readable status —
/// sidecar state and every virtual copy incl. auto-tone/matching values.
#[test]
fn inspect_json_reports_sidecar_status_and_virtual_copies() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 42);
    // No sidecar yet → "missing" with the default copy.
    let missing = Cli::try_parse_from(["lumina", "inspect", &input.display().to_string()]).unwrap();
    assert!(matches!(
        missing.command,
        Command::Inspect(InspectArgs { json: false, .. })
    ));
    inspect(InspectArgs {
        input: input.clone(),
        json: true,
    })
    .unwrap();

    // With a sidecar the JSON path succeeds for the valid state too (the
    // payload itself goes to stdout; here we pin that both states run).
    let bytes = fs::read(&input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    inspect(InspectArgs { input, json: true }).unwrap();
}

/// R2-CLI-01 end-to-end guard: batch collection must FIND every RAW
/// extension the SOLL lists. The fixture files are synthetic (garbage
/// payloads are fine — `--dry-run` never decodes), which keeps the test
/// focused on exactly the regression: the old private 9-extension copy of
/// `has_image_extension` silently skipped RAF/ORF/etc., so no status files
/// would have been written for them.
#[test]
fn batch_finds_every_supported_raw_extension_in_a_directory_tree() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let raw_extensions = [
        "arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2", "crw", "pef", "srw", "3fr", "iiq",
        "rwl", "mos", "erf", "kdc", "x3f",
    ];
    for (index, extension) in raw_extensions.iter().enumerate() {
        fs::write(
            src.join(format!("IMG_{index:04}.{extension}")),
            b"synthetic",
        )
        .unwrap();
    }
    // A non-image file must stay ignored.
    fs::write(src.join("notes.txt"), b"ignore me").unwrap();

    let out = directory.path().join("out");
    batch(BatchArgs {
        input: src,
        output: out.clone(),
        jobs: 1,
        retry: 0,
        resume: false,
        dry_run: true,
        update_masks: false,
        force_render: false,
        json: false,
        format: "png".into(),
        quality: 90,
        virtual_copy: None,
        mask_policy: CliMaskPolicy::Warn,
        write_metadata: false,
    })
    .expect("dry-run batch over synthetic RAW fixtures must succeed");

    let mut statuses: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    statuses.sort();
    assert_eq!(
        statuses.len(),
        raw_extensions.len(),
        "every RAW extension must be collected: {statuses:?}"
    );
    for index in 0..raw_extensions.len() {
        assert!(
            statuses
                .iter()
                .any(|name| name.starts_with(&format!("IMG_{index:04}."))),
            "missing status file for IMG_{index:04}: {statuses:?}"
        );
    }
    assert!(!out.join("notes.txt.status.json").exists());
}
