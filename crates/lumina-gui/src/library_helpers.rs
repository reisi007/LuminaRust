//! Library panel value/decision helpers (pure, stateless).
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 11, file-size ratchet).
//! One responsibility: the value-level helpers the Library panel uses — the
//! virtual-copy color-label read, the compare/survey view mode and its key,
//! clamped raster navigation, and the cross-volume file move. No GUI state,
//! no recipe or sidecar write; every helper is unit-tested headless.

use super::*;

/// Read a color label (`0..=4`, `0` = none) from a virtual copy's `extras`
/// map. Missing, non-numeric or out-of-range values read as `0` (none): the
/// field is a forward-compatible cosmetic annotation, while the strict
/// `0..=4` gate lives on the [`LuminaApp::set_color_label`] write path.
/// Shared by the Library scan and the rating section so both read one path.
pub fn color_label_of(extras: &BTreeMap<String, serde_json::Value>) -> u8 {
    extras
        .get("color_label")
        .and_then(serde_json::Value::as_u64)
        .filter(|&n| n <= 4)
        .unwrap_or(0) as u8
}

/// Library compare/survey view (Welle 3, LR-20 light): `C` shows the
/// full-frame Before image through the existing [`LuminaApp::before_after`]
/// path (compare proxy), `N` jumps to the Library grid (survey proxy over
/// the file-browser entries). Pure function, unit-tested without an
/// [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareMode {
    Compare,
    Survey,
}

pub fn compare_mode_for_key(key: egui::Key) -> Option<CompareMode> {
    match key {
        egui::Key::C => Some(CompareMode::Compare),
        egui::Key::N => Some(CompareMode::Survey),
        _ => None,
    }
}

/// Clamped Library raster navigation (G-09): move `current` by `delta`
/// entries over `count` entries. Clamps at both ends (Lightroom-conform, no
/// wrap); empty listings and out-of-range starts clamp to `0`. Pure
/// function, unit-tested headless.
pub fn library_move_index(current: usize, delta: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let current = current.min(count - 1);
    current.saturating_add_signed(delta).min(count - 1)
}

/// Move one file to `target`, tolerating a cross-filesystem move: `rename`
/// fails with `CrossesDevices` (EXDEV) when source and target live on
/// different volumes, so fall back to copy + remove. Loud on error; a failed
/// source removal cleans up the copied target again (the source still exists,
/// so no data is lost).
pub(crate) fn move_file_cross_volume(source: &Path, target: &Path) -> std::io::Result<()> {
    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(source, target)?;
            if let Err(remove_error) = std::fs::remove_file(source) {
                let _ = std::fs::remove_file(target);
                return Err(remove_error);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}
