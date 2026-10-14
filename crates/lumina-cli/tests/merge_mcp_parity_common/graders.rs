//! MCP-PARITY-C: the graders of the byte-parity suite.
//!
//! They answer three questions, one per group, and nothing else:
//!
//! 1. **Do the two transports render one payload?** [`assert_same_payload`]
//!    compares an MCP `structuredContent` against the CLI's `--json` document,
//!    normalising only the two fixture roots (both echo absolute paths) and
//!    dropping only the one MCP-only field. Everything else is compared
//!    literally — that is the "1:1 fields" claim of §5 and §9.1 (1)–(3).
//!    [`assert_payload_field_set`] pins the field set itself, so a tool cannot
//!    answer with the right *values* in the wrong *shape*.
//! 2. **Do the two transports write one artifact?** [`assert_identical_files`]
//!    is a raw byte compare with no mask: the merge bundle appends no history
//!    entry, so nothing in it is process-dependent (the same argument
//!    `library_parity_common` makes for MCP-PARITY-B).
//! 3. **Do the two transports refuse identically?** [`cli_cause`] /
//!    [`mcp_cause`] / [`merge_clause`] extract the one piece of a refusal that
//!    SOLL §3 requires to survive the transport mapping verbatim — the
//!    canonical `merge <class>: …` clause — so a test can compare the *text* of
//!    a rejection and not merely its existence.
//!
//! [`snapshot`] and [`content_fingerprint`] serve the "a refusal leaves
//! nothing behind" claim (T18): a content fingerprint, not a cryptographic
//! digest, because its only job is to notice a changed byte.

#![allow(dead_code)]

use super::job::stderr_of;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The seven `--json` / `structuredContent` fields of §5: the CLI emits them and
/// the MCP tool must emit them unchanged.
pub const CORE_FIELDS: [&str; 7] = [
    "cached", "checksum", "command", "digest", "output", "sidecar", "status",
];

/// The ONE field the MCP payload adds on top of the CLI document (§5): the
/// residual warning, a string, set only when the alignment residual exceeded
/// the documented threshold. Nothing else may appear — no `saved`, `action`,
/// `text`, `mode` or `warnings[]`.
pub const MCP_ONLY_FIELDS: [&str; 1] = ["warning"];

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

/// The canonical `merge <class>: …` clause inside any refusal text.
///
/// This is the piece of a message that SOLL §3 requires to survive the
/// transport mapping verbatim ("damit `merge missing:` / `merge stale:` / …
/// clientseitig substring-prüfbar bleibt"), so it is what gets compared between
/// the two frontends. A refusal without a merge class — the list-arity rule,
/// which the CLI reports as its own message before `run_merge` — yields the
/// whole text.
pub fn merge_clause(text: &str) -> &str {
    const PREFIXES: [&str; 4] = [
        "merge missing:",
        "merge stale:",
        "merge unsupported:",
        "merge failed:",
    ];
    let start = PREFIXES
        .iter()
        .filter_map(|prefix| text.find(prefix))
        .min()
        .unwrap_or(0);
    &text[start..]
}

/// [`merge_clause`] with the two fixture roots rewritten, so a message naming an
/// absolute artifact path is comparable across the two trees. Every stale
/// refusal names the sidecar it read, and that sidecar lives at a different
/// absolute path in each tree — the path is not the state.
pub fn merge_clause_of(text: &str, roots: [&Path; 2]) -> String {
    roots
        .iter()
        .fold(merge_clause(text).to_string(), |text, root| {
            text.replace(&root.display().to_string(), "<TREE>")
        })
}

/// Rewrites the two fixture roots to one placeholder, so the compared documents
/// differ only in state and not in the address of the tree they were produced
/// in (the same idea as `library_parity_common::normalize_paths`).
pub fn normalize_paths(payload: &Value, mcp_dir: &Path, cli_dir: &Path) -> Value {
    let text = serde_json::to_string(payload).unwrap();
    let replaced = text
        .replace(&mcp_dir.display().to_string(), "<TREE>")
        .replace(&cli_dir.display().to_string(), "<TREE>");
    serde_json::from_str(&replaced)
        .unwrap_or_else(|error| panic!("not valid JSON after normalisation ({error}): {replaced}"))
}

/// Compares an MCP `structuredContent` against a CLI `--json` document.
///
/// Both roots are normalised and the one MCP-only field is dropped; **everything
/// else** — every §5 field, including `digest`, `checksum`, `output`, `sidecar`,
/// `command`, `cached` and `status` — must be equal. Strictly more than the
/// field-by-field assertions of §9.1 (3): it also proves the MCP tool invents
/// nothing, because an extra key would show up here.
pub fn assert_same_payload(label: &str, mcp: &Value, cli: &Value, mcp_dir: &Path, cli_dir: &Path) {
    let mut mcp_value = normalize_paths(mcp, mcp_dir, cli_dir);
    for key in MCP_ONLY_FIELDS {
        if let Some(value) = mcp_value.as_object_mut().unwrap().remove(key) {
            assert!(
                value.is_string(),
                "{label}: `{key}` may only be a string when present, got {value}"
            );
        }
    }
    assert_eq!(
        mcp_value,
        normalize_paths(cli, mcp_dir, cli_dir),
        "{label}: the MCP payload must render the same state as the CLI document"
    );
}

/// Asserts that the payload carries exactly the §5 field set: the seven CLI
/// fields plus the optional MCP-only `warning`, and nothing invented.
/// `expect_warning` selects the residual-warning branch, which the 64×48 §9.1
/// anchor reaches and the clean 32×24 pair does not.
pub fn assert_payload_field_set(label: &str, payload: &Value, expect_warning: bool) {
    let mut keys: Vec<&str> = payload
        .as_object()
        .unwrap_or_else(|| panic!("{label}: a flat payload object, got {payload}"))
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    let mut expected = CORE_FIELDS.to_vec();
    if expect_warning {
        expected.extend_from_slice(&MCP_ONLY_FIELDS);
    }
    expected.sort_unstable();
    assert_eq!(keys, expected, "{label}: exactly the §5 fields");
    if expect_warning {
        assert!(
            payload["warning"].is_string(),
            "{label}: `warning` must be a string when present: {payload}"
        );
    } else {
        assert!(
            payload.get("warning").is_none(),
            "{label}: no residual warning for a clean alignment: {payload}"
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

// --------------------------------------------------------------- sidecar

/// The DNG path a payload reports under `output` — the artifact every artifact
/// assertion then reads.
pub fn dng_of(payload: &Value) -> PathBuf {
    Path::new(payload["output"].as_str().unwrap()).to_path_buf()
}

pub fn sidecar_of(dng: &Path) -> std::path::PathBuf {
    lumina_sidecar::sidecar_path_for(dng)
}

/// The merge sidecar as a plain JSON document, so the envelope assertions do
/// not have to know the `SidecarDocument` schema. The same keys read through
/// `load_sidecar` in `tests/merge_e2e.rs:232-279`.
pub fn sidecar_json(dng: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(sidecar_of(dng)).unwrap()).unwrap()
}

/// The merge sidecar must carry no absolute path: the bundle stays valid when
/// moved as a whole (`tests/merge_e2e.rs:158-199`, §6 Invariante 7).
pub fn assert_no_absolute_paths(sidecar_text: &str, dir: &Path) {
    assert!(
        !sidecar_text.contains(&dir.to_string_lossy().into_owned()),
        "the merge sidecar must not contain the tree path"
    );
    let value: Value = serde_json::from_str(sidecar_text).unwrap();
    let mut paths = Vec::new();
    collect_path_fields(&value, &mut paths);
    assert!(
        !paths.is_empty(),
        "expected path fields in the merge sidecar"
    );
    for path in paths {
        assert!(
            !path.starts_with('/') && !path.contains('\\') && !path.contains(':'),
            "absolute path in merge sidecar: `{path}`"
        );
    }
}

fn collect_path_fields(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                if matches!(
                    key.as_str(),
                    "path" | "file" | "relative_path" | "relative_name"
                ) {
                    if let Some(text) = val.as_str() {
                        out.push(text.to_string());
                    }
                } else {
                    collect_path_fields(val, out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_path_fields(item, out);
            }
        }
        _ => {}
    }
}

// ----------------------------------------------------- snapshot: T18 support

/// The two byte-identical fixture trees both transports run on.
///
/// The trees are created empty; the fixtures are written by the caller. A
/// snapshot for the parity premise is taken *before* a merge — the premise
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

/// Byte-compares two merge sidecars, masking exactly the recipe's `created_at`
/// wall-clock stamp.
///
/// # Why this ONE comparison is masked
///
/// Measured, not assumed — the same discovery `stage_parity.rs:19-24` made for
/// the geometry/upright `history` stamp. The merge recipe carries
/// `created_at: now_rfc3339_utc()` (`bundle.rs:778`), a wall-clock stamp, so two
/// runs that straddle a second boundary produce different sidecar *bytes*. T14
/// hit exactly that before this mask existed (`23:37:49Z` vs `23:37:50Z`).
///
/// The stamp is metadata by the recipe's own contract: `MergeRecipe::digest`
/// excludes `output`, `created_at`, `status` and `error` because they "describe
/// the artefact, not the merge identity" (`merge_recipe.rs:182-188`). That is
/// why the digest, the DNG checksum and the DNG bytes stay byte-identical across
/// processes while this one field cannot. Everything else in the file — the
/// envelope, the recipe, the artifact reference, both source identities — is
/// compared raw, so a mutation of any other byte still fails.
pub fn assert_identical_sidecar(a: &Path, b: &Path, label: &str) {
    let read = |path: &Path| -> String {
        fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{label}: cannot read `{}`: {error}", path.display()))
    };
    let (a_text, b_text) = (read(a), read(b));
    let stamp_of = |text: &str| -> String {
        let value: Value = serde_json::from_str(text)
            .unwrap_or_else(|error| panic!("{label}: not a JSON document ({error})"));
        value["merge_recipe"]["created_at"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: no `merge_recipe.created_at` stamp to mask"))
            .to_string()
    };
    let (a_stamp, b_stamp) = (stamp_of(&a_text), stamp_of(&b_text));
    for stamp in [&a_stamp, &b_stamp] {
        // The mask is only legitimate on a real stamp; an absent or malformed
        // one is a different defect and must not be hidden by it.
        assert!(
            stamp.ends_with('Z') && stamp.len() >= 20,
            "{label}: `created_at` must be an RFC3339 UTC stamp, got `{stamp}`"
        );
    }
    let a_masked = a_text.replace(&a_stamp, "<CREATED_AT>");
    let b_masked = b_text.replace(&b_stamp, "<CREATED_AT>");
    assert_eq!(
        a_masked, b_masked,
        "{label}: the two sidecars must be byte-identical except for `created_at`"
    );
}
