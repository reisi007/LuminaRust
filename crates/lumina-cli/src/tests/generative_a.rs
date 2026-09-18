use super::*;

#[test]
fn generative_expand_generate_status_and_render_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let (input, source) = png_input(directory.path(), "input.png", 100);
    assert_eq!((source.width, source.height), (2, 2));

    // Produce and persist the composited canvas (fixture model, offline).
    generative(GenerativeArgs {
        generate: true,
        expand: true,
        canvas: Some("4x4+0+0".into()),
        prompt: Some("extend".into()),
        seed: Some(7),
        ..generative_args(&input)
    })
    .unwrap();
    assert!(lumina_sidecar::zdata_path_for(&input).is_file());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let edit = document.virtual_copies[0]
        .recipe
        .generative_edit
        .as_ref()
        .expect("generative_edit persisted");
    assert!(edit.effective_expand());
    assert!(
        edit.artifact.is_some(),
        "the artifact link must be persisted"
    );
    assert_eq!(edit.artifact.as_ref().unwrap().width, 4);

    // `--status` is healthy (exit 0).
    generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap();

    // Render consumes the artifact (loud on any drift).
    let output = directory.path().join("out.png");
    process_selected(
        process_args(&input, &output),
        90,
        None,
        MaskPolicy::Warn,
        &mut Vec::new(),
    )
    .unwrap();
    assert!(output.is_file());
    let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(
        (rendered.width, rendered.height),
        (4, 4),
        "the rendered frame is the expanded canvas"
    );
    // The generated border differs from the flat source, and the source
    // block is preserved at the offset (top-left here).
    assert_eq!(&rendered.pixels[..4], &source.pixels[..4]);
    let border = ((3 * 4 + 3) * 4) as usize;
    assert_ne!(
        &rendered.pixels[border..border + 3],
        &source.pixels[..3],
        "the generated canvas border must differ from the flat source"
    );
}

#[test]
fn generative_status_without_artifact_exits_with_error() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _frame) = png_input(directory.path(), "input.png", 100);
    // Persist a generative edit without an artifact link.
    let mut document = SidecarDocument::new(
        source_identity(
            &input,
            &fs::read(&input).unwrap(),
            &ImageFrame::decode(&fs::read(&input).unwrap()).unwrap(),
            None,
        )
        .unwrap(),
        "raster-mvp-1",
    );
    document.virtual_copies[0].recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(GenerativeCanvas {
            output_width: 4,
            output_height: 4,
            source_offset_x: 0,
            source_offset_y: 0,
            extras: Default::default(),
        }),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: Some(7),
        prompt: None,
        extras: Default::default(),
    });
    save_sidecar(&sidecar_path_for(&input), &document).unwrap();

    // Status reports `missing` and fails loudly (exit code 1).
    let error = generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap_err();
    assert!(error.to_string().contains("missing"), "got {error}");

    // Rendering the same recipe without an artifact is also loud.
    let output = directory.path().join("out.png");
    let error = process_selected(
        process_args(&input, &output),
        90,
        None,
        MaskPolicy::Warn,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("generative_canvas"),
        "got {error}"
    );
    assert!(!output.exists(), "no output on a refused render");
}

#[test]
fn generative_expand_without_canvas_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _frame) = png_input(directory.path(), "input.png", 100);
    let error = generative(GenerativeArgs {
        generate: true,
        expand: true,
        ..generative_args(&input)
    })
    .unwrap_err();
    assert!(error.to_string().contains("--canvas"), "got {error}");
    assert!(!lumina_sidecar::zdata_path_for(&input).exists());
}

/// GEN-ONNX-1 BLOCKER fix: a prompt change after generation must be detected
/// as `stale` — the old canvas is never silently adopted.
#[test]
fn generative_prompt_change_is_detected_as_stale() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _frame) = png_input(directory.path(), "input.png", 100);
    generative(GenerativeArgs {
        generate: true,
        expand: true,
        canvas: Some("4x4+0+0".into()),
        prompt: Some("A".into()),
        seed: Some(7),
        ..generative_args(&input)
    })
    .unwrap();
    generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap();

    // Simulate a persisted prompt change (recipe edit).
    let sidecar_path = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0]
        .recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .prompt = Some("B".into());
    save_sidecar(&sidecar_path, &document).unwrap();

    // Status is now stale and fails loudly (exit 1).
    let error = generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("stale"),
        "prompt drift must be reported stale, got {error}"
    );

    // And the render refuses to adopt the old canvas.
    let output = directory.path().join("out.png");
    let error = process_selected(
        process_args(&input, &output),
        90,
        None,
        MaskPolicy::Warn,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("stale"),
        "prompt drift must refuse the render, got {error}"
    );
    assert!(!output.exists(), "no output on a refused render");
}

/// GEN-ONNX-1 Welle 2a: a negative-prompt change after generation must be
/// detected as `stale` (the additive schema field is identity-bearing).
#[test]
fn generative_negative_prompt_change_is_detected_as_stale() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _frame) = png_input(directory.path(), "input.png", 100);
    generative(GenerativeArgs {
        generate: true,
        expand: true,
        canvas: Some("4x4+0+0".into()),
        prompt: Some("A".into()),
        negative_prompt: Some("blurry".into()),
        seed: Some(7),
        ..generative_args(&input)
    })
    .unwrap();
    generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap();

    // The sidecar JSON carries the additive top-level field.
    let sidecar_path = sidecar_path_for(&input);
    let raw = fs::read_to_string(&sidecar_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value["virtual_copies"][0]["recipe"]["generative_edit"]["negative_prompt"],
        serde_json::json!("blurry"),
        "negative_prompt must roundtrip as a top-level JSON field"
    );

    // Simulate a persisted negative-prompt change.
    let mut document = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0]
        .recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .set_negative_prompt(Some("noisy".into()));
    save_sidecar(&sidecar_path, &document).unwrap();

    let error = generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("stale"),
        "negative-prompt drift must be reported stale, got {error}"
    );
}
