use super::*;

/// R2-CLI-01 drift guard: the exported extension list stays pinned to the
/// SOLL format list in `feature/platform/cli-gui-wasm.md` (18 formats) and
/// contains no duplicates.
#[test]
fn raw_extensions_match_the_soll_format_list() {
    assert_eq!(
        RAW_EXTENSIONS.len(),
        18,
        "extension list drifted from the SOLL; update feature/platform/cli-gui-wasm.md + this test"
    );
    let mut unique = RAW_EXTENSIONS.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), RAW_EXTENSIONS.len(), "duplicate extensions");
    for expected in [
        "arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2", "crw", "pef", "srw", "3fr", "iiq",
        "rwl", "mos", "erf", "kdc", "x3f",
    ] {
        assert!(
            RAW_EXTENSIONS.contains(&expected),
            "SOLL format `{expected}` missing from RAW_EXTENSIONS"
        );
    }
}

/// R2-CLI-01: matching is ASCII-case-insensitive and rejects non-RAW
/// extensions (including Lumina's own sidecar suffixes).
#[test]
fn is_raw_extension_is_case_insensitive_and_strict() {
    for extension in RAW_EXTENSIONS {
        assert!(is_raw_extension(extension), "`{extension}` must match");
        let upper = extension.to_ascii_uppercase();
        assert!(is_raw_extension(&upper), "`{upper}` must match");
    }
    for foreign in [
        "png",
        "jpg",
        "jpeg",
        "webp",
        "txt",
        "lumina.json",
        "",
        "RAWS",
    ] {
        assert!(
            !is_raw_extension(foreign),
            "`{foreign}` must NOT be treated as RAW"
        );
    }
}

/// R2-CLI-04: `read_metadata_bytes` must report the same geometry,
/// orientation and EXIF identity as a FULL decode of the same fixture —
/// without running demosaic/processing. The portrait fixture carries dcraw
/// flip 5 → EXIF orientation 8, so this also pins the width/height swap
/// rule for the 90° family on the metadata-only path.
#[test]
fn read_metadata_matches_full_decode_on_portrait_fixture() {
    let bytes = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");
    let metadata = read_metadata_bytes(bytes, "aircraft-portrait.cr3").unwrap();
    // Pinned by the full-decode test `aircraft_portrait_fixture_applies_exif_orientation`.
    assert_eq!(metadata.orientation, 8);
    assert_eq!((metadata.width, metadata.height), (4024, 6032));
    assert_eq!(
        metadata.lens.as_deref(),
        Some("RF200-800mm F6.3-9 IS USM"),
        "metadata-only path must expose the same EXIF lens as a full decode"
    );
    assert_eq!(
        metadata
            .camera_make
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("canon")
    );
}

/// R2-CLI-04 landscape counterpart: identity orientation must keep
/// width/height unswapped on the metadata-only path.
#[test]
fn read_metadata_matches_full_decode_on_landscape_fixture() {
    let bytes = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
    let metadata = read_metadata_bytes(bytes, "aircraft-landscape.cr3").unwrap();
    // Pinned by `aircraft_landscape_fixture_has_expected_geometry_and_metadata`.
    assert_eq!(metadata.orientation, 1);
    assert_eq!((metadata.width, metadata.height), (6032, 4024));
}

/// R2-CLI-04: garbage input fails loudly (never `UnsupportedPlatform`),
/// mirroring the decode-path error contract.
#[test]
fn read_metadata_reports_non_raw_input_as_decode_error() {
    let error = read_metadata_bytes(b"not a raw", "bad.cr2").unwrap_err();
    assert!(
        !matches!(error, RawError::UnsupportedPlatform),
        "unexpected error shape: {error:?}"
    );
    assert!(read_metadata_bytes(&[], "empty.nef").is_err());
}

#[test]
fn non_raw_bytes_are_reported_as_decode_errors() {
    let error = decode_bytes(b"not a raw", "bad.cr2").unwrap_err();
    assert!(!matches!(error, RawError::UnsupportedPlatform));
}

#[test]
fn empty_bytes_are_rejected() {
    assert!(decode_bytes(&[], "empty.nef").is_err());
}

/// Pins [`DemosaicMethod::libraw_value`] against LibRaw's documented
/// `output_params.user_qual` semantics (R2-RAW-01): `dcraw_process`
/// dispatches 0=linear, 1=VNG, 2=PPG, 3=AHD, 4=DCB, 11=DHT, 12=AAHD.
///
/// Reference: official LibRaw API reference (`libraw_output_params_t.
/// user_qual`) and the interpolation dispatch chain in
/// `src/postprocessing/dcraw_process.cpp` of the linked 0.22.x sources —
/// any other value falls through the chain without selecting an
/// algorithm (silent fallback). Before this test existed, the mapping was
/// shifted by +1 (`Linear→1 … Aahd→13`), so every explicit choice ran the
/// wrong algorithm and AAHD silently degraded to AHD.
#[test]
fn demosaic_libraw_values_match_dcraw_process_user_qual_table() {
    let expected = [
        // The default deliberately maps to None: no `libraw_set_demosaic`
        // call at all, so dcraw_process uses its own quality selection.
        (DemosaicMethod::LibRawDefault, None),
        (DemosaicMethod::Linear, Some(0)),
        (DemosaicMethod::Vng, Some(1)),
        (DemosaicMethod::Ppg, Some(2)),
        (DemosaicMethod::Ahd, Some(3)),
        (DemosaicMethod::Dcb, Some(4)),
        (DemosaicMethod::Dht, Some(11)),
        (DemosaicMethod::Aahd, Some(12)),
    ];
    for (method, value) in expected {
        assert_eq!(
            method.libraw_value(),
            value,
            "{method:?} must map to user_qual {value:?}"
        );
    }
}

/// Guard against future variants reintroducing the silent fallback: every
/// explicit method must land on a `user_qual` that dcraw_process actually
/// dispatches on (0..=4, 11, 12) — never on an unmapped value.
#[test]
fn demosaic_values_never_hit_the_dcraw_silent_fallback() {
    let all = [
        DemosaicMethod::LibRawDefault,
        DemosaicMethod::Linear,
        DemosaicMethod::Vng,
        DemosaicMethod::Ppg,
        DemosaicMethod::Ahd,
        DemosaicMethod::Dcb,
        DemosaicMethod::Dht,
        DemosaicMethod::Aahd,
    ];
    for method in all {
        match method.libraw_value() {
            None => {} // default: intentionally no set_demosaic call
            Some(value) => assert!(
                matches!(value, 0..=4 | 11 | 12),
                "{method:?} → user_qual {value} would fall through the \
                 dcraw_process dispatch (silent fallback)"
            ),
        }
    }
}

/// FIXTURE-ENV-1: reads the committed CR3s via `include_bytes!` like the
/// neighbouring tests (no env gate, no skip branch), geometry pinned per fixture.
#[test]
fn committed_fixtures_decode_orientation_and_dimensions() {
    let landscape = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
    let portrait = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");
    let check = |(name, bytes, orientation, w, h): (&str, &[u8], u8, u32, u32)| {
        let RawImage { metadata, frame } = decode_bytes(bytes, name).unwrap();
        let len = w as usize * h as usize * 4;
        assert_eq!(metadata.orientation, orientation, "{name} orientation");
        assert_eq!((frame.width(), frame.height()), (w, h), "{name} frame");
        assert_eq!(frame.pixels().len(), len, "{name} buffer");
    };
    check(("aircraft-landscape.cr3", landscape, 1, 6032, 4024));
    check(("aircraft-portrait.cr3", portrait, 8, 4024, 6032));
}

#[test]
fn aircraft_landscape_fixture_has_expected_geometry_and_metadata() {
    let bytes = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
    let image = decode_bytes(bytes, "aircraft-landscape.cr3").unwrap();
    assert_eq!(image.metadata.orientation, 1);
    assert_eq!((image.frame.width(), image.frame.height()), (6032, 4024));
    assert_eq!((image.metadata.width, image.metadata.height), (6032, 4024));
    assert_eq!(image.frame.pixels().len(), 6032 * 4024 * 4);
}

/// Content-driven verification note (REVIEW-RAW-FLIP-1): a planned
/// luminance-band heuristic ("bright sky above dark ground") was measured
/// against this fixture and turned out FALSE (top-row mean luminance
/// ≈ 111.0 vs bottom-row ≈ 118.9; the scene has no reliable vertical
/// brightness gradient). Content-driven rotation verification is therefore
/// done against ground truth instead of scene assumptions:
/// `tests/abi_layout.rs` compares the promoted output byte-for-byte against
/// LibRaw's OWN rotation (`user_flip=k` → `dcraw_make_mem_image`, which
/// applies `flip_index`), for every dcraw flip 0..=7 and for this fixture
/// end-to-end.
#[test]
fn aircraft_portrait_fixture_applies_exif_orientation() {
    // The fixture carries dcraw flip 5, which is EXIF orientation **8**
    // ("Rotate 270 CW", confirmed via exiftool), NOT orientation 5. Before
    // REVIEW-RAW-FLIP-1 the raw flip value was persisted 1:1 as
    // `orientation` and the promotion step applied a transpose instead of
    // the correct 90° CCW rotation (dimensions matched by coincidence,
    // pixel content was mirrored).
    let bytes = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");
    let image = decode_bytes(bytes, "aircraft-portrait.cr3").unwrap();
    assert_eq!(image.metadata.orientation, 8);
    assert_eq!((image.frame.width(), image.frame.height()), (4024, 6032));
    assert_eq!((image.metadata.width, image.metadata.height), (4024, 6032));
    assert_eq!(image.frame.pixels().len(), 4024 * 6032 * 4);
}

#[test]
fn aircraft_fixtures_expose_exif_lens_model() {
    // REVIEW-RAW-N2: both committed CR3 fixtures carry the EXIF LensModel
    // tag 0xA434 with value "RF200-800mm F6.3-9 IS USM" (verified with
    // exiftool). LibRaw parses that tag into `imgdata.lens.Lens`, which
    // `RawMetadata.lens` must now expose verbatim instead of a constant
    // `None`. Pins the EXIF-first source priority end-to-end.
    let fixtures: [(&str, &[u8]); 2] = [
        (
            "aircraft-landscape.cr3",
            include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3"),
        ),
        (
            "aircraft-portrait.cr3",
            include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3"),
        ),
    ];
    for (name, bytes) in fixtures {
        let image = decode_bytes(bytes, name).unwrap();
        assert_eq!(
            image.metadata.lens.as_deref(),
            Some("RF200-800mm F6.3-9 IS USM"),
            "fixture {name} must report its EXIF lens model"
        );
    }
}

/// R2-RAW-04: the 16-bit decode path (`output_bits = 16` → LibRaw u16
/// output → high-byte promotion in `rgba_from_words`) previously ran only
/// in synthetic unit tests, so the real Budget-Gate/Promotion chain was
/// never exercised end-to-end. This test decodes a committed fixture at
/// 16 bit and pins the same geometry as the verified 8-bit decode.
#[test]
fn aircraft_landscape_fixture_decodes_with_output_bits_16() {
    let bytes = include_bytes!("../../../sample-data/raw/aircraft-landscape.cr3");
    let image = decode_bytes_with_options(
        bytes,
        "aircraft-landscape.cr3",
        &RawDecodeOptions {
            demosaicing: DemosaicMethod::LibRawDefault,
            output_bits: 16,
        },
    )
    .unwrap();
    assert_eq!(image.metadata.orientation, 1);
    assert_eq!((image.frame.width(), image.frame.height()), (6032, 4024));
    assert_eq!((image.metadata.width, image.metadata.height), (6032, 4024));
    assert_eq!(image.frame.pixels().len(), 6032 * 4024 * 4);
    // The committed fixtures decode as 3-channel RGB; the promoted RGBA
    // frame must be fully opaque on this path too.
    assert!(
        image
            .frame
            .pixels()
            .iter()
            .skip(3)
            .step_by(4)
            .all(|&a| a == 255),
        "16-bit promotion must emit opaque alpha"
    );
}

/// R2-RAW-02: `decode_file` must stream through `libraw_open_file`
/// instead of reading the whole RAW into a heap buffer first. Decoding a
/// temporary copy of a committed fixture exercises the file path
/// end-to-end and must produce the known geometry. The second half pins
/// the error contract: an unopenable path stays a `RawError::Io`.
#[test]
fn decode_file_streams_a_fixture_via_libraw_open_file() {
    let bytes = include_bytes!("../../../sample-data/raw/aircraft-portrait.cr3");
    let mut fixture = tempfile::NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut fixture, bytes).expect("fixture copy");

    let image = decode_file(fixture.path()).unwrap();
    assert_eq!(image.metadata.orientation, 8);
    assert_eq!((image.frame.width(), image.frame.height()), (4024, 6032));
    assert_eq!((image.metadata.width, image.metadata.height), (4024, 6032));
    assert_eq!(image.frame.pixels().len(), 4024 * 6032 * 4);

    let missing = decode_file("/nonexistent/lumina-streaming-probe.cr3").unwrap_err();
    assert!(
        matches!(missing, RawError::Io { .. }),
        "unopenable file must stay a RawError::Io, got {missing:?}"
    );
}

#[test]
fn dcraw_flip_maps_to_verified_exif_orientation_table() {
    // Verified against LibRaw 0.22.2: flip_index() bit semantics and the
    // inverse "50132467" lookup in src/metadata/tiff.cpp.
    let expected = [
        (0, 1), // identity
        (1, 2), // mirror horizontal
        (2, 4), // mirror vertical
        (3, 3), // rotate 180°
        (4, 5), // transpose (main diagonal)
        (5, 8), // rotate 90° CCW (= 270° CW)
        (6, 6), // rotate 90° CW
        (7, 7), // anti-transpose (anti-diagonal)
    ];
    for (flip, orientation) in expected {
        assert_eq!(
            dcraw_flip_to_exif_orientation(flip),
            orientation,
            "dcraw flip {flip} must map to EXIF orientation {orientation}"
        );
    }
}

#[test]
fn dcraw_flip_out_of_range_maps_to_no_rotation() {
    // After identify() LibRaw only reports 0..=7; unknown flips are reset
    // to 0 (= no rotation). Anything else must defensively behave the same
    // way instead of wrapping into a bogus orientation.
    for flip in [-5i32, -1, 8, 9, 90, 180, 270, i32::MAX, i32::MIN] {
        assert_eq!(
            dcraw_flip_to_exif_orientation(flip),
            1,
            "out-of-range flip {flip} must map to EXIF orientation 1"
        );
    }
}

/// Round-trip through LibRaw's own inverse table:
/// `src/metadata/tiff.cpp` maps EXIF orientation → flip via the string
/// `"50132467"[orientation & 7]`. Our translation must be its exact
/// inverse — this pins the table to LibRaw's semantics, not to a copy of
/// itself.
#[test]
fn exif_orientation_round_trips_through_libraw_inverse_table() {
    const LIBRAW_ORIENTATION_TO_FLIP: [u8; 8] = [5, 0, 1, 3, 2, 4, 6, 7]; // "50132467"
    for orientation in 1u8..=8 {
        let flip = LIBRAW_ORIENTATION_TO_FLIP[(orientation & 7) as usize];
        assert_eq!(
            dcraw_flip_to_exif_orientation(i32::from(flip)),
            orientation,
            "round trip failed for EXIF orientation {orientation}"
        );
    }
}

#[test]
fn libraw_version_is_present_and_well_formed() {
    let version = libraw_version().expect("native LibRaw should report a version");
    assert!(!version.is_empty(), "version must not be empty");
    let parts = version.split('.').collect::<Vec<_>>();
    assert!(
        parts.len() >= 2,
        "version should contain at least major.minor, got `{version}`"
    );
    assert!(
        parts.iter().all(|part| !part.is_empty()),
        "version parts must not be empty, got `{version}`"
    );
}

#[test]
fn libraw_decode_version_carries_decode_generation() {
    let decode_version = libraw_decode_version();
    assert!(
        decode_version.ends_with("+luminaabi3"),
        "decode identity must carry the current decode generation suffix, got `{decode_version}`"
    );
    assert!(
        decode_version.starts_with("0.22."),
        "native decode identity should start with the linked version, got `{decode_version}`"
    );
}
