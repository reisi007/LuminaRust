//! AGENT-HARNESS-3 Develop-Slider-Klassen: je Klasse ein
//! Edit → Commit → Sidecar → Reload-Nachweis (DoD §1, Klassen vollständig
//! nach DoD §3 — keine Stichprobe).
//!
//! F-100-Reihenfolge: globale Tonwerte → Kurve → HSL → Color Grading →
//! Präsenz → Dynamik/Sättigung → Schärfen → Rauschreduzierung →
//! Vignette/Körnung → Objektivkorrektur → Perspektive → Crop/Geometrie.
//!
//! Harness-Regel: nur LESEND/nutzend aufrufen, nie `crates/` schreiben.
//! Öffentlich setzbare Klassen werden als volle Kette geprüft (PASS/FAIL);
//! Klassen mit privatem Setter erhalten ein ehrliches OPEN-Verdict mit der
//! Folgeaufgabe (Datei + Zeile + Signatur) statt eines Workarounds.

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

fn sidecar_for(source: &std::path::Path) -> std::path::PathBuf {
    source.with_extension("png.lumina.json")
}

fn settle(probe: &mut GuiProbe) {
    probe.run_steps(15);
    probe.wait_quiescent(60, 6);
}

#[test]
#[ignore = "headless GPU required; run: cargo test --test develop_sliders -- --ignored"]
fn develop_slider_classes() {
    let dir = artifact_dir("develop_sliders");
    let workdir = tempfile::tempdir().expect("temp dir");
    let source = workdir.path().join("photo.png");
    std::fs::write(&source, lumina_gui::LuminaApp::sample_image_png()).unwrap();

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

    // Klasse 1: globale Tonwerte via set_adjustment (alle 6 Mitglieder).
    for (key, value) in [
        ("exposure", 1.5),
        ("contrast", 0.4),
        ("highlights", -0.3),
        ("shadows", 0.2),
        ("whites", 0.1),
        ("blacks", -0.1),
    ] {
        probe.app_mut().set_adjustment(key, value);
        settle(&mut probe);
        let session = probe.app().recipe().adjustments.get(key).copied();
        let file = recipe_value(&sidecar_for(&source), &format!("/adjustments/{key}"))
            .and_then(|v| v.as_f64());
        match (session, file) {
            (Some(s), Some(f)) if (s - value).abs() < 1e-9 && (f - value).abs() < 1e-6 => {
                verdicts.push(Verdict::pass(
                    format!("tone {key} persists"),
                    format!("session={s} sidecar={f}"),
                ));
            }
            other => verdicts.push(Verdict::fail(
                format!("tone {key} persists"),
                format!("expected {value}, got session+file={other:?}"),
            )),
        }
    }

    // Klasse 5: Präsenz via set_presence (alle 3 Mitglieder).
    for (field, value) in [("texture", 0.5), ("clarity", -0.3), ("dehaze", 0.2)] {
        probe.app_mut().set_presence(field, value);
        settle(&mut probe);
        let session = probe.app().recipe().presence.as_ref().map(|p| match field {
            "texture" => f64::from(p.texture),
            "clarity" => f64::from(p.clarity),
            _ => f64::from(p.dehaze),
        });
        let file = recipe_value(
            &sidecar_for(&source),
            &format!("/adjustments/presence/{field}"),
        )
        .and_then(|v| v.as_f64());
        match (session, file) {
            (Some(s), Some(f)) if (s - value).abs() < 1e-6 && (f - value).abs() < 1e-6 => {
                verdicts.push(Verdict::pass(
                    format!("presence {field} persists"),
                    format!("session={s} sidecar={f}"),
                ));
            }
            other => verdicts.push(Verdict::fail(
                format!("presence {field} persists"),
                format!("expected {value}, got session+file={other:?}"),
            )),
        }
    }

    // Klasse 6: Dynamik/Sättigung via set_adjustment.
    for (key, value) in [("vibrance", 0.3), ("saturation", -0.2)] {
        probe.app_mut().set_adjustment(key, value);
        settle(&mut probe);
        let session = probe.app().recipe().adjustments.get(key).copied();
        let file = recipe_value(&sidecar_for(&source), &format!("/adjustments/{key}"))
            .and_then(|v| v.as_f64());
        match (session, file) {
            (Some(s), Some(f)) if (s - value).abs() < 1e-9 && (f - value).abs() < 1e-6 => {
                verdicts.push(Verdict::pass(
                    format!("color {key} persists"),
                    format!("session={s} sidecar={f}"),
                ));
            }
            other => verdicts.push(Verdict::fail(
                format!("color {key} persists"),
                format!("expected {value}, got session+file={other:?}"),
            )),
        }
    }

    // Weißabgleich flach via set_adjustment (öffentlicher Pfad neben Pick).
    probe.app_mut().set_adjustment("wb_temperature", 5500.0);
    settle(&mut probe);
    let file =
        recipe_value(&sidecar_for(&source), "/adjustments/wb_temperature").and_then(|v| v.as_f64());
    match file {
        Some(f) if (f - 5500.0).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "wb_temperature persists",
            format!("sidecar={f}"),
        )),
        other => verdicts.push(Verdict::fail(
            "wb_temperature persists",
            format!("expected 5500, got {other:?}"),
        )),
    }

    // Klasse 12 (Geometrie, öffentlich): Rotation + Spiegelung.
    probe.app_mut().set_geometry_rotation(15.0);
    settle(&mut probe);
    let rot = probe
        .app()
        .recipe()
        .geometry
        .as_ref()
        .map(|g| f64::from(g.rotation_degrees));
    let file_rot =
        recipe_value(&sidecar_for(&source), "/geometry/rotation_degrees").and_then(|v| v.as_f64());
    match (rot, file_rot) {
        (Some(s), Some(f)) if (s - 15.0).abs() < 1e-6 && (f - 15.0).abs() < 1e-6 => {
            verdicts.push(Verdict::pass(
                "geometry rotation persists",
                format!("session={s} sidecar={f}"),
            ));
        }
        other => verdicts.push(Verdict::fail(
            "geometry rotation persists",
            format!("expected 15, got {other:?}"),
        )),
    }
    probe.app_mut().set_geometry_mirror(true, false);
    settle(&mut probe);
    let mirror_h = probe
        .app()
        .recipe()
        .geometry
        .as_ref()
        .map(|g| g.mirror_horizontal);
    let file_mirror = recipe_value(&sidecar_for(&source), "/geometry/mirror_horizontal")
        .and_then(|v| v.as_bool());
    match (mirror_h, file_mirror) {
        (Some(true), Some(true)) => verdicts.push(Verdict::pass(
            "geometry mirror persists",
            "horizontal=true in session + sidecar".to_string(),
        )),
        other => verdicts.push(Verdict::fail(
            "geometry mirror persists",
            format!("expected true/true, got {other:?}"),
        )),
    }

    // Einzel-Reset ist ein Commit wie ein Drag (öffentlich).
    probe.app_mut().reset_single_adjustment("exposure");
    settle(&mut probe);
    let reset_val = probe.app().recipe().adjustments.get("exposure").copied();
    match reset_val {
        Some(v) if v.abs() < 1e-12 => verdicts.push(Verdict::pass(
            "single-slider reset commits default",
            format!("exposure={v}"),
        )),
        other => verdicts.push(Verdict::fail(
            "single-slider reset commits default",
            format!("expected 0.0, got {other:?}"),
        )),
    }

    // Reload-Link (DoD §1): frische Probe öffnet dasselbe Sidecar.
    std::fs::copy(sidecar_for(&source), dir.join("sidecar.json")).ok();
    let mut reopened = GuiProbe::new();
    reopened.app_mut().open_file(source.display().to_string());
    reopened.run_steps(80);
    reopened.wait_quiescent(120, 8);
    for (key, expected) in [
        ("contrast", 0.4),
        ("vibrance", 0.3),
        ("saturation", -0.2),
        ("wb_temperature", 5500.0),
    ] {
        let restored = reopened.app().recipe().adjustments.get(key).copied();
        match restored {
            Some(v) if (v - expected).abs() < 1e-6 => verdicts.push(Verdict::pass(
                format!("reload restores {key}"),
                format!("{v}"),
            )),
            other => verdicts.push(Verdict::fail(
                format!("reload restores {key}"),
                format!("expected {expected}, got {other:?}"),
            )),
        }
    }
    let restored_tex = reopened
        .app()
        .recipe()
        .presence
        .as_ref()
        .map(|p| f64::from(p.texture));
    match restored_tex {
        Some(v) if (v - 0.5).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "reload restores presence.texture",
            format!("{v}"),
        )),
        other => verdicts.push(Verdict::fail(
            "reload restores presence.texture",
            format!("expected 0.5, got {other:?}"),
        )),
    }
    let restored_rot = reopened
        .app()
        .recipe()
        .geometry
        .as_ref()
        .map(|g| f64::from(g.rotation_degrees));
    match restored_rot {
        Some(v) if (v - 15.0).abs() < 1e-6 => verdicts.push(Verdict::pass(
            "reload restores geometry.rotation",
            format!("{v}"),
        )),
        other => verdicts.push(Verdict::fail(
            "reload restores geometry.rotation",
            format!("expected 15, got {other:?}"),
        )),
    }

    // Klassen ohne öffentlichen Setter: ehrlich OPEN + Folgeaufgabe
    // (kein Workaround, keine direkte Rezept-Mutation harness-seitig).
    for (class, detail) in [
        (
            "tone curve",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_tone_curve_region(&mut self, region: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4195 (derzeit privat `fn`)",
        ),
        (
            "HSL mixer",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_hsl_value(&mut self, channel: &str, field: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4220 (derzeit privat `fn`)",
        ),
        (
            "color grading",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_color_grading_value(&mut self, range: &str, field: &str, value: f64)` (:4250) + `pub fn set_color_grading_balance(&mut self, value: f64)` (:4290), derzeit privat",
        ),
        (
            "effects (vignette/grain)",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_effects_value(&mut self, group: &str, field: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4315 (derzeit privat `fn`)",
        ),
        (
            "sharpening",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_sharpening_value(&mut self, field: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4383 (derzeit privat `fn`)",
        ),
        (
            "noise reduction",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_noise_reduction_value(&mut self, field: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4407 (derzeit privat `fn`)",
        ),
        (
            "lens correction",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_lens_correction_value(&mut self, field: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4427 (derzeit privat `fn`)",
        ),
        (
            "perspective",
            "kein öffentlicher Setter; Folgeaufgabe: `pub fn set_perspective_value(&mut self, field: &str, value: f64)` in crates/lumina-gui/src/lib.rs:4524 (derzeit privat `fn`)",
        ),
        (
            "crop",
            "kein öffentlicher Crop-Setter auffindbar (nur `toggle_crop_mode` öffentlich); Folgeaufgabe: öffentlicher Crop-Edit-Pfad in crates/lumina-gui/src/lib.rs definieren (Geometrie-Sektion)",
        ),
    ] {
        verdicts.push(Verdict::open(
            format!("{class}: Edit→Commit→Sidecar→Reload unbelegbar"),
            detail.to_string(),
        ));
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
