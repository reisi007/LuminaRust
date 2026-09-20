//! Masking UI strings (G-03 masking parity + LRPAR-G03-MASKGROUP-03 group panel).
//!
//! [`crate::i18n::Str::t`] delegates the masking variants here so the central
//! table (`i18n.rs`) stays within its file-size ratchet while staying
//! exhaustive: the delegating match arms in `i18n.rs` still list every variant,
//! so a new `Str` variant without a translation is a compile error.
//!
//! `masking_text` is only called from those guarded arms; the `_` arm is
//! therefore unreachable by construction (documented loudly, not a fallback).

use super::Str;

/// English text for the masking [`Str`] variants.
pub(crate) fn masking_text(s: Str) -> &'static str {
    match s {
        Str::SpotOverlayHint => {
            "Overlay/pins follow the global Tool overlay + Edit pins modes (Masking section)"
        }
        Str::ShowOverlay => "Show overlay",
        Str::OverlayColor => "Overlay color",
        Str::MaskEye => "Eye",
        Str::AiSelectLabel => "AI select",
        Str::AiSubject => "Subject",
        Str::AiSky => "Sky",
        Str::AiBackground => "Background",
        Str::AiObjects => "Objects",
        Str::AiPeople => "People",
        Str::DetailLabel => "Detail (optional)",
        Str::AddAiMask => "Add AI mask",
        Str::LuminanceRange => "Luminance range",
        Str::ColorRange => "Color range",
        Str::AddRange => "Add range",
        Str::CombineLabel => "Combine",
        Str::CombineAdd => "Add",
        Str::CombineSubtract => "Subtract",
        // LRPAR-G03-MASKGROUP-03: the deep, independent copy is labelled "Copy";
        // the group variant below is the pointer-based "Duplicate".
        Str::DuplicateMask => "Copy",
        Str::OtherMask => "Other",
        Str::MaskStatusLabel => "Status",
        Str::SoftproofOn => "Softproof preview on (S, full gamut simulation follows in G-10)",
        Str::SoftproofOff => "Softproof preview off",
        Str::SoftproofToggle => "Softproof preview (S)",
        Str::AutoEndpointAppliedPattern => "Auto {} applied (Shift+double-click)",
        Str::MaskingPreviewPattern => "Masking preview: {} (Alt held)",
        Str::OverlayNever => "Never",
        Str::OverlayModeSetPattern => "Tool overlay: {}",
        Str::PinVisibilityLabel => "Edit pins",
        Str::SoloMode => "Solo mode (one section open)",
        Str::SoloModeOn => "Solo mode on (opening a section closes the others)",
        Str::SoloModeOff => "Solo mode off",
        Str::AllPanelsHiddenOn => "All panels hidden (Shift+Tab to show)",
        Str::AllPanelsHiddenOff => "All panels shown",
        // LRPAR-G03-MASKGROUP-03: group panel.
        Str::MaskGroupsLabel => "Mask groups",
        Str::GroupMembersLabel => "Members",
        Str::GroupSelected => "Group selected",
        Str::DuplicateGroup => "Duplicate (group)",
        Str::Ungroup => "Ungroup",
        Str::GroupActive => "Active",
        Str::GroupFeatherOffset => "Feather offset",
        Str::GroupDensityOffset => "Density offset",
        Str::GroupApplyOffsets => "Apply offsets",
        Str::GroupOffsetsApplied => "Applied offsets to {} layer(s)",
        Str::MaskGroupedPattern => "Grouped {} mask(s)",
        Str::MaskGroupDissolved => "Group removed (masks kept)",
        Str::MaskDeletedPattern => "Mask deleted ({} frozen)",
        Str::DeleteMaskButton => "Delete mask",
        Str::MaskGroupNotFound => "Mask group not found",
        Str::SelectMasksToGroup => "Select at least one mask to group",
        // Only reachable if a non-masking variant is passed; the sole caller
        // (`Str::t`) guards with the exact variant set, so this is unreachable
        // by construction and never a silent fallback.
        other => unreachable!("masking_text called for non-masking string {other:?}"),
    }
}
