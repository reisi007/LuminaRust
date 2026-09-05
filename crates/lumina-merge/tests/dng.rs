//! MERGE-DNG-1 integration tests: writer roundtrip through LibRaw re-import.
//!
//! Decode context (documented, not asserted byte-exact): `lumina-raw`
//! `decode_file` runs the pinned LibRaw 0.22.2 full pipeline (demosaic n/a
//! for linear data, camera white balance, sRGB output, 8-bit quantisation).
//! The tests therefore assert geometry, EXIF identity, grey neutrality and
//! decode determinism — not pixel identity with the linear input.
//!
//! Timezone caveat (LibRaw `get_timestamp`, `misc_parsers.cpp`): the EXIF
//! `DateTimeOriginal` string carries no zone; LibRaw assumes UTC but parses
//! via `mktime` (process-local zone). The timestamp roundtrip is exact only
//! where the process zone is UTC (e.g. CI); elsewhere it shifts by the local
//! UTC offset. The test asserts *presence* plus the exact formatter contract
//! (`exif_timestamp_utc` unit tests), never an exact re-parsed value.

use lumina_merge::{
    encode_linear_dng, merge_dng_filename, write_merge_dng, DngError, DngExif, LinearImage,
};
use lumina_sidecar::MergeMode;

fn exif_full() -> DngExif {
    DngExif {
        make: Some("LuminaMergeTest".into()),
        model: Some("HDR-Fixture-1".into()),
        lens: Some("TestLens 24-70/2.8".into()),
        exposure_time_s: Some(0.01),
        f_number: Some(8.0),
        iso_speed: Some(100),
        timestamp: Some(1_788_602_400),
    }
}

fn gradient(w: u32, h: u32) -> LinearImage {
    let mut px = Vec::with_capacity(w as usize * h as usize * 3);
    for y in 0..h {
        for x in 0..w {
            let v = ((x + y * w) % 251) as f32 / 255.0;
            px.extend_from_slice(&[v, v / 2.0, 1.0 - v]);
        }
    }
    LinearImage::new(w, h, px).unwrap()
}

#[test]
fn writer_roundtrip_hash_stable_and_reimportable() {
    let dir = tempfile::tempdir().unwrap();
    let img = gradient(32, 24);
    let file = merge_dng_filename("IMG_0001.ARW", MergeMode::Hdr).unwrap();
    assert_eq!(file, "IMG_0001-HDR.dng");

    let bytes_a = encode_linear_dng(&img, MergeMode::Hdr, &exif_full()).unwrap();
    let bytes_b = encode_linear_dng(&img, MergeMode::Hdr, &exif_full()).unwrap();
    assert_eq!(
        blake3::hash(&bytes_a),
        blake3::hash(&bytes_b),
        "same inputs -> byte-identical DNG (hash-stable)"
    );

    let path = write_merge_dng(dir.path(), &file, &img, MergeMode::Hdr, &exif_full()).unwrap();
    assert_eq!(path, dir.path().join("IMG_0001-HDR.dng"));
    assert!(
        !dir.path()
            .join(format!("{file}.tmp-{}", std::process::id()))
            .exists(),
        "no temp file left behind after atomic rename"
    );
    // The file on disk is exactly the encoded bytes (atomic write, no repair).
    assert_eq!(std::fs::read(&path).unwrap(), bytes_a);

    let meta = lumina_raw::read_metadata(&path).unwrap();
    assert_eq!((meta.width, meta.height), (32, 24));
    assert_eq!(meta.orientation, 1);
    assert_eq!(meta.camera_make.as_deref(), Some("LuminaMergeTest"));
    assert_eq!(meta.camera_model.as_deref(), Some("HDR-Fixture-1"));

    let img1 = lumina_raw::decode_file(&path).unwrap();
    let img2 = lumina_raw::decode_file(&path).unwrap();
    assert_eq!((img1.frame.width, img1.frame.height), (32, 24));
    assert_eq!(
        img1.frame.pixels, img2.frame.pixels,
        "re-import decode is deterministic (hash-stable)"
    );
    assert_eq!(img1.metadata.iso, Some(100.0));
    assert!((img1.metadata.shutter.unwrap() - 0.01).abs() < 1e-6);
    assert!((img1.metadata.aperture.unwrap() - 8.0).abs() < 1e-6);
    assert_eq!(img1.metadata.lens.as_deref(), Some("TestLens 24-70/2.8"));
    assert!(
        img1.metadata.timestamp.is_some(),
        "DateTimeOriginal carried over (exact value is TZ-dependent, see header)"
    );
}

#[test]
fn pano_suffix_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let file = merge_dng_filename("PANO_0101.NEF", MergeMode::Panorama).unwrap();
    assert_eq!(file, "PANO_0101-Pano.dng");
    let img = gradient(40, 24);
    let path = write_merge_dng(
        dir.path(),
        &file,
        &img,
        MergeMode::Panorama,
        &DngExif::default(),
    )
    .unwrap();
    let meta = lumina_raw::read_metadata(&path).unwrap();
    assert_eq!((meta.width, meta.height), (40, 24));
    // No EXIF supplied: Make/Model tags omitted. LibRaw then derives identity
    // from UniqueCameraModel (`tiff.cpp`: split at the first space) with
    // `make` defaulting to "DNG" — deterministic LibRaw behaviour, not an
    // EXIF fallback of ours. True absence (ISO) stays absent.
    let decoded = lumina_raw::decode_file(&path).unwrap();
    assert_eq!(decoded.metadata.camera_make.as_deref(), Some("LuminaMerge"));
    assert_eq!(decoded.metadata.camera_model.as_deref(), Some("Pano"));
    assert!(decoded.metadata.iso.is_none());
}

#[test]
fn grey_stays_grey_and_tone_is_plausible() {
    // Decode context: `decode_file` runs LibRaw with auto-brighten enabled
    // (`no_auto_bright = 0`, like every other source), so a uniform mid-grey
    // frame is scaled towards white — absolute mid-tone values are *not*
    // asserted. Anchors: black stays black, white stays white, neutral input
    // decodes neutral (identity white balance + sRGB matrix roundtrip), and a
    // grey ramp decodes monotonically (HDR range preserved, not posterised).
    let dir = tempfile::tempdir().unwrap();
    for (level, name, expect) in [
        (0.0f32, "black", 0u8),
        (0.25, "grey", 255u8),
        (1.0, "white", 255u8),
    ] {
        let img = LinearImage::solid(32, 24, [level, level, level]);
        let path = write_merge_dng(
            dir.path(),
            &format!("tone-{name}.dng"),
            &img,
            MergeMode::Hdr,
            &DngExif::default(),
        )
        .unwrap();
        let decoded = lumina_raw::decode_file(&path).unwrap();
        let px = &decoded.frame.pixels;
        assert_eq!(px.len(), 32 * 24 * 4);
        let c = (12 * 32 + 16) * 4;
        let (r, g, b, a) = (px[c], px[c + 1], px[c + 2], px[c + 3]);
        assert_eq!(a, 255);
        assert!(
            r.abs_diff(g) <= 4 && g.abs_diff(b) <= 4 && r.abs_diff(b) <= 4,
            "grey neutrality violated at level {level}: {r} {g} {b}"
        );
        assert!(
            r.abs_diff(expect) <= 4 && g.abs_diff(expect) <= 4 && b.abs_diff(expect) <= 4,
            "tone {name} (level {level}) decoded to {r} {g} {b}, expected ~{expect} (auto-bright context)"
        );
    }

    // Grey ramp 0..1 across 32 px: decode must be monotone (no clipping of
    // the interior steps, no inversion).
    let mut px = Vec::with_capacity(32 * 24 * 3);
    for _ in 0..24 {
        for x in 0..32 {
            let v = x as f32 / 31.0;
            px.extend_from_slice(&[v, v, v]);
        }
    }
    let ramp = LinearImage::new(32, 24, px).unwrap();
    let path = write_merge_dng(
        dir.path(),
        "ramp.dng",
        &ramp,
        MergeMode::Hdr,
        &DngExif::default(),
    )
    .unwrap();
    let decoded = lumina_raw::decode_file(&path).unwrap();
    let out = &decoded.frame.pixels;
    let mut prev = 0u8;
    for x in 0..32 {
        let r = out[(12 * 32 + x) * 4];
        assert!(
            r >= prev.saturating_sub(1),
            "ramp not monotone at x={x}: {r} < {prev}"
        );
        prev = r;
    }
    assert!(prev > out[12 * 32 * 4], "ramp must span a visible range");
}

#[test]
fn envelope_type_key_is_not_a_recipe_field() {
    // Envelope decision (MERGE-DNG-1, normative in the decision document):
    // `"type": "merge"` lives on the embedding sidecar document, never in
    // `MergeRecipe` (`deny_unknown_fields` rejects it loudly).
    let recipe = lumina_sidecar::MergeRecipe::from_json(&minimal_recipe_json()).unwrap();
    assert_eq!(recipe.output.file, "IMG_0001-HDR.dng");
    let mut parsed: serde_json::Value = serde_json::from_str(&recipe.to_json().unwrap()).unwrap();
    parsed["type"] = serde_json::Value::from("merge");
    let err = lumina_sidecar::MergeRecipe::from_json(&serde_json::to_string(&parsed).unwrap())
        .expect_err("`type` must not be a recipe field");
    assert!(
        matches!(err, lumina_sidecar::SidecarError::Json(_)),
        "unknown-field rejection is a loud parse error, got: {err}"
    );
}

fn minimal_recipe_json() -> String {
    let hash = format!("blake3:{}", "ab".repeat(32));
    serde_json::json!({
        "merge_version": 1,
        "mode": "hdr",
        "sources": [
            {"path": "IMG_0001.ARW", "content_hash": hash,
             "decode_context": {"decoder": "libraw", "decode_version": "0.22.2+luminaabi3", "orientation": 1},
             "exposure": {"exposure_time_s": 0.01, "iso": 100, "f_number": 8.0}},
            {"path": "IMG_0002.ARW", "content_hash": hash,
             "decode_context": {"decoder": "libraw", "decode_version": "0.22.2+luminaabi3", "orientation": 1},
             "exposure": {"exposure_time_s": 0.04, "iso": 100, "f_number": 8.0}}
        ],
        "alignment": {"method": "hdr_translate",
            "transforms": [{"source_index": 1, "matrix_3x3": [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0]}],
            "residual_px": 0.4, "projection": "none", "blend_width_px": 64},
        "output": {"file": "IMG_0001-HDR.dng", "bits": 16, "mosaic": false},
        "created_at": "2026-09-05T00:00:00Z",
        "status": "ok",
        "error": null
    })
    .to_string()
}

#[test]
fn changed_pixels_change_the_artefact_hash() {
    // Stale anchor on writer level: any pixel change alters the DNG bytes,
    // so a changed source set can never silently match a stored artefact.
    let img = gradient(32, 24);
    let base = encode_linear_dng(&img, MergeMode::Hdr, &exif_full()).unwrap();
    let mut px = img.pixels().to_vec();
    px[0] += 0.001;
    let changed_img = LinearImage::new(32, 24, px).unwrap();
    let changed = encode_linear_dng(&changed_img, MergeMode::Hdr, &exif_full()).unwrap();
    assert_ne!(
        blake3::hash(&base),
        blake3::hash(&changed),
        "pixel change must alter the artefact hash (stale detection)"
    );
}

#[test]
fn missing_and_unsupported_paths_are_loud() {
    // Missing: re-import of a nonexistent file is a loud error, never an
    // empty frame or silent fallback.
    let dir = tempfile::tempdir().unwrap();
    assert!(lumina_raw::decode_file(dir.path().join("gone-HDR.dng")).is_err());
    assert!(lumina_raw::read_metadata(dir.path().join("gone-HDR.dng")).is_err());

    // Unsupported: frames below the LibRaw 22px minimum cannot be written.
    let tiny = LinearImage::solid(8, 8, [0.3, 0.3, 0.3]);
    let err = write_merge_dng(
        dir.path(),
        "tiny-HDR.dng",
        &tiny,
        MergeMode::Hdr,
        &exif_full(),
    )
    .unwrap_err();
    assert!(matches!(err, DngError::Unsupported(_)), "got: {err}");
    assert!(!dir.path().join("tiny-HDR.dng").exists());
    assert!(
        !dir.path()
            .join(format!("tiny-HDR.dng.tmp-{}", std::process::id()))
            .exists(),
        "failed write leaves no temp file behind"
    );

    // Invalid: absolute or escaping file names are rejected before any IO.
    let img = gradient(32, 24);
    for bad in ["/abs/x-HDR.dng", "../x-HDR.dng", ""] {
        let err = write_merge_dng(dir.path(), bad, &img, MergeMode::Hdr, &exif_full()).unwrap_err();
        assert!(matches!(err, DngError::Invalid(_)), "`{bad}`: got: {err}");
    }

    // Io: missing target directory is a loud IO error, not a silent skip.
    let err = write_merge_dng(
        &dir.path().join("no-such-dir"),
        "x-HDR.dng",
        &img,
        MergeMode::Hdr,
        &exif_full(),
    )
    .unwrap_err();
    assert!(matches!(err, DngError::Io { .. }), "got: {err}");
}

#[test]
fn plain_tiff_without_dng_tags_is_no_substitute() {
    // No-silent-substitute anchor: a baseline RGB TIFF (same pixels, no DNG
    // tags) is rejected by LibRaw — the DNG tags are mandatory, and a
    // TIFF/PNG "DNG replacement" must surface as `unsupported`, never decode.
    use tiff::encoder::{colortype, TiffEncoder};
    let dir = tempfile::tempdir().unwrap();
    let img = gradient(32, 24);
    let (samples, _) = lumina_merge::linear_to_u16(&img);
    let path = dir.path().join("plain.tiff");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        let mut enc = TiffEncoder::new(&mut f).unwrap();
        enc.write_image::<colortype::RGB16>(32, 24, &samples)
            .unwrap();
    }
    let err = lumina_raw::decode_file(&path).expect_err("baseline TIFF must be rejected");
    assert!(err.to_string().contains("Unsupported"), "got: {err}");
}

#[test]
fn decode_context_documents_pinned_decoder() {
    // Decode-context anchor: the re-import path runs the pinned LibRaw with
    // the Lumina ABI generation suffix; caches/artefacts key on this string.
    let version = lumina_raw::libraw_decode_version();
    assert!(
        version.contains("+luminaabi"),
        "decode version must carry the ABI generation, got: {version}"
    );
    println!("decode context under test: {version}");
}
