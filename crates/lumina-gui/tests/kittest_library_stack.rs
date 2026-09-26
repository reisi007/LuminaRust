//! R5-STACKVIS-21 (User-Order 2026-09-20): kittest golden for the stack
//! membership sign.
//!
//! A stacked cell is marked in **amber/gold** (`#E8A91C` bracket + offset
//! cards) while the selection keeps the **blue** `theme::ACCENT` frame. This
//! golden pins both marks on the same cells (the whole stack is selected) plus
//! an unstacked neighbor without either. Extracted into its own integration
//! file so the oversized `kittest_snapshots.rs` (file-size ratchet) does not
//! grow.
//!
//! Requires a working GPU / headless wgpu backend, so it is `#[ignore]`d by
//! default (same policy as `kittest_snapshots`). Run locally with:
//!
//! ```text
//! UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_library_stack -- --ignored
//! ```

use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module};
use std::path::Path;

// GOLDEN-FIXT-31: the real-RAW fixture contract, shared with
// `kittest_snapshots` (staging the two licensed CR3s from `sample-data/raw/`
// plus the "the worker really decoded" settle/guards).
mod kittest_fixtures_support;
use kittest_fixtures_support::*;

/// Fixed, committed fixture directory (relative, so no tempdir randomness can
/// leak into folder-tree / path-field pixels — same rationale as
/// `kittest_snapshots::LIBRARY_FIXTURE_DIR`). Nothing binary is committed
/// here: the three CR3s are staged from `sample-data/raw/` per run.
///
/// The staged files sit one level deeper (`…/images/`) on purpose: the
/// Library folder tree counts RAW files depth-limited to `FOLDER_SCAN_DEPTH`
/// (= 3) below a node, so a fixture at `tests/fixtures/library_stack/*.cr3`
/// would bump the un-pinned `library_people_empty` golden's `tests (7)` counter
/// to `tests (10)`. At `…/library_stack/images/*.cr3` the counter stays stable
/// and that unrelated golden needs no rebaseline (R5-STACKVIS-21).
const FIXTURE_DIR: &str = "tests/fixtures/library_stack/images";

/// Staged RAW files: the first two are the stack members (adjacent in name
/// order), the third is the unstacked neighbor that must carry neither mark.
const FIXTURE_FILES: &[(&str, &str)] = &[
    ("a_stack1.cr3", "aircraft-landscape.cr3"),
    ("b_stack2.cr3", "aircraft-portrait.cr3"),
    ("z_solo.cr3", "aircraft-landscape.cr3"),
];

/// The two stack members, cover first.
const STACK_MEMBERS: [&str; 2] = ["a_stack1.cr3", "b_stack2.cr3"];
const STACK_COVER: &str = "a_stack1.cr3";

/// Amber membership color, mirrored from the production constant on purpose:
/// the golden's pixel guard must not reuse the value it verifies.
const STACK_AMBER: [u8; 3] = [0xE8, 0xA9, 0x1C];

fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Drive the async folder scan (and the auto-load decode it starts) to settle
/// before snapshotting — copied from `kittest_snapshots_support::settle_scan`,
/// including its GOLDEN-FIXT-31 deadline bound: a real 24-megapixel RAW fixture
/// needs far more wall time than the old failing sentinel did.
fn settle_scan(harness: &mut Harness<'_, LuminaApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        harness.step();
        if !harness.state().scan_pending() && !harness.state().decode_pending() {
            harness.step();
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "folder scan/decode did not settle within the bounded deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// (Re-)stage the real RAW files, pre-create the folder cache and write the
/// Sidecar-first stack membership (`SidecarDocument.stack`) into both members.
/// Idempotent. The `.lumina/` cache is gitignored (rebuilt per run) and stays
/// **empty of previews**: the cells must be painted by the production
/// thumbnail worker from a real CR3 decode, otherwise a cache hit could keep a
/// broken decode green (GOLDEN-FIXT-31).
fn ensure_fixture() -> Vec<StagedEntry> {
    use lumina_sidecar::{SidecarDocument, StackMembership};

    let root = Path::new(FIXTURE_DIR);
    std::fs::create_dir_all(root).expect("create stack fixture dir");
    let _ = std::fs::remove_dir_all(root.join(".lumina"));
    let members: Vec<String> = STACK_MEMBERS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let section = StackMembership::new(
        StackMembership::stack_id_for_members(&members),
        STACK_COVER,
        members,
    )
    .expect("valid stack section");

    let mut entries: Vec<StagedEntry> = Vec::new();
    for &(name, source) in FIXTURE_FILES {
        entries.push((stage_raw(root, name, source), source));
        let mut document =
            SidecarDocument::new(staged_source_identity(name, source), "raster-mvp-1");
        if STACK_MEMBERS.contains(&name) {
            document.stack = Some(section.clone());
        }
        let sidecar = lumina_sidecar::sidecar_path_for(&root.join(name));
        lumina_sidecar::save_sidecar(&sidecar, &document).expect("seed stack sidecar");
    }
    prepare_folder_cache(root);
    entries
}

/// Non-vacuous pixel guard: the rendered frame contains the exact amber
/// membership color. A fixture without stack membership paints no amber, so the
/// count stays 0.
fn assert_stack_amber_painted(harness: &mut Harness<'_, LuminaApp>) {
    let rendered = harness.render().expect("kittest renders the frame");
    let [r, g, b] = STACK_AMBER;
    let count = rendered
        .pixels()
        .filter(|pixel| {
            let [pr, pg, pb, _a] = pixel.0;
            pr == r && pg == g && pb == b
        })
        .count();
    assert!(
        count >= 100,
        "stacked cells must paint the amber membership color; got {count} exact-amber pixels"
    );
}

/// R5-STACKVIS-21: with the whole stack selected, the grid paints the amber
/// membership sign **and** the blue selection frame on the same cells, and the
/// unstacked neighbor carries neither.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_library_stack -- --ignored"]
fn library_stack_membership() {
    let entries = ensure_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    harness.state_mut().set_directory(FIXTURE_DIR.to_owned());
    settle_scan(&mut harness);

    // Select the stack cover: the stack-as-unit rule selects both members
    // (LRPAR-G15-STACK-15), so both cells show the blue frame next to their
    // amber membership sign.
    let cover = format!("{FIXTURE_DIR}/{STACK_COVER}");
    harness
        .state_mut()
        .select_filmstrip_path(cover, false, false);
    assert_eq!(
        harness.state().filmstrip_selection().len(),
        2,
        "selecting the cover must select the whole stack (unit selection)"
    );
    // The fixture really carries Sidecar-first membership (not a paint-only
    // story): both members' sidecars hold the same stack section.
    for name in STACK_MEMBERS {
        let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(
            &Path::new(FIXTURE_DIR).join(name),
        ))
        .expect("stack member sidecar loads");
        assert!(
            document.stack.is_some(),
            "{name} must carry the persisted stack membership"
        );
    }

    // Wait for the real CR3 thumbnails instead of a fixed frame budget.
    settle_thumbnails(&mut harness, &entries);
    assert_no_raw_decode_failure(&mut harness);
    assert_stack_amber_painted(&mut harness);
    harness.snapshot("library_stack_membership");
}
