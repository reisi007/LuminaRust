use super::*;

pub(crate) fn valid_mask_definition(
    id: &str,
    operation: lumina_sidecar::MaskOperation,
    references: Vec<lumina_sidecar::MaskReference>,
    identity: &SourceIdentity,
    width: u32,
    height: u32,
) -> lumina_sidecar::MaskDefinition {
    use lumina_sidecar::{
        CoordinateSystem, Extras, GeometryFingerprint, ModelIdentity, Preprocessing, Resolution,
    };
    // Build a *confirmably valid* persisted mask: its source/decode/model
    // identity matches the running source and the wired BiRefNet descriptor
    // (F-048), and it carries an artifact reference. F-047's persisted
    // masks always carry an `artifact`, so this mirrors real persistence.
    lumina_sidecar::MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: lumina_sidecar::SourceFingerprint {
            content_hash: identity.content_hash.clone(),
            byte_length: identity.byte_length,
            extras: Extras::new(),
        },
        decode_context: identity.decode_fingerprint.clone(),
        geometry_context: GeometryFingerprint {
            width: 2,
            height: 2,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: ModelIdentity {
            name: "BiRefNet".into(),
            version: "1.0.0".into(),
            hash: "pending-integration".into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width,
            height,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status: MaskStatus::Valid,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: Some(lumina_sidecar::ArtifactReference {
            relative_path: "x.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "c".into(),
            width,
            height,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }),
        operation,
        references,
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    }
}

pub(crate) fn write_sidecar_with_valid_layer(
    input: &Path,
    bytes: &[u8],
    frame: &ImageFrame,
) -> lumina_sidecar::SidecarDocument {
    // F-082-FOLLOWUP: every mask-work render reaches the CLI inference
    // wiring gate; under `onnx-rt` this configures the real-engine test
    // model so the render exercises the ORT path instead of hard-failing
    // on an unset `LUMINA_MODEL_PATH` (default builds stay on the stub).
    #[cfg(feature = "onnx-rt")]
    ensure_onnx_test_engine();
    let identity = source_identity(input, bytes, frame, None).unwrap();
    let mut document = SidecarDocument::new(identity.clone(), "raster-mvp-1");
    let copy = &mut document.virtual_copies[0];
    copy.mask_library = vec![valid_mask_definition(
        "subject",
        lumina_sidecar::MaskOperation::Source,
        vec![],
        &identity,
        frame.width,
        frame.height,
    )];
    copy.mask_layers = vec![lumina_sidecar::MaskLayer {
        id: "layer-1".into(),
        mask: lumina_sidecar::MaskReference {
            copy_id: copy.id.clone(),
            mask_id: "subject".into(),
            extras: BTreeMap::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: BTreeMap::new(),
        visible: true,
        local_adjustments: None,
    }];
    save_sidecar(&sidecar_path_for(input), &document).unwrap();
    document
}

// ---- G-03 Maskierungs-Parität: mask DAG CLI ----

pub(crate) fn mask_imported_input(directory: &Path, name: &str) -> PathBuf {
    let (input, _frame) = png_input(directory, name, 100);
    import_file(ImportArgs {
        input: input.clone(),
        json: true,
        migrate: false,
    })
    .unwrap();
    input
}

pub(crate) fn mask_args(input: PathBuf) -> MaskArgs {
    MaskArgs {
        input,
        update_masks: false,
        virtual_copy: None,
        json: true,
        list: false,
        add_ai_select: None,
        name: None,
        detail: None,
        add_luminance_range: false,
        range_min: None,
        range_max: None,
        add_color_range: false,
        hue_center: None,
        hue_width: None,
        sat_min: None,
        sat_max: None,
        lum_min: None,
        lum_max: None,
        feather: None,
        combine: None,
        inputs: None,
        duplicate: None,
        attach_layer: None,
        show_layer: None,
        hide_layer: None,
        local_layer: None,
        set_local_adjustments: Vec::new(),
        reset_local_adjustments: Vec::new(),
    }
}

pub(crate) fn mask_library_ids(input: &Path) -> Vec<String> {
    load_sidecar(&sidecar_path_for(input))
        .unwrap()
        .virtual_copies[0]
        .mask_library
        .iter()
        .map(|mask| mask.id.clone())
        .collect()
}

// ---- Review fixes (2026-08 wave): one-shot mask flags, per-copy zdata
// tiles, harmonized mask policy, batch collisions/resume, reindex exit
// codes, symlink-safe collection, overwrite guards, import hash check,
// dust-removal ordering. ----

/// Writes a tiny 2x2 PNG and returns its path plus the frame.
pub(crate) fn png_input(directory: &Path, name: &str, pixel: u8) -> (PathBuf, ImageFrame) {
    let input = directory.join(name);
    let frame = ImageFrame::new(2, 2, vec![pixel; 16]).unwrap();
    fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    (input, frame)
}
