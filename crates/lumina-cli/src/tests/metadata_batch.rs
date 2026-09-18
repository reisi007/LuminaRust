use super::*;

/// LRPAR-G15-IPTC-S6: `--write-metadata` exists on `export`/`process`/
/// `batch` (and only there — `render` has no such flag), defaults to off
/// everywhere, and parses on all three paths.
#[test]
fn write_metadata_flag_defaults_off_and_parses_on_export_process_batch() {
    // Default: off (no metadata, exactly today's behavior).
    let cli =
        Cli::try_parse_from(["lumina", "export", "--input", "a.png", "--output", "b.jpg"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Export(ExportArgs {
            write_metadata: false,
            ..
        })
    ));
    let cli = Cli::try_parse_from(["lumina", "process", "--input", "a.png", "--output", "b.jpg"])
        .unwrap();
    assert!(matches!(
        cli.command,
        Command::Process(ProcessArgs {
            write_metadata: false,
            ..
        })
    ));
    let cli =
        Cli::try_parse_from(["lumina", "batch", "--input", "src", "--output", "out"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Batch(BatchArgs {
            write_metadata: false,
            ..
        })
    ));
    // Opt-in: on, on every path.
    let cli = Cli::try_parse_from([
        "lumina",
        "export",
        "--input",
        "a.png",
        "--output",
        "b.jpg",
        "--write-metadata",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Command::Export(ExportArgs {
            write_metadata: true,
            ..
        })
    ));
    let cli = Cli::try_parse_from([
        "lumina",
        "process",
        "--input",
        "a.png",
        "--output",
        "b.jpg",
        "--write-metadata",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Command::Process(ProcessArgs {
            write_metadata: true,
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
        "--write-metadata",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Command::Batch(BatchArgs {
            write_metadata: true,
            ..
        })
    ));
    // `render` deliberately offers no bake-in flag (SOLL §7: only
    // export/process/batch).
    assert!(Cli::try_parse_from([
        "lumina",
        "render",
        "--input",
        "a.png",
        "--output",
        "b.jpg",
        "--write-metadata",
    ])
    .is_err());
}

#[test]
fn batch_rejects_colliding_output_names_before_writing() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(src.join("a")).unwrap();
    fs::create_dir_all(src.join("b")).unwrap();
    let (_, frame_a) = png_input(&src.join("a"), "x.png", 10);
    let (_, frame_b) = png_input(&src.join("b"), "x.arw", 20);
    drop(frame_a);
    drop(frame_b);

    let out = directory.path().join("out");
    let error = batch(BatchArgs {
        input: src,
        output: out.clone(),
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
    assert!(error.to_string().contains("collision"));
    // The refusal happens before the output directory exists.
    assert!(!out.exists());
}

#[test]
fn batch_resume_requires_parsed_ok_status() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "a.png", 30);
    let out_dir = directory.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let output = out_dir.join("a.png");
    fs::write(&output, b"previous").unwrap();
    let status = out_dir.join("a.png.status.json");
    let args = BatchArgs {
        input: input.clone(),
        output: out_dir.clone(),
        jobs: 1,
        retry: 0,
        resume: true,
        dry_run: true,
        update_masks: false,
        force_render: false,
        json: false,
        format: "png".into(),
        quality: 90,
        virtual_copy: None,
        mask_policy: CliMaskPolicy::Warn,
        write_metadata: false,
    };

    // A spaced `"status": "ok"` parses as done (the old substring match
    // failed here and reprocessed the item).
    fs::write(&status, r#"{ "input": "a.png", "status": "ok" }"#).unwrap();
    let before = fs::read_to_string(&status).unwrap();
    batch_one(&input, 0, 1, &args).unwrap();
    assert_eq!(fs::read_to_string(&status).unwrap(), before);

    // A parsed non-ok status means "not done": the item is reprocessed
    // and the status file rewritten by this (dry) run.
    fs::write(
        &status,
        r#"{"note":"\"status\":\"ok\" decoy","status":"failed"}"#,
    )
    .unwrap();
    batch_one(&input, 0, 1, &args).unwrap();
    let rewritten = fs::read_to_string(&status).unwrap();
    assert!(rewritten.contains("\"dry-run\""), "{rewritten}");

    // Malformed JSON counts as not done, too.
    fs::write(&status, "not json at all").unwrap();
    batch_one(&input, 0, 1, &args).unwrap();
    let rewritten = fs::read_to_string(&status).unwrap();
    assert!(rewritten.contains("\"dry-run\""), "{rewritten}");
}

#[test]
fn reindex_fails_when_a_sidecar_is_corrupt() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "good.png", 40);
    let bytes = fs::read(&input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // All-valid directory → success.
    reindex(IndexArgs {
        input: directory.path().to_path_buf(),
        json: true,
        migrate: false,
    })
    .unwrap();

    // One corrupt sidecar → loud failure (non-zero exit via `main`).
    fs::write(directory.path().join("broken.lumina.json"), "{ truncated").unwrap();
    let error = reindex(IndexArgs {
        input: directory.path().to_path_buf(),
        json: true,
        migrate: false,
    })
    .unwrap_err();
    assert!(error.to_string().contains("invalid sidecar"));
}

#[cfg(unix)]
#[test]
fn collect_images_survives_symlink_loops() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir_all(root.join("sub")).unwrap();
    let (_, top) = png_input(&root, "top.png", 50);
    let (_, deep) = png_input(&root.join("sub"), "deep.png", 60);
    drop(top);
    drop(deep);
    // Self-referencing directory loop plus an alias onto a subdirectory.
    symlink(&root, root.join("loop")).unwrap();
    symlink(root.join("sub"), root.join("link-sub")).unwrap();
    // A file symlink stays collectable (reading it cannot cycle).
    symlink(root.join("top.png"), root.join("alias.png")).unwrap();

    let mut found = Vec::new();
    collect_images(&root, &mut found).unwrap();
    found.sort();
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["alias.png", "deep.png", "top.png"]);
}

#[cfg(unix)]
#[test]
fn collect_sidecars_survives_symlink_loops() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    fs::create_dir_all(root.join("sub")).unwrap();
    // Sidecar collection is purely path-based, so plain marker files are
    // enough here.
    let top = root.join("top.png.lumina.json");
    fs::write(&top, b"{}").unwrap();
    fs::write(root.join("sub/deep.png.lumina.json"), b"{}").unwrap();
    // Self-referencing directory loop plus an alias onto a subdirectory —
    // neither may be followed during the sidecar walk (REVIEW-CLI-
    // FOLLOWUP-1; without the guard this test recurses until the stack
    // overflows).
    symlink(&root, root.join("loop")).unwrap();
    symlink(root.join("sub"), root.join("link-sub")).unwrap();
    // A file symlink stays collectable (reading it cannot cycle).
    symlink(&top, root.join("alias.png.lumina.json")).unwrap();

    let mut found = Vec::new();
    collect_sidecars(&root, &mut found).unwrap();
    // The shared walk sorts every directory level, so the collected
    // sequence itself is already deterministic.
    let mut sorted = found.clone();
    sorted.sort();
    assert_eq!(found, sorted);
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "alias.png.lumina.json",
            "deep.png.lumina.json",
            "top.png.lumina.json"
        ]
    );
}

#[test]
fn output_guard_rejects_sidecar_zdata_and_hardlink_targets() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 70);

    // Not-yet-existing bundle targets are protected at their future path…
    assert!(reject_protected_output(&input, &sidecar_path_for(&input)).is_err());
    assert!(reject_protected_output(&input, &lumina_sidecar::zdata_path_for(&input)).is_err());

    // …and equally once they exist.
    fs::write(sidecar_path_for(&input), b"{}").unwrap();
    assert!(reject_protected_output(&input, &sidecar_path_for(&input)).is_err());

    // A benign sibling path stays writable.
    let ok = directory.path().join("elsewhere.png");
    assert!(reject_protected_output(&input, &ok).is_ok());

    #[cfg(unix)]
    {
        let hardlink = directory.path().join("hardlink.png");
        fs::hard_link(&input, &hardlink).unwrap();
        let error = reject_protected_output(&input, &hardlink).unwrap_err();
        assert!(error.to_string().contains("hard link"));

        // CLI-GUARD-HARDLINK-1: hard links to the bundle are caught by
        // `(dev, inode)` identity, not only by path equality. The sidecar
        // already exists above; the zdata is materialized for the check.
        let sidecar_hardlink = directory.path().join("sidecar-hardlink.json");
        fs::hard_link(sidecar_path_for(&input), &sidecar_hardlink).unwrap();
        let error = reject_protected_output(&input, &sidecar_hardlink).unwrap_err();
        assert!(error.to_string().contains("hard link"), "error: {error}");
        assert!(error.to_string().contains("sidecar"), "error: {error}");

        let zdata = lumina_sidecar::zdata_path_for(&input);
        fs::write(&zdata, b"zdata").unwrap();
        let zdata_hardlink = directory.path().join("zdata-hardlink.bin");
        fs::hard_link(&zdata, &zdata_hardlink).unwrap();
        let error = reject_protected_output(&input, &zdata_hardlink).unwrap_err();
        assert!(error.to_string().contains("hard link"), "error: {error}");
        assert!(error.to_string().contains("bundle"), "error: {error}");
    }
}
