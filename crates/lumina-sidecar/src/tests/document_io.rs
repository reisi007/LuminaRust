use super::*;

#[test]
fn sidecar_path_keeps_full_source_name() {
    assert_eq!(
        sidecar_path_for(Path::new("/photos/IMG_0001.ARW")),
        PathBuf::from("/photos/IMG_0001.ARW.lumina.json")
    );
}

#[test]
fn file_roundtrip_and_missing_case() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("photo.png");
    let path = sidecar_path_for(&source_path);
    let document = SidecarDocument::new(source(), "pipeline-1");
    save_sidecar(&path, &document).unwrap();
    assert_eq!(load_sidecar(&path).unwrap(), document);
    assert!(matches!(
        load_sidecar(&directory.path().join("missing.json")),
        Err(SidecarError::Missing(_))
    ));
}

#[test]
fn corrupt_json_is_reported() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photo.png.lumina.json");
    std::fs::write(&path, b"{not-json").unwrap();
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
}

#[test]
fn unknown_fields_roundtrip() {
    let json = r#"{"format":"lumina-sidecar","schema_version":1,"pipeline_version":"p","source":{"relative_name":"x.raw","content_hash":"h","byte_length":1,"modified_at":null,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0},"future":42},"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[],"future_copy":true}],"presets":[],"future_root":"kept"}"#;
    let d = SidecarDocument::from_json(json).unwrap();
    let out = d.to_json().unwrap();
    assert!(out.contains("future_root"));
    assert!(out.contains("future_copy"));
    assert!(out.contains("\"future\": 42"));
}

#[test]
fn schema_version_one_missing_operation_defaults_to_source() {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].mask_library.push(mask("legacy"));
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["virtual_copies"][0]["mask_library"][0]
        .as_object_mut()
        .unwrap()
        .remove("operation");

    let legacy_json = serde_json::to_string(&value).unwrap();
    let decoded = SidecarDocument::from_json(&legacy_json).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].mask_library[0].operation,
        MaskOperation::Source
    );
    let roundtripped: Value = serde_json::from_str(&decoded.to_json().unwrap()).unwrap();
    assert_eq!(
        roundtripped["virtual_copies"][0]["mask_library"][0]["operation"],
        "source"
    );
}

#[test]
fn unsafe_paths_are_rejected() {
    for path in [
        "../outside",
        "a/../../x",
        "/tmp/x",
        "C:\\x",
        "C:/x",
        "\\\\server\\share\\x",
        "a\\b",
        "a/./b",
    ] {
        let mut d = SidecarDocument::new(source(), "p");
        d.source.relative_name = path.into();
        assert!(d.validate().is_err(), "{path}");
    }
}

#[test]
fn default_and_original_are_exact() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].id = "other".into();
    assert!(d.validate().is_err());
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies.push(d.virtual_copies[0].clone());
    assert!(d.validate().is_err());
}
