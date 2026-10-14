//! MCP-PARITY-C: the Real-RAW gate (SOLL §8, T21 [G]).
//!
//! One `#[ignore]`d test in its own file, because RAW bytes are the one input
//! class the default fixture set cannot afford: a merge run over real camera
//! frames is measured at ~50 s per frontend in the debug profile (two 24 MP
//! LibRaw decodes, the alignment passes, a 145 MB linear-DNG encode), so the
//! full CLI≡MCP RAW proof costs ~5 minutes. The default gate of this slice
//! runs in 66 s (`cargo test -p lumina-mcp -p lumina-cli -p lumina-stages
//! -p lumina-merge`, measured); a 5-minute member would multiply that, so the
//! RAW proof is invoked explicitly instead —
//! `cargo test -p lumina-cli --test merge_mcp_parity_gate -- --ignored`, the
//! same shape as `matrix_e2e.rs:421`.
//!
//! # What the gate proves, in the two worlds it runs in
//!
//! 1. **`LUMINA_RAW_MERGE_SERIES` set** (a directory with ≥ 2 real RAW
//!    frames): the transport parity of a real bracket — what SOLL §7 demands
//!    for release 2.0. That fixture class still does not exist (§13 Punkt 3),
//!    so this branch stays the open part of the gate. The routing control run
//!    needs `-- --ignored` as well — the whole test is `#[ignore]`d, so a run
//!    without it selects no test at all.
//! 2. **Unset — the inventory as committed:** the maximal RAW→DNG proof the
//!    inventory allows. The two committed CR3s are neither an exposure bracket
//!    nor a panorama series (`sample-data/raw/README.md`: different subjects,
//!    1/1000 s / ISO 1000 and 1250), so:
//!    * the **mixed pair** refuses loudly at the first merge-domain boundary
//!      both frontends share — the dimension check *after* LibRaw decode and
//!      exposure resolution — with the identical `merge unsupported:` clause;
//!    * the **same-frame pair** (one committed CR3 in both slots, the alignment
//!      radius pinned to 0 because the frames are identical) runs the *whole*
//!      RAW→DNG pipeline on both transports and proves byte identity: the four
//!      pinned digest/checksum anchors, DNG bytes, sidecar bytes, the LibRaw
//!      decode context (`decoder`/`decode_version`/`orientation`, SOLL §6
//!      Invariante 2), the RAW-EXIF provenance exposure of the panorama mode,
//!      the DNG re-import and the `cached: true` second call.
//!
//! What is deliberately NOT claimed: that a committed fixture is a bracket or
//! a panorama series. The real-RAW acceptance gate stays open until an owner-
//! documented series exists; this file proves the *path*, not the *series*.

#[path = "merge_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, job::*};
use lumina_raw::RawMetadata;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// T21 — the Real-RAW acceptance gate (SOLL §8 [G], §7).
///
/// # Why this is `#[ignore]` and not part of the default run
///
/// A merge run over the committed 24 MP CR3s is measured at ~50 s per
/// frontend (two LibRaw decodes, the alignment passes, a 145 MB DNG encode),
/// so the full proof costs ~5 minutes against a 66 s default gate — invoked
/// on purpose instead, and with no environment fixture needed for the
/// committed path: the RAW files are in the tree
/// (`sample-data/raw/README.md`: author evidenced, unrestricted project
/// licence, 2026-08-20).
///
/// The two branches are described in the module doc: a real series from
/// `LUMINA_RAW_MERGE_SERIES` is proven end-to-end (still open per §7 /
/// §13 Punkt 3), the committed inventory proves what it can.
#[test]
#[ignore = "T21 Real-RAW gate: ~50 s per RAW merge frontend (~5 min for the full proof) against a 66 s default 4-crate gate - invoke on purpose: cargo test -p lumina-cli --test merge_mcp_parity_gate -- --ignored (set LUMINA_RAW_MERGE_SERIES=<dir with >= 2 RAW frames> to prove a real bracket instead; SOLL §7, §13 Punkt 3)"]
fn t21_real_raw_bracket_and_panorama_series_are_byte_identical() {
    let series_dir = std::env::var("LUMINA_RAW_MERGE_SERIES").unwrap_or_default();
    if !series_dir.is_empty() {
        real_series_parity(Path::new(&series_dir));
    } else {
        committed_fixture_pair_parity();
    }
}

// ---------------------------------------------------------------- branch 1

/// The original T21 body: a real RAW series from
/// `LUMINA_RAW_MERGE_SERIES`, byte-identical over both transports.
///
/// A real bracket is proven end-to-end here because its frames carry EXIF
/// exposure, so even without explicit lists the HDR exposure resolves — the
/// one thing a PNG anchor can never show.
fn real_series_parity(root: &Path) {
    assert!(
        root.is_dir(),
        "T21 needs a real exposure bracket (>= 2 RAW frames) and a panorama \
         series in `LUMINA_RAW_MERGE_SERIES`; none exists in the inventory, so \
         the gate stays open (SOLL §7, §13 Punkt 3)"
    );

    // The RAW frames, in merge order: first = reference.
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    inputs.sort();
    assert!(
        inputs.len() >= 2,
        "T21 needs at least 2 RAW frames, found {} in `{}`",
        inputs.len(),
        root.display()
    );

    // A byte-identical copy of the series per transport — the T11 premise on
    // real RAW bytes.
    let tree = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(tree.path());
    let mirror = |dir: &Path| -> Vec<PathBuf> {
        inputs
            .iter()
            .map(|source| {
                let target = dir.join(source.file_name().unwrap());
                std::fs::copy(source, &target).unwrap();
                target
            })
            .collect()
    };
    let cli_call = MergeCall::new(mirror(&cli_dir));
    let mcp_call = MergeCall::new(mirror(&mcp_dir));

    let cli_report = cli_call.cli_json("merge-hdr");
    let mcp_payload = mcp_call.mcp_ok(HDR_TOOL);
    assert_eq!(mcp_payload["digest"], cli_report["digest"], "RAW digest");
    assert_eq!(
        mcp_payload["checksum"], cli_report["checksum"],
        "RAW checksum"
    );

    // The part no PNG fixture can reach: both frontends decoded through LibRaw
    // and recorded it in the digested decode context.
    let sidecar = sidecar_json(&dng_of(&mcp_payload));
    for source in sidecar["merge_recipe"]["sources"].as_array().unwrap() {
        assert_eq!(
            source["decode_context"]["decoder"],
            json!("libraw"),
            "T21 must run the LibRaw decode path, not the image-crate fallback"
        );
    }
}

// ---------------------------------------------------------------- branch 2

/// The committed-inventory branch: mixed pair refuses identically, same-frame
/// pair merges byte-identically — both pipelines, both transports.
fn committed_fixture_pair_parity() {
    mixed_pair_refuses_identically_in_both_frontends();
    same_frame_pair_merges_byte_identically("merge-hdr", HDR_TOOL);
    same_frame_pair_merges_byte_identically("merge-pano", PANO_TOOL);
}

/// The committed RAW directory (provenance/licence:
/// `sample-data/raw/README.md`). Resolved from `CARGO_MANIFEST_DIR`, so the
/// gate does not depend on the test process' working directory.
fn committed_raw_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-data/raw")
}

/// Copies committed fixtures into `dir` under the given names, in merge order.
fn mirror_into(dir: &Path, files: &[(&str, &str)]) -> Vec<PathBuf> {
    fs::create_dir_all(dir).unwrap();
    files
        .iter()
        .map(|(source, target)| {
            let path = dir.join(target);
            fs::copy(committed_raw_dir().join(source), &path).unwrap();
            path
        })
        .collect()
}

/// The two committed CR3s as one input set: different subjects, different
/// visible geometry (6032×4024 vs 4024×6032) — the inventory's only RAW pair.
fn mixed_pair(dir: &Path) -> Vec<PathBuf> {
    mirror_into(
        dir,
        &[
            ("aircraft-landscape.cr3", "landscape.cr3"),
            ("aircraft-portrait.cr3", "portrait.cr3"),
        ],
    )
}

/// One committed CR3 in both slots, byte-identical: the only RAW input set of
/// equal geometry the inventory offers. The portrait fixture is chosen because
/// its EXIF orientation is 8 — the value a non-RAW fallback could never
/// produce (it would be the default 1).
fn same_frame_pair(dir: &Path) -> Vec<PathBuf> {
    mirror_into(
        dir,
        &[
            ("aircraft-portrait.cr3", "frame-a.cr3"),
            ("aircraft-portrait.cr3", "frame-b.cr3"),
        ],
    )
}

/// One mixed-pair merge call: `explicit` adds the HDR exposure lists (HDR
/// otherwise falls back to RAW EXIF, panorama needs none). The radius is
/// pinned to 0 — the dimension check precedes the search, so the value is
/// irrelevant to the outcome and pinning keeps the calls comparable.
fn mixed_call(dir: &Path, explicit: bool) -> MergeCall {
    let call = MergeCall::new(mixed_pair(dir)).max_shift_px(0);
    if explicit {
        call.exposure(&[0.001, 0.00125], &[1000, 1250], &[8.0, 8.0])
    } else {
        call
    }
}

/// One RAW refusal, proven on both transports: the CLI exits non-zero with the
/// expected `merge unsupported:` clause, and the MCP handler answers with the
/// mapped `MergeUnsupported` class (`-32012`) and the *same* clause text, both
/// in-process and over the `tools/call` seam. Returns the CLI output, so the
/// caller can inspect the stderr log that proves how far the run got.
fn assert_raw_refusal(
    pipeline: (&str, &str),
    explicit: bool,
    label: &str,
    dirs: (&Path, &Path),
) -> std::process::Output {
    let (command, tool) = pipeline;
    let (cli_dir, mcp_dir) = dirs;
    let (cli_call, mcp_call) = (mixed_call(cli_dir, explicit), mixed_call(mcp_dir, explicit));
    let output = cli_call.cli(command, true);
    assert_ne!(output.status.code(), Some(0), "{label} must refuse");
    let cause = cli_cause(&output);
    // The expected clause is derived from the command under test
    // (`merge-hdr` → `… hdr alignment …`), so it cannot drift from the text.
    let prefix = format!(
        "merge unsupported: invalid merge input: {} alignment needs equal dimensions",
        command.trim_start_matches("merge-")
    );
    assert!(cause.starts_with(&prefix), "{label}: {cause}");
    let error = mcp_call.mcp(tool).unwrap_err();
    assert_eq!(error.name(), "MergeUnsupported", "{label}: {error:?}");
    assert_eq!(error.code(), -32012, "{label}: {error:?}");
    let seam = mcp_call.mcp_seam_failure(tool);
    assert_eq!(seam["error"], json!("MergeUnsupported"), "{label}");
    assert_eq!(
        merge_clause_of(&mcp_cause(&error), [mcp_dir, cli_dir]),
        merge_clause_of(&cause, [mcp_dir, cli_dir]),
        "{label}: same RAW refusal clause"
    );
    output
}

/// The mixed pair proves the RAW path up to its first merge-domain boundary:
/// LibRaw decode (both frames) and exposure resolution succeed, the dimension
/// check then refuses — and both frontends refuse with the *same* clause and
/// the *same* MCP class (SOLL §3, §6 Inv. 2 up to the boundary).
fn mixed_pair_refuses_identically_in_both_frontends() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let dirs = (cli_dir.as_path(), mcp_dir.as_path());
    // The trees start empty; a refusal must not ADD anything. The baseline is
    // therefore the two copied CR3s themselves — `assert_raw_refusal` copies
    // them again per run, byte-identical, so the copies cannot move the
    // fingerprint either.
    mixed_pair(&cli_dir);
    mixed_pair(&mcp_dir);
    let before = (snapshot(&cli_dir), snapshot(&mcp_dir));

    // HDR with explicit exposure lists: the resolution itself must succeed, so
    // the refusal that follows is the alignment boundary and nothing earlier.
    let output = assert_raw_refusal(("merge-hdr", HDR_TOOL), true, "hdr", dirs);
    // Both frames reached the LibRaw decoder before the boundary.
    let cli_log = stderr_of(&output);
    assert!(
        cli_log.contains("(6032x4024, libraw") && cli_log.contains("(4024x6032, libraw"),
        "both CR3s must decode through LibRaw first: {cli_log}"
    );

    // HDR without any explicit list: the RAW EXIF exposure resolves, so the
    // refusal stays the alignment boundary (SOLL §6 Invariante 4, EXIF
    // fallback — a PNG cannot reach this branch at all).
    let exif_output = assert_raw_refusal(("merge-hdr", HDR_TOOL), false, "exif", dirs);
    assert!(
        !stderr_of(&exif_output).contains("no EXIF exposure"),
        "the committed CR3s carry EXIF exposure: {}",
        stderr_of(&exif_output)
    );

    // The panorama mirror of the same boundary: different code path
    // (`estimate_pano_transform`), identical refuse-loudly behaviour — and
    // panorama needs no exposure at all (SOLL §6 Invariante 5).
    assert_raw_refusal(("merge-pano", PANO_TOOL), false, "pano", dirs);

    // A refusal leaves nothing behind (T18): not one byte in either tree.
    assert_eq!(snapshot(&cli_dir), before.0, "the CLI tree is unchanged");
    assert_eq!(snapshot(&mcp_dir), before.1, "the MCP tree is unchanged");
}

/// The RAW metadata of a committed fixture — read metadata-only (no demosaic),
/// so the expected orientation/exposure values come from the file itself and
/// not from a literal this test could drift from.
fn raw_metadata_of(path: &Path) -> RawMetadata {
    lumina_raw::read_metadata(path).unwrap_or_else(|error| {
        panic!(
            "committed RAW fixture `{}` must decode metadata: {error}",
            path.display()
        )
    })
}

/// The same-frame pair runs the whole RAW→DNG pipeline on both transports and
/// proves byte identity end to end: payload (SOLL §5), DNG bytes, sidecar
/// bytes, the LibRaw decode context (SOLL §6 Invariante 2), the DNG re-import
/// and the `cached: true` second call, in both pipelines.
///
/// The alignment radius is pinned to 0: both slots carry the *same* bytes, so
/// the shift search has nothing to find. The claim is the RAW decode path and
/// the transport parity, not an estimated transform — a real bracket remains
/// branch 1 of the gate.
fn same_frame_pair_merges_byte_identically(command: &str, tool: &str) {
    let explicit_hdr = command == "merge-hdr";
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let (cli_inputs, mcp_inputs) = (same_frame_pair(&cli_dir), same_frame_pair(&mcp_dir));
    // The expected RAW values are read from the fixture itself (metadata-only,
    // no demosaic), so the assertions below pin the file, not a literal.
    let expected = raw_metadata_of(&mcp_inputs[0]);
    let hdr_exposure = |call: MergeCall| {
        call.exposure(&[0.001, 0.00125], &[1000, 1250], &[8.0, 8.0])
            .max_shift_px(0)
    };
    // Explicit lists win over EXIF on HDR (SOLL §6 Inv. 4); the radius is
    // pinned to 0 in both modes because identical frames have no shift to find.
    let (cli_call, mcp_call) = if explicit_hdr {
        (
            hdr_exposure(MergeCall::new(cli_inputs)),
            hdr_exposure(MergeCall::new(mcp_inputs)),
        )
    } else {
        (
            MergeCall::new(cli_inputs).max_shift_px(0),
            MergeCall::new(mcp_inputs).max_shift_px(0),
        )
    };
    let (cli_tree, mcp_tree) = (snapshot(&cli_dir), snapshot(&mcp_dir));
    assert_eq!(cli_tree, mcp_tree, "identical start trees");

    let cli_report = cli_call.cli_json(command);
    let mcp_payload = mcp_call.mcp_seam(tool);

    // §5 1:1 with the two roots normalised — no invented field, and no
    // residual warning (identical frames align at 0.00px).
    assert_payload_field_set(&format!("{command} RAW CLI --json"), &cli_report, false);
    assert_payload_field_set(&format!("{command} RAW MCP"), &mcp_payload, false);
    assert_same_payload(
        "RAW {command}",
        &mcp_payload,
        &cli_report,
        &mcp_dir,
        &cli_dir,
    );
    assert_eq!(mcp_payload["command"], json!(command));
    assert_eq!(mcp_payload["status"], json!("ok"));
    assert_eq!(mcp_payload["cached"], json!(false));

    // The DNG itself: identical bytes on both transports, and `checksum` is
    // the blake3 of exactly those bytes.
    let (cli_dng, mcp_dng) = (dng_of(&cli_report), dng_of(&mcp_payload));
    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap());
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap());
    assert_identical_files(&cli_dng, &mcp_dng, "{command} RAW DNG");
    // The four pinned anchors of this exact input set, measured in this tree on
    // BOTH transports. They are a pure function of the fixture bytes
    // (`aircraft-portrait.cr3` twice, named frame-a/frame-b.cr3 — the names
    // enter the digested recipe), `--max-shift-px 0`, blend width 64 (default),
    // the explicit exposure lists (HDR) resp. the RAW EXIF provenance (pano),
    // `merge_version` 1 and the pinned LibRaw 0.22.2
    // (`.github/workflows/ci-libraw-image.yml`): a LibRaw upgrade becomes a
    // deliberate, visible digest change, not a silent drift.
    let (pinned_digest, pinned_checksum) = if explicit_hdr {
        (
            "blake3:2c421f7ddacd5f609614bcd00d41fb2166fe6cb0f15a83695c97d3b0b0f42b97",
            "blake3:a465795c8bdf7ae572a2dbc3064e2be74c2adbf4e4ccbc72df82000dafecfe78",
        )
    } else {
        (
            "blake3:8d0e44abc0e3d265bf5d345bb43590215677ac00455d14057cf66d75ccd8c6bd",
            "blake3:245b915218fad32d9135d9cf4c144e474628cb806c44cdff70817341fc93d602",
        )
    };
    for (field, pinned) in [("digest", pinned_digest), ("checksum", pinned_checksum)] {
        for payload in [&cli_report, &mcp_payload] {
            assert_eq!(payload[field], json!(pinned), "{command} {field} pin");
        }
    }
    assert_eq!(mcp_payload["checksum"], json!(blake3_file(&mcp_dng)));

    // The sidecar: byte-identical except the one masked wall-clock stamp.
    assert_identical_sidecar(cli_sidecar, mcp_sidecar, "{command} RAW");

    let sidecar = sidecar_json(&mcp_dng);
    assert_eq!(sidecar["type"], json!("merge"), "the merge envelope");
    let recipe = &sidecar["merge_recipe"];
    let expected_mode = if explicit_hdr { "hdr" } else { "panorama" };
    assert_eq!(recipe["mode"], json!(expected_mode));
    assert_eq!(recipe["merge_version"], json!(1));
    assert_eq!(recipe["status"], json!("ok"));
    assert_eq!(
        sidecar["merge_artifact"]["checksum"], mcp_payload["checksum"],
        "merge_artifact.checksum must be the reported checksum"
    );

    // SOLL §6 Invariante 2, the part no PNG can reach: the digested decode
    // context of BOTH sources is the LibRaw one, with the version the CLI
    // passes and the orientation the RAW file itself carries.
    let (decode_version, orientation) = (lumina_raw::libraw_decode_version(), expected.orientation);
    assert_eq!(
        expected.orientation, 8,
        "aircraft-portrait.cr3's EXIF orientation; a replaced fixture needs \
         these pins re-measured"
    );
    for source in recipe["sources"].as_array().unwrap() {
        let context = &source["decode_context"];
        assert_eq!(context["decoder"], json!("libraw"), "decoder");
        assert_eq!(context["decode_version"], json!(decode_version), "version");
        assert_eq!(context["orientation"], json!(orientation), "orientation");
    }

    // Exposure provenance: the explicit lists on HDR, the RAW EXIF values on
    // panorama (SOLL §6 Invarianten 4/5).
    let close = |stored: f64, expected: f64| (stored - expected).abs() < 1e-6;
    for (index, source) in recipe["sources"].as_array().unwrap().iter().enumerate() {
        let stored = &source["exposure"];
        if explicit_hdr {
            let expected_times = [0.001, 0.00125];
            let expected_isos = [1000, 1250];
            assert_eq!(stored["exposure_time_s"], json!(expected_times[index]));
            assert_eq!(stored["iso"], json!(expected_isos[index]));
            assert_eq!(stored["f_number"], json!(8.0));
        } else {
            let exif = [
                ("exposure_time_s", expected.shutter),
                ("iso", expected.iso),
                ("f_number", expected.aperture),
            ];
            for (key, exif_value) in exif {
                let expected = exif_value.expect("RAW EXIF value") as f64;
                let sampled = stored[key].as_f64().unwrap();
                assert!(close(sampled, expected), "RAW EXIF {key}");
            }
        }
    }

    // Re-import: the merged artifact is a real DNG at the RAW's visible
    // geometry (`merge_e2e.rs:281-283` pattern, geometry from the fixture).
    let decoded =
        lumina_raw::decode_bytes(&bytes(&mcp_dng), "merged.dng").expect("the RAW DNG re-imports");
    assert_eq!(
        (decoded.frame.width, decoded.frame.height),
        (expected.width, expected.height),
        "the DNG re-imports at the RAW's visible geometry"
    );
    assert_no_absolute_paths(&fs::read_to_string(mcp_sidecar).unwrap(), &mcp_dir);

    // A second identical RAW call is a cache hit that rewrites no byte
    // (§9.1 (7), on RAW inputs, in BOTH pipelines — the PNG anchor proves the
    // same in T15).
    let (dng_before, sidecar_before) = (bytes(&mcp_dng), bytes(mcp_sidecar));
    let cached = mcp_call.mcp_seam(tool);
    assert_eq!(cached["cached"], json!(true), "RAW cache hit");
    assert_eq!(cached["status"], json!("ok"));
    assert_eq!(cached["digest"], mcp_payload["digest"]);
    assert_eq!(cached["checksum"], mcp_payload["checksum"]);
    assert_eq!(bytes(&mcp_dng), dng_before, "no DNG rewrite");
    assert_eq!(bytes(mcp_sidecar), sidecar_before, "no sidecar rewrite");
}
