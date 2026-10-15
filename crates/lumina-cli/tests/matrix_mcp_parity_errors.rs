//! MCP-PARITY-C: the loud matrix rejections on both transports (SOLL §9
//! phase 3: T19–T21; the byte-parity half lives in `matrix_mcp_parity.rs`).
//!
//! # What is compared
//!
//! Not the *existence* of a rejection but its **class and its text**. For one
//! and the same input the CLI binary must exit non-zero with `error: <text>`
//! on stderr and the MCP tool must answer with the `MatrixError` class
//! (`-32014`) carrying the *same* text. SOLL §2 is what makes this
//! load-bearing: a handler that pre-checked a matrix-domain rule would answer
//! `InvalidParams` where the CLI says its domain text — the exact divergence
//! T19 exists to catch.
//!
//! # What each test proves
//!
//! * T19 — every domain rejection of the §7 table is reached on the MCP side
//!   **and** the text matches the CLI's, root-normalised; pre-report
//!   refusals carry no `report` (§7.1 boundary).
//! * T20 — a refusal writes nothing on either transport. A "loud failure"
//!   that left a half-written export behind would still look loud.
//! * T21 — the red run: the report arrives **and** is an error — `isError:
//!   true`, `data.report` == the CLI stdout JSON, `failed > 0`, and the
//!   `matrix failed:` summary in the message.
//!
//! Everything runs on generated PNGs, so every case is deterministic; the one
//! thing these tests cannot exercise is the RAW decode path (T26 stays
//! `#[ignore]`, `matrix_mcp_parity_gate.rs`).

#[path = "matrix_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, job::*};
use serde_json::json;
use std::path::Path;

/// One refusal expressed once, built fresh inside whichever tree needs it.
struct RefusalCase {
    label: &'static str,
    /// A substring the CLI text must contain — proves the case reaches its
    /// own rule and not an earlier one. `None` where the exact CLI wording is
    /// not pinned elsewhere; the CLI≡MCP equality below still holds.
    must_contain: Option<&'static str>,
    /// Whether the built inputs are byte-identical across trees. `false`
    /// exactly when the fixture itself names its own tree (the absolute
    /// sample path): cross-tree start-identity is then unstatable by
    /// construction, while per-tree "leaves nothing" still is.
    same_inputs_both_trees: bool,
    build: Box<dyn Fn(&Path) -> MatrixCall>,
}

/// The anchor sample plus the default anchor set, ready for a domain rule to
/// fire.
fn anchor_call(dir: &Path) -> MatrixCall {
    anchor_tree(dir);
    MatrixCall::verify(dir.join("recipe-set.json"), dir.join("work"))
}

const DEPTH_RECIPE: &str = r#"{
    "id": "with-depth",
    "goals": ["G-05"],
    "stages": ["lens_blur"],
    "tolerance": "robust",
    "expected_route": "gpu",
    "recipe": {
      "lens_blur": {
        "version": 1,
        "enabled": true,
        "focus_rect": { "x": 0.2, "y": 0.2, "width": 0.4, "height": 0.4 },
        "focal_near": 0.0,
        "focal_far": 0.2,
        "blur_amount": 0.5,
        "bokeh": "round",
        "depth_artifact": { "relative_path": "depth.bin", "sha256": "abc" }
      }
    }
  }"#;

const TYPO_RECIPE: &str = r#"{
    "id": "typo",
    "goals": ["G-06"],
    "stages": ["geometry"],
    "tolerance": "standard",
    "expected_route": "gpu",
    "recipe": {
      "geometri": { "version": 1, "rotation_degrees": 1.0 }
    }
  }"#;

/// The whole §7 table, one entry per row that an input set can actually
/// reach. Every case is runnable up to the point its own rule fires, so a
/// failure is attributable to that rule and not to a fixture.
fn refusal_cases() -> Vec<RefusalCase> {
    let mut cases = vec![
        // A set file that is not there (`load_recipe_set`).
        RefusalCase {
            label: "a set file that is not there",
            must_contain: Some("gone.json"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| {
                anchor_sample(dir);
                MatrixCall::verify(dir.join("gone.json"), dir.join("work"))
            }),
        },
        // `comparison_width == 0` is domain, never a clamp (SOLL §2 Pkt. 3).
        RefusalCase {
            label: "comparison width 0",
            must_contain: Some("comparison_width"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| anchor_call(dir).comparison_width(0)),
        },
        // An unknown recipe id (`prepare_recipes`).
        RefusalCase {
            label: "an unknown recipe id",
            must_contain: Some("unknown recipe id"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| anchor_call(dir).recipes(&["nope"])),
        },
        // A set that selects no recipes.
        RefusalCase {
            label: "a set that selects no recipes",
            must_contain: None,
            same_inputs_both_trees: true,
            build: Box::new(|dir| {
                anchor_sample(dir);
                let set = dir.join("recipe-set.json");
                std::fs::write(
                    &set,
                    r#"{
  "schema_version": 1,
  "pipeline_version": "raster-mvp-1",
  "comparison_width": 48,
  "samples": [{ "id": "sample", "path": "sample.png" }],
  "recipes": []
}"#,
                )
                .unwrap();
                MatrixCall::verify(set, dir.join("work"))
            }),
        },
        // A sample file that is not there.
        RefusalCase {
            label: "a sample file that is not there",
            must_contain: Some("not found"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| {
                let set = anchor_set(dir);
                MatrixCall::verify(set, dir.join("work"))
            }),
        },
        // A wrong `schema_version`.
        RefusalCase {
            label: "a wrong schema version",
            must_contain: None,
            same_inputs_both_trees: true,
            build: Box::new(|dir| {
                anchor_sample(dir);
                let set = anchor_set(dir);
                let text = std::fs::read_to_string(&set).unwrap();
                std::fs::write(
                    &set,
                    text.replace("\"schema_version\": 1", "\"schema_version\": 2"),
                )
                .unwrap();
                MatrixCall::verify(set, dir.join("work"))
            }),
        },
        // An absolute sample path (the set validation rejects it loudly).
        RefusalCase {
            label: "an absolute sample path",
            must_contain: None,
            same_inputs_both_trees: false,
            build: Box::new(|dir| {
                anchor_sample(dir);
                let set = dir.join("recipe-set.json");
                std::fs::write(
                    &set,
                    format!(
                        r#"{{
  "schema_version": 1,
  "pipeline_version": "raster-mvp-1",
  "comparison_width": 48,
  "samples": [{{ "id": "sample", "path": "{}/sample.png" }}],
  "recipes": [{IDENTITY_RECIPE}]
}}"#,
                        dir.display()
                    ),
                )
                .unwrap();
                MatrixCall::verify(set, dir.join("work"))
            }),
        },
        // An artifact-backed recipe is rejected loudly, never skipped.
        RefusalCase {
            label: "an artifact-backed recipe",
            must_contain: Some("depth_artifact"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| {
                anchor_sample(dir);
                let set = write_recipe_set(dir, DEPTH_RECIPE);
                MatrixCall::verify(set, dir.join("work"))
            }),
        },
        // A typo top-level recipe key is rejected loudly.
        RefusalCase {
            label: "an unknown top-level recipe key",
            must_contain: Some("unknown top-level key"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| {
                anchor_sample(dir);
                let set = write_recipe_set(dir, TYPO_RECIPE);
                MatrixCall::verify(set, dir.join("work"))
            }),
        },
        // `--require-gpu` is meaningless in baseline mode.
        RefusalCase {
            label: "require-gpu combined with baseline mode",
            must_contain: Some("cannot be combined with --update-goldens"),
            same_inputs_both_trees: true,
            build: Box::new(|dir| anchor_call(dir).baseline().require_gpu()),
        },
    ];
    // CPU-only build: `--require-gpu` fails at the entry point.
    #[cfg(not(feature = "gpu"))]
    cases.push(RefusalCase {
        label: "require-gpu without a GPU backend",
        must_contain: Some("no GPU backend"),
        same_inputs_both_trees: true,
        build: Box::new(|dir| anchor_call(dir).require_gpu()),
    });
    cases
}

/// T19 — for one and the same input, the CLI's class and text and the MCP
/// tool's class and text are the same (§7 table).
#[test]
fn t19_every_matrix_error_text_matches_the_cli() {
    for case in refusal_cases() {
        let root = tempfile::tempdir().unwrap();
        let (cli_dir, mcp_dir) = both_trees(root.path());
        let cli_call = (case.build)(&cli_dir);
        let mcp_call = (case.build)(&mcp_dir);

        // CLI side: non-zero exit and an `error: <text>` line.
        let cli_output = cli_call.cli_failure();
        let cli_text = cli_cause(&cli_output);
        if let Some(marker) = case.must_contain {
            assert!(
                cli_text.contains(marker),
                "{}: the CLI must say `{marker}`, got `{cli_text}`",
                case.label
            );
        }

        // MCP side: the `MatrixError` class, with the stable name, code and
        // `data.error` of the §7 table.
        let error = mcp_call.mcp().unwrap_err();
        assert_eq!(error.name(), "MatrixError", "{}: class", case.label);
        assert_eq!(error.code(), -32014, "{}: {}", case.label, error.message());
        assert_eq!(
            error.data()["error"],
            json!("MatrixError"),
            "{}: `data.error`",
            case.label
        );

        // Same TEXT, not merely the same class — roots normalised, because a
        // refusal may name the absolute file it rejected.
        let roots = [mcp_dir.as_path(), cli_dir.as_path()];
        assert_eq!(
            normalize_roots_text(&mcp_cause(&error), roots),
            normalize_roots_text(&cli_text, roots),
            "{}: both frontends must report the same text",
            case.label
        );

        // Equally loud over the `tools/call` seam — and with no `report`: a
        // pre-report refusal carries none (§7.1 boundary).
        let structured = mcp_call.mcp_seam_failure();
        assert_eq!(
            structured["error"],
            json!("MatrixError"),
            "{}: the seam must carry `data.error`",
            case.label
        );
        assert!(
            structured.get("report").is_none(),
            "{}: a pre-report refusal carries no report",
            case.label
        );
    }
}

/// T20 — every refusal, on both transports, changes nothing.
#[test]
fn t20_every_rejection_leaves_no_artifact_behind() {
    for case in refusal_cases() {
        let root = tempfile::tempdir().unwrap();
        let (cli_dir, mcp_dir) = both_trees(root.path());
        let cli_call = (case.build)(&cli_dir);
        let mcp_call = (case.build)(&mcp_dir);

        let cli_before = snapshot(&cli_dir);
        let mcp_before = snapshot(&mcp_dir);
        // Teeth: an empty tree would make "nothing changed" vacuous.
        assert!(
            !cli_before.is_empty() && !mcp_before.is_empty(),
            "{}: both trees need files for the snapshot to mean anything",
            case.label
        );

        let cli_output = cli_call.cli_failure();
        assert!(
            mcp_call.mcp().is_err(),
            "{}: the MCP tool must refuse",
            case.label
        );

        assert_eq!(
            cli_before,
            snapshot(&cli_dir),
            "{}: the CLI refusal must leave the tree untouched",
            case.label
        );
        assert_eq!(
            mcp_before,
            snapshot(&mcp_dir),
            "{}: the MCP refusal must leave the tree untouched",
            case.label
        );
        if case.same_inputs_both_trees {
            // Cross-tree start identity: only statable when the fixture
            // itself is tree-agnostic (the absolute-path case names its own
            // tree, so its two inputs differ by construction).
            assert_eq!(
                cli_before, mcp_before,
                "{}: the two trees must start identical",
                case.label
            );
        }
        assert!(
            !cli_cause(&cli_output).is_empty(),
            "{}: a refusal must say why",
            case.label
        );
    }
}

/// T21 — the red run: the report arrives **and** is an error.
///
/// §9 T21 / §7.1: no baseline, so both pairs miss their goldens — `failed >
/// 0`. The CLI prints the full report on stdout and fails afterwards; the MCP
/// tool answers `isError: true` with the same report under `data.report` and
/// the `matrix failed:` summary in the message.
#[test]
fn t21_a_red_run_reports_and_fails_on_both_transports() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    anchor_tree(&cli_dir);
    anchor_tree(&mcp_dir);

    let cli_call = MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work"));
    let mcp_call = MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work"));

    // CLI: the stdout JSON is the full report even though the exit is loud.
    let (cli_report, cli_output) = cli_call.cli_red_json();
    assert!(cli_report["failed"].as_u64().unwrap() > 0, "a red run");
    let cli_summary = cli_cause(&cli_output);
    assert!(
        cli_summary.starts_with("matrix failed:"),
        "the CLI summary names the failure: {cli_summary}"
    );

    // MCP over the seam: `isError: true` with the report inside.
    let mcp_structured = mcp_call.mcp_seam_failure();
    assert_eq!(mcp_structured["error"], json!("MatrixError"));
    let mcp_report = &mcp_structured["report"];
    assert!(mcp_report.is_object(), "a red run carries its report");
    assert!(mcp_report["failed"].as_u64().unwrap() > 0, "a red run");
    assert_same_report("T21 red", mcp_report, &cli_report, &mcp_dir, &cli_dir);

    // The message carries the CLI summary sentence (wall-clock masked: the
    // count is state, the milliseconds are not).
    assert!(
        mcp_structured["message"]
            .as_str()
            .unwrap()
            .contains("matrix failed:"),
        "the message names the failure: {}",
        mcp_structured["message"]
    );
    assert_eq!(
        mask_run_ms(mcp_structured["message"].as_str().unwrap()),
        mask_run_ms(&cli_summary),
        "the MCP message is the CLI summary"
    );

    // The same report through the in-process handler error (§7.1 contract).
    let error = mcp_call.mcp().unwrap_err();
    assert_eq!(error.name(), "MatrixError");
    assert_eq!(
        masked(&error.data()["report"], &mcp_dir, &cli_dir),
        masked(&cli_report, &mcp_dir, &cli_dir),
        "the handler error carries the CLI report"
    );

    // Every pair failed loudly on the missing golden.
    for pair in mcp_report["pairs"].as_array().unwrap() {
        assert_eq!(pair["status"], json!("fail"), "{pair}");
        assert!(
            pair["message"].as_str().unwrap().contains("missing golden"),
            "unexpected pair message: {}",
            pair["message"]
        );
    }
}
