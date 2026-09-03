//! AGENT-HARNESS-3 Sync/Match-Selection: N Sidecars, Fehler isoliert laut.
//!
//! F-100 SOLL: Sync Settings wendet das Rezept der aktiven Kopie auf alle
//! ausgewählten Bilder an (je eigenes Sidecar, Fehler einzeln laut);
//! Match Total Exposures gleicht die Belichtung über die Auswahl an
//! (Core-`match_total_exposure` je Bild gegen Auswahl-Median).
//!
//! Auswahl über Plain/Toggle auf echten (kopierten) RAW-Fixtures — dieselbe
//! Order, die auch das Filmstrip-Rendering trägt. Der Hintergrund-Decode
//! wird wie in `library_greenpath` Phase B bewusst nicht gestartet
//! (headless-Texture-Cap); Sync/Match dekodieren ihre Ziele selbst synchron
//! (CPU, keine Textur). Original-Fixtures werden nur gelesen.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

fn recipe_pointer(source: &std::path::Path, pointer: &str) -> Option<serde_json::Value> {
    let stem = source.file_name()?.to_str()?;
    let sidecar = source.with_file_name(format!("{stem}.lumina.json"));
    let text = std::fs::read_to_string(sidecar).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let copies = v.get("virtual_copies")?.as_array()?;
    let copy = copies
        .iter()
        .find(|c| {
            c.get("is_default")
                .and_then(|b| b.as_bool())
                .unwrap_or(false)
        })
        .or_else(|| copies.first())?;
    copy.pointer(&format!("/recipe{pointer}")).cloned()
}

fn f64_at(source: &std::path::Path, pointer: &str) -> Option<f64> {
    recipe_pointer(source, pointer).and_then(|v| v.as_f64())
}

fn bool_at(source: &std::path::Path, pointer: &str) -> Option<bool> {
    recipe_pointer(source, pointer).and_then(|v| v.as_bool())
}

fn settle(probe: &mut GuiProbe) {
    probe.run_steps(15);
    probe.wait_quiescent(60, 6);
}

#[test]
#[ignore = "headless GPU required; run: cargo test --test sync_match -- --ignored"]
fn sync_match_selection() {
    let dir = artifact_dir("sync_match");
    let workdir = tempfile::tempdir().expect("temp dir");
    for (src, dst) in [
        ("aircraft-landscape.cr3", "s_A.cr3"),
        ("aircraft-portrait.cr3", "s_B.cr3"),
        ("aircraft-landscape.cr3", "s_C.cr3"),
        ("aircraft-portrait.cr3", "s_D.cr3"),
    ] {
        std::fs::copy(
            std::path::PathBuf::from("../sample-data/raw").join(src),
            workdir.path().join(dst),
        )
        .unwrap();
    }
    let paths: Vec<std::path::PathBuf> = ["s_A.cr3", "s_B.cr3", "s_C.cr3", "s_D.cr3"]
        .iter()
        .map(|n| workdir.path().join(n))
        .collect();
    let display: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();

    let mut probe = GuiProbe::new();
    let tiny = workdir.path().join("tiny.png");
    std::fs::write(&tiny, lumina_gui::LuminaApp::sample_image_png()).unwrap();
    probe.app_mut().open_file(tiny.display().to_string());
    probe.run_steps(80);
    probe.wait_quiescent(120, 8);
    assert!(
        probe.app().preview().is_some(),
        "tiny png must decode, error={:?}",
        probe.app().error()
    );
    probe
        .app_mut()
        .set_directory(workdir.path().display().to_string());
    probe.run_steps(5);

    let mut verdicts: Vec<Verdict> = Vec::new();

    // Auswahl {A, B}: Plain-Click setzt exakt, Toggle fügt hinzu.
    // (Die tiny.png-Auswahl bleibt nach dem Verzeichniswechsel bestehen,
    // weil sie weiter gelistet ist — daher explizites Zurücksetzen.)
    probe
        .app_mut()
        .select_filmstrip_path(display[0].clone(), false, false);
    probe
        .app_mut()
        .select_filmstrip_path(display[1].clone(), true, false);
    let selection = probe.app().filmstrip_selection();
    if selection == display[0..2] {
        verdicts.push(Verdict::pass(
            "selection holds N images",
            format!("{selection:?}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "selection holds N images",
            format!("expected {:?}, got {selection:?}", &display[0..2]),
        ));
    }

    // Sync#1: aktives Rezept (exposure 0.5) auf beide Sidecars.
    probe.app_mut().set_adjustment("exposure", 0.5);
    settle(&mut probe);
    let report = probe.app_mut().sync_settings_to_selection();
    if report.applied_count() == 2 && report.failed_count() == 0 {
        verdicts.push(Verdict::pass(
            "sync applies to all",
            format!(
                "applied={} status={:?}",
                report.applied_count(),
                probe.app().status()
            ),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "sync applies to all",
            format!("applied={:?} failed={:?}", report.applied, report.failed),
        ));
    }
    let exposures = [
        f64_at(&paths[0], "/adjustments/exposure"),
        f64_at(&paths[1], "/adjustments/exposure"),
    ];
    if exposures
        .iter()
        .all(|e| matches!(e, Some(v) if (v - 0.5).abs() < 1e-6))
    {
        verdicts.push(Verdict::pass(
            "sync sidecars carry recipe",
            format!("{exposures:?}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "sync sidecars carry recipe",
            format!("expected all 0.5, got {exposures:?}"),
        ));
    }

    // Match#1 über {A, B}: Median-Angleich, beide getaggt.
    let report = probe.app_mut().match_exposures_of_selection();
    if report.applied_count() == 2 && report.failed_count() == 0 {
        verdicts.push(Verdict::pass(
            "match applies to all",
            format!(
                "applied={:?} status={:?}",
                report.applied,
                probe.app().status()
            ),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "match applies to all",
            format!("applied={:?} failed={:?}", report.applied, report.failed),
        ));
    }
    let flags = [
        bool_at(&paths[0], "/auto_features/match_total_exposure"),
        bool_at(&paths[1], "/auto_features/match_total_exposure"),
    ];
    if flags == [Some(true), Some(true)] {
        verdicts.push(Verdict::pass("match tags sidecars", format!("{flags:?}")));
    } else {
        verdicts.push(Verdict::fail(
            "match tags sidecars",
            format!("expected [true, true], got {flags:?}"),
        ));
    }

    // Befund: Sync-Wiederholung über identische Ziele scheitert LAUT an
    // doppelten History-IDs (`sync-0`) — Sync ist nicht idempotent.
    // Laut, aber eine legitime Wiederholung scheitert (Folgebefund Prod).
    let report = probe.app_mut().sync_settings_to_selection();
    let loud = probe.app().error().map(str::to_owned);
    if report.failed_count() == 2
        && loud
            .as_deref()
            .is_some_and(|e| e.contains("duplicate history entry id"))
    {
        verdicts.push(Verdict::pass(
            "repeat sync fails loudly (non-idempotent history ids — Befund)",
            format!("failed={:?}", report.failed),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "repeat sync fails loudly (non-idempotent history ids — Befund)",
            format!(
                "applied={:?} failed={:?} error={loud:?}",
                report.applied, report.failed
            ),
        ));
    }

    // Fehlerisolation auf frischen Zielen {C, D}: D löschen → C applied,
    // D scheitert LAUT, Rest läuft weiter.
    probe
        .app_mut()
        .select_filmstrip_path(display[2].clone(), false, false);
    probe
        .app_mut()
        .select_filmstrip_path(display[3].clone(), true, false);
    std::fs::remove_file(&paths[3]).unwrap();
    probe.app_mut().set_adjustment("exposure", 0.9);
    settle(&mut probe);
    let report = probe.app_mut().sync_settings_to_selection();
    let loud = probe.app().error().map(str::to_owned);
    if report.applied == vec![display[2].clone()]
        && report.failed.len() == 1
        && report.failed[0].0 == display[3]
        && loud.is_some()
    {
        verdicts.push(Verdict::pass(
            "sync failure isolated and loud",
            format!("failed={:?} error={loud:?}", report.failed),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "sync failure isolated and loud",
            format!(
                "applied={:?} failed={:?} error={loud:?}",
                report.applied, report.failed
            ),
        ));
    }
    match f64_at(&paths[2], "/adjustments/exposure") {
        Some(v) if (v - 0.9).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "sync survivor carries recipe",
            format!("{v}"),
        )),
        other => verdicts.push(Verdict::fail(
            "sync survivor carries recipe",
            format!("expected 0.9, got {other:?}"),
        )),
    }

    // Match#2 über {C, D}: C applied+getaggt, D laut.
    let report = probe.app_mut().match_exposures_of_selection();
    let loud = probe.app().error().map(str::to_owned);
    if report.applied == vec![display[2].clone()]
        && report.failed.len() == 1
        && report.failed[0].0 == display[3]
        && loud.is_some()
    {
        verdicts.push(Verdict::pass(
            "match failure isolated and loud",
            format!(
                "applied={:?} failed={:?} error={loud:?}",
                report.applied, report.failed
            ),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "match failure isolated and loud",
            format!(
                "applied={:?} failed={:?} error={loud:?}",
                report.applied, report.failed
            ),
        ));
    }
    match bool_at(&paths[2], "/auto_features/match_total_exposure") {
        Some(true) => verdicts.push(Verdict::pass(
            "match tags survivor",
            "s_C carries match_total_exposure=true".to_string(),
        )),
        other => verdicts.push(Verdict::fail(
            "match tags survivor",
            format!("expected true, got {other:?}"),
        )),
    }

    let shot = dir.join("shot.png");
    probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    verdicts.insert(
        0,
        Verdict::pass("harness ran", format!("shot={}", shot.display())),
    );
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
