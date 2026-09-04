//! XMP APP1 writer (via `xmp-writer`) and packet parser.
//!
//! Write path uses the `xmp-writer` compile-time dependency (pure Rust, no
//! transitive dependencies; MIT OR Apache-2.0). Read path is a small,
//! dependency-free scanner for the properties this crate writes, using the
//! conventional prefixes (`dc:`, `photoshop:`, `xmpRights:`) that current
//! tools (Lightroom, ExifTool) share.

use xmp_writer::{CustomNamespace, LangId, Namespace, XmpWriter};

use crate::error::IptcError;
use crate::registry::IptcMetadata;

/// APP1 identifier for XMP (`http://ns.adobe.com/xap/1.0/` + NUL).
pub const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// Photoshop namespace for the writer (`photoshop:`, Adobe NS).
fn photoshop_ns() -> Namespace<'static> {
    Namespace::Custom(Box::new(CustomNamespace::new(
        "Photoshop",
        "photoshop",
        "http://ns.adobe.com/photoshop/1.0/",
    )))
}

/// Build the XMP packet for metadata (validates first).
///
/// Returns `Ok(None)` when the metadata is empty: no properties means no
/// packet, and the APP1 segment is omitted on splice.
pub fn build_xmp_packet(meta: &IptcMetadata) -> Result<Option<String>, IptcError> {
    meta.validate()?;
    let norm = meta.normalized();
    if norm.is_empty() {
        return Ok(None);
    }
    let mut writer = XmpWriter::new();
    if let Some(v) = norm.title.as_deref() {
        writer.title([(None, v)]);
    }
    if let Some(v) = norm.description.as_deref() {
        writer.description([(None, v)]);
    }
    if let Some(v) = norm.copyright_notice.as_deref() {
        writer.rights([(None, v)]);
        writer.marked(true);
    }
    if let Some(v) = norm.creator.as_deref() {
        writer.creator([v]);
    }
    if !norm.keywords.is_empty() {
        writer.subject(norm.keywords.iter().map(String::as_str));
    }
    simple(&mut writer, "Headline", norm.headline.as_deref());
    simple(&mut writer, "Credit", norm.credit.as_deref());
    simple(&mut writer, "Source", norm.source.as_deref());
    simple(&mut writer, "City", norm.city.as_deref());
    simple(&mut writer, "State", norm.state_province.as_deref());
    simple(&mut writer, "Country", norm.country.as_deref());
    simple(&mut writer, "DateCreated", norm.date_created.as_deref());
    Ok(Some(writer.finish(None)))
}

/// Write a plain `photoshop:` text property when present.
fn simple(writer: &mut XmpWriter<'_>, name: &'static str, value: Option<&str>) {
    if let Some(v) = value {
        writer.element(name, photoshop_ns()).value(v);
    }
}

/// Wrap an XMP packet in an APP1 segment payload (identifier + packet).
pub fn build_app1(packet: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(XMP_HEADER.len() + packet.len());
    out.extend_from_slice(XMP_HEADER);
    out.extend_from_slice(packet.as_bytes());
    out
}

/// Split an APP1 segment payload into the XMP packet.
///
/// Returns `Ok(None)` for non-XMP APP1 segments (e.g. Exif): absence is not
/// an error. A truncated XMP header is a loud error.
pub fn parse_app1(payload: &[u8]) -> Result<Option<&str>, IptcError> {
    if !payload.starts_with(XMP_HEADER) {
        return Ok(None);
    }
    std::str::from_utf8(&payload[XMP_HEADER.len()..])
        .map(Some)
        .map_err(|_| IptcError::InvalidXmp {
            reason: "XMP-Paket ist kein gültiges UTF-8".into(),
        })
}

/// Parse an XMP packet into metadata.
///
/// Only the registry properties are extracted; anything else is ignored.
/// `xmpRights:Marked` is write-only context (set whenever a copyright notice
/// is present) and is not mapped back to a field.
pub fn parse_xmp_packet(packet: &str) -> Result<IptcMetadata, IptcError> {
    if !packet.contains("<x:xmpmeta") && !packet.contains("<xap:xmpmeta") {
        return Err(IptcError::InvalidXmp {
            reason: "kein XMP-Root-Element gefunden".into(),
        });
    }
    let keywords = all_array_items(packet, "dc", "subject");
    Ok(IptcMetadata {
        title: lang_alt(packet, "dc", "title"),
        description: lang_alt(packet, "dc", "description"),
        copyright_notice: lang_alt(packet, "dc", "rights"),
        creator: first_array_item(packet, "dc", "creator"),
        headline: simple_text(packet, "photoshop", "Headline"),
        credit: simple_text(packet, "photoshop", "Credit"),
        source: simple_text(packet, "photoshop", "Source"),
        city: simple_text(packet, "photoshop", "City"),
        state_province: simple_text(packet, "photoshop", "State"),
        country: simple_text(packet, "photoshop", "Country"),
        date_created: simple_text(packet, "photoshop", "DateCreated"),
        keywords,
    })
}

/// Extract a `rdf:Alt` language alternative (first `rdf:li` wins).
fn lang_alt(packet: &str, prefix: &str, local: &str) -> Option<String> {
    let body = element_body(packet, prefix, local)?;
    li_items(body).into_iter().next()
}

/// Extract the first item of an `rdf:Seq`/`rdf:Bag` array property.
fn first_array_item(packet: &str, prefix: &str, local: &str) -> Option<String> {
    let body = element_body(packet, prefix, local)?;
    li_items(body).into_iter().next()
}

/// Extract all items of an `rdf:Seq`/`rdf:Bag` array property, in order.
fn all_array_items(packet: &str, prefix: &str, local: &str) -> Vec<String> {
    element_body(packet, prefix, local).map_or_else(Vec::new, li_items)
}

/// Extract a plain-text property (`<prefix:local>value</prefix:local>`).
///
/// Returns `None` for structured values (nested `rdf:` content): those belong
/// to the array/lang-alt extractors, never to silent truncation.
fn simple_text(packet: &str, prefix: &str, local: &str) -> Option<String> {
    let body = element_body(packet, prefix, local)?;
    if body.contains("<rdf:") {
        return None;
    }
    let value = xml_unescape(body.trim());
    (!value.is_empty()).then_some(value)
}

/// Inner XML of `<prefix:local ...>...</prefix:local>`, if present.
fn element_body<'a>(packet: &'a str, prefix: &str, local: &str) -> Option<&'a str> {
    let open = format!("<{prefix}:{local}");
    let mut search = 0;
    loop {
        let start = packet[search..].find(&open)? + search;
        let after = start + open.len();
        let tag_end = packet[after..].find('>')? + after;
        let tag = &packet[after..tag_end];
        // Skip self-closing or names that merely share a prefix
        // (`<dc:titles>` must not match `dc:title`).
        if tag.starts_with('/') || (!tag.is_empty() && !tag.starts_with(char::is_whitespace)) {
            search = tag_end + 1;
            continue;
        }
        if packet[tag_end.saturating_sub(1)..tag_end].starts_with('/') {
            return Some(""); // self-closing: empty body
        }
        let close = format!("</{prefix}:{local}>");
        let end = packet[tag_end..].find(&close)? + tag_end;
        return Some(&packet[tag_end + 1..end]);
    }
}

/// All `<rdf:li ...>value</rdf:li>` items of an array body, in order.
fn li_items(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut search = 0;
    while let Some(start) = body[search..].find("<rdf:li") {
        let mut pos = search + start + "<rdf:li".len();
        let Some(tag_end) = body[pos..].find('>') else {
            break;
        };
        pos += tag_end;
        if body[pos.saturating_sub(1)..pos].starts_with('/') {
            search = pos + 1; // self-closing li: no value
            continue;
        }
        let Some(end) = body[pos..].find("</rdf:li>") else {
            break;
        };
        let value = xml_unescape(body[pos + 1..pos + end].trim());
        if !value.is_empty() {
            items.push(value);
        }
        search = pos + end + "</rdf:li>".len();
    }
    items
}

/// Unescape XML entities, including numeric character references.
fn xml_unescape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = rest.find(';').map_or(rest.len(), |i| i + 1);
        let entity = &rest[..end.min(rest.len())];
        match entity {
            "&lt;" => out.push('<'),
            "&gt;" => out.push('>'),
            "&amp;" => out.push('&'),
            "&apos;" => out.push('\''),
            "&quot;" => out.push('"'),
            _ if entity.starts_with("&#") && entity.ends_with(';') => {
                let num = &entity[2..entity.len() - 1];
                let code =
                    if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                match code.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push_str(entity),
                }
            }
            _ => out.push_str(entity),
        }
        rest = &rest[end.min(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// Silence the unused-import lint if the writer API changes; documents that
/// language ids default to `x-default` via `None`.
#[allow(dead_code)]
fn _lang_id_note() {
    let _ = LangId("x-default");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::IptcMetadata;

    fn full_meta() -> IptcMetadata {
        IptcMetadata {
            title: Some("Startschuss & <Auftakt>".into()),
            headline: Some("Schlagzeile \"heute\"".into()),
            description: Some("Zeile 1\nZeile 2 mit äöü, 中文, 🎉".into()),
            copyright_notice: Some("© 2026 Beispiel 'Foto'".into()),
            creator: Some("Max & Moritz".into()),
            credit: Some("Agentur <A>".into()),
            source: Some("Archiv".into()),
            city: Some("Berlin".into()),
            state_province: Some("Berlin".into()),
            country: Some("Deutschland".into()),
            date_created: Some("2026-09-04".into()),
            keywords: vec![
                "Sport".into(),
                "Finale & \"Nachspiel\"".into(),
                "U&D <Tag>".into(),
            ],
        }
    }

    #[test]
    fn xmp_roundtrip_preserves_all_fields_with_escapes() {
        let meta = full_meta();
        let packet = build_xmp_packet(&meta).expect("build").expect("non-empty");
        let parsed = parse_xmp_packet(&packet).expect("parse");
        assert_eq!(parsed, meta.normalized());
    }

    #[test]
    fn xmp_packet_marks_copyright() {
        let meta = IptcMetadata {
            copyright_notice: Some("© 2026".into()),
            ..IptcMetadata::default()
        };
        let packet = build_xmp_packet(&meta).expect("build").expect("non-empty");
        assert!(packet.contains("xmpRights:Marked"));
        assert!(packet.contains(">True<"));
    }

    #[test]
    fn xmp_empty_meta_builds_no_packet() {
        assert_eq!(
            build_xmp_packet(&IptcMetadata::default()).expect("build"),
            None
        );
    }

    #[test]
    fn xmp_rejects_non_packet_loudly() {
        assert!(matches!(
            parse_xmp_packet("<html>kein xmp</html>"),
            Err(IptcError::InvalidXmp { .. })
        ));
    }

    #[test]
    fn xmp_app1_envelope_roundtrip() {
        let packet = "<x:xmpmeta>test</x:xmpmeta>".to_string();
        let payload = build_app1(&packet);
        assert!(payload.starts_with(XMP_HEADER));
        assert_eq!(parse_app1(&payload).expect("parse"), Some(packet.as_str()));
        assert_eq!(parse_app1(b"Exif\0\0rest").expect("parse"), None);
    }

    #[test]
    fn xmp_numeric_entities_unescape() {
        assert_eq!(xml_unescape("A&#38;B&#x3c;C"), "A&B<C");
    }
}
