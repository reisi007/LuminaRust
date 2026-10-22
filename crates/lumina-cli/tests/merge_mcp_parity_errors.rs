//! MCP-PARITY-C: the loud rejections of the two merge tools, on both transports
//! (SOLL §8 T17–T20; the byte-parity half lives in `merge_mcp_parity.rs`).
//!
//! # What is compared
//!
//! Not the *existence* of a rejection but its **class and its text**. For one
//! and the same input set the CLI binary must exit non-zero with
//! `error: merge <class>: <cause>` on stderr and the MCP tool must answer
//! `isError: true` carrying the same `merge <class>: <cause>` clause plus the
//! stable name and code of the §3 table. SOLL §2 Pkt. 1/3/4 is what makes this
//! load-bearing: a handler that pre-checked a merge-domain rule would answer
//! `InvalidParams` where the CLI says `merge failed:` — the exact divergence
//! T17 exists to catch.
//!
//! # What each test proves
//!
//! * T17 — every class in the §3 table is reached on the MCP side **and** the
//!   text matches the CLI's, including the `merge stale:`/`merge unsupported:`
//!   arms of `map_merge_run_error` (`error.rs:139-146`).
//! * T18 — a refusal writes nothing on either transport. A "loud failure" that
//!   left a half-written DNG behind would still look loud.
//! * T19 — the merge touches neither the source bytes nor their sidecars, and a
//!   source outside the bundle is refused identically (§6 Invariante 7).
//! * the MERGE-DNG-1 envelope gate has its own file:
//!   `merge_mcp_parity_envelope.rs` (T20).
//!
//! Everything runs on generated PNGs, so every case is deterministic; the one
//! thing these tests cannot exercise is the RAW decode path (T21 stays
//! `#[ignore]`, `merge_mcp_parity.rs`).

// CLI-NO-DEFAULT-TESTBUILD-80: the whole target is CLI<->MCP parity work; the
// shared harness needs `lumina_mcp`, which only exists under the default `mcp`
// feature, so the file is gated on that feature as a whole.
#![cfg(feature = "mcp")]

#[path = "merge_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, job::*};
use serde_json::json;
use std::path::Path;

/// The HDR command under test, the MCP twin, and the exposure list every
/// mergeable case uses.
const COMMAND: &str = "merge-hdr";
const EXPOSURE: (&[f64], &[u32], &[f64]) = (&[0.01, 0.02], &[100, 100], &[8.0, 8.0]);

/// One refusal expressed once, built fresh inside whichever tree needs it.
struct ClassCase {
    label: &'static str,
    /// The canonical CLI prefix (`merge.rs:14-19` + `bundle.rs:229-241`); empty
    /// for the tool-level class, which has no merge prefix.
    prefix: &'static str,
    /// The stable MCP error name and JSON-RPC code of the §3 table.
    name: &'static str,
    code: i64,
    build: Box<dyn Fn(&Path) -> MergeCall>,
}

/// The mergeable 32×24 pair with the exposure list every mergeable case uses.
fn mergeable_hdr(dir: &Path) -> MergeCall {
    MergeCall::new(hdr_pair(dir)).exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2)
}

/// The whole §3 table, one entry per row that an input set can actually reach.
///
/// Every case is mergeable up to the point its own rule fires, so a failure is
/// attributable to that rule and not to a fixture. The ordering clue for each
/// case is the filename comment: `run_merge` validates the count first
/// (`bundle.rs:369`), then decodes, then resolves exposure, then aligns, then
/// resolves the DNG target, and only then validates the recipe
/// (`merge.rs` sequence documented in `bundle.rs:347-351`).
fn class_cases() -> Vec<ClassCase> {
    vec![
        // `merge missing:` — a source that is not there (`decode_source`).
        ClassCase {
            label: "a source that is not there",
            prefix: "merge missing:",
            name: "FileNotFound",
            code: -32001,
            build: Box::new(|dir| {
                let mut inputs = hdr_pair(dir);
                inputs[1] = dir.join("gone.png");
                MergeCall::new(inputs).exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2)
            }),
        },
        // `merge failed:` — one source (`validate_source_count`, before any I/O).
        ClassCase {
            label: "a single source",
            prefix: "merge failed:",
            name: "MergeFailed",
            code: -32013,
            build: Box::new(|dir| {
                MergeCall::new(vec![gradient_png(dir, "only.png", 1.0, 0)]).exposure(
                    &[0.01],
                    &[100],
                    &[8.0],
                )
            }),
        },
        // `merge failed:` — 257 sources. The two real frames keep the fixture
        // tree non-empty; `validate_source_count` runs before any decode, so the
        // other 255 paths are never read.
        ClassCase {
            label: "257 sources",
            prefix: "merge failed:",
            name: "MergeFailed",
            code: -32013,
            build: Box::new(|dir| {
                let mut inputs = hdr_pair(dir);
                inputs.extend((0..255).map(|i| dir.join(format!("extra-{i:03}.png"))));
                // No explicit lists: with 257 inputs the arity rule would fire
                // first, and that is a different case (the last row below).
                MergeCall::new(inputs)
            }),
        },
        // `merge failed:` — the output form is merge domain (`resolve_dng_path`),
        // so it must NOT become a tool-level `InvalidParams` (§2 Pkt. 3).
        ClassCase {
            label: "an output that is not a .dng",
            prefix: "merge failed:",
            name: "MergeFailed",
            code: -32013,
            build: Box::new(|dir| {
                let mut call = mergeable_hdr(dir);
                call = call.output(dir.join("out.png"));
                call
            }),
        },
        // `merge failed:` — `recipe.validate()` range check (§6 Invariante 10).
        ClassCase {
            label: "blend_width_px 65537",
            prefix: "merge failed:",
            name: "MergeFailed",
            code: -32013,
            build: Box::new(|dir| mergeable_hdr(dir).blend_width_px(65537)),
        },
        // `merge unsupported:` — HDR without exposure, never guessed from pixels.
        ClassCase {
            label: "HDR without exposure",
            prefix: "merge unsupported:",
            name: "MergeUnsupported",
            code: -32012,
            build: Box::new(|dir| MergeCall::new(hdr_pair(dir))),
        },
        // `merge unsupported:` — an out-of-range explicit exposure value.
        ClassCase {
            label: "an out-of-range exposure value",
            prefix: "merge unsupported:",
            name: "MergeUnsupported",
            code: -32012,
            build: Box::new(|dir| {
                MergeCall::new(hdr_pair(dir)).exposure(&[0.0, 0.02], &[100, 100], &[8.0, 8.0])
            }),
        },
        // `merge unsupported:` — the frames do not align geometrically.
        ClassCase {
            label: "a dimension mismatch",
            prefix: "merge unsupported:",
            name: "MergeUnsupported",
            code: -32012,
            build: Box::new(|dir| {
                MergeCall::new(vec![
                    solid_png(dir, "a.png", 32, 24, 120),
                    solid_png(dir, "b.png", 40, 32, 120),
                ])
                .exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2)
            }),
        },
        // `merge unsupported:` — below the writer's frame limit.
        ClassCase {
            label: "undersized frames",
            prefix: "merge unsupported:",
            name: "MergeUnsupported",
            code: -32012,
            build: Box::new(|dir| {
                MergeCall::new(vec![
                    solid_png(dir, "a.png", 8, 8, 120),
                    solid_png(dir, "b.png", 8, 8, 200),
                ])
                .exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2)
            }),
        },
        // The one tool-level class: the arity rule the CLI also checks itself
        // *before* `run_merge`, so it has no merge prefix at all.
        ClassCase {
            label: "a list that does not match the input count",
            prefix: "",
            name: "InvalidParams",
            code: -32602,
            build: Box::new(|dir| MergeCall::new(hdr_pair(dir)).exposure(&[0.01], &[100], &[8.0])),
        },
    ]
}

/// T17 — for one and the same input set, the CLI's class and text and the MCP
/// tool's class and text are the same (§3 table, §9.1 (9)).
#[test]
fn t17_every_merge_error_class_and_text_matches_the_cli() {
    for case in class_cases() {
        let root = tempfile::tempdir().unwrap();
        let (cli_dir, mcp_dir) = both_trees(root.path());
        let cli_call = (case.build)(&cli_dir);
        let mcp_call = (case.build)(&mcp_dir);

        // CLI side: non-zero exit and an `error: <text>` line.
        let cli_output = cli_call.cli_failure(COMMAND);
        let cli_text = cli_cause(&cli_output);
        assert!(
            case.prefix.is_empty() || cli_text.starts_with(case.prefix),
            "{}: the CLI must say `{}`, got `{cli_text}`",
            case.label,
            case.prefix
        );

        // MCP side: the mapped class, with the stable name, code and `data.error`.
        let error = mcp_call.mcp(HDR_TOOL).unwrap_err();
        assert_eq!(error.name(), case.name, "{}: class", case.label);
        assert_eq!(
            error.code(),
            case.code,
            "{}: {}",
            case.label,
            error.message()
        );
        assert_eq!(
            error.data()["error"],
            json!(case.name),
            "{}: `data.error`",
            case.label
        );

        // Same TEXT, not merely the same class. For a merge class that is the
        // canonical clause; for the tool-level class it is the shared message.
        let roots = [mcp_dir.as_path(), cli_dir.as_path()];
        let mcp_text = if case.prefix.is_empty() {
            mcp_cause(&error)
        } else {
            merge_clause_of(&error.message(), roots)
        };
        let cli_clause = if case.prefix.is_empty() {
            cli_text.clone()
        } else {
            merge_clause_of(&cli_text, roots)
        };
        assert_eq!(
            mcp_text, cli_clause,
            "{}: both frontends must report the same text",
            case.label
        );

        // §9.1 (9): the refusal is equally loud over the `tools/call` seam.
        let structured = mcp_call.mcp_seam_failure(HDR_TOOL);
        assert_eq!(
            structured["error"],
            json!(case.name),
            "{}: the seam must carry `data.error`",
            case.label
        );
    }
}

/// T18 — every refusal, on both transports, changes nothing (§8 T18).
#[test]
fn t18_every_rejection_leaves_no_artifact_behind() {
    for case in class_cases() {
        let root = tempfile::tempdir().unwrap();
        let (cli_dir, mcp_dir) = both_trees(root.path());
        let cli_call = (case.build)(&cli_dir);
        let mcp_call = (case.build)(&mcp_dir);

        let cli_before = snapshot(&cli_dir);
        let mcp_before = snapshot(&mcp_dir);
        // Teeth: an empty tree would make "nothing changed" vacuous.
        assert!(
            !cli_before.is_empty(),
            "{}: the CLI tree needs files for the snapshot to mean anything",
            case.label
        );
        assert_eq!(
            cli_before, mcp_before,
            "{}: the two trees must start identical",
            case.label
        );

        let cli_output = cli_call.cli_failure(COMMAND);
        assert!(
            mcp_call.mcp(HDR_TOOL).is_err(),
            "{}: the MCP tool must refuse",
            case.label
        );

        let assert_refusal_snapshot =
            |dir: &Path, before: &[(String, usize, u64)], transport: &str| {
                assert!(
                    before
                        .iter()
                        .all(|(name, _, _)| name != ".a-HDR.dng.lumina.json.lock"),
                    "{}: lock rendezvous token must be new for this fixture",
                    case.label
                );
                let after = snapshot(dir);
                let mut without_lock: Vec<_> = after
                    .iter()
                    .filter(|(name, _, _)| name != ".a-HDR.dng.lumina.json.lock")
                    .cloned()
                    .collect();
                let mut expected = before.to_vec();
                without_lock.sort();
                expected.sort();
                assert_eq!(
                    expected, without_lock,
                    "{}: {transport} refusal must not create a DNG, sidecar, or staging artifact",
                    case.label
                );
                let lock_entries: Vec<_> = after
                    .into_iter()
                    .filter(|(name, _, _)| name == ".a-HDR.dng.lumina.json.lock")
                    .collect();
                assert!(
                    lock_entries.len() <= 1,
                    "{}: at most the exact rendezvous lock is allowed",
                    case.label
                );
            };
        assert_refusal_snapshot(&cli_dir, &cli_before, "CLI");
        assert_refusal_snapshot(&mcp_dir, &mcp_before, "MCP");
        assert!(
            !cli_cause(&cli_output).is_empty(),
            "{}: a refusal must say why",
            case.label
        );
    }
}

/// T19 — the merge touches neither the source bytes nor their sidecars, and a
/// source outside the bundle is refused identically (§8 T19, §6 Invariante 7).
#[test]
fn t19_the_merge_touches_neither_the_sources_nor_their_sidecars() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call = MergeCall::new(hdr_pair(&cli_dir)).exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2);
    let mcp_call = MergeCall::new(hdr_pair(&mcp_dir)).exposure(EXPOSURE.0, EXPOSURE.1, EXPOSURE.2);

    // Both sources carry their own single-image sidecar first — the state of a
    // working library, and the state in which an accidental overwrite would show.
    for dir in [&cli_dir, &mcp_dir] {
        for name in ["a.png", "b.png"] {
            let imported = cli()
                .arg("import")
                .arg("--input")
                .arg(dir.join(name))
                .output()
                .unwrap();
            assert!(
                imported.status.success(),
                "import failed: {}",
                String::from_utf8_lossy(&imported.stderr)
            );
        }
    }
    let source_state = |dir: &Path| -> Vec<(Vec<u8>, Vec<u8>)> {
        ["a.png", "b.png"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                (bytes(&path), bytes(&sidecar_of(&path)))
            })
            .collect()
    };
    let (cli_before, mcp_before) = (source_state(&cli_dir), source_state(&mcp_dir));

    let cli_report = cli_call.cli_json(COMMAND);
    let mcp_payload = mcp_call.mcp_ok(HDR_TOOL);
    assert_eq!(
        mcp_payload["digest"], cli_report["digest"],
        "T19 still needs T11's premise: the same input set, one digest"
    );

    for (index, name) in ["a.png", "b.png"].iter().enumerate() {
        let cli_path = cli_dir.join(name);
        let mcp_path = mcp_dir.join(name);
        assert_eq!(
            bytes(&cli_path),
            cli_before[index].0,
            "{name}: a merge must not rewrite a source byte (cli)"
        );
        assert_eq!(
            bytes(&sidecar_of(&cli_path)),
            cli_before[index].1,
            "{name}: a merge must not rewrite a source sidecar (cli)"
        );
        assert_eq!(
            bytes(&mcp_path),
            mcp_before[index].0,
            "{name}: a merge must not rewrite a source byte (mcp)"
        );
        assert_eq!(
            bytes(&sidecar_of(&mcp_path)),
            mcp_before[index].1,
            "{name}: a merge must not rewrite a source sidecar (mcp)"
        );
        assert_eq!(
            cli_before[index], mcp_before[index],
            "{name}: both transports leave the same source state"
        );
    }

    // A source outside the merge bundle. ONE tree for both transports here: the
    // refusal names the absolute bundle directory, so the two texts are only
    // comparable when they address the same one (`merge_e2e.rs:580-605`).
    let dir = tempfile::tempdir().unwrap();
    let outside = MergeCall::new(vec![
        gradient_png(dir.path(), "a.png", 1.0, 0),
        gradient_png(&dir.path().join("sub"), "b.png", 1.0, 4),
    ])
    .output(dir.path().join("sub").join("a-Pano.dng"))
    .blend_width_px(4);

    let cli_output = outside.cli_failure("merge-pano");
    let cli_text = cli_cause(&cli_output);
    assert!(
        cli_text.starts_with("merge unsupported:") && cli_text.contains("outside"),
        "a source outside the bundle must be loud `merge unsupported:`, got `{cli_text}`"
    );
    let error = outside.mcp(PANO_TOOL).unwrap_err();
    assert_eq!(error.name(), "MergeUnsupported", "{error:?}");
    assert_eq!(error.code(), -32012);
    assert_eq!(error.data()["error"], json!("MergeUnsupported"));
    assert_eq!(
        merge_clause(&error.message()),
        &cli_text,
        "the outside-bundle rejection must be the same text on both frontends"
    );
    assert!(
        !dir.path().join("sub").join("a-Pano.dng").exists(),
        "a refused merge must not write the DNG"
    );
}
