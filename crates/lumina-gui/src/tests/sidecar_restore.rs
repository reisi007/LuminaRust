//! sidecar restore into sliders/preview tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-PREVIEW-NOISE-1: adopting a cached neighbor frame books it as a
/// low-res draft stand-in — placement source, draft flag and derived
/// analysis state describe the stand-in, never a committed full render.
#[test]
fn neighbor_preview_paint_books_draft_state() {
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    assert!(app.render_key().is_some());
    assert!(app.current_histogram().is_some());
    let generation = app.preview_generation();
    // A smaller stand-in frame, as the neighbor cache would serve it.
    let stand_in = ImageFrame::new(16, 10, vec![90u8; 16 * 10 * 4]).unwrap();
    app.adopt_neighbor_preview_frame(stand_in);
    assert!(app.preview_generation() > generation);
    assert!(app.texture_identity.is_none());
    assert_eq!(app.preview_render_src, Some((16, 10)));
    assert!(app.preview_roi.is_none());
    assert!(
        app.preview_is_draft,
        "a neighbor stand-in must read as draft in the HUD"
    );
    assert!(
        app.render_key().is_none(),
        "no committed render key may describe the stand-in"
    );
    assert!(
        app.current_histogram().is_none(),
        "no stale histogram may describe the stand-in"
    );
    assert!(
        app.tone_analysis.is_none(),
        "no stale tone analysis may describe the stand-in"
    );
}

/// GUI-SIDECAR-RESTORE-1 (DoD-§1-Anker): values that reach the disk
/// sidecar from *outside* the session (here: exposure −0.62 written by an
/// external edit, simulating the user's file) must reappear after reopen
/// in the recipe, on the Basic slider readout AND in the rendered preview
/// — proving the display comes from the file, never from session memory.
#[test]
fn sidecar_restore_from_file_applies_to_sliders_and_preview() {
    use lumina_sidecar::{load_sidecar, save_sidecar as raw_save, sidecar_path_for};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("restore.png");
    let (png, original) = synthetic_gradient_png();
    std::fs::write(&source, &png).unwrap();

    // Session 1 only creates the sidecar (exposure +1.0, unrelated value).
    let mut first = new_app();
    open_and_decode(&mut first, source.display().to_string());
    assert!(first.error().is_none());
    first.set_adjustment("exposure", 1.0);
    first.render().unwrap();
    first.save_sidecar();
    assert!(first.error().is_none());
    drop(first);

    // External edit (another session / hand edit): exposure −0.62.
    let sidecar_path = sidecar_path_for(&source);
    let mut external = load_sidecar(&sidecar_path).unwrap();
    external.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), -0.62);
    raw_save(&sidecar_path, &external).unwrap();

    // Session 2 (fresh): reopen must restore the FILE value, not the
    // first session's +1.0.
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.error().is_none(), "reopen must succeed");
    assert_eq!(
        app.recipe().adjustments.get("exposure"),
        Some(&-0.62),
        "recipe must carry the file value −0.62, not the old session value"
    );
    // Preview applies the restored recipe: byte-identical to a direct
    // core render with the restored recipe, and visibly darker than the
    // default render.
    let preview = app.preview().expect("preview after reopen").clone();
    let restored_ctx = RenderContext {
        recipe: app.recipe(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let direct = render_frame(&original, &restored_ctx).unwrap().frame;
    assert_eq!(
        preview.pixels, direct.pixels,
        "reopened preview must render the restored recipe"
    );
    let default_ctx = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let default_frame = render_frame(&original, &default_ctx).unwrap().frame;
    assert_ne!(
        preview.pixels, default_frame.pixels,
        "restored exposure must visibly change the preview"
    );
    assert!(
        avg_luminance(&preview) < avg_luminance(&default_frame),
        "exposure −0.62 must darken the preview"
    );
    // Basic slider readout shows the restored value (identity scale,
    // 1 decimal → "-0.6"): the slider binds the recipe every frame.
    let shapes = headless_shapes(&mut app, |app, ui| {
        app.adjustment_slider(
            ui,
            "exposure",
            Str::Exposure.t(),
            identity_spec(-10.0..=10.0, 0.0, 0.1),
        );
    });
    let texts: Vec<String> = shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t == "-0.6"),
        "exposure slider must display the restored value (−0.6), painted: {texts:?}"
    );
}

/// P0-Audit (DoD §3): alle sechs Basic-Regler überleben einen externen
/// Sidecar-Edit und werden beim Reopen in Rezept, Slider-Readout und
/// Preview sichtbar.
#[test]
fn sidecar_restore_all_basic_sliders_reopen() {
    use crate::slider::{to_display, DisplayScale};
    // (key, externer Wert, Display-Skala, erwarteter Readout).
    // Exposure ist Identity-Domain, der Rest Prozent-Domain (×100).
    let cases: [(&str, f64, DisplayScale, f64); 6] = [
        ("exposure", 1.5, DisplayScale::Identity, 1.5),
        ("contrast", 0.4, DisplayScale::Percent, 40.0),
        ("highlights", -0.3, DisplayScale::Percent, -30.0),
        ("shadows", 0.5, DisplayScale::Percent, 50.0),
        ("whites", 0.6, DisplayScale::Percent, 60.0),
        ("blacks", -0.5, DisplayScale::Percent, -50.0),
    ];
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    let baseline = app.preview().unwrap().pixels.clone();
    // Sidecar-Datei materialisieren, dann EXTERN (am App vorbei, wie ein
    // fremder Prozess) alle sechs Regler setzen.
    app.set_adjustment("exposure", 0.1);
    app.save_sidecar();
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    let mut document = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    for (key, value, _, _) in &cases {
        document.virtual_copies[0]
            .recipe
            .adjustments
            .insert((*key).into(), *value);
    }
    lumina_sidecar::save_sidecar(&sidecar_path, &document).unwrap();
    // Reopen: Rezept, Readout-Mapping und Preview müssen den Edit zeigen.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    reopened.render().unwrap();
    for (key, value, scale, readout) in &cases {
        assert_eq!(
            reopened.recipe().adjustments.get(*key),
            Some(value),
            "rezept muss {key}={value} nach Reopen enthalten"
        );
        // Slider-Readout: exakt die Abbildung, die `lr_slider` malt.
        let shown = to_display(*value, *scale);
        assert!(
            (shown - readout).abs() < 1e-12,
            "readout für {key}: gezeigt {shown}, erwartet {readout}"
        );
    }
    let pixels = reopened.preview().unwrap().pixels.clone();
    assert_ne!(
        pixels, baseline,
        "preview muss sich nach dem externen 6-Regler-Edit ändern"
    );
}

/// GUI-SIDECAR-READ-1 (DoD-§1): switching images restores each file's
/// sidecar values to the recipe AND the slider readouts — the display
/// comes from the file, never from session memory.
#[test]
fn switching_image_restores_sidecar_values_to_sliders() {
    fn exposure_text(app: &mut LuminaApp) -> Vec<String> {
        painted_texts(&headless_shapes(app, |app, ui| {
            app.adjustment_slider(
                ui,
                "exposure",
                Str::Exposure.t(),
                identity_spec(-10.0..=10.0, 0.0, 0.1),
            );
        }))
    }

    /// Open `path` and wait until its background decode landed (the shared
    /// `open_and_decode` helper only waits for *any* image — on a switch
    /// the previous frame would satisfy it immediately, so the switch
    /// itself must be awaited by path).
    fn switch_and_wait(app: &mut LuminaApp, path: &str) {
        app.open_file(path.to_string());
        for _ in 0..2000 {
            app.poll_decode();
            if app.path == path || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(app.path, path, "the switched image must finish loading");
        assert!(app.error().is_none(), "switch must succeed");
    }

    let directory = tempfile::tempdir().unwrap();
    let source_a = directory.path().join("a.png");
    let source_b = directory.path().join("b.png");
    save_png(&source_a);
    save_png(&source_b);
    let mut app = new_app();
    switch_and_wait(&mut app, &source_a.display().to_string());
    app.set_adjustment("exposure", 1.5);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    switch_and_wait(&mut app, &source_b.display().to_string());
    app.set_adjustment("exposure", -0.5);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    // Back to A: the file value 1.5 returns to recipe and slider.
    switch_and_wait(&mut app, &source_a.display().to_string());
    assert_eq!(app.recipe().adjustments.get("exposure"), Some(&1.5));
    assert!(
        exposure_text(&mut app).iter().any(|t| t == "1.5"),
        "slider must display A's restored exposure"
    );
    // Over to B: the file value −0.5 returns to recipe and slider.
    switch_and_wait(&mut app, &source_b.display().to_string());
    assert_eq!(app.recipe().adjustments.get("exposure"), Some(&-0.5));
    assert!(
        exposure_text(&mut app).iter().any(|t| t == "-0.5"),
        "slider must display B's restored exposure"
    );
}

/// SIDECAR-REBASE-1 (DoD §1): an overtaking save (external write bumps the
/// sidecar revision) must be rebased, not dropped. The foreign edit and the
/// session edit both end up on disk, no conflict dialog is shown, and a fresh
/// session reloads the merged state.
#[test]
fn sidecar_save_rebases_overtaking_save_and_keeps_both_edits() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 0.2);
    app.save_sidecar();
    assert!(app.error().is_none());
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(app.sidecar_revision.is_some());

    // Overtaking external write: contrast changed outside this session.
    let mut external = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    external.virtual_copies[0]
        .recipe
        .adjustments
        .insert("contrast".into(), 0.4);
    lumina_sidecar::save_sidecar(&sidecar, &external).unwrap();

    // The session still holds the stale revision: the next save must rebase.
    app.set_adjustment("highlights", -0.3);
    app.save_sidecar();
    assert!(
        app.error().is_none(),
        "rebase must not surface a conflict dialog: {:?}",
        app.error()
    );
    let merged = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let adjustments = &merged.virtual_copies[0].recipe.adjustments;
    assert_eq!(adjustments.get("exposure"), Some(&0.2));
    assert_eq!(
        adjustments.get("contrast"),
        Some(&0.4),
        "the foreign edit must survive the rebase"
    );
    assert_eq!(
        adjustments.get("highlights"),
        Some(&-0.3),
        "the local edit must be applied"
    );
    assert_eq!(
        app.sidecar_revision.as_deref(),
        Some(lumina_sidecar::document_revision(&merged).unwrap().as_str()),
        "the session must adopt the rebased revision"
    );

    // Roundtrip: a fresh session restores the merged state.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.recipe().adjustments.get("exposure"), Some(&0.2));
    assert_eq!(reopened.recipe().adjustments.get("contrast"), Some(&0.4));
    assert_eq!(reopened.recipe().adjustments.get("highlights"), Some(&-0.3));
}

/// SIDECAR-REBASE-1: a writer that keeps overtaking every retry is surfaced
/// loudly after exactly [`MAX_REBASE_ATTEMPTS`] retries — never an endless
/// loop and never a silent overwrite.
#[test]
fn sidecar_rebase_gives_up_loudly_after_bounded_attempts() {
    use crate::sidecar_rebase::{save_rebased, set_conflict_hook, MAX_REBASE_ATTEMPTS};
    use lumina_sidecar::SidecarError;
    use std::cell::Cell;
    use std::rc::Rc;

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 0.1);
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);

    let base = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let base_revision = lumina_sidecar::document_revision(&base).unwrap();
    let mut local = base.clone();
    local.virtual_copies[0]
        .recipe
        .adjustments
        .insert("highlights".into(), 0.5);

    // A writer that overtakes before every attempt: the retry budget is
    // exhausted, so the save must fail loudly instead of looping forever.
    let attempts = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&attempts);
    let hook_path = sidecar.clone();
    set_conflict_hook(Some(Box::new(move |_| {
        let next = counter.get() + 1;
        counter.set(next);
        let mut disk = lumina_sidecar::load_sidecar(&hook_path).unwrap();
        disk.virtual_copies[0]
            .recipe
            .adjustments
            .insert("shadows".into(), 0.1 * next as f64);
        lumina_sidecar::save_sidecar(&hook_path, &disk).unwrap();
    })));
    let result = save_rebased(
        &sidecar,
        &base,
        &local,
        Some(&base_revision),
        MAX_REBASE_ATTEMPTS,
    );
    set_conflict_hook(None);
    assert_eq!(
        attempts.get(),
        MAX_REBASE_ATTEMPTS + 1,
        "one overtake per attempt: the initial save plus one per rebase retry"
    );
    match result {
        Err(SidecarError::Conflict(message)) => assert!(
            message.contains("rebase attempt"),
            "the loud conflict must name the exhausted attempts: {message}"
        ),
        other => panic!("expected a loud conflict after the retries, got {other:?}"),
    }
}

/// SIDECAR-REBASE-1: a section-only save (`culling`/`face`) applies exactly
/// that section onto the current file, so a foreign change to another field
/// survives the rebase.
#[test]
fn sidecar_section_rebase_preserves_foreign_recipe_change() {
    use crate::sidecar_rebase::RebaseSection;

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 0.2);
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);

    // Overtaking external write: culling set AND exposure/contrast changed.
    let mut external = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    external.virtual_copies[0]
        .recipe
        .adjustments
        .insert("contrast".into(), 0.4);
    external.culling = Some(culling_section());
    lumina_sidecar::save_sidecar(&sidecar, &external).unwrap();

    // The session clears the culling section on a stale CAS revision: the
    // rebase must apply the clear while the foreign recipe change survives.
    let local = app.document.clone().unwrap();
    let path = app.path.clone();
    app.save_section_with_rebase(&path, local, RebaseSection::Culling)
        .unwrap();
    let merged = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(merged.culling.is_none(), "the explicit clear must win");
    assert_eq!(
        merged.virtual_copies[0].recipe.adjustments.get("contrast"),
        Some(&0.4),
        "the foreign recipe change must survive a section rebase"
    );
    assert_eq!(
        merged.virtual_copies[0].recipe.adjustments.get("exposure"),
        Some(&0.2)
    );
}

/// A structurally valid culling section for the section-rebase test.
fn culling_section() -> lumina_sidecar::CullingSection {
    use lumina_sidecar::{
        CullProposal, CullingSection, CullingStatus, DecodeFingerprint, GeometryFingerprint,
        Resolution, SourceFingerprint, CULLING_SCHEMA_VERSION,
    };
    CullingSection {
        version: CULLING_SCHEMA_VERSION,
        proposal: CullProposal::Keep,
        score: 0.5,
        reasons: vec!["sharpness_low".into()],
        identity: lumina_cull::heuristic_identity(
            SourceFingerprint {
                content_hash: "blake3:a".into(),
                byte_length: 1,
                extras: Default::default(),
            },
            DecodeFingerprint {
                decoder: "image".into(),
                version: "1".into(),
                parameters: Default::default(),
                extras: Default::default(),
            },
            GeometryFingerprint {
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
        status: CullingStatus::Valid,
        error: None,
        extras: Default::default(),
    }
}
