//! GUI-LIBRARY-SUBFOLDERS-1: the Library grid must aggregate the chosen
//! folder *including* subfolders (recursive, symlink-/loop-safe, depth
//! limited), with a relative-path badge per cell.
//!
//! Headless checks: `entries()` count flat (1) vs recursive (3), symlink loop
//! terminates, badge labels appear in the Library tree.

use agent_harness::{artifact_dir, save_report, GuiProbe, Verdict};
use lumina_gui::Module;

#[test]
#[ignore = "headless GPU required; run: cargo test --test subfolders -- --ignored"]
fn subfolders() {
    let dir = artifact_dir("subfolders");
    let workdir = tempfile::tempdir().expect("temp dir");
    // Dummy RAW files (content never decodes): the Library grid, rail and
    // filmstrip are RAW-only by design, so `.ARW` fixtures render grid cells
    // with path badges while undecodable content stays a placeholder.
    for rel in ["top.ARW", "sub/a.ARW", "sub/deep/b.ARW"] {
        let path = workdir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, b"not a real raw file").unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(workdir.path(), workdir.path().join("sub").join("loop")).ok();

    let mut probe = GuiProbe::new();
    probe
        .app_mut()
        .set_directory(workdir.path().display().to_string());
    // `set_directory` lists flat by design.
    let flat = probe.app().entries().len();
    // Recursive aggregation (F-100 Library SOLL).
    probe.app_mut().list_directory();
    let recursive = probe.app().entries().len();
    let keys: Vec<String> = probe
        .app()
        .entries()
        .iter()
        .map(|e| e.thumb_key().to_string())
        .collect();
    let distinct_keys = {
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        sorted.len()
    };

    probe.app_mut().set_module(Module::Library);
    probe.run_steps(10);
    probe.wait_quiescent(120, 8);
    let shot = dir.join("shot.png");
    let (w, h) = probe.render_png(&shot).expect("render PNG");
    let tree = probe.ui_tree_json();
    let nodes = probe.tree_nodes();
    let badge_hit = nodes.iter().any(|n| n.text_contains("sub"));

    let mut verdicts = vec![
        Verdict::pass(
            "harness ran",
            format!("png={w}x{h} flat={flat} recursive={recursive} distinct_keys={distinct_keys}"),
        ),
        Verdict::pass("shot saved", format!("{}", shot.display())),
    ];
    if flat == 1 && recursive == 3 && distinct_keys == 3 {
        verdicts.push(Verdict::pass(
            "recursive aggregation",
            "flat=1 top-level, recursive=3 with distinct thumbnail keys (symlink loop terminated)",
        ));
    } else {
        verdicts.push(Verdict::fail(
            "recursive aggregation",
            format!(
                "flat={flat} (want 1) recursive={recursive} (want 3) distinct_keys={distinct_keys}"
            ),
        ));
    }
    if badge_hit {
        verdicts.push(Verdict::pass(
            "path badge",
            "a 'sub' path label is present in the Library tree",
        ));
    } else {
        verdicts.push(Verdict::open(
            "path badge",
            "no 'sub' label in AccessKit tree headless; PNG needs vision review for per-cell path badges",
        ));
    }
    save_report(&dir, &verdicts, &tree);
    assert!(shot.is_file());
}
