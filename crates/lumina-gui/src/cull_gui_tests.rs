//! Headless tests for the Library culling model and explicit adopt action.

use super::*;
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for};
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

/// The authoritative read state detects a changed source as stale when the
/// sidecar's source identity itself is current.
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

    save_png(&source, 9);
    let sidecar_path = sidecar_path_for(&source);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    let live_bytes = std::fs::read(&source).unwrap();
    document.source.content_hash = format!("blake3:{}", blake3::hash(&live_bytes).to_hex());
    document.source.byte_length = live_bytes.len() as u64;
    save_sidecar(&sidecar_path, &document).unwrap();
    let sidecar_before = std::fs::read(&sidecar_path).unwrap();

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
    let live_identity = reopened.culling_current_identity().unwrap();
    assert_eq!(
        live_identity.source.byte_length,
        reopened.source_bytes.as_ref().unwrap().len() as u64,
        "live identity must use loaded source bytes, not SidecarDocument.source"
    );
    assert_eq!(std::fs::read(&sidecar_path).unwrap(), sidecar_before);
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
