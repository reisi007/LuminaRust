//! badge/rating/flag/color-label rendering and persistence tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// Badge helpers (LR-01/LR-17 light): the Library grid badge composes
/// `stars_for_rating` + `flag_label` + `color_label_name`, and both the
/// grid scan and the rating section read via `color_label_of`. Pure
/// functions, pinned headless so badge visibility never regresses
/// silently.
#[test]
fn color_label_names_and_parsing_for_badges() {
    assert_eq!(color_label_name(1), "Red");
    assert_eq!(color_label_name(2), "Yellow");
    assert_eq!(color_label_name(3), "Green");
    assert_eq!(color_label_name(4), "Blue");
    assert_eq!(color_label_name(0), "Color Label");
    assert_eq!(color_label_name(9), "Color Label");
    assert_eq!(color_label_of(&BTreeMap::new()), 0);
    let labelled = BTreeMap::from([("color_label".to_string(), serde_json::Value::from(3))]);
    assert_eq!(color_label_of(&labelled), 3);
    let out_of_range = BTreeMap::from([("color_label".to_string(), serde_json::Value::from(7))]);
    assert_eq!(color_label_of(&out_of_range), 0);
    let non_numeric = BTreeMap::from([("color_label".to_string(), serde_json::Value::from("red"))]);
    assert_eq!(color_label_of(&non_numeric), 0);
}

/// UX-SLICE-1 (UXG-09): the filmstrip/grid badge text is composed once and
/// stays empty for a clean cell; rating, flag and color label all surface.
/// (Painted badges are primitives, so this pins the data path headless.)
#[test]
fn entry_badge_text_covers_rating_flag_label() {
    let mut entry = raw_entry(std::path::Path::new("/tmp"), "IMG_0001.ARW");
    assert_eq!(entry_badge_text(&entry), None, "clean cell has no badge");
    entry.rating = 3;
    assert_eq!(entry_badge_text(&entry).as_deref(), Some("★★★☆☆"));
    entry.flag = lumina_sidecar::Flag::Pick;
    assert_eq!(entry_badge_text(&entry).as_deref(), Some("★★★☆☆ P"));
    entry.flag = lumina_sidecar::Flag::Reject;
    assert_eq!(entry_badge_text(&entry).as_deref(), Some("★★★☆☆ X"));
    entry.flag = lumina_sidecar::Flag::Unflagged;
    entry.color_label = 2;
    assert_eq!(entry_badge_text(&entry).as_deref(), Some("★★★☆☆ ●Yellow"));
}

#[test]
fn stars_and_flag_labels_render_for_badges() {
    assert_eq!(stars_for_rating(0), "☆☆☆☆☆");
    assert_eq!(stars_for_rating(1), "★☆☆☆☆");
    assert_eq!(stars_for_rating(3), "★★★☆☆");
    assert_eq!(stars_for_rating(5), "★★★★★");
    assert_eq!(flag_label(Flag::Pick), "Pick");
    assert_eq!(flag_label(Flag::Reject), "Reject");
    assert_eq!(flag_label(Flag::Unflagged), "Unflagged");
}

#[test]
fn clip_fractions_counts_pure_black_and_white() {
    // 2×2: black, white, mid-grey, white → 25% shadow, 50% highlight.
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            0, 0, 0, 255, 255, 255, 255, 255, 10, 20, 30, 255, 255, 255, 255, 255,
        ],
    )
    .unwrap();
    let (shadow, highlight) = clip_fractions(&frame);
    assert!((shadow - 0.25).abs() < 1e-12);
    assert!((highlight - 0.5).abs() < 1e-12);
    // A coloured frame clips nothing; alpha is ignored.
    let coloured = ImageFrame::new(1, 1, vec![128, 64, 200, 0]).unwrap();
    assert_eq!(clip_fractions(&coloured), (0.0, 0.0));
}

#[test]
fn color_label_set_persists_and_rejects_invalid() {
    // Welle 2: `extras["color_label"]` roundtrips through save/reopen and
    // the Library scan; out-of-range values fail loudly.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert_eq!(app.color_label(), None);
    app.set_color_label(2).unwrap();
    assert_eq!(app.color_label(), Some(2));
    assert!(app.set_color_label(5).is_err());
    assert!(app.set_color_label(255).is_err());
    // The rejected writes left the stored label untouched.
    assert_eq!(app.color_label(), Some(2));
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(color_label_of(&document.virtual_copies[0].extras), 2);
    // Corrupt/foreign values read as none (forward-compatible cosmetic).
    assert_eq!(
        color_label_of(&BTreeMap::from([(
            "color_label".into(),
            serde_json::Value::from(9u64)
        )])),
        0
    );
    assert_eq!(
        color_label_of(&BTreeMap::from([(
            "color_label".into(),
            serde_json::Value::from("red")
        )])),
        0
    );
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.color_label(), Some(2));
    reopened.set_directory(directory.path().display().to_string());
    let entry = reopened
        .entries
        .iter()
        .find(|entry| entry.name == "photo.png")
        .unwrap();
    assert_eq!(entry.color_label, 2);
}

/// GUI-LIBRARY-BADGE-CONTRAST-1: white badge text on the badge chip meets
/// AA normal-text contrast (≥ 4.5), while the chip itself stays a
/// dark-theme surface (luminance < 0.12, same bar as `theme.rs`).
#[test]
fn library_badge_contrast_meets_aa() {
    fn linearize(c: u8) -> f32 {
        let s = c as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    fn luminance(c: egui::Color32) -> f32 {
        0.2126 * linearize(c.r()) + 0.7152 * linearize(c.g()) + 0.0722 * linearize(c.b())
    }
    let lum_bg = luminance(LIBRARY_BADGE_BG);
    assert!(
        lum_bg < 0.12,
        "badge chip must stay a dark-theme surface, luminance {lum_bg:.4}"
    );
    let ratio = (1.0 + 0.05) / (lum_bg + 0.05);
    assert!(
        ratio >= 4.5,
        "white badge text on the chip must meet AA (ratio {ratio:.2} < 4.5)"
    );
}
