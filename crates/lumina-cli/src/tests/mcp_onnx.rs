use super::*;

/// F-101-F1 smoke test: the `lumina mcp` subcommand delegates to the
/// shared `lumina_mcp` server pipeline; assert the handshake and the full
/// documented tool set through that exact pipeline.
#[cfg(feature = "mcp")]
#[test]
fn mcp_subcommand_pipeline_answers_handshake_and_lists_all_tools() {
    std::env::set_var("LUMINA_MCP_PREVIEW_DIR", std::env::temp_dir());
    let mut server = lumina_mcp::Server::new();
    let handshake = server
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
        .expect("initialize expects a response");
    assert_eq!(handshake["result"]["serverInfo"]["name"], "lumina-mcp");

    let listing = server
        .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
        .expect("tools/list expects a response");
    let tools = listing["result"]["tools"].as_array().unwrap();
    // Drift guard pinned to the SOLL (feature/platform/mcp-server.md):
    // 7 editing tools + lumina_analyze + 4 F-101-F1 CLI-coverage tools
    // + 5 LRPAR-G15-IPTC-S7 metadata tools
    // + 4 MCP-PARITY-A stage-editor tools (spot, lens-blur, geometry, upright).
    assert_eq!(tools.len(), 21, "tool set drifted; update SOLL + tests");
    // The four stage editors must be visible, not just counted.
    for name in [
        "lumina_spot",
        "lumina_lens_blur",
        "lumina_geometry",
        "lumina_upright",
    ] {
        assert!(
            tools.iter().any(|tool| tool["name"] == name),
            "tools/list is missing {name}"
        );
    }
}

// ---- F-082-FOLLOWUP: onnx-rt wiring semantics ----

/// Default build: the deterministic StubBackend is the wiring default,
/// independent of the mask-work gate.
#[cfg(not(feature = "onnx-rt"))]
#[test]
fn default_build_wires_deterministic_stub_regardless_of_gate() {
    let engine = resolve_mask_inference_engine(true).expect("stub must resolve");
    let engine = engine.expect("default build must wire the stub");
    assert!(engine.is_available());
    let frame = ImageFrame::new(4, 4, vec![10u8; 4 * 4 * 4]).unwrap();
    let plane = engine.infer(&frame).expect("stub must infer");
    assert_eq!((plane.width, plane.height), (4, 4));
    assert_eq!(plane.values.len(), 16);

    // The gate flag is irrelevant without `onnx-rt`: the stub is the
    // default even when the run could not request inference.
    assert!(resolve_mask_inference_engine(false)
        .expect("stub must resolve")
        .is_some());
}

/// `onnx-rt`: a loadable, identity-compatible artifact wires the REAL
/// engine (not the stub), and the engine actually infers a matte.
#[cfg(feature = "onnx-rt")]
#[test]
fn onnx_rt_resolves_real_engine_from_working_artifact() {
    let engine = resolve_onnx_engine_from_path(&onnx_test_fixture_path())
        .expect("real engine must load")
        .expect("real engine must be wired");
    assert!(engine.is_available());
    let frame = ImageFrame::new(4, 4, vec![120u8; 4 * 4 * 4]).unwrap();
    // The crafted ReduceMax graph emits a deterministic, uniform matte on a
    // uniform frame (same contract as the lumina-onnx fixture tests).
    let plane = engine.infer(&frame).expect("real engine must infer");
    assert_eq!((plane.width, plane.height), (4, 4));
    let first = plane.values[0];
    assert!(
        plane.values.iter().all(|value| *value == first),
        "uniform frame must yield a uniform matte from the real engine"
    );
}

/// `onnx-rt`: the full env-var path (`resolve_mask_inference_engine(true)`)
/// wires the real engine when `LUMINA_MODEL_PATH` is the runnable test
/// model. All env mutations in the suite write the identical value, so this
/// cannot race other render tests.
#[cfg(feature = "onnx-rt")]
#[test]
fn onnx_rt_full_resolve_uses_real_engine_from_env() {
    std::env::set_var("LUMINA_MODEL_PATH", onnx_test_fixture_path());
    let engine = resolve_mask_inference_engine(true)
        .expect("configured onnx-rt resolve must succeed")
        .expect("real engine must be wired");
    assert!(engine.is_available());
    let frame = ImageFrame::new(2, 2, vec![90u8; 2 * 2 * 4]).unwrap();
    let plane = engine.infer(&frame).expect("real engine must infer");
    assert_eq!((plane.width, plane.height), (2, 2));
}

/// `onnx-rt`: a missing artifact is a HARD error carrying the resolver's
/// `MissingModel` text — never a stub masquerading as a real engine.
#[cfg(feature = "onnx-rt")]
#[test]
fn onnx_rt_missing_artifact_is_hard_error_never_stub() {
    let error = resolve_onnx_engine_from_path(Path::new("/nonexistent/lumina-model.onnx"))
        .err()
        .expect("missing artifact must fail; an engine must never be returned");
    let text = error.to_string();
    assert!(text.contains("is not available"), "{text}");
    assert!(text.contains("no silent fallback"), "{text}");
}

/// `onnx-rt`: a present-but-useless (garbage) artifact is a HARD error,
/// never silently replaced by the stub.
#[cfg(feature = "onnx-rt")]
#[test]
fn onnx_rt_garbage_artifact_is_hard_error_never_stub() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("garbage.onnx");
    fs::write(&path, b"not an onnx model").unwrap();
    let error = resolve_onnx_engine_from_path(&path)
        .err()
        .expect("garbage must fail; an engine must never be returned");
    assert!(error.to_string().contains("no silent fallback"), "{error}");
}

/// `onnx-rt`: without mask work no engine is requested (the gate), so
/// nothing is loaded and nothing fails.
#[cfg(feature = "onnx-rt")]
#[test]
fn onnx_rt_no_mask_work_requests_no_engine() {
    assert!(
        resolve_mask_inference_engine(false)
            .expect("no request must not fail")
            .is_none(),
        "without mask work the CLI must not request any engine"
    );
}
