use super::*;

/// R2-CLI-10: `import` no longer accepts render-only flags that were
/// silently ignored before (`--output`, `--format`, `--quality`,
/// `--force-render`, `--virtual-copy`, `--mask-policy`).
#[test]
fn import_rejects_inherited_render_only_flags() {
    for flag in [
        "--output",
        "--format",
        "--quality",
        "--force-render",
        "--virtual-copy",
        "--mask-policy",
    ] {
        let parsed = Cli::try_parse_from([
            "lumina",
            "import",
            "--input",
            "a.png",
            flag,
            if flag == "--format" || flag == "--mask-policy" || flag == "--virtual-copy" {
                "png"
            } else if flag == "--quality" {
                "90"
            } else {
                "b.png"
            },
        ]);
        assert!(
            parsed.is_err(),
            "`lumina import {flag}` must be rejected as unknown"
        );
    }
    // The slim set still parses.
    let ok = Cli::try_parse_from(["lumina", "import", "--input", "a.png", "--json"]).unwrap();
    assert!(matches!(
        ok.command,
        Command::Import(ImportArgs { json: true, .. })
    ));
}

/// R2-CLI-09: out-of-range/non-finite develop values fail up front with
/// the allowed range in the message (mirroring MCP `lumina_edit`) — not
/// later as a generic save-time rejection.
#[test]
fn develop_rejects_out_of_range_values_before_touching_the_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 60);
    import_file(ImportArgs {
        input: input.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read_to_string(&sidecar_path).unwrap();

    for (name, value) in [
        ("exposure", 999.0),
        ("exposure", -10.1),
        ("exposure", f64::NAN),
        ("contrast", 1.5),
        ("contrast", -1.1),
        ("contrast", f64::INFINITY),
    ] {
        let error = develop(DevelopArgs {
            input: input.clone(),
            virtual_copy: None,
            exposure: (name == "exposure").then_some(value),
            contrast: (name == "contrast").then_some(value),
            treatment: None,
            profile: None,
            update_masks: false,
            migrate: false,
            json: false,
        })
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(&format!("invalid adjustment `{name}`"))
                && message.contains("outside allowed range"),
            "{name}={value} must be rejected with the range, got: {message}"
        );
        if name == "exposure" {
            assert!(message.contains("-10..=10"), "{message}");
        } else {
            assert!(message.contains("-1..=1"), "{message}");
        }
    }

    // The failed runs never mutated the sidecar.
    assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), before);

    // Boundary values stay valid and DO apply.
    develop(DevelopArgs {
        input: input.clone(),
        virtual_copy: None,
        exposure: Some(-10.0),
        contrast: Some(1.0),
        treatment: None,
        profile: None,
        update_masks: false,
        migrate: false,
        json: false,
    })
    .unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        -10.0
    );
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["contrast"],
        1.0
    );
}

/// LRPAR-G01-BASIC: `develop --treatment/--profile` roundtrips through
/// the sidecar, invalid values fail before any mutation, and `inspect`
/// reports both fields. The original image bytes are never touched.
#[test]
fn develop_treatment_profile_roundtrip_and_rejection() {
    use lumina_sidecar::{DEFAULT_DEVELOP_PROFILE, TREATMENT_BW, TREATMENT_COLOR};
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 60);
    let original_bytes = fs::read(&input).unwrap();
    import_file(ImportArgs {
        input: input.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    let sidecar_path = sidecar_path_for(&input);
    let develop_base = || DevelopArgs {
        input: input.clone(),
        virtual_copy: None,
        exposure: None,
        contrast: None,
        treatment: None,
        profile: None,
        update_masks: false,
        migrate: false,
        json: false,
    };
    // Set both fields in one run.
    let mut set = develop_base();
    set.treatment = Some(TREATMENT_BW.into());
    set.profile = Some("vivid".into());
    develop(set).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    assert_eq!(recipe.treatment(), TREATMENT_BW);
    assert_eq!(recipe.develop_profile(), "vivid");
    assert_eq!(recipe.adjustments.get("saturation"), Some(&-1.0));
    // `inspect --json` reports both fields per copy.
    inspect(InspectArgs {
        input: input.clone(),
        json: true,
    })
    .unwrap();
    // Back to color restores identity (stash roundtrip through files).
    let mut back = develop_base();
    back.treatment = Some(TREATMENT_COLOR.into());
    back.profile = Some(DEFAULT_DEVELOP_PROFILE.into());
    develop(back).unwrap();
    let document = load_sidecar(&sidecar_path).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    assert_eq!(recipe.treatment(), TREATMENT_COLOR);
    assert_eq!(recipe.develop_profile(), DEFAULT_DEVELOP_PROFILE);
    assert!(!recipe.adjustments.contains_key("saturation"));
    // Invalid values fail loudly and leave the sidecar byte-identical.
    let before = fs::read(&sidecar_path).unwrap();
    for (treatment, profile) in [
        (Some("sepia".to_string()), None),
        (None, Some("adobe-color".to_string())),
        (None, Some(String::new())),
    ] {
        let mut bad = develop_base();
        bad.treatment = treatment;
        bad.profile = profile;
        develop(bad).unwrap_err();
    }
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}
