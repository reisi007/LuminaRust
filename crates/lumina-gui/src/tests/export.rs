//! GUI export parity, targets and format/options tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- F-103-N5: shared export path is byte-identical to the CLI ----

#[test]
fn gui_export_is_byte_identical_to_shared_export_path() {
    // The GUI export module must produce the exact same bytes as the CLI's
    // shared `lumina_core::export_image` (render + encode) for the same
    // source frame, recipe and export options. PNG is used for the byte
    // comparison because its encoder is deterministic (see
    // feature/platform/cli-gui-wasm.md, "Desktop-GUI / Export-Determinismus").
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let bytes = std::fs::read(&source).unwrap();
    let recipe = EditRecipe {
        adjustments: std::collections::BTreeMap::from([("exposure".into(), 0.7)]),
        ..Default::default()
    };

    // CLI-style shared path: decode + render + encode via the single shared
    // function, with the same neutral context the GUI uses (no masks, no
    // source actions, no white balance for a raster PNG).
    let frame = ImageFrame::decode(&bytes).unwrap();
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let options = ExportOptions {
        format: ImageFileFormat::Png,
        quality: 90,
        dither: false,
        ..Default::default()
    };
    let cli_bytes = lumina_core::export_image(&frame, &context, options).unwrap();

    // GUI path: load the same bytes, set the same recipe, then export via the
    // module's own `export_to` (which internally calls `export_image`).
    let mut app = new_app();
    app.load_bytes(bytes.clone(), "photo.png").unwrap();
    app.set_adjustment("exposure", 0.7);
    app.render().unwrap();
    app.export_format = ImageFileFormat::Png;
    app.export_quality = 90;
    let out = directory.path().join("photo_export.png");
    app.export_to(out.clone()).unwrap();
    let gui_bytes = std::fs::read(&out).unwrap();

    assert_eq!(
        cli_bytes, gui_bytes,
        "GUI and CLI/shared export paths must be byte-identical"
    );
}

#[test]
fn gui_jpeg_export_is_functional_and_byte_identical_to_shared_path() {
    // JPEG is functionally validated (deterministic within one encoder
    // version, see feature/platform/cli-gui-wasm.md,
    // "Desktop-GUI / Export-Determinismus"): the GUI JPEG export equals the
    // shared `export_image` JPEG export and decodes to the same dimensions
    // as the source.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let bytes = std::fs::read(&source).unwrap();

    let frame = ImageFrame::decode(&bytes).unwrap();
    let context = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let options = ExportOptions {
        format: ImageFileFormat::Jpeg,
        quality: 85,
        dither: false,
        ..Default::default()
    };
    let cli_bytes = lumina_core::export_image(&frame, &context, options).unwrap();

    let mut app = new_app();
    app.load_bytes(bytes.clone(), "photo.png").unwrap();
    app.export_format = ImageFileFormat::Jpeg;
    app.export_quality = 85;
    let out = directory.path().join("photo_export.jpg");
    app.export_to(out.clone()).unwrap();
    let gui_bytes = std::fs::read(&out).unwrap();

    // Both paths use the identical image encoder call, so the bytes match.
    assert_eq!(
        cli_bytes, gui_bytes,
        "JPEG GUI and shared export must match"
    );
    // And the decoded JPEG has the same pixel dimensions as the source.
    let decoded = ImageFrame::decode(&gui_bytes).unwrap();
    assert_eq!((decoded.width, decoded.height), (frame.width, frame.height));
}

#[test]
fn export_rejects_same_path_as_gui_error() {
    // Exporting onto the source file is rejected (non-destructive contract);
    // nothing is written.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 0.3);
    let result = app.export_to(source.clone());
    assert!(result.is_err(), "exporting onto the source must fail");
    // No export artifact was created with the source's name.
    assert!(!source.with_extension("jpg").exists());
    assert!(source.is_file());
}

#[test]
fn export_rejects_extensionless_target_resolving_onto_source() {
    // REVIEW-GUI-EXPORT-1 regression: the extension must be applied BEFORE
    // the same-path check. Target `/d/photo` with format PNG resolves to
    // `/d/photo.png`, which IS the loaded source — the old pre-extension
    // guard compared `/d/photo` against `/d/photo.png` and let the export
    // overwrite the original in full.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let before = blake3::hash(&std::fs::read(&source).unwrap());
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.export_format = ImageFileFormat::Png;

    // Extensionless target that derives onto the source.
    let result = app.export_to(directory.path().join("photo"));
    assert!(
        result.is_err(),
        "extensionless target deriving onto the source must be refused"
    );
    // And onto the source's sidecar / zdata artefacts as well (pure-logic
    // level, see `resolve_export_target_applies_extension_before_guard`;
    // through `export_to` the format extension always lands on png/jpg/webp,
    // so only the source collision itself is reachable end-to-end).

    // The original is untouched and no stray artifacts appeared.
    let after = blake3::hash(&std::fs::read(&source).unwrap());
    assert_eq!(before, after, "original must remain byte-identical");
    let entries: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "only the source file may exist: {entries:?}"
    );
}

#[test]
fn resolve_export_target_applies_extension_before_guard() {
    // Pure-logic coverage of the REVIEW-GUI-EXPORT-1 guard.
    let directory = tempfile::tempdir().unwrap();
    let dir = directory.path();
    let source = dir.join("photo.png");
    std::fs::write(&source, b"original").unwrap();

    // Extensionless target + PNG derives exactly onto the source: refuse.
    let err =
        LuminaApp::resolve_export_target(&source.display().to_string(), dir.join("photo"), "png")
            .expect_err("must refuse target that resolves onto the source");
    assert!(err.to_string().contains("refusing"), "{err}");

    // Same target with a different format is fine (different file).
    let resolved =
        LuminaApp::resolve_export_target(&source.display().to_string(), dir.join("photo"), "jpg")
            .unwrap();
    assert_eq!(resolved, dir.join("photo.jpg"));

    // Sidecar and binary mask bundle targets are protected too.
    for blocked_ext in ["lumina.json", "lumina.zdata"] {
        // A typed-in full artefact name keeps its stem; simulate a target
        // whose post-extension form equals the artefact by requesting the
        // matching extension-less stem plus the artefact's own extension
        // via an explicit path.
        let artefact = dir.join(format!("photo.png.{blocked_ext}"));
        std::fs::write(&artefact, b"artefact").unwrap();
        let err = LuminaApp::resolve_export_target(
            &source.display().to_string(),
            artefact.clone(),
            if blocked_ext.ends_with("json") {
                "json"
            } else {
                "zdata"
            },
        )
        .expect_err("must refuse target that resolves onto a persisted artefact");
        assert!(err.to_string().contains("refusing"), "{err}");
        assert_eq!(
            std::fs::read(&artefact).unwrap(),
            b"artefact",
            "protected artefact must stay untouched"
        );
    }

    // An unrelated target passes through with the extension applied.
    let ok =
        LuminaApp::resolve_export_target(&source.display().to_string(), dir.join("export"), "png")
            .unwrap();
    assert_eq!(ok, dir.join("export.png"));

    // Empty source (nothing loaded) never blocks.
    let ok = LuminaApp::resolve_export_target("  ", dir.join("out"), "png").unwrap();
    assert_eq!(ok, dir.join("out.png"));
}

#[test]
fn export_preserves_original_bytes_unchanged() {
    // The original source file is byte-for-byte untouched by an export.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let before = blake3::hash(&std::fs::read(&source).unwrap());
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("contrast", 0.4);
    app.render().unwrap();
    let out = directory.path().join("photo_out.png");
    app.export_to(out.clone()).unwrap();
    let after = blake3::hash(&std::fs::read(&source).unwrap());
    assert_eq!(
        before, after,
        "original must be byte-identical after export"
    );
    assert!(out.is_file(), "export artifact was written");
}

#[test]
fn export_options_validate_quality_range() {
    // Quality must be in 1..=100; 0 and 101 are rejected, 1 and 100 ok.
    assert!(ExportOptions {
        format: ImageFileFormat::Png,
        quality: 0,
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(ExportOptions {
        format: ImageFileFormat::Png,
        quality: 101,
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(ExportOptions {
        format: ImageFileFormat::Png,
        quality: 1,
        ..Default::default()
    }
    .validate()
    .is_ok());
    assert!(ExportOptions {
        format: ImageFileFormat::Png,
        quality: 100,
        ..Default::default()
    }
    .validate()
    .is_ok());
}

#[test]
fn image_format_from_extension_parses_known_and_rejects_unknown() {
    assert_eq!(
        ImageFileFormat::from_extension("png"),
        Some(ImageFileFormat::Png)
    );
    assert_eq!(
        ImageFileFormat::from_extension("JPG"),
        Some(ImageFileFormat::Jpeg)
    );
    assert_eq!(
        ImageFileFormat::from_extension("jpeg"),
        Some(ImageFileFormat::Jpeg)
    );
    assert_eq!(
        ImageFileFormat::from_extension("webp"),
        Some(ImageFileFormat::WebP)
    );
    assert_eq!(ImageFileFormat::from_extension("tiff"), None);
    assert_eq!(ImageFileFormat::from_extension(""), None);
    assert_eq!(
        ImageFileFormat::default_extension(ImageFileFormat::Jpeg),
        "jpg"
    );
    assert_eq!(
        ImageFileFormat::default_extension(ImageFileFormat::Png),
        "png"
    );
}
