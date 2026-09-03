//! AGENT-HARNESS-3 Export-Green-Path: Datei byte-valide, Quelle
//! unangetastet, Determinismus wo behauptet.
//!
//! F-100 SOLL: GUI-Export läuft über die gemeinsame
//! `lumina_core::export_image`-Logik — byte-identisch zum CLI-Export;
//! JPEG fester Qualität deterministisch, PNG primärer Byte-Anker;
//! Originalpfad wird nie überschrieben (nicht-destruktiv).

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test export_greenpath -- --ignored"]
fn export_file_valid() {
    let dir = artifact_dir("export_greenpath");
    let workdir = tempfile::tempdir().expect("temp dir");
    let source = workdir.path().join("photo.png");
    let source_bytes = lumina_gui::LuminaApp::sample_image_png();
    std::fs::write(&source, &source_bytes).unwrap();

    let mut probe = GuiProbe::new();
    probe.app_mut().open_file(source.display().to_string());
    probe.run_steps(80);
    probe.wait_quiescent(120, 8);
    assert!(
        probe.app().preview().is_some(),
        "sample must decode, error={:?}",
        probe.app().error()
    );
    probe.app_mut().set_adjustment("exposure", 0.8);
    probe.run_steps(15);
    probe.wait_quiescent(60, 6);

    let mut verdicts: Vec<Verdict> = Vec::new();
    let out1 = workdir.path().join("export1.png");
    let out2 = workdir.path().join("export2.png");

    match probe.app_mut().export_to(out1.clone()) {
        Ok(()) => {
            if !out1.is_file() {
                verdicts.push(Verdict::fail(
                    "export writes file",
                    "Ok but file missing".to_string(),
                ));
            } else {
                match image::open(&out1) {
                    Ok(img) => verdicts.push(Verdict::pass(
                        "export file byte-valid",
                        format!("readable {}x{}", img.width(), img.height()),
                    )),
                    Err(e) => verdicts.push(Verdict::fail(
                        "export file byte-valid",
                        format!("unreadable: {e}"),
                    )),
                }
            }
        }
        Err(e) => verdicts.push(Verdict::fail("export writes file", e.to_string())),
    }

    // Quelle unangetastet (nicht-destruktiv).
    let after = std::fs::read(&source).unwrap();
    if after == source_bytes {
        verdicts.push(Verdict::pass(
            "export leaves source untouched",
            format!("{} bytes identical", after.len()),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "export leaves source untouched",
            "source bytes changed".to_string(),
        ));
    }

    // Determinismus: zweiter Export derselben Eingabe → gleiche Bytes (PNG).
    match probe.app_mut().export_to(out2.clone()) {
        Ok(()) => {
            let b1 = std::fs::read(&out1).unwrap_or_default();
            let b2 = std::fs::read(&out2).unwrap_or_default();
            if !b1.is_empty() && b1 == b2 {
                verdicts.push(Verdict::pass(
                    "export deterministic (PNG)",
                    format!("{} bytes identical", b1.len()),
                ));
            } else {
                verdicts.push(Verdict::fail(
                    "export deterministic (PNG)",
                    format!("len {} vs {}", b1.len(), b2.len()),
                ));
            }
        }
        Err(e) => verdicts.push(Verdict::fail("second export runs", e.to_string())),
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
