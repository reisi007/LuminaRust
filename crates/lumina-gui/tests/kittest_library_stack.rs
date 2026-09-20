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
use lumina_core::cache::{disk::DiskFolderCache, PreviewKind};
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{LuminaApp, Module};
use std::collections::BTreeMap;
use std::path::Path;

/// Fixed, committed fixture directory (relative, so no tempdir randomness can
/// leak into folder-tree / path-field pixels — same rationale as
/// `kittest_snapshots::LIBRARY_FIXTURE_DIR`).
///
/// The sentinel files sit one level deeper (`…/images/`) on purpose: the
/// Library folder tree counts RAW files depth-limited to `FOLDER_SCAN_DEPTH`
/// (= 3) below a node, so a fixture at `tests/fixtures/library_stack/*.arw`
/// would bump the un-pinned `library_people_empty` golden's `tests (7)` counter
/// to `tests (10)`. At `…/library_stack/images/*.arw` the counter stays stable
/// and that unrelated golden needs no rebaseline (R5-STACKVIS-21).
const FIXTURE_DIR: &str = "tests/fixtures/library_stack/images";

/// RAW sentinels with the base color of their seeded Standard preview. The
/// first two are the stack members (adjacent in name order), the third is the
/// unstacked neighbor that must carry neither mark.
const FIXTURE_FILES: &[(&str, [u8; 3])] = &[
    ("a_stack1.arw", [200, 60, 50]),
    ("b_stack2.arw", [60, 170, 80]),
    ("z_solo.arw", [70, 110, 200]),
];

/// The two stack members, cover first.
const STACK_MEMBERS: [&str; 2] = ["a_stack1.arw", "b_stack2.arw"];
const STACK_COVER: &str = "a_stack1.arw";

/// Amber membership color, mirrored from the production constant on purpose:
/// the golden's pixel guard must not reuse the value it verifies.
const STACK_AMBER: [u8; 3] = [0xE8, 0xA9, 0x1C];

/// Seeded preview dimensions (same as the other Library goldens so the cells
/// render real, distinct thumbnails).
const PREVIEW_SIZE: (u32, u32) = (288, 192);

fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Drive the async folder scan (and the auto-load decode it starts) to settle
/// before snapshotting — copied from `kittest_snapshots_support::settle_scan`.
fn settle_scan(harness: &mut Harness<'_, LuminaApp>) {
    for _ in 0..500 {
        harness.step();
        if !harness.state().scan_pending() && !harness.state().decode_pending() {
            harness.step();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("folder scan/decode did not settle within the bounded frame budget");
}

/// Deterministic preview pixels: vertical gradient around `base` (same helper
/// shape as `kittest_snapshots::library_views_preview_png`).
fn preview_png(base: [u8; 3]) -> Vec<u8> {
    let (width, height) = PREVIEW_SIZE;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let factor = 192 + ((y * 63) / height.max(1));
        for _ in 0..width {
            for channel in base {
                pixels.push(((u32::from(channel) * factor) / 255) as u8);
            }
            pixels.push(255);
        }
    }
    ImageFrame::new(width, height, pixels)
        .expect("fixture frame")
        .encode(ImageFileFormat::Png)
        .expect("fixture preview encodes")
}

/// (Re-)write the sentinel RAWs, seed their Standard previews and write the
/// Sidecar-first stack membership (`SidecarDocument.stack`) into both members.
/// Idempotent. The `.lumina/` cache is gitignored (rebuilt per run), so only
/// the sentinel `.arw` files are committed.
fn ensure_fixture() {
    use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SidecarDocument, SourceIdentity};

    let root = Path::new(FIXTURE_DIR);
    std::fs::create_dir_all(root).expect("create stack fixture dir");
    let _ = std::fs::remove_dir_all(root.join(".lumina"));
    let bytes = b"lumina-raw-fixture";
    let content_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
    let members: Vec<String> = STACK_MEMBERS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let section = lumina_sidecar::StackMembership::new(
        lumina_sidecar::StackMembership::stack_id_for_members(&members),
        STACK_COVER,
        members,
    )
    .expect("valid stack section");

    for &(name, base) in FIXTURE_FILES {
        std::fs::write(root.join(name), bytes).expect("write stack fixture");
        let png = preview_png(base);
        let cache = DiskFolderCache::for_image(root.join(name)).expect("stack fixture cache");
        assert!(
            cache
                .store_preview(name, "vc-original", PreviewKind::Standard, &png)
                .expect("seed stack preview"),
            "Standard previews must be enabled for {name}"
        );
        let identity = SourceIdentity {
            relative_name: name.to_owned(),
            content_hash: content_hash.clone(),
            byte_length: bytes.len() as u64,
            modified_at: None,
            raw_format: "ARW".to_owned(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "kittest".to_owned(),
                version: "1".to_owned(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 2,
                height: 2,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            extras: BTreeMap::new(),
        };
        let mut document = SidecarDocument::new(identity, "raster-mvp-1");
        if STACK_MEMBERS.contains(&name) {
            document.stack = Some(section.clone());
        }
        let sidecar = lumina_sidecar::sidecar_path_for(&root.join(name));
        lumina_sidecar::save_sidecar(&sidecar, &document).expect("seed stack sidecar");
    }
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
    ensure_fixture();
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

    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    assert_stack_amber_painted(&mut harness);
    harness.snapshot("library_stack_membership");
}
