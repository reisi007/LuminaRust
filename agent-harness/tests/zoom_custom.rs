//! GUI-ZOOM-CUSTOM-1: right after loading, the zoom readout must say `Fit`
//! (SOLL default); `Custom` only after pan/zoom.
//!
//! Headless check: AccessKit nodes mentioning "Zoom" must name Fit, not Custom.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test zoom_custom -- --ignored"]
fn zoom_custom() {
    let dir = artifact_dir("zoom_custom");
    let mut probe = GuiProbe::develop_with_sample();
    // Real interactions (also keep this shot distinct per Befund 4):
    // navigator opened for zoom context + History expanded for right-panel
    // context (right_thumb expands Presets instead, so the two shots never
    // coincide without reason). The readout must still say Fit — only an
    // actual zoom-in may pin Custom (Custom-gate, main commit 777f3e6).
    probe.app_mut().set_navigator_open(true);
    probe.click_label_contains("History");
    probe.run_steps(2);
    let (frames, gen) = probe.wait_quiescent(120, 8);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    let zoom_nodes: Vec<_> = nodes.iter().filter(|n| n.text_contains("Zoom")).collect();
    let texts: Vec<_> = zoom_nodes
        .iter()
        .map(|n| {
            if n.label.is_empty() {
                n.value.clone()
            } else if n.value.is_empty() {
                n.label.clone()
            } else {
                format!("{} | {}", n.label, n.value)
            }
        })
        .collect();
    let shows_fit = texts.iter().any(|l| l.contains("Fit"));
    let shows_custom = texts.iter().any(|l| l.contains("Custom"));

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!(
                "frames={frames} generation={gen} png={w}x{h} nodes={}",
                nodes.len()
            ),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    if zoom_nodes.is_empty() {
        verdicts.push(Verdict::open(
            "zoom default Fit",
            "no 'Zoom' node in AccessKit tree headless; PNG needs vision review of the zoom readout",
        ));
    } else if shows_fit && !shows_custom {
        verdicts.push(Verdict::pass(
            "zoom default Fit",
            format!("zoom readout names Fit: {texts:?}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "zoom default Fit",
            format!(
                "zoom readout wrong after load (fit={shows_fit} custom={shows_custom}): {texts:?}"
            ),
        ));
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
    assert!(gen >= 1, "sample must render at least once");
}
