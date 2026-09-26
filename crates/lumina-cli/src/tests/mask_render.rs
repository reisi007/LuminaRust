use super::*;

#[test]
fn one_shot_mask_flags_are_consumed_and_removed_from_the_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = png_input(directory.path(), "input.png", 100);
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // Persisted artifact under the composite record id so the render is
    // warning-free once the flag is consumed.
    let tile = lumina_sidecar::MaskTile {
        mask_id: zdata_mask_tile_id("vc-original", "subject"),
        tile_x: 0,
        tile_y: 0,
        width: 2,
        height: 2,
        values: vec![65535; 4],
    };
    let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
    lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

    // develop/batch-style: persist the one-shot requests into the recipe.
    let sidecar_path = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0]
        .recipe
        .options
        .insert("update_masks".into(), "true".into());
    document.virtual_copies[0]
        .recipe
        .options
        .insert("force_render".into(), "true".into());
    save_sidecar(&sidecar_path, &document).unwrap();

    let output = directory.path().join("output.png");
    let mut warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input.clone(),
            output,
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
        },
        90,
        None,
        MaskPolicy::Warn,
        &mut warnings,
    )
    .unwrap();

    // REVIEW-CLI-MASKFLAG-1: after a successful run the consumed flags
    // must be gone from the persisted recipe — otherwise every future
    // run would re-infer despite a valid persisted mask.
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(!document.virtual_copies[0]
        .recipe
        .options
        .contains_key("update_masks"));
    assert!(!document.virtual_copies[0]
        .recipe
        .options
        .contains_key("force_render"));
}

#[test]
fn zdata_tiles_are_scoped_per_virtual_copy() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = png_input(directory.path(), "input.png", 100);
    let bytes = fs::read(&input).unwrap();
    let identity = source_identity(&input, &bytes, &frame, None).unwrap();
    let mut document = SidecarDocument::new(identity.clone(), "raster-mvp-1");
    document.virtual_copies[0].mask_library = vec![valid_mask_definition(
        "subject",
        lumina_sidecar::MaskOperation::Source,
        vec![],
        &identity,
        2,
        2,
    )];
    // Second copy with the SAME mask id — the previous keying shared one
    // matte between both copies (REVIEW-CLI-N1).
    let mut second = document.virtual_copies[0].clone();
    second.id = "vc-two".into();
    second.name = "Two".into();
    document.virtual_copies.push(second);

    // Distinct planes under the composite record ids.
    let original_tile = lumina_sidecar::MaskTile {
        mask_id: zdata_mask_tile_id("vc-original", "subject"),
        tile_x: 0,
        tile_y: 0,
        width: 2,
        height: 2,
        values: vec![0; 4],
    };
    let two_tile = lumina_sidecar::MaskTile {
        mask_id: zdata_mask_tile_id("vc-two", "subject"),
        tile_x: 0,
        tile_y: 0,
        width: 2,
        height: 2,
        values: vec![65535; 4],
    };
    let container = lumina_sidecar::ZDataContainer::new(vec![original_tile, two_tile]).unwrap();
    let zdata_path = lumina_sidecar::zdata_path_for(&input);
    lumina_sidecar::save_zdata(&zdata_path, &container).unwrap();

    let mut warnings = Vec::new();
    let planes = load_persisted_mask_planes(&document, &zdata_path, &mut warnings);
    assert!(warnings.is_empty());
    assert_eq!(planes.len(), 2);
    assert_eq!(
        planes[&("vc-original".into(), "subject".into())].values,
        vec![0; 4]
    );
    assert_eq!(
        planes[&("vc-two".into(), "subject".into())].values,
        vec![65535; 4]
    );

    // Legacy bundles that stored the plane under the plain mask id are
    // deliberately NOT picked up any more (pre-MVP schema decision): a
    // silently shared matte is exactly what the fix removes.
    let legacy = lumina_sidecar::MaskTile {
        mask_id: "subject".into(),
        tile_x: 0,
        tile_y: 0,
        width: 2,
        height: 2,
        values: vec![12345; 4],
    };
    let container = lumina_sidecar::ZDataContainer::new(vec![legacy]).unwrap();
    lumina_sidecar::save_zdata(&zdata_path, &container).unwrap();
    let mut warnings = Vec::new();
    assert!(load_persisted_mask_planes(&document, &zdata_path, &mut warnings).is_empty());
    // Legacy plain-id tiles are ABSENCE (no record), not corruption: the
    // decision layer reports them as missing — no corrupt warning here.
    assert!(warnings.is_empty());
}

/// R2-CLI-05: a `.lumina.zdata` bundle that exists but is unreadable must
/// surface as an explicit "corrupt" warning (stderr + mask warnings
/// channel) instead of being silently treated like a missing bundle.
#[test]
fn render_with_corrupt_mask_zdata_warns_loudly_and_continues() {
    let directory = tempfile::tempdir().unwrap();
    let (input, frame) = png_input(directory.path(), "input.png", 100);
    let bytes = fs::read(&input).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // Corrupt payload in place of a valid bundle.
    fs::write(
        lumina_sidecar::zdata_path_for(&input),
        b"definitely not zdata",
    )
    .unwrap();

    let output = directory.path().join("output.png");
    let mut warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input.clone(),
            output,
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
        },
        90,
        None,
        MaskPolicy::Warn,
        &mut warnings,
    )
    .expect("warn policy continues past the corrupt bundle");

    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("unreadable or corrupt")),
        "the corrupt bundle must be reported through the mask-warning channel: {warnings:?}"
    );

    // A MISSING bundle (nothing persisted) stays warning-free — only
    // existing-but-unreadable bundles warn.
    let (input2, frame2) = png_input(directory.path(), "clean.png", 101);
    let bytes2 = fs::read(&input2).unwrap();
    write_sidecar_with_valid_layer(&input2, &bytes2, &frame2);
    let mut clean_warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input2,
            output: directory.path().join("output-clean.png"),
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
        },
        90,
        None,
        MaskPolicy::Warn,
        &mut clean_warnings,
    )
    .unwrap();
    assert!(
        !clean_warnings
            .iter()
            .any(|warning| warning.contains("corrupt")),
        "a missing bundle is not corruption: {clean_warnings:?}"
    );
}

#[test]
fn export_with_stale_masks_continues_by_default_and_aborts_under_strict() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 90);
    let bytes = fs::read(&input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    // Valid-status mask whose artifact is NOT available (no `.lumina.zdata`).
    write_sidecar_with_valid_layer(&input, &bytes, &frame);

    // Default `warn`: warn-and-continue, export succeeds (the wired stub
    // engine even re-infers during the render).
    let output = directory.path().join("out-warn.png");
    export(ExportArgs {
        input: input.clone(),
        output: output.clone(),
        format: "png".into(),
        quality: 90,
        virtual_copy: None,
        update_masks: false,
        force_render: false,
        migrate: false,
        json: false,
        mask_policy: CliMaskPolicy::Warn,
        write_metadata: false,
    })
    .unwrap();
    assert!(output.is_file());

    // `strict`: aborts BEFORE anything is decoded or written.
    let strict_output = directory.path().join("out-strict.png");
    let error = export(ExportArgs {
        input: input.clone(),
        output: strict_output.clone(),
        format: "png".into(),
        quality: 90,
        virtual_copy: None,
        update_masks: false,
        force_render: false,
        migrate: false,
        json: false,
        mask_policy: CliMaskPolicy::Strict,
        write_metadata: false,
    })
    .unwrap_err();
    assert!(error.to_string().contains("strict mask policy"));
    assert!(!strict_output.exists());
}

#[test]
fn mask_policy_flag_defaults_to_warn_and_parses_strict() {
    let cli =
        Cli::try_parse_from(["lumina", "export", "--input", "a.png", "--output", "b.png"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Export(ExportArgs {
            mask_policy: CliMaskPolicy::Warn,
            ..
        })
    ));

    let cli = Cli::try_parse_from([
        "lumina",
        "batch",
        "--input",
        "src",
        "--output",
        "out",
        "--mask-policy",
        "strict",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Command::Batch(BatchArgs {
            mask_policy: CliMaskPolicy::Strict,
            ..
        })
    ));

    assert!(Cli::try_parse_from([
        "lumina",
        "render",
        "--input",
        "a.png",
        "--output",
        "b.png",
        "--mask-policy",
        "bogus",
    ])
    .is_err());
}
