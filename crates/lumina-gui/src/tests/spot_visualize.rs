//! spot visualize overlay persistence tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn g04_spot_visualize_slider_persists_file_to_reload() {
    // DoD §1 E2E: slider edit → debounced commit → sidecar file → reload.
    // DoD §2: the 150-ms debounce path is driven headless via
    // `commit_pending_slider_save` (the same hook all sliders use).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    std::fs::write(&source, dark_block_png()).unwrap();
    let original_bytes = std::fs::read(&source).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert_eq!(app.spot_visualize_threshold(), None);
    // Out-of-range values fail loudly and change nothing.
    assert!(app.set_spot_visualize(Some(1.5)).is_err());
    assert!(app.set_spot_visualize(Some(f32::NAN)).is_err());
    assert_eq!(app.spot_visualize_threshold(), None);
    // Set + drive the debounced commit hook (render + save + info! log).
    app.set_spot_visualize(Some(0.3)).unwrap();
    assert_eq!(app.spot_visualize_threshold(), Some(0.3));
    assert_eq!(
        app.pending_slider_commit,
        Some(("spot.visualize".into(), f64::from(0.3f32)))
    );
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.spot_visualize_threshold(),
        Some(0.3)
    );
    // Reload leg: a fresh app restores the threshold from the file alone.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.spot_visualize_threshold(), Some(0.3));
    // Clearing persists the removal too.
    reopened.set_spot_visualize(None).unwrap();
    reopened.commit_pending_slider_save([0, 0]);
    assert!(reopened.error().is_none());
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.spot_visualize_threshold(),
        None
    );
    let mut reopened2 = new_app();
    open_and_decode(&mut reopened2, source.display().to_string());
    assert_eq!(reopened2.spot_visualize_threshold(), None);
    // The original image is byte-identical throughout.
    assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
}

#[test]
fn g04_spot_visualize_overlay_visible_in_preview_after_reload() {
    // G04-FOLLOWUP-1 B1: the persisted threshold has a functional
    // consumer — the preview gate tints candidate pixels red
    // (DoD §1 E2E: Datei -> Reload -> Render; PSNR-Gate wo visuell).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    std::fs::write(&source, dark_block_png()).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.error().is_none());
    // No threshold: gate off, preview is the clean render; the session
    // detect default is 0.5 without a recipe value.
    assert_eq!(app.spot_visualize_overlay_threshold(), None);
    assert_eq!(app.spot_detect_effective_threshold(), 0.5);
    let clean = app.preview().expect("preview rendered on load").clone();
    // Set + drive the debounced commit hook (render + save + info! log).
    app.set_spot_visualize(Some(0.3)).unwrap();
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    assert_eq!(app.spot_visualize_overlay_threshold(), Some(0.3));
    assert_eq!(app.spot_detect_effective_threshold(), 0.3);
    let tinted = app.preview().expect("preview after commit").clone();
    assert_eq!((tinted.width, tinted.height), (clean.width, clean.height));
    assert_ne!(
        tinted.pixels, clean.pixels,
        "the overlay must visibly change the preview"
    );
    // Only candidate pixels (the dark 8x8 block) follow the deterministic
    // red-tint formula; everything else (incl. alpha) is byte-identical.
    for y in 0..16 {
        for x in 0..16 {
            let i = (y * 16 + x) as usize * 4;
            if x < 8 && y < 8 {
                let (r, g, b) = (clean.pixels[i], clean.pixels[i + 1], clean.pixels[i + 2]);
                assert_eq!(
                    tinted.pixels[i],
                    ((u16::from(r) + 255) / 2).min(255) as u8,
                    "candidate red at {x},{y}"
                );
                assert_eq!(tinted.pixels[i + 1], (u16::from(g) / 2) as u8);
                assert_eq!(tinted.pixels[i + 2], (u16::from(b) / 2) as u8);
                assert_ne!(
                    &tinted.pixels[i..i + 3],
                    &clean.pixels[i..i + 3],
                    "candidate must change at {x},{y}"
                );
            } else {
                assert_eq!(
                    &tinted.pixels[i..i + 4],
                    &clean.pixels[i..i + 4],
                    "non-candidate byte-identical at {x},{y}"
                );
            }
            assert_eq!(
                tinted.pixels[i + 3],
                clean.pixels[i + 3],
                "alpha untouched at {x},{y}"
            );
        }
    }
    let psnr = lumina_core::psnr(&clean, &tinted);
    assert!(
        psnr.is_finite() && psnr > 5.0,
        "overlay PSNR gate (only candidates change): {psnr}"
    );
    // Reload leg: a fresh app restores the threshold from the file alone
    // and the preview is tinted deterministically (byte-identical).
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert!(reopened.error().is_none());
    assert_eq!(reopened.spot_visualize_threshold(), Some(0.3));
    // The session detect slider synced from the recipe on load.
    assert_eq!(reopened.spot_detect_effective_threshold(), 0.3);
    let reloaded = reopened.preview().expect("preview after reload").clone();
    assert_eq!(
        reloaded.pixels, tinted.pixels,
        "overlay deterministic across reload"
    );
    // `Never` hides the overlay without touching the recipe (no silent
    // fallback: the threshold persists, untinted render is explicit).
    reopened.set_overlay_mode(OverlayMode::Never);
    assert_eq!(reopened.spot_visualize_overlay_threshold(), None);
    reopened.render().unwrap();
    let hidden = reopened.preview().expect("preview in Never mode").clone();
    assert_eq!(hidden.pixels, clean.pixels);
    assert_eq!(reopened.spot_visualize_threshold(), Some(0.3));
    // `Auto` hides while no tool is armed and shows once the spot tool
    // (Q) is armed — the G-11 rule, headless-pinned.
    reopened.set_overlay_mode(OverlayMode::Auto);
    assert_eq!(reopened.spot_visualize_overlay_threshold(), None);
    reopened.set_spot_tool(SpotTool::Heal);
    assert_eq!(reopened.spot_visualize_overlay_threshold(), Some(0.3));
    reopened.set_spot_tool(SpotTool::None);
    assert_eq!(reopened.spot_visualize_overlay_threshold(), None);
}
