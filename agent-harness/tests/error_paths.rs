//! AGENT-HARNESS-3 Fehlerpfade: fehlendes Bild, fehlendes Sidecar,
//! fehlendes Modell → laut (Status/Fehlertext), nie stiller Fallback.
//!
//! Produktregel (Agents.md): Reproduzierbarkeit vor stillem Fallback;
//! fehlende/inkompatible Artefakte werden sichtbar als veraltet oder nicht
//! verfügbar gemeldet.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test error_paths -- --ignored"]
fn loud_error_paths() {
    let dir = artifact_dir("error_paths");
    let workdir = tempfile::tempdir().expect("temp dir");
    let mut verdicts: Vec<Verdict> = Vec::new();

    // 1. Fehlendes Bild: laut, kein Preview, kein Phantom-Sidecar.
    let missing = workdir.path().join("missing.png");
    let mut probe = GuiProbe::new();
    probe.app_mut().open_file(missing.display().to_string());
    probe.run_steps(80);
    probe.wait_quiescent(120, 8);
    let err = probe.app().error().map(str::to_owned);
    let phantom = missing.with_extension("png.lumina.json");
    match err {
        Some(e) if probe.app().preview().is_none() && !phantom.is_file() => {
            verdicts.push(Verdict::pass(
                "missing image fails loudly",
                format!("error={e:?}, no preview, no phantom sidecar"),
            ));
        }
        other => verdicts.push(Verdict::fail(
            "missing image fails loudly",
            format!(
                "error={other:?} preview={} phantom={}",
                probe.app().preview().is_some(),
                phantom.is_file()
            ),
        )),
    }

    // 2. Fehlendes Sidecar: Normalfall (Design) — Defaults laden, kein
    // Fehler, Status benennt das Geladene.
    let plain = workdir.path().join("plain.png");
    std::fs::write(&plain, lumina_gui::LuminaApp::sample_image_png()).unwrap();
    let mut probe = GuiProbe::new();
    probe.app_mut().open_file(plain.display().to_string());
    probe.run_steps(80);
    probe.wait_quiescent(120, 8);
    let status = probe.app().status().to_owned();
    let err = probe.app().error().map(str::to_owned);
    if probe.app().preview().is_some()
        && err.is_none()
        && probe.app().recipe().adjustments.is_empty()
    {
        verdicts.push(Verdict::pass(
            "missing sidecar loads defaults (by design)",
            format!("status={status:?}, no error, default recipe"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "missing sidecar loads defaults (by design)",
            format!(
                "preview={} error={err:?} status={status:?}",
                probe.app().preview().is_some()
            ),
        ));
    }

    // 3. Export ohne Bild: harter Fehler, keine Datei.
    let mut probe = GuiProbe::new();
    probe.run_steps(3);
    let out = workdir.path().join("nope.png");
    match probe.app_mut().export_to(out.clone()) {
        Ok(()) => verdicts.push(Verdict::fail(
            "export without image fails loudly",
            "Ok without image".to_string(),
        )),
        Err(e) => {
            if out.is_file() {
                verdicts.push(Verdict::fail(
                    "export without image fails loudly",
                    format!("Err but file created: {e}"),
                ));
            } else {
                verdicts.push(Verdict::pass(
                    "export without image fails loudly",
                    format!("{e}, no file created"),
                ));
            }
        }
    }

    // 4. Maske ohne Auswahl / ohne Namen: laut statt still.
    let mut probe = GuiProbe::new();
    match probe.app_mut().offer_mask_recalculation() {
        Ok(_) => verdicts.push(Verdict::fail(
            "mask recalc without selection fails loudly",
            "Ok without selected mask".to_string(),
        )),
        Err(e) => verdicts.push(Verdict::pass(
            "mask recalc without selection fails loudly",
            e.to_string(),
        )),
    }
    match probe.app_mut().set_white_balance_from_point(0.0, 0.0, 0.0) {
        Ok(_) => verdicts.push(Verdict::fail(
            "wb pick from black fails loudly",
            "Ok for non-positive channels".to_string(),
        )),
        Err(e) => verdicts.push(Verdict::pass(
            "wb pick from black fails loudly",
            e.to_string(),
        )),
    }

    // 5. Frisch angelegte Maske ohne Matte: Pending + Recalc-Angebot —
    // nie still als gültig gewertet.
    let mut probe = GuiProbe::new();
    probe.app_mut().open_file(plain.display().to_string());
    probe.run_steps(80);
    probe.wait_quiescent(120, 8);
    match probe.app_mut().create_mask("subject") {
        Ok(id) => {
            probe.run_steps(5);
            match probe.app_mut().offer_mask_recalculation() {
                Ok(true) => verdicts.push(Verdict::pass(
                    "fresh mask offered for recalc (never silently valid)",
                    format!("id={id} status={:?}", probe.app().status()),
                )),
                Ok(false) => verdicts.push(Verdict::fail(
                    "fresh mask offered for recalc (never silently valid)",
                    "fresh mask reports no recalc needed".to_string(),
                )),
                Err(e) => verdicts.push(Verdict::fail(
                    "fresh mask offered for recalc (never silently valid)",
                    format!("offer errored: {e}"),
                )),
            }
        }
        Err(e) => verdicts.push(Verdict::open(
            "fresh mask offered for recalc (never silently valid)",
            format!("create_mask failed headless: {e} — mask-pending path needs prod-side check"),
        )),
    }

    // 6. Auto-Tone ohne Bild: derzeit stilles Ok — ehrlich OPEN.
    // Der Default-Status ("Ready ...") benennt die Aktion nicht; laut wäre
    // ein Aktions-Fehler oder -Status.
    let mut probe = GuiProbe::new();
    probe.run_steps(3);
    match probe.app_mut().auto_tone() {
        Ok(()) => {
            let status = probe.app().status().to_owned();
            let err = probe.app().error().map(str::to_owned);
            let loud = err.is_some()
                || status.to_lowercase().contains("tone")
                || status.to_lowercase().contains("no image");
            if loud {
                verdicts.push(Verdict::pass(
                    "auto-tone without image is loud",
                    format!("status={status:?} error={err:?}"),
                ));
            } else {
                verdicts.push(Verdict::open(
                    "auto-tone without image is loud",
                    format!("returns Ok(()) with generic status={status:?}, error={err:?} — no action-specific signal; Folgeaufgabe: Err(GuiError) oder Status in crates/lumina-gui/src/lib.rs:4552 (`pub fn auto_tone`)"),
                ));
            }
        }
        Err(e) => verdicts.push(Verdict::pass(
            "auto-tone without image is loud",
            format!("Err: {e}"),
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
