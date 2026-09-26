//! AUTO-TONE-CLI-6: the structural pin of „one write path“.
//!
//! The behavioural tests in this slice prove that `process --auto-tone` and
//! `regenerate --module auto-tone` agree today. This test pins the *structure*
//! that keeps them in agreement: the CLI crate must contain **exactly one**
//! place that writes the six AUTO-TONE adjustment keys, the six
//! `auto_features` mirrors and the analysis fingerprint, and it must be
//! `crates/lumina-cli/src/auto_tone_cli.rs`. A second, slimmer copy anywhere
//! else in the crate (or inside the writer itself) fails here.

use super::*;
use std::path::{Path, PathBuf};

/// Every `.rs` file of the CLI crate except the test modules (`src/tests/**`),
/// which contain fixtures, not writers.
fn cli_sources() -> Vec<PathBuf> {
    fn walk(directory: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

/// Counts the occurrences of `needle` per file (only files with at least one).
fn occurrences(needle: &str) -> Vec<(PathBuf, usize)> {
    cli_sources()
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            let count = text.matches(needle).count();
            (path, count)
        })
        .filter(|(_, count)| *count > 0)
        .collect()
}

/// The six `auto_features` mirror assignments, the fingerprint assignment and
/// the six adjustment writes exist exactly once each — in
/// `auto_tone_cli.rs`.
#[test]
fn there_is_exactly_one_auto_tone_write_path() {
    let writer = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/auto_tone_cli.rs");
    let mut checked = 0;
    for mirror in [
        "auto_exposure = Some(",
        "auto_contrast = Some(",
        "auto_whites = Some(",
        "auto_blacks = Some(",
        "auto_highlights = Some(",
        "auto_shadows = Some(",
    ] {
        let found = occurrences(mirror);
        assert_eq!(
            found,
            vec![(writer.clone(), 1)],
            "the mirror write `{mirror}` must exist exactly once in the whole CLI crate and live \
             in auto_tone_cli.rs"
        );
        checked += 1;
    }
    // The analysis fingerprint is written by the same single path.
    let found = occurrences("analysis_fingerprint = Some(");
    assert_eq!(
        found,
        vec![(writer.clone(), 1)],
        "the analysis fingerprint must be written exactly once, in auto_tone_cli.rs"
    );
    checked += 1;
    // The six adjustment keys are written by the single `write_auto_tone_state`
    // loop, not six times: the key table is the single source of their names,
    // and it may only be touched inside the writer module.
    let found = occurrences("AUTO_TONE_ADJUSTMENT_KEYS");
    assert_eq!(
        found.len(),
        1,
        "`AUTO_TONE_ADJUSTMENT_KEYS` may only be referenced inside auto_tone_cli.rs, found {found:?}"
    );
    assert_eq!(
        found[0].0, writer,
        "the key table lives in auto_tone_cli.rs"
    );
    assert!(
        found[0].1 >= 3,
        "the key table must be used by the definition, the write loop and the freshness predicate \
         (found {} references)",
        found[0].1
    );
    checked += 1;
    // ...and exactly one place iterates it to write sliders, so even a *second
    // writer inside the writer module* (a "slimmer copy") is caught.
    let found = occurrences("AUTO_TONE_ADJUSTMENT_KEYS.iter().zip");
    assert_eq!(
        found,
        vec![(writer.clone(), 1)],
        "exactly one loop may write the six sliders from the key table — a second, slimmer writer \
         inside auto_tone_cli.rs is a contract violation"
    );
    checked += 1;
    assert_eq!(
        checked, 9,
        "the scan must cover all thirteen written fields (six mirrors + fingerprint + the key \
         table + its single write loop)"
    );
}

/// The two callers really do go through the shared writer, and no other file
/// calls `suggest_auto_tone`/`tone_fingerprint` for the Auto-Tone path.
#[test]
fn only_the_two_documented_callers_reach_the_writer() {
    let writer = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/auto_tone_cli.rs");
    let main = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    for (needle, callers) in [
        // `process_selected` and `regenerate --module auto-tone`
        ("apply_auto_tone_result(", 2usize),
        // the `regenerate` freshness predicate
        ("auto_tone_is_fresh(", 1usize),
    ] {
        let found = occurrences(needle);
        assert_eq!(
            found,
            vec![(writer.clone(), 1), (main.clone(), callers)],
            "expected the definition in auto_tone_cli.rs plus exactly {callers} caller(s) in \
             main.rs for `{needle}`, found {found:?}"
        );
    }
    // `auto_tone_input_fingerprint` is the only producer of the fingerprint the
    // reuse/freshness decisions are made against.
    let found = occurrences("tone_fingerprint(");
    assert_eq!(
        found,
        vec![(writer, 1)],
        "the Auto-Tone analysis fingerprint must be produced only by the shared helper"
    );
    // `suggest_auto_tone` is called only inside the shared writer, so no caller
    // can bypass the all-or-nothing reuse rule.
    let found = occurrences("suggest_auto_tone(");
    assert_eq!(
        found,
        vec![(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/auto_tone_cli.rs"),
            1
        )],
        "`suggest_auto_tone` must be called only by the shared writer"
    );
}

/// `process_selected` and `regenerate` contain no Auto-Tone write of their own:
/// both blocks are gone from the entry point.
#[test]
fn the_entry_point_carries_no_auto_tone_write() {
    let main =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    for needle in [
        "auto_exposure = Some(",
        "auto_contrast = Some(",
        "auto_whites = Some(",
        "auto_blacks = Some(",
        "auto_highlights = Some(",
        "auto_shadows = Some(",
        "analysis_fingerprint = Some(",
        "suggest_auto_tone(",
        "tone_fingerprint(",
    ] {
        assert!(
            !main.contains(needle),
            "main.rs must not contain `{needle}` — the Auto-Tone write path lives in \
             auto_tone_cli.rs"
        );
    }
    // Both callers of the shared writer are wired up.
    assert!(main.contains("apply_auto_tone_result("));
    assert!(main.contains("auto_tone_is_fresh("));
}
