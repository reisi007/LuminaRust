//! End-to-end property tests: splice pixel-identity, full roundtrips,
//! determinism, malformed inputs. No network, no external tools.

use lumina_iptc::{embed_metadata, extract_metadata, IptcMetadata};
use proptest::prelude::*;

/// Minimal valid JPEG (same layout as the crate fixture).
fn minimal_jpeg() -> Vec<u8> {
    lumina_iptc::test_fixture_jpeg()
}

/// Arbitrary text incl. multi-byte UTF-8 and XML-significant characters,
/// capped so every registry octet limit holds (≤ 24 chars ≈ ≤ 96 octets).
fn field_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just('a'),
            Just('Z'),
            Just('0'),
            Just(' '),
            Just('-'),
            Just('ä'),
            Just('ß'),
            Just('中'),
            Just('🎉'),
            Just('&'),
            Just('<'),
            Just('>'),
            Just('"'),
            Just('\''),
            Just('\n'),
        ],
        0..24,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn opt_field() -> impl Strategy<Value = Option<String>> {
    prop::option::of(field_text())
}

fn arb_meta() -> impl Strategy<Value = IptcMetadata> {
    (
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        opt_field(),
        prop::option::of(prop_oneof![
            Just("2026-09-04".to_string()),
            Just("2024-02-29".to_string()),
            Just("2000-01-01".to_string()),
        ]),
        prop::collection::vec(field_text(), 0..5),
    )
        .prop_map(
            |(
                title,
                headline,
                description,
                copyright_notice,
                creator,
                credit,
                source,
                city,
                state_province,
                country,
                date_created,
                keywords,
            )| {
                IptcMetadata {
                    title,
                    headline,
                    description,
                    copyright_notice,
                    creator,
                    credit,
                    source,
                    city,
                    state_province,
                    country,
                    date_created,
                    keywords,
                }
            },
        )
}

fn sos_offset(jpeg: &[u8]) -> usize {
    // First FF DA that starts the SOS marker (fixture has no earlier DA).
    jpeg.windows(2)
        .position(|w| w == [0xFF, 0xDA])
        .expect("SOS in fixture")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Splice is pixel-identical: the SOS tail is copied verbatim, and the
    /// merged re-read equals the normalized input.
    #[test]
    fn splice_keeps_pixel_bytes_identical(meta in arb_meta(), mut scan_extra in prop::collection::vec(any::<u8>(), 0..64)) {
        // Keep scan data free of accidental EOI; the tail must end with FFD9.
        scan_extra.retain(|b| *b != 0xFF);
        let mut jpeg = minimal_jpeg();
        let eoi = jpeg.len() - 2;
        jpeg.splice(eoi..eoi, scan_extra);
        let embedded = embed_metadata(&jpeg, &meta).expect("embed must succeed for valid meta");
        let a = sos_offset(&jpeg);
        let b = sos_offset(&embedded);
        prop_assert_eq!(&jpeg[a..], &embedded[b..], "SOS tail must be byte-identical");
        let back = extract_metadata(&embedded).expect("extract");
        prop_assert_eq!(back, meta.normalized());
    }

    /// Same input + same metadata → byte-identical output.
    #[test]
    fn embed_is_deterministic(meta in arb_meta()) {
        let jpeg = minimal_jpeg();
        prop_assert_eq!(
            embed_metadata(&jpeg, &meta).expect("embed a"),
            embed_metadata(&jpeg, &meta).expect("embed b")
        );
    }

    /// Raw IIM encode/decode roundtrips every registry field.
    #[test]
    fn iim_roundtrip_all_fields(meta in arb_meta()) {
        let norm = meta.normalized();
        let bytes = lumina_iptc::encode_iim(&norm).expect("encode");
        let back = lumina_iptc::decode_iim(&bytes).expect("decode");
        prop_assert_eq!(back, norm);
    }

    /// XMP build/parse roundtrips every registry field incl. escapes.
    #[test]
    fn xmp_roundtrip_all_fields(meta in arb_meta()) {
        let norm = meta.normalized();
        match lumina_iptc::build_xmp_packet(&norm).expect("build") {
            None => prop_assert!(norm.is_empty()),
            Some(packet) => {
                let back = lumina_iptc::parse_xmp_packet(&packet).expect("parse");
                prop_assert_eq!(back, norm);
            }
        }
    }

    /// Random tails can't break the splice as long as framing holds.
    #[test]
    fn malformed_inputs_always_fail_loudly(data in prop::collection::vec(any::<u8>(), 0..128)) {
        if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
            return Ok(()); // may be quasi-valid; covered elsewhere
        }
        let meta = IptcMetadata { title: Some("x".into()), ..IptcMetadata::default() };
        prop_assert!(embed_metadata(&data, &meta).is_err());
        prop_assert!(extract_metadata(&data).is_err());
    }
}

/// Limit violations name field + limit and never truncate.
#[test]
fn limit_errors_name_field_and_limit() {
    let over = "ä".repeat(200); // 400 octets
    let meta = IptcMetadata {
        city: Some(over),
        ..IptcMetadata::default()
    };
    let err = embed_metadata(&minimal_jpeg(), &meta).expect_err("must fail");
    let msg = err.to_string();
    assert!(msg.contains("city"), "error names field: {msg}");
    assert!(msg.contains("128"), "error names limit: {msg}");

    let long_kw = "k".repeat(129);
    let meta = IptcMetadata {
        keywords: vec![long_kw],
        ..IptcMetadata::default()
    };
    assert!(embed_metadata(&minimal_jpeg(), &meta).is_err());

    let many: Vec<String> = (0..513).map(|i| format!("k{i}")).collect();
    let meta = IptcMetadata {
        keywords: many,
        ..IptcMetadata::default()
    };
    let err = embed_metadata(&minimal_jpeg(), &meta).expect_err("must fail");
    assert!(err.to_string().contains("512"), "names entry limit: {err}");

    let meta = IptcMetadata {
        date_created: Some("2026-02-30".into()),
        ..IptcMetadata::default()
    };
    assert!(embed_metadata(&minimal_jpeg(), &meta).is_err());

    let meta = IptcMetadata {
        title: Some("ok\x07bell".into()),
        ..IptcMetadata::default()
    };
    assert!(embed_metadata(&minimal_jpeg(), &meta).is_err());
}
