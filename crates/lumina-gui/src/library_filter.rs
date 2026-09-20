//! LRPAR-G15-STACK-15 (off-ratchet extraction): the Library `\`-filter
//! predicates and the metadata batch-operation parser.
//!
//! Extracted verbatim from `lib.rs` to keep the large crate root within its
//! file-size ratchet baseline; the public functions are re-exported by the
//! crate root, so every existing call site keeps working unchanged. Pure
//! functions only (no IO, no GUI), unit-tested headless via the crate root's
//! `#[cfg(test)] mod tests`.

use super::*;

/// Simple Library filter match (Welle 3, LR-13 light) over metadata the
/// directory scan already holds — no index, no extra IO. An empty query
/// matches everything. Tokens (whitespace-separated) combine with AND; each
/// token is one of `rating:<0-5>`, `flag:pick|reject|unflagged`,
/// `label:red|yellow|green|blue|none`, `keyword:<exact>` (case-sensitive,
/// needs entry data — see [`library_entry_matches`]), `collection:<id|name>`,
/// `camera:<substring>`, `iso:<number>`, `focal:|focal_length:<mm>`, or a
/// case-insensitive substring match on the file name. A recognised prefix
/// with an unparseable value matches nothing (visible empty grid, never a
/// silent pass-through). Pure function, unit-tested headless.
///
/// This overload carries no per-entry keyword/collection/EXIF data, so the
/// `keyword:`/`collection:`/`camera:`/`iso:`/`focal:` predicates match
/// nothing here (missing data is never a silent pass-through); use
/// [`library_entry_matches`] for the full entry-aware evaluation.
pub fn library_filter_matches(
    name: &str,
    rating: u8,
    flag: Flag,
    color_label: u8,
    query: &str,
) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    query
        .split_whitespace()
        .all(|token| library_filter_token_matches(token, name, rating, flag, color_label))
}

/// One whitespace-separated token of [`library_filter_matches`]. Pure
/// function shared by both filter overloads.
fn library_filter_token_matches(
    token: &str,
    name: &str,
    rating: u8,
    flag: Flag,
    color_label: u8,
) -> bool {
    let lowered = token.to_lowercase();
    if let Some(rest) = lowered.strip_prefix("rating:") {
        return rest.trim().parse::<u8>().is_ok_and(|want| want == rating);
    }
    if let Some(rest) = lowered.strip_prefix("flag:") {
        let want = match rest.trim() {
            "pick" => Flag::Pick,
            "reject" => Flag::Reject,
            "unflagged" => Flag::Unflagged,
            _ => return false,
        };
        return want == flag;
    }
    if let Some(rest) = lowered.strip_prefix("label:") {
        let want = match rest.trim() {
            "red" => 1,
            "yellow" => 2,
            "green" => 3,
            "blue" => 4,
            "none" => 0,
            _ => return false,
        };
        return want == color_label;
    }
    // Extended G-15 predicates need per-entry data (keywords, collections,
    // EXIF) that this overload does not carry: without data they match
    // nothing rather than passing silently. The prefix itself must still be
    // recognised here so `keyword:x` is not misread as a file-name search.
    // `keyword:` compares case-sensitively on the original token.
    if token.len() >= 8 && token[..8].eq_ignore_ascii_case("keyword:") {
        return false;
    }
    for prefix in [
        "collection:",
        "camera:",
        "iso:",
        "focal:",
        "focal_length:",
        "cull:",
        "person:",
    ] {
        if lowered.starts_with(prefix) {
            return false;
        }
    }
    name.to_lowercase().contains(&lowered)
}

/// Full G-15 META-MVP (Slice 3) Library filter over a scanned
/// [`FileBrowserEntry`]: the `\`-query tokens (see
/// [`library_filter_matches`]) AND-combined, where the extended predicates
/// evaluate against cached entry data — `keyword:` exact case-sensitive
/// (Slice-1 semantics), `collection:` exact `id` or exact `name`
/// (case-insensitive), `camera:` case-insensitive substring of
/// `make + model`, `iso:` exact-vs-epsilon numeric match,
/// `focal:`/`focal_length:` exact-vs-epsilon match in mm. Missing entry
/// data matches nothing for that predicate. `collection:` and `camera:`
/// values may contain spaces: following tokens without a `:` belong to the
/// value (`collection:best of` matches the collection named `Best Of`).
/// Pure function, unit-tested headless.
pub fn library_entry_matches(entry: &FileBrowserEntry, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let tokens: Vec<&str> = query.split_whitespace().collect();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let lowered = token.to_lowercase();
        // Multi-word values: `collection:`/`camera:` consume following
        // tokens that carry no `:` of their own.
        let multi = ["collection:", "camera:"]
            .into_iter()
            .find(|prefix| lowered.starts_with(prefix));
        if let Some(prefix) = multi {
            let mut value = token[prefix.len()..].to_string();
            let mut next = index + 1;
            while next < tokens.len() && !tokens[next].contains(':') {
                value.push(' ');
                value.push_str(tokens[next]);
                next += 1;
            }
            if !entry_multi_token_matches(entry, prefix, &value) {
                return false;
            }
            index = next;
            continue;
        }
        if !entry_single_token_matches(entry, token) {
            return false;
        }
        index += 1;
    }
    true
}

/// `collection:`/`camera:` value match (see [`library_entry_matches`]).
/// Empty values match nothing — never a silent pass-through.
fn entry_multi_token_matches(entry: &FileBrowserEntry, prefix: &str, value: &str) -> bool {
    let want = value.trim();
    if want.is_empty() {
        return false;
    }
    match prefix {
        "collection:" => {
            let want = want.to_lowercase();
            entry
                .collections
                .iter()
                .any(|m| m.id.to_lowercase() == want || m.name.to_lowercase() == want)
        }
        "camera:" => entry
            .camera
            .as_deref()
            .is_some_and(|camera| camera.to_lowercase().contains(&want.to_lowercase())),
        _ => false,
    }
}

/// One `\` token against a scanned entry (see [`library_entry_matches`]):
/// `keyword:` (exact, case-sensitive), `iso:`/`focal:`/`focal_length:`
/// (numeric, unparseable matches nothing), then the shared
/// rating/flag/label/name matcher.
fn entry_single_token_matches(entry: &FileBrowserEntry, token: &str) -> bool {
    // `keyword:` compares case-sensitively on the original token.
    if token.len() >= 8 && token[..8].eq_ignore_ascii_case("keyword:") {
        let want = &token[8..];
        return entry.keywords.iter().any(|k| k == want);
    }
    let lowered = token.to_lowercase();
    // LRPAR-G12-FACE-20 (S5): `person:<name>` over the scanned source-level
    // person labels of the sidecar's face analysis (exact, case-insensitive).
    // An empty value matches nothing (never a silent pass-through).
    if let Some(rest) = lowered.strip_prefix("person:") {
        let want = rest.trim();
        return !want.is_empty()
            && entry
                .face_persons
                .iter()
                .any(|person| person.to_lowercase() == want);
    }
    // LRPAR-G09-CULL-25: `cull:keep|review|reject|none|stale` over the
    // scan-level assisted-culling badge. An unknown value matches nothing (a
    // visible empty grid, never a silent pass-through) and is warned loudly.
    if let Some(rest) = lowered.strip_prefix("cull:") {
        let rest = rest.trim();
        if cull_gui::CullBadge::from_token(rest).is_none() {
            cull_gui::warn_unknown_cull_token(rest);
            return false;
        }
        return cull_gui::cull_filter_token_matches(&lowered, entry.cull_badge).unwrap_or(false);
    }
    if let Some(rest) = lowered.strip_prefix("iso:") {
        let parsed: Option<f32> = rest.trim().parse().ok();
        let Some(want) = parsed.filter(|v| v.is_finite()) else {
            return false;
        };
        return entry
            .iso
            .is_some_and(|iso| (iso - want).abs() <= (want.abs() * 1e-3 + 1e-6));
    }
    if let Some(rest) = lowered
        .strip_prefix("focal_length:")
        .or_else(|| lowered.strip_prefix("focal:"))
    {
        let parsed: Option<f32> = rest.trim().parse().ok();
        let Some(want) = parsed.filter(|v| v.is_finite()) else {
            return false;
        };
        return entry
            .focal_length
            .is_some_and(|focal| (focal - want).abs() <= (want.abs() * 1e-3 + 1e-6));
    }
    library_filter_token_matches(
        token,
        &entry.name,
        entry.rating,
        entry.flag,
        entry.color_label,
    )
}

/// Which collection view filters the Library grid (G-15 META-MVP, Slice 3):
/// none (all images), one static collection (by stable `id`), or one smart
/// collection (by stable `id`, resolved against the loaded catalog).
/// Pure data, unit-tested headless via [`collection_filter_matches_entry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionFilter {
    Static { id: String },
    Smart { id: String },
}

/// Whether `entry` passes `filter`. A static filter matches membership `id`
/// (exact); a smart filter evaluates the catalog rule with
/// `matches_any_copy` over a synthetic single-copy view of the entry
/// (entry keywords + default-copy rating/flag — the same state the grid
/// badge shows). Unknown smart `id` or evaluation error matches nothing
/// loudly at the call site (this pure helper returns `false`; the panel
/// surfaces the error text). Pure function, unit-tested headless.
pub fn collection_filter_matches_entry(
    entry: &FileBrowserEntry,
    filter: &CollectionFilter,
    smart_catalog: &[SmartCollectionDef],
) -> bool {
    match filter {
        CollectionFilter::Static { id } => entry.collections.iter().any(|m| m.id == *id),
        CollectionFilter::Smart { id } => {
            let Some(def) = smart_catalog.iter().find(|def| def.id == *id) else {
                return false;
            };
            // Entry-level view: keywords plus the default copy's rating/flag.
            let rule_result = def.rule.matches(&entry.keywords, entry.rating, entry.flag);
            // A stale `version` must not silently match: only evaluate when
            // the definition validates.
            if lumina_sidecar::validate_smart_collection_def(def).is_err() {
                return false;
            }
            rule_result
        }
    }
}

/// Parse a batch-operation selector of the Library batch bar into the
/// Slice-1 [`BatchOp`] language (G-15 META-MVP, Slice 3). `kind` is one of
/// `add_keyword`, `remove_keyword`, `add_to_collection` (`value` =
/// `id=name`), `remove_from_collection` (`value` = `id`), `set_rating`
/// (`value` = `0..=5`), `set_flag` (`value` =
/// `pick|reject|unflagged`). Anything else — unknown kind, malformed value,
/// out-of-range rating — is a loud `Err`, never a silent no-op. Pure
/// function, unit-tested headless.
pub fn parse_metadata_batch_op(kind: &str, value: &str) -> Result<BatchOp, String> {
    match kind {
        "add_keyword" => Ok(BatchOp::AddKeyword {
            keyword: value.to_string(),
        }),
        "remove_keyword" => Ok(BatchOp::RemoveKeyword {
            keyword: value.to_string(),
        }),
        "add_to_collection" => {
            let (id, name) = value.split_once('=').ok_or_else(|| {
                format!("invalid collection assignment `{value}`: expected `id=name`")
            })?;
            Ok(BatchOp::AddToCollection {
                id: id.to_string(),
                name: name.to_string(),
            })
        }
        "remove_from_collection" => Ok(BatchOp::RemoveFromCollection {
            id: value.to_string(),
        }),
        "set_rating" => {
            // Loud validation here mirrors `set_rating` (never clamp); the
            // sidecar validates again on apply.
            let rating: u8 = value
                .trim()
                .parse()
                .map_err(|_| format!("invalid rating `{value}`: expected 0..=5"))?;
            if rating > 5 {
                return Err(format!("invalid rating `{value}`: expected 0..=5"));
            }
            Ok(BatchOp::SetRating {
                copy_id: String::new(),
                rating,
            })
        }
        "set_flag" => {
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
            Ok(BatchOp::SetFlag {
                copy_id: String::new(),
                flag,
            })
        }
        _ => Err(format!("unknown batch operation `{kind}`")),
    }
}
