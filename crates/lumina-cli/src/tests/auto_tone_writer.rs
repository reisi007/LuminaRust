//! AUTO-TONE-CLI-6: the structural pin of „one write path“.
//!
//! The behavioural tests in this slice prove that `process --auto-tone` and
//! `regenerate --module auto-tone` agree today. This test pins the *structure*
//! that keeps them in agreement: the workspace must contain **exactly one**
//! place that writes the six AUTO-TONE adjustment keys, the six
//! `auto_features` mirrors and the analysis fingerprint, and it must be
//! `crates/lumina-stages/src/auto_tone.rs`. A second, slimmer copy anywhere
//! else (or inside the writer itself) fails here.
//!
//! # MCP-PARITY-B: why the scan grew
//!
//! The writer used to live in `crates/lumina-cli/src/auto_tone_cli.rs` and the
//! scan covered the CLI crate alone. `lumina_regenerate op="auto_tone"` now
//! reaches the same writer through `lumina-stages`, so the scan covers **both**
//! source trees. That makes every claim below strictly stronger than before — a
//! second copy anywhere in `lumina-cli` *or* in `lumina-stages` now fails —
//! while the "exactly once" counts are unchanged.

use super::*;
use std::path::{Path, PathBuf};

/// Every non-test `.rs` file of the two crates that can contain the writer:
/// the CLI entry point and the shared stage/artefact crate.
fn writer_sources() -> Vec<PathBuf> {
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
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["lumina-cli", "lumina-stages"] {
        walk(&crates.join("../").join(name).join("src"), &mut out);
    }
    out
}

/// The one file allowed to write the Auto-Tone state.
fn writer_module() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lumina-stages/src/auto_tone.rs")
        .canonicalize()
        .unwrap()
}

/// Counts the occurrences of `needle` per file (only files with at least one).
/// Paths are canonicalised so the expectation can be spelled without the
/// `..` segments the crate-relative roots produce.
fn occurrences(needle: &str) -> Vec<(PathBuf, usize)> {
    writer_sources()
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            let count = text.matches(needle).count();
            (path.canonicalize().unwrap(), count)
        })
        .filter(|(_, count)| *count > 0)
        .collect()
}

/// The six `auto_features` mirror assignments, the fingerprint assignment and
/// the six adjustment writes exist exactly once each — in
/// `lumina-stages/src/auto_tone.rs`.
#[test]
fn there_is_exactly_one_auto_tone_write_path() {
    let writer = writer_module();
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
            "the mirror write `{mirror}` must exist exactly once across lumina-cli and \
             lumina-stages and live in lumina-stages/src/auto_tone.rs"
        );
        checked += 1;
    }
    // The analysis fingerprint is written by the same single path.
    let found = occurrences("analysis_fingerprint = Some(");
    assert_eq!(
        found,
        vec![(writer.clone(), 1)],
        "the analysis fingerprint must be written exactly once, in \
         lumina-stages/src/auto_tone.rs"
    );
    checked += 1;
    // The six adjustment keys are written by the single `write_auto_tone_state`
    // loop, not six times: the key table is the single source of their names.
    //
    // MCP-PARITY-B briefly relaxed the next assertion to "defined once" on the
    // stated ground that the CLI's preset layer reads the names. That ground was
    // **false** — that layer hardcodes them as literals, the identifier exists
    // only in the writer — so the strict form is restored. Relaxing it was not
    // merely unnecessary but harmful: independent verification defeated the
    // relaxed form with a real, compiled, registered second writer, because the
    // remaining checks only recognised the literal `KEYS.iter().zip()` spelling.
    // Hence the two extra guards below, which hold regardless of spelling.
    let found = occurrences("AUTO_TONE_ADJUSTMENT_KEYS");
    let off_writer: Vec<_> = found.iter().filter(|(path, _)| path != &writer).collect();
    assert!(
        off_writer.is_empty(),
        "`AUTO_TONE_ADJUSTMENT_KEYS` may only be referenced inside the writer module; these \
         references are elsewhere: {off_writer:?}"
    );
    assert!(
        found.iter().any(|(path, count)| path == &writer && *count >= 3),
        "the key table must be used by the definition, the write loop and the freshness predicate, \
         found {found:?}"
    );
    checked += 1;
    // Spelling-independent, and the guard that actually closes the hole the
    // relaxed form left. Any Auto-Tone writer — however it spells its key list,
    // even as bare literals — has to switch the feature on, and nothing else in
    // production code does. The test-support files are excluded from the scan,
    // so exactly one production occurrence must exist, in the writer.
    //
    // (An earlier attempt pinned `adjustments.insert(` instead. That was wrong:
    // `main.rs` and `regenerate.rs` legitimately write *other* adjustments, so
    // the needle measured something else entirely.)
    let found = occurrences("enable_auto_tone = true");
    assert_eq!(
        found,
        vec![(writer.clone(), 1)],
        "the `enable_auto_tone` switch must be flipped exactly once across both crates, in the \
         writer module; a second writer cannot avoid it, found {found:?}"
    );
    checked += 1;
    // ...and exactly one place iterates it to write sliders, so even a *second
    // writer inside the writer module* (a "slimmer copy") is caught.
    let found = occurrences("AUTO_TONE_ADJUSTMENT_KEYS.iter().zip");
    assert_eq!(
        found,
        vec![(writer.clone(), 1)],
        "exactly one loop may write the six sliders from the key table — a second, slimmer writer \
         inside the writer module is a contract violation"
    );
    checked += 1;
    // `checked` counts distinct invariants, not fields: the six mirrors are
    // covered by one loop, and MCP-PARITY-B added two — the restored strict
    // table-reference pin and the spelling-independent `enable_auto_tone` pin.
    assert_eq!(
        checked, 10,
        "every pinned invariant must have run: six mirrors (one loop) + fingerprint + the key \
         table's single definition + its write loop + the strict reference pin + the \
         spelling-independent `enable_auto_tone` pin + the three caller pins"
    );
}

/// The three documented callers really do go through the shared writer, and no
/// other file calls `suggest_auto_tone`/`tone_fingerprint` for the Auto-Tone
/// path. MCP-PARITY-B added the third caller: `lumina_regenerate op="auto_tone"`
/// lives in the shared crate, so the freshness predicate is called from there
/// instead of from `main.rs`.
#[test]
fn only_the_documented_callers_reach_the_writer() {
    let writer = writer_module();
    let main = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/main.rs")
        .canonicalize()
        .unwrap();
    let regenerate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../lumina-stages/src/regenerate.rs")
        .canonicalize()
        .unwrap();
    // `process_selected` (`process --auto-tone`) and `regenerate --module
    // auto-tone` / `lumina_regenerate op="auto_tone"` all reach the writer.
    // The scan walks `lumina-cli` first, so that is the expected order.
    assert_eq!(
        occurrences("apply_auto_tone_result("),
        vec![
            (main.clone(), 1),
            (writer.clone(), 1),
            (regenerate.clone(), 1)
        ],
        "expected the definition in the writer module plus `process_selected` in main.rs and \
         the `auto-tone` module in the shared crate"
    );
    // The freshness predicate is the `regenerate` decision and is now called
    // only from there.
    assert_eq!(
        occurrences("auto_tone_is_fresh("),
        vec![(writer.clone(), 1), (regenerate.clone(), 1)],
        "expected the definition in the writer module plus the shared `regenerate`"
    );
    // `auto_tone_input_fingerprint` is the only producer of the fingerprint the
    // reuse/freshness decisions are made against.
    assert_eq!(
        occurrences("tone_fingerprint("),
        vec![(writer.clone(), 1)],
        "the Auto-Tone analysis fingerprint must be produced only by the shared helper"
    );
    // `suggest_auto_tone` is called only inside the shared writer, so no caller
    // can bypass the all-or-nothing reuse rule.
    assert_eq!(
        occurrences("suggest_auto_tone("),
        vec![(writer, 1)],
        "`suggest_auto_tone` must be called only by the shared writer"
    );
}

/// `process_selected` and the shared `regenerate` contain no Auto-Tone write of
/// their own: the write is only reachable through the shared writer module.
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
             lumina-stages/src/auto_tone.rs"
        );
    }
    // `process_selected` reaches the shared writer.
    assert!(main.contains("apply_auto_tone_result("));
    // The `regenerate` freshness predicate is no longer in the entry point: the
    // whole command moved, so a freshness decision there is now the shared
    // module's business.
    assert!(
        !main.contains("auto_tone_is_fresh("),
        "main.rs must not contain `auto_tone_is_fresh(` — the freshness predicate of \
         `regenerate` lives in the shared crate with the command"
    );
    let regenerate = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lumina-stages/src/regenerate.rs"),
    )
    .unwrap();
    assert!(
        regenerate.contains("auto_tone_is_fresh("),
        "the shared `regenerate` must be the caller of the freshness predicate"
    );
}
