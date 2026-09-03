//! GUI-FILMSTRIP-DUP-1: each image exactly one miniature per view; selection
//! sync across all views.
//!
//! NOTE: the filmstrip is RAW-only by design (`draw_filmstrip` filters
//! `is_raw_name`; jpg/png/webp never appear there), so this scenario uses
//! dummy `.ARW` files like the upstream
//! `filmstrip_is_single_row_horizontal` test. Decode failures still yield
//! placeholder cells, which is sufficient for the duplication geometry.
//!
//! Headless check: with 3 files, the filmstrip band must hold exactly 3
//! cells, one per filename.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};
use lumina_gui::Module;
use std::collections::BTreeMap;

#[test]
#[ignore = "headless GPU required; run: cargo test --test filmstrip_dup -- --ignored"]
fn filmstrip_dup() {
    let dir = artifact_dir("filmstrip_dup");
    let workdir = tempfile::tempdir().expect("temp dir");
    let names = ["IMG_A.ARW", "IMG_B.ARW", "IMG_C.ARW"];
    for name in names {
        std::fs::write(workdir.path().join(name), b"not a real raw file").unwrap();
    }

    let mut probe = GuiProbe::new();
    probe
        .app_mut()
        .set_directory(workdir.path().display().to_string());
    probe.app_mut().set_module(Module::Develop);
    probe.run_steps(10);
    probe.wait_quiescent(200, 8);

    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();

    // Filmstrip = bottom panel band: wide Unknown-role click cells plus the
    // per-cell filename Label (placeholder text carries the entry name).
    let cells: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.role.contains("Unknown") && n.y0 > 600.0 && n.height() > 60.0 && n.width() > 130.0
        })
        .collect();
    let mut per_name: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        let stem = name.trim_end_matches(".ARW");
        let count = nodes
            .iter()
            .filter(|n| n.role.contains("Label") && n.y0 > 660.0 && n.text_contains(stem))
            .count();
        per_name.insert(name, count);
    }

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!(
                "png={w}x{h} filmstrip_cells={} nodes={}",
                cells.len(),
                nodes.len()
            ),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    let dup_free = cells.len() == names.len() && per_name.values().all(|&c| c == 1);
    if dup_free {
        verdicts.push(Verdict::pass(
            "filmstrip duplication",
            format!("each file exactly once: {per_name:?}"),
        ));
    } else {
        verdicts.push(Verdict::fail(
            "filmstrip duplication",
            format!(
                "expected 1 cell per file, got cells={} per_name={per_name:?}",
                cells.len()
            ),
        ));
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
