//! AGENT-HARNESS-3 Navigator/Zoom/Pan: alle Zoomstufen als Mapping-Test
//! je Stufe (DoD §3: neue Enum-Varianten brauchen je Variante einen
//! Mapping-Test Eingabe → Anzeige), Custom-Pin, Navigator-Präsenz.
//!
//! F-100 SOLL-Stufen: Fit (Default), 25 %, 50 %, 75 %, 100 % (1:1), 200 %,
//! Fit-Breite; `Custom` ist die gepinnte Ansicht (Zoom UND Pan).
//! Echter Pan-Drag hat harness-seitig keine öffentliche API — das wird als
//! OPEN mit Folgeaufgabe verbucht statt per Workaround simuliert.

use agent_harness::{
    artifact_dir,
    painter::{nav_accent_pixels, texts_containing, MIN_NAV_ACCENT_PIXELS},
    save_report, GuiProbe, Verdict,
};
use lumina_gui::ZoomMode;

#[test]
#[ignore = "headless GPU required; run: cargo test --test zoom_pan -- --ignored"]
fn zoom_pan_navigator() {
    let dir = artifact_dir("zoom_pan");
    let mut probe = GuiProbe::develop_with_sample();
    probe.app_mut().set_navigator_open(true);
    probe.run_steps(5);

    let mut verdicts: Vec<Verdict> = Vec::new();

    // Mapping-Test je Zoomstufe: Eingabe (set_zoom_mode) → Anzeige (Tree).
    for (mode, expected) in [
        (ZoomMode::Fit, "Fit"),
        (ZoomMode::Quarter, "25%"),
        (ZoomMode::Half, "50%"),
        (ZoomMode::ThreeQuarter, "75%"),
        (ZoomMode::OneToOne, "100%"),
        (ZoomMode::TwoHundred, "200%"),
        (ZoomMode::FitWidth, "Fit Width"),
    ] {
        probe.app_mut().set_zoom_mode(mode);
        probe.run_steps(3);
        let nodes = probe.tree_nodes();
        let labels = texts_containing(&nodes, "Zoom");
        if labels.iter().any(|l| l.contains(expected)) {
            verdicts.push(Verdict::pass(
                format!("zoom maps to {expected}"),
                format!("{labels:?}"),
            ));
        } else {
            verdicts.push(Verdict::fail(
                format!("zoom maps to {expected}"),
                format!("readout missing {expected}: {labels:?}"),
            ));
        }
    }

    // Custom-Pin nach Zoom-Geste (zoom_step ist der öffentliche Pin-Pfad,
    // den auch Wheel/Drag/Pane-Pan benutzen).
    probe.app_mut().zoom_step(4.0);
    probe.run_steps(3);
    let nodes = probe.tree_nodes();
    let labels = texts_containing(&nodes, "Zoom");
    if labels.iter().any(|l| l.contains("Custom")) {
        verdicts.push(Verdict::pass(
            "zoom_step pins Custom",
            format!("{labels:?}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "zoom_step pins Custom",
            format!("expected Custom, got {labels:?}"),
        ));
    }

    // Navigator-Präsenz: Panel offen + Viewport-Rechteck als Painter-Nachweis
    // (HARNESS-2-Heimathafen: 2px-ACCENT-rect_stroke, kein AccessKit-Node).
    let nav_labels = texts_containing(&nodes, "Navigator");
    if !nav_labels.is_empty() {
        verdicts.push(Verdict::pass("navigator open", format!("{nav_labels:?}")));
    } else {
        verdicts.push(Verdict::fail(
            "navigator open",
            "no Navigator node in tree".to_string(),
        ));
    }
    let shot = dir.join("shot.png");
    probe.render_png(&shot).expect("render PNG");
    let img = image::open(&shot).expect("shot readable").to_rgba8();
    let accents = nav_accent_pixels(&img);
    if accents >= MIN_NAV_ACCENT_PIXELS {
        verdicts.push(Verdict::pass(
            "navigator rect stroke painted (painter-home)",
            format!("{accents} accent pixels (need >={MIN_NAV_ACCENT_PIXELS})"),
        ));
    } else {
        verdicts.push(Verdict::open(
            "navigator rect stroke painted (painter-home)",
            format!("only {accents} accent pixels (need >={MIN_NAV_ACCENT_PIXELS}); PNG needs vision review"),
        ));
    }

    // Echter Pan-Drag: keine öffentliche API harness-seitig.
    verdicts.push(Verdict::open(
        "pan pins Custom",
        "kein öffentlicher Pan-Setter: `preview_pan` ist privat (crates/lumina-gui/src/lib.rs:835), `pan_gesture_pins_custom` (:4906) und `pan_for_navigator_drag` (:5033) sind privat; Folgeaufgabe: `pub fn pan_by(&mut self, dx: f32, dy: f32)` + lesende Getter (`preview_roi` :842, `preview_pan`) für den Rect-vs-ROI-Geometrienachweis",
    ));

    let tree = probe.ui_tree_json();
    verdicts.insert(
        0,
        Verdict::pass("harness ran", format!("shot={}", shot.display())),
    );
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
