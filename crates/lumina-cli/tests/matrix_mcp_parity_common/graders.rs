//! MCP-PARITY-C: the graders of the matrix byte-parity suite.
//!
//! They answer four questions, one per group, and nothing else:
//!
//! 1. **Do the two transports render one report?** [`assert_same_report`]
//!    compares an MCP `structuredContent` against the CLI's `--json` document
//!    under exactly the two §3.1 masks (`duration_ms`, absolute paths) —
//!    everything else, including the `psnr_db` **strings**, `expected_route`,
//!    `route` and `route_reasons`, is compared literally. That is the "1:1
//!    fields" claim of §3 and §10 (4). [`assert_report_field_set`] pins the
//!    field set itself, so a tool cannot answer with the right *values* in
//!    the wrong *shape* (§10 (1)).
//! 2. **Do the two transports write one artifact?** [`assert_identical_files`]
//!    is a raw byte compare with no mask: golden PNGs, exports and diff
//!    images are deterministic renders, so nothing in them is
//!    process-dependent.
//! 3. **Do the two transports refuse identically?** [`cli_cause`] /
//!    [`mcp_cause`] extract the one loud line of a refusal on each side, so a
//!    test can compare the *text* of a rejection and not merely its
//!    existence (§7: the canon is the texts).
//! 4. **Does a refusal leave nothing behind?** [`snapshot`] and
//!    [`content_fingerprint`] serve the "a refusal leaves nothing" claim
//!    (T20): a content fingerprint, not a cryptographic digest, because its
//!    only job is to notice a changed byte.

#![allow(dead_code)]

use super::job::stderr_of;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// The ten `--json` / `structuredContent` top-level fields of §3: the CLI
/// emits them and the MCP tool must emit them unchanged — no `saved`,
/// `action`, `text`, `warnings[]`.
pub const TOP_FIELDS: [&str; 10] = [
    "command",
    "mode",
    "recipe_set",
    "golden_dir",
    "comparison_width",
    "pairs_total",
    "passed",
    "failed",
    "duration_ms",
    "pairs",
];

/// The thirteen per-pair fields of §3 (`matrix_report.rs:173-193`).
pub const PAIR_FIELDS: [&str; 13] = [
    "sample",
    "recipe",
    "status",
    "psnr_db",
    "min_psnr_db",
    "max_abs_diff",
    "duration_ms",
    "expected_route",
    "route",
    "route_reasons",
    "golden",
    "output",
    "message",
];

/// The CLI's loud refusal line: `error: <text>` (`main.rs:1529`). Stderr also
/// carries the stderr logger's `[lumina][INFO]` lines, so the cause is the one
/// line starting with `error:`.
pub fn cli_cause(output: &std::process::Output) -> String {
    let stderr = stderr_of(output);
    stderr
        .lines()
        .find_map(|line| line.strip_prefix("error: "))
        .unwrap_or_else(|| panic!("no `error:` line in stderr: {stderr}"))
        .to_string()
}

/// The shared cause text of an MCP refusal. The message is
/// `<StableName>: <text>`, so the text is what follows the name.
pub fn mcp_cause(error: &lumina_mcp::error::McpError) -> String {
    let prefix = format!("{}: ", error.name());
    error
        .message()
        .strip_prefix(&prefix)
        .map(str::to_string)
        .unwrap_or_else(|| error.message())
}

/// Rewrites the two fixture roots to one placeholder inside free text, so a
/// refusal naming an absolute path is comparable across the two trees. Every
/// path-carrying refusal names a file that lives at a different absolute path
/// in each tree — the path is not the state.
pub fn normalize_roots_text(text: &str, roots: [&Path; 2]) -> String {
    roots.iter().fold(text.to_string(), |text, root| {
        text.replace(&root.display().to_string(), "<TREE>")
    })
}

/// Masks the wall-clock stamp of a red-run summary
/// (`matrix failed: N of M pair(s) not green (12 ms)`): the count is state,
/// the milliseconds are not. Applied to both sides before the compare, so the
/// summary text itself stays under comparison.
pub fn mask_run_ms(text: &str) -> String {
    let mut out = text.to_string();
    let mut start = 0;
    while let Some(open) = out[start..].find(" (") {
        let open = start + open;
        if let Some(close) = out[open..].find(" ms)") {
            let close = open + close + " ms)".len();
            out.replace_range(open..close, " (<MS>)");
            start = open + " (<MS>)".len();
        } else {
            break;
        }
    }
    out
}

/// Applies both §3.1 masks to a report: drops `duration_ms` at the top level
/// and in every pair, and rewrites both fixture roots to `<TREE>` across the
/// whole serialised document (fields and any path echoed inside `message`).
/// Everything else stays raw — that is the point.
pub fn masked(payload: &Value, mcp_dir: &Path, cli_dir: &Path) -> Value {
    let mut value: Value = serde_json::from_str(&normalize_roots_text(
        &serde_json::to_string(payload).unwrap(),
        [mcp_dir, cli_dir],
    ))
    .unwrap_or_else(|error| panic!("not valid JSON after normalisation ({error}): {payload}"));
    if let Some(object) = value.as_object_mut() {
        object.remove("duration_ms");
        if let Some(pairs) = object.get_mut("pairs").and_then(Value::as_array_mut) {
            for pair in pairs {
                if let Some(pair) = pair.as_object_mut() {
                    pair.remove("duration_ms");
                }
            }
        }
    }
    value
}

/// Compares an MCP `structuredContent` against a CLI `--json` document under
/// exactly the two §3.1 masks. **Everything else must be equal** — so the MCP
/// tool also invents no extra key.
pub fn assert_same_report(label: &str, mcp: &Value, cli: &Value, mcp_dir: &Path, cli_dir: &Path) {
    assert_eq!(
        masked(mcp, mcp_dir, cli_dir),
        masked(cli, mcp_dir, cli_dir),
        "{label}: the MCP payload must render the same state as the CLI document"
    );
}

/// Asserts that the payload carries exactly the §3 field set: the ten
/// top-level fields and, per pair, the thirteen pair fields — and nothing
/// invented (§10 (1): no `saved`/`action`/`text`, no `warnings[]`).
pub fn assert_report_field_set(label: &str, payload: &Value) {
    let mut keys: Vec<&str> = payload
        .as_object()
        .unwrap_or_else(|| panic!("{label}: a flat payload object, got {payload}"))
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    let mut expected = TOP_FIELDS.to_vec();
    expected.sort_unstable();
    assert_eq!(keys, expected, "{label}: exactly the §3 top-level fields");
    for (index, pair) in payload["pairs"]
        .as_array()
        .unwrap_or_else(|| panic!("{label}: `pairs` must be an array: {payload}"))
        .iter()
        .enumerate()
    {
        let mut pair_keys: Vec<&str> = pair
            .as_object()
            .unwrap_or_else(|| panic!("{label}: pair {index} must be an object: {pair}"))
            .keys()
            .map(String::as_str)
            .collect();
        pair_keys.sort_unstable();
        let mut expected_pair = PAIR_FIELDS.to_vec();
        expected_pair.sort_unstable();
        assert_eq!(
            pair_keys, expected_pair,
            "{label}: exactly the §3 pair fields in pair {index}"
        );
    }
}

/// Asserts that two produced files are byte-identical, with no mask at all.
pub fn assert_identical_files(a: &Path, b: &Path, label: &str) {
    assert_eq!(
        super::fixtures::bytes(a),
        super::fixtures::bytes(b),
        "{label}: the two files must be byte-identical"
    );
}

/// The `output` (full-resolution export) path a report carries for `recipe`.
pub fn export_of(payload: &Value, recipe: &str) -> PathBuf {
    payload["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair["recipe"] == json!(recipe))
        .unwrap_or_else(|| panic!("no pair for recipe `{recipe}` in {payload}"))["output"]
        .as_str()
        .map(PathBuf::from)
        .unwrap()
}

/// The `golden` path a report carries for `recipe`.
pub fn golden_of(payload: &Value, recipe: &str) -> PathBuf {
    payload["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair["recipe"] == json!(recipe))
        .unwrap_or_else(|| panic!("no pair for recipe `{recipe}` in {payload}"))["golden"]
        .as_str()
        .map(PathBuf::from)
        .unwrap()
}

// ----------------------------------------------------- snapshot: T20 support

/// The two byte-identical fixture trees both transports run on.
///
/// The trees are created empty; the fixtures are written by the caller. A
/// snapshot for the parity premise is taken *before* a run — the premise
/// (identical input) is checked by each test, never assumed.
pub fn both_trees(root: &Path) -> (PathBuf, PathBuf) {
    let (cli_dir, mcp_dir) = (root.join("cli"), root.join("mcp"));
    fs::create_dir_all(&cli_dir).unwrap();
    fs::create_dir_all(&mcp_dir).unwrap();
    (cli_dir, mcp_dir)
}

/// A stable `name -> (length, content fingerprint)` snapshot of every file under
/// `dir`, so a refused call can be proven to have changed nothing.
pub fn snapshot(dir: &Path) -> Vec<(String, usize, u64)> {
    let mut out = Vec::new();
    collect(dir, dir, &mut out);
    out.sort();
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, usize, u64)>) {
    let mut entries: Vec<std::path::PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let bytes = super::fixtures::bytes(&path);
            out.push((
                path.strip_prefix(root).unwrap().display().to_string(),
                bytes.len(),
                content_fingerprint(&bytes),
            ));
        }
    }
}

/// FNV-1a 64: a content fingerprint, not a cryptographic digest.
pub fn content_fingerprint(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}
