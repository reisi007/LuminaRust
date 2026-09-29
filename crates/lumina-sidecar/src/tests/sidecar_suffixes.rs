use super::*;

// ----- SUFFIX-SCOPE-2: literal pins on the on-disk suffix values -----
//
// The suite converts the remaining `*.lumina.json` / `*.lumina.zdata` literals
// onto `SIDECAR_FILE_SUFFIX` / `ZDATA_FILE_SUFFIX`, so after that change **no**
// consumer carries the value any more. This module is the one place that states
// the values as literals, so a rename cannot pass the whole suite by moving
// every caller onto the new constant at once.
//
// Two pins that are deliberately *not* the same assertion:
//
// * `document_io::sidecar_path_keeps_full_source_name` pins the **derived
//   path** (`sidecar_path_for` appends `.lumina.json`). It would stay green if
//   `sidecar_path_for` hardcoded the suffix and the constant drifted apart.
// * The pins below pin the **constants themselves**. They would stay green if
//   `sidecar_path_for` were rewritten to build a different name.
//
// They can only both be green while the constant, the builder, and the pinned
// on-disk contract agree — which is the property the rename decision
// (NAMING-F1) has to change deliberately.
//
// The expectation lives in this file and not next to the constants in
// `src/sidecar_paths.rs` (DoD §9): a pin whose expectation sits in the same
// source file as the code it guards is a declaration of intent, not a guard.

/// The JSON sidecar's on-disk suffix, as a literal.
///
/// Existing sidecars on user disks carry this exact name; changing it is a
/// migration (NAMING-F1), not a refactor.
#[test]
fn sidecar_file_suffix_is_pinned_to_its_committed_literal() {
    assert_eq!(SIDECAR_FILE_SUFFIX, ".lumina.json");
}

/// The binary bundle's on-disk suffix, as a literal.
///
/// This value had **no direct** pin before this module. Measured 2026-09-29:
/// mutating it to `.lumina.MUTANT.zdata` left the default-feature
/// `lumina-sidecar` lib suite green (278 passed / 0 failed) — but the `zdata`
/// module is `#[cfg(feature = "zdata")]`, and with `--all-features` four
/// pre-existing `zdata::tests` plus one `lumina-mcp` consumer fail, because
/// they reach the value indirectly through `zdata_path_for`. This module adds
/// the direct pin that holds in every configuration. SUFFIX-SCOPE-2 routes
/// the CLI's `unwrap_or("bundle.lumina.zdata")` write-path fallback onto this
/// constant, which makes the unpinned value a write-path value.
#[test]
fn zdata_file_suffix_is_pinned_to_its_committed_literal() {
    assert_eq!(ZDATA_FILE_SUFFIX, ".lumina.zdata");
}

/// The two suffixes are distinct, so a path that satisfies one predicate can
/// never satisfy the other by accident.
///
/// Without this, setting both constants to the same string would keep both
/// literal pins' *intent* intact while making every `ends_with` check in the
/// tree ambiguous between the sidecar and its bundle.
#[test]
fn the_two_suffixes_are_distinct() {
    assert_ne!(SIDECAR_FILE_SUFFIX, ZDATA_FILE_SUFFIX);
    assert!(
        !SIDECAR_FILE_SUFFIX.ends_with(ZDATA_FILE_SUFFIX),
        "the sidecar suffix must not also match the bundle suffix"
    );
}

/// The builder and the constant are one source: `sidecar_path_for` must derive
/// its name from [`SIDECAR_FILE_SUFFIX`], not from a second copy of the suffix.
///
/// This is the pair that `document_io::sidecar_path_keeps_full_source_name`
/// cannot cover on its own. That test pins the *result* against a literal, so
/// it stays green when the builder hardcodes `.lumina.json` and the constant is
/// changed to something else — the two would then disagree, and every consumer
/// converted by SUFFIX-SCOPE-2 would follow the constant while the builder
/// keeps writing the old name. Here the expectation is built *from* the
/// constant, so a builder whose output diverges from it fails here
/// (measured: hardcoded builder plus drifted constant turns red). A builder
/// that merely duplicates the literal while the constant is unchanged still
/// passes — this test guards divergence, not duplication.
#[test]
fn the_builder_uses_the_pinned_constant() {
    let source = Path::new("/photos/IMG_0001.ARW");
    let built = sidecar_path_for(source);
    let expected = Path::new("/photos").join(format!("IMG_0001.ARW{SIDECAR_FILE_SUFFIX}"));
    assert_eq!(built, expected);
    // And the derived name is the one the committed literal describes, so the
    // constant-derived and literal-pinned views agree.
    assert!(built.to_string_lossy().ends_with(".lumina.json"));
}
