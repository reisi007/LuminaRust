//! `lumina-iptc`: pure-Rust IPTC IIM + XMP embedding for JPEG exports.
//!
//! Implements the writer side of `feature/product/iptc-metadata.md` §4
//! (field registry with IIM/XMP mappings) and §7 (JPEG segment splice).
//! Scope: JPEG only, no EXIF write, no source-metadata adoption. Only
//! compiled-in code and the `xmp-writer` compile-time dependency — no
//! runtime tools, no subprocesses, no downloads.
//!
//! # Example
//!
//! ```rust
//! use lumina_iptc::{IptcMetadata, embed_metadata, extract_metadata};
//!
//! let meta = IptcMetadata {
//!     title: Some("Startschuss".into()),
//!     date_created: Some("2026-09-04".into()),
//!     keywords: vec!["Sport".into()],
//!     ..IptcMetadata::default()
//! };
//! # let jpeg = lumina_iptc::test_fixture_jpeg();
//! let embedded = embed_metadata(&jpeg, &meta).expect("embed");
//! let back = extract_metadata(&embedded).expect("extract");
//! assert_eq!(back, meta.normalized());
//! ```

mod error;
mod iim;
mod jpeg;
mod registry;
mod xmp;

pub use error::IptcError;
pub use iim::{build_app13, decode_iim, encode_iim, parse_app13, APP13_HEADER};
pub use jpeg::{embed_metadata, extract_metadata};
pub use registry::{
    field_by_id, field_by_iim, FieldDef, IptcMetadata, MAX_KEYWORDS, MAX_KEYWORD_OCTETS, REGISTRY,
};
pub use xmp::{build_app1, build_xmp_packet, parse_app1, parse_xmp_packet, XMP_HEADER};

/// Minimal valid JPEG for doctests and downstream tests (SOI … SOS … EOI).
///
/// Not part of the stable API contract; tests should prefer their own
/// fixtures where the marker layout matters.
#[doc(hidden)]
pub fn test_fixture_jpeg() -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8];
    let seg = |marker: u8, data: &[u8], out: &mut Vec<u8>| {
        out.push(0xFF);
        out.push(marker);
        out.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(data);
    };
    seg(0xE0, b"JFIF\0\x01\x02", &mut jpeg);
    seg(0xDB, &[0x00; 65], &mut jpeg);
    seg(
        0xC0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
        &mut jpeg,
    );
    seg(0xC4, &[0x00; 20], &mut jpeg);
    seg(0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00], &mut jpeg);
    jpeg.extend_from_slice(&[0x11, 0x22, 0xFF, 0x00, 0x33]);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}
