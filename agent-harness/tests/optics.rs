//! GUI-OPTICS-1: the Optics section must show a visible profile status
//! (name, or "no profile — correction inactive") plus understandable labels,
//! instead of bare "(unset)" rows with no visible correction effect.
//!
//! Headless check: expand Optics, collect section labels, require a profile
//! status row and flag unexplained "(unset)"-only content.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};

#[test]
#[ignore = "headless GPU required; run: cargo test --test optics -- --ignored"]
fn optics() {
    let dir = artifact_dir("optics");
    let mut probe = GuiProbe::develop_with_sample();
    // Expand the Optics section (CollapsingHeader starts closed).
    let found = probe.click_label_contains("Optics");
    probe.run_steps(2);
    probe.wait_quiescent(120, 8);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    let texts: Vec<String> = nodes
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
    let joined = texts.join("\n");
    let unset_hits: Vec<_> = nodes
        .iter()
        .filter(|n| n.text_contains("(unset)"))
        .map(|n| n.value.clone())
        .collect();
    let optics_hits: Vec<_> = nodes
        .iter()
        .filter(|n| {
            let l = format!("{} {}", n.label, n.value).to_lowercase();
            l.contains("lens")
                || l.contains("profil")
                || l.contains("chromatic")
                || l.contains("vignett")
        })
        .map(|n| n.value.clone())
        .collect();
    let has_status = joined.contains("No lens profile")
        || joined.contains("Lens profile:")
        || joined.to_lowercase().contains("correction inactive");
    // New SOLL (main commit 777f3e6): every manual parameter is always a
    // settable slider with a self-explanatory label — no bare "(unset)" rows.
    let unset_count = unset_hits.len();
    let group_hits: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.text_contains("Distortion (radial)")
                || n.text_contains("Vignette (light falloff)")
                || n.text_contains("lateral")
        })
        .map(|n| n.value.clone())
        .collect();
    let _ = w;
    let _ = h;

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!(
                "optics_header_found={found} nodes={} optics_labels={:?}",
                nodes.len(),
                optics_hits
            ),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    if !found {
        verdicts.push(Verdict::open(
            "optics status visible",
            "Optics header not clickable headless; PNG needs vision review",
        ));
    } else if has_status && unset_count == 0 {
        verdicts.push(Verdict::pass(
            "optics status visible",
            format!("profile status row found (name or explicit inactive note); manual groups present: {group_hits:?}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "optics status visible",
            format!("status={has_status} (unset) rows: {unset_hits:?}; groups: {group_hits:?}; optics labels: {optics_hits:?}"),
        ));
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
