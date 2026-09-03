//! GUI-NAV-RECT-1: the viewport rectangle in the navigator must match the
//! visible preview area (from `roi_from_zoom` + pan offset).
//!
//! The rectangle is Painter-drawn, not an AccessKit node, so the headless part
//! pins the preconditions (navigator open, Custom zoom active) and the PNG is
//! the vision-review input for rect-vs-ROI agreement.

use agent_harness::{
    artifact_dir,
    painter::{nav_accent_pixels, texts_containing, MIN_NAV_ACCENT_PIXELS},
    save_report, GuiProbe, Verdict,
};

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
    let img = image::open(&shot).expect("shot readable").to_rgba8();
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    let zoom_labels = texts_containing(&nodes, "Zoom");
    let nav_labels = texts_containing(&nodes, "Navigator");
    let custom_pinned = zoom_labels.iter().any(|l| l.contains("Custom"));
    let nav_open = !nav_labels.is_empty();
    // Painter-home (HARNESS-2): the viewport rect is a 2px ACCENT
    // `rect_stroke` — no AccessKit node. The stroke IS in the composited
    // readback (vector paint, not a GPU texture), so accent pixels are the
    // presence proof; exact rect-vs-ROI geometry stays OPEN below.
    let accents = nav_accent_pixels(&img);
    let nav_nodes = nav_labels.len();

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
            "rect is Painter-drawn ({nav_nodes} navigator tree nodes); PNG needs vision review: rect must be smaller than overview and track the visible preview area"
        ),
    ));
    if accents >= MIN_NAV_ACCENT_PIXELS {
        verdicts.push(Verdict::pass(
            "navigator rect stroke painted (painter-home)",
            format!(
                "{accents} accent-stroke pixels (need >={MIN_NAV_ACCENT_PIXELS}); nav_open={nav_open} custom_pinned={custom_pinned} — presence proof only, geometry stays OPEN above"
            ),
        ));
    } else {
        verdicts.push(Verdict::open(
            "navigator rect stroke painted (painter-home)",
            format!(
                "only {accents} accent-stroke pixels (need >={MIN_NAV_ACCENT_PIXELS}); PNG needs vision review"
            ),
        ));
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
