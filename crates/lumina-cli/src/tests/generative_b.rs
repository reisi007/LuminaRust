use super::*;

/// GEN-ONNX-1 (CLI double-role follow-up): a single record carrying both
/// roles (`auto_fill_transparent` + `expand_beyond_image`) generates both
/// canvases (`Lens → auto-fill → Perspective → expand`), links the
/// canvas-defining expand record, reports `available`, and renders
/// byte-identically to the core oracle.
#[test]
fn generative_double_role_generate_render_and_status() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    // 2x2 with the top-left pixel transparent so the auto-fill role triggers.
    let source = ImageFrame::new(
        2,
        2,
        vec![
            10, 20, 30, 0, 10, 20, 30, 255, 10, 20, 30, 255, 10, 20, 30, 255,
        ],
    )
    .unwrap();
    fs::write(&input, source.encode(ImageFileFormat::Png).unwrap()).unwrap();

    // One `--generate` produces both role canvases from the double role
    // (`--json` exercises the multi-role payload branch).
    generative(GenerativeArgs {
        generate: true,
        auto_fill: true,
        expand: true,
        canvas: Some("4x4+1+1".into()),
        prompt: Some("extend".into()),
        seed: Some(7),
        json: true,
        ..generative_args(&input)
    })
    .unwrap();

    // Both deterministic identity records are persisted in one bundle.
    let zdata = lumina_sidecar::zdata_path_for(&input);
    let container = load_zdata(&zdata).unwrap();
    let canvas_ids: Vec<String> = container
        .decode_all()
        .unwrap()
        .into_iter()
        .filter_map(|spec| match spec {
            lumina_sidecar::RecordSpec::GenerativeCanvas(canvas) => Some(canvas.id),
            _ => None,
        })
        .collect();
    assert_eq!(
        canvas_ids.len(),
        2,
        "the double role must persist the auto-fill AND the expand canvas, got {canvas_ids:?}"
    );

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let edit = document.virtual_copies[0]
        .recipe
        .generative_edit
        .as_ref()
        .unwrap();
    assert!(edit.auto_fill_transparent.unwrap_or(false) && edit.effective_expand());
    let link = edit.artifact.as_ref().expect("expand link persisted");
    assert_eq!(link.width, 4, "the linked canvas is the expand role");

    // Status reports both roles `available` (exit 0); `--json` covers the
    // multi-role status payload.
    generative(GenerativeArgs {
        status: true,
        json: true,
        ..generative_args(&input)
    })
    .unwrap();

    // Render + byte-identical core oracle.
    let output = directory.path().join("out.png");
    process_selected(
        process_args(&input, &output),
        90,
        None,
        MaskPolicy::Warn,
        &mut Vec::new(),
    )
    .unwrap();
    let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
    assert_eq!((rendered.width, rendered.height), (4, 4));
    assert!(
        rendered
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255),
        "auto-fill + expand must leave no transparent pixels"
    );

    let recipe = document.virtual_copies[0].recipe.clone();
    let artifacts = resolve_generative_artifacts(
        &source,
        &recipe,
        None,
        &[],
        &lumina_sidecar::zdata_path_for(&input),
        None,
    )
    .unwrap();
    assert!(artifacts.auto_fill.is_some());
    assert!(artifacts.expand.is_some());
    let oracle = render_frame_with_generative(
        &source,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        artifacts.input(),
    )
    .unwrap()
    .frame;
    assert_eq!(
        rendered.pixels, oracle.pixels,
        "the CLI render must be byte-identical to the core oracle"
    );
}

/// GEN-ONNX-1: a double-role record without any artifact is a loud refusal
/// (render and status exit non-zero), never a silent unexpanded render.
#[test]
fn generative_double_role_without_artifact_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let source = ImageFrame::new(
        2,
        2,
        vec![
            10, 20, 30, 0, 10, 20, 30, 255, 10, 20, 30, 255, 10, 20, 30, 255,
        ],
    )
    .unwrap();
    fs::write(&input, source.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let bytes = fs::read(&input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    let mut document = SidecarDocument::new(
        source_identity(&input, &bytes, &frame, None).unwrap(),
        "raster-mvp-1",
    );
    document.virtual_copies[0].recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(GenerativeCanvas {
            output_width: 4,
            output_height: 4,
            source_offset_x: 1,
            source_offset_y: 1,
            extras: Default::default(),
        }),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: Some(true),
        seed: Some(7),
        prompt: None,
        extras: Default::default(),
    });
    save_sidecar(&sidecar_path_for(&input), &document).unwrap();

    // Status is loud (auto-fill record missing).
    let error = generative(GenerativeArgs {
        status: true,
        ..generative_args(&input)
    })
    .unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("missing"),
        "double-role status without a canvas must be loud, got {error}"
    );

    // The render refuses without writing an output.
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
        "render must refuse a double role without artifacts, got {error}"
    );
    assert!(!output.exists(), "no output on a refused render");
}

/// GEN-ONNX-1: an explicit `--remove` unlinks the role; the resolver must
/// not silently re-adopt the still-present bundle record (the record is
/// kept by design, the recipe is not).
#[test]
fn generative_removed_link_stays_loud_with_bundle_record() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _source) = png_input(directory.path(), "input.png", 100);
    generative(GenerativeArgs {
        generate: true,
        expand: true,
        canvas: Some("4x4+0+0".into()),
        seed: Some(7),
        ..generative_args(&input)
    })
    .unwrap();
    let zdata = lumina_sidecar::zdata_path_for(&input);
    assert!(zdata.is_file());

    generative(GenerativeArgs {
        remove: true,
        ..generative_args(&input)
    })
    .unwrap();
    assert!(zdata.is_file(), "`--remove` keeps the bundle record");

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
        "an unlinked active role must stay loud, got {error}"
    );
    assert!(!output.exists(), "no output on a refused render");
}

/// GEN-ONNX-1 Welle 2a Follow-up (1): the normative auto-fill caller
/// convention — `auto_fill = None` (identity, no artifact) when the post-lens
/// frame has no transparent pixels. Pins that the CLI does not demand an
/// artifact link in that case and renders the identity.
#[test]
fn generative_auto_fill_without_transparency_needs_no_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    // Fully opaque frame (alpha 255) — no transparent pixels after lens.
    let source = ImageFrame::new(
        2,
        2,
        vec![
            100, 100, 100, 255, 100, 100, 100, 255, 100, 100, 100, 255, 100, 100, 100, 255,
        ],
    )
    .unwrap();
    fs::write(&input, source.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let bytes = fs::read(&input).unwrap();
    let frame = ImageFrame::decode(&bytes).unwrap();
    let mut document = SidecarDocument::new(
        source_identity(&input, &bytes, &frame, None).unwrap(),
        "raster-mvp-1",
    );
    document.virtual_copies[0].recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: None,
        seed: Some(0),
        prompt: None,
        extras: Default::default(),
    });
    save_sidecar(&sidecar_path_for(&input), &document).unwrap();

    let output = directory.path().join("out.png");
    process_selected(
        process_args(&input, &output),
        90,
        None,
        MaskPolicy::Warn,
        &mut Vec::new(),
    )
    .unwrap();
    let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(
        rendered.pixels, source.pixels,
        "no transparency after lens => identity, no artifact required (caller passes None)"
    );
}
