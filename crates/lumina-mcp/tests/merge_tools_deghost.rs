//! Slice B (deghosting): the MCP argument surface of the `deghost` field —
//! JSON shape vs. merge domain (SOLL §2 Trennregel, B.5/B.6).
//!
//! Own file rather than an addition to `merge_tools_validation.rs`: that suite
//! is the parity-C core contract and carries its own file-size entry, and a
//! feature-scoped argument test is the same split the CLI side uses
//! (`tests/merge_deghost_parity.rs` next to `tests/merge_gain_parity.rs`).
//!
//! The through-line is the same as the core suite's: a JSON type violation is
//! `InvalidParams`; every merge-domain rule keeps the CLI's own error class.
//! For `deghost` the split is unusually fine-grained, because the field is
//! numeric on the wire:
//!
//! - SHAPE (handler-side `InvalidParams`): a value that is neither a number
//!   nor the `off` string, a non-finite number, and a number outside the
//!   documented range `(0.05, 2.0]` — checked before any I/O, exactly like the
//!   other JSON-type errors;
//! - DOMAIN (`merge failed:` via `MergeRunError::Failed`): the HDR-only rule
//!   with a *valid* threshold on `merge-pano`, which the CLI-side mirror
//!   B-T7 proves on both transports;
//! - the range is additionally enforced in `recipe.validate()`, so a persisted
//!   out-of-range threshold is `Invalid` → `merge failed:` even when it never
//!   came through a frontend.

use serde_json::json;

#[path = "merge_tools_common/mod.rs"]
mod support;
use support::*;

#[test]
fn deghost_shape_is_invalid_params_while_absent_and_null_reach_the_domain() {
    for args in [
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": "on" }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": true }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": [0.25] }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": 0 }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": 0.05 }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": 2.1 }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": -1.0 }),
    ] {
        let error = run(MERGE_TOOLS[0], args.clone()).unwrap_err();
        assert_eq!(error.name(), "InvalidParams", "{args}");
        assert!(
            error.message().contains("deghost"),
            "{args}: {}",
            error.message()
        );
    }
    // Absent, explicit `null`, the `off` string and an in-range number are
    // shape-valid: they pass the handler and fail in the merge domain on the
    // missing file, never as `InvalidParams`.
    for args in [
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"] }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": null }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": "off" }),
        json!({ "inputs": ["/nonexistent/a.png", "/nonexistent/b.png"], "deghost": 0.25 }),
    ] {
        let error = run(MERGE_TOOLS[0], args.clone()).unwrap_err();
        assert_eq!(error.name(), "FileNotFound", "{args}: {error:?}");
    }
}
