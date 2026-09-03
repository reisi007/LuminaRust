//! AGENT-HARNESS-3 Develop-Aktionen: Auto-Tone, Match, WB-Pick, Rotate,
//! Reset, Render/Apply — je Aktion Ansteuerung + Persistenz-Nachweis
//! (Edit → Commit → Sidecar → Reload, DoD §1).

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

fn recipe_value(sidecar: &std::path::Path, pointer: &str) -> Option<serde_json::Value> {
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
    copy.get("recipe")?.pointer(pointer).cloned()
}

fn settle(probe: &mut GuiProbe) {
    probe.run_steps(15);
    probe.wait_quiescent(60, 6);
}

#[test]
#[ignore = "headless GPU required; run: cargo test --test develop_actions -- --ignored"]
fn develop_actions() {
    let dir = artifact_dir("develop_actions");
    let workdir = tempfile::tempdir().expect("temp dir");
    let source = workdir.path().join("photo.png");
    std::fs::write(&source, lumina_gui::LuminaApp::sample_image_png()).unwrap();
    let sidecar = source.with_extension("png.lumina.json");

    let mut probe = GuiProbe::new();
    probe.app_mut().open_file(source.display().to_string());
    probe.run_steps(80);
    probe.wait_quiescent(120, 8);
    assert!(
        probe.app().preview().is_some(),
        "sample must decode, error={:?}",
        probe.app().error()
    );

    let mut verdicts: Vec<Verdict> = Vec::new();

    // Render/Apply: expliziter Re-Render ist Ok und bumpt die Generation.
    let gen_before = probe.app().preview_generation();
    match probe.app_mut().render() {
        Ok(()) => {
            let gen_after = probe.app().preview_generation();
            verdicts.push(Verdict::pass(
                "render/apply",
                format!(
                    "ok, generation {gen_before}->{gen_after}, preview={}",
                    probe.app().preview().is_some()
                ),
            ));
        }
        Err(e) => verdicts.push(Verdict::fail("render/apply", e.to_string())),
    }

    // Auto-Tone: 6 Regler + Spiegel + Sidecar-Save im selben Aufruf.
    match probe.app_mut().auto_tone() {
        Ok(()) => {
            settle(&mut probe);
            let enabled = probe.app().recipe().auto_features.enable_auto_tone;
            let exposure = probe.app().recipe().adjustments.get("exposure").copied();
            let file_flag =
                recipe_value(&sidecar, "/auto_features/enable_auto_tone").and_then(|v| v.as_bool());
            if enabled && file_flag == Some(true) {
                verdicts.push(Verdict::pass(
                    "auto-tone persists",
                    format!("enable_auto_tone=true exposure={exposure:?}"),
                ));
            } else {
                verdicts.push(Verdict::fail(
                    "auto-tone persists",
                    format!("session enabled={enabled} file={file_flag:?}"),
                ));
            }
        }
        Err(e) => verdicts.push(Verdict::fail("auto-tone runs", e.to_string())),
    }

    // Match Total Exposure gegen explizites Ziel.
    match probe.app_mut().match_total_exposure(0.5) {
        Ok(()) => {
            settle(&mut probe);
            let matched = probe.app().recipe().auto_features.matched_exposure;
            let file_matched =
                recipe_value(&sidecar, "/auto_features/matched_exposure").and_then(|v| v.as_f64());
            match (matched, file_matched) {
                (Some(s), Some(f)) => verdicts.push(Verdict::pass(
                    "match total exposure persists",
                    format!("session delta={s} file delta={f}"),
                )),
                other => verdicts.push(Verdict::fail(
                    "match total exposure persists",
                    format!("expected Some/Some, got {other:?}"),
                )),
            }
        }
        Err(e) => verdicts.push(Verdict::fail("match total exposure runs", e.to_string())),
    }

    // WB-Pick: deterministische Ableitung aus einem Punkt.
    match probe
        .app_mut()
        .set_white_balance_from_point(200.0, 180.0, 160.0)
    {
        Ok(()) => {
            settle(&mut probe);
            let temp = probe
                .app()
                .recipe()
                .adjustments
                .get("wb_temperature")
                .copied();
            match temp {
                Some(t) if (1500.0..=12000.0).contains(&t) => {
                    verdicts.push(Verdict::pass("wb pick sets temperature", format!("{t}")));
                }
                other => verdicts.push(Verdict::fail(
                    "wb pick sets temperature",
                    format!("out of domain: {other:?}"),
                )),
            }
            // WB-Pick läuft über mark_dirty ohne eigenen Commit-Hook-Verdacht:
            // Sidecar-Nachweis wie bei Slidern.
            let file_temp =
                recipe_value(&sidecar, "/adjustments/wb_temperature").and_then(|v| v.as_f64());
            match (temp, file_temp) {
                (Some(s), Some(f)) if (s - f).abs() < 1e-6 => verdicts.push(Verdict::pass(
                    "wb pick persists",
                    format!("session={s} sidecar={f}"),
                )),
                other => verdicts.push(Verdict::fail(
                    "wb pick persists",
                    format!("session+file={other:?}"),
                )),
            }
        }
        Err(e) => verdicts.push(Verdict::fail("wb pick runs", e.to_string())),
    }

    // Rotate: ±90°-Schritte teilen sich einen Save-Pfad mit dem Slider.
    probe.app_mut().set_geometry_rotation(0.0);
    settle(&mut probe);
    probe.app_mut().rotate_step(90.0);
    settle(&mut probe);
    let rot = probe
        .app()
        .recipe()
        .geometry
        .as_ref()
        .map(|g| f64::from(g.rotation_degrees));
    match rot {
        Some(v) if (v - 90.0).abs() < 1e-6 => {
            verdicts.push(Verdict::pass("rotate_step +90", format!("{v}")))
        }
        other => verdicts.push(Verdict::fail("rotate_step +90", format!("got {other:?}"))),
    }
    // Normalisierung in (-180, 180]: 90 + 200 = 290 → -70.
    probe.app_mut().rotate_step(200.0);
    settle(&mut probe);
    let rot = probe
        .app()
        .recipe()
        .geometry
        .as_ref()
        .map(|g| f64::from(g.rotation_degrees));
    match rot {
        Some(v) if (v - -70.0).abs() < 1e-6 => {
            verdicts.push(Verdict::pass("rotate_step normalizes", format!("{v}")))
        }
        other => verdicts.push(Verdict::fail(
            "rotate_step normalizes",
            format!("expected -70, got {other:?}"),
        )),
    }

    // Reset: Rezept wird ersetzt; der nächste Commit persistiert den
    // Reset-Stand (alter Wert weg, neuer Wert da) — Reload-Nachweis.
    probe.app_mut().set_adjustment("exposure", 1.5);
    settle(&mut probe);
    probe.app_mut().reset();
    let cleared = probe
        .app()
        .recipe()
        .adjustments
        .get("exposure")
        .copied()
        .unwrap_or(0.0);
    if cleared.abs() < 1e-12 {
        verdicts.push(Verdict::pass(
            "reset clears in session",
            format!("exposure={cleared}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "reset clears in session",
            format!("expected 0, got {cleared}"),
        ));
    }
    probe.app_mut().set_adjustment("contrast", 0.3);
    settle(&mut probe);
    let mut reopened = GuiProbe::new();
    reopened.app_mut().open_file(source.display().to_string());
    reopened.run_steps(80);
    reopened.wait_quiescent(120, 8);
    let re_exp = reopened
        .app()
        .recipe()
        .adjustments
        .get("exposure")
        .copied()
        .unwrap_or(0.0);
    let re_con = reopened.app().recipe().adjustments.get("contrast").copied();
    match (re_exp.abs() < 1e-9, re_con) {
        (true, Some(c)) if (c - 0.3).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "reset persists via next commit",
            format!("exposure={re_exp} contrast={c}"),
        )),
        other => verdicts.push(Verdict::fail(
            "reset persists via next commit",
            format!("got {other:?}"),
        )),
    }

    std::fs::copy(&sidecar, dir.join("sidecar.json")).ok();
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
