use super::*;
use lumina_sidecar::save_sidecar;
mod sec6_regression;

fn explicit_exposures(count: usize) -> Vec<MergeExposure> {
    (0..count)
        .map(|index| MergeExposure {
            exposure_time_s: 0.01 * (index as f64 + 1.0),
            iso: 100,
            f_number: 8.0,
        })
        .collect()
}

fn options() -> MergeRunOptions {
    MergeRunOptions {
        output: None,
        max_shift_px: 16,
        blend_width_px: 64,
        gain_compensation: PanoGainMode::Off,
        deghost: HdrDeghostMode::Off,
        force: false,
        dng_decode_version: "test".into(),
    }
}

fn options_with_output(output: &Path) -> MergeRunOptions {
    MergeRunOptions {
        output: Some(output.to_path_buf()),
        ..options()
    }
}

/// Minimal frontend-style decode adapter for the PNG test fixtures.
fn decode_source(path: &Path) -> Result<MergeSourceFrame, MergeRunError> {
    let bytes = fs::read(path).map_err(|error| {
        MergeRunError::Missing(format!("cannot read source `{}`: {error}", path.display()))
    })?;
    let frame = lumina_core::ImageFrame::decode(&bytes)
        .map_err(|error| MergeRunError::Unsupported(error.to_string()))?;
    let linear = linear_from_rgba(&frame)
        .ok_or_else(|| MergeRunError::Unsupported("malformed RGBA8 frame".to_string()))?;
    Ok(MergeSourceFrame {
        path: path.to_path_buf(),
        content_hash: merge_checksum(&bytes),
        frame: linear,
        decode_context: MergeDecodeContext {
            decoder: "image".into(),
            decode_version: env!("CARGO_PKG_VERSION").into(),
            orientation: 1,
        },
        exif: MergeExif::default(),
    })
}

fn resolve_explicit(
    exposures: &[MergeExposure],
) -> impl FnMut(MergeMode, usize, &MergeSourceFrame) -> Result<MergeExposure, MergeRunError> + '_ {
    move |_mode, index, _source| Ok(exposures[index].clone())
}

/// Runs the shared orchestrator end-to-end through a frontend-style
/// decode closure and explicit per-source exposures.
fn run_hdr_with(
    inputs: &[PathBuf],
    options: &MergeRunOptions,
) -> Result<MergeRunOutcome, MergeRunError> {
    let exposures = explicit_exposures(inputs.len());
    run_merge(
        MergeMode::Hdr,
        inputs,
        options,
        decode_source,
        resolve_explicit(&exposures),
    )
}

fn run_hdr(inputs: &[PathBuf]) -> Result<MergeRunOutcome, MergeRunError> {
    run_hdr_with(inputs, &options())
}

fn write_gradient(dir: &Path, name: &str, level: u8) -> PathBuf {
    let pixels: Vec<u8> = (0..64 * 48)
        .flat_map(|i| {
            let x = (i % 64) as u8;
            let value = level.saturating_add(x % 16);
            [value, value, value, 255]
        })
        .collect();
    let png = lumina_core::ImageFrame::new(64, 48, pixels)
        .unwrap()
        .encode(lumina_core::ImageFileFormat::Png)
        .unwrap();
    let path = dir.join(name);
    fs::write(&path, png).unwrap();
    path
}

#[test]
fn run_merge_writes_bundle_and_reports_cached_on_second_run() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "a.png", 40);
    let b = write_gradient(dir.path(), "b.png", 160);
    let inputs = vec![a, b];
    let first = run_hdr(&inputs).unwrap();
    assert!(!first.cached);
    assert_eq!(first.dng_path, dir.path().join("a-HDR.dng"));
    assert!(first.dng_path.is_file());
    assert!(first.sidecar_path.is_file());
    let document = load_sidecar(&first.sidecar_path).unwrap();
    assert_eq!(classify_merge_document(&document), MergeDocumentKind::Merge);
    let recipe = stored_merge_recipe(&document).unwrap();
    assert_eq!(recipe.digest(), first.digest);
    let artifact = stored_merge_artifact(&document).unwrap();
    assert_eq!(artifact.checksum, first.checksum);
    assert_eq!(
        merge_checksum(&fs::read(&first.dng_path).unwrap()),
        first.checksum,
        "DNG checksum anchor"
    );

    // The envelope, recipe and artifact travel as sidecar extras and
    // survive a save/load roundtrip without absolute paths.
    let json = document.to_json().unwrap();
    assert!(json.contains("\"type\": \"merge\""));
    assert!(
        !json.contains(&dir.path().to_string_lossy().to_string()),
        "merge sidecar must stay bundle-relative: {json}"
    );
    let parsed = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(classify_merge_document(&parsed), MergeDocumentKind::Merge);
    assert_eq!(stored_merge_recipe(&parsed).unwrap().digest(), first.digest);
    assert_eq!(
        stored_merge_artifact(&parsed).unwrap().checksum,
        first.checksum
    );

    let second = run_hdr(&inputs).unwrap();
    assert!(second.cached, "unchanged inputs report the current bundle");
    assert_eq!(second.checksum, first.checksum);
}

#[test]
fn run_merge_is_deterministic_for_identical_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "d_a.png", 40);
    let b = write_gradient(dir.path(), "d_b.png", 160);
    let inputs = vec![a, b];
    let first = run_hdr(&inputs).unwrap();
    let second = run_hdr(&inputs).unwrap();
    assert_eq!(first.digest, second.digest);
    assert_eq!(first.checksum, second.checksum);
}

#[test]
fn standard_sidecar_envelope_conflict_is_loud_and_non_destructive() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "c_a.png", 40);
    let b = write_gradient(dir.path(), "c_b.png", 160);
    let inputs = vec![a, b];
    let first = run_hdr(&inputs).unwrap();
    let mut document = load_sidecar(&first.sidecar_path).unwrap();
    document.extras.remove("type");
    save_sidecar(&first.sidecar_path, &document).unwrap();
    let before = fs::read(&first.sidecar_path).unwrap();

    let error = run_hdr(&inputs).unwrap_err();
    assert!(matches!(error, MergeRunError::Stale(_)), "{error}");
    assert!(error.to_string().contains("standard sidecar"), "{error}");
    assert_eq!(fs::read(&first.sidecar_path).unwrap(), before);
}

#[test]
fn unknown_envelope_is_rejected_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "u_a.png", 40);
    let b = write_gradient(dir.path(), "u_b.png", 160);
    let inputs = vec![a, b];
    let first = run_hdr(&inputs).unwrap();
    let mut document = load_sidecar(&first.sidecar_path).unwrap();
    document
        .extras
        .insert("type".into(), serde_json::Value::from("merge-hdr"));
    save_sidecar(&first.sidecar_path, &document).unwrap();
    let error = run_hdr(&inputs).unwrap_err();
    assert!(matches!(error, MergeRunError::Stale(_)), "{error}");
    assert!(
        error.to_string().contains("unknown document type"),
        "{error}"
    );
}

#[test]
fn digest_mismatch_reports_stale_until_forced() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "s_a.png", 40);
    let b = write_gradient(dir.path(), "s_b.png", 160);
    let inputs = vec![a, b];
    run_hdr(&inputs).unwrap();
    // Change a source: the stored digest no longer matches.
    write_gradient(dir.path(), "s_b.png", 200);
    let error = run_hdr(&inputs).unwrap_err();
    assert!(matches!(error, MergeRunError::Stale(_)), "{error}");
    assert!(error.to_string().contains("merge stale"), "{error}");

    let mut forced = options();
    forced.force = true;
    let outcome = run_hdr_with(&inputs, &forced).unwrap();
    assert!(!outcome.cached);
}

#[test]
fn missing_dng_with_sidecar_present_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "m_a.png", 40);
    let b = write_gradient(dir.path(), "m_b.png", 160);
    let inputs = vec![a, b];
    let first = run_hdr(&inputs).unwrap();
    fs::remove_file(&first.dng_path).unwrap();
    let error = run_hdr(&inputs).unwrap_err();
    assert!(matches!(error, MergeRunError::Missing(_)), "{error}");
    assert!(error.to_string().contains("merge missing"), "{error}");
}

#[test]
fn source_count_and_shift_are_validated_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "v_a.png", 40);
    let error = run_hdr(&[a]).unwrap_err();
    assert!(error.to_string().contains("at least 2"), "{error}");

    let dir = tempfile::tempdir().unwrap();
    let a = write_gradient(dir.path(), "n_a.png", 40);
    let b = write_gradient(dir.path(), "n_b.png", 160);
    let mut bad = options();
    bad.max_shift_px = -1;
    let error = run_hdr_with(&[a, b], &bad).unwrap_err();
    assert!(error.to_string().contains("must be >= 0"), "{error}");
}

#[test]
fn exif_accessors_reject_out_of_range_values() {
    let exif = MergeExif {
        exposure_time_s: Some(f32::NAN),
        f_number: Some(f32::INFINITY),
        iso: Some(0.0),
        ..MergeExif::default()
    };
    assert_eq!(exif.valid_exposure_time_s(), None);
    assert_eq!(exif.valid_f_number(), None);
    assert_eq!(exif.valid_iso(), None);
    assert_eq!(exif.exposure(), None);

    let exif = MergeExif {
        exposure_time_s: Some(0.01),
        f_number: Some(8.0),
        iso: Some(100.0),
        ..MergeExif::default()
    };
    let exposure = exif.exposure().unwrap();
    assert!((exposure.exposure_time_s - 0.01).abs() < 1e-9);
    assert_eq!(exposure.iso, 100);
}

#[test]
fn classify_covers_standard_merge_and_unknown() {
    let make = |value: Option<&str>| {
        let source = SourceIdentity {
            relative_name: "a-HDR.dng".into(),
            content_hash: merge_checksum(b"x"),
            byte_length: 1,
            modified_at: None,
            raw_format: "DNG".into(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "libraw".into(),
                version: "test".into(),
                parameters: std::collections::BTreeMap::new(),
                extras: std::collections::BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 4,
                height: 4,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: std::collections::BTreeMap::new(),
            },
            extras: std::collections::BTreeMap::new(),
        };
        let mut document = SidecarDocument::new(source, MERGE_PIPELINE_VERSION);
        if let Some(value) = value {
            document
                .extras
                .insert("type".into(), serde_json::Value::from(value));
        }
        document
    };
    assert_eq!(
        classify_merge_document(&make(None)),
        MergeDocumentKind::Standard
    );
    assert_eq!(
        classify_merge_document(&make(Some("merge"))),
        MergeDocumentKind::Merge
    );
    assert_eq!(
        classify_merge_document(&make(Some("pano"))),
        MergeDocumentKind::Unknown("pano".into())
    );
}

#[test]
fn bundle_relative_paths_cover_same_dir_subdir_and_absolute() {
    let bundle = Path::new("/tmp/bundle");
    let inputs = vec![
        PathBuf::from("/tmp/bundle/a.png"),
        PathBuf::from("/tmp/bundle/sub/b.png"),
    ];
    assert_eq!(
        bundle_relative_paths(bundle, &inputs).unwrap(),
        vec!["a.png", "sub/b.png"]
    );
    assert!(bundle_relative_paths(bundle, &[PathBuf::from("/tmp/other/a.png")]).is_err());
    assert!(bundle_relative_paths(bundle, &[PathBuf::from("/tmp/bundle/../a.png")]).is_err());
}

#[test]
fn linear_conversion_is_exact_and_loud() {
    let frame = lumina_core::ImageFrame::new(2, 1, vec![255, 0, 0, 128, 0, 128, 0, 255]).unwrap();
    let linear = linear_from_rgba(&frame).unwrap();
    assert_eq!((linear.width(), linear.height()), (2, 1));
    assert_eq!(&linear.pixels()[0..3], &[1.0, 0.0, 0.0]);
    // FRAME-INVARIANT-SEAL-77: the dimensions side of the invariant is
    // sealed too — `width`/`height` are private, so a frame whose
    // dimensions no longer match its buffer can no longer be produced
    // by mutation from outside the crate; `new` rejects the mismatch
    // outright instead of the conversion guard ever seeing it.
    let broken = lumina_core::ImageFrame::new(3, 1, vec![255, 0, 0, 128, 0, 128, 0, 255]);
    assert!(
        broken.is_err(),
        "a frame whose dimensions do not match its buffer must be rejected, not reshaped"
    );
    // `new` still rejects a mismatched length outright.
    let broken = lumina_core::ImageFrame::new(2, 1, vec![255, 0, 0, 128, 0, 128, 0]);
    assert!(broken.is_err());
}
