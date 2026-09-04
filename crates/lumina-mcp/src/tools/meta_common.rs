//! Shared helpers for the five path-based IPTC metadata tools
//! (LRPAR-G15-IPTC-S7).
//!
//! Normative contracts: `feature/product/iptc-metadata.md` §9 and
//! `feature/platform/mcp-server.md` („Metadaten-Schnittstelle"). The tools run
//! **beside** the single-image session (F-101-F1 bulk-tool pattern): they never
//! read or mutate [`crate::session::McpSession`]. Every mutation goes through
//! the same sidecar path as the CLI (`load → mutate a clone → validate → CAS
//! via [`save_sidecar_if_unchanged`]`); a conflict is a loud
//! [`McpError::SidecarConflict`], never a silent last-write-wins.
//!
//! Reused S1/S4/S2 surfaces (no second implementation): the S1 draft helpers
//! (`apply_metadata_draft`, `validate_metadata_field_value`,
//! `is_metadata_field`), the S4 preset files (`load_meta_preset_file`,
//! `render_meta_preset`, `resolve_meta_preset_path`) and the S2 JPEG read
//! (`extract_metadata`).

use crate::error::McpError;
use lumina_sidecar::{
    document_revision, is_metadata_field, load_sidecar, sidecar_path_for, SidecarDocument,
    SidecarError,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// History origin recorded by the path-based draft mutation tools.
pub const META_ORIGIN_MCP: &str = "mcp";

/// URI scheme prefix of the read-only draft resource. The remainder is the
/// percent-encoded source path (see [`percent_encode_path`]); the address is
/// stable across server restarts because it carries no process-local
/// `image_id` (Entscheid 8, LRPAR-G15-META-15).
pub const RESOURCE_SCHEME_PREFIX: &str = "metadata://draft/";

/// Maps a sidecar failure onto the MCP error model. A compare-and-swap miss
/// becomes [`McpError::SidecarConflict`] (`-32010`, analog `lumina_edit`);
/// everything else is a [`McpError::Sidecar`].
pub fn map_sidecar_error(error: SidecarError) -> McpError {
    match error {
        SidecarError::Conflict(path) => McpError::SidecarConflict(path),
        other => McpError::Sidecar(format!("{other}")),
    }
}

/// Loads the sidecar belonging to `input`. A missing sidecar is a loud error
/// naming the remedy (`lumina_import` first) — a metadata tool never
/// materializes a source identity silently (SOLL §6, CLI `require_sidecar`
/// parity).
pub fn require_sidecar_for(input: &Path) -> Result<(PathBuf, SidecarDocument), McpError> {
    let path = sidecar_path_for(input);
    match load_sidecar(&path) {
        Ok(document) => Ok((path, document)),
        Err(SidecarError::Missing(_)) => Err(McpError::Sidecar(format!(
            "no sidecar for `{}`; run `lumina_import` first",
            input.display()
        ))),
        Err(error) => Err(McpError::Sidecar(format!("{error}"))),
    }
}

/// Current persisted revision of `document` (the compare-and-swap expectation).
pub fn expected_revision(document: &SidecarDocument) -> Result<String, McpError> {
    document_revision(document).map_err(|error| McpError::Sidecar(format!("{error}")))
}

/// Embedded IPTC read of one source file (S2 `extract_metadata`).
pub struct EmbeddedIptc {
    /// `true` when the source is a JPEG (IIM/XMP-readable). Non-JPEG sources
    /// report `available: false` loudly in the payload instead of failing —
    /// CLI `meta inspect` parity ("nicht verfügbar").
    pub available: bool,
    /// Embedded values by registry field id (only set fields).
    pub fields: BTreeMap<String, String>,
    /// Embedded keywords (IIM ∪ XMP order).
    pub keywords: Vec<String>,
}

/// Reads the embedded IPTC of `path`: JPEG via S2 IIM/XMP, anything else
/// yields `available: false`. Present-but-broken JPEG segments are a loud
/// [`McpError::Decode`], never a silent skip.
pub fn read_embedded(path: &Path) -> Result<EmbeddedIptc, McpError> {
    let bytes =
        fs::read(path).map_err(|error| McpError::FileNotFound(format!("{path:?}: {error}")))?;
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Ok(EmbeddedIptc {
            available: false,
            fields: BTreeMap::new(),
            keywords: Vec::new(),
        });
    }
    let meta = lumina_iptc::extract_metadata(&bytes)
        .map_err(|error| McpError::Decode(format!("embedded IPTC unreadable: {error}")))?;
    let mut fields = BTreeMap::new();
    for (id, value) in [
        ("title", meta.title.as_deref()),
        ("headline", meta.headline.as_deref()),
        ("description", meta.description.as_deref()),
        ("copyright_notice", meta.copyright_notice.as_deref()),
        ("creator", meta.creator.as_deref()),
        ("credit", meta.credit.as_deref()),
        ("source", meta.source.as_deref()),
        ("city", meta.city.as_deref()),
        ("state_province", meta.state_province.as_deref()),
        ("country", meta.country.as_deref()),
        ("date_created", meta.date_created.as_deref()),
    ] {
        if let Some(value) = value {
            fields.insert(id.to_string(), value.to_string());
        }
    }
    Ok(EmbeddedIptc {
        available: true,
        fields,
        keywords: meta.keywords,
    })
}

/// Builds the canonical draft payload shared by `lumina_get_metadata_draft`
/// and `resources/read` (both return byte-identical JSON by construction —
/// `resources/read` serializes exactly this value).
pub fn metadata_draft_payload(
    path_str: &str,
    document: &SidecarDocument,
    embedded: &EmbeddedIptc,
) -> Value {
    json!({
        "path": path_str,
        "embedded": {
            "available": embedded.available,
            "fields": embedded.fields,
            "keywords": embedded.keywords,
        },
        "draft": document.metadata.draft,
        "keywords": document.keywords,
        "history_len": document.metadata.history.len(),
        "status": "ok",
    })
}

/// Loads the sidecar and embedded IPTC for `path_str` and returns the
/// canonical draft payload. The source file must exist ([`McpError::FileNotFound`]);
/// its sidecar must exist ([`require_sidecar_for`]).
pub fn draft_payload_for_path(path_str: &str) -> Result<Value, McpError> {
    let path = Path::new(path_str);
    if !path.exists() {
        return Err(McpError::FileNotFound(path_str.to_string()));
    }
    let (_sidecar_path, document) = require_sidecar_for(path)?;
    let embedded = read_embedded(path)?;
    Ok(metadata_draft_payload(path_str, &document, &embedded))
}

/// Merges the source-level draft (+ the routed `keywords`, SOLL §4) into
/// `lumina-iptc` values (CLI `draft_to_iptc` parity, S6). Empty fields stay
/// absent: the tag is omitted on write, never written empty.
pub fn draft_to_iptc(document: &SidecarDocument) -> lumina_iptc::IptcMetadata {
    let get = |id: &str| document.metadata.get(id).map(str::to_string);
    lumina_iptc::IptcMetadata {
        title: get("title"),
        headline: get("headline"),
        description: get("description"),
        copyright_notice: get("copyright_notice"),
        creator: get("creator"),
        credit: get("credit"),
        source: get("source"),
        city: get("city"),
        state_province: get("state_province"),
        country: get("country"),
        date_created: get("date_created"),
        keywords: document.keywords.clone(),
    }
}

/// Percent-encodes a source path for the `metadata://draft/` resource URI:
/// UTF-8 bytes, unreserved characters and `/` stay literal, everything else
/// (spaces, `%`, non-ASCII, …) becomes `%XX`. `/` stays literal so the URI
/// keeps its path shape; a literal `%` in a file name is encoded as `%25`,
/// so decoding is unambiguous.
pub fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/')
        {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Inverse of [`percent_encode_path`]. A malformed `%` escape or non-UTF-8
/// bytes are a loud [`McpError::InvalidParams`] — never a guessed path.
pub fn percent_decode_path(encoded: &str) -> Result<String, McpError> {
    let raw = encoded.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' {
            let hex = encoded.get(index + 1..index + 3).ok_or_else(|| {
                McpError::InvalidParams(format!(
                    "malformed resource path `{encoded}`: truncated `%` escape"
                ))
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                McpError::InvalidParams(format!(
                    "malformed resource path `{encoded}`: invalid `%` escape `%{hex}`"
                ))
            })?;
            bytes.push(byte);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).map_err(|_| {
        McpError::InvalidParams(format!(
            "malformed resource path `{encoded}`: not valid UTF-8"
        ))
    })
}

/// Builds the stable resource URI for a source path.
pub fn resource_uri_for(path_str: &str) -> String {
    format!("{RESOURCE_SCHEME_PREFIX}{}", percent_encode_path(path_str))
}

/// Extracts the source path from a `metadata://draft/` URI. A foreign scheme
/// or an empty path is a loud [`McpError::InvalidParams`].
pub fn path_from_resource_uri(uri: &str) -> Result<String, McpError> {
    let encoded = uri.strip_prefix(RESOURCE_SCHEME_PREFIX).ok_or_else(|| {
        McpError::InvalidParams(format!(
            "unknown resource `{uri}` (expected `{RESOURCE_SCHEME_PREFIX}<urlencoded-path>`)"
        ))
    })?;
    if encoded.is_empty() {
        return Err(McpError::InvalidParams(format!(
            "resource `{uri}` carries no path"
        )));
    }
    percent_decode_path(encoded)
}

/// True for draft registry field ids. `keywords` is **not** a draft field —
/// callers map it to the dedicated error so the routing rule stays visible
/// instead of failing as a generic "unknown field".
pub fn is_draft_field(id: &str) -> bool {
    is_metadata_field(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_uri_roundtrips_paths_with_spaces_and_percent() {
        for path in [
            "/tmp/photo.png",
            "/tmp/mein foto 1.png",
            "/tmp/100%.png",
            "/tmp/äöü/Grüße.png",
        ] {
            let uri = resource_uri_for(path);
            assert!(
                uri.starts_with(RESOURCE_SCHEME_PREFIX),
                "scheme prefix: {uri}"
            );
            assert_eq!(path_from_resource_uri(&uri).unwrap(), path);
        }
        // `/` stays literal, `%` is escaped (no ambiguity on decode).
        assert_eq!(
            resource_uri_for("/a b/c%d.png"),
            "metadata://draft//a%20b/c%25d.png"
        );
    }

    #[test]
    fn malformed_resource_uris_fail_loudly() {
        assert!(path_from_resource_uri("metadata://other/x").is_err());
        assert!(path_from_resource_uri("metadata://draft/").is_err());
        assert!(path_from_resource_uri("metadata://draft/a%2").is_err());
        assert!(path_from_resource_uri("metadata://draft/a%zz").is_err());
        assert!(matches!(
            path_from_resource_uri("metadata://draft/a%2").unwrap_err(),
            McpError::InvalidParams(_)
        ));
    }
}
