//! GUI-INT-MASKLOCAL-38: kittest goldens for the four **mask-local** editors
//! (MASK-LOCAL-P1.2a Tone Curves, P1.2b Color, P1.2c Presence, P1.2d Detail).
//!
//! # Why a separate target
//!
//! `tests/kittest_snapshots.rs` is ratchet-baselined at its exact current size
//! and must not grow (`scripts/file_size_baseline.txt`, `Agents.md` / DoD §8),
//! and these four surfaces are one feature's coverage rather than a slice of
//! the general Develop-section sweep. A feature-specific target is the pattern
//! the other feature-bound goldens already use (`kittest_spot_tool`,
//! `kittest_crop_overlay`, `kittest_mask_visibility`, `kittest_library_stack`).
//!
//! # Fixture class — C (Chrome/Layout) on the S2 smoke fixture
//!
//! All four are **class C** on the **S2** smoke fixture
//! (`LuminaApp::sample_image_png()`, 4x3 px), loaded in memory via
//! `load_bytes` — exactly the fixture and the loading mode of the 46
//! `kittest_snapshots` Develop goldens. They pin the **editor surface**: the
//! block caption, the channel/band/range selectors, the drawn curve graph with
//! its mandatory endpoints, the slider rows with their labels and neutral
//! readouts, the reset buttons, and the vertical order of the four blocks.
//!
//! They are **not** a render proof. A 4x3 synthetic source cannot reveal a
//! decode, demosaic, colour or geometry regression, and no mask-local golden
//! may ever be cited as one. `assert_no_decode_failure` enforces the
//! GOLDEN-FIXT-31 rule that a decode failure is never a golden's intended
//! content — the mistake the removed Library goldens made when they captured
//! the red "LibRaw opening input failed" banner.
//!
//! # What is deliberately *not* in these frames
//!
//! **No applied edit.** The in-memory `load_bytes` source has no persistence
//! target, so the debounced sidecar save that any local edit arms fails loudly
//! and paints a modal error dialog across the preview. Two fixtures were
//! measured and rejected for that reason:
//!
//! * a `tempfile::tempdir` source makes the save succeed but leaks the
//!   machine's absolute `$TMPDIR` into the Generative Expand panel's path field
//!   — a machine-specific string in a committed golden, which
//!   `golden-references.md` §6.2 explicitly forbids;
//! * a committed relative fixture makes the save succeed, but the four tests
//!   would then share one sidecar (parallel runs and repeat runs collide on the
//!   persisted mask) and would add committed files to the `fixtures.digest`,
//!   which is `GOLDEN-BASELINE-32`'s to re-pin.
//!
//! So the pixels here pin **layout**, and the **values** are pinned where they
//! belong: `tests/mask_local_editors.rs` drives the real editors with real
//! pointer events and asserts the persisted sidecar bytes, and
//! `lumina-core`'s exact CPU goldens pin the stage maths. Nothing is lost by the
//! split, and each half stays free of the other's failure modes.
//!
//! One consequence is named rather than hidden: at the 1024x720 reference
//! viewport a mask-local block is taller than the panel, so each golden pins
//! the part of its editor that fits above the fold (the `scroll_into_view`
//! guard proves the anchor is pixel-visible). The rows below the fold — the
//! grading block of the colour editor, the block resets of the lower editors —
//! are covered by the interaction target, not by these pixels.
//!
//! # Running them
//!
//! `#[ignore]`d, because the wgpu backend needs a real adapter and CI has no
//! GPU runner: they are a **local** macOS gate and are never verified in CI
//! (`feature/quality/golden-references.md` §10). Locally:
//!
//! ```text
//! cargo test -p lumina-gui --test kittest_mask_local -- --ignored
//! ```

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module, SECTION_COUNT, SECTION_MASKING};

/// Headless harness at the reference viewport (golden-references.md §2.1:
/// 1024x720, `pixels_per_point` 1.0).
fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Open the S2 smoke source, seed one selected mask and open exactly the
/// Masking section.
///
/// `create_mask` is pure session state, the same side-effect-free seed the
/// existing `develop_section_masking` golden uses. Without a selected mask the
/// four mask-local editors are gated off (`selected_mask_id.is_some()` in
/// `draw_masking`), so this is what makes their surfaces reachable at all.
fn open_masking(harness: &mut Harness<'static, LuminaApp>, mask_name: &str) {
    harness.state_mut().set_module(Module::Develop);
    harness
        .state_mut()
        .load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .expect("sample image loads");
    harness
        .state_mut()
        .create_mask(mask_name)
        .expect("seed mask layer");
    for index in 0..SECTION_COUNT {
        let open = index == SECTION_MASKING;
        harness.state_mut().set_section_open(index, open);
    }
    harness.run();
    assert!(
        harness.state().selected_mask_local_presence().is_ok(),
        "the seeded mask layer must be selected once the panel is laid out"
    );
}

/// Scroll the widget carrying `label` into view and prove it is pixel-visible.
///
/// Existence in the accesskit tree alone is not enough: at 1024x720 a widget
/// below the `ScrollArea` fold is laid out but never painted, so a golden could
/// pass on pixels that do not contain the editor at all. This is the
/// non-vacuity guard every one of these goldens depends on.
fn scroll_into_view(harness: &mut Harness<'static, LuminaApp>, label: &str) {
    let found = harness
        .query_all_by_label(label)
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        found,
        "scroll target {label:?} not found in the headed Masking harness"
    );
    // One frame dispatches the ScrollIntoView event, the second settles the
    // scrolled layout before snapshotting.
    harness.run();
    harness.run();
    let rect = harness
        .query_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("label {label:?} disappeared after scrolling"))
        .rect();
    let screen = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(1024.0, 720.0),
    );
    assert!(
        rect.min.y >= screen.min.y - 0.5
            && rect.max.y <= screen.max.y + 0.5
            && rect.min.x >= screen.min.x - 0.5
            && rect.max.x <= screen.max.x + 0.5,
        "label {label:?} must be pixel-visible in the 1024x720 viewport, got {rect:?} in {screen:?}"
    );
}

/// Non-vacuity guard: a RAW decode failure must never be the golden's content
/// (golden-fixtures.md §2 rule 5).
fn assert_no_decode_failure(harness: &Harness<'static, LuminaApp>) {
    assert!(
        harness.state().error().is_none(),
        "a decode failure must never be a golden's intended content: {:?}",
        harness.state().error()
    );
    assert!(
        harness
            .query_all_by_label_contains("LibRaw")
            .next()
            .is_none(),
        "the raw-decode failure banner must never be rendered (GOLDEN-FIXT-31)"
    );
    assert!(
        !harness.state().status().contains("LibRaw"),
        "the status line must not report a decode failure, got {:?}",
        harness.state().status()
    );
}

/// MASK-LOCAL-P1.2a: the mask-local tone-curve editor — the channel selector
/// (Master/R/G/B), the drawn curve graph with its two mandatory `(0,0)`/`(1,1)`
/// endpoints, and the per-channel + block reset levels.
///
/// Class C / S2. Pinned above the fold: the channel row, the graph, both reset
/// buttons and the numeric readout.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_mask_local -- --ignored"]
fn mask_local_tone_curve() {
    let mut harness = build_harness();
    open_masking(&mut harness, "Mask local tone curve");
    assert_no_decode_failure(&harness);
    scroll_into_view(&mut harness, "Point curve");
    harness.snapshot("mask_local_tone_curve");
}

/// MASK-LOCAL-P1.2b: the mask-local colour editor — the HSL band row, its
/// Hue/Saturation/Luminance rows, vibrance/saturation, the point-colour block
/// and the block reset.
///
/// Class C / S2. Pinned above the fold: the band row through the point-colour
/// block. The grading block sits below the 1024x720 fold and is covered by
/// `mask_local_editors.rs`, not by these pixels.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_mask_local -- --ignored"]
fn mask_local_color() {
    let mut harness = build_harness();
    open_masking(&mut harness, "Mask local color");
    assert_no_decode_failure(&harness);
    scroll_into_view(&mut harness, "HSL / Color Mixer");
    harness.snapshot("mask_local_color");
}

/// MASK-LOCAL-P1.2c: the mask-local presence editor — texture, clarity, dehaze
/// and the block reset.
///
/// Class C / S2. Pinned in full: the block is short enough to fit above the fold.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_mask_local -- --ignored"]
fn mask_local_presence() {
    let mut harness = build_harness();
    open_masking(&mut harness, "Mask local presence");
    assert_no_decode_failure(&harness);
    scroll_into_view(&mut harness, "Presence");
    harness.snapshot("mask_local_presence");
}

/// MASK-LOCAL-P1.2d: the mask-local detail editor — the sharpening rows
/// (amount/radius/detail/masking), the noise-reduction rows (luminance/color)
/// and the three reset levels.
///
/// Class C / S2. Pinned above the fold: both sub-blocks and the two sub-block
/// resets. The block reset is the last row and may sit at the fold.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_mask_local -- --ignored"]
fn mask_local_detail() {
    let mut harness = build_harness();
    open_masking(&mut harness, "Mask local detail");
    assert_no_decode_failure(&harness);
    scroll_into_view(&mut harness, "Amount");
    harness.snapshot("mask_local_detail");
}
