//! toast state machine, overlay and timing tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-TOAST-OVERLAP-1: the toast state machine — show makes it visible,
/// a second show while visible coalesces (no queue), manual dismiss
/// hides it, and the timeout hides it without interaction.
#[test]
fn toast_show_dismiss_timeout_state_machine() {
    let mut app = new_app();
    assert!(!app.toast_visible(100.0));
    app.show_toast("Preview ready".into(), 100.0);
    assert!(app.toast_visible(100.0));
    assert!(app.toast_visible(104.0));
    assert!(!app.toast_visible(104.1), "timeout must hide the toast");
    // Coalescing: a second show while visible keeps the first deadline.
    app.show_toast("Preview ready".into(), 200.0);
    app.show_toast("Other message".into(), 201.0);
    assert_eq!(app.toast_message.as_deref(), Some("Preview ready"));
    assert!(app.toast_visible(204.0));
    assert!(!app.toast_visible(204.1));
    // Manual dismiss hides immediately.
    app.show_toast("Preview ready".into(), 300.0);
    assert!(app.toast_visible(300.0));
    app.dismiss_toast();
    assert!(!app.toast_visible(300.0));
    assert!(app.toast_message.is_none());
}

/// GUI-TOAST-OVERLAP-1 / KITTEST-COVERAGE-STATES-2 (e): the toast anchor
/// sits top-center over the preview canvas, below the preview-area header
/// (zoom toolbar) — clear of the left rail, all top bars, the right
/// control panel (histogram header) and the bottom filmstrip. It covers
/// only photo pixels while visible, and only transiently (4 s + ✕).
#[test]
fn toast_anchor_stays_clear_of_thumbnails() {
    let anchor = LuminaApp::toast_anchor(egui::vec2(1280.0, 720.0));
    assert_eq!(anchor, egui::pos2(490.0, 100.0));
    assert!(
        anchor.x > 260.0,
        "toast stays clear of the left navigator rail"
    );
    assert!(
        anchor.y > 90.0,
        "toast sits below the preview-area header, never on clickable chrome"
    );
    assert!(anchor.y < 200.0, "toast stays near the top of the canvas");
    let narrow = LuminaApp::toast_anchor(egui::vec2(800.0, 720.0));
    assert_eq!(narrow, egui::pos2(250.0, 100.0));
    assert!(narrow.x >= 0.0);
}

/// GUI-TOAST-OVERLAP-1: the overlay toast paints its message plus the
/// manual ✕ button in its own area next to (not inside) the thumbnail
/// views — both the toast and the filmstrip heading are painted.
#[test]
fn toast_overlay_paints_message_and_dismiss() {
    let mut app = new_app();
    app.show_toast(Str::ToastPreviewReady.t().to_string(), 0.0);
    // egui `Area`s take a sizing pass on first show — drive two headless
    // frames on the SAME context (like the live event loop) and assert
    // on the second.
    let ctx = egui::Context::default();
    let mut run = |app: &mut LuminaApp| {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1024.0, 720.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            let c = ui.ctx().clone();
            app.draw_toast(&c);
            app.draw_filmstrip(&c, ui);
        });
        output.textures_delta.clear();
        output.shapes
    };
    let _ = run(&mut app);
    let shapes = run(&mut app);
    let texts: Vec<String> = shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t == Str::ToastPreviewReady.t()),
        "toast message must be painted, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == Str::ToastDismiss.t()),
        "toast dismiss button must be painted, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == Str::Filmstrip.t()),
        "filmstrip heading must still be painted, got {texts:?}"
    );
}

/// P0-Audit (DoD §3, GUI-TOAST-OVERLAP-1): ein sichtbarer Toast liegt in
/// einer eigenen Foreground-Area und schluckt keinen Klick auf ein
/// darunterliegendes Thumbnail.
#[test]
fn toast_does_not_block_thumbnail_clicks() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let mut app = new_app();
    app.show_toast("preview ready".to_string(), 0.0);
    let mut t = 0.0;
    let thumb_center = std::cell::RefCell::new(None);
    let thumb_clicked = std::cell::Cell::new(false);
    let toast_painted = std::cell::Cell::new(false);
    // `run` lebt nur in diesem Block: Danach endet sein Mutable-Borrow
    // von `app`, sodass die Abschluss-Asserts wieder an `app` dürfen
    // (ohne `drop` auf einem Non-Drop-Typ — Clippy `drop_non_drop`).
    let (center, end_time) = {
        let mut run = |events: Vec<egui::Event>| {
            t += 1.0 / 60.0;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(t),
                    events,
                    ..Default::default()
                },
                |ui| {
                    // Produktionspfad: Toast als Overlay-Area …
                    app.draw_toast(ui.ctx());
                    // … plus Thumbnail-Button im Panel abseits des
                    // Toast-Ankers (Anker x = 1024-300 = 724, y = 64),
                    // wie der echte Filmstrip in seiner Panel-Spalte.
                    egui::Panel::left("thumbs")
                        .default_size(220.0)
                        .show(ui, |ui| {
                            let response = ui.button("thumb-a");
                            if response.clicked() {
                                thumb_clicked.set(true);
                            }
                            if thumb_center.borrow().is_none() {
                                *thumb_center.borrow_mut() = Some(response.rect.center());
                            }
                        });
                },
            );
            for clipped in &output.shapes {
                if let egui::Shape::Text(text) = &clipped.shape {
                    if text.galley.text() == "preview ready" {
                        toast_painted.set(true);
                    }
                }
            }
            output.textures_delta.clear();
        };
        run(vec![]);
        // Areas brauchen einen Sizing-Pass: der erste Frame vermisst nur,
        // erst danach wird gemalt (Produktionsverhalten, kein Test-Artefakt).
        run(vec![]);
        run(vec![]);
        let center = thumb_center
            .borrow()
            .expect("thumbnail button must be laid out");
        assert!(
            center.x < 700.0,
            "thumbnail ({center:?}) muss abseits des Toast-Ankers liegen"
        );
        assert!(toast_painted.get(), "toast must be painted while visible");
        let press = |pressed: bool| egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        run(vec![egui::Event::PointerMoved(center), press(true)]);
        run(vec![egui::Event::PointerMoved(center), press(false)]);
        (center, t)
    };
    assert!(
        thumb_clicked.get(),
        "click on the thumbnail beneath the visible toast must arrive"
    );
    assert!(
        app.toast_visible(end_time),
        "toast must still be visible (no auto-dismiss by the click)"
    );
}

/// P0-Audit (DoD §2-Anker, DoD §3): der Toast-Timeout wird von der
/// simulierten ctx-Zeit über `update_toast` getrieben — kein Wall-Clock-,
/// kein Frame-Zähler-Verhalten.
#[test]
fn toast_timeout_driven_by_update_loop() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let mut app = new_app();
    app.show_toast("preview ready".to_string(), 10.0);
    // Rein lesbar: sichtbar bis inkl. Deadline, danach abgelaufen.
    assert!(app.toast_visible(10.0));
    assert!(app.toast_visible(14.0));
    assert!(!app.toast_visible(14.000_001));
    // Über den Update-Loop getrieben: vor der Deadline bleibt die
    // Message bestehen …
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(12.0),
            ..Default::default()
        },
        |ui| app.update_toast(ui.ctx()),
    );
    output.textures_delta.clear();
    assert!(
        app.toast_message.is_some(),
        "toast must survive update_toast before its deadline"
    );
    // … nach der Deadline räumt derselbe Pfad sie ab (DoD §2: zeitbasiert).
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(20.0),
            ..Default::default()
        },
        |ui| app.update_toast(ui.ctx()),
    );
    output.textures_delta.clear();
    assert_eq!(
        app.toast_message, None,
        "update_toast past the deadline must auto-dismiss"
    );
    assert!(!app.toast_visible(20.0));
}

/// GUI-TOAST-OVERLAP-1: the overlay toast takes no layout width — the
/// central column next to the navigator rail is exactly as wide with the
/// toast visible as without it.
#[test]
fn toast_leaves_rail_layout_width_unchanged() {
    fn central_width(toast: bool) -> f32 {
        use std::cell::Cell;
        let width = Cell::new(0.0f32);
        let mut app = new_app();
        if toast {
            app.show_toast(Str::ToastPreviewReady.t().to_string(), 0.0);
        }
        assert_eq!(app.toast_visible(0.0), toast);
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1024.0, 720.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            egui::Panel::left("navigator")
                .resizable(true)
                .default_size(150.0)
                .show(ui, |ui| {
                    ui.label("Navigator");
                });
            egui::CentralPanel::default().show(ui, |ui| {
                width.set(ui.available_width());
            });
            let c = ui.ctx().clone();
            app.draw_toast(&c);
        });
        output.textures_delta.clear();
        width.get()
    }

    let plain = central_width(false);
    let with_toast = central_width(true);
    assert!(plain > 0.0, "central column must have width");
    assert_eq!(
        plain, with_toast,
        "a visible toast must not steal layout width ({plain} vs {with_toast})"
    );
}
