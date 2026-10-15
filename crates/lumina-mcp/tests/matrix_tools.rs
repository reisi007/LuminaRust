//! MCP-PARITY-C: the MCP surface of `lumina_matrix` — registry, schema,
//! argument validation and error classes (SOLL §9: T1–T4, T6–T14). Pixel
//! parity (T15+) is a later slice (Impl-D).
//!
//! Convention (like `merge_tools.rs`): a fresh `Server` per call, the real
//! `tools/list` seam for registry claims, `dispatch_tool` directly for the
//! error-class claims (`McpError` exposes `name()`/`code()`).

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::error::McpError;
use lumina_mcp::tools::{dispatch_tool, matrix};
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

const NAME: &str = matrix::NAME;

fn server() -> Server {
    Server::with_preview_dir(std::env::temp_dir())
}

/// Calls the handler and returns the mapped [`McpError`].
fn run(args: Value) -> Result<Value, McpError> {
    dispatch_tool(&mut server(), NAME, &args)
}

fn refused(args: Value) -> McpError {
    run(args).expect_err("matrix call must be refused")
}

/// A tempdir with a 64×48 gradient sample (the `matrix_e2e.rs` anchor shape).
/// Written always, so every rejection is safe regardless of check order.
fn fixture_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut pixels = Vec::with_capacity(64 * 48 * 4);
    for y in 0..48u32 {
        for x in 0..64u32 {
            pixels.extend_from_slice(&[(x * 4) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255]);
        }
    }
    let frame = ImageFrame::new(64, 48, pixels).unwrap();
    fs::write(
        dir.path().join("sample.png"),
        frame.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    dir
}

fn write_set(dir: &tempfile::TempDir, body: &str) -> PathBuf {
    let path = dir.path().join("recipe-set.json");
    fs::write(&path, body).unwrap();
    path
}

const SAMPLES: &str = r#"[{"id": "sample", "path": "sample.png"}]"#;
const IDENTITY: &str = r#"{"adjustments": {}}"#;

/// A full recipe-set document from fragments.
fn full_set(schema: &str, pipeline: &str, samples: &str, recipes: &str) -> String {
    format!(
        r#"{{"schema_version": {schema}, "pipeline_version": {pipeline}, "comparison_width": 48, "samples": {samples}, "recipes": {recipes}}}"#
    )
}

/// A minimal valid set over one caller-supplied recipes array.
fn set_json(recipes: &str) -> String {
    full_set("1", r#""raster-mvp-1""#, SAMPLES, recipes)
}

/// One recipe entry with an `exact` identity default around `recipe`.
fn entry(id: &str, recipe: &str) -> String {
    format!(
        r#"{{"id": "{id}", "goals": ["G-01"], "stages": ["exposure"], "tolerance": "exact", "expected_route": "gpu", "recipe": {recipe}}}"#
    )
}

fn minimal_set(dir: &tempfile::TempDir) -> PathBuf {
    write_set(
        dir,
        &set_json(&format!("[{}]", entry("identity", IDENTITY))),
    )
}

// ---------------------------------------------------------- T1: registry

/// `lumina_matrix` is listed, known and dispatchable — and a listed-but-dead
/// tool (a missing `dispatch_tool` arm) fails here: the unknown-field call
/// must reach the handler (`InvalidParams`), never the `MethodNotFound`
/// fall-through.
#[test]
fn t1_matrix_tool_is_registered_in_list_dispatch_and_is_known_tool() {
    let definitions = lumina_mcp::tools::list_tool_definitions();
    let listed: Vec<&str> = definitions
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(
        listed.contains(&NAME),
        "missing from tools/list: {listed:?}"
    );
    assert!(lumina_mcp::tools::is_known_tool(NAME));
    let response = server()
        .handle_message(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names.iter().filter(|name| **name == NAME).count(),
        1,
        "tools/list must report {NAME} exactly once: {names:?}"
    );
    let error = refused(json!({"bogus_field": true}));
    assert_eq!(
        error.name(),
        "InvalidParams",
        "no dispatch arm: {}",
        error.message()
    );
}

// ------------------------------------------------- T2: committed-set pin

/// The committed set is read (never written): 2 samples, 11 recipes, 10×
/// `gpu` + the documented CPU exemption, comparison width 384.
#[test]
fn t2_committed_recipe_set_content_is_pinned_read_only() {
    let path = format!(
        "{}/../../testdata/matrix/recipe-set.v1.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = fs::read_to_string(&path).unwrap();
    let set: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(set["comparison_width"], 384);
    assert_eq!(set["samples"].as_array().unwrap().len(), 2);
    let recipes = set["recipes"].as_array().unwrap();
    assert_eq!(recipes.len(), 11);
    let mut gpu = 0;
    let mut cpu = 0;
    for recipe in recipes {
        match &recipe["expected_route"] {
            Value::String(route) if route == "gpu" => gpu += 1,
            Value::Object(_) => {
                assert_eq!(
                    recipe["expected_route"],
                    json!({"cpu": "geometry (default content crop)"}),
                    "only the documented CPU exemption may exist"
                );
                cpu += 1;
            }
            other => panic!("unexpected expected_route: {other}"),
        }
    }
    assert_eq!((gpu, cpu), (10, 1));
}

// ---------------------------------------------------------- T3: schema

/// `tools/list` publishes the §2 contract: no unknown fields, nothing
/// required, the seven fields with their types, `comparison_width` integer
/// with descriptive `minimum: 1`.
#[test]
fn t3_tools_list_publishes_the_schema_with_the_soll_contract() {
    let response = server()
        .handle_message(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    let tool = tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some(NAME))
        .expect("tool");
    assert!(!tool["description"].as_str().unwrap().is_empty());
    let schema = &tool["inputSchema"];
    assert_eq!(schema["type"], json!("object"));
    assert_eq!(schema["additionalProperties"], json!(false));
    assert_eq!(schema["required"], json!([]));
    let properties = schema["properties"].as_object().unwrap();
    let mut keys: Vec<&str> = properties.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "comparison_width",
            "golden_dir",
            "recipe_set",
            "recipes",
            "require_gpu",
            "update_goldens",
            "work_dir"
        ]
    );
    for key in ["recipe_set", "golden_dir", "work_dir"] {
        assert_eq!(properties[key]["type"], json!(["string", "null"]), "{key}");
    }
    assert_eq!(properties["recipes"]["type"], json!("array"));
    assert_eq!(properties["recipes"]["items"]["type"], json!("string"));
    assert_eq!(properties["comparison_width"]["type"], json!("integer"));
    assert_eq!(properties["comparison_width"]["minimum"], json!(1));
    for key in ["update_goldens", "require_gpu"] {
        assert_eq!(properties[key]["type"], json!("boolean"), "{key}");
    }
}

// -------------------------------------- T4: unknown fields stay InvalidParams

/// The shape gate runs before any file access: an unknown field plus a
/// missing set file is still `InvalidParams`, never `Matrix`.
#[test]
fn t4_unknown_fields_are_rejected_before_any_file_access() {
    let error = refused(json!({"bogus": 1, "recipe_set": "/nonexistent/set.json"}));
    assert_eq!(error.name(), "InvalidParams");
    assert_eq!(error.code(), -32602);
}

// -------------------------------------------- T6: type errors stay InvalidParams

/// JSON-shape/type violations (§2): only the handler may report them.
#[test]
fn t6_json_shape_violations_are_invalid_params() {
    let cases: &[(&str, Value)] = &[
        ("recipe_set number", json!({"recipe_set": 1})),
        ("recipes string", json!({"recipes": "identity"})),
        ("recipes non-string element", json!({"recipes": [1]})),
        ("recipes mixed element", json!({"recipes": ["identity", 1]})),
        ("width float", json!({"comparison_width": 1.5})),
        ("width string", json!({"comparison_width": "384"})),
        ("width negative", json!({"comparison_width": -1})),
        ("update_goldens string", json!({"update_goldens": "yes"})),
        ("require_gpu number", json!({"require_gpu": 1})),
    ];
    for (what, args) in cases {
        let error = refused(args.clone());
        assert_eq!(error.name(), "InvalidParams", "{what}: {}", error.message());
        assert_eq!(error.code(), -32602, "{what}");
    }
}

// --------------------------------- T7: missing set file stays Matrix

/// A missing set file is domain (`load_recipe_set`), not a shape error — the
/// text carries the path.
#[test]
fn t7_missing_recipe_set_file_is_a_matrix_error() {
    let dir = fixture_dir();
    let missing = dir.path().join("nope.json");
    let error = refused(json!({"recipe_set": missing}));
    assert_eq!(error.name(), "MatrixError");
    assert_eq!(error.code(), -32014);
    assert!(error.message().contains("nope.json"), "{}", error.message());
}

// --------------------------------- T8: comparison_width 0 vs null

/// `0` is a valid integer that fails loudly in the domain; `null` takes the
/// set default (48 here, proven by a real baseline run).
#[test]
fn t8_zero_width_is_domain_null_takes_the_set_default() {
    let dir = fixture_dir();
    let set = minimal_set(&dir);
    let error = refused(json!({"recipe_set": set, "comparison_width": 0}));
    assert_eq!(error.name(), "MatrixError");
    assert!(
        error.message().contains("invalid comparison_width 0"),
        "{}",
        error.message()
    );

    let payload = run(json!({"recipe_set": set, "update_goldens": true})).expect("baseline");
    assert_eq!(payload["command"], "matrix");
    assert_eq!(payload["mode"], "baseline");
    assert_eq!(payload["comparison_width"], 48);
    assert_eq!(payload["pairs_total"], 1);
    assert_eq!(payload["pairs"][0]["status"], "baseline-created");
}

// --------------------------------- T9: empty selection, unknown id

#[test]
fn t9_empty_and_unknown_recipe_selections_are_matrix_errors() {
    // NOTE vs. SOLL §9 T9: a set with `"recipes": []` fails in
    // `load_recipe_set` ("declares no recipes") before the runner's "selected
    // no recipes" branch is reachable — this is the real CLI text on the same
    // input (shared runner), so the class parity pinned here is exact.
    let dir = fixture_dir();
    let empty = write_set(&dir, &full_set("1", r#""raster-mvp-1""#, SAMPLES, "[]"));
    let error = refused(json!({"recipe_set": empty}));
    assert_eq!(error.name(), "MatrixError");
    assert!(
        error.message().contains("declares no recipes"),
        "{}",
        error.message()
    );

    let set = minimal_set(&dir);
    let error = refused(json!({"recipe_set": set, "recipes": ["nope"]}));
    assert_eq!(error.name(), "MatrixError");
    assert!(
        error.message().contains("unknown recipe id"),
        "{}",
        error.message()
    );
}

// --------------------------------- T10: missing sample

#[test]
fn t10_missing_sample_file_is_a_matrix_error() {
    let dir = tempfile::tempdir().unwrap();
    let set = write_set(
        &dir,
        &set_json(&format!("[{}]", entry("identity", IDENTITY))),
    );
    let error = refused(json!({"recipe_set": set}));
    assert_eq!(error.name(), "MatrixError");
    assert!(
        error
            .message()
            .contains("matrix sample `sample` not found at"),
        "{}",
        error.message()
    );
}

// --------------------------------- T11: six invalid sets

/// Every set-validation rule (§7) keeps the CLI text under `MatrixError`.
#[test]
fn t11_invalid_sets_are_matrix_errors_with_the_cli_text() {
    let dup_samples = r#"[{"id": "sample", "path": "a.png"}, {"id": "sample", "path": "b.png"}]"#;
    let abs_sample = r#"[{"id": "sample", "path": "/tmp/abs.png"}]"#;
    let cpu_empty = format!(
        r#"[{{"id": "r", "goals": [], "stages": [], "tolerance": "exact", "expected_route": {{"cpu": ""}}, "recipe": {IDENTITY}}}]"#
    );
    let identity = format!("[{}]", entry("identity", IDENTITY));
    let cases: &[(&str, String, &str)] = &[
        (
            "schema_version",
            full_set("2", r#""raster-mvp-1""#, SAMPLES, &identity),
            "unsupported schema_version",
        ),
        (
            "pipeline_version",
            full_set("1", r#""nope""#, SAMPLES, &identity),
            "targets pipeline_version",
        ),
        (
            "no samples",
            full_set("1", r#""raster-mvp-1""#, "[]", &identity),
            "declares no samples",
        ),
        (
            "absolute path",
            full_set("1", r#""raster-mvp-1""#, abs_sample, &identity),
            "must be relative",
        ),
        (
            "duplicate id",
            full_set("1", r#""raster-mvp-1""#, dup_samples, &identity),
            "duplicate sample id",
        ),
        (
            "empty cpu reason",
            full_set("1", r#""raster-mvp-1""#, SAMPLES, &cpu_empty),
            "empty CPU-route reason",
        ),
    ];
    for (what, body, expect) in cases {
        let dir = fixture_dir();
        let set = write_set(&dir, body);
        let error = refused(json!({"recipe_set": set}));
        assert_eq!(error.name(), "MatrixError", "{what}");
        assert_eq!(error.code(), -32014, "{what}");
        assert!(
            error.message().contains(expect),
            "{what}: {}",
            error.message()
        );
    }
}

// --------------------------------- T12: six loud recipe rejections

/// Artifact-backed recipes and typo keys abort loudly (never skipped); the
/// foreign spot mode fails the typed `EditRecipe` parse first, which is the
/// real CLI text on that input.
#[test]
fn t12_artifact_backed_and_typo_recipes_are_rejected_loudly() {
    let depth = r#"{"adjustments": {}, "lens_blur": {"version": 1, "enabled": true, "focus_rect": {"x": 0.2, "y": 0.2, "width": 0.4, "height": 0.4}, "focal_near": 0.0, "focal_far": 0.2, "blur_amount": 0.5, "bokeh": "round", "depth_artifact": {"relative_path": "depth.bin", "sha256": "abc"}}}"#;
    let cases: &[(&str, String, &str)] = &[
        (
            "source_actions",
            entry(
                "r",
                r#"{"adjustments": {}, "source_actions": [{"version": 1, "kind": "dustremoval", "artifact": {"id": "r1", "relative_path": "r.bin", "checksum": "abc"}}]}"#,
            ),
            "references `source_actions`",
        ),
        (
            "generative_edit",
            entry(
                "r",
                r#"{"adjustments": {}, "generative_edit": {"version": 1}}"#,
            ),
            "references `generative_edit`",
        ),
        ("depth_artifact", entry("r", depth), "depth_artifact"),
        (
            "generative spot",
            entry(
                "r",
                r#"{"adjustments": {}, "spot_removals": [{"id": "s1", "version": 1, "mode": "generative"}]}"#,
            ),
            "generative spot removal",
        ),
        (
            "foreign spot mode",
            entry(
                "r",
                r#"{"adjustments": {}, "spot_removals": [{"id": "s1", "version": 1, "mode": "clone"}]}"#,
            ),
            "not a valid EditRecipe",
        ),
        (
            "typo key",
            entry("r", r#"{"geometri": {"version": 1}}"#),
            "unknown top-level key",
        ),
    ];
    for (what, recipe_entry, expect) in cases {
        let dir = fixture_dir();
        let set = write_set(&dir, &set_json(&format!("[{recipe_entry}]")));
        let error = refused(json!({"recipe_set": set}));
        assert_eq!(error.name(), "MatrixError", "{what}");
        assert!(
            error.message().contains(expect),
            "{what}: {}",
            error.message()
        );
    }
}

// --------------------------------- T13: flag combination stays domain

/// `require_gpu` + `update_goldens` fails before any I/O with the CLI text.
#[test]
fn t13_require_gpu_with_update_goldens_is_a_matrix_error() {
    let error = refused(json!({"require_gpu": true, "update_goldens": true}));
    assert_eq!(error.name(), "MatrixError");
    assert!(
        error
            .message()
            .contains("cannot be combined with --update-goldens"),
        "{}",
        error.message()
    );
}

// --------------------------------- T14: require_gpu without a GPU backend

/// CPU build: loud `Matrix`, never a silent CPU fallback.
#[cfg(not(feature = "gpu"))]
#[test]
fn t14_require_gpu_without_backend_is_a_matrix_error() {
    let error = refused(json!({"require_gpu": true}));
    assert_eq!(error.name(), "MatrixError");
    assert!(
        error.message().contains("no GPU backend"),
        "{}",
        error.message()
    );
}

/// GPU build: the flag must never degrade to `InvalidParams` — the outcome
/// (green verify or loud route verdict) depends on the adapter, the class
/// does not.
#[cfg(feature = "gpu")]
#[test]
fn t14_require_gpu_in_gpu_build_never_degrades_to_invalid_params() {
    let dir = fixture_dir();
    let set = minimal_set(&dir);
    let baseline = run(json!({"recipe_set": set, "update_goldens": true})).expect("baseline");
    assert_eq!(baseline["mode"], "baseline");
    match run(json!({"recipe_set": set, "require_gpu": true})) {
        Ok(payload) => assert_eq!(payload["mode"], "verify"),
        Err(error) => assert_eq!(error.name(), "MatrixError", "{}", error.message()),
    }
}
