use super::*;

#[test]
fn history_entry_stores_final_recipe_and_snapshot_reproduces_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let bytes = frame.encode(ImageFileFormat::Png).unwrap();
    fs::write(&input, &bytes).unwrap();
    process(ProcessArgs {
        input: input.clone(),
        output: output.clone(),
        preset: None,
        exposure: Some(0.5),
        contrast: Some(-0.2),
        highlights: Some(0.1),
        shadows: None,
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap();
    assert!(output.is_file());

    let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let copy = &sidecar.virtual_copies[0];
    // Exactly one new history entry; its recipe snapshot is the final
    // recipe of the process run. `assert_eq` on EditRecipe covers all
    // relevant fields (adjustments, auto_features, nested stages).
    assert_eq!(copy.history.len(), 1);
    let entry = &copy.history[0];
    assert_eq!(entry.recipe, copy.recipe);
    assert_eq!(entry.recipe.adjustments["exposure"], 0.5);
    assert_eq!(entry.recipe.adjustments["contrast"], -0.2);
    assert_eq!(entry.recipe.adjustments["highlights"], 0.1);
    assert!(!entry.recipe.auto_features.enable_auto_tone);
    assert!(!entry.recipe.auto_features.match_total_exposure);
    assert!(entry.recorded_at.is_some());

    // Snapshot reproducibility: applying the stored recipe alone to the
    // original frame reproduces the process output byte-identically (PNG
    // is lossless and the encoder is deterministic), plus a decoded-pixel
    // cross-check.
    let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let rendered = render_frame(
        &source,
        &RenderContext {
            recipe: &entry.recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            // F-098-N2: this is a synthetic, recipe-only render without RAW
            // metadata/EXIF, so no Lensfun corrector can be built — `None`
            // (manual model) is the correct, expected state here.
            lensfun: None,
            depth: None,
        },
    )
    .unwrap();
    let expected = fs::read(&output).unwrap();
    assert_eq!(
        rendered.frame.encode(ImageFileFormat::Png).unwrap(),
        expected
    );
    assert_eq!(ImageFrame::decode(&expected).unwrap(), rendered.frame);
}

// ---- F-103-N8: no-match export reuses the warning render (no duplicate) ----
#[test]
fn no_match_export_is_byte_identical_to_single_render() {
    // The no-match export path must reuse the warning render instead of
    // re-rendering through `export_image`. The produced file must stay
    // byte-identical to a single `export_image` pass with the same final
    // recipe — i.e. exactly the pre-optimization output (F-103-N8). This
    // guards the optimization against any silent output drift.
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.webp");
    // A non-uniform spatial gradient so exposure/contrast actually move
    // pixels; a uniform frame can be invariant under 8-bit rounding.
    let width: u32 = 16;
    let height: u32 = 16;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x + y) % 256) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    process(ProcessArgs {
        input: input.clone(),
        output: output.clone(),
        preset: None,
        exposure: Some(0.3),
        contrast: Some(0.2),
        highlights: Some(-0.1),
        shadows: Some(0.1),
        auto_tone: false,
        match_total_exposure: false,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap();

    let actual = fs::read(&output).unwrap();
    // The final recipe persisted by `process` (incl. the CLI adjustments
    // above) is the recipe `export_image` would have rendered.
    let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let final_recipe = sidecar.virtual_copies[0].recipe.clone();
    let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    // `process` always uses the default quality 90 and `dither: false`,
    // matching the historical `frame.encode(format)` output.
    let options = ExportOptions {
        format: ImageFileFormat::WebP,
        quality: 90,
        dither: false,
        ..Default::default()
    };
    let expected = export_image(
        &source,
        &RenderContext {
            recipe: &final_recipe,
            camera_white_balance: None,
            source_actions: &[],
            // No mask library in this test → empty mask context, which
            // renders identically to `None` (see render.rs
            // `no_layers_is_identical_to_no_mask_context`).
            masks: None,
            lensfun: None,
            depth: None,
        },
        options,
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn match_total_exposure_still_rerenders_with_matched_recipe() {
    // Complementary guard to F-103-N8: when matching is ON the CLI must
    // still re-render with the matched exposure (the output must differ
    // from the *unmatched* single render, confirming the second render is
    // not silently skipped).
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    let width: u32 = 16;
    let height: u32 = 16;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x + y) % 256) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    process(ProcessArgs {
        input: input.clone(),
        output: output.clone(),
        preset: None,
        exposure: None,
        contrast: None,
        highlights: None,
        shadows: None,
        auto_tone: false,
        match_total_exposure: true,
        target_luminance: 0.5,
        write_metadata: false,
    })
    .unwrap();
    let matched = fs::read(&output).unwrap();
    assert!(output.is_file());

    // The unmatched single render (recipe without the matched exposure) must
    // differ from the matched output, proving the second render actually ran.
    let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let mut unmatched_recipe = sidecar.virtual_copies[0].recipe.clone();
    unmatched_recipe.adjustments.remove("exposure");
    let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let options = ExportOptions {
        format: ImageFileFormat::Png,
        quality: 90,
        dither: false,
        ..Default::default()
    };
    let unmatched = export_image(
        &source,
        &RenderContext {
            recipe: &unmatched_recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        options,
    )
    .unwrap();
    assert_ne!(matched, unmatched);
}

#[test]
fn valid_mask_with_match_total_exposure_measures_masked_domain() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let output = directory.path().join("output.png");
    // 8x8 bimodal gray frame: left half (pixels 0..32) is 200, right half
    // (pixels 32..64) is 60. Unmasked mean = 130/255 ~= 0.51.
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for index in 0..64 {
        let value = if index < 32 { 200u8 } else { 60u8 };
        pixels.extend_from_slice(&[value, value, value, 255]);
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let bytes = frame.encode(ImageFileFormat::Png).unwrap();
    fs::write(&input, &bytes).unwrap();
    write_sidecar_with_valid_layer(&input, &bytes, &frame);

    // Valid artifact plane for `subject` at frame resolution. F-042 does
    // not modulate pixels yet (pixel modulation is F-049), but F-041
    // already weights the measurement domain: the bright left half is
    // fully masked (0), the dark right half fully visible (u16::MAX).
    //   weighted mean: 60/255 ~= 0.2353
    //   masked delta:  log2(0.5 / (60/255)) = log2(2.125) ~= 1.08746
    //   unmasked delta: log2(0.5 / (130/255)) ~= -0.0280
    let tile = lumina_sidecar::MaskTile {
        mask_id: zdata_mask_tile_id("vc-original", "subject"),
        tile_x: 0,
        tile_y: 0,
        width: 8,
        height: 8,
        values: (0..64).map(|i| if i < 32 { 0 } else { 65535 }).collect(),
    };
    let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
    lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

    let unmasked = match_total_exposure_masked(&frame, 0.5, &[]).unwrap();
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
            match_total_exposure: true,
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
    assert!(
        warnings.is_empty(),
        "valid mask must not warn: {warnings:?}"
    );

    // The persisted matching result follows the masked measurement domain
    // and demonstrably differs from the unmasked result (F-041).
    let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let auto = &sidecar.virtual_copies[0].recipe.auto_features;
    assert!(auto.match_total_exposure);
    let matched = auto.matched_exposure.unwrap();
    assert!(
        (matched - 1.08746).abs() < 0.001,
        "persisted matched exposure {matched}"
    );
    assert!(
        (matched - unmasked).abs() > 1.0,
        "masked delta {matched} must differ from unmasked {unmasked}"
    );

    // Applying the delta reaches the *masked* target: the visible (right)
    // half of the exported frame (60 * 2^1.08746 = 60 * 2.125 = 127.5 ->
    // 128, mean ~= 0.502) is within tolerance, while the masked-out left
    // half clamps at 255 and must not be part of the target check.
    let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
    let visible_mean = rendered
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(index, _)| *index >= 32)
        .map(|(_, pixel)| {
            (0.2126 * f64::from(pixel[0])
                + 0.7152 * f64::from(pixel[1])
                + 0.0722 * f64::from(pixel[2]))
                / 255.0
        })
        .sum::<f64>()
        / 32.0;
    assert!(
        (visible_mean - 0.5).abs() <= 0.02,
        "post-match visible mean {visible_mean} not within 0.02 of target 0.5"
    );
}
