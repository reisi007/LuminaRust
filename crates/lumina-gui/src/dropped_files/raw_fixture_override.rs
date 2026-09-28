//! The additive operator override for the real-RAW proof (`Regel 5`).
//!
//! `feature/quality/fixtures-licensing.md` §3.2.1 **Regel 5** makes
//! `LUMINA_RAW_FIXTURE` **additive, never substituting**: the proof decodes the
//! complete committed set unconditionally, and the override can only **add**
//! one further file. There is therefore no decision point left that could
//! switch the committed set off. The failure class of four verification rounds
//! — a substituted set that waived the coverage claim — is structurally gone
//! rather than moved to the next waist.
//!
//! Two consequences are encoded here:
//!
//! * **An addition never enters the committed set.** The committed set is
//!   resolved in [`super::raw_fixture_scope`] and handled on its own; nothing
//!   here can widen, narrow or reorder it.
//! * **An addition whose file name equals a committed fixture is
//!   superfluous.** The committed file with that name is decoded regardless, so
//!   the addition is **not** decoded a second time; it is reported on the real
//!   stderr instead of becoming an exception that would silently narrow the
//!   committed set.

use std::path::{Path, PathBuf};

use super::raw_fixture_scope::{workspace_root, CommittedFixture};

/// An operator-supplied RAW operand, constructed **only** from
/// `LUMINA_RAW_FIXTURE` in [`operator_operand`].
///
/// The type is the provenance. The field is private and the type has no public
/// constructor, so the only way to obtain one is to read the environment here —
/// no consumer can label a committed fixture as operator-supplied by matching on
/// a pattern, and a false `operator-supplied` message is not expressible without
/// changing this producer (`DoD.md` §10).
pub(super) struct OperatorFixture(PathBuf);

impl OperatorFixture {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

/// What the operator operand adds to the already scope-checked committed set.
pub(super) enum OperatorAddition {
    /// A file the committed set does not already carry: appended to the proof
    /// as a weaker, additional operand.
    Append(OperatorFixture),
    /// A file whose name a committed fixture already carries: the committed
    /// file is decoded regardless, so the addition is superfluous and is
    /// reported, not decoded (Regel 5).
    Redundant {
        operand: OperatorFixture,
        committed: PathBuf,
    },
}

/// Resolve the operator operand from `LUMINA_RAW_FIXTURE`, if set, against the
/// committed set.
///
/// Returned as an **addition decision**: the caller appends
/// [`OperatorAddition::Append`] to the committed set and reports
/// [`OperatorAddition::Redundant`] without decoding it. This function never
/// removes anything from `committed`.
pub(super) fn operator_addition(committed: &[CommittedFixture]) -> Option<OperatorAddition> {
    let operand = operator_operand()?;
    let collision = committed
        .iter()
        .find(|fixture| fixture.path.file_name() == operand.path().file_name());
    Some(match collision {
        Some(fixture) => OperatorAddition::Redundant {
            operand,
            committed: fixture.path.clone(),
        },
        None => OperatorAddition::Append(operand),
    })
}

/// The operator's RAW operand from `LUMINA_RAW_FIXTURE`, if set.
///
/// This is the only environment access on the proof's path, and it is read
/// **here**, in the additive module: the committed set has already passed its
/// scope check, so this read can no longer decide whether that check runs. It
/// is also the **only** constructor of [`OperatorFixture`].
pub(super) fn operator_operand() -> Option<OperatorFixture> {
    std::env::var_os("LUMINA_RAW_FIXTURE")
        .map(|raw| OperatorFixture(resolve_fixture_path(PathBuf::from(raw))))
}

/// An absolute operand stays as given; a relative one is anchored where
/// `sample-data/` lives (the workspace root), not at the test process' CWD
/// (`crates/lumina-gui`), where a path that demonstrably exists would not
/// resolve.
fn resolve_fixture_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        workspace_root().join(path)
    }
}
