//! MCP-PARITY-C: the argument-shape half of the matrix byte-parity suite
//! (SOLL §9 phase 3: T22–T25; the core T15–T18 lives in
//! `matrix_mcp_parity.rs`).
//!
//! # What is actually compared
//!
//! The same two-tree premise as `matrix_mcp_parity.rs` — the real CLI process
//! on one anchor tree, the MCP tool on a second byte-identical tree — with
//! one argument varied per test:
//!
//! * an explicit `golden_dir` is used by both transports and the report
//!   paths point there (T22);
//! * the recipe filter selects exactly one pair on both (T23);
//! * with no path passed at all, both resolve through the same env chain and
//!   the same `<set>/golden` default (T24);
//! * samples and their sidecars are byte-identical after the run (T25).
//!
//! T24 additionally masks the work-dir parent: with no `work_dir` passed both
//! transports render into a fresh tempdir that is removed afterwards, so the
//! two addresses cannot agree — only the state under them can.

#[path = "matrix_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, green::*, job::*};
use serde_json::{json, Value};

/// T22 — golden-dir override: an explicit `golden_dir` is used by both
/// transports and the report paths point there (normalised).
///
/// §9 T22: baseline + verify against a golden dir outside `<set>/golden`;
/// reports agree field for field and the written goldens agree byte for byte.
#[test]
fn t22_explicit_golden_dir_is_used_by_both_transports() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    anchor_tree(&cli_dir);
    anchor_tree(&mcp_dir);
    let (cli_golden, mcp_golden) = (cli_dir.join("goldens"), mcp_dir.join("goldens"));

    let cli_base = MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work"))
        .golden_dir(cli_golden.clone())
        .baseline()
        .cli_json();
    let mcp_base = MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work"))
        .golden_dir(mcp_golden.clone())
        .baseline()
        .mcp_seam();
    assert_same_report("T22 baseline", &mcp_base, &cli_base, &mcp_dir, &cli_dir);

    let cli_report = MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work"))
        .golden_dir(cli_golden.clone())
        .cli_json();
    let mcp_payload = MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work"))
        .golden_dir(mcp_golden.clone())
        .mcp_seam();
    assert_same_report("T22 verify", &mcp_payload, &cli_report, &mcp_dir, &cli_dir);

    // The override is where the reports point (normalised) and where the
    // identical bytes lie.
    for recipe in ["identity", "tone"] {
        let (cli_path, mcp_path) = (
            golden_of(&cli_report, recipe),
            golden_of(&mcp_payload, recipe),
        );
        assert_eq!(cli_path.parent().unwrap(), cli_golden.as_path());
        assert_eq!(mcp_path.parent().unwrap(), mcp_golden.as_path());
        assert_identical_files(&cli_path, &mcp_path, &format!("T22 golden {recipe}"));
    }
    assert!(
        !cli_dir.join("golden").exists(),
        "the default golden dir must stay untouched"
    );
}

/// T23 — recipe filter: `recipes: ["identity"]` runs exactly that pair on
/// both transports, `pairs_total == 1`.
///
/// §9 T23: one CLI flag ↔ one JSON array, rendered from the same
/// [`MatrixCall`] so the filter cannot differ between transports.
#[test]
fn t23_recipe_filter_selects_exactly_one_pair_on_both_transports() {
    let green = green_trees();
    let (cli_dir, mcp_dir) = (&green.cli_dir, &green.mcp_dir);

    let cli_report = MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work"))
        .recipes(&["identity"])
        .cli_json();
    let mcp_payload = MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work"))
        .recipes(&["identity"])
        .mcp_seam();

    for (label, payload) in [("CLI", &cli_report), ("MCP", &mcp_payload)] {
        assert_eq!(payload["pairs_total"], json!(1), "{label}: pairs_total");
        assert_eq!(payload["passed"], json!(1), "{label}: passed");
        assert_eq!(payload["failed"], json!(0), "{label}: failed");
        assert_eq!(payload["pairs"][0]["recipe"], json!("identity"));
        assert_eq!(payload["pairs"][0]["status"], json!("pass"));
    }
    assert_same_report("T23 filtered", &mcp_payload, &cli_report, mcp_dir, cli_dir);
}

/// T24 — default resolution: with neither `recipe_set` nor
/// `golden_dir`/`work_dir` passed, both transports resolve through the same
/// env chain and report the same (normalised) addresses.
///
/// §9 T24: `LUMINA_MATRIX_RECIPE_SET` points at each tree's set in turn; the
/// CLI subprocess and the in-process tool read the same process environment.
/// The work-dir default (a fresh tempdir, removed afterwards) cannot agree on
/// an address, so each payload's export parent is masked with `<WORKDIR>` —
/// and then the state under it must agree.
#[test]
fn t24_defaults_resolve_through_the_same_env_chain() {
    const VAR: &str = "LUMINA_MATRIX_RECIPE_SET";
    let previous = std::env::var(VAR).ok();
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    anchor_tree(&cli_dir);
    anchor_tree(&mcp_dir);

    // Sequential: the variable addresses one tree at a time.
    std::env::set_var(VAR, cli_dir.join("recipe-set.json"));
    let cli_base = MatrixCall::new().baseline().cli_json();
    let cli_report = MatrixCall::new().cli_json();
    std::env::set_var(VAR, mcp_dir.join("recipe-set.json"));
    let mcp_base = MatrixCall::new().baseline().mcp_seam();
    let mcp_payload = MatrixCall::new().mcp_seam();
    match previous {
        Some(value) => std::env::set_var(VAR, value),
        None => std::env::remove_var(VAR),
    }

    // The work-dir default is a removed tempdir: mask each payload's export
    // parent, prove the default happened (outside the trees, directory gone),
    // then compare the state.
    let mask_workdir = |payload: &Value| -> Value {
        let mut value = payload.clone();
        for pair in value["pairs"].as_array_mut().unwrap() {
            let output = pair["output"].as_str().unwrap().to_string();
            let parent = std::path::Path::new(&output)
                .parent()
                .unwrap()
                .display()
                .to_string();
            assert!(
                !output.starts_with(mcp_dir.display().to_string().as_str())
                    && !output.starts_with(cli_dir.display().to_string().as_str()),
                "the default work dir must be a tempdir outside the trees: {output}"
            );
            assert!(
                !std::path::Path::new(&parent).exists(),
                "the default work dir must be removed afterwards: {parent}"
            );
            pair["output"] = json!(output.replace(&parent, "<WORKDIR>"));
        }
        value
    };

    assert_same_report(
        "T24 baseline",
        &mask_workdir(&mcp_base),
        &mask_workdir(&cli_base),
        &mcp_dir,
        &cli_dir,
    );
    // The golden default is `<set-dir>/golden`: inside the trees, hence
    // normalisable — both reports must point at the same normalised address.
    assert_eq!(
        masked(&mcp_payload, &mcp_dir, &cli_dir)["golden_dir"],
        masked(&cli_report, &mcp_dir, &cli_dir)["golden_dir"],
        "the default golden dir resolves to <set>/golden on both"
    );
    assert_same_report(
        "T24 verify",
        &mask_workdir(&mcp_payload),
        &mask_workdir(&cli_report),
        &mcp_dir,
        &cli_dir,
    );
}

/// T25 — sample/sidecar untouchedness: samples and their sidecars are
/// byte-identical after the run on both transports.
///
/// §9 T25 / Invariante 10: the runner reads samples only and writes
/// exclusively into `work_dir` (exports/diffs) and `golden_dir` (baseline).
/// The sidecars exist because the samples were imported first — the state of
/// a working library, in which an accidental overwrite would show.
#[test]
fn t25_samples_and_sidecars_are_untouched_by_both_transports() {
    let green = green_trees();
    let (cli_dir, mcp_dir) = (&green.cli_dir, &green.mcp_dir);

    for dir in [cli_dir, mcp_dir] {
        let imported = cli()
            .arg("import")
            .arg("--input")
            .arg(dir.join("sample.png"))
            .output()
            .unwrap();
        assert!(
            imported.status.success(),
            "import failed: {}",
            stderr_of(&imported)
        );
    }
    let state = |dir: &std::path::Path| -> (Vec<u8>, Vec<u8>) {
        let sample = dir.join("sample.png");
        let sidecar = lumina_sidecar::sidecar_path_for(&sample);
        assert!(sidecar.is_file(), "the import must write a sidecar");
        (bytes(&sample), bytes(&sidecar))
    };
    let (cli_before, mcp_before) = (state(cli_dir), state(mcp_dir));
    assert_eq!(cli_before, mcp_before, "identical source state");

    let cli_report =
        MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work")).cli_json();
    let mcp_payload =
        MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work")).mcp_seam();
    assert_same_report("T25 verify", &mcp_payload, &cli_report, mcp_dir, cli_dir);

    assert_eq!(state(cli_dir), cli_before, "CLI leaves sources alone");
    assert_eq!(state(mcp_dir), mcp_before, "MCP leaves sources alone");
}
