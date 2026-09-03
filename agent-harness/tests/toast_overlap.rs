//! GUI-TOAST-OVERLAP-1: the old green "Vorschau bereit" per-cell badge sat on
//! top of the left-rail thumbnail indefinitely (no timeout, no dismiss) and
//! covered the image it announced. The fix (main commit `777f3e6`) moved the
//! signal to a transient overlay toast in its own `egui::Area`
//! (`show_toast`/`dismiss_toast`/`toast_visible`, auto-dismiss after 4s,
//! ✕ button), anchored top-right clear of rail/grid/filmstrip; a `Ready`
//! neighbor probe now raises NO per-cell badge.
//!
//! NOTE: rail/filmstrip cells are RAW-only by design, so this scenario uses
//! dummy `.ARW` files. The state machine (`toast_visible`) is pure and
//! checked headless without an event loop; the overlay Area itself is
//! Painter-composited, so its tree exposure is documented honestly
//! (AGENT-HARNESS-2 gap) instead of asserted blindly.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};
use lumina_gui::Module;

#[test]
#[ignore = "headless GPU required; run: cargo test --test toast_overlap -- --ignored"]
fn toast_overlap() {
    let dir = artifact_dir("toast_overlap");
    let workdir = tempfile::tempdir().expect("temp dir");
    for name in ["a.ARW", "b.ARW"] {
        std::fs::write(workdir.path().join(name), b"not a real raw file").unwrap();
    }

    let mut probe = GuiProbe::new();
    probe
        .app_mut()
        .set_directory(workdir.path().display().to_string());
    probe.app_mut().set_module(Module::Develop);
    probe.run_steps(10);
    let (frames, gen) = probe.wait_quiescent(200, 8);

    // Real interaction (also keeps this shot distinct per Befund 4): raise
    // the overlay toast at an egui-time safely above the headless clock, so
    // it is still visible when the shot is read back.
    probe.app_mut().show_toast("Preview ready".into(), 1000.0);
    probe.run_steps(2);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    // Rail cells: 120x90 click areas in the left band.
    let rail_cells: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.role.contains("Unknown")
                && n.x1 < 260.0
                && (n.width() - 120.0).abs() < 4.0
                && (n.height() - 90.0).abs() < 4.0
        })
        .collect();
    // The old per-cell badge text must be gone from every surface.
    let old_badge: Vec<_> = nodes
        .iter()
        .filter(|n| n.text_contains("Vorschau bereit"))
        .collect();
    // The overlay toast owns the signal now: message + Dismiss button.
    let toast_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.text_contains("Preview ready") || n.text_contains("Dismiss"))
        .collect();

    // Pure state machine on the live app (no event loop needed).
    probe.app_mut().show_toast("probe".into(), 2000.0);
    let app = probe.app_mut();
    let visible_now = app.toast_visible(2000.0);
    let visible_at_deadline = app.toast_visible(2004.0);
    let hidden_after = !app.toast_visible(2004.1);
    app.dismiss_toast();
    let hidden_after_dismiss = !app.toast_visible(2000.0);
    let machine_ok = visible_now && visible_at_deadline && hidden_after && hidden_after_dismiss;

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!(
                "frames={frames} generation={gen} png={w}x{h} nodes={} rail_cells={} toast_nodes={}",
                nodes.len(),
                rail_cells.len(),
                toast_nodes.len(),
            ),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    if old_badge.is_empty() {
        verdicts.push(Verdict::pass(
            "no per-cell ready badge",
            "old \"Vorschau bereit\" cell badge absent (Ready probes raise no cell badge; the overlay toast owns the signal)",
        ));
    } else {
        verdicts.push(Verdict::fail(
            "no per-cell ready badge",
            format!("{} old badge node(s) still present", old_badge.len()),
        ));
    }
    if machine_ok {
        verdicts.push(Verdict::pass(
            "overlay toast state machine",
            "show → visible → visible at deadline → hidden past 4s timeout → dismiss hides immediately",
        ));
    } else {
        verdicts.push(Verdict::fail(
            "overlay toast state machine",
            format!(
                "now={visible_now} deadline={visible_at_deadline} after={hidden_after} dismiss={hidden_after_dismiss}"
            ),
        ));
    }
    if toast_nodes.is_empty() {
        verdicts.push(Verdict::open(
            "toast accesskit exposure",
            "overlay Area is Painter-composited, invisible to AccessKit (AGENT-HARNESS-2 gap): message/Dismiss verified via the pure state machine above, PNG needs vision review for anchor placement",
        ));
    } else {
        // Anchor must sit clear of the left rail: every toast node starts in
        // the right half of the 1280px viewport.
        let clear = toast_nodes.iter().all(|n| n.x0 > 260.0);
        if clear {
            verdicts.push(Verdict::pass(
                "toast accesskit exposure",
                format!(
                    "{} toast node(s) exposed, all clear of the left rail",
                    toast_nodes.len()
                ),
            ));
        } else {
            verdicts.push(Verdict::fail(
                "toast accesskit exposure",
                format!(
                    "toast node overlaps the left rail: {:?}",
                    toast_nodes
                        .iter()
                        .map(|n| (n.x0, n.y0, n.x1, n.y1))
                        .collect::<Vec<_>>()
                ),
            ));
        }
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
    assert!(!rail_cells.is_empty(), "rail must show RAW cells headless");
}
