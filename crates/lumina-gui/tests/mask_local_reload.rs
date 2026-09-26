//! GUI-INT-MASKLOCAL-38: the **reload leg** of DoD §1's chain for the
//! mask-local editors — `Edit → Commit/Debounce → Sidecar-Datei → Reload →
//! Wert wiederhergestellt`.
//!
//! # Why this is its own target
//!
//! The three editor tests in `mask_local_editors.rs` stop one link short: they
//! prove the debounced commit wrote the *bytes*, which is not the same claim as
//! "a reopened project shows the value again". A second `LuminaApp` over the
//! same tempdir is a different claim with a different failure mode (the reload
//! path in `finish_decode`, not the input path), and keeping it separate keeps
//! `mask_local_editors.rs` inside the 500-line rule.
//!
//! # What must happen
//!
//! The **production** reload: `open_file` → decode → `finish_decode` validates
//! the sidecar against the decoded bytes, resolves the virtual copy *by
//! identity* (never positionally) and selects the persisted mask layer.
//! Nothing here seeds a second mask layer — a reloaded value that came from a
//! freshly created layer would prove nothing.
//!
//! The chain is walked over **both** input kinds, so a reload path that only
//! restores what a drag wrote would still be caught: a dragged scalar
//! (vibrance) and a clicked button (a point-colour entry) must both come back,
//! and the reloaded layer must accept a *new* click on top of them.

mod mask_local_editors_support;
mod mask_local_slider_support;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module};
use mask_local_editors_support::*;
use mask_local_slider_support::*;

/// Re-open the *same* directory in a **fresh** `LuminaApp`, without seeding a
/// second mask layer.
///
/// `open_masking_panel` is deliberately not reused: it calls `create_mask`,
/// which would add another layer and make "the value came back" meaningless.
fn reopen_over_existing_source(dir: &tempfile::TempDir) -> Harness<'static, LuminaApp> {
    let mut harness = Harness::builder()
        .with_size(PANEL_VIEWPORT)
        .with_step_dt(STEP_DT)
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()));
    harness.state_mut().set_module(Module::Develop);
    let source = smoke_png(dir);
    harness.state_mut().open_file(source.display().to_string());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while harness.state().decode_pending() {
        assert!(
            std::time::Instant::now() < deadline,
            "the re-opened source did not decode within the bounded deadline"
        );
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    for index in 0..lumina_gui::SECTION_COUNT {
        let open = index == lumina_gui::SECTION_MASKING;
        harness.state_mut().set_section_open(index, open);
    }
    settle_persisted(&mut harness);
    harness
}

/// A dragged scalar and a clicked entry both survive the round trip through the
/// sidecar into a **second** `LuminaApp`, and that layer is editable again.
#[test]
fn a_persisted_mask_local_edit_is_restored_in_a_reopened_project() {
    let dir = tempfile::tempdir().unwrap();
    let (vibrance, saturation, entry_id) = {
        let mut harness = open_masking_panel(&dir);
        // 1) A drag: the vibrance slider of the local colour block.
        let rail = slider_rail(&harness, ("HSL / Color Mixer", 0), Row::Below, "Vibrance");
        drag_slider(&mut harness, rail, 0.8);
        let rail = slider_rail(&harness, ("Vibrance", 0), Row::Below, "Saturation");
        drag_slider(&mut harness, rail, 0.2);
        let (vibrance, saturation) = harness.state().selected_mask_local_vibrance().unwrap();
        assert!(
            vibrance > 0.5 && saturation < -0.5,
            "the two drags must have changed both values, got {vibrance}/{saturation}"
        );
        // 2) A click: the point-colour entry the "Add color" button creates.
        let target = only_rect(&harness, "Add color");
        click(&mut harness, target);
        let entries = harness.state().selected_mask_local_point_color().unwrap();
        assert_eq!(
            entries.len(),
            1,
            "Add color must create one entry: {entries:?}"
        );
        // 3) The middle link of the chain, read from the file itself.
        settle_persisted(&mut harness);
        let on_disk = persisted_local_recipe(&dir);
        assert!(
            close(on_disk.vibrance, vibrance) && close(on_disk.saturation, saturation),
            "the file must hold the dragged values before the reopen: \
             {}/{}, in memory {vibrance}/{saturation}",
            on_disk.vibrance,
            on_disk.saturation
        );
        let entry_id = entries[0].id.clone();
        assert_eq!(
            on_disk
                .point_color
                .as_ref()
                .map(|block| block.entries.len()),
            Some(1),
            "the file must hold the clicked entry before the reopen"
        );
        (vibrance, saturation, entry_id)
    };
    // The first harness is dropped here: the second app must read every value
    // from the file, not from surviving session state.
    let mut reopened = reopen_over_existing_source(&dir);

    // 4) Reload: the dragged pair is back …
    let (vibrance_back, saturation_back) = reopened.state().selected_mask_local_vibrance().unwrap();
    assert!(
        close(vibrance_back, vibrance) && close(saturation_back, saturation),
        "the reloaded vibrance pair must match what was saved: \
         {vibrance_back}/{saturation_back} vs {vibrance}/{saturation}"
    );
    assert!(
        reopened.state().has_mask_local_color().unwrap(),
        "the reloaded layer must still report a pixel-changing local block"
    );
    // … and so is the clicked entry, by its stable id.
    let entries = reopened.state().selected_mask_local_point_color().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "the clicked entry must come back: {entries:?}"
    );
    assert_eq!(entries[0].id, entry_id, "the entry must keep its stable id");

    // 5) The restored layer is *editable*, not merely present: the panel paints
    //    the reloaded entry's own Remove button, and clicking it writes through.
    //
    //    The `all local color reset` lookup below is a **layout** precondition,
    //    not an edit: it asserts the point-colour block is painted exactly once
    //    on the reopened panel. Without it, a `Remove` lookup could match
    //    against a panel that has not drawn the block yet and pass for the
    //    wrong reason. `only_rect` returns a rect, so the value is discarded
    //    deliberately — the assertion lives inside the call.
    only_rect(&reopened, "all local color reset");
    let target = only_rect(&reopened, "Remove");
    click(&mut reopened, target);
    assert!(
        reopened
            .state()
            .selected_mask_local_point_color()
            .unwrap()
            .is_empty(),
        "the reopened panel must accept a new click on the reloaded layer"
    );
    assert!(
        close(
            reopened.state().selected_mask_local_vibrance().unwrap().0,
            vibrance
        ),
        "removing the entry must not disturb the reloaded vibrance value"
    );
}
