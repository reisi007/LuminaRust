//! F-103-N9: headless UI snapshot regression tests via `egui_kittest`.
//!
//! These tests render the Lumina GUI with the wgpu backend and compare the
//! rendered frame against a committed golden PNG under `tests/snapshots/`.
//!
//! They require a working GPU / headless wgpu backend, so they are `#[ignore]`d
//! by default. CI without a GPU therefore stays green. Run them locally with:
//!
//! ```text
//! UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_snapshots -- --ignored
//! ```
//!
//! `UPDATE_SNAPSHOTS=true` writes/updates the goldens; without it the tests
//! compare against the committed goldens and fail (producing a `<name>.diff.png`)
//! when the rendered UI changed.
//!
//! A red snapshot means the rendered UI differs from the committed golden. Most
//! often this is a legitimate UI change that needs a golden refresh; occasionally
//! it is a regression. Inspect the generated `<name>.diff.png` and, if the change
//! is intended, re-run with `UPDATE_SNAPSHOTS=true`.

use egui_kittest::kittest::NodeT;
use egui_kittest::{kittest::Queryable, Harness};
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{
    LibraryView, LuminaApp, Module, PinVisibility, ZoomMode, LIBRARY_BADGE_BG, SECTION_COLOR,
    SECTION_COUNT, SECTION_DETAIL, SECTION_EFFECTS, SECTION_GEOMETRY, SECTION_MASKING,
    SECTION_OPTICS, SECTION_TONE_CURVE,
};
use std::path::{Path, PathBuf};

// UX-LOOK-HISTORY-18: shared harness helpers moved here so this file stays
// within its committed size baseline (see the support module docs).
mod kittest_snapshots_support;
use kittest_snapshots_support::*;

// GOLDEN-FIXT-31: the real-RAW fixture contract (staging the two licensed CR3s
// from `sample-data/raw/`, plus the "the worker really decoded" settle/guards).
// Shared with `kittest_library_stack`, which owns the stack-membership golden.
mod kittest_fixtures_support;
use kittest_fixtures_support::*;

// KITT-IDENTITY-49: the wall-clock bound for opening a real file and the state
// report it prints on timeout (shared with `kittest_crop_overlay` and
// `kittest_sidecar_identity`); KITT-DECODE-CONTRACT-52: all `DecodeSettled`.
mod kittest_decode_support;
use kittest_decode_support::{pump_until_ready, DecodeSettled, SETTLE_DEADLINE};
// KITT-IDENTITY-49: sidecar fixtures must carry the fingerprint of the bytes on
// disk; see the module docs for why the History seed used to violate that.
mod kittest_sidecar_support;
use kittest_sidecar_support::{seed_metadata_history_sidecar, test_jpeg_bytes};

/// The preview has rendered at least once — the state most file-open goldens
/// below wait for before snapshotting. Described here so an expiry report names
/// it next to the values it found.
fn ready_rendered() -> DecodeSettled<impl FnMut(&LuminaApp) -> bool> {
    DecodeSettled::new("preview_generation() >= 1", |app: &LuminaApp| {
        app.preview_generation() >= 1
    })
}

/// The document was adopted with **exactly** `count` metadata history entries.
/// The exact count is the point: a sidecar refused as stale leaves the history
/// empty, and "some history" would hide exactly that.
fn ready_history_of(count: usize) -> DecodeSettled<impl FnMut(&LuminaApp) -> bool> {
    DecodeSettled::new(
        format!("metadata_history().len() == {count}"),
        move |app: &LuminaApp| app.metadata_history().len() == count,
    )
}

/// The preview rendered **and** the source's embedded IPTC became readable.
fn ready_rendered_with_embedded_iptc() -> DecodeSettled<impl FnMut(&LuminaApp) -> bool> {
    DecodeSettled::new(
        "preview_generation() >= 1 && embedded_metadata() is Some",
        |app: &LuminaApp| {
            app.preview_generation() >= 1
                && app
                    .embedded_metadata()
                    .unwrap_or_else(|error| {
                        panic!("embedded IPTC must be readable once the preview rendered: {error}")
                    })
                    .is_some()
        },
    )
}

/// Documented reason for `#[ignore]` so CI without a GPU stays green:
/// "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"
/// (repeated inline on each `#[ignore]` attribute below, since the attribute
/// requires a string literal).
///
/// Collapsing-header labels of the Develop panel, in draw order.
///
/// Kept in sync with `LuminaApp::DEVELOP_SECTIONS` (single source of truth in
/// `crates/lumina-gui/src/develop_frame.rs`, labels via `Str::*` in
/// `crates/lumina-gui/src/i18n.rs`): the eight F-100 sections `Basic` … `Masking`
/// after UX-LOOK-LAYOUT-18 moved Presets/History to the left rail (`develop_left_rail.rs`).
/// F-103-N10 (user decision 2026-08-25): Detail BEFORE Effects (Lightroom
/// Classic panel order). The committed goldens were rebaselined to the
/// Detail-before-Effects layout (GUI-KIT-01-REFRESH).
const DEVELOP_SECTIONS: &[&str] = &[
    "Basic",
    "Tone Curve",
    "Color",
    "Detail",
    "Effects",
    "Optics",
    "Geometry",
    "Masking",
];

/// Expand exactly the Develop sections listed in `keep`.
///
/// egui 0.36 `CollapsingHeader` is **closed by default** (previously open), so
/// the old "collapse everything except keep" logic kept all ten sections closed.
/// The correct operation is now to *open* the keep-sections. Each `click()` is
/// followed by a `harness.run()` so the click event is dispatched before the
/// next query — bundling all clicks into a single final `run()` only scrolls the
/// `ScrollArea` and the clicks never register (verified via diff of the two
/// goldens: previously both showed all 9 sections collapsed).
fn collapse_except(harness: &mut Harness<'_, LuminaApp>, keep: &[&str]) {
    // Ensure the first frame is laid out so `query_all_by_label` can find the
    // headers (egui_kittest requires a `run()` before querying).
    harness.run();
    for section in keep {
        // `DEVELOP_SECTIONS` is the authoritative label list; keep-entries must
        // be a subset of it — a missing label is a test bug, not a silent skip.
        assert!(
            DEVELOP_SECTIONS.contains(section),
            "unknown Develop section label {:?}; expected one of {:?}",
            section,
            DEVELOP_SECTIONS
        );
        let clicked = {
            if let Some(node) = harness.query_all_by_label(section).next() {
                node.click();
                true
            } else {
                panic!("Develop section label not found in headed harness: {section:?}");
            }
        };
        if clicked {
            harness.run();
        }
    }
}

/// Fixture directory for the deterministic Library snapshots below.
///
/// `library_empty` / `library_with_image` used to render with the default
/// workdir (`"."` = the live crate checkout), so the Folders tree listed
/// whatever the checkout contained (`src/`, `benches/`, …) and the goldens
/// broke on every unrelated file addition (e.g. a 415px diff from a single
/// `benches/` row).
///
/// A `tempfile::tempdir` was considered (preferred option in GUI-KIT-01-REFRESH)
/// but rejected: the folder tree renders both the full directory string (path
/// text field) and the root basename, so a random `tmp.XXXXXX` path would leak
/// nondeterministic pixels into every snapshot. This committed, read-only
/// fixture with a *relative* path keeps every rendered string fixed:
/// `is_supported_image` ignores `.gitkeep`, so the grid stays empty, the RAW
/// count stays 0, and the tree shows only the fixed root label `library`.
/// Cargo runs integration tests with CWD = the package root, so the relative
/// path resolves on every machine. The fixture is never written to (both
/// tests only list it), hence sharing it between the two tests is race-free.
const LIBRARY_FIXTURE_DIR: &str = "tests/fixtures/library";

/// Point the app at the deterministic fixture directory (see above).
fn use_library_fixture(harness: &mut Harness<'_, LuminaApp>) {
    set_directory_and_settle(harness, LIBRARY_FIXTURE_DIR.to_owned());
}

// FILE-SIZE-1000-ALL-EXCEPTIONS-100: the 56 goldens are grouped by theme
// into child modules of this test binary (pure move; no test body changed).
#[path = "kittest_snapshots/develop.rs"]
mod develop;
#[path = "kittest_snapshots/library.rs"]
mod library;
#[path = "kittest_snapshots/metadata.rs"]
mod metadata;
#[path = "kittest_snapshots/overlays.rs"]
mod overlays;

/// Open exactly one Develop section via the public `set_section_open`
/// setter (deterministic, no header clicks — the other seven stay closed)
/// and scroll `target_label` into view so its widgets are pixel-visible in
/// the golden at the default 1024x720 viewport.
///
/// KITTEST-EXPANDED-VIEWPORT-1: `develop_sections_expanded` above expands
/// Basic + Color + Masking at once, so Color/Masking content is pushed
/// below the viewport fold and never pixel-visible (1024x720 clipping, a
/// single frame cannot show the Basic top and the Masking bottom at
/// once). These per-section snapshots are the chosen "einzeln snapshotten"
/// alternative: one open section each, scrolled into view.
fn expand_and_scroll_to(harness: &mut Harness<'_, LuminaApp>, section: usize, target_label: &str) {
    assert!(
        section < SECTION_COUNT,
        "unknown Develop section index {section}; SECTION_COUNT = {SECTION_COUNT}"
    );
    for i in 0..SECTION_COUNT {
        harness.state_mut().set_section_open(i, i == section);
    }
    // Layout frame so the accesskit tree contains the opened section.
    harness.run();
    let found = harness
        .query_all_by_label(target_label)
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        found,
        "scroll target {target_label:?} not found in headed harness (section {section})"
    );
    // One frame dispatches the ScrollIntoView event, the second settles the
    // scrolled layout before snapshotting.
    harness.run();
    harness.run();
}

/// Open a real file (async decode) and pump headed frames until `ready`
/// holds, then point the browser back at the deterministic fixture
/// directory: `open_file` adopts the file's parent as the browser
/// directory, and the tempdir prefix must never leak into folders/grid/
/// filmstrip pixels (same rationale as `use_library_fixture`).
///
/// The bound is a wall-clock deadline for the same reason as `settle_scan`
/// (GOLDEN-FIXT-31): a 24-megapixel RAW needs seconds, not 500 fast frames.
/// `ready` carries its own description, so a timeout prints the expected state
/// next to the found one (KITT-IDENTITY-49: a bare "never settled" cost five
/// minutes to diagnose).
fn open_file_and_restore_fixture<F: FnMut(&LuminaApp) -> bool>(
    harness: &mut Harness<'_, LuminaApp>,
    path: &Path,
    ready: DecodeSettled<F>,
) {
    pump_until_ready(harness, path, ready, SETTLE_DEADLINE);
    set_directory_and_settle(harness, LIBRARY_FIXTURE_DIR.to_owned());
    harness.run();
}

/// Write the bundled sample PNG as `photo.png` into a fresh tempdir (fixed
/// file name → deterministic status/identity strings; the random tempdir
/// prefix never renders, see `assert_no_tmp_leak`).
fn photo_png_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = dir.path().join("photo.png");
    std::fs::write(&photo, LuminaApp::sample_image_png()).expect("write png fixture");
    (dir, photo)
}

/// Proves the preview is past the empty state: a real source rendered
/// (`preview_generation() >= 1`) and the empty-state placeholder is absent
/// (its label is `Str::NoImage`: "Drop an image here or load a path").
///
/// Headless trait (corrected, KITTEST-COVERAGE-OVERLAYS-1): texture blits
/// **are** captured in this environment — the `library_loupe` golden paints
/// real thumbnail textures, and the sample preview's own 4x3 pixels are
/// present in the Develop goldens (a handful of native-size texels; the CPU
/// `Image` widget draws the texture at its exact size rather than the fitted
/// pane). Painter primitives (pin circles, rect strokes) are captured too.
/// The mask matte is a texture blit whose golden was previously blank only
/// because `draw_mask_overlay` dropped its per-frame texture handle before the
/// paint; that production bug is fixed and `assert_mask_overlay_visible`
/// now asserts the matte pixels directly.
fn assert_preview_loaded(harness: &mut Harness<'_, LuminaApp>) {
    assert!(
        harness.state_mut().preview_generation() >= 1,
        "a real source must render at least once (no empty-preview golden)"
    );
    assert!(
        harness
            .query_all_by_label("Drop an image here or load a path")
            .next()
            .is_none(),
        "empty-state placeholder must be gone (source loaded)"
    );
}

/// Leakage proof for tempdir-backed overlay tests: the browser directory is
/// back on the relative fixture (the only path surfaces are the path field
/// and the folder/navigator tree, both fixture-driven after the reset) and
/// no accessible label contains the random tempdir component.
fn assert_no_tmp_leak(harness: &mut Harness<'_, LuminaApp>, tmp: &Path) {
    assert_eq!(
        harness.state_mut().directory(),
        LIBRARY_FIXTURE_DIR,
        "browser must render the relative fixture dir (no tmp-prefix pixels)"
    );
    let component = tmp
        .file_name()
        .and_then(|name| name.to_str())
        .expect("tmp basename");
    assert!(
        harness
            .query_all_by_label_contains(component)
            .next()
            .is_none(),
        "tempdir prefix {component:?} leaked into accessible labels"
    );
}
