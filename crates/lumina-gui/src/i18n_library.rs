//! Library-module UI strings (Welle 3 Filter/Compare/Survey, LRPAR-G15-STACK-15
//! Stack, LRPAR-G09-SORT-09 Sortierung).
//!
//! [`crate::i18n::Str::t`] delegates the library variants here so the central
//! table (`i18n.rs`) stays within its file-size ratchet while staying
//! exhaustive: the single delegating match arm in `i18n.rs` still lists every
//! variant, so a new `Str` variant without a translation is a compile error.
//!
//! `library_text` is only called from that guarded arm; the `_` arm is
//! therefore unreachable by construction (documented loudly, not a fallback).

use super::Str;

/// English text for the library-module [`Str`] variants.
pub(crate) fn library_text(s: Str) -> &'static str {
    match s {
        Str::FilterBar => "Filter (\\)",
        Str::FilterPlaceholder => {
            "Name, rating:0-5, flag:pick/reject, label:red/yellow/green/blue/none"
        }
        Str::FilterShown => "Library filter on (\\) — type to filter, Quick Develop below",
        Str::FilterHidden => "Library filter off",
        Str::QuickDevelop => "Quick Develop",
        Str::CompareModeCompare => "Compare",
        Str::CompareModeSurvey => "Survey",
        Str::CompareOnPattern => "Compare view on ({})",
        Str::CompareOff => "Compare view off",
        Str::SurveyOn => "Survey (N): Library grid",
        Str::QuickDevelopAppliedPattern => "Quick develop applied: {}",
        Str::StackGroupedPattern => "Added to stack {}",
        Str::StackUngrouped => "Removed from stack",
        Str::StackGroup => "Stack",
        Str::StackUngroup => "Unstack",
        // LRPAR-G09-SORT-09: the three sort-mode buttons + the reorder status.
        Str::LibrarySortName => "Name",
        Str::LibrarySortDate => "Capture Date",
        Str::LibrarySortCustom => "Custom",
        Str::LibrarySortReorderedPattern => "Moved {} (custom order)",
        // Only reachable if a non-library variant is passed; the sole caller
        // (`Str::t`) guards with the exact variant set, so this is unreachable
        // by construction and never a silent fallback.
        other => unreachable!("library_text called for non-library string {other:?}"),
    }
}
