//! GUI-RIGHT-THUMB-1: the same thumbnail must not appear 3x (left rail,
//! filmstrip, right panel above Presets). Decided in main commit `777f3e6`
//! (F-100): the right-panel `draw_crop_thumb` was removed — it tripled the
//! preview and showed ROI crops as full frames — so the Develop panel starts
//! with Presets/History and the right band must hold no Image node.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test right_thumb -- --ignored"]
fn right_thumb() {
    let dir = artifact_dir("right_thumb");
    let mut probe = GuiProbe::develop_with_sample();
    // Real interaction (also keeps this shot distinct per Befund 4): expand
    // the Presets section at the top of the right Develop panel.
    let presets_open = probe.click_label_contains("Presets");
    probe.run_steps(2);
    probe.wait_quiescent(120, 8);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    // Image-role nodes are picture depictions; band them by screen region.
    // Known widgets: navigator overview (left), main preview (center),
    // display-only crop thumbnail at the top of the right Develop panel
    // (`draw_crop_thumb` — deliberate LR-like feature, paints black headless).
    let images: Vec<_> = nodes.iter().filter(|n| n.role.contains("Image")).collect();
    let left = images.iter().filter(|n| n.x1 < 300.0).count();
    let right = images.iter().filter(|n| n.x0 > 980.0).count();
    let center = images.len().saturating_sub(left + right);
    let bottom = images.iter().filter(|n| n.y0 > 620.0).count();
    let right_rects: Vec<_> = images
        .iter()
        .filter(|n| n.x0 > 980.0)
        .map(|n| (n.x0, n.y0, n.x1, n.y1))
        .collect();

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!("png={w}x{h} image_nodes={} left={left} center={center} right={right} bottom={bottom} presets_expanded={presets_open}", images.len()),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    // Fixed state: no panel thumbnail in the right band. A reappearing
    // right-band depiction reopens the duplication question (vision review
    // with painted pixels decides identity then).
    if right == 0 {
        verdicts.push(Verdict::pass(
            "right panel duplication",
            "no Image node in the right panel band — crop thumbnail removed, no 3x duplication",
        ));
    } else if left >= 1 && center >= 1 {
        verdicts.push(Verdict::fail(
            "right panel duplication",
            format!("right-band depiction back (rects={right_rects:?}); 3x duplication regressed"),
        ));
    } else {
        verdicts.push(Verdict::open(
            "right panel duplication",
            format!(
                "left={left} center={center} right={right} bottom={bottom}; needs vision review"
            ),
        ));
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
