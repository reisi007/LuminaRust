use super::*;

#[test]
fn generative_link_status_delegates_to_artifact_status() {
    // Missing: nothing on disk.
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(
        generative_link().artifact_status(directory.path()),
        ArtifactStatus::Missing
    );
    // Corrupt: undersized file can never be a bundle.
    std::fs::write(directory.path().join("IMG_0001.ARW.lumina.zdata"), b"short").unwrap();
    assert_eq!(
        generative_link().artifact_status(directory.path()),
        ArtifactStatus::Corrupt
    );
    // Corrupt: zdata-declared format without container magic is mislabeled.
    std::fs::write(
        directory.path().join("IMG_0001.ARW.lumina.zdata"),
        b"definitely not a container, long enough",
    )
    .unwrap();
    assert_eq!(
        generative_link().artifact_status(directory.path()),
        ArtifactStatus::Corrupt
    );
    // Available (structural): opaque non-container payloads pass checks
    // 1-2; deep checksum verification of real bundles is covered by the
    // zdata-gated end-to-end test below.
    let opaque = ArtifactReference {
        relative_path: "payload.bin".into(),
        format: "opaque".into(),
        checksum: "c".into(),
        width: 1,
        height: 1,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    std::fs::write(directory.path().join("payload.bin"), b"12345678").unwrap();
    assert_eq!(
        artifact_status(directory.path(), &opaque),
        ArtifactStatus::Available
    );
}

// End-to-end bundle linkage needs the codec.
#[cfg(feature = "zdata")]
#[test]
fn generative_links_resolve_against_real_bundle_eager() {
    use crate::{GenerativeCanvasArtifact, SpotHealGenerativeArtifact};
    let directory = tempfile::tempdir().unwrap();
    let bundle = directory.path().join("IMG_0001.ARW.lumina.zdata");
    let canvas = GenerativeCanvasArtifact {
        id: "gen-canvas-1".into(),
        width: 2,
        height: 2,
        pixels: vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
        ],
    };
    let spot = SpotHealGenerativeArtifact {
        id: "spot-1".into(),
        width: 1,
        height: 1,
        pixels: vec![9, 9, 9, 255],
    };
    let container = ZDataContainer::new(vec![]).unwrap();
    let container = container.add_generative_canvas(canvas.clone()).unwrap();
    let container = container.add_spot_heal_generative(spot.clone()).unwrap();
    save_zdata(&bundle, &container).unwrap();

    let canvas_link = GenerativeArtifactRef {
        id: canvas.id.clone(),
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: canvas.checksum(),
        width: canvas.width,
        height: canvas.height,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    let spot_link = GenerativeArtifactRef {
        id: spot.id.clone(),
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: spot.checksum(),
        width: spot.width,
        height: spot.height,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    // Recipe carrying both links validates.
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    let mut edit = generative_edit_with_link();
    edit.artifact = Some(canvas_link.clone());
    document.virtual_copies[0].recipe.generative_edit = Some(edit);
    document.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
        SpotRemovalMode::Generative,
        Some(spot_link.clone()),
    )];
    document.validate().unwrap();
    // Eager status: intact bundle is Available for both links.
    assert_eq!(
        canvas_link.artifact_status(directory.path()),
        ArtifactStatus::Available
    );
    assert_eq!(
        spot_link.artifact_status(directory.path()),
        ArtifactStatus::Available
    );
    // Kind separation is strict: neither id resolves under the other kind.
    let loaded = load_zdata(&bundle).unwrap();
    assert!(loaded.spot_heal_generative(&canvas.id).is_err());
    assert!(loaded.generative_canvas(&spot.id).is_err());
    // Bitflip => eager Corrupt (never Available).
    let mut bytes = std::fs::read(&bundle).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 1;
    std::fs::write(&bundle, &bytes).unwrap();
    assert_eq!(
        canvas_link.artifact_status(directory.path()),
        ArtifactStatus::Corrupt
    );
    // Deleted bundle => Missing.
    std::fs::remove_file(&bundle).unwrap();
    assert_eq!(
        spot_link.artifact_status(directory.path()),
        ArtifactStatus::Missing
    );
}

// GEN-EXPAND-CACHE-1: the generative identity is additive in `extras` and
// must roundtrip without an explicit schema field.
#[test]
fn generative_identity_roundtrips_through_json() {
    let link = GenerativeArtifactRef {
        id: "gen-canvas-1".into(),
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: "blake3:abc".into(),
        width: 2,
        height: 2,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    // A link written before identity pinning carries none.
    assert!(link.identity().is_none());
    let legacy: GenerativeArtifactRef =
        serde_json::from_str(&serde_json::to_string(&link).unwrap()).unwrap();
    assert!(legacy.identity().is_none(), "no identity is invented");
    assert_eq!(legacy, link);

    // A pinned link roundtrips the identity verbatim.
    let pinned = link.clone().with_identity("gen:key-1");
    assert_eq!(pinned.identity(), Some("gen:key-1"));
    let back: GenerativeArtifactRef =
        serde_json::from_str(&serde_json::to_string(&pinned).unwrap()).unwrap();
    assert_eq!(back, pinned);
    assert_eq!(back.identity(), Some("gen:key-1"));
}

// GEN-EXPAND-CACHE-1: a persisted canvas may only be served when its pinned
// identity still matches the current generative identity; otherwise it is
// `Stale` (loud), never silently used.
#[cfg(feature = "zdata")]
#[test]
fn generative_artifact_status_is_identity_verified() {
    use crate::GenerativeCanvasArtifact;
    let directory = tempfile::tempdir().unwrap();
    let bundle = directory.path().join("IMG_0001.ARW.lumina.zdata");
    let canvas = GenerativeCanvasArtifact {
        id: "gen-canvas-1".into(),
        width: 2,
        height: 2,
        pixels: vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
        ],
    };
    let container = ZDataContainer::new(vec![])
        .unwrap()
        .add_generative_canvas(canvas.clone())
        .unwrap();
    save_zdata(&bundle, &container).unwrap();
    let link = GenerativeArtifactRef {
        id: canvas.id.clone(),
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: canvas.checksum(),
        width: canvas.width,
        height: canvas.height,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    }
    .with_identity("identity-A");

    // Matching identity + intact bundle = Available.
    assert_eq!(
        generative_artifact_status(directory.path(), &link, "identity-A"),
        GenerativeArtifactStatus::Available
    );
    // Recipe/seed/canvas changed => different identity => Stale.
    assert_eq!(
        generative_artifact_status(directory.path(), &link, "identity-B"),
        GenerativeArtifactStatus::Stale
    );
    // Legacy link without a pinned identity can never be proven current.
    let legacy = {
        let mut l = link.clone();
        l.extras.remove(GENERATIVE_IDENTITY_KEY);
        l
    };
    assert_eq!(
        generative_artifact_status(directory.path(), &legacy, "identity-A"),
        GenerativeArtifactStatus::Stale
    );
    // Missing bundle stays Missing regardless of identity.
    std::fs::remove_file(&bundle).unwrap();
    assert_eq!(
        generative_artifact_status(directory.path(), &link, "identity-A"),
        GenerativeArtifactStatus::Missing
    );
    // Corrupt bundle stays Corrupt.
    std::fs::write(&bundle, b"definitely not zdata").unwrap();
    assert_eq!(
        generative_artifact_status(directory.path(), &link, "identity-A"),
        GenerativeArtifactStatus::Corrupt
    );
}

// Bundle moves keep relative links valid.
#[cfg(feature = "zdata")]
#[test]
fn generative_links_survive_bundle_move() {
    use crate::GenerativeCanvasArtifact;
    let directory = tempfile::tempdir().unwrap();
    let from_dir = directory.path().join("a");
    std::fs::create_dir(&from_dir).unwrap();
    let bundle = from_dir.join("IMG_0001.ARW.lumina.zdata");
    let canvas = GenerativeCanvasArtifact {
        id: "gen-canvas-1".into(),
        width: 1,
        height: 1,
        pixels: vec![1, 2, 3, 255],
    };
    let container = ZDataContainer::new(vec![])
        .unwrap()
        .add_generative_canvas(canvas.clone())
        .unwrap();
    save_zdata(&bundle, &container).unwrap();
    let link = GenerativeArtifactRef {
        id: canvas.id.clone(),
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: canvas.checksum(),
        width: 1,
        height: 1,
        channels: "rgba8".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    assert_eq!(link.artifact_status(&from_dir), ArtifactStatus::Available);
    // Move the whole bundle directory; the relative link stays valid.
    let to_dir = directory.path().join("b");
    std::fs::rename(&from_dir, &to_dir).unwrap();
    assert_eq!(link.artifact_status(&to_dir), ArtifactStatus::Available);
}

// GEN-ONNX-1: explicit regeneration replaces only the `generative_canvas`
// record; the default append path stays non-destructive.
#[cfg(feature = "zdata")]
#[test]
fn generative_canvas_replace_is_explicit_and_kind_safe() {
    use crate::GenerativeCanvasArtifact;
    let directory = tempfile::tempdir().unwrap();
    let bundle = directory.path().join("IMG_0001.ARW.lumina.zdata");
    let canvas = |value: u8| GenerativeCanvasArtifact {
        id: "gen-1".into(),
        width: 1,
        height: 1,
        pixels: vec![value, 2, 3, 255],
    };
    save_generative_canvas(&bundle, canvas(10), false).unwrap();
    // The non-destructive append path rejects the duplicate id.
    assert!(
        save_generative_canvas(&bundle, canvas(11), false).is_err(),
        "append must not silently overwrite an existing record"
    );
    // The explicit replace path installs the new bytes.
    save_generative_canvas(&bundle, canvas(11), true).unwrap();
    let container = load_zdata(&bundle).unwrap();
    assert_eq!(container.generative_canvas("gen-1").unwrap().pixels[0], 11);

    // The recipe link built from the record verifies `Available` and
    // round-trips its identity.
    let link = GenerativeArtifactRef::from_generative_canvas(
        &canvas(11),
        "IMG_0001.ARW.lumina.zdata",
        "id-1",
    );
    assert_eq!(
        link.artifact_status(directory.path()),
        ArtifactStatus::Available
    );
    assert_eq!(link.identity(), Some("id-1"));
    assert_eq!(
        generative_artifact_status(directory.path(), &link, "id-1"),
        GenerativeArtifactStatus::Available
    );
    assert_eq!(
        generative_artifact_status(directory.path(), &link, "id-2"),
        GenerativeArtifactStatus::Stale
    );
}

// GEN-ONNX-1 Welle 2a: the additive `negative_prompt` field roundtrips as a
// top-level JSON key and a mistyped value is rejected loudly.
#[test]
fn generative_negative_prompt_is_additive_and_validated() {
    let mut edit = GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: Some(7),
        prompt: Some("extend".into()),
        extras: Extras::new(),
    };
    assert_eq!(edit.negative_prompt(), None);
    assert!(edit.validate_edit_extras().is_ok());

    edit.set_negative_prompt(Some("blurry".into()));
    assert_eq!(edit.negative_prompt(), Some("blurry"));

    // JSON: additive top-level field, roundtrip-stable.
    let value = serde_json::to_value(&edit).unwrap();
    assert_eq!(value["negative_prompt"], serde_json::json!("blurry"));
    let back: GenerativeEdit = serde_json::from_value(value).unwrap();
    assert_eq!(back.negative_prompt(), Some("blurry"));

    // Clearing removes the key (absent, not implicitly empty).
    edit.set_negative_prompt(None);
    assert_eq!(edit.negative_prompt(), None);
    assert!(serde_json::to_value(&edit)
        .unwrap()
        .get("negative_prompt")
        .is_none());

    // A mistyped value is a hard error, never silently ignored.
    edit.extras
        .insert(GENERATIVE_NEGATIVE_PROMPT_KEY.into(), serde_json::json!(42));
    assert!(edit.validate_edit_extras().is_err());
    // `null` is accepted as identity.
    edit.extras.insert(
        GENERATIVE_NEGATIVE_PROMPT_KEY.into(),
        serde_json::Value::Null,
    );
    assert!(edit.validate_edit_extras().is_ok());
    assert_eq!(edit.negative_prompt(), None);
}
