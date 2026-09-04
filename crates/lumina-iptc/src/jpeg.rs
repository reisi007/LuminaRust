//! JPEG marker splice: embed/extract IPTC (APP13) + XMP (APP1).
//!
//! Insertion order is deterministic: `SOI, APP13 (Photoshop IPTC), APP1
//! (XMP), <all other original pre-SOS segments in original order>, SOS…EOI`.
//! Existing Photoshop-APP13 and XMP-APP1 segments are replaced, never
//! duplicated. Everything from SOS onward (scan data, pixel bytes) is copied
//! verbatim, so the splice never touches image pixels.

use crate::error::IptcError;
use crate::iim::APP13_HEADER;
use crate::iim::{build_app13, decode_iim, encode_iim, parse_app13};
use crate::registry::IptcMetadata;
use crate::xmp::{build_app1, build_xmp_packet, parse_app1, parse_xmp_packet, XMP_HEADER};

/// JPEG markers used by the splice.
const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const APP1: u8 = 0xE1;
const APP13: u8 = 0xED;

/// Maximum payload of a single JPEG segment (64 KiB minus marker framing).
const MAX_SEGMENT_PAYLOAD: usize = 0xFFFF - 2;

/// A length-prefixed segment before SOS.
#[derive(Debug, Clone)]
struct Segment {
    marker: u8,
    data: Vec<u8>,
}

/// Embed metadata into a JPEG, returning a new JPEG.
///
/// Steps: validate → parse markers → drop old Photoshop-APP13/XMP-APP1 →
/// insert fresh `APP13, APP1` right after SOI → copy the SOS tail verbatim.
/// Empty metadata strips existing IPTC/XMP segments and writes none.
/// Deterministic: same input + same metadata → byte-identical output.
pub fn embed_metadata(jpeg: &[u8], meta: &IptcMetadata) -> Result<Vec<u8>, IptcError> {
    meta.validate()?;
    let (segments, sos_off) = split_segments(jpeg)?;
    let mut kept = Vec::with_capacity(segments.len());
    for seg in &segments {
        if seg.marker == APP13 && seg.data.starts_with(APP13_HEADER) {
            continue; // replaced below
        }
        if seg.marker == APP1 && seg.data.starts_with(XMP_HEADER) {
            continue; // replaced below
        }
        kept.push(seg.clone());
    }
    let mut out = Vec::with_capacity(jpeg.len() + 4096);
    out.extend_from_slice(&[0xFF, SOI]);
    if !meta.normalized().is_empty() {
        let iim = encode_iim(meta)?;
        let app13 = build_app13(&iim);
        push_segment(&mut out, APP13, &app13, "iim")?;
        if let Some(packet) = build_xmp_packet(meta)? {
            let app1 = build_app1(&packet);
            push_segment(&mut out, APP1, &app1, "xmp")?;
        }
    }
    for seg in &kept {
        push_segment(&mut out, seg.marker, &seg.data, "meta")?;
    }
    out.extend_from_slice(&jpeg[sos_off..]);
    Ok(out)
}

/// Extract merged metadata from a JPEG (IIM ∪ XMP).
///
/// Absent IPTC/XMP segments mean "no embedded metadata" (`Ok(empty)`), not an
/// error. Present-but-broken segments are loud errors. Where both formats
/// carry a field, IIM wins; keywords are unioned (IIM order first).
pub fn extract_metadata(jpeg: &[u8]) -> Result<IptcMetadata, IptcError> {
    let (segments, _) = split_segments(jpeg)?;
    let mut from_iim: Option<IptcMetadata> = None;
    let mut from_xmp: Option<IptcMetadata> = None;
    for seg in &segments {
        if seg.marker == APP13 && seg.data.starts_with(APP13_HEADER) {
            if let Some(iim) = parse_app13(&seg.data)? {
                if from_iim.is_none() {
                    from_iim = Some(decode_iim(&iim)?);
                }
            }
        }
        if seg.marker == APP1 && seg.data.starts_with(XMP_HEADER) {
            let packet = parse_app1(&seg.data)?.ok_or_else(|| IptcError::InvalidXmp {
                reason: "XMP-APP1-Segment ohne Paket".into(),
            })?;
            if from_xmp.is_none() {
                from_xmp = Some(parse_xmp_packet(packet)?);
            }
        }
    }
    Ok(merge(from_iim, from_xmp))
}

/// Merge IIM and XMP reads: IIM wins per single field, keywords unioned.
fn merge(iim: Option<IptcMetadata>, xmp: Option<IptcMetadata>) -> IptcMetadata {
    let mut out = iim.unwrap_or_default();
    let x = xmp.unwrap_or_default();
    macro_rules! fill {
        ($field:ident) => {
            if out.$field.as_deref().is_none_or(|v| v.trim().is_empty()) {
                out.$field = x.$field;
            }
        };
    }
    fill!(title);
    fill!(headline);
    fill!(description);
    fill!(copyright_notice);
    fill!(creator);
    fill!(credit);
    fill!(source);
    fill!(city);
    fill!(state_province);
    fill!(country);
    fill!(date_created);
    for keyword in x.keywords {
        if !out.keywords.iter().any(|k| k == &keyword) {
            out.keywords.push(keyword);
        }
    }
    out
}

/// Split a JPEG into pre-SOS segments plus the byte offset of the SOS marker.
///
/// Loud errors for: non-JPEG input, truncation, impossible lengths, EOI
/// before SOS, missing EOI at the end.
fn split_segments(jpeg: &[u8]) -> Result<(Vec<Segment>, usize), IptcError> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != SOI {
        return Err(IptcError::NotJpeg);
    }
    let malformed = |reason: &str| IptcError::MalformedJpeg {
        reason: reason.to_string(),
    };
    let mut segments = Vec::new();
    let mut pos = 2;
    loop {
        if pos + 2 > jpeg.len() {
            return Err(malformed("unerwartetes Dateiende im Marker-Header"));
        }
        if jpeg[pos] != 0xFF {
            return Err(malformed("erwartetes Marker-Präfix 0xFF"));
        }
        // Skip fill bytes (tolerated), then read the marker byte.
        let mut m = pos + 1;
        while m < jpeg.len() && jpeg[m] == 0xFF {
            m += 1;
        }
        if m >= jpeg.len() {
            return Err(malformed("unerwartetes Dateiende im Marker"));
        }
        let marker = jpeg[m];
        if marker == EOI {
            return Err(malformed("EOI vor SOS"));
        }
        if marker == SOI || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            pos = m + 1; // standalone markers without length
            continue;
        }
        if m + 3 > jpeg.len() {
            return Err(malformed("abgeschnittene Segmentlänge"));
        }
        let len = u16::from_be_bytes([jpeg[m + 1], jpeg[m + 2]]) as usize;
        if len < 2 {
            return Err(malformed("Segmentlänge < 2"));
        }
        let data_start = m + 3;
        let data_end = m + 1 + len;
        if data_end > jpeg.len() {
            return Err(malformed("Segment länger als Restdatei"));
        }
        if marker == SOS {
            let tail = &jpeg[pos..];
            if tail.len() < 2 || tail[tail.len() - 2] != 0xFF || tail[tail.len() - 1] != EOI {
                return Err(malformed("fehlendes EOI am Dateiende"));
            }
            return Ok((segments, pos));
        }
        segments.push(Segment {
            marker,
            data: jpeg[data_start..data_end].to_vec(),
        });
        pos = data_end;
    }
}

/// Append one length-prefixed segment (`FF marker u16-BE payload`).
fn push_segment(
    out: &mut Vec<u8>,
    marker: u8,
    payload: &[u8],
    field: &'static str,
) -> Result<(), IptcError> {
    if payload.len() > MAX_SEGMENT_PAYLOAD {
        return Err(IptcError::SegmentTooLarge {
            field,
            bytes: payload.len(),
            limit: MAX_SEGMENT_PAYLOAD,
        });
    }
    out.push(0xFF);
    out.push(marker);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid JPEG: SOI, APP0, DQT, SOF0, DHT, SOS, scan data, EOI.
    pub fn minimal_jpeg() -> Vec<u8> {
        let mut jpeg = vec![0xFF, SOI];
        let seg = |marker: u8, data: &[u8], out: &mut Vec<u8>| {
            out.push(0xFF);
            out.push(marker);
            out.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(data);
        };
        seg(0xE0, b"JFIF\0\x01\x02", &mut jpeg); // APP0
        seg(0xDB, &[0x00; 65], &mut jpeg); // DQT
        seg(
            0xC0,
            &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
            &mut jpeg,
        ); // SOF0
        seg(0xC4, &[0x00; 20], &mut jpeg); // DHT
        seg(SOS, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00], &mut jpeg); // SOS header
        jpeg.extend_from_slice(&[0x11, 0x22, 0xFF, 0x00, 0x33]); // scan data (stuffed)
        jpeg.extend_from_slice(&[0xFF, EOI]);
        jpeg
    }

    fn sample_meta() -> IptcMetadata {
        IptcMetadata {
            title: Some("Titel".into()),
            description: Some("Beschreibung".into()),
            date_created: Some("2026-09-04".into()),
            keywords: vec!["eins".into(), "zwei".into()],
            ..IptcMetadata::default()
        }
    }

    #[test]
    fn embed_extract_roundtrip() {
        let jpeg = minimal_jpeg();
        let meta = sample_meta();
        let embedded = embed_metadata(&jpeg, &meta).expect("embed");
        let back = extract_metadata(&embedded).expect("extract");
        assert_eq!(back, meta.normalized());
    }

    #[test]
    fn splice_leaves_pixel_bytes_identical() {
        let jpeg = minimal_jpeg();
        let embedded = embed_metadata(&jpeg, &sample_meta()).expect("embed");
        let (_, sos_orig) = split_segments(&jpeg).expect("split orig");
        let (_, sos_new) = split_segments(&embedded).expect("split new");
        assert_eq!(&jpeg[sos_orig..], &embedded[sos_new..]);
    }

    #[test]
    fn insertion_order_is_deterministic() {
        let jpeg = minimal_jpeg();
        let a = embed_metadata(&jpeg, &sample_meta()).expect("embed a");
        let b = embed_metadata(&jpeg, &sample_meta()).expect("embed b");
        assert_eq!(a, b);
        // APP13 (0xED) before APP1 (0xE1), both right after SOI.
        assert_eq!(&a[0..2], &[0xFF, SOI]);
        assert_eq!(&a[2..4], &[0xFF, APP13]);
        let (segments, _) = split_segments(&a).expect("split");
        let markers: Vec<u8> = segments.iter().map(|s| s.marker).collect();
        assert_eq!(&markers[0..2], &[APP13, APP1]);
    }

    #[test]
    fn re_embed_replaces_without_duplicates() {
        let jpeg = minimal_jpeg();
        let once = embed_metadata(&jpeg, &sample_meta()).expect("embed 1");
        let other = IptcMetadata {
            title: Some("Neu".into()),
            ..IptcMetadata::default()
        };
        let twice = embed_metadata(&once, &other).expect("embed 2");
        assert_eq!(
            extract_metadata(&twice).expect("extract"),
            other.normalized()
        );
        let (segments, _) = split_segments(&twice).expect("split");
        assert_eq!(segments.iter().filter(|s| s.marker == APP13).count(), 1);
        assert_eq!(segments.iter().filter(|s| s.marker == APP1).count(), 1);
    }

    #[test]
    fn empty_meta_strips_and_writes_nothing() {
        let jpeg = minimal_jpeg();
        let embedded = embed_metadata(&jpeg, &sample_meta()).expect("embed");
        let stripped = embed_metadata(&embedded, &IptcMetadata::default()).expect("strip");
        assert_eq!(
            extract_metadata(&stripped).expect("extract"),
            IptcMetadata::default()
        );
        let (segments, _) = split_segments(&stripped).expect("split");
        assert!(segments.iter().all(|s| s.marker != APP13));
    }

    #[test]
    fn foreign_segments_survive_splice() {
        let mut jpeg = vec![0xFF, SOI];
        jpeg.extend_from_slice(&[0xFF, APP1, 0x00, 0x08, b'E', b'x', b'i', b'f', 0x00, 0x00]);
        jpeg.extend_from_slice(&minimal_jpeg()[2..]);
        let embedded = embed_metadata(&jpeg, &sample_meta()).expect("embed");
        let (segments, _) = split_segments(&embedded).expect("split");
        let exif = segments
            .iter()
            .filter(|s| s.marker == APP1 && !s.data.starts_with(XMP_HEADER))
            .count();
        assert_eq!(exif, 1);
    }

    #[test]
    fn malformed_inputs_fail_loudly() {
        assert_eq!(
            embed_metadata(&[], &sample_meta()).unwrap_err(),
            IptcError::NotJpeg
        );
        // PNG magic: loud NotJpeg, never a silent skip.
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(
            embed_metadata(&png, &sample_meta()).unwrap_err(),
            IptcError::NotJpeg
        );
        assert!(extract_metadata(&png).is_err());
        // Truncated JPEG.
        let mut jpeg = minimal_jpeg();
        jpeg.truncate(10);
        assert!(matches!(
            extract_metadata(&jpeg),
            Err(IptcError::MalformedJpeg { .. })
        ));
        // Missing EOI.
        let mut no_eoi = minimal_jpeg();
        no_eoi.pop();
        no_eoi.pop();
        assert!(matches!(
            extract_metadata(&no_eoi),
            Err(IptcError::MalformedJpeg { .. })
        ));
    }
}
