//! preview render smoke and sidecar persist-on-close tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn preview_correctly_rendered() {
    // (1) Preview korrekt gerendert — synthetisches 8×8 via LuminaApp
    let (png, original) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "synthetic.png").unwrap();
    // Preview vorhanden, generation gebumpt, render_key vorhanden
    let preview = app.preview().expect("preview after load").clone();
    assert_eq!((preview.width, preview.height), (8, 8));
    assert!(app.preview_generation() > 0, "preview_generation must bump");
    assert!(app.render_key().is_some(), "render_key after load");
    assert!(!app.preview_is_draft(), "load produces full render");
    // Byte-identisch gegen direkte Pipeline ohne adjustments
    let ctx = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let direct = lumina_core::render_frame(&original, &ctx).unwrap().frame;
    assert_eq!(
        preview.pixels, direct.pixels,
        "LuminaApp preview must match direct pipeline render at exposure 0"
    );
    // Exposure +1 → heller, nicht identisch, innerhalb Toleranz (kein Clipping auf 255 für alle)
    let baseline_avg = avg_luminance(&preview);
    app.set_adjustment("exposure", 1.0);
    app.render().unwrap();
    let bright = app.preview().unwrap().clone();
    assert_ne!(
        preview.pixels, bright.pixels,
        "exposure +1 must change pixels"
    );
    let bright_avg = avg_luminance(&bright);
    assert!(
        bright_avg > baseline_avg + 5.0,
        "brighter exposure must increase avg luminance {baseline_avg} -> {bright_avg}"
    );
    // Kein stiller Fallback bei leerem Frame: no original → preview None, no panic
    let mut empty = new_app();
    // render() on empty is Ok, preview stays None, render_key None, status NoImageLoaded
    empty.render().unwrap();
    assert!(empty.preview().is_none(), "empty app has no preview");
    assert!(empty.render_key().is_none(), "empty app has no render_key");
    assert!(
        empty.status().contains("No image") || empty.status().contains("Bereit"),
        "empty render status {:?}",
        empty.status()
    );
}

#[test]
fn slider_changes_preview() {
    // (2) Regler-Bewegung ändert Preview — exposure/contrast/whites/blacks
    let (png, _) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png, "synthetic.png").unwrap();
    app.render().unwrap();
    let baseline_key = app.render_key().cloned().unwrap();
    let baseline_gen = app.preview_generation();
    let baseline_pixels = app.preview().unwrap().pixels.clone();
    let baseline_avg = avg_luminance(app.preview().unwrap());

    let cases: &[(&str, f64)] = &[
        ("exposure", 1.0),
        ("contrast", 0.5),
        ("whites", 0.6),
        ("blacks", -0.6),
    ];
    for (key, value) in cases {
        // Reset to baseline before each isolated check
        let mut app2 = new_app();
        let (png2, _) = synthetic_8x8_png();
        app2.load_bytes(png2, "synthetic.png").unwrap();
        app2.render().unwrap();
        let before_key = app2.render_key().cloned().unwrap();
        let before_gen = app2.preview_generation();
        let before_pixels = app2.preview().unwrap().pixels.clone();

        app2.set_adjustment(key, *value);
        // Debounce/ROI stabil: vor render ist key invalidiert, aber noch kein
        // async nötig — direkt synchron rendern, nicht über debounce warten
        assert!(
            app2.render_key().is_none(),
            "render_key invalidated after set_adjustment {key}"
        );
        assert!(app2.pending_full_render, "pending_full_render after {key}");
        app2.render().unwrap();
        assert!(app2.render_key().is_some(), "render_key after render {key}");
        assert_ne!(
            before_key.digest(),
            app2.render_key().unwrap().digest(),
            "render_key must change for {key}={value}"
        );
        assert!(
            app2.preview_generation() > before_gen,
            "preview_generation must bump for {key}"
        );
        // Bild unterscheidet sich: byte != ODER Histogram-Delta (beides gilt;
        // byte != ist strenger und für diese synthetischen Frames erfüllt)
        let after_pixels = app2.preview().unwrap().pixels.clone();
        assert_ne!(
            before_pixels, after_pixels,
            "pixels must differ after {key}={value}"
        );
        // Nicht flaky: erneutes render ohne Änderung ändert nichts mehr
        let gen_after = app2.preview_generation();
        let key_after = app2.render_key().cloned().unwrap();
        app2.render().unwrap();
        // Second render without edit may still re-render but pixels must stay identical
        assert_eq!(
            app2.preview().unwrap().pixels,
            after_pixels,
            "stable pixels on re-render without edit for {key}"
        );
        // Generation may bump again on re-render — we only require no pixel drift
        let _ = (gen_after, key_after);
    }

    // Gegenprobe: gleicher Wert wie zuvor führt nach erneutem Render zum selben Key
    // (Exposure 0 vs. absent sind unterschiedliche Rezepte — der zweite Set mit
    // identischem Wert ändert den Digest nicht). Stabile Re-Renders ohne Edit.
    let mut app3 = new_app();
    let (png3, _) = synthetic_8x8_png();
    app3.load_bytes(png3, "synthetic.png").unwrap();
    app3.set_adjustment("exposure", 0.7);
    app3.render().unwrap();
    let key_before = app3.render_key().cloned().unwrap();
    let pixels_before = app3.preview().unwrap().pixels.clone();
    app3.set_adjustment("exposure", 0.7);
    app3.render().unwrap();
    assert_eq!(
        key_before.digest(),
        app3.render_key().unwrap().digest(),
        "same value must yield same render_key"
    );
    assert_eq!(
        pixels_before,
        app3.preview().unwrap().pixels,
        "same value must yield identical pixels"
    );
    // Unterdrückte Warnung für baseline_* die oben für Vollständigkeit existieren
    let _ = (baseline_key, baseline_gen, baseline_pixels, baseline_avg);
}

#[test]
fn sidecar_persist_on_close_and_reload() {
    // (3) Änderungen spätestens beim Schließen im Sidecar persistiert und nach
    // Reload byte-identisch wiederhergestellt — CAS, Konflikt sichtbar, atomar,
    // relative Pfade, Original unverändert.
    use lumina_sidecar::{
        document_revision, load_sidecar, save_sidecar as raw_save, sidecar_path_for,
    };

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("original.png");
    let (png, _) = synthetic_8x8_png();
    let original_hash_before = blake3::hash(&png).to_hex().to_string();
    std::fs::write(&source, &png).unwrap();

    // Session 1: load, edit exposure, save (CAS expected None → fresh)
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.error().is_none(), "decode must succeed");
    let path_before_save = source.display().to_string();
    assert_eq!(app.path, path_before_save);
    app.set_adjustment("exposure", 1.2);
    app.render().unwrap();
    app.save_sidecar();
    assert!(
        app.error().is_none(),
        "first save must succeed: {:?}",
        app.error()
    );
    assert_eq!(app.status(), Str::SidecarSaved.t());
    let sidecar_path = sidecar_path_for(&source);
    assert!(sidecar_path.is_file(), "sidecar file must exist after save");
    // Original unverändert
    let original_hash_after = blake3::hash(&std::fs::read(&source).unwrap())
        .to_hex()
        .to_string();
    assert_eq!(
        original_hash_before, original_hash_after,
        "original must remain byte-identical"
    );
    // Sidecar enthält relative Pfade, keine absoluten
    let doc = load_sidecar(&sidecar_path).unwrap();
    assert!(
        !doc.source.relative_name.contains('/'.to_string().as_str())
            || !doc.source.relative_name.starts_with('/'),
        "relative_name must be portable, got {:?}",
        doc.source.relative_name
    );
    assert!(
        !doc.source.relative_name.starts_with('/'),
        "no absolute path in sidecar, got {:?}",
        doc.source.relative_name
    );
    // Recipe roundtrip
    let vc = doc
        .virtual_copies
        .iter()
        .find(|c| c.id == "vc-original")
        .unwrap();
    assert_eq!(vc.recipe.adjustments.get("exposure"), Some(&1.2));
    // document_revision neu nach Save
    let rev1 = document_revision(&doc).unwrap();
    assert!(!rev1.is_empty());
    assert_eq!(app.sidecar_revision.as_deref(), Some(rev1.as_str()));
    // Byte-identisch: to_json roundtrip über save/load
    let json_before = doc.to_json().unwrap();
    let doc_reloaded = load_sidecar(&sidecar_path).unwrap();
    let json_after = doc_reloaded.to_json().unwrap();
    assert_eq!(
        json_before, json_after,
        "sidecar JSON must be byte-identical after roundtrip"
    );
    assert_eq!(doc, doc_reloaded, "document must be byte-identical");

    // Erneutes save ohne externe Änderung: kein Konflikt (CAS mit rev1)
    app.set_adjustment("contrast", 0.4);
    app.render().unwrap();
    app.save_sidecar();
    assert!(
        app.error().is_none(),
        "second save without external change must succeed"
    );
    let doc2 = load_sidecar(&sidecar_path).unwrap();
    let rev2 = document_revision(&doc2).unwrap();
    assert_ne!(rev1, rev2, "revision must advance after second save");
    assert_eq!(
        doc2.virtual_copies
            .iter()
            .find(|c| c.id == "vc-original")
            .unwrap()
            .recipe
            .adjustments
            .get("contrast"),
        Some(&0.4)
    );
    assert_eq!(
        doc2.virtual_copies
            .iter()
            .find(|c| c.id == "vc-original")
            .unwrap()
            .recipe
            .adjustments
            .get("exposure"),
        Some(&1.2),
        "previous exposure must survive second save"
    );

    // Externer Conflict-Fall: externe Modifikation hinter dem Rücken der GUI
    let mut external = load_sidecar(&sidecar_path).unwrap();
    external.virtual_copies[0]
        .recipe
        .adjustments
        .insert("whites".into(), 0.9);
    raw_save(&sidecar_path, &external).unwrap();
    let rev_external = document_revision(&load_sidecar(&sidecar_path).unwrap()).unwrap();
    assert_ne!(rev2, rev_external);

    // Lokaler Versuch mit veralteter Revision → Conflict sichtbar, kein stiller Fallback
    app.set_adjustment("exposure", 2.0);
    app.render().unwrap();
    app.save_sidecar();
    assert!(app.error().is_some(), "conflict must be visible");
    assert_eq!(
        app.status(),
        Str::Error.t(),
        "conflict status must be Error"
    );
    let err_msg = app.error().unwrap().to_string();
    assert!(
        err_msg.to_lowercase().contains("conflict")
            || err_msg.contains("changed concurrently")
            || err_msg.contains("sidecar"),
        "conflict error must mention conflict, got {err_msg:?}"
    );
    // On-disk ist externe Version unverändert (lokaler 2.0 nicht überschrieben)
    let on_disk = load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        on_disk.virtual_copies[0].recipe.adjustments.get("exposure"),
        Some(&1.2),
        "conflicting save must not overwrite on-disk exposure"
    );
    assert_eq!(
        on_disk.virtual_copies[0].recipe.adjustments.get("whites"),
        Some(&0.9)
    );

    // Simuliere Schließen via Drop nach erfolgreichem Save — Reload via neuem LuminaApp
    drop(app);
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    // Nach Reload muss die zuletzt erfolgreich persistierte Recipe byte-identisch da sein
    assert_eq!(
        reopened.recipe().adjustments.get("exposure"),
        Some(&1.2),
        "exposure after reload must be last persisted (1.2), not conflict attempt 2.0"
    );
    assert_eq!(reopened.recipe().adjustments.get("contrast"), Some(&0.4));
    assert_eq!(
        reopened.recipe().adjustments.get("whites"),
        Some(&0.9),
        "external whites must be visible after reload"
    );
    // Ungültige Conflict-Änderung (2.0) darf nicht wieder auftauchen
    assert_ne!(
        reopened.recipe().adjustments.get("exposure"),
        Some(&2.0),
        "conflicted unsaved edit must not leak into reload"
    );
    // Negativ-Test: ohne persistierten Sidecar wäre Reload leer — belege dass
    // der positive Pfad wirklich aus dem Sidecar kam (kein In-Memory-Carry).
    let sidecar_json = std::fs::read_to_string(&sidecar_path).unwrap();
    assert!(
        sidecar_json.contains("\"exposure\"") && sidecar_json.contains("1.2"),
        "sidecar JSON must contain persisted exposure"
    );
}
