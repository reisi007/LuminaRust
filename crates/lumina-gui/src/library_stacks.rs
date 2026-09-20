//! LRPAR-G15-STACK-15: source-level image stacks in the Library Grid and
//! Filmstrip.
//!
//! A stack groups ≥2 source images of the same folder into one collapsible
//! unit. Membership lives Sidecar-first in every member's sidecar
//! (`lumina_sidecar::StackMembership`, `SidecarDocument::stack`); this module
//! owns the GUI projection: which entries are hidden behind a collapsed cover,
//! selecting a stack as a unit, the collapse/expand/create/unstack mutations
//! (atomic CAS writes through `sidecar_rebase`) and the painted stack badge.
//!
//! Loud by construction: every failure is per image (`error!` + `Result`), no
//! silent fallback, no sidecar is invented for an unstacked/unedited image.

use super::*;
use log::{error, info};
use std::path::Path;

/// R5-STACK-3: colour of the stack membership bracket painted around a stacked
/// grid/filmstrip cell (distinct from the selection stroke). Display-only.
const STACK_GROUP_FRAME: egui::Color32 = egui::Color32::from_rgb(0, 190, 235);

/// The Library surface a stack badge is painted on. R5-STACK-2: the grid and
/// the filmstrip are drawn in the **same** frame and both show the same image,
/// so a badge id derived from `thumb_key` alone collided — egui keys widget
/// interaction by id, so the second registration made one surface's badge
/// unclickable. The surface is part of the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackBadgeSurface {
    Grid,
    Filmstrip,
}

/// Stable egui id of the painted stack badge for one cell, so the headless
/// tests can locate and click it (F-100 clickability). Unique per surface
/// (R5-STACK-2).
pub(crate) fn stack_badge_id(surface: StackBadgeSurface, thumb_key: &str) -> egui::Id {
    egui::Id::new(("lumina-stack-badge", surface as u8, thumb_key))
}

impl LuminaApp {
    // ---- read helpers ----------------------------------------------------

    /// Entry index of a display-string path, if the current listing holds it.
    fn stack_entry_index(&self, path: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.path.display().to_string() == path)
    }

    /// The persisted stack section of the entry at `path`, if stacked.
    pub fn stack_for_path(&self, path: &str) -> Option<lumina_sidecar::StackMembership> {
        self.stack_entry_index(path)
            .and_then(|index| self.entries[index].stack.clone())
    }

    /// Effective collapse state of the stack `entry` belongs to: the cover's
    /// sidecar value wins (last-writer-wins is read from the cover), falling
    /// back to the entry's own value only when the cover is not in the listing.
    fn stack_collapsed_for_entry(&self, entry: &FileBrowserEntry) -> bool {
        let Some(stack) = entry.stack.as_ref() else {
            return false;
        };
        let dir = entry.path.parent().unwrap_or_else(|| Path::new("."));
        let cover_path = dir.join(&stack.cover);
        self.entries
            .iter()
            .find(|candidate| candidate.path == cover_path)
            .and_then(|cover| cover.stack.as_ref())
            .map_or(stack.collapsed, |cover| cover.collapsed)
    }

    /// True when `index` is a non-cover member hidden behind a collapsed stack.
    /// A stack is only collapsed to its cover when that cover is actually
    /// listed — a stack whose cover is missing keeps showing its present
    /// members instead of vanishing silently.
    pub(crate) fn stack_entry_hidden(&self, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        let Some(stack) = entry.stack.as_ref() else {
            return false;
        };
        if stack.is_cover(&entry.name) {
            return false;
        }
        let dir = entry.path.parent().unwrap_or_else(|| Path::new("."));
        let cover_path = dir.join(&stack.cover);
        let cover_present = self
            .entries
            .iter()
            .any(|candidate| candidate.path == cover_path);
        cover_present && self.stack_collapsed_for_entry(entry)
    }

    /// Filters a display-order index list down to the visible stack entries
    /// (a collapsed stack contributes only its cover).
    pub(crate) fn collapse_stacked_indices(&self, indices: Vec<usize>) -> Vec<usize> {
        indices
            .into_iter()
            .filter(|&index| !self.stack_entry_hidden(index))
            .collect()
    }

    /// Display paths of the members of `index`'s stack that are present in the
    /// current listing (the cover is always included when listed). A single
    /// non-stacked entry yields its own path.
    pub(crate) fn stack_present_paths(&self, index: usize) -> Vec<String> {
        let Some(entry) = self.entries.get(index) else {
            return Vec::new();
        };
        let Some(stack) = entry.stack.as_ref() else {
            return vec![entry.path.display().to_string()];
        };
        let dir = entry.path.parent().unwrap_or_else(|| Path::new("."));
        stack
            .members
            .iter()
            .filter_map(|member| {
                let path = dir.join(member);
                self.entries
                    .iter()
                    .find(|candidate| candidate.path == path)
                    .map(|candidate| candidate.path.display().to_string())
            })
            .collect()
    }

    /// Adds every present member of each selected stack, so Sync/Batch/Previous
    /// act on the stack as a unit.
    pub(crate) fn expand_selection_to_stacks(
        &self,
        selection: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        let mut out = selection.clone();
        for path in selection {
            if let Some(index) = self.stack_entry_index(path) {
                for member in self.stack_present_paths(index) {
                    out.insert(member);
                }
            }
        }
        out
    }

    /// Adjusts the selection for a click: a non-toggle click selects the whole
    /// stack, toggling the stack off removes every member so the unit can never
    /// be half-selected.
    pub(crate) fn apply_stack_selection(
        &self,
        next: BTreeSet<String>,
        clicked: &str,
        toggle: bool,
    ) -> BTreeSet<String> {
        let Some(index) = self.stack_entry_index(clicked) else {
            return next;
        };
        if self.entries[index].stack.is_none() {
            return next;
        }
        if toggle && !next.contains(clicked) {
            let mut out = next;
            for member in self.stack_present_paths(index) {
                out.remove(&member);
            }
            out
        } else {
            self.expand_selection_to_stacks(&next)
        }
    }

    /// Collapse state of the active image's stack, or `None` when not stacked.
    pub fn active_stack_collapsed(&self) -> Option<bool> {
        let index = self.stack_entry_index(self.path.trim())?;
        let entry = self.entries.get(index)?;
        entry.stack.as_ref()?;
        Some(self.stack_collapsed_for_entry(entry))
    }

    /// Short human-readable stack state of the active image for the panel row.
    pub fn stack_status_label(&self) -> String {
        let Some(index) = self.stack_entry_index(self.path.trim()) else {
            return "Not stacked".into();
        };
        let entry = &self.entries[index];
        let Some(stack) = entry.stack.as_ref() else {
            return "Not stacked".into();
        };
        let state = if self.stack_collapsed_for_entry(entry) {
            "collapsed"
        } else {
            "expanded"
        };
        format!("{} image(s), {state}", stack.members.len())
    }

    // ---- mutations -------------------------------------------------------

    /// Creates a stack from the current selection (fallback: the loaded image).
    /// All targets must be distinct images of the **same folder** with an
    /// existing sidecar; a violated precondition is a loud error that writes
    /// nothing. A failure while writing one member is reported loudly (SOLL §6,
    /// `error!` per image) and never aborts the remaining members: the members
    /// that could be written carry the section, the collected error names the
    /// failed ones.
    pub fn create_stack_from_selection(&mut self) -> Result<(), String> {
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() && !self.path.trim().is_empty() {
            targets.push(self.path.trim().to_string());
        }
        targets.sort();
        targets.dedup();
        if targets.len() < 2 {
            return Err("A stack needs at least two selected images".into());
        }
        let folder = Path::new(&targets[0]).parent().map(Path::to_path_buf);
        let same_folder = folder.is_some()
            && targets
                .iter()
                .all(|target| Path::new(target).parent().map(Path::to_path_buf) == folder);
        if !same_folder {
            return Err("Stack members must all be in the same folder".into());
        }
        let mut names = Vec::with_capacity(targets.len());
        for target in &targets {
            let sidecar = lumina_sidecar::sidecar_path_for(Path::new(target));
            if !sidecar.is_file() {
                return Err(format!(
                    "No sidecar for `{target}`; open or edit it before stacking"
                ));
            }
            let name = Path::new(target)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .ok_or_else(|| format!("invalid file name `{target}`"))?;
            names.push(name);
        }
        let active_name = self
            .entries
            .iter()
            .find(|entry| entry.path.display().to_string() == self.path.trim())
            .map(|entry| entry.name.clone());
        let cover = names
            .iter()
            .find(|name| Some(*name) == active_name.as_ref())
            .cloned()
            .unwrap_or_else(|| names[0].clone());
        let stack_id = lumina_sidecar::StackMembership::stack_id_for_members(&names);
        let section = lumina_sidecar::StackMembership::new(stack_id.clone(), cover, names)
            .map_err(|error| error.to_string())?;
        let mut failures: Vec<String> = Vec::new();
        for target in &targets {
            match Self::set_stack_on_path(Path::new(target), Some(section.clone())) {
                Ok(_) => {
                    info!("stack {stack_id}: `{target}` joined");
                    self.refresh_entry(Path::new(target));
                    self.reload_loaded_document_if(Path::new(target));
                }
                Err(message) => {
                    // Per-image failure: loud, never abort the remaining
                    // members (sync/batch pattern, SOLL §6).
                    error!("stack create failed for {target}: {message}");
                    failures.push(format!("{target}: {message}"));
                }
            }
        }
        if failures.is_empty() {
            self.status = Str::StackGroupedPattern.format_arg(&stack_id);
            Ok(())
        } else {
            Err(format!(
                "Stack create failed for {} image(s): {}",
                failures.len(),
                failures.join("; ")
            ))
        }
    }

    /// Dissolves the stack(s) of the current selection (fallback: the loaded
    /// image) by clearing `stack` in every present member's sidecar.
    pub fn unstack_selection(&mut self) -> Result<(), String> {
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() && !self.path.trim().is_empty() {
            targets.push(self.path.trim().to_string());
        }
        if targets.is_empty() {
            self.status = Str::NoImagesSelected.t().into();
            return Err("No images selected".into());
        }
        let mut dissolved: Vec<String> = Vec::new();
        for target in &targets {
            match self.stack_entry_index(target) {
                Some(index) => dissolved.extend(self.stack_present_paths(index)),
                None => dissolved.push(target.clone()),
            }
        }
        dissolved.sort();
        dissolved.dedup();
        let mut failures: Vec<String> = Vec::new();
        for target in &dissolved {
            match Self::set_stack_on_path(Path::new(target), None) {
                Ok(true) => {
                    info!("stack: `{target}` unstacked");
                    self.refresh_entry(Path::new(target));
                    self.reload_loaded_document_if(Path::new(target));
                }
                Ok(false) => {}
                Err(message) => {
                    // Per-image failure: loud, never abort the remaining
                    // members (sync/batch pattern, SOLL §6).
                    error!("unstack failed for {target}: {message}");
                    failures.push(format!("{target}: {message}"));
                }
            }
        }
        if failures.is_empty() {
            self.status = Str::StackUngrouped.t().into();
            Ok(())
        } else {
            Err(format!(
                "Unstack failed for {} image(s): {}",
                failures.len(),
                failures.join("; ")
            ))
        }
    }

    /// Toggles the collapse state of the active image's stack and persists it in
    /// every present member (last-writer-wins on the cover).
    pub fn toggle_stack_collapse(&mut self) -> Result<(), String> {
        let path = self.path.trim().to_string();
        self.toggle_stack_collapse_for_path(&path)
    }

    /// [`Self::toggle_stack_collapse`] for an explicit path (used by the
    /// clickable badge of any cell, not only the loaded one).
    pub fn toggle_stack_collapse_for_path(&mut self, path: &str) -> Result<(), String> {
        let Some(index) = self.stack_entry_index(path) else {
            return Err("No image loaded".into());
        };
        let Some(stack) = self.entries[index].stack.clone() else {
            return Err("The active image is not in a stack".into());
        };
        let collapsed = !self.stack_collapsed_for_entry(&self.entries[index]);
        let updated = stack.with_collapsed(collapsed);
        let dir = self.entries[index]
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let mut failures: Vec<String> = Vec::new();
        for member in updated.members.clone() {
            let member_path = dir.join(&member);
            if !self.entries.iter().any(|entry| entry.path == member_path) {
                continue;
            }
            match Self::set_stack_on_path(&member_path, Some(updated.clone())) {
                Ok(_) => {
                    self.refresh_entry(&member_path);
                    self.reload_loaded_document_if(&member_path);
                }
                Err(message) => {
                    // Per-image failure: loud, never abort the remaining
                    // members (sync/batch pattern, SOLL §6).
                    error!(
                        "stack collapse failed for {}: {message}",
                        member_path.display()
                    );
                    failures.push(format!("{}: {message}", member_path.display()));
                }
            }
        }
        if failures.is_empty() {
            info!(
                "stack {}: {}",
                updated.stack_id,
                if collapsed { "collapsed" } else { "expanded" }
            );
            self.status = format!(
                "Stack {} ({} images)",
                if collapsed { "collapsed" } else { "expanded" },
                updated.members.len()
            );
            Ok(())
        } else {
            Err(format!(
                "Stack collapse failed for {} image(s): {}",
                failures.len(),
                failures.join("; ")
            ))
        }
    }

    /// One atomic stack write for `path`: `None` clears the membership. A
    /// missing sidecar or an invalid section is a loud error; the original
    /// bytes are never read or written.
    pub(crate) fn set_stack_on_path(
        path: &Path,
        stack: Option<lumina_sidecar::StackMembership>,
    ) -> Result<bool, String> {
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        let mut document = lumina_sidecar::load_sidecar(&sidecar_path)
            .map_err(|error| format!("{}: {error}", sidecar_path.display()))?;
        if document.stack == stack {
            return Ok(false);
        }
        let base = document.clone();
        document.stack = stack;
        document.validate().map_err(|error| error.to_string())?;
        let expected =
            lumina_sidecar::document_revision(&base).map_err(|error| error.to_string())?;
        sidecar_rebase::save_rebased_unit(&sidecar_path, &base, &document, Some(&expected))
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// Re-read the loaded document (and re-anchor its CAS revision) when a
    /// stack write touched the open image, so a later recipe save cannot erase
    /// the fresh `stack` section.
    fn reload_loaded_document_if(&mut self, path: &Path) {
        if self.path.trim() == path.display().to_string() {
            self.sidecar_revision = None;
            self.reload_document_for_batch_target(path);
        }
    }

    // ---- painting --------------------------------------------------------

    /// Paints the stack badge over a grid/filmstrip cell. Returns `true` when
    /// the badge was clicked, so the caller toggles the collapse instead of
    /// running the plain cell click. No badge is painted for a non-stacked
    /// entry (zero visual change for existing listings).
    pub(crate) fn paint_stack_badge(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        entry: &FileBrowserEntry,
        surface: StackBadgeSurface,
    ) -> bool {
        let Some(stack) = entry.stack.as_ref() else {
            return false;
        };
        let collapsed = self.stack_collapsed_for_entry(entry);
        let symbol = if collapsed { "⊞" } else { "⊟" };
        let count = stack.members.len();
        let index = stack
            .members
            .iter()
            .position(|member| member == &entry.name)
            .map(|position| position + 1)
            .unwrap_or(1);
        // R5-STACK-3 (User-Entscheid 2026-09-20): a visible membership bracket
        // around every stacked cell plus the "index/count" position makes the
        // grouping recognizable without selecting the stack. Collapsed keeps
        // the clear stack symbol. Pure display — never recipe/sidecar.
        ui.painter().rect_stroke(
            rect.expand(1.0),
            2.0,
            egui::Stroke::new(1.5_f32, STACK_GROUP_FRAME),
            egui::StrokeKind::Outside,
        );
        let label = format!("{symbol} {index}/{count}");
        // Top-centre keeps clear of the folder badge (top-left), the
        // assisted-culling badge (top-right) and the rating/flag/color label
        // badge (bottom edge), so no two chips overlap.
        let badge = egui::Rect::from_min_size(
            egui::pos2(rect.center().x - 26.0, rect.top() + 2.0),
            egui::vec2(52.0, 16.0),
        );
        let response = ui.interact(
            badge,
            stack_badge_id(surface, &entry.thumb_key),
            egui::Sense::click(),
        );
        ui.painter().rect_filled(
            badge,
            2.0,
            egui::Color32::from_rgba_unmultiplied(20, 20, 20, 180),
        );
        ui.painter().text(
            badge.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::monospace(11.0),
            egui::Color32::WHITE,
        );
        response.clicked()
    }
}
