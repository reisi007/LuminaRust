//! LRPAR-G09-CULL-25 — GUI slice: Library badges, filter and the explicit
//! adopt action.
//!
//! SOLL: `feature/decisions/LRPAR-G09-CULL-25.md` §1/§3 (assisted culling only,
//! never an automatic rating/flag/label), §5 (source-level persistence) and
//! §6 (Library-only badges + `\`-Leiste filter). The GUI owns **no** culling
//! image logic: the analysis runs in `lumina_cull` (the same Stage-1 heuristic
//! the CLI uses), the read state is `lumina_cull::CullingReadState`, and the
//! write is `lumina_cull::record_culling` + the shared atomic sidecar writer.
//!
//! ## What the adopt action writes (and does not)
//!
//! [`LuminaApp::adopt_culling`] is the one explicit user action of this slice.
//! It analyzes the **currently loaded** frame through
//! [`lumina_cull::analyze_heuristic`] and records the resulting proposal as the
//! source-level `document.culling` section. It writes **only** that section:
//! `rating`, `flag`, `color_label`, the recipe and every virtual copy stay
//! byte-identical (asserted by test). There is no automatic rating, no batch
//! adoption and no silent re-run — a proposal is never invented when the
//! analysis is unavailable.
//!
//! ## Badge / filter states
//!
//! The grid badge and the `cull:` filter expose exactly
//! `keep`/`review`/`reject`/`none`/`stale`:
//!
//! * `none` — no `culling` section (the valid "no proposal" state),
//! * `keep`/`review`/`reject` — a usable proposal,
//! * `stale` — a persisted section that is outdated (`CullingReadState::Stale`)
//!   **or** explicitly unusable (`missing`/`corrupt`): both are surfaced as the
//!   documented "outdated/unusable" state; a proposal is never shown as current
//!   when it is not.
//!
//! The scan-level badge uses the entry's cached `source_status` (a changed
//! source makes its proposal outdated); the authoritative read state is
//! computed by [`LuminaApp::culling_read_state`] against the live decode
//! context.

use log::{info, warn};
use lumina_cull::{
    analyze_heuristic, evaluate_section, heuristic_identity, CullConfig, CullSourceInput,
    CullingReadState,
};
use lumina_sidecar::{
    now_rfc3339_utc, CullProposal, CullingSection, CullingStatus, Resolution, SourceStatus,
    CULLING_SCHEMA_VERSION,
};

use crate::i18n::Str;
use crate::{FileBrowserEntry, GuiError, LuminaApp};

/// Library badge / filter state of one source image (decision §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullBadge {
    /// No `culling` section: the valid "no proposal" state.
    None,
    Keep,
    Review,
    Reject,
    /// Persisted but outdated/unusable — never presented as a current proposal.
    Stale,
}

impl CullBadge {
    /// Derives the scan-level badge from the persisted section and the source
    /// status (a changed source makes its proposal outdated).
    pub(crate) fn from_section(section: Option<&CullingSection>, source_unchanged: bool) -> Self {
        let Some(section) = section else {
            return CullBadge::None;
        };
        if section.status != CullingStatus::Valid || !source_unchanged {
            return CullBadge::Stale;
        }
        match section.proposal {
            CullProposal::Keep => CullBadge::Keep,
            CullProposal::Review => CullBadge::Review,
            CullProposal::RejectCandidate => CullBadge::Reject,
        }
    }

    /// Derives the badge from the authoritative read state.
    pub(crate) fn from_read_state(state: &CullingReadState) -> Self {
        match state {
            CullingReadState::NoProposal => CullBadge::None,
            CullingReadState::Valid(section) => CullBadge::from_section(Some(section), true),
            // `stale` and explicitly unusable sections share the documented
            // "outdated/unusable" state; neither is shown as current.
            CullingReadState::Stale { .. } | CullingReadState::Unusable { .. } => CullBadge::Stale,
        }
    }

    /// Stable lowercase filter token (`cull:<token>`).
    pub(crate) fn token(self) -> &'static str {
        match self {
            CullBadge::None => "none",
            CullBadge::Keep => "keep",
            CullBadge::Review => "review",
            CullBadge::Reject => "reject",
            CullBadge::Stale => "stale",
        }
    }

    /// Parses a `cull:` filter value; an unknown value matches nothing (a
    /// visible empty grid, never a silent pass-through).
    pub(crate) fn from_token(token: &str) -> Option<Self> {
        match token {
            "none" => Some(CullBadge::None),
            "keep" => Some(CullBadge::Keep),
            "review" => Some(CullBadge::Review),
            "reject" => Some(CullBadge::Reject),
            "stale" => Some(CullBadge::Stale),
            _ => None,
        }
    }

    /// Visible badge text (chip in the grid cell), `None` for a clean cell.
    pub(crate) fn badge_text(self) -> Option<&'static str> {
        match self {
            CullBadge::None => None,
            CullBadge::Keep => Some(Str::CullingKeep.t()),
            CullBadge::Review => Some(Str::CullingReview.t()),
            CullBadge::Reject => Some(Str::CullingReject.t()),
            CullBadge::Stale => Some(Str::CullingStale.t()),
        }
    }
}

/// One `cull:` filter token against a scan-level badge. `None` when the token
/// is not a `cull:` predicate (the caller then evaluates the other prefixes).
pub(crate) fn cull_filter_token_matches(token_lower: &str, badge: CullBadge) -> Option<bool> {
    let value = token_lower.strip_prefix("cull:")?.trim();
    Some(CullBadge::from_token(value).is_some_and(|want| want == badge))
}

/// Outcome of an explicit adopt/clear action (status line + tests).
#[derive(Debug, Clone, PartialEq)]
pub struct CullAdoptReport {
    pub path: String,
    pub badge: CullBadge,
    pub score: f32,
}

impl LuminaApp {
    /// Scan-level culling badge of the active entry (testable model getter,
    /// decision §6 testability rule), or `None` when nothing is loaded.
    pub fn culling_badge(&self) -> Option<CullBadge> {
        self.entries
            .iter()
            .find(|entry| entry.path.display().to_string() == self.path)
            .map(|entry| entry.cull_badge)
    }

    /// Authoritative read state of the loaded image against the live decode
    /// context. Missing prerequisites degrade to the visible `NoProposal`
    /// state — never an invented recommendation.
    pub fn culling_read_state(&mut self) -> CullingReadState {
        let Some(current) = self.culling_current_identity() else {
            return CullingReadState::NoProposal;
        };
        match &self.document {
            Some(document) => evaluate_section(document.culling.as_ref(), &current),
            None => CullingReadState::NoProposal,
        }
    }

    /// Live culling identity of the loaded image (source content hash + decode
    /// context + geometry + analyzer + analysis resolution + preprocessing).
    fn culling_current_identity(&mut self) -> Option<lumina_sidecar::CullingIdentity> {
        let source_hash = self.resolved_source_hash();
        let frame = self.original.as_ref()?.clone();
        let byte_length = self.source_bytes.as_ref()?.len() as u64;
        let decode_fingerprint = self.document.as_ref()?.source.decode_fingerprint.clone();
        let analysis =
            lumina_core::downscale_bilinear(&frame, CullConfig::default().analysis_max_width)
                .ok()?;
        Some(heuristic_identity(
            lumina_sidecar::SourceFingerprint {
                content_hash: source_hash,
                byte_length,
                extras: Default::default(),
            },
            decode_fingerprint,
            lumina_sidecar::GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: self.raw_orientation,
                pixel_aspect_ratio: 1.0,
                extras: Default::default(),
            },
            Resolution {
                width: analysis.width,
                height: analysis.height,
                extras: Default::default(),
            },
        ))
    }

    /// Explicit adopt action (decision §1/§3): analyzes the loaded frame with
    /// the Stage-1 heuristic and records the proposal as the source-level
    /// `document.culling` section. Writes **only** that section — `rating`,
    /// `flag`, `color_label`, the recipe and the virtual copies are untouched.
    pub fn adopt_culling(&mut self) -> Result<CullAdoptReport, GuiError> {
        let frame = self
            .original
            .clone()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        self.ensure_document_loaded()?;
        let identity = self
            .culling_current_identity()
            .ok_or_else(|| GuiError::Io(Str::CullingNoIdentity.t().to_string()))?;
        let sidecar_source = &self
            .document
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::CullingNoIdentity.t().to_string()))?
            .source;
        if identity.source.content_hash != sidecar_source.content_hash
            || identity.source.byte_length != sidecar_source.byte_length
        {
            return Err(GuiError::Io(format!(
                "source identity conflict for `{}`: live source hash/byte length does not \
                 match SidecarDocument.source; refusing culling write",
                self.path
            )));
        }
        let config = CullConfig::default();
        let analysis = analyze_heuristic(
            &CullSourceInput {
                frame: &frame,
                iso: None,
            },
            &config,
        )
        .map_err(|error| GuiError::Io(error.to_string()))?;
        let section = CullingSection {
            version: CULLING_SCHEMA_VERSION,
            proposal: analysis.proposal(),
            score: analysis.score(),
            reasons: analysis.reasons().to_vec(),
            identity,
            created_at: now_rfc3339_utc(),
            status: CullingStatus::Valid,
            error: None,
            extras: Default::default(),
        };
        let badge = CullBadge::from_section(Some(&section), true);
        let score = section.score;
        let path = self.path.clone();
        let mut document = self
            .document
            .take()
            .ok_or_else(|| GuiError::Io(Str::CullingNoIdentity.t().to_string()))?;
        if let Err(error) = lumina_cull::record_culling(&mut document, section) {
            self.document = Some(document);
            return Err(GuiError::Io(error.to_string()));
        }
        // SIDECAR-REBASE-1: same CAS writer, but only the `culling` section is
        // written, so a concurrent change is rebased by applying this section
        // onto the current file (every other foreign field survives).
        match self.save_section_with_rebase(
            &path,
            document,
            crate::sidecar_rebase::RebaseSection::Culling,
        ) {
            Ok(_revision) => {
                self.refresh_entry(std::path::Path::new(&path));
                self.status = Str::CullingAdoptedPattern.format_arg(&path);
                info!(
                    "culling proposal adopted for `{path}` (proposal={:?} score={score:.3}, \
                     recipe/rating/flag/label untouched)",
                    analysis.proposal()
                );
                Ok(CullAdoptReport { path, badge, score })
            }
            Err(error) => Err(GuiError::Sidecar(error)),
        }
    }

    /// Explicit clear: removes the source-level proposal ("no proposal"), never
    /// inventing a replacement. Recipe/rating/flag/label untouched.
    pub fn clear_culling_proposal(&mut self) -> Result<(), GuiError> {
        self.ensure_document_loaded()?;
        let path = self.path.clone();
        let mut document = self
            .document
            .take()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?;
        if document.culling.is_none() {
            self.document = Some(document);
            self.status = Str::CullingNoProposal.t().into();
            return Ok(());
        }
        lumina_cull::clear_culling(&mut document);
        // SIDECAR-REBASE-1: explicit clear of the `culling` section through the
        // rebase helper (foreign changes to every other field survive).
        match self.save_section_with_rebase(
            &path,
            document,
            crate::sidecar_rebase::RebaseSection::Culling,
        ) {
            Ok(_revision) => {
                self.refresh_entry(std::path::Path::new(&path));
                self.status = Str::CullingClearedPattern.format_arg(&path);
                info!("culling proposal cleared for `{path}` (no proposal)");
                Ok(())
            }
            Err(error) => Err(GuiError::Sidecar(error)),
        }
    }

    /// Library metadata-panel section: the read state, the proposal with its
    /// reason codes and the explicit adopt/clear actions. Library-only
    /// (decision §6: no Develop/Export changes).
    pub(crate) fn draw_culling_section(&mut self, ui: &mut crate::egui::Ui) {
        // Collapsed by default like every other Library sub-section: the
        // panel's 320 px default width only carries the narrow headers.
        ui.collapsing(Str::CullingSection.t(), |ui| {
            if self.path.is_empty() || self.document.is_none() {
                ui.label(Str::CullingNoProposal.t());
                return;
            }
            let state = self.culling_read_state();
            let badge = CullBadge::from_read_state(&state);
            let (line, detail) = match &state {
                CullingReadState::NoProposal => {
                    (Str::CullingNoProposal.t().to_string(), String::new())
                }
                CullingReadState::Valid(section) => (
                    Str::CullingProposalPattern.format_arg(&format!(
                        "{} {:.2}",
                        badge.token(),
                        section.score
                    )),
                    if section.reasons.is_empty() {
                        String::new()
                    } else {
                        Str::CullingReasonsPattern.format_arg(&section.reasons.join(", "))
                    },
                ),
                CullingReadState::Stale {
                    section,
                    mismatches,
                } => (
                    Str::CullingStalePattern.format_arg(&format!("{:.2}", section.score)),
                    mismatches
                        .iter()
                        .map(|mismatch| format!("{mismatch:?}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                CullingReadState::Unusable { section } => (
                    Str::CullingStalePattern.format_arg(&format!("{:?}", section.status)),
                    section.error.clone().unwrap_or_default(),
                ),
            };
            ui.label(line);
            if !detail.is_empty() {
                ui.label(detail);
            }
            ui.horizontal(|ui| {
                if ui.button(Str::CullingAnalyze.t()).clicked() {
                    match self.adopt_culling() {
                        Ok(_) => {}
                        Err(error) => self.show_error(error),
                    }
                }
                if ui.button(Str::CullingClear.t()).clicked() {
                    match self.clear_culling_proposal() {
                        Ok(()) => {}
                        Err(error) => self.show_error(error),
                    }
                }
            });
            ui.add(
                crate::egui::Label::new(Str::CullingFilterHint.t())
                    .wrap()
                    .halign(crate::egui::Align::LEFT),
            );
        });
    }
}

/// Paints the culling badge chip at the top-left of a Library grid cell.
/// Visually distinct from the (bottom-left) rating/flag/label badge so the
/// assisted proposal can never be mistaken for a manual rating.
pub(crate) fn paint_cull_badge(ui: &crate::egui::Ui, rect: crate::egui::Rect, badge: CullBadge) {
    let Some(text) = badge.badge_text() else {
        return;
    };
    let color = cull_badge_color(badge);
    // Top-RIGHT: the path badge owns the top-left corner and the manual
    // rating/flag/label badge the bottom edge, so no two chips can overlap.
    let size = crate::egui::vec2(text.len() as f32 * 6.6 + 10.0, 15.0);
    let badge_pos = crate::egui::pos2(rect.right() - size.x - 4.0, rect.top() + 4.0);
    ui.painter().rect_filled(
        crate::egui::Rect::from_min_size(badge_pos, size),
        2.0,
        color,
    );
    ui.painter().text(
        badge_pos + crate::egui::vec2(5.0, 1.5),
        crate::egui::Align2::LEFT_TOP,
        text,
        crate::egui::FontId::monospace(10.0),
        crate::egui::Color32::BLACK,
    );
}

/// Chip colour per badge state (pure, unit-tested).
pub(crate) fn cull_badge_color(badge: CullBadge) -> crate::egui::Color32 {
    match badge {
        CullBadge::None => crate::egui::Color32::TRANSPARENT,
        CullBadge::Keep => crate::egui::Color32::from_rgb(0x8D, 0xC7, 0x6A),
        CullBadge::Review => crate::egui::Color32::from_rgb(0xE6, 0xB4, 0x32),
        CullBadge::Reject => crate::egui::Color32::from_rgb(0xE0, 0x6C, 0x3C),
        CullBadge::Stale => crate::egui::Color32::from_rgb(0x9A, 0x9A, 0x9A),
    }
}

/// Scan-time badge of one entry: the persisted section plus the cached source
/// status. Pure; used by [`FileBrowserEntry`] construction.
pub(crate) fn scan_cull_badge(
    section: Option<&CullingSection>,
    source_status: SourceStatus,
) -> CullBadge {
    CullBadge::from_section(section, matches!(source_status, SourceStatus::Unchanged))
}

/// Per-entry badge accessor used by the grid painter.
pub(crate) fn entry_cull_badge(entry: &FileBrowserEntry) -> CullBadge {
    entry.cull_badge
}

/// Warns loudly once per unsupported `cull:` value (visible empty grid).
pub(crate) fn warn_unknown_cull_token(token: &str) {
    warn!("unknown cull filter value `{token}` — matching nothing (expected keep|review|reject|none|stale)");
}

#[cfg(test)]
#[path = "cull_gui_tests.rs"]
mod tests;
