//! LRPAR-G09-CULL-IMPL-25 (CLI slice): end-to-end tests for `lumina cull`.
//!
//! SOLL: `feature/decisions/LRPAR-G09-CULL-25.md` §2/§5/§7.3. The proposal is
//! source-level only and never writes `rating`, `flag`, `color_label` or a
//! recipe field; multi-item failures are isolated (exit 3), hard errors exit 1.

use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for, Flag};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// Non-flat gradient fixture (gives the Stage-1 heuristic real gradients).
fn gradient_png(dir: &tempfile::TempDir, name: &str, offset: u8) -> PathBuf {
    let (w, h) = (32u32, 24u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = (x as u8)
                .wrapping_mul(5)
                .wrapping_add((y as u8).wrapping_mul(3))
                .wrapping_add(offset);
            rgba.extend_from_slice(&[v, v.wrapping_add(11), v.wrapping_add(23), 255]);
        }
    }
    let path = dir.path().join(name);
    let frame = lumina_core::ImageFrame::new(w, h, rgba).unwrap();
    fs::write(
        &path,
        frame.encode(lumina_core::ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    path
}

fn import(input: &Path) {
    let output = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
}

fn run(args: &[&str]) -> std::process::Output {
    cli().args(args).output().unwrap()
}

/// The proposal is persisted source-level and rating/flag/label/recipe are
/// untouched (decisions §3 rule 1).
#[test]
fn analyze_persists_source_level_proposal_and_never_touches_rating_flag_label() {
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(&dir, "a.png", 0);
    let b = gradient_png(&dir, "b.png", 64);
    import(&a);
    import(&b);

    // Seed the per-copy manual metadata and snapshot the recipe.
    let path_a = sidecar_path_for(&a);
    let mut document = load_sidecar(&path_a).unwrap();
    document.virtual_copies[0].rating = 4;
    document.virtual_copies[0].flag = Flag::Pick;
    document.virtual_copies[0]
        .extras
        .insert("color_label".into(), serde_json::json!(2));
    let recipe_before = serde_json::to_value(&document.virtual_copies[0].recipe).unwrap();
    save_sidecar(&path_a, &document).unwrap();

    let output = run(&[
        "cull",
        "--input",
        a.to_str().unwrap(),
        "--input",
        b.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "cull analyze failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let after = load_sidecar(&path_a).unwrap();
    let culling = after.culling.as_ref().expect("proposal persisted");
    assert!((0.0..=1.0).contains(&culling.score), "score in 0..=1");
    // The proposal never writes rating/flag/label or the recipe.
    assert_eq!(after.virtual_copies[0].rating, 4);
    assert_eq!(after.virtual_copies[0].flag, Flag::Pick);
    assert_eq!(
        after.virtual_copies[0].extras.get("color_label"),
        Some(&serde_json::json!(2))
    );
    assert_eq!(
        serde_json::to_value(&after.virtual_copies[0].recipe).unwrap(),
        recipe_before
    );
    // The second image is independent and equally analyzed.
    assert!(load_sidecar(&sidecar_path_for(&b))
        .unwrap()
        .culling
        .is_some());

    // Read-only status reports `valid`; a re-run is skipped without `--force`.
    let status = run(&["cull", "--input", a.to_str().unwrap(), "--status", "--json"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"status\":\"valid\""));

    let again = run(&[
        "cull",
        "--input",
        a.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stdout).contains("\"status\":\"current\""));
}

/// Batch isolation: one undecodable item does not abort the others; the run
/// exits 3 with the good item analyzed.
#[test]
fn analysis_isolates_item_failures_and_exits_three() {
    let dir = tempfile::tempdir().unwrap();
    let good = gradient_png(&dir, "good.png", 0);
    import(&good);
    let missing = dir.path().join("missing.png");

    let output = run(&[
        "cull",
        "--input",
        good.to_str().unwrap(),
        "--input",
        missing.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"status\":\"analyzed\""));
    assert!(stdout.contains("\"status\":\"failed\""));
    assert!(load_sidecar(&sidecar_path_for(&good))
        .unwrap()
        .culling
        .is_some());
}

/// Status of an image without a sidecar is an isolated item failure → exit 3.
#[test]
fn status_item_failure_exits_three() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png", 0);
    let output = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--status",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"status\":\"failed\""));
}

/// An empty selection is a hard error (exit 1).
#[test]
fn empty_selection_is_a_hard_error() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let output = run(&["cull", "--input", empty.to_str().unwrap(), "--analyze"]);
    assert_eq!(output.status.code(), Some(1));
}

/// A valid sidecar without a `culling` section is the explicit "no proposal"
/// state (exit 0), never an invented recommendation.
#[test]
fn status_without_proposal_is_no_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png", 0);
    import(&input);
    let output = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--status",
        "--json",
    ]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"status\":\"no-proposal\""));
}

/// F2: `--status` and `--analyze` are mutually exclusive flags — a usage error
/// (exit 2), not a runtime failure (exit 1) and never a silent precedence pick.
#[test]
fn status_and_analyze_are_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png", 0);
    import(&input);
    let output = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--status",
        "--analyze",
    ]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "mutually exclusive flags must exit 2: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("mutually exclusive"),
        "stderr must explain the conflict: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// F3: without `--force` a valid identity-matching proposal is kept as it is;
/// `--force` re-runs the heuristic and rewrites the stored proposal.
#[test]
fn force_reanalyzes_and_replaces_the_stored_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png", 0);
    import(&input);
    let path = sidecar_path_for(&input);

    let first = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert!(first.status.success());
    assert!(String::from_utf8_lossy(&first.stdout).contains("\"status\":\"analyzed\""));

    // A sentinel score that only a rewrite can remove.
    let sentinel = 0.123_456_f32;
    let mut document = load_sidecar(&path).unwrap();
    document.culling.as_mut().unwrap().score = sentinel;
    save_sidecar(&path, &document).unwrap();

    // Without `--force` the valid proposal is reported `current` and untouched.
    let kept = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert!(kept.status.success());
    let kept_stdout = String::from_utf8_lossy(&kept.stdout);
    assert!(kept_stdout.contains("\"status\":\"current\""));
    assert_eq!(
        load_sidecar(&path).unwrap().culling.unwrap().score,
        sentinel
    );

    // With `--force` the heuristic overwrites the stored proposal.
    let forced = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--analyze",
        "--force",
        "--json",
    ]);
    assert!(forced.status.success());
    assert!(String::from_utf8_lossy(&forced.stdout).contains("\"status\":\"analyzed\""));
    let rewritten = load_sidecar(&path).unwrap().culling.unwrap().score;
    assert!(
        (rewritten - sentinel).abs() > f32::EPSILON,
        "force must rewrite the stored score, got {rewritten}"
    );
    assert!((0.0..=1.0).contains(&rewritten));
}

/// Status compares the persisted proposal with the source bytes read in this
/// invocation, so replacing the file makes the source identity visibly stale.
#[test]
fn status_uses_live_source_identity_after_source_change() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png", 0);
    import(&input);
    let analyzed = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert!(analyzed.status.success());
    let sidecar = sidecar_path_for(&input);
    let sidecar_before = fs::read(&sidecar).unwrap();

    gradient_png(&dir, "input.png", 211);
    let output = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--status",
        "--json",
    ]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let item = &report["items"][0];
    assert_eq!(item["status"], "stale");
    assert!(item["proposal"]["mismatches"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mismatch| mismatch == "SourceContentHash"));
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
}

/// A stale sidecar source is a per-item write refusal (exit 3). The normal and
/// forced paths both leave the sidecar byte-identical.
#[test]
fn changed_source_analysis_exits_three_and_never_writes_even_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir, "input.png", 17);
    import(&input);
    let first = run(&[
        "cull",
        "--input",
        input.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert!(first.status.success());
    let sidecar = sidecar_path_for(&input);
    let sidecar_before = fs::read(&sidecar).unwrap();
    gradient_png(&dir, "input.png", 199);

    for force in [false, true] {
        let mut args = vec![
            "cull",
            "--input",
            input.to_str().unwrap(),
            "--analyze",
            "--json",
        ];
        if force {
            args.push("--force");
        }
        let output = run(&args);
        assert_eq!(output.status.code(), Some(3), "force={force}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["items"][0]["status"], "failed");
        assert!(report["items"][0]["error"]
            .as_str()
            .unwrap()
            .contains("source identity conflict"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("refusing culling write"));
        assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
    }
}

/// One conflicted source does not abort or roll back a healthy batch item.
#[test]
fn mixed_healthy_and_conflicted_batch_is_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let healthy = gradient_png(&dir, "healthy.png", 23);
    let conflicted = gradient_png(&dir, "conflicted.png", 71);
    import(&healthy);
    import(&conflicted);
    let conflicted_sidecar = sidecar_path_for(&conflicted);
    let conflicted_before = fs::read(&conflicted_sidecar).unwrap();
    gradient_png(&dir, "conflicted.png", 173);

    let output = run(&[
        "cull",
        "--input",
        healthy.to_str().unwrap(),
        "--input",
        conflicted.to_str().unwrap(),
        "--analyze",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let items = report["items"].as_array().unwrap();
    let healthy_item = items
        .iter()
        .find(|item| item["input"] == serde_json::json!(healthy))
        .unwrap();
    let conflicted_item = items
        .iter()
        .find(|item| item["input"] == serde_json::json!(conflicted))
        .unwrap();
    assert_eq!(healthy_item["status"], "analyzed");
    assert_eq!(conflicted_item["status"], "failed");
    assert!(conflicted_item["error"]
        .as_str()
        .unwrap()
        .contains("source identity conflict"));
    assert!(load_sidecar(&sidecar_path_for(&healthy))
        .unwrap()
        .culling
        .is_some());
    assert_eq!(fs::read(conflicted_sidecar).unwrap(), conflicted_before);
}
