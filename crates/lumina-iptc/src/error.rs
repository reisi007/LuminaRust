//! Typed errors for IPTC IIM / XMP / JPEG-segment handling.
//!
//! Every failure is loud: oversized fields, invalid dates, control characters
//! and malformed JPEG input are reported with the offending field and limit.
//! Nothing is ever silently truncated or defaulted.

use thiserror::Error;

/// Errors returned by [`crate`] operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IptcError {
    /// A single-valued field exceeds its IIM octet limit.
    ///
    /// Octets are UTF-8 bytes, not characters: multi-byte text can exceed the
    /// limit while staying within the Sidecar character budget. The export
    /// must fail loudly (field + limit named) instead of silently truncating.
    #[error("Feld '{field}' überschreitet IIM-Oktettlimit ({actual_octets} > {limit} Oktette)")]
    FieldTooLong {
        /// Stable field id from the registry (e.g. `"title"`).
        field: &'static str,
        /// Limit in octets (UTF-8 bytes).
        limit: usize,
        /// Actual size in octets (UTF-8 bytes).
        actual_octets: usize,
    },

    /// A single keyword exceeds its IIM octet limit.
    #[error("Schlüsselwort in Feld 'keywords' überschreitet IIM-Oktettlimit ({actual_octets} > {limit} Oktette)")]
    KeywordTooLong {
        /// Limit in octets (UTF-8 bytes).
        limit: usize,
        /// Actual size in octets (UTF-8 bytes).
        actual_octets: usize,
    },

    /// Too many keywords for IIM 2:25.
    #[error("Feld 'keywords' überschreitet Eintragslimit ({actual} > {limit} Einträge)")]
    TooManyKeywords {
        /// Maximum number of entries.
        limit: usize,
        /// Actual number of entries.
        actual: usize,
    },

    /// `date_created` is not a valid `YYYY-MM-DD` calendar date.
    #[error("Feld 'date_created' hat ungültiges Datum '{value}' (erwartet YYYY-MM-DD)")]
    InvalidDate {
        /// Stable field id (`"date_created"`).
        field: &'static str,
        /// The offending value.
        value: String,
    },

    /// A field contains control characters outside the allowed whitespace
    /// (`\n`, `\r`, `\t`).
    #[error("Feld '{field}' enthält unzulässige Steuerzeichen")]
    ControlCharacters {
        /// Stable field id from the registry.
        field: &'static str,
    },

    /// The input is not a JPEG (missing SOI marker).
    ///
    /// This covers non-JPEG input such as PNG or WebP: only JPEG supports the
    /// APP13/APP1 bake-in, anything else is a loud error, never a silent skip.
    #[error("kein JPEG (fehlender SOI-Marker)")]
    NotJpeg,

    /// The input starts like a JPEG but its marker structure is broken
    /// (truncated segment, impossible length, missing EOI, ...).
    #[error("fehlerhaftes JPEG: {reason}")]
    MalformedJpeg {
        /// Machine-readable-ish reason for the failure.
        reason: String,
    },

    /// Raw IIM bytes (or the APP13/8BIM envelope) cannot be parsed.
    #[error("ungültige IIM-Daten: {reason}")]
    InvalidIim {
        /// Reason for the failure.
        reason: String,
    },

    /// An XMP packet cannot be parsed.
    #[error("ungültige XMP-Daten: {reason}")]
    InvalidXmp {
        /// Reason for the failure.
        reason: String,
    },

    /// A single JPEG segment would exceed the 64 KiB marker size limit.
    #[error("JPEG-Segment für Feld '{field}' zu groß ({bytes} > {limit} Bytes)")]
    SegmentTooLarge {
        /// Stable field id (or `"xmp"` for the APP1 packet).
        field: &'static str,
        /// Actual segment payload size in bytes.
        bytes: usize,
        /// Maximum payload size in bytes.
        limit: usize,
    },
}
