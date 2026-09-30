//! AUTO-TONE-CLI-6 clause (1): **one** writer for the six sliders, the six
//! `auto_features` mirrors and the analysis fingerprint.
//!
//! The task named three writers — `process --auto-tone`, `regenerate
//! --module auto-tone` and `LuminaApp::auto_tone` — and required them to share
//! one implementation and one contract. Measured before this change: the CLI
//! and the regenerate path both already went through
//! `lumina_stages::auto_tone::apply_auto_tone_result`, and the **GUI carried a
//! complete second copy** (`lib.rs`: six slider inserts, six mirror writes and
//! the fingerprint, with the algorithm as a hand-copied string literal).
//!
//! Three tests, deliberately different in kind:
//!
//! * `the_gui_writes_exactly_what_the_shared_writer_writes` is a **differential**
//!   test: it runs both writers on equal frames and compares the resulting
//!   contract field by field. It cannot be satisfied by a stray reference to the
//!   shared constants, and it fails loudly if the GUI ever writes something extra
//!   or something different.
//! * `a_gui_auto_tone_recipe_is_fresh_for_the_shared_regenerate_predicate` states
//!   the consequence on its own, so a failure says *which* half broke.
//! * `the_tone_algorithm_literal_is_defined_once_and_never_hand_copied` is the
//!   **structural** guard the task asks for: a hand-copied literal is the
//!   concrete way this defect returns, and nothing else would notice.

use super::*;

/// One decoded app plus its frame, the state both writers need.
fn decoded_app(name: &str) -> (LuminaApp, ImageFrame) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join(name);
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let frame = app.original.clone().expect("a decoded frame");
    (app, frame)
}

/// The six mirrors as a comparable tuple, in [`AUTO_TONE_ADJUSTMENT_KEYS`] order.
fn mirrors(recipe: &lumina_sidecar::EditRecipe) -> [Option<f64>; 6] {
    let auto = &recipe.auto_features;
    [
        auto.auto_exposure,
        auto.auto_contrast,
        auto.auto_whites,
        auto.auto_blacks,
        auto.auto_highlights,
        auto.auto_shadows,
    ]
}

/// Clause (1), behaviourally: the GUI must not have a second idea of what the
/// contract looks like. Both apps get the same PNG; one goes through the GUI
/// action, the other through the shared writer. Every field of the contract must
/// agree.
///
/// The 1e-12 tolerance is not slack: `commit_pending_slider_save` round-trips
/// the recipe through JSON, which may re-round the last ulp. The same reason is
/// named in `g16_shortcuts.rs`, and the tolerance applies to the SIX VALUES
/// only — the fingerprint strings are compared exactly, because a JSON roundtrip
/// cannot change a string and a diverging algorithm is exactly the defect.
#[test]
fn the_gui_writes_exactly_what_the_shared_writer_writes() {
    use lumina_stages::auto_tone::{
        apply_auto_tone_result, PersistedAutoTone, AUTO_TONE_ADJUSTMENT_KEYS,
        FINGERPRINT_ALGORITHM, FINGERPRINT_VERSION,
    };

    let (mut via_gui, frame) = decoded_app("via-gui.png");
    via_gui.auto_tone().expect("the GUI action succeeds");

    let (mut via_writer, writer_frame) = decoded_app("via-writer.png");
    let target = via_writer.recipe.auto_features.target_luminance;
    apply_auto_tone_result(
        &mut via_writer.recipe,
        &writer_frame,
        target,
        PersistedAutoTone::AlwaysRecompute,
        None,
    )
    .expect("the shared writer succeeds");

    // The GUI's own frame must be the one the writer saw, or the comparison
    // below would be between two different inputs and would prove nothing.
    assert_eq!(
        frame.width, writer_frame.width,
        "both writers must see the same input, or this comparison is vacuous"
    );
    assert_eq!(frame.pixels, writer_frame.pixels, "same input pixels");

    for key in AUTO_TONE_ADJUSTMENT_KEYS {
        let from_gui = via_gui
            .recipe
            .adjustments
            .get(key)
            .copied()
            .unwrap_or_else(|| panic!("the GUI must persist `{key}`"));
        let from_writer = via_writer
            .recipe
            .adjustments
            .get(key)
            .copied()
            .unwrap_or_else(|| panic!("the shared writer must persist `{key}`"));
        assert!(
            (from_gui - from_writer).abs() < 1e-12,
            "`{key}`: GUI {from_gui} vs shared writer {from_writer}"
        );
    }

    assert_eq!(
        mirrors(&via_gui.recipe),
        mirrors(&via_writer.recipe),
        "the six auto mirrors must agree, or the GUI keeps its own write path"
    );

    let gui_fp = via_gui
        .recipe
        .auto_features
        .analysis_fingerprint
        .as_ref()
        .expect("the GUI must persist a fingerprint");
    let writer_fp = via_writer
        .recipe
        .auto_features
        .analysis_fingerprint
        .as_ref()
        .expect("the shared writer must persist a fingerprint");
    assert_eq!(gui_fp.algorithm, writer_fp.algorithm, "algorithm identity");
    assert_eq!(gui_fp.version, writer_fp.version, "fingerprint version");
    assert_eq!(
        gui_fp.input_fingerprint, writer_fp.input_fingerprint,
        "the analysis fingerprint must be the same value, not merely the same algorithm"
    );
    assert_eq!(
        gui_fp.algorithm, FINGERPRINT_ALGORITHM,
        "the identity constants are the shared ones, by name"
    );
    assert_eq!(gui_fp.version, FINGERPRINT_VERSION);

    assert!(
        via_gui.recipe.auto_features.enable_auto_tone,
        "the GUI action must arm the auto-tone flag"
    );
    assert_eq!(
        via_gui.recipe.auto_features.target_luminance, target,
        "the GUI must not change the target luminance it was given"
    );
}

/// The consequence, stated on its own so a failure says which half broke: a
/// GUI-written auto-tone must survive the *shared* freshness predicate. That
/// predicate is what the next `regenerate` run decides with, so passing it here
/// means the two paths agree about freshness as well as about values.
#[test]
fn a_gui_auto_tone_recipe_is_fresh_for_the_shared_regenerate_predicate() {
    use lumina_stages::auto_tone::{auto_tone_input_fingerprint, auto_tone_is_fresh};

    let (mut app, frame) = decoded_app("freshness.png");
    app.auto_tone().expect("the GUI action succeeds");
    let target = app.recipe.auto_features.target_luminance;
    let input_fingerprint = auto_tone_input_fingerprint(&frame, target);

    assert!(
        auto_tone_is_fresh(&app.recipe, &input_fingerprint),
        "a GUI-written auto-tone must survive the shared freshness predicate; \
         if it does not, the next regenerate run overwrites it"
    );
}

/// Clause (1), structurally. A hand-copied `"tone-rgba8-rec709"` in a crate
/// that WRITES the contract is a second definition of "which analysis produced
/// these mirrors", and the freshness predicate compares exactly that string — so
/// a divergence stays silent until a regeneration run overwrites somebody's
/// edit.
///
/// Scope, and why it is not wider. The first version of this test scanned
/// `lumina-core` too and **failed**, correctly reporting three files. Two of them
/// turned out to be legitimate, and both are named here rather than filtered
/// away silently:
///
/// * `lumina-core/src/tone.rs` *produces* the fingerprint's **value**
///   (`format!("tone-rgba8-rec709:{hash}")`) — a different string from the
///   `algorithm` field, and that crate is its producer by definition.
/// * `lumina-core/src/render.rs` carries a `#[cfg(test)]` **fixture** inside an
///   inline test module, which cannot import the constant: `lumina-stages`
///   depends on `lumina-core`, not the other way round.
/// * `lumina-core/src/upright.rs` owns a *different* analysis identity (upright
///   rotation), so the tone algorithm is not its business.
///
/// The exclusion is therefore pinned, not assumed: the allowlist below names
/// `lumina-core`'s two legitimate files, so a **third** production copy there
/// fails this test instead of quietly passing by virtue of the exclusion.
#[test]
fn the_tone_algorithm_literal_is_defined_once_and_never_hand_copied() {
    use lumina_stages::auto_tone::FINGERPRINT_ALGORITHM;
    use std::path::{Path, PathBuf};

    /// Recursively collect the `.rs` files under `dir`, skipping test modules.
    fn production_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                production_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let is_test_module = path.components().any(|c| c.as_os_str() == "tests")
                    || path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|name| name.ends_with("_test") || name.ends_with("_tests"));
                if !is_test_module {
                    out.push(path);
                }
            }
        }
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the gui crate lives inside crates/")
        .to_path_buf();
    let relative = |path: &Path| {
        path.strip_prefix(&workspace)
            .unwrap_or(path)
            .display()
            .to_string()
    };
    let sites_in = |crate_name: &str| -> Vec<(String, usize)> {
        let mut files = Vec::new();
        production_files(&workspace.join(crate_name).join("src"), &mut files);
        let mut sites: Vec<(String, usize)> = files
            .iter()
            .filter_map(|path| {
                let source = std::fs::read_to_string(path).ok()?;
                let count = source.matches(FINGERPRINT_ALGORITHM).count();
                (count > 0).then(|| (relative(path), count))
            })
            .collect();
        sites.sort();
        sites
    };

    // The four crates that own the auto-tone contract: the writer, the two front
    // ends and the MCP surface. Any hand copy in any of them is a defect.
    //
    // The invariant is (file, count), NOT a line number: pinning a line makes the
    // test fail whenever an unrelated doc comment above the definition grows —
    // a failure that says nothing about the contract. The count still makes a
    // second copy *inside* the sanctioned file a defect.
    let mut contract_writers: Vec<(String, usize)> =
        ["lumina-stages", "lumina-gui", "lumina-cli", "lumina-mcp"]
            .iter()
            .flat_map(|name| sites_in(name))
            .collect();
    contract_writers.sort();

    assert_eq!(
        contract_writers,
        vec![("lumina-stages/src/auto_tone.rs".to_string(), 1)],
        "the tone algorithm must appear exactly once in the crates that write the \
         contract, and that one place is the constant definition. A second \
         occurrence is a hand copy: {contract_writers:?}"
    );

    assert_eq!(
        sites_in("lumina-core"),
        vec![
            // 2x, not 1: the inline #[cfg(test)] fixture names the algorithm in
            // `algorithm:` AND repeats it inside the `input_fingerprint` value it
            // fakes. Both are one fixture, not two definitions.
            ("lumina-core/src/render.rs".to_string(), 2),
            ("lumina-core/src/tone.rs".to_string(), 1),
        ],
        "lumina-core may name the tone algorithm in `tone.rs` (it produces the \
         fingerprint VALUE) and twice in `render.rs` (one inline #[cfg(test)] \
         fixture, which cannot import the constant because lumina-stages depends \
         on lumina-core). A THIRD site here is a competing definition."
    );
}
