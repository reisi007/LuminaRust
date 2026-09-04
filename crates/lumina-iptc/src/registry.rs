//! Feld-Registry (MVP) nach `feature/product/iptc-metadata.md` §4.
//!
//! The registry is the contract: stable field ids, IIM record/dataset numbers,
//! XMP property names and IIM octet limits. Limits are enforced in **octets**
//! (UTF-8 bytes), never in characters, and violations are loud errors —
//! nothing is silently truncated.

use crate::error::IptcError;

/// Definition of a single registry field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDef {
    /// Stable field id (e.g. `"title"`).
    pub id: &'static str,
    /// IIM record number and dataset number (e.g. `(2, 5)` for 2:05).
    pub iim: (u8, u8),
    /// XMP property name with prefix (e.g. `"dc:title"`).
    pub xmp: &'static str,
    /// IIM limit in octets (UTF-8 bytes). `None` for fields with a fixed
    /// format (`date_created`) instead of a length budget.
    pub max_octets: Option<usize>,
}

/// Maximum number of `keywords` entries (IIM 2:25, shared with the Sidecar
/// keyword validation).
pub const MAX_KEYWORDS: usize = 512;

/// Octet limit for a single keyword.
pub const MAX_KEYWORD_OCTETS: usize = 128;

/// MVP field registry. Order is the deterministic IIM write order for
/// single-value datasets (REGISTRY order: 2:05 … 2:120 aufsteigend, dann
/// `date_created` 2:55); `keywords` (2:25) ist repeatable und folgt zuletzt,
/// ein Dataset pro Eintrag in Vektor-Reihenfolge.
pub const REGISTRY: &[FieldDef] = &[
    FieldDef {
        id: "title",
        iim: (2, 5),
        xmp: "dc:title",
        max_octets: Some(256),
    },
    FieldDef {
        id: "creator",
        iim: (2, 80),
        xmp: "dc:creator",
        max_octets: Some(256),
    },
    FieldDef {
        id: "city",
        iim: (2, 90),
        xmp: "photoshop:City",
        max_octets: Some(128),
    },
    FieldDef {
        id: "state_province",
        iim: (2, 95),
        xmp: "photoshop:State",
        max_octets: Some(128),
    },
    FieldDef {
        id: "country",
        iim: (2, 101),
        xmp: "photoshop:Country",
        max_octets: Some(128),
    },
    FieldDef {
        id: "headline",
        iim: (2, 105),
        xmp: "photoshop:Headline",
        max_octets: Some(256),
    },
    FieldDef {
        id: "credit",
        iim: (2, 110),
        xmp: "photoshop:Credit",
        max_octets: Some(256),
    },
    FieldDef {
        id: "source",
        iim: (2, 115),
        xmp: "photoshop:Source",
        max_octets: Some(256),
    },
    FieldDef {
        id: "copyright_notice",
        iim: (2, 116),
        xmp: "dc:rights",
        max_octets: Some(256),
    },
    FieldDef {
        id: "description",
        iim: (2, 120),
        xmp: "dc:description",
        max_octets: Some(2000),
    },
    FieldDef {
        id: "date_created",
        iim: (2, 55),
        xmp: "photoshop:DateCreated",
        max_octets: None,
    },
];

/// Look up a field definition by stable id.
pub fn field_by_id(id: &str) -> Option<&'static FieldDef> {
    REGISTRY.iter().find(|f| f.id == id)
}

/// Look up a field definition by IIM record/dataset number.
pub fn field_by_iim(record: u8, dataset: u8) -> Option<&'static FieldDef> {
    REGISTRY.iter().find(|f| f.iim == (record, dataset))
}

/// IPTC metadata as plain values, mirroring the Sidecar draft plus the routed
/// `keywords`.
///
/// `None` (or an empty/whitespace-only string) means "field absent": the tag
/// is omitted on write, never written empty. `date_created` uses the Sidecar
/// format `YYYY-MM-DD` and is converted to IIM `YYYYMMDD` on write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IptcMetadata {
    /// Titel (2:05, `dc:title`), ≤ 256 Oktette.
    pub title: Option<String>,
    /// Schlagzeile (2:105, `photoshop:Headline`), ≤ 256 Oktette.
    pub headline: Option<String>,
    /// Beschreibung (2:120, `dc:description`), ≤ 2000 Oktette.
    pub description: Option<String>,
    /// Urheberrechtsvermerk (2:116, `dc:rights` + `xmpRights:Marked=true`),
    /// ≤ 256 Oktette.
    pub copyright_notice: Option<String>,
    /// Ersteller (2:80, `dc:creator`), ≤ 256 Oktette.
    pub creator: Option<String>,
    /// Credit (2:110, `photoshop:Credit`), ≤ 256 Oktette.
    pub credit: Option<String>,
    /// Quelle (2:115, `photoshop:Source`), ≤ 256 Oktette.
    pub source: Option<String>,
    /// Ort/Stadt (2:90, `photoshop:City`), ≤ 128 Oktette.
    pub city: Option<String>,
    /// Bundesland/Kanton (2:95, `photoshop:State`), ≤ 128 Oktette.
    pub state_province: Option<String>,
    /// Land (2:101, `photoshop:Country`), ≤ 128 Oktette.
    pub country: Option<String>,
    /// Aufnahmedatum (2:55, `photoshop:DateCreated`), Format `YYYY-MM-DD`.
    pub date_created: Option<String>,
    /// Schlüsselwörter (2:25 + `dc:subject`), je ≤ 128 Oktette, ≤ 512 Einträge.
    pub keywords: Vec<String>,
}

impl IptcMetadata {
    /// `true` when no field and no keyword carries a non-empty value.
    pub fn is_empty(&self) -> bool {
        self.normalized().is_absent()
    }

    /// Validate all present values against the registry.
    ///
    /// Empty/whitespace-only values are absent, not invalid: they are omitted
    /// on write. Everything else that violates the contract (octet limits,
    /// date format, control characters, keyword count) is a loud error.
    pub fn validate(&self) -> Result<(), IptcError> {
        for def in REGISTRY {
            if def.id == "date_created" {
                if let Some(raw) = self.value_by_id("date_created") {
                    if !raw.trim().is_empty() {
                        validate_date(raw.trim())?;
                    }
                }
                continue;
            }
            let limit = def.max_octets.unwrap_or(usize::MAX);
            if let Some(raw) = self.value_by_id(def.id) {
                let value = raw.trim();
                if value.is_empty() {
                    continue;
                }
                check_text(def.id, value, limit)?;
            }
        }
        if self.keywords.len() > MAX_KEYWORDS {
            return Err(IptcError::TooManyKeywords {
                limit: MAX_KEYWORDS,
                actual: self.keywords.len(),
            });
        }
        for keyword in &self.keywords {
            let value = keyword.trim();
            if value.is_empty() {
                continue;
            }
            check_text("keywords", value, MAX_KEYWORD_OCTETS).map_err(|e| match e {
                IptcError::FieldTooLong {
                    limit,
                    actual_octets,
                    ..
                } => IptcError::KeywordTooLong {
                    limit,
                    actual_octets,
                },
                IptcError::ControlCharacters { .. } => {
                    IptcError::ControlCharacters { field: "keywords" }
                }
                other => other,
            })?;
        }
        Ok(())
    }

    /// Normalized form: trimmed values, empty values dropped.
    ///
    /// Used to compare roundtrips (`decode(encode(m)) == m.normalized()`).
    pub fn normalized(&self) -> IptcMetadata {
        let mut out = IptcMetadata::default();
        for def in REGISTRY {
            let value = self
                .value_by_id(def.id)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string);
            out.set_by_id(def.id, value);
        }
        out.keywords = self
            .keywords
            .iter()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        out
    }

    fn is_absent(&self) -> bool {
        REGISTRY
            .iter()
            .all(|def| self.value_by_id(def.id).is_none_or(|v| v.trim().is_empty()))
            && self.keywords.iter().all(|k| k.trim().is_empty())
    }

    fn value_by_id(&self, id: &str) -> Option<&str> {
        match id {
            "title" => self.title.as_deref(),
            "headline" => self.headline.as_deref(),
            "description" => self.description.as_deref(),
            "copyright_notice" => self.copyright_notice.as_deref(),
            "creator" => self.creator.as_deref(),
            "credit" => self.credit.as_deref(),
            "source" => self.source.as_deref(),
            "city" => self.city.as_deref(),
            "state_province" => self.state_province.as_deref(),
            "country" => self.country.as_deref(),
            "date_created" => self.date_created.as_deref(),
            _ => None,
        }
    }

    fn set_by_id(&mut self, id: &str, value: Option<String>) {
        match id {
            "title" => self.title = value,
            "headline" => self.headline = value,
            "description" => self.description = value,
            "copyright_notice" => self.copyright_notice = value,
            "creator" => self.creator = value,
            "credit" => self.credit = value,
            "source" => self.source = value,
            "city" => self.city = value,
            "state_province" => self.state_province = value,
            "country" => self.country = value,
            "date_created" => self.date_created = value,
            _ => {}
        }
    }
}

/// Check octet length and control characters of a single non-empty value.
fn check_text(field: &'static str, value: &str, limit: usize) -> Result<(), IptcError> {
    let octets = value.len();
    if octets > limit {
        return Err(IptcError::FieldTooLong {
            field,
            limit,
            actual_octets: octets,
        });
    }
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(IptcError::ControlCharacters { field });
    }
    Ok(())
}

/// Validate `YYYY-MM-DD` as a real calendar date.
fn validate_date(value: &str) -> Result<(), IptcError> {
    let invalid = || IptcError::InvalidDate {
        field: "date_created",
        value: value.to_string(),
    };
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(invalid());
    }
    let digits = |lo: usize, hi: usize| -> Result<u32, IptcError> {
        value[lo..hi].parse::<u32>().map_err(|_| invalid())
    };
    let year = digits(0, 4)?;
    let month = digits(5, 7)?;
    let day = digits(8, 10)?;
    if year == 0 || !(1..=12).contains(&month) || day == 0 {
        return Err(invalid());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(invalid()),
    };
    if day > max_day {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_all_single_value_iim_tags() {
        // Single-value fields of REGISTRY plus the repeatable keywords 2:25
        // must cover exactly the task's tag list:
        // 2:05/2:80/2:90/2:95/2:101/2:105/2:110/2:115/2:116/2:120/2:55 + 2:25.
        let mut tags: Vec<(u8, u8)> = REGISTRY.iter().map(|f| f.iim).collect();
        tags.push((2, 25)); // keywords, repeatable
        tags.sort();
        assert_eq!(
            tags,
            vec![
                (2, 5),
                (2, 25),
                (2, 55),
                (2, 80),
                (2, 90),
                (2, 95),
                (2, 101),
                (2, 105),
                (2, 110),
                (2, 115),
                (2, 116),
                (2, 120),
            ]
        );
    }

    #[test]
    fn octet_limit_counts_bytes_not_chars() {
        // 64 × 4-byte emoji = 256 octets: ok. One more char: loud error.
        let ok = "🎉".repeat(64);
        assert!(check_text("title", &ok, 256).is_ok());
        let over = "🎉".repeat(65);
        assert_eq!(
            check_text("title", &over, 256),
            Err(IptcError::FieldTooLong {
                field: "title",
                limit: 256,
                actual_octets: 260
            })
        );
    }

    #[test]
    fn date_validation() {
        assert!(validate_date("2026-09-04").is_ok());
        assert!(validate_date("2024-02-29").is_ok());
        for bad in [
            "2026-13-01",
            "2026-00-10",
            "2026-02-30",
            "2023-02-29",
            "2026-9-4",
            "2026/09/04",
            "",
            "gestern",
        ] {
            assert!(validate_date(bad).is_err(), "{bad} must be rejected");
        }
    }
}
