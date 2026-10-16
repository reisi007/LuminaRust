//! G-15 smart rules, smart collections, and Library metadata field helpers.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 6, file-size ratchet).
//! Pure data types and pure helpers; no GUI behaviour change.

use super::*;

/// Build one [`SmartRule`] from the Library smart-editor inputs (G-15
/// META-MVP, Slice 3). `kind` is one of `all`, `none`, `keyword`
/// (`value` = exact case-sensitive keyword), `rating_at_least`,
/// `rating_equals` (`value` = `0..=5`), `flag` (`value` =
/// `pick|reject|unflagged`). Anything else is a loud `Err`. Pure function,
/// unit-tested headless; `And`/`Or`/`Not` composition happens on the
/// caller-held rule stack ([`combine_smart_rules`]).
pub fn build_smart_rule(kind: &str, value: &str) -> Result<SmartRule, String> {
    match kind {
        "all" => Ok(SmartRule::All),
        "none" => Ok(SmartRule::None),
        "keyword" => Ok(SmartRule::Keyword {
            keyword: value.to_string(),
        }),
        "rating_at_least" => {
            let rating: u8 = value
                .trim()
                .parse()
                .map_err(|_| format!("invalid rating `{value}`: expected 0..=5"))?;
            if rating > 5 {
                return Err(format!("invalid rating `{value}`: expected 0..=5"));
            }
            Ok(SmartRule::RatingAtLeast { rating })
        }
        "rating_equals" => {
            let rating: u8 = value
                .trim()
                .parse()
                .map_err(|_| format!("invalid rating `{value}`: expected 0..=5"))?;
            if rating > 5 {
                return Err(format!("invalid rating `{value}`: expected 0..=5"));
            }
            Ok(SmartRule::RatingEquals { rating })
        }
        "flag" => {
            let flag = match value.trim().to_lowercase().as_str() {
                "pick" => Flag::Pick,
                "reject" => Flag::Reject,
                "unflagged" => Flag::Unflagged,
                _ => {
                    return Err(format!(
                        "invalid flag `{value}`: expected pick|reject|unflagged"
                    ));
                }
            };
            Ok(SmartRule::Flag { flag })
        }
        _ => Err(format!("unknown smart-rule kind `{kind}`")),
    }
}

/// Combine caller-held [`SmartRule`] stack entries (G-15 META-MVP, Slice 3):
/// `and`/`or` need ≥2 entries, `not` needs ≥1 (pops it, pushes the
/// negation). Underflow is a loud `Err`. Pure function, unit-tested
/// headless.
pub fn combine_smart_rules(stack: &mut Vec<SmartRule>, op: &str) -> Result<(), String> {
    match op {
        "and" | "or" => {
            if stack.len() < 2 {
                return Err(format!("cannot combine `{op}`: need at least 2 rules"));
            }
            let rules = std::mem::take(stack);
            stack.push(if op == "and" {
                SmartRule::And { rules }
            } else {
                SmartRule::Or { rules }
            });
            Ok(())
        }
        "not" => {
            let rule = stack
                .pop()
                .ok_or_else(|| "cannot negate: no rule on the stack".to_string())?;
            stack.push(SmartRule::Not {
                rule: Box::new(rule),
            });
            Ok(())
        }
        _ => Err(format!("unknown smart-rule combinator `{op}`")),
    }
}

/// Read the stack-group proxy id (Welle 3, LR-17 light) from a virtual
/// copy's `extras["stack_group"]` — no sidecar schema change. Missing,
/// non-string or empty values read as `None`: the field is a
/// forward-compatible grouping annotation, while the write path
/// ([`LuminaApp::toggle_stack_group`]) is the only place ids are minted.
/// Shared by the toggle and headless tests so both read one path.
pub fn stack_id_of(extras: &BTreeMap<String, serde_json::Value>) -> Option<String> {
    extras
        .get("stack_group")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Format marker of the portable smart-collection catalog file (G-15
/// META-MVP, Slice 3) — identical to the CLI (`SmartCatalogFile` there), so
/// both sides read and write the same bytes. The file carries versioned
/// rule data only, never absolute paths.
pub const SMART_CATALOG_FORMAT: &str = "lumina-smart-catalog";

/// Portable smart-collection catalog file (G-15 META-MVP, Slice 3),
/// CLI-identical envelope. Serialized with `version =
/// SMART_COLLECTION_VERSION` and validated per definition with
/// `validate_smart_collection_def` on load and before save.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SmartCatalogFile {
    pub(super) format: String,
    pub(super) version: u8,
    pub(super) collections: Vec<SmartCollectionDef>,
}

/// The default virtual copy id of `document` (first copy when no default is
/// flagged); `None` only when the document carries no copies. Used to
/// resolve selector batch ops (`SetRating`/`SetFlag` with empty `copy_id`)
/// per target file.
pub fn default_copy_id(document: &SidecarDocument) -> Option<String> {
    document
        .virtual_copies
        .iter()
        .find(|copy| copy.is_default)
        .or_else(|| document.virtual_copies.first())
        .map(|copy| copy.id.clone())
}

/// LRPAR-G15-IPTC-S8: default field selection for
/// [`LuminaApp::sync_metadata_to_selection`]: every registry draft field plus
/// `keywords` (SOLL §10: Default alle Draft-Felder + Keywords). Unchecked
/// fields are left untouched on the targets (no mirror for them).
pub fn default_meta_sync_fields() -> BTreeMap<String, bool> {
    let mut fields = BTreeMap::new();
    for id in METADATA_FIELD_IDS {
        fields.insert((*id).to_string(), true);
    }
    fields.insert("keywords".to_string(), true);
    fields
}

/// LRPAR-G15-IPTC-S8: user-visible label of a registry draft field ID,
/// routed through [`Str`] so no panel carries a free-form literal. `None`
/// for unknown IDs (callers reject those loudly before painting).
pub fn metadata_field_label(field: &str) -> Option<&'static str> {
    match field {
        "title" => Some(Str::MetadataFieldTitle.t()),
        "headline" => Some(Str::MetadataFieldHeadline.t()),
        "description" => Some(Str::MetadataFieldDescription.t()),
        "copyright_notice" => Some(Str::MetadataFieldCopyrightNotice.t()),
        "creator" => Some(Str::MetadataFieldCreator.t()),
        "credit" => Some(Str::MetadataFieldCredit.t()),
        "source" => Some(Str::MetadataFieldSource.t()),
        "city" => Some(Str::MetadataFieldCity.t()),
        "state_province" => Some(Str::MetadataFieldStateProvince.t()),
        "country" => Some(Str::MetadataFieldCountry.t()),
        "date_created" => Some(Str::MetadataFieldDateCreated.t()),
        _ => None,
    }
}

/// LRPAR-G15-IPTC-S8: maps a registry field ID onto the embedded value of a
/// JPEG (display-only, same mapping as the CLI `meta inspect`; IDs come from
/// `METADATA_FIELD_IDS`, no second registry).
pub fn embedded_field_value<'a>(meta: &'a IptcMetadata, id: &str) -> Option<&'a str> {
    match id {
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
        "date_created" => meta.date_created.as_deref(),
        _ => None,
    }
}
