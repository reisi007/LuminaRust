use super::*;

#[test]
fn render_with_valid_mask_zdata_has_no_warning() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let bytes = frame.encode(ImageFileFormat::Png).unwrap();
    fs::write(&input, &bytes).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);

    // Provide a 2x2 fully-filled artifact plane for `subject`, stored
    // under the per-copy composite record id (REVIEW-CLI-N1).
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

    let mut warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
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
    assert!(output.is_file());
    assert!(warnings.is_empty());
}

#[test]
fn render_with_missing_mask_zdata_reinfers_and_succeeds() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let bytes = frame.encode(ImageFileFormat::Png).unwrap();
    fs::write(&input, &bytes).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);
    // No zdata file on purpose. With the inference model wired (F-048), the
    // missing artifact is (re-)inferred; the render succeeds with a
    // produced mask. F-100/GUI-GEN-GRANULAR-10: that implicit re-inference
    // is surfaced loudly (it was previously silent).

    let mut warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
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
    assert!(output.is_file());
    assert_eq!(warnings.len(), 1, "implicit re-inference must be loud");
    assert!(warnings[0].contains("re-inferred"), "{warnings:?}");
}
