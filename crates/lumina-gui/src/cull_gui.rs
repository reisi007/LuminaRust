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
    now_rfc3339_utc, save_sidecar_if_unchanged, sidecar_path_for, CullProposal, CullingSection,
    CullingStatus, Resolution, SourceStatus, CULLING_SCHEMA_VERSION,
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
        let byte_length = self.document.as_ref()?.source.byte_length;
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
        let config = CullConfig::default();
        let analysis = analyze_heuristic(
            &CullSourceInput {
                frame: &frame,
                iso: None,
            },
            &config,
        )
        .map_err(|error| GuiError::Io(error.to_string()))?;
        let identity = self
            .culling_current_identity()
            .ok_or_else(|| GuiError::Io(Str::CullingNoIdentity.t().to_string()))?;
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
        // The write goes through the same compare-and-swap atomic writer as
        // every other sidecar edit; only the `culling` section is added.
        let expected = self.sidecar_revision.clone();
        let sidecar_path = sidecar_path_for(std::path::Path::new(&path));
        match save_sidecar_if_unchanged(&sidecar_path, &document, expected.as_deref()) {
            Ok(revision) => {
                self.sidecar_revision = Some(revision);
                self.document = Some(document);
                self.refresh_entry(std::path::Path::new(&path));
                self.status = Str::CullingAdoptedPattern.format_arg(&path);
                info!(
                    "culling proposal adopted for `{path}` (proposal={:?} score={score:.3}, \
                     recipe/rating/flag/label untouched)",
                    analysis.proposal()
                );
                Ok(CullAdoptReport { path, badge, score })
            }
            Err(error) => {
                self.document = Some(document);
                Err(GuiError::Sidecar(error))
            }
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
        let expected = self.sidecar_revision.clone();
        let sidecar_path = sidecar_path_for(std::path::Path::new(&path));
        match save_sidecar_if_unchanged(&sidecar_path, &document, expected.as_deref()) {
            Ok(revision) => {
                self.sidecar_revision = Some(revision);
                self.document = Some(document);
                self.refresh_entry(std::path::Path::new(&path));
                self.status = Str::CullingClearedPattern.format_arg(&path);
                info!("culling proposal cleared for `{path}` (no proposal)");
                Ok(())
            }
            Err(error) => {
                self.document = Some(document);
                Err(GuiError::Sidecar(error))
            }
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
mod tests {
    use super::*;
    use lumina_core::{ImageFileFormat, ImageFrame};
    use lumina_sidecar::load_sidecar;
    use std::path::Path;

    fn new_app() -> LuminaApp {
        LuminaApp::new(crate::egui::Context::default())
    }

    fn save_png(path: &Path, variant: u8) {
        let pixels: Vec<u8> = (0..64 * 48)
            .flat_map(|i| {
                let value = ((i as u32 * 7 + variant as u32 * 13) % 200) as u8;
                [value, value.wrapping_add(20), value.wrapping_sub(10), 255]
            })
            .collect();
        let png = ImageFrame::new(64, 48, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    fn open_and_decode(app: &mut LuminaApp, path: &Path) {
        app.open_file(path.display().to_string());
        for _ in 0..2000 {
            app.poll_decode();
            if app.original.is_some() || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.error().is_none(), "decode failed: {:?}", app.error());
    }

    #[test]
    fn badge_tokens_roundtrip_and_unknown_matches_nothing() {
        for badge in [
            CullBadge::None,
            CullBadge::Keep,
            CullBadge::Review,
            CullBadge::Reject,
            CullBadge::Stale,
        ] {
            assert_eq!(CullBadge::from_token(badge.token()), Some(badge));
            assert_eq!(
                cull_filter_token_matches(&format!("cull:{}", badge.token()), badge),
                Some(true)
            );
            let _ = cull_badge_color(badge);
        }
        assert_eq!(CullBadge::from_token("bogus"), None);
        assert_eq!(
            cull_filter_token_matches("cull:bogus", CullBadge::Keep),
            Some(false)
        );
        assert_eq!(cull_filter_token_matches("rating:3", CullBadge::Keep), None);
        assert_eq!(CullBadge::None.badge_text(), None);
        for badge in [
            CullBadge::Keep,
            CullBadge::Review,
            CullBadge::Reject,
            CullBadge::Stale,
        ] {
            assert!(badge.badge_text().is_some(), "{badge:?} paints a chip");
        }
    }

    /// Decision §2: a stale/unusable section is never presented as current.
    #[test]
    fn read_state_maps_to_badge_and_stale_is_never_current() {
        fn section(proposal: CullProposal, status: CullingStatus) -> CullingSection {
            CullingSection {
                version: CULLING_SCHEMA_VERSION,
                proposal,
                score: 0.5,
                reasons: vec!["sharpness_low".into()],
                identity: lumina_cull::heuristic_identity(
                    lumina_sidecar::SourceFingerprint {
                        content_hash: "blake3:a".into(),
                        byte_length: 1,
                        extras: Default::default(),
                    },
                    lumina_sidecar::DecodeFingerprint {
                        decoder: "image".into(),
                        version: "1".into(),
                        parameters: Default::default(),
                        extras: Default::default(),
                    },
                    lumina_sidecar::GeometryFingerprint {
                        width: 8,
                        height: 6,
                        orientation: 1,
                        pixel_aspect_ratio: 1.0,
                        extras: Default::default(),
                    },
                    Resolution {
                        width: 8,
                        height: 6,
                        extras: Default::default(),
                    },
                ),
                created_at: "2026-09-16T00:00:00Z".into(),
                status,
                error: None,
                extras: Default::default(),
            }
        }
        assert_eq!(CullBadge::from_section(None, true), CullBadge::None);
        assert_eq!(
            CullBadge::from_section(
                Some(&section(CullProposal::Keep, CullingStatus::Valid)),
                true
            ),
            CullBadge::Keep
        );
        assert_eq!(
            CullBadge::from_section(
                Some(&section(
                    CullProposal::RejectCandidate,
                    CullingStatus::Valid
                )),
                false
            ),
            CullBadge::Stale,
            "a changed source makes the proposal stale"
        );
        assert_eq!(
            CullBadge::from_section(
                Some(&section(CullProposal::Keep, CullingStatus::Corrupt)),
                true
            ),
            CullBadge::Stale
        );
    }

    /// E2E (DoD §1): explicit adopt → sidecar file → reload shows the badge.
    #[test]
    fn adopt_writes_only_culling_and_reloads() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source, 1);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        // Seed a manual rating/flag/label first: the adopt action must not
        // touch any of them.
        app.set_rating(4).unwrap();
        app.set_flag(lumina_sidecar::Flag::Pick).unwrap();
        app.set_color_label(2).unwrap();
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none(), "{:?}", app.error());
        let before = load_sidecar(&sidecar_path_for(&source)).unwrap();
        let before_recipe = before.virtual_copies[0].recipe.clone();

        let report = app.adopt_culling().unwrap();
        assert!(report.badge != CullBadge::Stale);
        let after = load_sidecar(&sidecar_path_for(&source)).unwrap();
        let section = after.culling.clone().expect("culling recorded");
        assert_eq!(section.status, CullingStatus::Valid);
        assert!(cull_badge_is_proposal(CullBadge::from_section(
            Some(&section),
            true
        )));
        assert_eq!(
            after.virtual_copies[0].rating, 4,
            "adopt must never write rating"
        );
        assert_eq!(after.virtual_copies[0].flag, lumina_sidecar::Flag::Pick);
        assert_eq!(
            color_label_of(&after.virtual_copies[0].extras),
            2,
            "the manual label survives and no other label is written"
        );
        assert_eq!(
            after.virtual_copies[0].recipe, before_recipe,
            "adopt must not rewrite the recipe"
        );

        // Reload: the grid badge reflects the persisted proposal.
        let mut reopened = new_app();
        reopened.list_directory_flat();
        reopened.open_file(source.display().to_string());
        for _ in 0..2000 {
            reopened.poll_decode();
            if reopened.original.is_some() || reopened.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(reopened.culling_badge().is_some(), "badge after reload");
        assert_ne!(reopened.culling_badge(), Some(CullBadge::None));
    }

    /// Decision §1: no automatic rating/flag/label from the proposal.
    #[test]
    fn adopt_is_explicit_and_never_auto_rates() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source, 2);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        // Before any explicit action there is no proposal at all.
        assert_eq!(app.culling_read_state(), CullingReadState::NoProposal);
        assert_eq!(
            app.recipe().adjustments.get("exposure"),
            None,
            "analysis never edits the recipe"
        );
        app.adopt_culling().unwrap();
        let document = app.document.clone().unwrap();
        assert!(document.culling.is_some());
        assert_eq!(document.virtual_copies[0].rating, 0, "no auto rating");
        assert_eq!(
            document.virtual_copies[0].flag,
            lumina_sidecar::Flag::Unflagged
        );
    }

    /// The authoritative read state detects a changed source as stale.
    #[test]
    fn changed_source_makes_the_proposal_stale() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source, 3);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.adopt_culling().unwrap();
        assert!(matches!(
            app.culling_read_state(),
            CullingReadState::Valid(_)
        ));
        // Change the source pixels: the content hash no longer matches.
        save_png(&source, 9);
        let mut reopened = new_app();
        open_and_decode(&mut reopened, &source);
        assert!(matches!(
            reopened.culling_read_state(),
            CullingReadState::Stale { .. }
        ));
        assert_eq!(
            CullBadge::from_read_state(&reopened.culling_read_state()),
            CullBadge::Stale
        );
    }

    /// Explicit clear removes the proposal without touching the recipe.
    #[test]
    fn clear_removes_only_the_proposal() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source, 4);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.adopt_culling().unwrap();
        let recipe = app.recipe().clone();
        app.clear_culling_proposal().unwrap();
        let document = load_sidecar(&sidecar_path_for(&source)).unwrap();
        assert!(document.culling.is_none());
        assert_eq!(document.virtual_copies[0].recipe, recipe);
    }

    /// The `\`-filter understands the cull tokens and rejects unknown values
    /// (pure entry predicate; the RAW-only grid order is asserted elsewhere).
    #[test]
    fn library_filter_supports_cull_tokens() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source, 5);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        app.adopt_culling().unwrap();
        let entry = app
            .entries()
            .iter()
            .find(|entry| entry.path.display().to_string() == app.path)
            .cloned()
            .expect("scan entry");
        let badge = entry.cull_badge;
        assert!(badge != CullBadge::None && badge != CullBadge::Stale);
        assert!(crate::library_entry_matches(
            &entry,
            &format!("cull:{}", badge.token())
        ));
        assert!(!crate::library_entry_matches(&entry, "cull:stale"));
        assert!(!crate::library_entry_matches(&entry, "cull:bogus"));
        // The prefix is recognized even when the entry carries no proposal.
        let fresh = FileBrowserEntry {
            cull_badge: CullBadge::None,
            ..entry.clone()
        };
        assert!(crate::library_entry_matches(&fresh, "cull:none"));
        assert!(!crate::library_entry_matches(&fresh, "cull:keep"));
    }

    fn cull_badge_is_proposal(badge: CullBadge) -> bool {
        matches!(
            badge,
            CullBadge::Keep | CullBadge::Review | CullBadge::Reject
        )
    }

    fn color_label_of(extras: &lumina_sidecar::Extras) -> u8 {
        crate::color_label_of(extras)
    }
}
