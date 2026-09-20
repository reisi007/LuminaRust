//! LRPAR-G03-MASKGROUP-03: GUI model + persistence + panel tests
//! (Copy vs. Duplicate, propagation, late grouping, group actions, source
//! deletion materialization with history, reload, loud refusals).

use super::*;

fn group_of(app: &LuminaApp, group_id: &str) -> lumina_sidecar::MaskGroup {
    app.mask_groups()
        .unwrap()
        .into_iter()
        .find(|group| group.id == group_id)
        .expect("group exists")
}

/// E2E (DoD §1): Copy (deep, independent) vs. Duplicate (group pointer).
/// Changing the source propagates to the group member but not to the copy;
/// both survive a sidecar reload.
#[test]
fn g03_group_copy_vs_duplicate_propagates_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    let a = app.create_luminance_range_mask(0.0, 1.0, 0.0, "A").unwrap();
    // Copy = deep independent node.
    let copy = app.duplicate_mask(&a, "A copy").unwrap();
    // Duplicate = group with a pointer member on A.
    let group_id = app.group_duplicate_mask(&a, "A group").unwrap();
    let group = group_of(&app, &group_id);
    assert_eq!(group.members.len(), 1);
    assert_eq!(group.members[0].mask_id, a, "Duplicate must point at A");

    // Source change propagates to the member pointer, not to the deep copy.
    app.rename_mask(&a, "A renamed").unwrap();
    let member_id = group_of(&app, &group_id).members[0].mask_id.clone();
    let document = app.document.as_ref().unwrap();
    let library = &document.virtual_copies[0].mask_library;
    assert_eq!(
        library.iter().find(|m| m.id == member_id).unwrap().name,
        "A renamed",
        "the group member must follow the source (no silent decoupling)"
    );
    assert_eq!(
        library.iter().find(|m| m.id == copy).unwrap().name,
        "A copy",
        "the deep copy must stay independent"
    );

    // Reload: the group and its member pointer survive.
    let reopened = reopen_app(&source);
    let reloaded = group_of(&reopened, &group_id);
    assert_eq!(reloaded.name, "A group");
    assert_eq!(reloaded.members[0].mask_id, a);
    assert!(reopened.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .iter()
        .any(|m| m.id == copy));
}

/// E2E: late grouping + group actions (select, activate/deactivate, reorder,
/// shared offsets, collapse, ungroup) persist across a reload; members are kept
/// on ungroup.
#[test]
fn g03_group_actions_persist_and_keep_members() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    let a = app.create_luminance_range_mask(0.0, 0.4, 0.0, "A").unwrap();
    let b = app.create_luminance_range_mask(0.6, 1.0, 0.0, "B").unwrap();
    // Late grouping via the session member selection.
    app.toggle_group_member_selection(&a);
    app.toggle_group_member_selection(&b);
    assert!(app.group_member_selected(&a) && app.group_member_selected(&b));
    let group_id = app.create_mask_group("G", &[a.clone(), b.clone()]).unwrap();
    // The selection is cleared after grouping.
    assert!(!app.group_member_selected(&a));

    // Collapse persists.
    app.set_mask_group_collapsed(&group_id, true).unwrap();
    assert!(group_of(&app, &group_id).collapsed);

    // Reorder: move B before A.
    app.move_mask_group_member(&group_id, &b, -1).unwrap();
    let order: Vec<String> = group_of(&app, &group_id)
        .members
        .iter()
        .map(|m| m.mask_id.clone())
        .collect();
    assert_eq!(order, vec![b.clone(), a.clone()]);

    // Deactivate the unit: both member masks become invisible.
    app.set_mask_group_visible(&group_id, false).unwrap();
    assert!(!app.mask_visible(&a) && !app.mask_visible(&b));
    app.set_mask_group_visible(&group_id, true).unwrap();
    assert!(app.mask_visible(&a) && app.mask_visible(&b));

    // Shared offsets touch both member layers (clamped).
    let touched = app.adjust_mask_group_offsets(&group_id, 0.5, 0.0).unwrap();
    assert_eq!(touched, 2);
    let layers = &app.document.as_ref().unwrap().virtual_copies[0].mask_layers;
    assert!(layers.iter().all(|layer| layer.feather >= 0.5));

    // Reload: group, collapse state, order survive.
    let reopened = reopen_app(&source);
    let reloaded = group_of(&reopened, &group_id);
    assert!(reloaded.collapsed);
    let order: Vec<String> = reloaded.members.iter().map(|m| m.mask_id.clone()).collect();
    assert_eq!(order, vec![b, a]);

    // Ungroup dissolves the container but keeps both masks.
    let mut reopened = reopened;
    reopened.remove_mask_group(&group_id).unwrap();
    assert!(reopened.mask_groups().unwrap().is_empty());
    assert_eq!(
        reopened.document.as_ref().unwrap().virtual_copies[0]
            .mask_library
            .len(),
        2
    );
}

/// Source deletion materializes one frozen copy, re-points the group member and
/// the layer, records a history entry and survives a reload.
#[test]
fn g03_group_source_deletion_materializes_with_history_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    let a = app.create_luminance_range_mask(0.0, 1.0, 0.0, "A").unwrap();
    let group_id = app.group_duplicate_mask(&a, "A group").unwrap();
    app.delete_mask(&a).unwrap();

    // The source is gone; a frozen copy exists and the member points at it.
    let document = app.document.as_ref().unwrap();
    let copy = &document.virtual_copies[0];
    assert!(!copy.mask_library.iter().any(|m| m.id == a));
    let frozen = group_of(&app, &group_id).members[0].mask_id.clone();
    assert_ne!(frozen, a);
    assert!(copy.mask_library.iter().any(|m| m.id == frozen));
    // The referencing layer follows the frozen copy (no dangling reference).
    assert!(copy.mask_layers.iter().all(|l| l.mask.mask_id == frozen));
    // History records the loud materialization.
    let entry = copy
        .history
        .iter()
        .find(|entry| {
            entry.extras.get("action").and_then(Value::as_str) == Some("mask.group.materialize")
        })
        .expect("materialization history entry");
    assert!(entry.extras.contains_key("frozen"));
    assert!(document.validate().is_ok());

    // Reload: the group member still points at the persisted frozen copy.
    let reopened = reopen_app(&source);
    let reloaded = group_of(&reopened, &group_id);
    assert_eq!(reloaded.members[0].mask_id, frozen);
    assert!(reopened.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .iter()
        .any(|m| m.id == frozen));
    assert!(reopened.document.as_ref().unwrap().validate().is_ok());
}

/// Loud refusals: unknown masks, empty/duplicate grouping, NaN offsets and
/// unknown groups leave the document valid (no half-written state).
#[test]
fn g03_group_refusals_are_loud_and_leave_document_valid() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    let a = app.create_luminance_range_mask(0.0, 1.0, 0.0, "A").unwrap();

    assert!(app.group_duplicate_mask("missing", "G").is_err());
    assert!(app.create_mask_group("G", &[]).is_err());
    assert!(app
        .create_mask_group("  ", std::slice::from_ref(&a))
        .is_err());

    let group_id = app.group_duplicate_mask(&a, "G").unwrap();
    // A node already in a group cannot join a second one.
    assert!(app
        .create_mask_group("H", std::slice::from_ref(&a))
        .is_err());
    assert!(app
        .adjust_mask_group_offsets(&group_id, f32::NAN, 0.0)
        .is_err());
    assert!(app.adjust_mask_group_offsets("missing", 0.0, 0.0).is_err());
    assert!(app.remove_mask_group("missing").is_err());
    assert!(app.delete_mask("missing").is_err());
    assert!(app.select_mask_group("missing").is_err());

    // The single valid group is untouched and the document still validates.
    assert_eq!(app.mask_groups().unwrap().len(), 1);
    assert!(app.document.as_ref().unwrap().validate().is_ok());
}

/// Klickbarkeit (DoD §5): the Duplicate (group) and Group selected buttons are
/// wired to the tested model methods, not just painted.
#[test]
fn g03_group_buttons_are_clickable() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.ensure_document_loaded().unwrap();
    let a = app
        .create_luminance_range_mask(0.0, 1.0, 0.0, "Full")
        .unwrap();
    app.duplicate_name_input = "G".into();

    let draw = |app: &mut LuminaApp, ui: &mut egui::Ui| {
        let document = app.document.clone().expect("document loaded");
        app.draw_masking_g03(ui, &document);
    };
    headless_click_labels(&mut app, &[Str::DuplicateGroup.t()], draw);
    assert_eq!(
        app.mask_groups().unwrap().len(),
        1,
        "clicking Duplicate (group) must create a group"
    );

    // Group selected via the session member selection.
    app.toggle_group_member_selection(&a);
    app.group_name_input = "H".into();
    let draw = |app: &mut LuminaApp, ui: &mut egui::Ui| {
        let document = app.document.clone().expect("document loaded");
        app.draw_masking_g03(ui, &document);
    };
    headless_click_labels(&mut app, &[Str::GroupSelected.t()], draw);
    // `a` already belongs to the first group, so the late grouping is refused
    // loudly and no second group is created.
    assert_eq!(app.mask_groups().unwrap().len(), 1);
}

/// Panel-Präsenz headless (DoD §5): the group panel paints its controls inside
/// the 320px budget, including a selected group's member actions and offsets.
#[test]
fn g03_group_panel_paints_controls_inside_panel() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.ensure_document_loaded().unwrap();
    let a = app
        .create_luminance_range_mask(0.0, 1.0, 0.0, "Full")
        .unwrap();
    let group_id = app.group_duplicate_mask(&a, "Group one").unwrap();
    app.select_mask_group(&group_id).unwrap();

    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 2400.0));
    let mut panel_rect = egui::Rect::NOTHING;
    let mut shapes = Vec::new();
    for i in 0..2 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(i as f64 / 60.0),
                ..Default::default()
            },
            |ui| {
                let r = egui::Panel::right("controls")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| {
                        let document = app.document.clone().expect("document loaded");
                        app.draw_masking_g03(ui, &document);
                    });
                panel_rect = r.response.rect;
            },
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    for needle in [
        Str::MaskGroupsLabel.t(),
        Str::GroupMembersLabel.t(),
        Str::GroupSelected.t(),
        Str::DuplicateGroup.t(),
        Str::GroupActive.t(),
        Str::Ungroup.t(),
        Str::GroupApplyOffsets.t(),
        Str::DeleteMaskButton.t(),
    ] {
        assert_fully_visible(&shapes, needle);
    }
    assert!(
        text_contains(&shapes, "Group one"),
        "the collapsible group header must paint its name"
    );
    assert!(
        panel_rect.width() <= 321.0,
        "group rows must not push the panel past 320px (got {panel_rect:?})"
    );
}
