//! R5-DUST-23: golden snapshots for the Spot-Heal toolbar-near options strip.
//!
//! The Dust-Removal controls moved out of the Develop sidebar into the preview
//! tool strip (armed with the toolbar Heal icon / `Q`). These goldens pin the
//! new placement (primary Size/Feather/Opacity row over the image) and the
//! collapsed "Remove options" group hosting the G-04 extras.
//!
//! Like the other kittest goldens they need a headless wgpu backend and are
//! `#[ignore]`d by default:
//! `UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_spot_tool -- --ignored`
//! (or without `UPDATE_SNAPSHOTS` to compare against the committed goldens).

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module, SpotTool, SECTION_COUNT};

/// Headless harness at the fixed kittest window size (mirrors the shared
/// `kittest_snapshots_support::build_harness`; this target keeps its own copy
/// so the shared module's other helpers stay free of dead-code warnings).
fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Develop module with the sample image and the Heal tool armed, all eight
/// sections closed so the strip is the only new content over the image.
fn armed_harness() -> Harness<'static, LuminaApp> {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    harness
        .state_mut()
        .load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .expect("sample image loads");
    for i in 0..SECTION_COUNT {
        harness.state_mut().set_section_open(i, false);
    }
    harness.state_mut().set_spot_tool(SpotTool::Heal);
    harness.run();
    harness
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_spot_tool -- --ignored"]
fn develop_spot_tool_options() {
    let mut harness = armed_harness();
    harness.snapshot("develop_spot_tool_options");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_spot_tool -- --ignored"]
fn develop_spot_tool_options_expanded() {
    let mut harness = armed_harness();
    let clicked = harness
        .query_all_by_label("Remove options")
        .next()
        .map(|node| {
            node.click_accesskit();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "\"Remove options\" header must be painted");
    harness.run();
    harness.run();
    harness.snapshot("develop_spot_tool_options_expanded");
}

/// R5-DUST-23-FOLLOWUP: golden for the selected removal's detail (type /
/// status / parameter editor, single delete) — the detail renders only when
/// a removal is selected, like the selected mask.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_spot_tool -- --ignored"]
fn develop_spot_selected() {
    let mut harness = armed_harness();
    // A real file (not `load_bytes`) so the committed dab saves its sidecar
    // next to the source instead of opening the error dialog. Fixed,
    // cleaned directory under the gitignored workspace `target/`: the
    // status bar paints the path, so a random tempdir name would make the
    // golden flaky and an absolute path would differ per OS/user;
    // leftovers are removed so a re-run never stacks a duplicate dab onto
    // the previous sidecar. (Integration tests run with the package
    // directory as CWD, hence `../../target`.)
    let directory = std::path::PathBuf::from("../../target/lumina-kittest-spot-selected");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("golden dir");
    let source = directory.join("spot-selected.png");
    std::fs::write(&source, LuminaApp::sample_image_png()).expect("fixture written");
    // Drain until the temp file's decode + render landed (the sample image
    // is already previewed, so `preview().is_some()` alone returns at once;
    // the generation bump proves the new lineage adopted).
    let generation = harness.state_mut().preview_generation();
    harness.state_mut().open_file(source.display().to_string());
    for _ in 0..600 {
        harness.run();
        if harness.state_mut().preview_generation() > generation {
            break;
        }
    }
    assert!(
        harness.state_mut().preview_generation() > generation,
        "temp file must decode and render in the harness"
    );
    harness
        .state_mut()
        .commit_spot_heal(
            lumina_sidecar::Point2 { x: 0.5, y: 0.5 },
            18.0,
            0.5,
            lumina_sidecar::Point2 { x: 0.05, y: 0.0 },
            1.0,
        )
        .expect("demo spot commits");
    // A fresh dab selects itself, so its detail is what the golden pins.
    harness.run();
    harness.run();
    let clicked = harness
        .query_all_by_label("Remove options")
        .next()
        .map(|node| {
            node.click_accesskit();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "\"Remove options\" header must be painted");
    harness.run();
    harness.run();
    harness.snapshot("develop_spot_selected");
}
