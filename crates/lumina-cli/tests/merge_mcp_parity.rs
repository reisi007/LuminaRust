//! MCP-PARITY-C: byte identity between `lumina merge-hdr` / `merge-pano` (CLI)
//! and `lumina_merge_hdr` / `lumina_merge_pano` (MCP), on the §9.1 acceptance
//! anchor and its panorama mirror (SOLL §8, phase 3: T11–T16; T21 is the
//! Real-RAW gate and stays `#[ignore]`). The loud rejections live in
//! `merge_mcp_parity_errors.rs` (T17–T19) and the MERGE-DNG-1 envelope gate in
//! `merge_mcp_parity_envelope.rs` (T20); the Real-RAW gate T21 lives in
//! `merge_mcp_parity_gate.rs` and stays `#[ignore]`.
//!
//! # What is actually compared
//!
//! The real `lumina-cli` process runs on one fixture tree, the MCP tool runs
//! in-process on a *second, byte-identical* tree, and then:
//!
//! * the `--json` / `structuredContent` documents are compared field for field
//!   (T11, T14) — so the MCP tool cannot answer with the CLI's values in its
//!   own shape, nor invent a field (T15);
//! * the DNG is compared byte for byte (T12), and against the `checksum` field
//!   both transports report;
//! * the merge sidecar is compared byte for byte except for exactly one masked
//!   field (T13), and then read for the envelope §9.1 (6) demands — T13 on the
//!   HDR mode, T14 on the pano mode, through the one `assert_merge_envelope`
//!   grader;
//! * a cache hit must rewrite nothing (T15) and a force re-merge must land on
//!   the CLI's own digest again (T16) — the cache-hit claim T14 mirrors on the
//!   pano mode, on both frontends.
//!
//! # The two masks, and why they are the only two
//!
//! **The DNG and the payload are not masked at all.** That is a measured claim,
//! not an omission: a merge bundle appends no `history` entry, so no timestamp
//! enters the DNG, and the merge digest, the DNG checksum and the DNG bytes are
//! a pure function of the input bytes.
//!
//! Two masks exist, each named and each with a measured reason:
//!
//! 1. **`warning` in the payload.** The CLI prints the residual line on stderr
//!    and puts it nowhere in `--json`; MCP adds it as its single documented
//!    extra field (§5). `assert_same_payload` drops exactly that field.
//! 2. **`created_at` in the sidecar.** The recipe carries a
//!    `now_rfc3339_utc()` stamp, so two runs that straddle a second boundary
//!    write different bytes — MEASURED here as `23:37:49Z` vs `23:37:50Z` on the
//!    first pano run that did not mask it. `MergeRecipe::digest` excludes the
//!    stamp as metadata, which is why digest/checksum/DNG stay byte-identical
//!    while this one field cannot. `assert_identical_sidecar` masks exactly that
//!    field and compares everything else raw.
//!
//! These are also the tests that make a stub tool red (§9.1 (10)): a handler
//! that answered from a static string could not match the CLI's digest, checksum,
//! `output` path or `cached` flag for a tree it has never seen.

#[path = "merge_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, job::*};
use serde_json::json;
use std::path::Path;

/// T11 — byte-identical payload on the §9.1 acceptance anchor.
///
/// §8 T11: same input set over the CLI binary and the MCP server → **identical
/// `digest` and `checksum`** (string equality) plus identical `command`,
/// `status` and `cached`. §9.1 (1)–(4): the seven §5 fields on `tools/call`, no
/// `saved`/`action`/`text`/`mode`, `checksum == blake3(DNG)`.
#[test]
fn t11_hdr_anchor_payload_is_the_cli_json_field_for_field() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call =
        MergeCall::new(anchor_pair(&cli_dir)).exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0]);
    let mcp_call =
        MergeCall::new(anchor_pair(&mcp_dir)).exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0]);
    assert_eq!(
        snapshot(&cli_dir),
        snapshot(&mcp_dir),
        "the two input trees must start byte-identical"
    );

    let cli_report = cli_call.cli_json("merge-hdr");
    let mcp_payload = mcp_call.mcp_seam(HDR_TOOL);

    // §9.1 (1)+(2): the field set, `command`, `status`, `cached`. The CLI emits
    // exactly the seven fields; the MCP tool adds `warning` and nothing else.
    assert_payload_field_set("T11 CLI --json", &cli_report, false);
    assert_payload_field_set("T11 MCP structuredContent", &mcp_payload, true);
    assert_eq!(mcp_payload["command"], json!("merge-hdr"));
    assert_eq!(cli_report["command"], json!("merge-hdr"));
    assert_eq!(mcp_payload["status"], json!("ok"));
    assert_eq!(mcp_payload["cached"], json!(false));
    // The `warning` extra is the CLI's own residual line (`merge.rs:108-116`).
    let warning = mcp_payload["warning"].as_str().unwrap();
    assert!(
        warning.starts_with("merge-hdr aligned with residual ")
            && warning.ends_with("result kept, nothing cropped silently"),
        "unexpected warning text: {warning}"
    );

    // §9.1 (3)+(4): identical digest AND checksum strings, both `blake3:<64 hex>`,
    // and the checksum is the blake3 of the DNG this very run wrote.
    let mcp_dng = Path::new(mcp_payload["output"].as_str().unwrap()).to_path_buf();
    for field in ["digest", "checksum"] {
        let value = mcp_payload[field].as_str().unwrap();
        assert_eq!(
            cli_report[field],
            json!(value),
            "{field} must match the CLI"
        );
        assert!(
            value.starts_with("blake3:") && value.len() == "blake3:".len() + 64,
            "{field} must be `blake3:<64 hex>`, got `{value}`"
        );
    }
    assert_eq!(mcp_payload["checksum"], json!(blake3_file(&mcp_dng)));

    // §5 1:1, strictly more than the three field assertions above: with the two
    // roots normalised and the one MCP-only field dropped, the two documents must
    // be equal — so the MCP tool also invents no extra key.
    assert_same_payload(
        "T11 anchor hdr",
        &mcp_payload,
        &cli_report,
        &mcp_dir,
        &cli_dir,
    );
}

/// T12 — the DNG on disk is byte-identical, and re-imports at the anchor
/// geometry.
///
/// §8 T12: `blake3(MCP-DNG) == blake3(CLI-DNG) == checksum`-field. §9.1 (5): the
/// written DNG decodes via `lumina_raw` on 64×48 — the fixture geometry of the
/// anchor, not 32×24 (`merge_e2e.rs:281-283` shows the re-import pattern).
#[test]
fn t12_the_anchor_dng_is_byte_identical_and_re_imports_at_64x48() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_report = MergeCall::new(anchor_pair(&cli_dir))
        .exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0])
        .cli_json("merge-hdr");
    let mcp_payload = MergeCall::new(anchor_pair(&mcp_dir))
        .exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0])
        .mcp_ok(HDR_TOOL);

    let cli_dng = Path::new(cli_report["output"].as_str().unwrap()).to_path_buf();
    let mcp_dng = Path::new(mcp_payload["output"].as_str().unwrap()).to_path_buf();
    assert!(
        cli_dng.is_file() && mcp_dng.is_file(),
        "the DNG must lie under `output` on both transports"
    );
    assert_eq!(
        blake3_file(&cli_dng),
        blake3_file(&mcp_dng),
        "the two DNGs must be byte-identical"
    );
    assert_eq!(
        mcp_payload["checksum"],
        json!(blake3_file(&mcp_dng)),
        "`checksum` is the blake3 of the written DNG"
    );
    assert_eq!(cli_report["checksum"], mcp_payload["checksum"]);

    // Re-import: the merged artifact is a real DNG at the merged geometry.
    let decoded = lumina_raw::decode_bytes(&bytes(&mcp_dng), "a-HDR.dng")
        .expect("the written DNG re-imports");
    assert_eq!(
        (decoded.frame.width, decoded.frame.height),
        (ANCHOR_W, ANCHOR_H),
        "the DNG re-imports at the anchor geometry"
    );
    assert!(
        Path::new(mcp_payload["sidecar"].as_str().unwrap()).is_file(),
        "the merge sidecar lies under `sidecar`"
    );
}

/// T13 — the merge sidecar is byte-identical and carries the merge envelope.
///
/// §8 T13: sidecar bytes identical (an `assert_unchanged`-shaped byte compare,
/// `library_parity_common:277-284`), `"type": "merge"`, `merge_recipe.mode`,
/// `merge_version == 1`, `status ok`, `merge_artifact.checksum == checksum`, and
/// no absolute paths (`merge_e2e.rs:158-199,232-279`, §6 Invariante 7). The
/// envelope itself is read by `assert_merge_envelope` — T14 runs the very same
/// claim on the pano mode, so it must not exist twice.
#[test]
fn t13_the_anchor_sidecar_is_byte_identical_and_carries_the_merge_envelope() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_report = MergeCall::new(anchor_pair(&cli_dir))
        .exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0])
        .cli_json("merge-hdr");
    let mcp_payload = MergeCall::new(anchor_pair(&mcp_dir))
        .exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0])
        .mcp_ok(HDR_TOOL);

    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap()).to_path_buf();
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap()).to_path_buf();
    assert_identical_sidecar(&cli_sidecar, &mcp_sidecar, "T13 merge sidecar");

    // §9.1 (6): the envelope the sidecar must carry on the HDR mode.
    assert_merge_envelope(
        "T13 hdr",
        &mcp_sidecar,
        &mcp_dir,
        "hdr",
        &mcp_payload["checksum"],
        (ANCHOR_W, ANCHOR_H),
        &["a.png", "b.png"],
    );
}

/// T14 — the panorama mirror, with the neutral default exposure.
///
/// §8 T14: same input set, same digest/checksum, canvas grows by the estimated
/// offset. §6 Invariante 5 / §9.1 (8): a panorama merge without **any**
/// exposure list succeeds and stores the documented neutral defaults as
/// provenance only — the MCP tool must inherit that unweakened.
///
/// The pano mirror also carries the two claims T13 and T15 make on the HDR
/// anchor, so the synthetic PNG coverage matches what the RAW gate T21 already
/// measures for both pipelines (MCP-PARITY-C Folgepunkt (b)): the full §9.1 (6)
/// merge envelope and the §9.1 (7) cache hit, each on **both** frontends.
#[test]
fn t14_pano_parity_with_neutral_defaults_and_a_canvas_that_grows() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    // No exposure list at all: panorama pixels never use exposure.
    let cli_call = MergeCall::new(pano_pair(&cli_dir)).blend_width_px(4);
    let mcp_call = MergeCall::new(pano_pair(&mcp_dir)).blend_width_px(4);
    let cli_report = cli_call.cli_json("merge-pano");
    let mcp_payload = mcp_call.mcp_seam(PANO_TOOL);

    assert_payload_field_set("T14 CLI --json", &cli_report, false);
    assert_payload_field_set("T14 MCP structuredContent", &mcp_payload, false);
    assert_eq!(mcp_payload["command"], json!("merge-pano"));
    assert_eq!(mcp_payload["status"], json!("ok"));
    assert_eq!(mcp_payload["cached"], json!(false));
    assert_same_payload("T14 pano", &mcp_payload, &cli_report, &mcp_dir, &cli_dir);

    let cli_dng = dng_of(&cli_report);
    let mcp_dng = dng_of(&mcp_payload);
    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap()).to_path_buf();
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap()).to_path_buf();
    assert_identical_files(&cli_dng, &mcp_dng, "T14 pano DNG");
    assert_eq!(mcp_payload["checksum"], json!(blake3_file(&mcp_dng)));
    assert_identical_sidecar(&cli_sidecar, &mcp_sidecar, "T14 pano sidecar");

    // Canvas pin: sidecar recipe and decoded DNG agree, and the canvas grew by
    // the estimated offset (`merge_e2e.rs:303-316`).
    let sidecar = sidecar_json(&mcp_dng);
    assert_eq!(sidecar["merge_recipe"]["mode"], json!("panorama"));
    let matrix = sidecar["merge_recipe"]["alignment"]["transforms"][0]["matrix_3x3"]
        .as_array()
        .unwrap();
    let dx = matrix[2].as_f64().unwrap().round() as i32;
    assert!(dx.abs() > 0, "an offset was estimated, got {matrix:?}");
    let decoded = lumina_raw::decode_bytes(&bytes(&mcp_dng), "a-Pano.dng").unwrap();
    assert_eq!(
        (decoded.frame.width, decoded.frame.height),
        (GRADIENT_W + dx.unsigned_abs(), GRADIENT_H),
        "side-by-side canvas grows by the estimated offset"
    );
    // Neutral defaults are stored, not guessed per pixel (§6 Invariante 5).
    for source in sidecar["merge_recipe"]["sources"].as_array().unwrap() {
        assert!(
            source["exposure"]["exposure_time_s"].as_f64().unwrap() > 0.0,
            "a panorama source keeps a provenance exposure: {source}"
        );
    }

    // §9.1 (6): the pano sidecar carries the full merge envelope — T13's claim
    // on the pano mode, read on BOTH frontends. The canvas geometry is the pin
    // above, so the artifact reference and the decoded DNG must agree on it.
    let size = (GRADIENT_W + dx.unsigned_abs(), GRADIENT_H);
    assert_merge_envelope(
        "T14 MCP pano",
        &mcp_sidecar,
        &mcp_dir,
        "panorama",
        &mcp_payload["checksum"],
        size,
        &["a.png", "b.png"],
    );
    assert_merge_envelope(
        "T14 CLI pano",
        &cli_sidecar,
        &cli_dir,
        "panorama",
        &cli_report["checksum"],
        size,
        &["a.png", "b.png"],
    );

    // §9.1 (7): a second identical call is a cache hit that rewrites no byte —
    // T15's claim on the pano mode, again on BOTH frontends.
    let mcp_before = (bytes(&mcp_dng), bytes(&mcp_sidecar));
    let second = mcp_call.mcp_ok(PANO_TOOL);
    assert_cache_hit(
        "T14 MCP",
        &mcp_payload,
        &second,
        &[
            (mcp_dng.as_path(), &mcp_before.0),
            (mcp_sidecar.as_path(), &mcp_before.1),
        ],
        false,
    );
    let cli_before = (bytes(&cli_dng), bytes(&cli_sidecar));
    let cli_second = cli_call.cli_json("merge-pano");
    assert_cache_hit(
        "T14 CLI",
        &cli_report,
        &cli_second,
        &[
            (cli_dng.as_path(), &cli_before.0),
            (cli_sidecar.as_path(), &cli_before.1),
        ],
        false,
    );
    assert_eq!(
        cli_second["digest"], mcp_payload["digest"],
        "the cached CLI run keeps the digest both frontends agree on"
    );
}

/// T15 — a second identical call is a cache hit that rewrites no byte.
///
/// §8 T15 / §9.1 (7): `cached: true`, `status: "ok"`, DNG and sidecar bytes
/// unchanged, and no `saved`/`action` field invented. The CLI counterpart is the
/// `merge already current:` branch (`merge.rs:133-137`,
/// `merge_e2e.rs:323-348`). The claim itself lives in `assert_cache_hit`; T14
/// runs it on the pano mode.
#[test]
fn t15_a_second_identical_call_is_a_cache_hit_that_rewrites_no_byte() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call =
        MergeCall::new(anchor_pair(&cli_dir)).exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0]);
    let mcp_call =
        MergeCall::new(anchor_pair(&mcp_dir)).exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0]);

    let cli_first = cli_call.cli_json("merge-hdr");
    let first = mcp_call.mcp_ok(HDR_TOOL);
    assert_eq!(first["cached"], json!(false));
    let mcp_dng = dng_of(&first);
    let mcp_sidecar = Path::new(first["sidecar"].as_str().unwrap()).to_path_buf();
    let (dng_before, sidecar_before) = (bytes(&mcp_dng), bytes(&mcp_sidecar));

    // Second identical run on the MCP side: nothing is rewritten.
    let second = mcp_call.mcp_ok(HDR_TOOL);
    assert_cache_hit(
        "T15 MCP",
        &first,
        &second,
        &[
            (mcp_dng.as_path(), &dng_before),
            (mcp_sidecar.as_path(), &sidecar_before),
        ],
        true,
    );

    // The CLI agrees: its second report is a cache hit as well — on the same
    // digest both frontends share, and without rewriting one byte either.
    let cli_dng = Path::new(cli_first["output"].as_str().unwrap()).to_path_buf();
    let cli_sidecar = Path::new(cli_first["sidecar"].as_str().unwrap()).to_path_buf();
    let (cli_dng_before, cli_sidecar_before) = (bytes(&cli_dng), bytes(&cli_sidecar));
    let cli_second = cli_call.cli_json("merge-hdr");
    assert_cache_hit(
        "T15 CLI",
        &cli_first,
        &cli_second,
        &[
            (cli_dng.as_path(), &cli_dng_before),
            (cli_sidecar.as_path(), &cli_sidecar_before),
        ],
        false,
    );
    assert_eq!(
        cli_second["digest"], first["digest"],
        "a cached CLI run keeps the digest both frontends agree on"
    );
}

/// T16 — the force chain: a mutated source is loudly `stale`, and only `force`
/// re-merges back onto the CLI's own digest.
///
/// §8 T16 / §6 Invariante 6. This is also the only test that exercises the
/// `MergeStale` arm of the MCP error mapping on a *real* stale bundle
/// (`bundle.rs:801+`); the Impl-B/C verify measured that arm as unreached.
#[test]
fn t16_a_stale_bundle_fails_loudly_until_force_re_merges_to_the_cli_digest() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call =
        MergeCall::new(anchor_pair(&cli_dir)).exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0]);
    let mcp_call =
        MergeCall::new(anchor_pair(&mcp_dir)).exposure(&[0.004, 0.016], &[100, 100], &[8.0, 8.0]);

    let first = mcp_call.mcp_ok(HDR_TOOL);
    let cli_first = cli_call.cli_json("merge-hdr");
    assert_eq!(first["digest"], cli_first["digest"], "T11's premise");
    let dng_before = bytes(&dng_of(&first));
    let sidecar_before = bytes(Path::new(first["sidecar"].as_str().unwrap()));

    // One source changes — in BOTH trees, so the comparison after the forced
    // re-merge still compares the same input set.
    anchor_png(&cli_dir, "b.png", 175);
    anchor_png(&mcp_dir, "b.png", 175);

    // Without force: loud `merge stale:`, and not one byte rewritten.
    let stale = mcp_call.mcp(HDR_TOOL).unwrap_err();
    assert_eq!(stale.name(), "MergeStale", "{stale:?}");
    assert_eq!(stale.code(), -32011);
    assert_eq!(stale.data()["error"], json!("MergeStale"));
    assert!(
        stale.message().contains("merge stale:"),
        "the CLI prefix must survive: {stale:?}"
    );
    let stale_seam = mcp_call.mcp_seam_failure(HDR_TOOL);
    assert_eq!(
        stale_seam["error"],
        json!("MergeStale"),
        "the same class over the tools/call seam"
    );
    assert_eq!(bytes(&dng_of(&first)), dng_before, "stale rewrites nothing");
    assert_eq!(
        bytes(Path::new(first["sidecar"].as_str().unwrap())),
        sidecar_before,
        "stale rewrites no sidecar byte"
    );
    // The CLI reports the same class for the same input.
    let cli_stale = cli_call.cli_failure("merge-hdr");
    assert_eq!(
        merge_clause_of(&stale.message(), [&mcp_dir, &cli_dir]),
        merge_clause_of(&cli_cause(&cli_stale), [&mcp_dir, &cli_dir]),
        "CLI and MCP must report the same stale text"
    );

    // With force: both transports re-merge, and land on the same digest again.
    let forced = mcp_call.clone().force().mcp_ok(HDR_TOOL);
    let cli_forced = cli_call.clone().force().cli_json("merge-hdr");
    assert_eq!(forced["cached"], json!(false), "force rewrites the bundle");
    assert_eq!(forced["digest"], cli_forced["digest"]);
    assert_eq!(forced["checksum"], cli_forced["checksum"]);
    assert_ne!(
        forced["digest"], first["digest"],
        "a changed source must move the digest"
    );
    assert_eq!(forced["checksum"], json!(blake3_file(&dng_of(&forced))));
    assert_eq!(
        bytes(&dng_of(&forced)),
        bytes(Path::new(cli_forced["output"].as_str().unwrap())),
        "the forced DNG is byte-identical to the CLI's forced DNG"
    );
}
