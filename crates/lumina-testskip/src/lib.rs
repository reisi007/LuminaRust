//! Visible, explained skip reporting for env-gated tests
//! (`FIXTURE-SKIP-VISIBLE-2`).
//!
//! **Not product code.** It is a test-infrastructure crate with no runtime
//! consumers, and it exists because the two obvious channels for a skip are
//! both wrong — and because one of them was measured, not assumed.
//!
//! 1. A panic is not one failed assertion but an aborted `--ignored`
//!    invocation, and `--ignored` is *the* documented way to run the local
//!    proofs. One absent optional fixture would turn the whole run red and hide
//!    every test behind it.
//! 2. `eprintln!` alone is **invisible**: libtest discards the captured output
//!    of a *passing* test, so a skip reason printed that way never reaches the
//!    developer. Measured on `matrix::tests::real_matrix_headless`.
//!
//! So the reason goes to the **real** stderr, bypassing libtest's capture.
//!
//! # What is measured, and what is not
//!
//! **Measured** (this host, LibRaw 0.22.2, `cargo test -p lumina-gui --lib --
//! --ignored real_matrix_headless --test-threads=1`, **without** `--nocapture`):
//! the `SKIPPED …` line appears in the terminal output and the test reports
//! `ok`. So the bypass works, and the crate doc does not merely claim it.
//!
//! **Not enforced here:** a *new* site that silently falls back to `eprintln!`
//! would stay green. That is a policy gap, closed outside this crate by
//! `scripts/check_env_gate_reasons.sh` (every `#[ignore = "…"]` must name both
//! `needs` and `run:`), which is itself only a static check — it cannot see a
//! `eprintln!` in a function body.
//!
//! It lives in its own crate rather than in `lumina-core` because a shared
//! *test* helper inside a product crate makes the product depend on its own
//! test conventions, and the four sites this serves span four crates that do
//! not otherwise share one.

use std::io::Write as _;

/// Write one line to the **real** stderr, bypassing libtest's capture.
pub fn report_to_real_stderr(line: &str) {
    let mut stderr = std::io::stderr();
    // Best effort: an unwritable stderr must not turn a caveat into a failure.
    let _ = stderr.write_all(line.as_bytes());
    let _ = stderr.flush();
}

/// Build the skip line for a gate, so the test name appears **exactly once**.
///
/// This split exists because of a **measured** defect: passing a
/// [`env_gate`] string to a reporter that also prefixes the test name printed
/// `SKIPPED real_matrix_headless: real_matrix_headless: needs …` — the name
/// twice. Prose review did not catch it; running the test did. Hence the
/// reporter takes the finished line, not a name plus a reason.
#[must_use]
pub fn env_skip_line(gate: &str) -> String {
    format!("SKIPPED {gate}\n")
}

/// Report the skip for a gate built by [`env_gate`].
pub fn report_env_gate(gate: &str) {
    report_to_real_stderr(&env_skip_line(gate));
}

/// Report a **visible, explained** skip with a free-form reason.
///
/// Prefer [`report_env_gate`]: it takes a gate string that already carries the
/// test name, and therefore cannot print that name twice.
pub fn report_env_skip(test: &str, reason: &str) {
    report_to_real_stderr(&format!("SKIPPED {test}: {reason}\n"));
}

/// Build the reason for a gate: what is missing, then the exact command.
///
/// A gate that names two different commands is a gate nobody can reproduce: one
/// developer reads the attribute, another reads the log, and the two disagree.
#[must_use]
pub fn env_gate(test: &str, needs: &str, command: &str) -> String {
    format!("{test}: needs {needs}; run: {command}")
}

/// What a set of required environment variables says about an env-gated proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvState<'a> {
    /// **Not armed**: no variable is set. An ordinary skip — report it and
    /// return.
    Absent,
    /// **Misconfigured**: some but not all required variables are set. The
    /// operator asked for this proof, so a silent skip hides a typo. Never
    /// report this as an ordinary skip.
    Partial {
        /// The required variables that are **not** set, in the caller's order.
        missing: Vec<&'a str>,
    },
    /// **Armed**: every required variable is set.
    Armed,
}

/// Classify one env-gated proof from its required variables.
///
/// A single required variable can never be [`EnvState::Partial`]; the variant
/// exists for proofs that need a *set* (a model path plus its companion, a
/// fixture directory plus a checksum file).
#[must_use]
pub fn classify_required<'a>(vars: &[(&'a str, Option<&'a str>)]) -> EnvState<'a> {
    let missing: Vec<&str> = vars
        .iter()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| *name)
        .collect();
    match missing.len() {
        0 => EnvState::Armed,
        n if n == vars.len() => EnvState::Absent,
        _ => EnvState::Partial { missing },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate reason must name what is missing and the exact command, in that
    /// order — a reason that names the command first reads like an instruction
    /// to run something the developer may not have.
    #[test]
    fn a_gate_reason_names_what_is_missing_before_the_command() {
        let reason = env_gate(
            "real_matrix_headless",
            "LUMINA_MATRIX=1",
            "LUMINA_MATRIX=1 cargo test -p lumina-gui --lib -- --ignored real_matrix",
        );
        assert_eq!(
            reason,
            "real_matrix_headless: needs LUMINA_MATRIX=1; run: LUMINA_MATRIX=1 \
             cargo test -p lumina-gui --lib -- --ignored real_matrix"
        );
    }

    /// Regression pin for a **measured** defect: the reported skip carried the
    /// test name twice, because the reporter prefixed a name the gate string
    /// already began with.
    #[test]
    fn a_reported_skip_carries_the_test_name_exactly_once() {
        let name = "real_matrix_headless";
        let line = env_skip_line(&env_gate(
            name,
            "LUMINA_MATRIX=1",
            "LUMINA_MATRIX=1 cargo test -p lumina-gui --lib -- --ignored real_matrix",
        ));
        assert_eq!(
            line.matches(name).count(),
            1,
            "the test name must appear once, got: {line}"
        );
        assert!(
            line.starts_with(&format!("SKIPPED {name}: needs ")),
            "{line}"
        );
    }

    /// A proof nobody armed is an ordinary skip: `Absent`, never `Partial`.
    #[test]
    fn an_unarmed_proof_is_absent_and_names_nothing_as_missing() {
        assert_eq!(
            classify_required(&[("LUMINA_MATRIX", None), ("LUMINA_RECIPES", None)]),
            EnvState::Absent
        );
    }

    /// Every variable present is the only thing that arms a proof.
    #[test]
    fn a_fully_set_pair_is_armed() {
        assert_eq!(
            classify_required(&[
                ("LUMINA_MATRIX", Some("1")),
                ("LUMINA_RECIPES", Some("g01"))
            ]),
            EnvState::Armed
        );
    }

    /// The defect class this crate exists to end: one of a required pair set
    /// looks like "not armed" and skips quietly, hiding a typo in the name of
    /// the variable that *was* set.
    #[test]
    fn a_half_set_pair_names_the_missing_half_instead_of_reading_as_absent() {
        let state = classify_required(&[
            ("LUMINA_FACE_DETECT_MODEL_PATH", Some("/models/yunet.onnx")),
            ("LUMINA_FACE_EMBED_MODEL_PATH", None),
        ]);
        assert_eq!(
            state,
            EnvState::Partial {
                missing: vec!["LUMINA_FACE_EMBED_MODEL_PATH"]
            },
            "a half-set pair must not be indistinguishable from an unarmed proof"
        );
    }

    /// The missing half is named in the caller's order, so the message a
    /// developer reads is stable across runs.
    #[test]
    fn a_partial_pair_lists_every_missing_variable_not_just_the_first() {
        let state =
            classify_required(&[("A", Some("1")), ("B", None), ("C", None), ("D", Some("4"))]);
        assert_eq!(
            state,
            EnvState::Partial {
                missing: vec!["B", "C"]
            }
        );
    }
}
