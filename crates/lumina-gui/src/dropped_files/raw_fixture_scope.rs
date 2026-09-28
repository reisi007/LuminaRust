//! The committed RAW fixture set and the operator operand for the real-RAW
//! proof.
//!
//! What the proof decodes is fixed **in the code below**: two committed,
//! licensed CR3 fixtures resolved from `CARGO_MANIFEST_DIR`, each with the
//! geometry `sample-data/raw/README.md` documents. There is no environment gate
//! and no early return above the set, so `--ignored` — the documented way to
//! run the local proofs — decodes real data instead of announcing a skip
//! (`feature/quality/fixtures-licensing.md` §3.2.1 **Regel 1**).
//!
//! The geometry is carried as **literals** on the type, not read back from the
//! pipeline: the decode result is checked against these values, so the proof
//! pins the documented geometry itself rather than only showing that two
//! production paths agree (§3.2.1 **Regel 2**).
//!
//! Presence is decided by construction, not by a branch that could pick a
//! provenance: a [`CommittedFixture`] comes only from [`raw_fixtures`] and is a
//! hard failure when absent; an [`OperatorFixture`] comes only from
//! `LUMINA_RAW_FIXTURE` and is a reported skip when absent (§3.2.1 **Regel
//! 3**).
//!
//! **Named limit (Build-Agent 2026-09-28, after six verification rounds).** The
//! coverage of the committed fixtures is a property **readable in this code**,
//! not an enforced invariant. The proof iterates the set below and checks the
//! documented geometry of every fixture it names; removing a row removes a
//! check, and the remaining rows still prove exactly what they are for — that a
//! discarded CR3 keeps orientation, metadata and identity. The retired rules 4
//! and 6 were assertions **about the test itself**; they are documented as
//! withdrawn in `feature/quality/fixtures-licensing.md` §3.2.1.

use std::path::{Path, PathBuf};

use super::raw_fixture_override::OperatorFixture;
use super::test_support::{report_fixture_skip, report_weakened_proof};

/// The geometry `sample-data/raw/README.md` documents for one committed
/// fixture, kept as literals rather than read back from the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DocumentedGeometry {
    pub(super) orientation: u8,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// One committed RAW fixture [`raw_fixtures`] hands out.
///
/// The geometry is **mandatory**: a committed fixture always has an inventory
/// row to be held to. There is deliberately no optional variant here — an
/// optional field is the shape on which a producer could signal a substitute
/// set, and under Regel 5 there is nothing to substitute
/// (`fixtures-licensing.md` §3.2.1 Regel 5).
pub(super) struct CommittedFixture {
    pub(super) path: PathBuf,
    pub(super) documented: DocumentedGeometry,
}

/// The RAW fixtures the real-RAW proof decodes, in a deterministic order.
///
/// Resolution is rooted in `CARGO_MANIFEST_DIR` at **compile time** and points
/// at the committed fixtures, whose provenance and unrestricted
/// test/benchmark/reference licence the inventory documents. A plain
/// `cargo test -- --ignored` therefore decodes real data in any checkout, and
/// the proof cannot be switched off by forgetting a variable.
///
/// **Committed fixtures only.** `LUMINA_RAW_FIXTURE` is not read here: the
/// operator operand is an **addition** decided in
/// [`super::raw_fixture_override::operator_addition`], so this producer has no
/// optional field and cannot signal a substitute set (§3.2.1 Regel 5).
///
/// The geometry literals below are the values `sample-data/raw/README.md`
/// documents; the decode is checked against them, not only against
/// `lumina_raw::read_metadata` (§3.2.1 Regel 2). A row removed here removes a
/// check — the named limit at the top of this module, not a silently waived
/// invariant.
pub(super) fn raw_fixtures() -> Vec<CommittedFixture> {
    [
        (
            "aircraft-landscape.cr3",
            DocumentedGeometry {
                orientation: 1,
                width: 6032,
                height: 4024,
            },
        ),
        (
            "aircraft-portrait.cr3",
            DocumentedGeometry {
                orientation: 8,
                width: 4024,
                height: 6032,
            },
        ),
    ]
    .into_iter()
    .map(|(name, documented)| CommittedFixture {
        path: committed_directory().join(name),
        documented,
    })
    .collect()
}

/// Fail loudly when a **committed** fixture is absent from the tree.
///
/// The argument's type carries the provenance: a [`CommittedFixture`] is
/// produced only by [`raw_fixtures`], so this function has **no branch** that
/// could decide the file's origin — an absent committed file is always a
/// failure, and a message can therefore never claim an origin that is not its
/// own (`DoD.md` §10). A missing, licensed artifact is a failure rather than a
/// fallback (`Agents.md`), and a committed fixture that is missing is a broken
/// tree, not an operator request (§3.2.1 **Regel 3**).
pub(super) fn accept_committed_fixture(fixture: &CommittedFixture) {
    let path = &fixture.path;
    assert!(
        path.is_file(),
        "the committed RAW fixture {} is missing from the tree. A committed, licensed artifact \
         that this proof must decode is absent, which is a failure and not a skip: restore the \
         file from git.",
        path.display()
    );
}

/// Decide whether an **operator** addition may be decoded, reporting either way.
///
/// The argument is an [`OperatorFixture`], and that type is constructed **only**
/// from `LUMINA_RAW_FIXTURE` in
/// [`super::raw_fixture_override::operator_operand`]. Its origin is thus carried
/// by the construction of the call, not reconstructed from a pattern, and a
/// committed fixture cannot be passed here at all (`DoD.md` §10). An absent
/// operator operand is a reported skip; a present one is decoded as an
/// announced **weaker** proof because it has no documented-geometry anchor
/// (§3.2.1 **Regel 3** and **Regel 5**).
pub(super) fn accept_operator_fixture(operand: &OperatorFixture, test: &str) -> bool {
    let path = operand.path();
    if !path.is_file() {
        report_fixture_skip(test, &format!("fixture not present: {}", path.display()));
        return false;
    }
    report_weakened_proof(
        test,
        &format!(
            "operator-supplied fixture {} has no documented-geometry anchor, so this run checks \
             the decode against lumina_raw::read_metadata only",
            path.display()
        ),
    );
    true
}

/// The workspace root, where `sample-data/` lives.
///
/// Shared with the additive operand in `raw_fixture_override`, so a relative
/// override and the committed set are anchored at the same root (the workspace
/// root, not the test process' CWD `crates/lumina-gui`, where a path that
/// demonstrably exists would not resolve).
pub(super) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        // Two levels up is the workspace root, so a message names a plain path
        // instead of leaving an unresolved `../..` in it.
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR is <workspace>/crates/<crate>")
        .to_path_buf()
}

/// The directory holding the committed RAW fixtures.
fn committed_directory() -> PathBuf {
    workspace_root().join("sample-data/raw")
}
