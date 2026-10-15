//! MCP-PARITY-C: the MCP surface of the two path-based merge tools
//! `lumina_merge_hdr` / `lumina_merge_pano` — argument validation and error
//! classes (SOLL §8, phase 2; T5–T10) plus the §5 payload contract on a
//! successful run. The registry/schema half (phase 1: T1, T3, T4) lives in
//! `merge_tools.rs`, the byte identity against the real `lumina-cli` binary in
//! the later parity slice.
//!
//! The through-line is the §2 Trennregel: JSON shape and type violations are
//! `InvalidParams`; every merge-domain rule keeps the CLI's own error class
//! (`merge failed:`/`merge unsupported:`/`merge missing:` with their stable
//! `data.error` names), so an MCP call never re-classes what the CLI reports
//! as a merge failure (T17).

use serde_json::{json, Value};

#[path = "merge_tools_common/mod.rs"]
mod support;
use support::*;

// ------------------------------------------- T5: shape vs. merge-domain count

/// `inputs` shape violations are `InvalidParams`; the source COUNT is merge
/// domain (`validate_source_count` → `merge failed:`), exactly like the CLI.
/// The explicit-list arity rule (empty or exactly `len(inputs)`) is the one
/// handler-side gestalt check — the CLI also checks it before `run_merge` and
/// reports it as its own message, not as a `MergeRunError`.
#[test]
fn t5_input_shape_is_invalid_params_while_the_count_is_merge_failed() {
    // Shape: missing, non-array, non-string elements, explicit null.
    for args in [
        json!({}),
        json!({ "inputs": "a.png" }),
        json!({ "inputs": [1, 2] }),
        json!({ "inputs": null }),
        json!({ "inputs": { "0": "a.png" } }),
    ] {
        let error = run(MERGE_TOOLS[0], args.clone()).unwrap_err();
        assert_eq!(error.name(), "InvalidParams", "{args}");
    }

    // Count: 0 sources. `validate_source_count` runs before any decode, so no
    // fixture is needed.
    let zero = run(MERGE_TOOLS[0], json!({ "inputs": [] })).unwrap_err();
    assert_eq!(zero.name(), "MergeFailed", "{zero:?}");
    assert!(zero.message().contains("merge failed:"), "{zero:?}");
    assert!(zero.message().contains("got 0"), "{zero:?}");

    // Count: 1 source (a real file — the class must not depend on the file
    // existing; `validate_source_count` runs first).
    let dir = tempfile::tempdir().unwrap();
    let only = gradient_png(dir.path(), "only.png", 1.0, 0);
    let one = run(
        MERGE_TOOLS[0],
        json!({ "inputs": [only], "exposure_times": [0.01] }),
    )
    .unwrap_err();
    assert_eq!(one.name(), "MergeFailed", "{one:?}");
    assert!(one.message().contains("got 1"), "{one:?}");

    // Count: 257 non-existent paths still fail on the count, not on I/O.
    let many: Vec<String> = (0..257).map(|i| format!("/nonexistent/{i}.png")).collect();
    let over = run(MERGE_TOOLS[0], json!({ "inputs": many })).unwrap_err();
    assert_eq!(over.name(), "MergeFailed", "{over:?}");
    assert!(
        over.message().contains("exceeds maximum of 256"),
        "{over:?}"
    );
    assert!(over.message().contains("got 257"), "{over:?}");

    // Arity rule: one value for two inputs is the loud handler-side refusal.
    let arity = run(
        MERGE_TOOLS[0],
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "exposure_times": [0.01] }),
    )
    .unwrap_err();
    assert_eq!(arity.name(), "InvalidParams", "{arity:?}");
    assert!(
        arity.message().contains("expected 0 or 2 values"),
        "{arity:?}"
    );
    // A satisfied arity rule passes the handler and reaches the merge domain:
    // the nonexistent sources then fail as `merge missing:` (FileNotFound),
    // which proves the check above was the only thing that refused.
    let reached = run(
        MERGE_TOOLS[0],
        json!({
            "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"],
            "exposure_times": [0.01, 0.02],
            "isos": [100, 100],
            "f_numbers": [8.0, 8.0],
        }),
    )
    .unwrap_err();
    assert_eq!(reached.name(), "FileNotFound", "{reached:?}");
    assert!(reached.message().contains("merge missing:"), "{reached:?}");
}

// ------------------------------------------- T6: output type vs. output form

/// A present, non-string `output` is a JSON type error (`InvalidParams`); the
/// FORM (`.dng` ending, file name, not a source) is merge domain —
/// `resolve_dng_path` inside `run_merge` rejects it with the CLI's own
/// `merge failed:` class, never as a tool-level refusal (§2 Pkt. 3).
#[test]
fn t6_output_type_error_is_invalid_params_but_output_form_is_merge_failed() {
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(dir.path(), "a.png", 1.0, 0);
    let b = gradient_png(dir.path(), "b.png", 0.5, 0);
    let typed = run(
        MERGE_TOOLS[0],
        json!({
            "inputs": [a, b],
            "exposure_times": [0.01, 0.02],
            "isos": [100, 100],
            "f_numbers": [8.0, 8.0],
            "output": 7,
        }),
    )
    .unwrap_err();
    assert_eq!(typed.name(), "InvalidParams", "{typed:?}");

    // The form errors reach `resolve_dng_path` only after decode + exposure +
    // alignment, so the fixture must be mergeable (same timing as the CLI).
    for output in [
        dir.path().join("out.png").display().to_string(),
        String::new(),
        b.display().to_string(),
    ] {
        let error = run(
            MERGE_TOOLS[0],
            json!({
                "inputs": [a, b],
                "exposure_times": [0.01, 0.02],
                "isos": [100, 100],
                "f_numbers": [8.0, 8.0],
                "output": output,
            }),
        )
        .unwrap_err();
        assert_eq!(error.name(), "MergeFailed", "output `{output}`: {error:?}");
        assert!(error.message().contains("merge failed:"), "{error:?}");
        assert!(
            error.message().contains("--output"),
            "the CLI's own target message must survive: {error:?}"
        );
    }
    // A refusal leaves no DNG behind: the default target was never resolved.
    assert!(!dir.path().join("a-HDR.dng").exists());
}

// --------------------------------------- T7: JSON type vs. value-range split

/// JSON-type violations (`1.5` where an integer is promised, a string where a
/// number is promised, a non-bool `force`) are `InvalidParams`. The VALUE
/// ranges are merge domain: `blend_width_px: 65537` fails in
/// `recipe.validate()` and `max_shift_px: -1` in the library guard — both
/// `merge failed:`, never a handler-side `InvalidParams` (§2 Pkt. 4, T17).
#[test]
fn t7_json_type_errors_are_invalid_params_and_ranges_stay_merge_failed() {
    // Type errors fire at parse time, before any I/O: non-existent paths prove
    // the ordering (a decode failure would be FileNotFound instead).
    let cases: Vec<(Value, &str)> = vec![
        (
            json!({ "inputs": ["a.png", "b.png"], "isos": [1.5, 2] }),
            "isos",
        ),
        (
            json!({ "inputs": ["a.png", "b.png"], "exposure_times": ["x", 0.1] }),
            "exposure_times",
        ),
        (
            json!({ "inputs": ["a.png", "b.png"], "f_numbers": [true, 8.0] }),
            "f_numbers",
        ),
        (
            json!({ "inputs": ["a.png", "b.png"], "force": "yes" }),
            "force",
        ),
        (
            json!({ "inputs": ["a.png", "b.png"], "max_shift_px": 1.5 }),
            "max_shift_px",
        ),
        (
            json!({ "inputs": ["a.png", "b.png"], "max_shift_px": "8" }),
            "max_shift_px",
        ),
        (
            json!({ "inputs": ["a.png", "b.png"], "blend_width_px": -1 }),
            "blend_width_px",
        ),
    ];
    for (args, key) in cases {
        let error = run(MERGE_TOOLS[0], args.clone()).unwrap_err();
        assert_eq!(error.name(), "InvalidParams", "{args}");
        assert!(
            error.message().contains(key),
            "{args}: `{}` does not mention `{key}`",
            error.message()
        );
    }

    // Range violations: merge domain, so a real, mergeable fixture is needed.
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(dir.path(), "a.png", 1.0, 0);
    let b = gradient_png(dir.path(), "b.png", 0.5, 0);
    let blend = run(
        MERGE_TOOLS[0],
        json!({
            "inputs": [a, b],
            "exposure_times": [0.01, 0.02],
            "isos": [100, 100],
            "f_numbers": [8.0, 8.0],
            "blend_width_px": 65537,
        }),
    )
    .unwrap_err();
    assert_eq!(blend.name(), "MergeFailed", "{blend:?}");
    assert!(blend.message().contains("blend_width_px"), "{blend:?}");

    let shift = run(
        MERGE_TOOLS[0],
        json!({
            "inputs": [a, b],
            "exposure_times": [0.01, 0.02],
            "isos": [100, 100],
            "f_numbers": [8.0, 8.0],
            "max_shift_px": -1,
        }),
    )
    .unwrap_err();
    assert_eq!(shift.name(), "MergeFailed", "{shift:?}");
    assert!(shift.message().contains("--max-shift-px"), "{shift:?}");
}

// ------------------------------------------------------- T8: unknown fields

/// Unknown fields are a loud `InvalidParams` via `reject_unknown_fields` —
/// including the fields the CLI JSON *answers* with (`mode`) and the envelope
/// fields `result_payload` would add (`saved`/`action`/`text`): the tool must
/// not accept what its own contract forbids. The refusal happens before any
/// merge work (non-existent paths would otherwise fail as FileNotFound).
#[test]
fn t8_unknown_fields_are_refused_before_any_merge_work() {
    for field in [
        "nope", "mode", "json", "image_id", "saved", "action", "text", "warning",
    ] {
        let mut arguments = serde_json::Map::new();
        arguments.insert(
            "inputs".into(),
            json!(["/nonexistent/a.png", "/nonexistent/b.png"]),
        );
        arguments.insert(field.into(), json!(1));
        let error = run(MERGE_TOOLS[1], Value::Object(arguments)).unwrap_err();
        assert_eq!(error.name(), "InvalidParams", "field `{field}`");
        assert!(
            error
                .message()
                .contains(&format!("unknown field `{field}`")),
            "field `{field}`: {}",
            error.message()
        );
    }
    // A non-object `arguments` value is refused as shape, too.
    let error = run(MERGE_TOOLS[1], json!("inputs")).unwrap_err();
    assert_eq!(error.name(), "InvalidParams");
    assert!(error.message().contains("must be an object"), "{error:?}");
}

// ---------------------------------------------------- T9: missing source file

/// A missing source file is `merge missing:` → `FileNotFound` (-32001, stable
/// name in `data.error`) — the CLI's own message text survives verbatim.
#[test]
fn t9_a_missing_source_is_file_not_found_with_the_merge_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(dir.path(), "a.png", 1.0, 0);
    let gone = dir.path().join("gone.png");
    let args = json!({
        "inputs": [a, gone],
        "exposure_times": [0.01, 0.02],
        "isos": [100, 100],
        "f_numbers": [8.0, 8.0],
    });
    let error = run(MERGE_TOOLS[0], args.clone()).unwrap_err();
    assert_eq!(error.name(), "FileNotFound", "{error:?}");
    assert_eq!(error.code(), -32001);
    assert_eq!(error.data()["error"], json!("FileNotFound"));
    assert!(error.message().contains("merge missing:"), "{error:?}");
    // Same class over the protocol seam: `structuredContent` is `error.data()`
    // and the human-readable text (with the CLI prefix) is `content[0].text`.
    let response = call(MERGE_TOOLS[0], args);
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        json!("FileNotFound")
    );
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("merge missing:"),
        "the CLI prefix must survive in the message: {text}"
    );
}

// --------------------------------------------------------- T10: single source

/// One source is a merge-domain count violation (`merge failed:` →
/// `MergeFailed`), NOT a tool-level `InvalidParams`: the CLI has no other
/// class for it, so MCP must not invent one (T17).
#[test]
fn t10_a_single_source_is_merge_failed_never_invalid_params() {
    let dir = tempfile::tempdir().unwrap();
    let only = gradient_png(dir.path(), "only.png", 1.0, 0);
    let error = run(
        MERGE_TOOLS[0],
        json!({ "inputs": [only], "exposure_times": [0.01], "isos": [100], "f_numbers": [8.0] }),
    )
    .unwrap_err();
    assert_eq!(error.name(), "MergeFailed", "{error:?}");
    assert_ne!(error.name(), "InvalidParams");
    assert!(error.message().contains("merge failed:"), "{error:?}");
    assert!(error.message().contains("at least 2 sources"), "{error:?}");
    assert_eq!(error.code(), -32013);
}

// --------------------------- Slice A: gain_compensation shape vs. domain

/// Slice A: `gain_compensation` shape violations are handler-side
/// `InvalidParams` (a non-string, an unknown method string) — like every
/// other JSON-type error (T7), fired before any I/O (the non-existent paths
/// would otherwise fail as `FileNotFound`). Absent and explicit `null` are the
/// same shape-valid input (`off`), so they reach the merge domain. The
/// pano-only rule with a *valid* value stays merge domain (`merge failed:`,
/// the Slice-A mirror A-T8 proves it on both transports).
#[test]
fn gain_compensation_shape_is_invalid_params_while_absent_and_null_reach_the_domain() {
    for args in [
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": 1 }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": true }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": ["off"] }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": "exif-ratio" }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": "OverlapMedian" }),
    ] {
        let error = run(MERGE_TOOLS[1], args.clone()).unwrap_err();
        assert_eq!(error.name(), "InvalidParams", "{args}");
        assert!(
            error.message().contains("gain_compensation"),
            "{args}: {}",
            error.message()
        );
    }
    // Absent and explicit `null` are shape-valid (`off`): they pass the
    // handler and fail in the merge domain on the missing file, never as
    // `InvalidParams`.
    for args in [
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"] }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": null }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "gain_compensation": "off" }),
    ] {
        let error = run(MERGE_TOOLS[1], args.clone()).unwrap_err();
        assert_eq!(error.name(), "FileNotFound", "{args}: {error:?}");
    }
}

// ------------------------------------------- §5 payload on a successful run

/// The success payload is the CLI `--json` document, field for field (§5):
/// exactly `command`, `output`, `sidecar`, `digest`, `checksum`, `cached`,
/// `status` — no `saved`/`action`/`text`/`mode`/`warnings[]` envelope — plus
/// the optional `warning` string, which appears if and only if the alignment
/// residual exceeded the documented threshold. The 32×24 pair aligns exactly
/// (residual 0, no warning); the §9.1 64×48 anchor residual-warns, which pins
/// the warning branch on the acceptance fixture together with the digest /
/// checksum forms and the DNG re-import at the anchor geometry.
#[test]
fn a_successful_hdr_call_returns_exactly_the_cli_json_fields() {
    let core = [
        "cached", "checksum", "command", "digest", "output", "sidecar", "status",
    ];

    // Clean alignment: the seven fields and nothing else.
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(dir.path(), "a.png", 1.0, 0);
    let b = gradient_png(dir.path(), "b.png", 0.5, 0);
    let payload = ok(
        MERGE_TOOLS[0],
        json!({
            "inputs": [a, b],
            "exposure_times": [0.01, 0.02],
            "isos": [100, 100],
            "f_numbers": [8.0, 8.0],
        }),
    );
    let mut keys: Vec<&str> = payload
        .as_object()
        .expect("a flat payload object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys, core,
        "the payload must carry exactly the seven CLI JSON fields"
    );
    assert_eq!(payload["command"], json!("merge-hdr"));
    assert_eq!(payload["status"], json!("ok"));
    assert_eq!(payload["cached"], json!(false));

    // The §9.1 acceptance anchor: 64×48, levels 40/160, explicit exposition.
    // Its alignment residual is above the documented threshold, so this run
    // also pins the `warning` extra field and its text form.
    let anchor = tempfile::tempdir().unwrap();
    let anchor_a = anchor_png(anchor.path(), "a.png", 40);
    let anchor_b = anchor_png(anchor.path(), "b.png", 160);
    let payload = ok(
        MERGE_TOOLS[0],
        json!({
            "inputs": [anchor_a, anchor_b],
            "exposure_times": [0.004, 0.016],
            "isos": [100, 100],
            "f_numbers": [8.0, 8.0],
        }),
    );
    let mut keys: Vec<&str> = payload
        .as_object()
        .expect("a flat payload object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    let mut with_warning = core.to_vec();
    with_warning.sort_unstable();
    with_warning.push("warning");
    assert_eq!(
        keys, with_warning,
        "the anchor run adds `warning` and nothing else"
    );
    let warning = payload["warning"].as_str().unwrap();
    assert!(
        warning.starts_with("merge-hdr aligned with residual ")
            && warning.ends_with("result kept, nothing cropped silently"),
        "unexpected warning text: {warning}"
    );

    // Digest/checksum form, `checksum == blake3(DNG)`, sidecar beside the DNG.
    for field in ["digest", "checksum"] {
        let value = payload[field].as_str().unwrap();
        assert!(
            value.starts_with("blake3:") && value.len() == "blake3:".len() + 64,
            "{field} must be `blake3:<64 hex>`, got `{value}`"
        );
    }
    let dng = std::path::Path::new(payload["output"].as_str().unwrap()).to_path_buf();
    assert!(dng.is_file(), "the DNG must exist under `output`");
    assert_eq!(payload["checksum"], json!(blake3_file(&dng)));
    let sidecar = lumina_sidecar::sidecar_path_for(&dng);
    assert_eq!(
        payload["sidecar"],
        json!(sidecar.display().to_string()),
        "the sidecar path is `<dng>.lumina.json`"
    );
    assert!(sidecar.is_file());
    // The written DNG re-imports at the anchor geometry (64×48).
    let decoded = lumina_raw::decode_bytes(&std::fs::read(&dng).unwrap(), "a-HDR.dng")
        .expect("DNG re-import");
    assert_eq!((decoded.frame.width, decoded.frame.height), (64, 48));
}
