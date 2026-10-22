//! LRPAR-G15-IPTC-S1: the source-level IPTC metadata draft — schema, field
//! registry, value validation and the RFC 3339 UTC timestamp helper.
//!
//! SPLICE-SIDECAR-LIB-90 (Slice 7): this block was moved out of the sidecar
//! root file (file-size ratchet). The public symbols keep their old paths —
//! the root re-exports them, so `lumina_sidecar::MetadataDraft`,
//! `lumina_sidecar::is_metadata_field` and the rest stay resolvable. The
//! crate-internal symbols `validate_metadata_field_id`, `MetadataDraft::validate`
//! and `validate_metadata_date_created` are re-exported by the root for its
//! remaining call sites and for its doc link.

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::{
    invalid, SidecarError, MAX_METADATA_CITY_CHARS, MAX_METADATA_COPYRIGHT_NOTICE_CHARS,
    MAX_METADATA_COUNTRY_CHARS, MAX_METADATA_CREATOR_CHARS, MAX_METADATA_CREDIT_CHARS,
    MAX_METADATA_DESCRIPTION_CHARS, MAX_METADATA_HEADLINE_CHARS, MAX_METADATA_HISTORY_ENTRIES,
    MAX_METADATA_SOURCE_CHARS, MAX_METADATA_STATE_PROVINCE_CHARS, MAX_METADATA_TITLE_CHARS,
    METADATA_DRAFT_VERSION, METADATA_FIELD_IDS,
};
// `SidecarDocument` is referenced only by the intra-doc links of the moved
// block (`SidecarDocument::apply_metadata_draft`); the import keeps those
// links resolvable — without it rustdoc reports a broken intra-doc link.
#[allow(unused_imports)]
use crate::SidecarDocument;

/// LRPAR-G15-IPTC-S1: one entry of the metadata draft history. The history
/// is provenance/diagnostic context — **not** an undo log. Entries are
/// stored newest-first (the first entry carries the highest `rev`);
/// [`SidecarDocument::apply_metadata_draft`] and the clear helpers prepend
/// exactly one entry per mutating call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataHistoryEntry {
    /// Strictly monotonous revision, starting at 1. Strictly decreasing in
    /// stored order (newest first).
    pub rev: u64,
    /// RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SS[.frac]Z`), e.g.
    /// `2026-09-04T10:00:00Z`.
    pub timestamp: String,
    /// Mutation origin: `manual` | `cli` | `gui` | `mcp` |
    /// `preset:<name>` | `sync:<quell-sidecar-id>`.
    pub origin: String,
    /// Registry field IDs affected by the mutation (`keywords` allowed: a
    /// sync may carry keywords alongside draft fields). Sorted
    /// deterministically by the mutation helpers.
    pub changed: Vec<String>,
}

/// LRPAR-G15-IPTC-S1: the source-level IPTC metadata draft. Additive and
/// optional: absent in older sidecars reads as the empty draft (see the
/// `Default` impl) and an empty draft serializes back absent, so legacy
/// documents stay byte-stable. Drafts live on source-image level — never
/// per virtual copy — and sync/bake-in never touch recipes, masks or the
/// per-copy edit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataDraft {
    #[serde(default = "metadata_draft_version_default")]
    pub version: u8,
    /// Draft values keyed by registry field ID. A stored value is always
    /// trimmed and non-empty; clearing a field removes its key (see
    /// [`SidecarDocument::apply_metadata_draft`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub draft: BTreeMap<String, String>,
    /// Provenance history, newest first, capped at
    /// [`MAX_METADATA_HISTORY_ENTRIES`] entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<MetadataHistoryEntry>,
}

fn metadata_draft_version_default() -> u8 {
    METADATA_DRAFT_VERSION
}

impl Default for MetadataDraft {
    fn default() -> Self {
        Self {
            version: METADATA_DRAFT_VERSION,
            draft: BTreeMap::new(),
            history: Vec::new(),
        }
    }
}

impl MetadataDraft {
    /// True when both the draft map and the history are empty — the legacy
    /// identity, serialized back absent on `SidecarDocument`.
    pub fn is_empty(&self) -> bool {
        self.draft.is_empty() && self.history.is_empty()
    }

    /// Current draft value of `field`, if set.
    pub fn get(&self, field: &str) -> Option<&str> {
        self.draft.get(field).map(String::as_str)
    }

    /// Highest `rev` in the history, or 0 when the history is empty. The
    /// next mutation entry carries `latest_rev() + 1`.
    pub fn latest_rev(&self) -> u64 {
        self.history.first().map(|entry| entry.rev).unwrap_or(0)
    }

    pub(crate) fn validate(&self) -> Result<(), SidecarError> {
        if self.version != METADATA_DRAFT_VERSION {
            return invalid(format!(
                "unsupported metadata version {} (expected {METADATA_DRAFT_VERSION})",
                self.version
            ));
        }
        for (field, value) in &self.draft {
            validate_metadata_field_id(field)?;
            validate_metadata_stored_value(field, value)?;
        }
        if self.history.len() > MAX_METADATA_HISTORY_ENTRIES {
            return invalid(format!(
                "metadata history exceeds limit of {MAX_METADATA_HISTORY_ENTRIES} entries"
            ));
        }
        let mut previous_rev: Option<u64> = None;
        for entry in &self.history {
            if entry.rev == 0 {
                return invalid("metadata history rev must start at 1");
            }
            if let Some(previous) = previous_rev {
                if entry.rev >= previous {
                    return invalid(
                        "metadata history rev must be strictly decreasing (newest first)",
                    );
                }
            }
            previous_rev = Some(entry.rev);
            validate_metadata_timestamp(&entry.timestamp)?;
            validate_metadata_origin(&entry.origin)?;
            if entry.changed.is_empty() {
                return invalid(format!(
                    "metadata history rev {} must list at least one changed field",
                    entry.rev
                ));
            }
            let mut seen = BTreeSet::new();
            for field in &entry.changed {
                validate_metadata_changed_id(field)?;
                if !seen.insert(field) {
                    return invalid(format!(
                        "metadata history rev {} lists duplicate changed field `{field}`",
                        entry.rev
                    ));
                }
            }
        }
        Ok(())
    }
}

/// LRPAR-G15-IPTC-S1: true for the 11 registered draft field IDs (SOLL §4).
/// `keywords` is *not* a draft field (see [`METADATA_FIELD_IDS`]).
pub fn is_metadata_field(id: &str) -> bool {
    METADATA_FIELD_IDS.contains(&id)
}

/// LRPAR-G15-IPTC-S1: Sidecar character limit of a registered draft field,
/// or `None` for `date_created` (format-constrained instead). Returns `None`
/// for unknown IDs as well — callers must reject those loudly via
/// [`validate_metadata_field_id`].
pub fn metadata_field_limit(field: &str) -> Option<usize> {
    match field {
        "title" => Some(MAX_METADATA_TITLE_CHARS),
        "headline" => Some(MAX_METADATA_HEADLINE_CHARS),
        "description" => Some(MAX_METADATA_DESCRIPTION_CHARS),
        "copyright_notice" => Some(MAX_METADATA_COPYRIGHT_NOTICE_CHARS),
        "creator" => Some(MAX_METADATA_CREATOR_CHARS),
        "credit" => Some(MAX_METADATA_CREDIT_CHARS),
        "source" => Some(MAX_METADATA_SOURCE_CHARS),
        "city" => Some(MAX_METADATA_CITY_CHARS),
        "state_province" => Some(MAX_METADATA_STATE_PROVINCE_CHARS),
        "country" => Some(MAX_METADATA_COUNTRY_CHARS),
        "date_created" => None,
        _ => None,
    }
}

/// LRPAR-G15-IPTC-S1: rejects unknown draft field IDs loudly. `keywords`
/// gets an explicit routing hint instead of the generic unknown-ID error:
/// it stays the existing source-level sidecar field and is never duplicated
/// into the draft map.
pub(crate) fn validate_metadata_field_id(field: &str) -> Result<(), SidecarError> {
    if is_metadata_field(field) {
        return Ok(());
    }
    if field == "keywords" {
        return invalid(
            "keywords is not a metadata draft field; use the document `keywords` field",
        );
    }
    invalid(format!("unknown metadata field `{field}`"))
}

/// LRPAR-G15-IPTC-S1: IDs allowed in a history `changed` list — the draft
/// registry plus `keywords` (a field-selective sync may carry keywords
/// alongside draft fields; see SOLL §6).
fn validate_metadata_changed_id(field: &str) -> Result<(), SidecarError> {
    if is_metadata_field(field) || field == "keywords" {
        return Ok(());
    }
    invalid(format!("unknown metadata changed field `{field}`"))
}

/// LRPAR-G15-IPTC-S1: validates a candidate new value *before* mutation.
/// An empty or whitespace-only value is accepted and means "remove the
/// field" (see [`SidecarDocument::apply_metadata_draft`]); anything else
/// must be trimmed, free of control characters, within the field limit, and
/// — for `date_created` — a valid `YYYY-MM-DD` date. Unknown IDs fail
/// loudly; nothing is ever normalized silently.
pub fn validate_metadata_field_value(field: &str, value: &str) -> Result<(), SidecarError> {
    validate_metadata_field_id(field)?;
    if value.is_empty() || value.trim().is_empty() {
        return Ok(());
    }
    if value != value.trim() {
        return invalid(format!(
            "metadata field `{field}` must be trimmed (no leading/trailing whitespace)"
        ));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!(
            "metadata field `{field}` must not contain control characters"
        ));
    }
    if field == "date_created" {
        return validate_metadata_date_created(value);
    }
    if let Some(limit) = metadata_field_limit(field) {
        if value.chars().count() > limit {
            return invalid(format!(
                "metadata field `{field}` exceeds limit of {limit} characters"
            ));
        }
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: validates an already-stored draft value. Unlike the
/// mutation input (where empty means "remove"), a persisted empty or
/// untrimmed value is a loud schema violation — helpers never write such
/// states, so their presence means hand-editing or corruption.
fn validate_metadata_stored_value(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.is_empty() || value != value.trim() {
        return invalid(format!(
            "metadata field `{field}` must be stored trimmed and non-empty"
        ));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!(
            "metadata field `{field}` must not contain control characters"
        ));
    }
    if field == "date_created" {
        return validate_metadata_date_created(value);
    }
    if let Some(limit) = metadata_field_limit(field) {
        if value.chars().count() > limit {
            return invalid(format!(
                "metadata field `{field}` exceeds limit of {limit} characters"
            ));
        }
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: `date_created` accepts exactly `YYYY-MM-DD` with a
/// calendar-valid month/day (including leap years). Anything else —
/// datetimes, other separators, out-of-range months or impossible days —
/// fails loudly.
pub(crate) fn validate_metadata_date_created(value: &str) -> Result<(), SidecarError> {
    const HINT: &str = "metadata field `date_created` must use format `YYYY-MM-DD`";
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return invalid(HINT);
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let slice = value.get(range)?;
        if !slice.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        slice.parse().ok()
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return invalid(HINT);
    };
    if !(1..=12).contains(&month) {
        return invalid(HINT);
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > max_day {
        return invalid(HINT);
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: validates a history `origin`: `manual` | `cli` |
/// `gui` | `mcp` | `preset:<name>` | `sync:<quell-sidecar-id>`. The suffixed
/// forms require a trimmed, non-empty name without control characters; a
/// leading `/` is rejected so no absolute path can hide in `metadata`
/// (SOLL invariant "keine absoluten Pfade"). Unknown origins fail loudly.
pub fn validate_metadata_origin(origin: &str) -> Result<(), SidecarError> {
    const SIMPLE: &[&str] = &["manual", "cli", "gui", "mcp"];
    if SIMPLE.contains(&origin) {
        return Ok(());
    }
    for prefix in ["preset:", "sync:"] {
        if let Some(name) = origin.strip_prefix(prefix) {
            if name.is_empty()
                || name != name.trim()
                || name.chars().any(char::is_control)
                || name.starts_with('/')
            {
                return invalid(format!(
                    "metadata origin `{origin}` has an invalid `{prefix}` name \
                     (must be trimmed, non-empty, without control characters or leading `/`)"
                ));
            }
            return Ok(());
        }
    }
    invalid(format!(
        "unknown metadata origin `{origin}` \
         (expected `manual`|`cli`|`gui`|`mcp`|`preset:<name>`|`sync:<quell-id>`)"
    ))
}

/// LRPAR-G15-IPTC-S1: validates a history `timestamp` as canonical RFC 3339
/// UTC (`YYYY-MM-DDTHH:MM:SS[.frac]Z` with calendar-valid fields). Offsets
/// (`+02:00`) and offset-less datetimes are rejected loudly — history
/// timestamps are always UTC (`Z`).
pub fn validate_metadata_timestamp(timestamp: &str) -> Result<(), SidecarError> {
    const HINT: &str = "metadata timestamp must be RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`)";
    let inner = timestamp
        .strip_suffix('Z')
        .ok_or_else(|| SidecarError::Invalid(HINT.into()))?;
    let (date, time) = inner
        .split_once('T')
        .ok_or_else(|| SidecarError::Invalid(HINT.into()))?;
    let date_parts: Vec<&str> = date.split('-').collect();
    let time_parts: Vec<&str> = time.split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return invalid(HINT);
    }
    let number = |part: &str, len: usize| -> Option<u32> {
        if part.len() != len || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let (Some(year), Some(month), Some(day)) = (
        number(date_parts[0], 4),
        number(date_parts[1], 2),
        number(date_parts[2], 2),
    ) else {
        return invalid(HINT);
    };
    let (Some(hour), Some(minute)) = (number(time_parts[0], 2), number(time_parts[1], 2)) else {
        return invalid(HINT);
    };
    let second = match time_parts[2].split_once('.') {
        None => number(time_parts[2], 2),
        Some((secs, frac)) => {
            if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                number(secs, 2)
            }
        }
    };
    let Some(second) = second else {
        return invalid(HINT);
    };
    if month == 0 || month > 12 || hour > 23 || minute > 59 || second > 59 {
        return invalid(HINT);
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > max_day {
        return invalid(HINT);
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: current UTC time as canonical RFC 3339 (`...Z`),
/// std-only (no date crate needed in this crate). Mutation helpers take an
/// explicit `timestamp` so tests stay deterministic; CLI/GUI/MCP slices use
/// this constructor for real mutations.
pub fn now_rfc3339_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let second_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60
    )
}

/// Days since the Unix epoch → (year, month, day). Howard Hinnant's
/// `civil_from_days` algorithm; Euclidean division keeps pre-1970 inputs
/// well-defined (practically unreachable: falls back to the epoch).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
