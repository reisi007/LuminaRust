use super::*;

/// REVIEW-SIDECAR-TMP-1 helper: backdates a file's modification time so a
/// recovery sweep treats it as an orphaned crash leftover.
pub(crate) fn backdate(path: &Path, age: Duration) {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("backdate target must exist");
    file.set_modified(SystemTime::now() - age)
        .expect("set_modified must succeed on the host filesystem");
}

pub(crate) fn source() -> SourceIdentity {
    SourceIdentity {
        relative_name: "IMG_0001.ARW".into(),
        content_hash: "sha256:x".into(),
        byte_length: 42,
        modified_at: None,
        raw_format: "ARW".into(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "test".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: 10,
            height: 20,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        extras: Extras::new(),
    }
}

pub(crate) fn mask(id: &str) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "sha256:x".into(),
            byte_length: 42,
            extras: Extras::new(),
        },
        decode_context: source().decode_fingerprint.clone(),
        geometry_context: source().geometry_fingerprint.clone(),
        model: ModelIdentity {
            name: "model".into(),
            version: "1".into(),
            hash: "sha256:model".into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width: 10,
            height: 20,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "standard".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "bilinear".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status: MaskStatus::Valid,
        created_at: "2026-01-01T00:00:00Z".into(),
        generator_version: "generator-1".into(),
        error_text: None,
        artifact: None,
        operation: MaskOperation::Source,
        references: vec![],
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    }
}

// ---- LRPAR-G14-REDEYE-15: red_eye recipe schema field ----

pub(crate) fn red_eye_region(id: &str) -> RedEyeRegion {
    RedEyeRegion {
        id: id.into(),
        x: 0.25,
        y: 0.35,
        radius: 0.05,
        desaturate: 0.8,
        darken: 0.4,
    }
}

// ---- LRPAR-G06-UPRIGHT-15: upright recipe stage ----

pub(crate) fn upright_analysis() -> UprightAnalysis {
    UprightAnalysis {
        fingerprint: AnalysisFingerprint {
            algorithm: "upright-lines-v1".into(),
            version: "1".into(),
            input_fingerprint: "blake3:abc".into(),
            extras: Extras::new(),
        },
        vertical: 0.2,
        horizontal: -0.1,
        rotation: 0.05,
        line_count: 1234,
        confidence: 0.7,
    }
}

// ---- F-042-N1: source_actions recipe schema field ----

pub(crate) fn source_action_spec(version: u16, kind: SourceActionKind) -> SourceActionSpec {
    SourceActionSpec {
        version,
        kind,
        artifact: SourceActionArtifactRef {
            id: "repair-1".into(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            checksum: "blake3:abc".into(),
        },
    }
}

// ---- GEN-ZDATA-LINK-1: generative zdata recipe links ----

pub(crate) fn generative_link() -> GenerativeArtifactRef {
    GenerativeArtifactRef {
        id: "gen-canvas-1".into(),
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: "blake3:abc123".into(),
        width: 6000,
        height: 4000,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    }
}

pub(crate) fn generative_edit_with_link() -> GenerativeEdit {
    GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: Some(generative_link()),
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: None,
        seed: Some(42),
        prompt: Some("extend the sky".into()),
        extras: Extras::new(),
    }
}

pub(crate) fn spot_removal(
    mode: SpotRemovalMode,
    artifact: Option<GenerativeArtifactRef>,
) -> SpotRemoval {
    SpotRemoval {
        version: SPOT_REMOVAL_VERSION,
        mode,
        artifact,
    }
}

pub(crate) fn heuristic_spot_extra() -> Value {
    serde_json::json!({
        "id": "spot-1",
        "version": 1,
        "mode": "heuristic",
        "center_x": 0.25,
        "center_y": 0.5,
        "radius": 2.0,
        "feather": 0.5,
        "offset_dx": 0.5,
        "offset_dy": 0.0,
        "opacity": 1.0,
        "status": "valid"
    })
}

// ----- F-077: zdata-gated helpers -----

#[cfg(feature = "zdata")]
pub(crate) fn f077_tiles() -> Vec<MaskTile> {
    vec![MaskTile {
        mask_id: "subject".into(),
        tile_x: 0,
        tile_y: 0,
        width: 2,
        height: 2,
        values: vec![0, 1, 32768, 65535],
    }]
}

// =====================================================================
// G-15 META-MVP Slice 1: keywords, static collections, smart-collection
// criteria as data, and the batch-operation model.
// =====================================================================

pub(crate) fn meta_document() -> SidecarDocument {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.keywords = vec!["landscape".into(), "alps 2026".into()];
    d.collections = vec![
        CollectionMembership {
            id: "col-best".into(),
            name: "Best of 2026".into(),
        },
        CollectionMembership {
            id: "col-print".into(),
            name: "Print".into(),
        },
    ];
    d.virtual_copies[0].rating = 4;
    d.virtual_copies[0].flag = Flag::Pick;
    d
}

pub(crate) fn smart_def(rule: SmartRule) -> SmartCollectionDef {
    SmartCollectionDef {
        version: SMART_COLLECTION_VERSION,
        id: "smart-1".into(),
        name: "Picks".into(),
        rule,
    }
}

// =====================================================================
// LRPAR-G15-IPTC-S1: Sidecar-`metadata`-Draft (Datenmodell, Validierung,
// Historie, CAS-Persistenz).
// =====================================================================

pub(crate) fn draft_fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

pub(crate) fn metadata_doc() -> SidecarDocument {
    SidecarDocument::new(source(), "pipeline-1")
}
