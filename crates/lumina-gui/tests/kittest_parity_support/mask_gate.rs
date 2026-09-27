//! GPU-PARITY-MASKGATE-1 end-to-end cells: the GPU-present mask gate measured at
//! **really presented** frames (Metal adapter, `#[ignore]`d).
//!
//! The headless state tests in `src/tests/gpu_mask_gate.rs` pin the *routing
//! decision*; these cells pin the *pixels the user sees*, which is the question
//! the gate actually answers: *may the readback-free VRAM present replace the
//! CPU upload for this frame?*
//!
//! Method: the same state script runs on two harnesses — one with the shared
//! wgpu render state (so the VRAM present path is live) and one without (so
//! `gpu_present_if_ready` refuses at `wgpu_render_state` and the historical CPU
//! upload presents). Both windows are rendered and the **photo rect** region is
//! compared byte-for-byte. The photo rect is the region a mask matte can reach;
//! the routing badge lives outside it and is deliberately excluded, so the
//! comparison measures the image and nothing else.
//!
//! Cells:
//!
//! * [`mask_gate_states_stay_pixel_equal`] — the two states this task owns: the
//!   maskless default (VRAM route, pixel-equal) and the divergent state (a
//!   prompted mask in the document, none selected → CPU route).
//! * [`mask_gate_stale_pooled_mask_plane_is_an_open_finding`] — the guard for
//!   the measured stale-pooled-plane defect: after deleting the last mask the
//!   pooled VRAM mask texture must not still hold that mask's coverage. Read
//!   its module comment for the before/after measurements.
//!
//! No cell writes a golden (`harness.snapshot` is deliberately not called): the
//! frames are compared in memory, so `GOLDEN-BASELINE-32` remains the only
//! place that touches `tests/snapshots/`.

use super::{max_abs_diff, parity_harness, scene_source_png, SKIP_MESSAGE, SRC_H, SRC_W};
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module};
use lumina_sidecar::{BrushMark, BrushMarkSign};

/// A rendered window reduced to the photo rect: the pixels a mask matte can
/// reach. The tuple carries the bytes and their count.
type PhotoRegion = (Vec<u8>, usize);

/// Number of differing bytes between two equal-length byte runs.
fn differing_bytes(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len(), "compared runs must have equal length");
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// Render one frame and copy the photo rect out of it. Comparing the whole
/// window would fold in the routing badge (which legitimately differs between
/// the routes); the photo rect is the image the gate is about.
fn photo_region(harness: &mut Harness<'_, LuminaApp>) -> PhotoRegion {
    // Read the rect from the last completed frame (the same order as the
    // matrix's `assert_preview_pixels_present`) and render the frame that
    // belongs to it.
    let rect = harness
        .state()
        .preview_screen_rect()
        .expect("preview painted before the photo-region readback");
    let image = harness.render().expect("kittest renders the frame");
    let (width, height) = image.dimensions();
    let min_x = (rect.min.x.max(0.0).floor() as u32).min(width);
    let max_x = (rect.max.x.ceil() as u32).min(width);
    let min_y = (rect.min.y.max(0.0).floor() as u32).min(height);
    let max_y = (rect.max.y.ceil() as u32).min(height);
    assert!(
        max_x > min_x && max_y > min_y,
        "the photo rect {rect:?} must lie inside the {width}x{height} window"
    );
    let mut pixels = Vec::with_capacity(((max_x - min_x) * (max_y - min_y) * 4) as usize);
    for y in min_y..max_y {
        for x in min_x..max_x {
            pixels.extend_from_slice(&image.get_pixel(x, y).0);
        }
    }
    let bytes = pixels.len();
    (pixels, bytes)
}

/// A headless harness **without** a wgpu render state: `gpu_present_if_ready`
/// then refuses at `wgpu_render_state`, so the historical CPU upload presents.
/// This is the reference the VRAM route has to be pixel-equal to. The wgpu
/// *renderer* stays on, so both windows are painted by the same backend and the
/// comparison isolates the present path.
fn cpu_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// The maskless default Develop state: the shipped state after loading a source.
fn default_state(app: &mut LuminaApp) {
    app.set_module(Module::Develop);
    app.load_bytes(scene_source_png(), "parity_source.png")
        .expect("scene source loads");
    app.render().expect("default render");
}

/// The divergent state: two brushed masks, the first one selected, then the
/// selected one deleted. A mask with a prompt stays in the document while no
/// mask is selected any more — the state a normal delete produces. Returns the
/// id of the mask that is still present.
fn divergent_state(app: &mut LuminaApp) -> String {
    app.set_module(Module::Develop);
    app.load_bytes(scene_source_png(), "parity_source.png")
        .expect("scene source loads");
    let first = app.create_mask("First").expect("first mask");
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.3,
        y: 0.5,
        radius: 0.35,
        sign: BrushMarkSign::Positive,
        softness: 0.2,
        flow: 0.9,
    }])
    .expect("first brush stroke");
    let remaining = app.create_mask("Second").expect("second mask");
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.7,
        y: 0.5,
        radius: 0.35,
        sign: BrushMarkSign::Positive,
        softness: 0.2,
        flow: 0.9,
    }])
    .expect("second brush stroke");
    // `create_mask` selects the newest mask; the *first* one is the one this
    // cell deletes, so it must be selected explicitly.
    app.select_mask(&first).expect("select the first mask");
    app.set_section_open(lumina_gui::SECTION_MASKING, true);
    app.render().expect("two-mask render");
    app.delete_mask(&first).expect("delete the selected mask");
    app.render().expect("post-delete render");
    remaining
}

/// Settle both harnesses on the VRAM route: prime the VRAM result, run two
/// frames, and report the routing decision the gate made.
fn settle(
    label: &str,
    vram: &mut Harness<'_, LuminaApp>,
    cpu: &mut Harness<'_, LuminaApp>,
) -> Option<[usize; 2]> {
    assert!(
        vram.state_mut().prime_gpu_present(),
        "an available adapter must render the {label} VRAM result"
    );
    vram.run();
    vram.run();
    cpu.run();
    cpu.run();
    let present = vram.state().gpu_present_frame_size();
    eprintln!("maskgate[{label}]: vram_present={present:?}");
    present
}

/// Compare the presented photo of both routes and return
/// (`maxAbsDiff`, differing bytes, total bytes).
fn compare(
    label: &str,
    vram: &mut Harness<'_, LuminaApp>,
    cpu: &mut Harness<'_, LuminaApp>,
) -> (u8, usize, usize) {
    let (vram_pixels, bytes) = photo_region(vram);
    let (cpu_pixels, cpu_bytes) = photo_region(cpu);
    assert_eq!(bytes, cpu_bytes, "both photo rects must have equal size");
    let diff = max_abs_diff(&vram_pixels, &cpu_pixels);
    let differing = differing_bytes(&vram_pixels, &cpu_pixels);
    eprintln!(
        "maskgate[{label}]: maxAbsDiff={diff} differingBytes={differing} of {bytes} \
         (source {SRC_W}x{SRC_H})"
    );
    (diff, differing, bytes)
}

/// GPU-PARITY-MASKGATE-1 at the pixels: the maskless default takes the
/// readback-free path and is pixel-equal there, and the divergent state (a
/// prompted mask in the document, none selected) stays on the CPU path.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
fn mask_gate_states_stay_pixel_equal() {
    let mut vram = parity_harness();
    if !vram.state_mut().gpu_adapter_available() {
        eprintln!("{SKIP_MESSAGE} (mask-gate cell not executed: no adapter to present)");
        return;
    }
    let mut cpu = cpu_harness();

    // ---- 1) Control: the maskless default Develop state ----
    default_state(vram.state_mut());
    default_state(cpu.state_mut());
    let control_present = settle("control  ", &mut vram, &mut cpu);
    let (control_diff, control_differing, control_bytes) =
        compare("control  ", &mut vram, &mut cpu);
    assert!(
        control_present.is_some(),
        "GPU-PARITY-MASKGATE-1 case 1: the maskless default state must present \
         from VRAM (a permanent CPU route would be an off switch, not a split)"
    );
    assert_eq!(
        (control_diff, control_differing),
        (0, 0),
        "GPU-PARITY-MASKGATE-1 case 1: the maskless default must be pixel-equal \
         on both present paths (measured maxAbsDiff={control_diff} over \
         {control_bytes} presented photo bytes)"
    );

    // ---- 2) Diverging: a mask with a prompt in the document, none selected ----
    divergent_state(vram.state_mut());
    divergent_state(cpu.state_mut());
    let diverging_present = settle("diverging", &mut vram, &mut cpu);
    let (diverging_diff, diverging_differing, diverging_bytes) =
        compare("diverging", &mut vram, &mut cpu);
    assert!(
        diverging_present.is_none(),
        "GPU-PARITY-MASKGATE-1: a frame with an evaluated mask layer that the \
         CPU painter would not draw must stay on the CPU present path \
         (presented from VRAM as {diverging_present:?}; measured \
         maxAbsDiff={diverging_diff} over {diverging_differing} of \
         {diverging_bytes} presented photo bytes)"
    );
}

/// The regression guard for the stale-pooled-plane defect: deleting the **last**
/// mask must leave no coverage in the pooled VRAM mask texture.
///
/// The defect itself was real and measured. `lumina_gpu`'s pool keeps one entry
/// per `(width, height)` and `get_or_create` re-activates a reactivated entry
/// by only touching the LRU, and the old upload path returned early when no
/// layer was evaluated. Deleting the *last* mask therefore left its coverage in
/// the entry while the frame carried no evaluated layer — the exact state of
/// GPU-PARITY-MASKGATE-1 case 1, so the readback-free VRAM path was allowed and
/// presented a stale artifact as the current frame, with no way for the user to
/// switch it off (`mask_overlay_allowed` demands the selection that no longer
/// exists). Measured on this machine (Metal adapter) before the repair:
/// `vram_present=Some([160, 120])`, `maxAbsDiff=67` over 127 707 of 603 904
/// presented photo bytes.
///
/// The repair is at the production path, GUI-side: `present_mask_plane.rs`
/// writes zeros over the active entry's mask plane on every render whose frame
/// carries no coverage (`MaskPlaneIntent::Clear`), so the SOLL condition "no
/// stale mask plane resident" holds by construction instead of being tracked by
/// a flag. After the repair the same script measures `vram_present=Some([160,
/// 120])` with `maxAbsDiff=0 differingBytes=0 of 603904` — the VRAM route is
/// still taken (a `return false` fix would also measure 0/0 but would restore
/// the reachability regression; `mask_gate_states_stay_pixel_equal` guards that
/// half).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_parity -- --ignored"]
fn mask_gate_stale_pooled_mask_plane_is_an_open_finding() {
    let mut vram = parity_harness();
    if !vram.state_mut().gpu_adapter_available() {
        eprintln!("{SKIP_MESSAGE} (stale-plane guard not executed: no adapter)");
        return;
    }
    let mut cpu = cpu_harness();

    let remaining = divergent_state(vram.state_mut());
    divergent_state(cpu.state_mut());
    settle("stale-seed", &mut vram, &mut cpu);
    for harness in [&mut vram, &mut cpu] {
        let app = harness.state_mut();
        app.delete_mask(&remaining)
            .expect("delete the remaining mask");
        app.render().expect("mask-free render");
    }
    let present = settle("stale-plane", &mut vram, &mut cpu);
    let (diff, differing, bytes) = compare("stale-plane", &mut vram, &mut cpu);
    // Only an actual deviation is worth a finding line; a green run must not
    // print one (a log reader must never have to guess which case it was).
    if diff != 0 || differing != 0 {
        eprintln!(
            "maskgate[stale-plane]: FINDING — the frame carries no evaluated \
             layer (vram_present={present:?}) yet the presented photo differs by \
             maxAbsDiff={diff} over {differing} of {bytes} bytes: the pooled VRAM \
             mask texture still holds the deleted mask's coverage"
        );
    }
    assert_eq!(
        (diff, differing),
        (0, 0),
        "GPU-PARITY-MASKGATE-1 follow-up: deleting the last mask must not leave \
         leftover coverage in the pooled VRAM mask texture (vram_present={present:?})"
    );
}
