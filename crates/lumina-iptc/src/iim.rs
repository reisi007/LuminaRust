//! IPTC-IIM writer/parser and the JPEG APP13/8BIM envelope.
//!
//! Layout per dataset: `0x1C record dataset u16-BE-length bytes`.
//! `1:90` (CodedCharacterSet, UTF-8) is always written first; record-2
//! datasets follow in ascending dataset order (deterministic). Empty fields
//! are omitted, never written empty. `date_created` is stored as `YYYYMMDD`.

use crate::error::IptcError;
use crate::registry::{IptcMetadata, MAX_KEYWORDS, REGISTRY};

/// IIM dataset marker.
const TAG_MARKER: u8 = 0x1C;

/// CodedCharacterSet value for UTF-8 (`ESC % G`).
const CHARSET_UTF8: &[u8] = &[0x1B, 0x25, 0x47];

/// IIM record/dataset for the repeatable keywords.
const KEYWORDS_DATASET: (u8, u8) = (2, 25);

/// Photoshop 3.0 APP13 identifier (NUL-terminated).
pub const APP13_HEADER: &[u8] = b"Photoshop 3.0\0";

/// 8BIM resource id for IPTC-NAA data.
const IPTC_RESOURCE_ID: u16 = 0x0404;

/// Encode metadata to raw IIM bytes (validates first).
pub fn encode_iim(meta: &IptcMetadata) -> Result<Vec<u8>, IptcError> {
    meta.validate()?;
    let mut out = Vec::new();
    push_dataset(&mut out, 1, 90, CHARSET_UTF8);
    for def in REGISTRY {
        if def.id == "date_created" {
            if let Some(raw) = meta.date_created.as_deref() {
                let value = raw.trim();
                if !value.is_empty() {
                    push_dataset(&mut out, 2, 55, &iim_date(value));
                }
            }
            continue;
        }
        let slot: Option<&str> = match def.id {
            "title" => meta.title.as_deref(),
            "headline" => meta.headline.as_deref(),
            "description" => meta.description.as_deref(),
            "copyright_notice" => meta.copyright_notice.as_deref(),
            "creator" => meta.creator.as_deref(),
            "credit" => meta.credit.as_deref(),
            "source" => meta.source.as_deref(),
            "city" => meta.city.as_deref(),
            "state_province" => meta.state_province.as_deref(),
            "country" => meta.country.as_deref(),
            _ => None,
        };
        if let Some(raw) = slot {
            let value = raw.trim();
            if !value.is_empty() {
                push_dataset(&mut out, def.iim.0, def.iim.1, value.as_bytes());
            }
        }
    }
    for keyword in &meta.keywords {
        let value = keyword.trim();
        if !value.is_empty() {
            push_dataset(
                &mut out,
                KEYWORDS_DATASET.0,
                KEYWORDS_DATASET.1,
                value.as_bytes(),
            );
        }
    }
    Ok(out)
}

/// Decode raw IIM bytes back to metadata.
///
/// Unknown records/datasets are skipped (tolerant reader: other writers may
/// store more than the registry). Duplicate single-value datasets: last wins.
/// An empty payload for a single-value dataset means "absent".
pub fn decode_iim(data: &[u8]) -> Result<IptcMetadata, IptcError> {
    let mut meta = IptcMetadata::default();
    let mut pos = 0;
    while pos < data.len() {
        if data[pos] != TAG_MARKER {
            return Err(IptcError::InvalidIim {
                reason: format!("erwarteter Tag-Marker 0x1C bei Offset {pos}"),
            });
        }
        if pos + 5 > data.len() {
            return Err(IptcError::InvalidIim {
                reason: format!("abgeschnittener Dataset-Header bei Offset {pos}"),
            });
        }
        let record = data[pos + 1];
        let dataset = data[pos + 2];
        let len = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as usize;
        pos += 5;
        let bytes = data
            .get(pos..pos + len)
            .ok_or_else(|| IptcError::InvalidIim {
                reason: format!(
                    "Dataset {record}:{dataset} länger als Rest ({len} Bytes ab Offset {pos})"
                ),
            })?;
        pos += len;
        if (record, dataset) == (1, 90) {
            continue; // CodedCharacterSet: informational on read
        }
        if record != 2 {
            continue;
        }
        if (record, dataset) == KEYWORDS_DATASET {
            let value = std::str::from_utf8(bytes).map_err(|_| IptcError::InvalidIim {
                reason: format!(
                    "Schlüsselwort ist kein gültiges UTF-8 (Offset {})",
                    pos - len
                ),
            })?;
            if !value.trim().is_empty() {
                if meta.keywords.len() >= MAX_KEYWORDS {
                    return Err(IptcError::TooManyKeywords {
                        limit: MAX_KEYWORDS,
                        actual: meta.keywords.len() + 1,
                    });
                }
                meta.keywords.push(value.to_string());
            }
            continue;
        }
        let Some(def) = REGISTRY.iter().find(|f| f.iim == (record, dataset)) else {
            continue; // unknown dataset: skip, never fail
        };
        let value = std::str::from_utf8(bytes).map_err(|_| IptcError::InvalidIim {
            reason: format!("Feld '{}' ist kein gültiges UTF-8", def.id),
        })?;
        if value.trim().is_empty() {
            continue;
        }
        if def.id == "date_created" {
            meta.date_created = Some(parse_iim_date(value)?);
        } else {
            set_slot(&mut meta, def.id, value.to_string());
        }
    }
    Ok(meta)
}

/// Wrap raw IIM bytes in a Photoshop 3.0 APP13 segment payload
/// (identifier + 8BIM resource 0x0404, even-padded).
pub fn build_app13(iim: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(APP13_HEADER.len() + 12 + iim.len() + 1);
    out.extend_from_slice(APP13_HEADER);
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(&IPTC_RESOURCE_ID.to_be_bytes());
    // Empty Pascal name, padded to even size.
    out.extend_from_slice(&[0x00, 0x00]);
    out.extend_from_slice(&(iim.len() as u32).to_be_bytes());
    out.extend_from_slice(iim);
    if iim.len() % 2 == 1 {
        out.push(0x00);
    }
    out
}

/// Extract raw IIM bytes from an APP13 segment payload.
///
/// Returns `Ok(None)` when the payload carries no IPTC resource (e.g. a
/// foreign Photoshop segment): absence is not an error. Malformed envelopes
/// are loud errors.
pub fn parse_app13(payload: &[u8]) -> Result<Option<Vec<u8>>, IptcError> {
    if !payload.starts_with(APP13_HEADER) {
        return Ok(None);
    }
    let mut pos = APP13_HEADER.len();
    while pos < payload.len() {
        let rest = &payload[pos..];
        if rest.len() < 12 {
            return Err(IptcError::InvalidIim {
                reason: format!("abgeschnittene 8BIM-Ressource bei Offset {pos}"),
            });
        }
        if &rest[0..4] != b"8BIM" {
            return Err(IptcError::InvalidIim {
                reason: format!("erwartetes 8BIM bei Offset {pos}"),
            });
        }
        let id = u16::from_be_bytes([rest[4], rest[5]]);
        let name_len = rest[6] as usize;
        let name_field = 1 + name_len + ((1 + name_len) % 2);
        let size_off = pos + 6 + name_field;
        let size_bytes =
            payload
                .get(size_off..size_off + 4)
                .ok_or_else(|| IptcError::InvalidIim {
                    reason: format!("abgeschnittene Ressourcengröße bei Offset {pos}"),
                })?;
        let size = u32::from_be_bytes(size_bytes.try_into().expect("4 bytes")) as usize;
        let data_off = size_off + 4;
        let data = payload
            .get(data_off..data_off + size)
            .ok_or_else(|| IptcError::InvalidIim {
                reason: format!(
                    "Ressource {id:#06X} länger als Segment ({size} Bytes ab Offset {data_off})"
                ),
            })?;
        if id == IPTC_RESOURCE_ID {
            return Ok(Some(data.to_vec()));
        }
        pos = data_off + size + (size % 2);
    }
    Ok(None)
}

/// Convert Sidecar `YYYY-MM-DD` to IIM `YYYYMMDD` (validated beforehand).
fn iim_date(value: &str) -> Vec<u8> {
    value.bytes().filter(|b| *b != b'-').collect()
}

/// Convert IIM `YYYYMMDD` to Sidecar `YYYY-MM-DD`.
fn parse_iim_date(value: &str) -> Result<String, IptcError> {
    let invalid = || IptcError::InvalidIim {
        reason: format!("2:55 hat ungültiges Datum '{value}' (erwartet YYYYMMDD)"),
    };
    if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let dashed = format!("{}-{}-{}", &value[0..4], &value[4..6], &value[6..8]);
    // Reuse the registry calendar check via a probe metadata value.
    let probe = IptcMetadata {
        date_created: Some(dashed.clone()),
        ..IptcMetadata::default()
    };
    probe.validate().map_err(|_| invalid())?;
    Ok(dashed)
}

fn push_dataset(out: &mut Vec<u8>, record: u8, dataset: u8, bytes: &[u8]) {
    debug_assert!(
        bytes.len() <= u16::MAX as usize,
        "registry limits keep datasets small"
    );
    out.push(TAG_MARKER);
    out.push(record);
    out.push(dataset);
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn set_slot(meta: &mut IptcMetadata, id: &str, value: String) {
    match id {
        "title" => meta.title = Some(value),
        "headline" => meta.headline = Some(value),
        "description" => meta.description = Some(value),
        "copyright_notice" => meta.copyright_notice = Some(value),
        "creator" => meta.creator = Some(value),
        "credit" => meta.credit = Some(value),
        "source" => meta.source = Some(value),
        "city" => meta.city = Some(value),
        "state_province" => meta.state_province = Some(value),
        "country" => meta.country = Some(value),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_meta() -> IptcMetadata {
        IptcMetadata {
            title: Some("Startschuss".into()),
            headline: Some("Schlagzeile & <Escapes>".into()),
            description: Some("Zeile 1\nZeile 2 mit Umläuten äöü, 中文, 🎉".into()),
            copyright_notice: Some("© 2026 Beispiel".into()),
            creator: Some("Max Mustermann".into()),
            credit: Some("Agentur".into()),
            source: Some("Archiv".into()),
            city: Some("Berlin".into()),
            state_province: Some("Berlin".into()),
            country: Some("Deutschland".into()),
            date_created: Some("2026-09-04".into()),
            keywords: vec!["Sport".into(), "Finale & \"Nachspiel\"".into()],
        }
    }

    #[test]
    fn iim_roundtrip_preserves_all_fields() {
        let meta = full_meta();
        let decoded = decode_iim(&encode_iim(&meta).expect("encode")).expect("decode");
        assert_eq!(decoded, meta.normalized());
    }

    #[test]
    fn iim_always_sets_charset_utf8_first() {
        let bytes = encode_iim(&full_meta()).expect("encode");
        assert!(bytes.starts_with(&[0x1C, 1, 90, 0, 3, 0x1B, 0x25, 0x47]));
    }

    #[test]
    fn iim_skips_empty_fields() {
        let meta = IptcMetadata {
            title: Some("   ".into()),
            ..IptcMetadata::default()
        };
        let bytes = encode_iim(&meta).expect("encode");
        assert_eq!(bytes.len(), 8); // only 1:90 (1+1+1+2+3)
        assert_eq!(decode_iim(&bytes).expect("decode"), IptcMetadata::default());
    }

    #[test]
    fn iim_date_format_conversion() {
        let meta = IptcMetadata {
            date_created: Some("2026-09-04".into()),
            ..IptcMetadata::default()
        };
        let bytes = encode_iim(&meta).expect("encode");
        assert!(bytes.windows(8).any(|w| w == b"20260904"));
        let decoded = decode_iim(&bytes).expect("decode");
        assert_eq!(decoded.date_created.as_deref(), Some("2026-09-04"));
    }

    #[test]
    fn iim_rejects_bad_marker_loudly() {
        assert!(matches!(
            decode_iim(&[0x00, 0x01]),
            Err(IptcError::InvalidIim { .. })
        ));
        assert!(matches!(
            decode_iim(&[0x1C, 2]),
            Err(IptcError::InvalidIim { .. })
        ));
    }

    #[test]
    fn app13_envelope_roundtrip() {
        let iim = encode_iim(&full_meta()).expect("encode");
        let payload = build_app13(&iim);
        assert!(payload.starts_with(APP13_HEADER));
        assert_eq!(parse_app13(&payload).expect("parse"), Some(iim));
    }

    #[test]
    fn app13_without_iptc_resource_is_absence_not_error() {
        let mut payload = APP13_HEADER.to_vec();
        payload.extend_from_slice(b"8BIM");
        payload.extend_from_slice(&0x0405u16.to_be_bytes()); // thumbnail, not IPTC
        payload.extend_from_slice(&[0x00, 0x00]);
        payload.extend_from_slice(&2u32.to_be_bytes());
        payload.extend_from_slice(&[0xAA, 0xBB]);
        assert_eq!(parse_app13(&payload).expect("parse"), None);
    }

    #[test]
    fn app13_odd_length_padding() {
        let payload = build_app13(&[0x1C, 2, 5]);
        assert_eq!(payload.len() % 2, 0);
        assert_eq!(
            parse_app13(&payload).expect("parse"),
            Some(vec![0x1C, 2, 5])
        );
    }
}
