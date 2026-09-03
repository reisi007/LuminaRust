//! GUI-NAV-RECT-1: the viewport rectangle in the navigator must match the
//! visible preview area (from `roi_from_zoom` + pan offset).
//!
//! The rectangle is Painter-drawn, not an AccessKit node, so the headless part
//! pins the preconditions (navigator open, Custom zoom active) and the PNG is
//! the vision-review input for rect-vs-ROI agreement.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test nav_rect -- --ignored"]
fn nav_rect() {
    let dir = artifact_dir("nav_rect");
    let mut probe = GuiProbe::develop_with_sample();
    probe.app_mut().set_navigator_open(true);
    // Custom zoom above fit: the viewport rectangle must be strictly smaller
    // than the navigator overview (mirrors the upstream navigator_viewport).
    probe.app_mut().zoom_step(4.0);
    probe.run_steps(2);
    probe.wait_quiescent(120, 8);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    let zoom_labels: Vec<_> = nodes
        .iter()
        .filter(|n| n.text_contains("Zoom"))
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
    let custom_pinned = zoom_labels.iter().any(|l| l.contains("Custom"));
    let nav_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label.to_lowercase().contains("navigator")
                || n.label.to_lowercase().contains("navigation")
        })
        .collect();

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!(
                "png={w}x{h} nodes={} zoom_labels={zoom_labels:?}",
                nodes.len()
            ),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    if custom_pinned {
        verdicts.push(Verdict::pass(
            "custom zoom pinned",
            "zoom readout names Custom after zoom_step(4.0)",
        ));
    } else {
        verdicts.push(Verdict::fail(
            "custom zoom pinned",
            format!("zoom did not pin Custom: {zoom_labels:?}"),
        ));
    }
    verdicts.push(Verdict::open(
        "navigator rect vs ROI",
        format!(
            "rect is Painter-drawn ({} navigator tree nodes); PNG needs vision review: rect must be smaller than overview and track the visible preview area",
            nav_nodes.len()
        ),
    ));
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
