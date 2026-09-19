//! UX-LOOK-HISTORY-18 (Release 1.0): GUI helpers for the readable edit-history
//! step (`parameter from → to` plus the stored time) and for deriving the
//! structured changes of a recipe mutation.
//!
//! The persisted schema lives in `lumina-sidecar::history`; this module only
//! builds the display label and the change list from a before/after recipe.
//! Nothing here writes silently: a recipe pair is diffed deterministically and
//! the entries are attached through [`HistoryEntry::set_changes`], which
//! validates them loudly.

use lumina_sidecar::{
    EditRecipe, HistoryChange, HistoryEntry, MAX_HISTORY_CHANGES, MAX_HISTORY_CHANGE_VALUE_CHARS,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// Deterministic, deterministic-order diff of two recipes into structured
/// history changes. Only genuinely differing leaves become changes; object
/// containers recurse. The list is capped at the schema limit.
pub(crate) fn recipe_changes(before: &EditRecipe, after: &EditRecipe) -> Vec<HistoryChange> {
    let before = serde_json::to_value(before).unwrap_or(Value::Null);
    let after = serde_json::to_value(after).unwrap_or(Value::Null);
    let mut out = Vec::new();
    collect_changes("", &before, &after, &mut out);
    out
}

fn collect_changes(prefix: &str, before: &Value, after: &Value, out: &mut Vec<HistoryChange>) {
    if out.len() >= MAX_HISTORY_CHANGES {
        return;
    }
    if let (Value::Object(before), Value::Object(after)) = (before, after) {
        let mut keys: BTreeSet<&String> = before.keys().collect();
        keys.extend(after.keys());
        for key in keys {
            if out.len() >= MAX_HISTORY_CHANGES {
                return;
            }
            let child = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            collect_changes(
                &child,
                before.get(key).unwrap_or(&Value::Null),
                after.get(key).unwrap_or(&Value::Null),
                out,
            );
        }
        return;
    }
    if before != after {
        out.push(HistoryChange {
            parameter: display_parameter(prefix),
            from: display_value(before),
            to: display_value(after),
        });
    }
}

/// The user-facing control name: the raw recipe path without the internal
/// `adjustments.` container prefix (`adjustments.exposure` → `exposure`).
fn display_parameter(path: &str) -> String {
    path.strip_prefix("adjustments.")
        .unwrap_or(path)
        .to_string()
}

/// Compact, bounded display value of a recipe leaf. `Null` (an absent field)
/// renders as the empty string, which the label presents as "set".
fn display_value(value: &Value) -> String {
    let raw = match value {
        Value::Null => String::new(),
        Value::Bool(true) => "on".to_string(),
        Value::Bool(false) => "off".to_string(),
        Value::Number(number) => format_number(number),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    truncate_chars(&raw, MAX_HISTORY_CHANGE_VALUE_CHARS)
}

fn format_number(number: &serde_json::Number) -> String {
    if let Some(value) = number.as_f64() {
        if value == 0.0 {
            return "0".to_string();
        }
        let mut text = format!("{value}");
        if text.contains('.') {
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        return text;
    }
    number.to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// One rendered change: `parameter from → to` (or `parameter = to` when the
/// previous value was unset).
pub(crate) fn render_change(change: &HistoryChange) -> String {
    if change.from.is_empty() {
        format!("{} = {}", change.parameter, change.to)
    } else {
        format!("{} {} → {}", change.parameter, change.from, change.to)
    }
}

/// Human-readable, clickable history label: `"{ordinal}. {changes} — {time}"`.
///
/// * Structured entries render every change; multiple changes are joined with
///   `; ` (a preset can touch several controls).
/// * Legacy/state entries without structured changes fall back to the stored
///   snapshot name or action label, then to the entry id. Nothing is invented:
///   a state entry stays identifiable, it just has no old→new values.
/// * A malformed structured value is surfaced (`invalid changes`) instead of
///   being hidden; the loud loader rejection normally prevents it from ever
///   reaching the UI.
pub(crate) fn format_history_label(entry: &HistoryEntry, ordinal: usize) -> String {
    let mut label = format!("{ordinal}. ");
    match entry.changes() {
        Ok(changes) if !changes.is_empty() => {
            let rendered: Vec<String> = changes.iter().map(render_change).collect();
            label.push_str(&rendered.join("; "));
        }
        Ok(_) => label.push_str(&fallback_label(entry)),
        Err(_) => label.push_str(&format!("{} (invalid changes)", entry.id)),
    }
    if let Some(recorded_at) = &entry.recorded_at {
        label.push_str(&format!(" — {recorded_at}"));
    }
    label
}

fn fallback_label(entry: &HistoryEntry) -> String {
    if let Some(name) = entry.extras.get("snapshot_name").and_then(Value::as_str) {
        return format!("{} {name}", crate::i18n::Str::SnapshotButton.t());
    }
    if let Some(action) = entry.extras.get("action").and_then(Value::as_str) {
        return action.to_string();
    }
    entry.id.clone()
}

/// Current UTC time as RFC 3339 seconds (`YYYY-MM-DDTHH:MM:SSZ`) for
/// [`HistoryEntry::recorded_at`]. The structured change fields are written
/// alongside it; a failing system clock degrades to the Unix epoch, never to a
/// missing timestamp.
pub(crate) fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format_rfc3339(seconds)
}

impl crate::LuminaApp {
    /// Timestamp for a new history step: the session-only test override when
    /// set, otherwise the real UTC clock.
    pub(crate) fn history_timestamp(&self) -> String {
        self.history_timestamp_override
            .clone()
            .unwrap_or_else(now_rfc3339)
    }

    /// Session-only clock override for deterministic history timestamps in
    /// headless/kittest tests. Never persisted; `None` restores the real clock.
    pub fn set_history_timestamp_override(&mut self, value: Option<String>) {
        self.history_timestamp_override = value;
    }

    /// Formatted labels of the active virtual copy's history steps, newest
    /// first — exactly the strings the History panel renders. Read-only; used
    /// by the panel tests to locate a row without duplicating the formatting.
    pub fn history_labels(&self) -> Vec<String> {
        let Some(document) = &self.document else {
            return Vec::new();
        };
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
        else {
            return Vec::new();
        };
        copy.history
            .iter()
            .enumerate()
            .rev()
            .map(|(index, entry)| format_history_label(entry, index + 1))
            .collect()
    }
}

fn format_rfc3339(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;
    let second = remainder % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch → (year,
/// month, day) in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::HistoryChange;

    fn recipe_with(key: &str, value: f64) -> EditRecipe {
        let mut recipe = EditRecipe::default();
        recipe.adjustments.insert(key.into(), value);
        recipe
    }

    #[test]
    fn recipe_diff_reports_name_and_old_to_new() {
        let before = recipe_with("exposure", 0.0);
        let mut after = before.clone();
        after.adjustments.insert("exposure".into(), 0.5);
        after.adjustments.insert("contrast".into(), -0.2);
        let changes = recipe_changes(&before, &after);
        assert_eq!(
            changes,
            vec![
                HistoryChange {
                    parameter: "contrast".into(),
                    from: "".into(),
                    to: "-0.2".into(),
                },
                HistoryChange {
                    parameter: "exposure".into(),
                    from: "0".into(),
                    to: "0.5".into(),
                },
            ]
        );
    }

    #[test]
    fn recipe_diff_is_empty_for_identical_recipes() {
        let recipe = recipe_with("exposure", 1.25);
        assert!(recipe_changes(&recipe, &recipe).is_empty());
    }

    #[test]
    fn label_renders_parameter_arrow_and_time() {
        let mut entry = HistoryEntry {
            id: "history-1".into(),
            recipe: EditRecipe::default(),
            recorded_at: Some("2026-09-19T12:00:00Z".into()),
            extras: Default::default(),
        };
        entry
            .set_changes(vec![HistoryChange {
                parameter: "exposure".into(),
                from: "0".into(),
                to: "0.5".into(),
            }])
            .unwrap();
        assert_eq!(
            format_history_label(&entry, 1),
            "1. exposure 0 → 0.5 — 2026-09-19T12:00:00Z"
        );
    }

    #[test]
    fn label_falls_back_to_stored_action_and_id() {
        let mut entry = HistoryEntry {
            id: "geometry-2".into(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras: Default::default(),
        };
        entry
            .extras
            .insert("action".into(), Value::String("geometry.rotation".into()));
        assert_eq!(format_history_label(&entry, 2), "2. geometry.rotation");
        entry.extras.clear();
        assert_eq!(format_history_label(&entry, 2), "2. geometry-2");
    }

    #[test]
    fn rfc3339_epoch_and_known_instant() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
    }
}
