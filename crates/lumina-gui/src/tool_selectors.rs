//! The three interactive **tool selectors** that arm a preview drag gesture
//! (GUI-REFACTOR-W2-…: file-size-ratchet extraction from `lib.rs`).
//!
//! These three are one concern and belong together: each names *which* drag
//! gesture the preview should interpret, and each is the `#[default]`-carrying
//! counterpart of a non-default "a gesture is armed" state. They are data-only
//! enums with no behaviour of their own — every decision they drive lives in
//! the spot/mask interaction modules — which is why moving them is a pure
//! relocation: every use path is re-exported from the crate root, so no call
//! site changed.
//!
//! `ZoomMode` deliberately stays in `lib.rs`: it is the preview **zoom**
//! behaviour, not a drag-gesture selector, so folding it in here would group
//! two different concerns rather than extract one.

/// Active interactive masking tool (F-103-N4). `None` means the preview accepts
/// the ordinary click/eyedropper interactions; any other variant arms the
/// preview for a drag gesture that builds a [`super::MaskPrompt`] for the
/// selected mask. The tool only chooses *how* the drag is interpreted;
/// persistence goes through the existing sidecar paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpotTool {
    #[default]
    None,
    Heal,
}

/// Which Spot-Heal strategy a heal gesture runs under. Distinct from
/// [`SpotTool`]: that one says *whether* a spot gesture is armed, this one says
/// *how* the armed gesture heals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpotMode {
    #[default]
    Heuristic,
    Generative,
}

/// Which drag gesture the selected mask is armed for. `None` leaves the preview
/// on its ordinary interactions; the other variants arm a prompt-building drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskTool {
    #[default]
    None,
    Brush,
    LinearGradient,
    Radial,
}
