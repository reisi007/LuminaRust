//! MCP-PARITY-A: the loud side of the parity proof for `lumina_geometry` and
//! `lumina_upright`.
//!
//! Same contract as `stage_parity_errors.rs`: a rejected call must be a loud tool
//! error that changes no bytes, and the CLI must reject the same input with a
//! non-zero exit. No rejection is ever an "empty result as success".

#[path = "stage_parity_common/mod.rs"]
mod common;
use common::*;

#[test]
fn geometry_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let path = cli_input.to_str().unwrap().to_string();
    let cases: Vec<Case> = vec![
        Case {
            label: "unknown aspect preset",
            mcp: json!({ "op": "set_crop_aspect", "preset": "7:9" }),
            flags: &["--set-crop-aspect", "7:9"],
            mcp_fragment: "invalid aspect preset",
            cli_fragment: "invalid aspect preset",
        },
        Case {
            label: "malformed free crop",
            mcp: json!({ "op": "set_crop_free", "rect": { "x": 0.1, "y": 0.1, "width": 0.6, "height": 0.0 } }),
            flags: &["--set-crop-free", "0.1,0.1,0.6,0"],
            mcp_fragment: "invalid geometry free crop",
            cli_fragment: "invalid geometry free crop",
        },
        Case {
            label: "unknown mirror word",
            mcp: json!({ "op": "set_mirror", "mirror": "diagonal" }),
            flags: &["--set-mirror", "diagonal"],
            mcp_fragment: "invalid mirror",
            cli_fragment: "invalid mirror",
        },
        Case {
            label: "unknown lens field",
            mcp: json!({ "op": "set_lens_field", "field": "nope", "value": 1.0 }),
            flags: &["--set-lens", "nope:1"],
            mcp_fragment: "invalid lens field",
            cli_fragment: "invalid lens field",
        },
        Case {
            label: "unknown perspective field",
            mcp: json!({ "op": "set_perspective_field", "field": "nope", "value": 1.0 }),
            flags: &["--set-perspective", "nope:1"],
            mcp_fragment: "invalid perspective field",
            cli_fragment: "invalid perspective field",
        },
        Case {
            label: "out-of-range rotation",
            mcp: json!({ "op": "set_rotation", "value": 400.0 }),
            flags: &["--set-rotation", "400"],
            mcp_fragment: "invalid geometry version or rotation",
            cli_fragment: "invalid geometry version or rotation",
        },
        Case {
            label: "crop outside the frame",
            mcp: json!({
                "op": "set_crop_free",
                "rect": { "x": 0.1, "y": 0.1, "width": 0.95, "height": 0.95 }
            }),
            flags: &["--set-crop-free", "0.1,0.1,0.95,0.95"],
            mcp_fragment: "invalid geometry free crop",
            cli_fragment: "invalid geometry free crop",
        },
        Case {
            label: "unknown copy",
            mcp: json!({ "op": "set_rotation", "virtual_copy": "nope", "value": 5.0 }),
            flags: &["--virtual-copy", "nope", "--set-rotation", "5"],
            mcp_fragment: "unknown virtual copy `nope`",
            cli_fragment: "unknown virtual copy `nope`",
        },
    ];
    for case in cases {
        let before = session.sidecar_bytes();
        let error = session
            .call("geometry", case.mcp.clone())
            .expect_err(&format!("lumina_geometry {} must abort", case.label));
        assert!(
            error.message().contains(case.mcp_fragment),
            "lumina_geometry {}: `{}` does not mention `{}`",
            case.label,
            error.message(),
            case.mcp_fragment
        );
        assert_eq!(
            session.sidecar_bytes(),
            before,
            "lumina_geometry {} changed sidecar bytes despite aborting",
            case.label
        );
        let (stderr, code) =
            cli_fails(&[&["geometry", "--input", &path, "--json"][..], case.flags].concat());
        assert_ne!(code, 0, "lumina geometry {} must exit non-zero", case.label);
        assert!(
            stderr.contains(case.cli_fragment),
            "lumina geometry {}: `{stderr}` does not mention `{}`",
            case.label,
            case.cli_fragment
        );
    }
}

#[test]
fn upright_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let path = cli_input.to_str().unwrap().to_string();

    // `enable` without a persisted analysis is the loud case: no fallback, no
    // empty success.
    let before = session.sidecar_bytes();
    let error = session
        .call("upright", json!({ "op": "enable" }))
        .expect_err("enable without an analysis must abort");
    assert!(
        error.message().contains("no persisted upright analysis"),
        "{}",
        error.message()
    );
    assert_eq!(session.sidecar_bytes(), before);
    let (stderr, code) = cli_fails(&["upright", "--input", &path, "--json", "--enable"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("no persisted upright analysis"), "{stderr}");

    // An unknown copy aborts before any write.
    let error = session
        .call("upright", json!({ "op": "list", "virtual_copy": "nope" }))
        .expect_err("an unknown copy must abort");
    assert!(
        error.message().contains("unknown virtual copy `nope`"),
        "{error:?}"
    );
    assert_eq!(session.sidecar_bytes(), before);
    let (stderr, code) = cli_fails(&[
        "upright",
        "--input",
        &path,
        "--json",
        "--virtual-copy",
        "nope",
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("unknown virtual copy `nope`"), "{stderr}");

    // `disable_after_analyze` outside op=analyze is a contradiction, not a
    // silently ignored argument.
    let error = session
        .call(
            "upright",
            json!({ "op": "clear", "disable_after_analyze": true }),
        )
        .expect_err("a contradictory field must abort");
    assert!(
        error.message().contains("only applies to op=analyze"),
        "{}",
        error.message()
    );
    assert_eq!(session.sidecar_bytes(), before);
}

#[test]
fn every_stage_tool_reports_an_unknown_copy_loudly_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(&root.path().join("mcp"));
    let before = session.sidecar_bytes();
    for stage in STAGES {
        let error = session
            .call(stage, json!({ "op": "list", "virtual_copy": "nope" }))
            .expect_err(&format!("lumina_{stage} must not accept an unknown copy"));
        assert!(
            error.message().contains("unknown virtual copy `nope`"),
            "{stage}: {}",
            error.message()
        );
        assert_eq!(session.sidecar_bytes(), before, "{stage} wrote bytes");
    }
}
