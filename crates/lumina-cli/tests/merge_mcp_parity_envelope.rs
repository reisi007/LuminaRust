//! MCP-PARITY-C: the MERGE-DNG-1 envelope gate on both transports (SOLL §8, T20).
//!
//! A merge bundle may only be replaced when the document kind at the target says
//! so. Two foreign kinds exist, and both must be refused *identically* on the CLI
//! and the MCP side, with the same text:
//!
//! * a **standard** sidecar — an ordinary single-image document with no
//!   `"type": "merge"` envelope, i.e. exactly what a different feature would put
//!   there;
//! * an **unknown** envelope `type`.
//!
//! Both map onto `MergeRunError::Stale` (`bundle.rs:834-854`), so the CLI reports
//! `merge stale:` and MCP `MergeStale` — the class equality is T17's subject,
//! what is pinned here is the *envelope* outcome plus the fact that `force` is
//! the one non-silent recovery path: without it, a bundle whose sidecar was
//! touched could only be repaired by hand.
//!
//! The counterpart on the CLI side is `tests/merge_e2e.rs:727-799`.

#[path = "merge_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, job::*};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

/// The HDR command under test and the MCP twin.
const COMMAND: &str = "merge-hdr";
const HDR_TOOL: &str = "lumina_merge_hdr";

/// The exposure list the fixture merge uses.
const EXPOSURE: (&[f64], &[u32], &[f64]) = (&[0.01, 0.02], &[100, 100], &[8.0, 8.0]);

/// One envelope rejection, on both transports, plus the `force` replacement.
struct EnvelopeCase {
    label: &'static str,
    /// Turns the merge sidecar into a foreign document kind.
    mutate: Box<dyn Fn(&mut Value)>,
    /// The cause fragment the CLI's refusal must carry.
    cause: &'static str,
}

/// T20 — a foreign document kind at the target is refused identically, and only
/// `force` replaces it (§8 T20, MERGE-DNG-1 envelope gate,
/// `merge_e2e.rs:727-799`).
#[test]
fn t20_a_foreign_or_unknown_envelope_is_rejected_identically() {
    for case in [
        EnvelopeCase {
            label: "a standard (non-merge) sidecar at the target",
            mutate: Box::new(|value: &mut Value| {
                value.as_object_mut().unwrap().remove("type");
            }),
            cause: "standard sidecar",
        },
        EnvelopeCase {
            label: "an unknown envelope type",
            mutate: Box::new(|value: &mut Value| {
                value["type"] = json!("merge-hdr");
            }),
            cause: "unknown document type",
        },
    ] {
        let dir = tempfile::tempdir().unwrap();
        let call =
            MergeCall::new(hdr_pair(dir.path())).exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2);
        let first = call.mcp_ok(HDR_TOOL);
        assert_eq!(
            first["cached"],
            json!(false),
            "{}: the fixture merge must succeed first",
            case.label
        );
        let dng = Path::new(first["output"].as_str().unwrap()).to_path_buf();
        let sidecar = sidecar_of(&dng);

        // Rewrite the envelope in place, so the bundle is now foreign.
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(&sidecar).unwrap()).unwrap();
        (case.mutate)(&mut value);
        fs::write(&sidecar, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        // Both transports refuse the same document with the same text.
        let cli_output = call.cli_failure(COMMAND);
        let cli_text = cli_cause(&cli_output);
        assert!(
            cli_text.starts_with("merge stale:") && cli_text.contains(case.cause),
            "{}: expected `merge stale:` naming `{}`, got `{cli_text}`",
            case.label,
            case.cause
        );
        let error = call.mcp(HDR_TOOL).unwrap_err();
        assert_eq!(error.name(), "MergeStale", "{}: {error:?}", case.label);
        assert_eq!(error.code(), -32011);
        assert_eq!(error.data()["error"], json!("MergeStale"));
        assert_eq!(
            merge_clause(&error.message()),
            &cli_text,
            "{}: the envelope rejection must be the same text on both frontends",
            case.label
        );

        // `force` is the explicit, non-silent replacement path (so the bundle is
        // recoverable without hand-editing the sidecar).
        let forced = call.clone().force().mcp_ok(HDR_TOOL);
        assert_eq!(
            forced["cached"],
            json!(false),
            "{}: force rewrites",
            case.label
        );
        assert_eq!(
            sidecar_json(&dng)["type"],
            json!("merge"),
            "{}: force restores the merge envelope",
            case.label
        );
    }
}
