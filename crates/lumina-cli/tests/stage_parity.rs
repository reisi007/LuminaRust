//! MCP-PARITY-A: byte identity between an MCP stage-tool call and the
//! equivalent CLI subcommand call, for all four session-based recipe stage
//! editors, in BOTH directions.
//!
//! # What is actually compared
//!
//! Not a "structural" or "semantic" comparison: the real `lumina-cli` process
//! (`env!("CARGO_BIN_EXE_lumina-cli")`) runs against one fixture and the MCP
//! server tool runs in-process against a *second, byte-identical* fixture. Then
//!
//! * **read direction** — the sidecar bytes are compared with a literal byte
//!   compare (a read must change nothing) **and** the MCP tool payload is
//!   compared against the CLI's `--json` document, which is the strongest
//!   available statement that the two transports render one state, not two;
//! * **write direction** — the sidecar bytes are compared after the same
//!   operation on both sides. For `spot` and `lens-blur` that is a literal byte
//!   compare, with no mask at all: neither editor appends a history entry, so
//!   nothing in the file is process-dependent (see [`assert_identical_sidecar`]).
//!   `geometry` and `upright` additionally append exactly one history
//!   entry whose id and `recorded_at` are a **millisecond timestamp**
//!   (`geometry-<ms>` / `upright-<ms>`) — pre-existing CLI behaviour, not
//!   reproducible across two processes — so for those two the comparison masks
//!   exactly that stamp and fails on any other difference (see
//!   [`assert_same_sidecar_ignoring_history_stamp`]).
//!
//! The two fixtures live in two directories because the CLI's `--json` document
//! echoes the absolute `input` path; the compared stage section excludes that
//! transport fact.

#[path = "stage_parity_common/mod.rs"]
mod common;
use common::*;

// ---------------------------------------------------------------- read
// direction: one test per stage editor, each proving (a) the read changed no
// sidecar byte on either side and (b) both transports rendered the same state.

#[test]
fn spot_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // A read must not change a single byte on either side.
    let cli_before = bytes(&sidecar_path_for(&cli_input));
    let mcp_before = session.sidecar_bytes();
    assert_eq!(
        cli_before, mcp_before,
        "the two fixtures must start identical"
    );

    let cli_report = cli_json(&["spot", "--input", cli_input.to_str().unwrap(), "--json"]);
    let mcp_report = session.ok("spot", json!({ "op": "list" }));

    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "spot read",
    );
    assert_eq!(
        stage_section(&mcp_report, "spot"),
        stage_section(&cli_report, "spot"),
        "lumina_spot op=list must render the same state as `lumina spot --json`"
    );
    assert_eq!(
        mcp_report["saved"],
        json!(false),
        "a read must report saved=false"
    );
    assert_eq!(
        mcp_report["action"],
        json!(""),
        "a read must report no action"
    );
}

#[test]
fn lens_blur_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    assert_eq!(
        bytes(&sidecar_path_for(&cli_input)),
        session.sidecar_bytes(),
        "the two fixtures must start identical"
    );

    let cli_report = cli_json(&[
        "lens-blur",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
    ]);
    let mcp_report = session.ok("lens-blur", json!({ "op": "list" }));

    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "lens-blur read",
    );
    assert_eq!(
        stage_section(&mcp_report, "lens-blur"),
        stage_section(&cli_report, "lens-blur")
    );
    assert_eq!(mcp_report["saved"], json!(false));
}

#[test]
fn geometry_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    assert_eq!(
        bytes(&sidecar_path_for(&cli_input)),
        session.sidecar_bytes()
    );

    let cli_report = cli_json(&["geometry", "--input", cli_input.to_str().unwrap(), "--json"]);
    let mcp_report = session.ok("geometry", json!({ "op": "list" }));

    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "geometry read",
    );
    assert_eq!(
        stage_section(&mcp_report, "geometry"),
        stage_section(&cli_report, "geometry")
    );
    assert_eq!(mcp_report["saved"], json!(false));
}

#[test]
fn upright_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    assert_eq!(
        bytes(&sidecar_path_for(&cli_input)),
        session.sidecar_bytes()
    );

    let cli_report = cli_json(&["upright", "--input", cli_input.to_str().unwrap(), "--json"]);
    let mcp_report = session.ok("upright", json!({ "op": "list" }));

    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "upright read",
    );
    assert_eq!(
        stage_section(&mcp_report, "upright"),
        stage_section(&cli_report, "upright")
    );
    assert_eq!(mcp_report["saved"], json!(false));
    // Neither side reports a stage before an analysis ran.
    assert_eq!(mcp_report["status"], json!("none"));
}

// --------------------------------------------------------------- write
// direction: one test per stage editor. Each performs the SAME mutation through
// the MCP tool and through the CLI and compares the resulting sidecar bytes.

#[test]
fn spot_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // op=add: one explicit spot with explicit geometry. No whole-stage set.
    let cli_report = cli_json(&[
        "spot",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--add-heuristic",
        "--center-x",
        "0.4",
        "--center-y",
        "0.3",
        "--radius",
        "5",
        "--feather",
        "0.2",
        "--offset-dx",
        "0.05",
    ]);
    let mcp_report = session.ok(
        "spot",
        json!({
            "op": "add",
            "center_x": 0.4,
            "center_y": 0.3,
            "radius": 5.0,
            "feather": 0.2,
            "offset_dx": 0.05
        }),
    );

    // spot appends no history entry, so this is a literal byte compare.
    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "spot op=add",
    );
    assert_eq!(
        stage_section(&mcp_report, "spot"),
        stage_section(&cli_report, "spot"),
        "lumina_spot op=add must render the same state as the CLI"
    );
    assert_eq!(mcp_report["saved"], json!(true));
    assert_eq!(mcp_report["action"], json!("add-heuristic"));

    // A second, per-field write: update that spot by id.
    let spot_id = mcp_report["spots"][0]["id"].as_str().unwrap().to_string();
    let cli_report = cli_json(&[
        "spot",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--spot-id",
        &spot_id,
        "--set-radius",
        "9",
    ]);
    let mcp_report = session.ok(
        "spot",
        json!({ "op": "update", "spot_id": spot_id, "radius": 9.0 }),
    );
    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "spot op=update",
    );
    assert_eq!(
        stage_section(&mcp_report, "spot"),
        stage_section(&cli_report, "spot")
    );
}

#[test]
fn lens_blur_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // lens-blur appends no history entry: literal byte compare.
    let cli_report = cli_json(&[
        "lens-blur",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-amount",
        "0.75",
        "--set-focal-near",
        "0.1",
        "--set-focal-far",
        "0.6",
        "--set-bokeh",
        "hexagonal",
        "--set-focus-rect",
        "0.1,0.2,0.5,0.4",
    ]);
    let mcp_report = session.ok(
        "lens-blur",
        json!({
            "op": "set_amount",
            "amount": 0.75
        }),
    );
    session.ok(
        "lens-blur",
        json!({ "op": "set_focal_near", "focal_near": 0.1 }),
    );
    session.ok(
        "lens-blur",
        json!({ "op": "set_focal_far", "focal_far": 0.6 }),
    );
    session.ok(
        "lens-blur",
        json!({ "op": "set_bokeh", "bokeh": "hexagonal" }),
    );
    session.ok(
        "lens-blur",
        json!({
            "op": "set_focus_rect",
            "focus_rect": { "x": 0.1, "y": 0.2, "width": 0.5, "height": 0.4 }
        }),
    );
    let final_report = session.ok("lens-blur", json!({ "op": "list" }));

    assert_identical_sidecar(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "lens-blur writes",
    );
    assert_eq!(
        stage_section(&final_report, "lens-blur"),
        stage_section(&cli_report, "lens-blur"),
        "the accumulated lens-blur state must be the same on both sides"
    );
    assert_eq!(mcp_report["saved"], json!(true));
}

#[test]
fn geometry_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // geometry appends exactly one history entry per call, stamped with a
    // millisecond, so the two sidecars are compared with that stamp masked.
    cli_json(&[
        "geometry",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-rotation",
        "12.5",
    ]);
    cli_json(&[
        "geometry",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-crop-aspect",
        "4:5",
    ]);
    let cli_report = cli_json(&[
        "geometry",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--set-lens",
        "distortion_k1:-0.01",
    ]);

    session.ok("geometry", json!({ "op": "set_rotation", "value": 12.5 }));
    session.ok(
        "geometry",
        json!({ "op": "set_crop_aspect", "preset": "4:5" }),
    );
    let mcp_report = session.ok(
        "geometry",
        json!({ "op": "set_lens_field", "field": "distortion_k1", "value": -0.01 }),
    );

    assert_same_sidecar_ignoring_history_stamp(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "geometry writes",
    );
    assert_eq!(
        stage_section(&mcp_report, "geometry"),
        stage_section(&cli_report, "geometry")
    );
    assert_eq!(mcp_report["saved"], json!(true));
    // One history entry per call, on both sides.
    let document = lumina_sidecar::load_sidecar(&session.sidecar()).unwrap();
    assert_eq!(
        document.virtual_copies[0].history.len(),
        3,
        "each geometry call must append exactly one history entry"
    );
}

#[test]
fn upright_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let cli_input = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // The analysis fingerprint is derived from the source bytes, so both sides
    // must persist the identical `input_fingerprint`; that is the strongest
    // statement that the decode/identity path is shared, not duplicated.
    let cli_report = cli_json(&[
        "upright",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--analyze",
    ]);
    let mcp_report = session.ok("upright", json!({ "op": "analyze" }));

    assert_same_sidecar_ignoring_history_stamp(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "upright op=analyze",
    );
    assert_eq!(
        stage_section(&mcp_report, "upright"),
        stage_section(&cli_report, "upright"),
        "both sides must persist the same upright analysis and fingerprint"
    );
    assert_eq!(mcp_report["status"], json!("fresh"));
    assert_eq!(mcp_report["saved"], json!(true));

    // A second write: disable, which must keep the analysis persisted.
    let cli_report = cli_json(&[
        "upright",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
        "--disable",
    ]);
    let mcp_report = session.ok("upright", json!({ "op": "disable" }));
    assert_same_sidecar_ignoring_history_stamp(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "upright op=disable",
    );
    assert_eq!(
        stage_section(&mcp_report, "upright"),
        stage_section(&cli_report, "upright")
    );
    assert_eq!(mcp_report["enabled"], json!(false));
}
