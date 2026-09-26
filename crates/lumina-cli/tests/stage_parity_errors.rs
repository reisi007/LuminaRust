//! MCP-PARITY-A: the loud side of the stage-editor parity proof.
//!
//! One test per stage editor. Each runs a set of rejected calls through the MCP
//! tool and asserts (a) a tool error, never a success, (b) byte-identical
//! sidecar bytes, and (c) that the CLI rejects the same input with a non-zero
//! exit and the same text. A rejection is never an "empty result as success"
//! and never a silent fallback.

#[path = "stage_parity_common/mod.rs"]
mod common;
use common::*;

// ----------------------------------------------- loud errors change no bytes
//
// One test per stage editor, each running a set of rejected calls through the
// MCP tool and asserting (a) a tool error, (b) byte-identical sidecar, and
// (c) the same rejection text/exit code from the CLI. A rejection is never an
// "empty result as success".

#[test]
fn spot_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    // A known spot, so an unknown-id test is not trivially "no spots at all".
    session.ok(
        "spot",
        json!({ "op": "add", "center_x": 0.4, "center_y": 0.4, "radius": 5.0 }),
    );
    let before = session.sidecar_bytes();
    let path = cli_input.to_str().unwrap().to_string();

    let cases: Vec<Case> = vec![
        // The MCP layer can catch a missing value one step earlier than the CLI:
        // its schema is a JSON object, the CLI's is a text field. Both abort.
        Case {
            label: "add without a radius",
            mcp: json!({ "op": "add", "center_x": 0.5, "center_y": 0.5 }),
            flags: &["--add-heuristic", "--center-x", "0.5", "--center-y", "0.5"],
            mcp_fragment: "op `add` requires `radius`",
            cli_fragment: "--add-heuristic requires",
        },
        Case {
            label: "add",
            mcp: json!({ "op": "add", "center_x": 0.5, "center_y": 0.5, "radius": 999.0 }),
            flags: &[
                "--add-heuristic",
                "--center-x",
                "0.5",
                "--center-y",
                "0.5",
                "--radius",
                "999",
            ],
            mcp_fragment: "outside allowed range",
            cli_fragment: "outside allowed range",
        },
        Case {
            label: "add",
            mcp: json!({ "op": "add", "center_x": 5.0, "center_y": 0.5, "radius": 4.0 }),
            flags: &[
                "--add-heuristic",
                "--center-x",
                "5",
                "--center-y",
                "0.5",
                "--radius",
                "4",
            ],
            mcp_fragment: "outside allowed range",
            cli_fragment: "outside allowed range",
        },
        Case {
            label: "update",
            mcp: json!({ "op": "update", "spot_id": "missing", "radius": 3.0 }),
            flags: &["--spot-id", "missing", "--set-radius", "3"],
            mcp_fragment: "unknown spot",
            cli_fragment: "unknown spot",
        },
        Case {
            label: "update without a target id",
            mcp: json!({ "op": "update", "radius": 3.0 }),
            flags: &["--set-radius", "3"],
            mcp_fragment: "op `update` requires `spot_id`",
            cli_fragment: "require --spot-id",
        },
        Case {
            label: "remove",
            mcp: json!({ "op": "remove", "spot_id": "missing" }),
            flags: &["--remove-spot", "missing"],
            mcp_fragment: "unknown spot",
            cli_fragment: "unknown spot",
        },
        Case {
            label: "set_distraction",
            mcp: json!({ "op": "set_distraction", "distraction": "bogus=true" }),
            flags: &["--set-distraction", "bogus=true"],
            mcp_fragment: "unknown distraction key",
            cli_fragment: "unknown distraction key",
        },
        Case {
            label: "unknown copy",
            mcp: json!({
                "op": "add",
                "virtual_copy": "nope",
                "center_x": 0.5,
                "center_y": 0.5,
                "radius": 4.0
            }),
            flags: &[
                "--virtual-copy",
                "nope",
                "--add-heuristic",
                "--center-x",
                "0.5",
                "--center-y",
                "0.5",
                "--radius",
                "4",
            ],
            mcp_fragment: "unknown virtual copy `nope`",
            cli_fragment: "unknown virtual copy `nope`",
        },
    ];

    for case in cases {
        let before = session.sidecar_bytes();
        let error = session
            .call("spot", case.mcp.clone())
            .expect_err(&format!("lumina_spot {} must abort", case.label));
        assert_eq!(
            error.name(),
            "InvalidParams",
            "lumina_spot {}: {error:?}",
            case.label
        );
        assert!(
            error.message().contains(case.mcp_fragment),
            "lumina_spot {}: `{}` does not mention `{}`",
            case.label,
            error.message(),
            case.mcp_fragment
        );
        assert_eq!(
            session.sidecar_bytes(),
            before,
            "lumina_spot {} changed sidecar bytes despite aborting",
            case.label
        );
        // The same rejection through the CLI: non-zero exit, same verdict.
        let (stderr, code) =
            cli_fails(&[&["spot", "--input", &path, "--json"][..], case.flags].concat());
        assert_ne!(code, 0, "lumina spot {} must exit non-zero", case.label);
        assert!(
            stderr.contains(case.cli_fragment),
            "lumina spot {}: `{stderr}` does not mention `{}`",
            case.label,
            case.cli_fragment
        );
    }
    assert_eq!(before, session.sidecar_bytes());
}

#[test]
fn lens_blur_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let path = cli_input.to_str().unwrap().to_string();
    let cases: Vec<Case> = vec![
        Case {
            label: "unknown bokeh",
            mcp: json!({ "op": "set_bokeh", "bokeh": "triangle" }),
            flags: &["--set-bokeh", "triangle"],
            mcp_fragment: "invalid bokeh shape",
            cli_fragment: "invalid bokeh shape",
        },
        Case {
            label: "degenerate focus rect",
            mcp: json!({
                "op": "set_focus_rect",
                "focus_rect": { "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.0 }
            }),
            flags: &["--set-focus-rect", "0.1,0.1,0.5,0"],
            mcp_fragment: "invalid lens_blur focus_rect",
            cli_fragment: "invalid lens_blur focus_rect",
        },
        Case {
            label: "out-of-range amount",
            mcp: json!({ "op": "set_amount", "amount": 5.0 }),
            flags: &["--set-amount", "5"],
            mcp_fragment: "invalid lens_blur blur_amount",
            cli_fragment: "invalid lens_blur blur_amount",
        },
        Case {
            label: "absolute depth artifact",
            mcp: json!({
                "op": "set_depth_artifact",
                "depth_artifact": { "relative_path": "/abs/depth.bin", "sha256": "abc123" }
            }),
            flags: &["--set-depth-artifact", "/abs/depth.bin:abc123"],
            mcp_fragment: "lens_blur depth_artifact relative_path",
            cli_fragment: "lens_blur depth_artifact relative_path",
        },
    ];
    for case in cases {
        let before = session.sidecar_bytes();
        let error = session
            .call("lens-blur", case.mcp.clone())
            .expect_err(&format!("lumina_lens_blur {} must abort", case.label));
        assert!(
            error.message().contains(case.mcp_fragment),
            "lumina_lens_blur {}: `{}` does not mention `{}`",
            case.label,
            error.message(),
            case.mcp_fragment
        );
        assert_eq!(
            session.sidecar_bytes(),
            before,
            "lumina_lens_blur {} changed sidecar bytes despite aborting",
            case.label
        );
        let (stderr, code) =
            cli_fails(&[&["lens-blur", "--input", &path, "--json"][..], case.flags].concat());
        assert_ne!(
            code, 0,
            "lumina lens-blur {} must exit non-zero",
            case.label
        );
        assert!(
            stderr.contains(case.cli_fragment),
            "lumina lens-blur {}: `{stderr}` does not mention `{}`",
            case.label,
            case.cli_fragment
        );
    }
}

/// The inverted focal range is the loud case that only the *sidecar validator*
/// can see, so it is tested on its own: the MCP call must abort and write
/// nothing, and the CLI must reject the same pair of values.
#[test]
fn lens_blur_inverted_focal_range_aborts_and_changes_no_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    // Both sides must first reach the same state: a default stage is created on
    // first touch (near 0.0, far 0.2), then near is lowered to 0.1. Only then is
    // `focal_far = 0.05` an inversion.
    session.ok(
        "lens-blur",
        json!({ "op": "set_focal_near", "focal_near": 0.1 }),
    );
    cli_json(&[
        "lens-blur",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-focal-near",
        "0.1",
    ]);
    let before = session.sidecar_bytes();
    let error = session
        .call(
            "lens-blur",
            json!({ "op": "set_focal_far", "focal_far": 0.05 }),
        )
        .expect_err("focal_far below focal_near must abort");
    assert!(
        error.message().contains("invalid lens_blur focal range"),
        "{}",
        error.message()
    );
    assert_eq!(session.sidecar_bytes(), before);
    let (stderr, code) = cli_fails(&[
        "lens-blur",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-focal-far",
        "0.05",
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid lens_blur focal range"), "{stderr}");
}

/// A referenced-but-unresolved depth artifact is reported loudly by both
/// transports (never silently replaced by the heuristic), and the render that
/// would consume it aborts loudly on the CLI.
#[test]
fn lens_blur_missing_depth_artifact_is_reported_not_silently_replaced() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let reference = json!({
        "op": "set_depth_artifact",
        "depth_artifact": { "relative_path": "depth/absent.bin", "sha256": "abc123" }
    });
    session.ok("lens-blur", reference.clone());
    cli_json(&[
        "lens-blur",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-depth-artifact",
        "depth/absent.bin:abc123",
    ]);
    let cli_report = cli_json(&[
        "lens-blur",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
    ]);
    let mcp_report = session.ok("lens-blur", json!({ "op": "list" }));
    assert_eq!(
        mcp_report["status"],
        json!("missing depth artifact"),
        "a referenced but unresolved depth artifact must be reported, not replaced"
    );
    assert_eq!(
        mcp_report["status"], cli_report["status"],
        "both transports must report the same depth status"
    );
    // And a render aborts loudly instead of silently using the heuristic.
    let (stderr, code) = cli_fails(&[
        "render",
        "--input",
        cli_input.to_str().unwrap(),
        "--output",
        cli_input
            .parent()
            .unwrap()
            .join("out.png")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("lens_blur.depth_artifact"), "{stderr}");
}
