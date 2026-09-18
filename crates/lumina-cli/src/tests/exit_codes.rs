use super::*;

/// R2-CLI-07: a partially failed batch exits with its own documented code
/// (3) instead of the generic runtime-error code (1).
#[test]
fn batch_partial_failure_maps_to_exit_code_three() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let (_, frame) = png_input(&src, "good.png", 10);
    drop(frame);
    // A corrupt payload fails at decode time → the item fails.
    fs::write(src.join("broken.png"), b"not a png").unwrap();

    let error = batch(BatchArgs {
        input: src,
        output: directory.path().join("out"),
        jobs: 1,
        retry: 0,
        resume: false,
        dry_run: false,
        update_masks: false,
        force_render: false,
        json: false,
        format: "png".into(),
        quality: 90,
        virtual_copy: None,
        mask_policy: CliMaskPolicy::Warn,
        write_metadata: false,
    })
    .unwrap_err();
    match &error {
        CliError::BatchPartial { failed } => assert_eq!(*failed, 1),
        other => panic!("expected BatchPartial, got {other:?}"),
    }
    assert_eq!(error.exit_code(), 3);
    // Every other CLI error keeps the generic code 1.
    assert_eq!(CliError::Message("x".into()).exit_code(), 1);
}

/// R2-CLI-07/F2/F5: a usage error detected after clap parsing (mutually
/// exclusive action flags) exits with 2, matching clap's own usage code —
/// never the runtime-error code 1.
#[test]
fn usage_error_maps_to_exit_code_two() {
    assert_eq!(CliError::Usage("bad flags".into()).exit_code(), 2);
}

/// R2-CLI-11: batch inputs are deduplicated by filesystem identity so a
/// hard link under two names is processed once (unix).
#[cfg(unix)]
#[test]
fn batch_deduplicates_inputs_by_inode_identity() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.arw");
    let alias = directory.path().join("alias.arw");
    fs::write(&original, b"synthetic").unwrap();
    fs::hard_link(&original, &alias).unwrap();
    let distinct = directory.path().join("distinct.arw");
    fs::write(&distinct, b"synthetic").unwrap();

    let deduped = dedup_same_file_inputs(vec![original.clone(), alias, distinct.clone()]);
    assert_eq!(
        deduped,
        vec![original, distinct],
        "the inode alias must be dropped, first occurrence kept"
    );

    // Unreadable metadata entries are kept (they fail loudly at decode).
    let missing = directory.path().join("missing.arw");
    let kept = dedup_same_file_inputs(vec![missing]);
    assert_eq!(kept.len(), 1);
}

#[test]
fn dust_removal_leaves_no_orphan_bundle_when_the_copy_is_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 85);
    let bytes = fs::read(&input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    save_sidecar(
        &sidecar_path_for(&input),
        &SidecarDocument::new(
            source_identity(&input, &bytes, &frame, None).unwrap(),
            "raster-mvp-1",
        ),
    )
    .unwrap();

    // Replacement image matching the source dimensions.
    let replacement = directory.path().join("replacement.png");
    let replacement_frame = ImageFrame::new(2, 2, vec![200; 16]).unwrap();
    fs::write(
        &replacement,
        replacement_frame.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    let definition = directory.path().join("region.json");
    fs::write(
        &definition,
        serde_json::json!({
            "id": "r1",
            "region_width": 2,
            "region_height": 2,
            "region_values": [0, 0, 0, 0],
            "replacement_path": replacement,
        })
        .to_string(),
    )
    .unwrap();

    let error = dust_removal(DustRemovalArgs {
        input: input.clone(),
        repair_region: definition,
        virtual_copy: Some("ghost".into()),
        render_out: None,
        json: true,
    })
    .unwrap_err();
    assert!(error.to_string().contains("unknown virtual copy"));
    // REVIEW-CLI-N2: nothing was appended before validation failed.
    assert!(!lumina_sidecar::zdata_path_for(&input).exists());
}
