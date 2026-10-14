//! MCP-PARITY-C: the Real-RAW acceptance gate (SOLL §8, T21 [G]).
//!
//! One `#[ignore]`d test, in its own file, because it is the only test in this
//! slice whose fixture class does not exist yet: a real camera exposure bracket
//! (>= 2 frames) and a panorama series. The transport parity it would assert is
//! exactly what T11–T16 assert on synthetic PNGs; what it would add is the LibRaw
//! decode path, RAW EXIF exposure and the RAW orientation metadata — the three
//! values that enter the *digested* merge recipe (`merge.rs:172-180` /
//! `lumina_stages/src/merge.rs:95-103`) and that no PNG fixture can reach.

#[path = "merge_mcp_parity_common/mod.rs"]
mod common;
use common::{graders::*, job::*};
use serde_json::json;
use std::path::{Path, PathBuf};

/// T21 — the Real-RAW acceptance gate (SOLL §8 [G], §7).
///
/// # Why this is `#[ignore]` and not written with the fixtures that exist
///
/// §7 names the inventory gap and §13 Punkt 3 asks the owner for provenance:
/// the two committed CR3s (`sample-data/raw/aircraft-landscape.cr3`,
/// `-portrait.cr3`) are **not** an exposure bracket and **not** a panorama
/// series — different subjects, 1/1000 s / ISO 1000 and 1250
/// (`golden-fixtures.md:44`, `sample-data/raw/README.md`). A bracket has to be
/// captured or licensed first, and documenting it is an owner decision, not a
/// test-side one.
///
/// PNG mocks cannot stand in for it: they decode through the `image` crate, so
/// `decoder` is `"image"`, exposure comes from the explicit lists and
/// `orientation` is 1 — the three things a RAW bracket would actually prove
/// (`decoder: "libraw"`, RAW EXIF exposure, RAW orientation metadata, which all
/// three enter the *digested* recipe). Writing this test now would assert parity
/// across a path no fixture reaches: a green test with nothing behind it.
///
/// Until a fixture exists, the byte-parity claim rests on T11–T16 (synthetic,
/// `decoder: "image"`) and this gate stays open — SOLL §8 says so explicitly
/// ("Gates, die ausdrücklich nicht behauptet werden"). Set
/// `LUMINA_RAW_MERGE_SERIES` to a directory holding ≥2 RAW frames to run it.
#[test]
#[ignore = "T21 Real-RAW gate: no exposure-bracket fixture in the inventory (SOLL §7, §13 Punkt 3)"]
fn t21_real_raw_bracket_and_panorama_series_are_byte_identical() {
    let series_dir = std::env::var("LUMINA_RAW_MERGE_SERIES").unwrap_or_default();
    let root = Path::new(&series_dir);
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
