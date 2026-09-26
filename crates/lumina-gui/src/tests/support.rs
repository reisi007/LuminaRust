//! Shared headless test helpers (UX-LOOK-TOOLBAR-18 extraction).
//!
//! `lib.rs` is over the 500-line ratchet, so the shared draw/click helpers
//! live here instead of growing the crate root. Each test module pulls them in
//! through `crate::tests`'s `use support::*;` plus its own `use super::*;`.

use super::*;

// ---------------------------------------------------------------------------
// Shared log capture (LENSFUN-CALLER-37).
//
// `log` accepts **one** logger per process, and `set_logger` fails silently
// for the loser. Two test modules each installing their own capture therefore
// race: whichever runs first swallows every record the other one waits for.
// `tests/library_sort/migration.rs` had its own installer, and the Lensfun
// diagnostics tests needed a second one — so there is exactly **one** installer
// here, and both read from it.
//
// Records are bucketed **per thread**: the suite runs multi-threaded, so a
// global vector would mix unrelated tests' records into each other's
// assertions, and one test draining the buffer would starve another. Each
// bucket is capped so a chatty test cannot grow it without bound.
// ---------------------------------------------------------------------------

/// Per-thread captured records, formatted as `"<LEVEL>: <message>"`.
type CapturedLogs = std::sync::Mutex<std::collections::HashMap<std::thread::ThreadId, Vec<String>>>;

/// How many records one thread's bucket keeps before the oldest are dropped.
const CAPTURED_LOG_CAP: usize = 256;

static CAPTURED_LOGS: std::sync::OnceLock<CapturedLogs> = std::sync::OnceLock::new();

struct CaptureLogger;

impl log::Log for CaptureLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        // Everything: a filter here would decide another test's fate, and the
        // per-thread buckets keep the volume bounded anyway.
        true
    }

    fn log(&self, record: &log::Record) {
        if let Ok(mut logs) = CAPTURED_LOGS
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
            .lock()
        {
            let bucket = logs.entry(std::thread::current().id()).or_default();
            bucket.push(format!("{}: {}", record.level(), record.args()));
            if bucket.len() > CAPTURED_LOG_CAP {
                let excess = bucket.len() - CAPTURED_LOG_CAP;
                bucket.drain(..excess);
            }
        }
    }

    fn flush(&self) {}
}

/// Install the process-wide capture logger (idempotent, thread-safe).
///
/// `Trace` is the max level on purpose: a test that asserts "this is a `debug`
/// line" needs `debug!` records to reach the logger at all.
pub(super) fn init_log_capture() {
    static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INIT.get_or_init(|| {
        let _ = log::set_boxed_logger(Box::new(CaptureLogger));
        log::set_max_level(log::LevelFilter::Trace);
    });
}

/// The calling thread's captured records, oldest first.
pub(super) fn captured_logs() -> Vec<String> {
    init_log_capture();
    CAPTURED_LOGS
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map(|logs| {
            logs.get(&std::thread::current().id())
                .cloned()
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// Discard the calling thread's captured records.
pub(super) fn clear_captured_logs() {
    init_log_capture();
    if let Ok(mut logs) = CAPTURED_LOGS
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
    {
        logs.remove(&std::thread::current().id());
    }
}

// GUI-REFACTOR-W3-20 helpers moved out of the ratcheted crate root:
// `new_app`, `open_and_decode` and the 2×1 PNG fixture are shared by the
// thematic test modules through `crate::tests`'s `use support::*;`.
pub(super) fn new_app() -> LuminaApp {
    LuminaApp::new(egui::Context::default())
}

/// Open a file and synchronously drain the background decode (PERF-GUI-7)
/// channel. The headless test harness has no `update()` event loop, so the
/// async `decode_rx` must be pumped here before asserting on the result.
pub(super) fn open_and_decode(app: &mut LuminaApp, path: impl Into<String>) {
    app.open_file(path);
    // Pump the background decode channel; yield so the worker thread is
    // scheduled. Bounded so a genuine failure can't hang the suite.
    for _ in 0..2000 {
        app.poll_decode();
        if app.original.is_some() || app.error().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(super) fn png() -> Vec<u8> {
    ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// `(text rect, clip rect)` of every painted text shape whose full string
/// equals `needle` (button labels). A widget cut off at a panel edge is painted
/// with a clip rect smaller than its text rect. Exact match (not substring) so
/// unrelated labels can never trip the assertion.
pub(super) fn text_shapes_for(
    shapes: &[egui::epaint::ClippedShape],
    needle: &str,
) -> Vec<(egui::Rect, egui::Rect)> {
    let mut out = Vec::new();
    for clipped in shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            if text.galley.text() == needle {
                out.push((
                    egui::Rect::from_min_size(text.pos, text.galley.size()),
                    clipped.clip_rect,
                ));
            }
        }
    }
    out
}

/// Full text of every painted text shape (button/slider readouts).
pub(super) fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Every painted occurrence of the button label `needle` must lie fully inside
/// its clip rect (1px tolerance for rounding).
pub(super) fn assert_fully_visible(shapes: &[egui::epaint::ClippedShape], needle: &str) {
    let hits = text_shapes_for(shapes, needle);
    assert!(
        !hits.is_empty(),
        "{needle:?} must be painted; painted texts: {:?}",
        painted_texts(shapes)
    );
    for (rect, clip) in &hits {
        assert!(
            clip.expand(1.0).contains_rect(*rect),
            "{needle:?} text {rect:?} must be fully inside its clip {clip:?}"
        );
    }
}

/// Whether any painted text shape's galley contains `needle` (robust to label
/// wrapping, unlike the exact `text_shapes_for`).
pub(super) fn text_contains(shapes: &[egui::epaint::ClippedShape], needle: &str) -> bool {
    shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::Text(text) => text.galley.text().contains(needle),
        _ => false,
    })
}

/// GUI-VISION-1: drive one headless egui frame (`Context::run_ui`, no GPU
/// needed) and return the painted shapes. Layout-overflow regressions (buttons
/// clipped at the panel edge) fail here in `cargo test -p lumina-gui --lib`
/// instead of only in kittest goldens.
pub(super) fn headless_shapes(
    app: &mut LuminaApp,
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<egui::epaint::ClippedShape> {
    headless_shapes_sized(app, 720.0, draw)
}

/// `headless_shapes` with an explicit canvas height: panels whose controls
/// extend past the 720px fold are fully painted on a taller virtual screen
/// (the production panel scrolls; the test asserts the whole content).
pub(super) fn headless_shapes_sized(
    app: &mut LuminaApp,
    height: f32,
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<egui::epaint::ClippedShape> {
    headless_frame_sized(app, height, draw).0
}

/// UX-LOOK-TOOLBAR-18: like [`headless_shapes_sized`], but also returns the
/// `egui::Context`, so icon buttons (which paint no text label) can be located
/// via `Context::read_response(<ToolbarIcon as id>)`.
pub(super) fn headless_frame_sized(
    app: &mut LuminaApp,
    height: f32,
    mut draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> (Vec<egui::epaint::ClippedShape>, egui::Context) {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1200.0, height),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| draw(app, ui));
    // No GPU renderer consumes the per-frame texture deltas in these headless
    // tests; dropping them would trip epaint's "unapplied deltas" assertion.
    output.textures_delta.clear();
    (output.shapes, ctx)
}

/// [`headless_frame_sized`] on the default 720px canvas.
pub(super) fn headless_frame(
    app: &mut LuminaApp,
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> (Vec<egui::epaint::ClippedShape>, egui::Context) {
    headless_frame_sized(app, 720.0, draw)
}

/// UX-LOOK-TOOLBAR-18: the icon button must be registered (so a real click
/// lands on it), lie fully inside the virtual screen and paint at least one
/// primitive inside its rect. Icon buttons have no text label, so this is the
/// paint/visibility guard that replaces `assert_fully_visible`.
pub(super) fn assert_icon_painted(
    shapes: &[egui::epaint::ClippedShape],
    ctx: &egui::Context,
    icon: crate::icon_toolbar::ToolbarIcon,
) {
    let response = ctx
        .read_response(icon.id())
        .unwrap_or_else(|| panic!("icon {icon:?} must be registered/painted in its surface"));
    let rect = response.rect;
    assert!(
        rect.is_positive() && rect.is_finite(),
        "icon {icon:?} rect must be valid: {rect:?}"
    );
    let screen = ctx.viewport_rect();
    assert!(
        screen.expand(0.5).contains_rect(rect),
        "icon {icon:?} {rect:?} must lie inside the screen {screen:?}"
    );
    let painted = shapes
        .iter()
        .any(|clipped| clipped.shape.visual_bounding_rect().intersects(rect));
    assert!(
        painted,
        "icon {icon:?} must paint at least one shape inside {rect:?}"
    );
}

/// Draw only the preview area (the view-toolbar host) in a headless pass.
pub(super) fn draw_preview_area_only(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.draw_preview_area(&ctx, ui);
}

/// Paint the preview area once (no GPU) and return the painted shapes plus the
/// context, so the icon buttons can be located by widget id.
pub(super) fn preview_area_frame(
    app: &mut LuminaApp,
) -> (Vec<egui::epaint::ClippedShape>, egui::Context) {
    headless_frame(app, draw_preview_area_only)
}

/// Tall variant for the state badges painted *after* the preview image (below
/// the 720px fold in the normal harness).
pub(super) fn preview_area_badge_shapes(app: &mut LuminaApp) -> Vec<egui::epaint::ClippedShape> {
    headless_shapes_sized(app, 2000.0, draw_preview_area_only)
}

/// Paint `draw` headless, locate the button painted with `label`, click it
/// (press + release on the text centre) and return the settled frame's shapes.
/// A single persistent `egui::Context` across frames is what makes the click
/// register.
pub(super) fn headless_click_label(
    app: &mut LuminaApp,
    label: &str,
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<egui::epaint::ClippedShape> {
    headless_click_labels(app, &[label], draw)
}

/// Like [`headless_click_label`], but clicks several labels in order inside one
/// persistent `egui::Context`: the first click can open a collapsing section
/// (History/Rating/Dust Removal), the next clicks its buttons. The returned
/// shapes are the last settled frame.
pub(super) fn headless_click_labels(
    app: &mut LuminaApp,
    labels: &[&str],
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<egui::epaint::ClippedShape> {
    headless_click_labels_sized(app, 720.0, labels, draw)
}

/// [`headless_click_labels`] returning the persistent context too.
pub(super) fn headless_click_labels_frame(
    app: &mut LuminaApp,
    labels: &[&str],
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> (Vec<egui::epaint::ClippedShape>, egui::Context) {
    headless_click_labels_sized_frame(app, 720.0, labels, draw)
}

/// [`headless_click_labels`] with an explicit canvas height: panels whose
/// sub-sections extend past the 720px fold (the Metadata panel) are fully
/// clickable on a taller virtual screen.
pub(super) fn headless_click_labels_sized(
    app: &mut LuminaApp,
    height: f32,
    labels: &[&str],
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<egui::epaint::ClippedShape> {
    headless_click_labels_sized_frame(app, height, labels, draw).0
}

/// [`headless_click_labels_sized`] returning the persistent context too, so
/// icon buttons can be checked after text-driven clicks (F-100 audit).
pub(super) fn headless_click_labels_sized_frame(
    app: &mut LuminaApp,
    height: f32,
    labels: &[&str],
    mut draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> (Vec<egui::epaint::ClippedShape>, egui::Context) {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, height));
    let mut time = 0.0_f64;
    let mut run = |app: &mut LuminaApp, events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| draw(app, ui),
        );
        output.textures_delta.clear();
        output.shapes
    };
    let mut shapes = run(app, vec![]);
    for label in labels {
        assert_fully_visible(&shapes, label);
        let pos = text_shapes_for(&shapes, label)
            .into_iter()
            .next()
            .expect("button label painted")
            .0
            .center();
        let click = |pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        run(app, vec![egui::Event::PointerMoved(pos), click(true)]);
        run(app, vec![egui::Event::PointerMoved(pos), click(false)]);
        // 30 frames settle the collapse/expand animation (~0.5 s).
        shapes = Vec::new();
        for _ in 0..30 {
            shapes = run(app, vec![]);
        }
    }
    (shapes, ctx)
}

/// UX-LOOK-TOOLBAR-18: click several icon buttons in order inside one
/// persistent `egui::Context` (icon buttons paint no text label, so the click
/// position comes from `Context::read_response`). Returns the last settled
/// frame plus the context.
pub(super) fn headless_click_icons_frame(
    app: &mut LuminaApp,
    icons: &[crate::icon_toolbar::ToolbarIcon],
    mut draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> (Vec<egui::epaint::ClippedShape>, egui::Context) {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 720.0));
    let mut time = 0.0_f64;
    let mut run = |app: &mut LuminaApp, events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| draw(app, ui),
        );
        output.textures_delta.clear();
        output.shapes
    };
    let mut shapes = run(app, vec![]);
    for icon in icons {
        let pos = ctx
            .read_response(icon.id())
            .unwrap_or_else(|| panic!("icon {icon:?} must be painted before clicking"))
            .rect
            .center();
        let click = |pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        run(app, vec![egui::Event::PointerMoved(pos), click(true)]);
        run(app, vec![egui::Event::PointerMoved(pos), click(false)]);
        shapes = Vec::new();
        for _ in 0..4 {
            shapes = run(app, vec![]);
        }
    }
    (shapes, ctx)
}

/// F-100 Klickbarkeit: exhaustive mapping of every keyboard-toggle enum variant
/// to the `Str` label of its clickable button. No `_` arm — a new
/// `ViewToggle`/`PanelToggle` variant fails compilation here, so a new keyboard
/// toggle cannot land without its button and audit entry.
pub(super) fn view_toggle_button_label(toggle: ViewToggle) -> Str {
    match toggle {
        ViewToggle::BlackWhite => Str::TreatmentBlackWhite,
        ViewToggle::Clipping => Str::ViewToolbarClipping,
        ViewToggle::LightsOut => Str::ViewToolbarLightsOut,
    }
}

/// Exhaustive mapping of every `PanelToggle` variant to its button label
/// (see [`view_toggle_button_label`]).
pub(super) fn panel_toggle_button_label(toggle: PanelToggle) -> Str {
    match toggle {
        PanelToggle::CropMode => Str::ViewToolbarCrop,
        PanelToggle::PanelsHidden => Str::ViewToolbarPanels,
    }
}

/// UX-LOOK-TOOLBAR-18: click `icon` twice in the preview toolbar; assert the
/// state flips to `on`, then back off, with the expected status text, and that
/// the icon stays painted (registered + visible) after each flip.
pub(super) fn assert_preview_icon_toggles(
    app: &mut LuminaApp,
    icon: crate::icon_toolbar::ToolbarIcon,
    on: impl Fn(&LuminaApp) -> bool,
    on_status: &str,
    off_status: &str,
) {
    let (shapes, ctx) = headless_click_icons_frame(app, &[icon], draw_preview_area_only);
    assert!(on(app), "{icon:?} click must arm the state");
    assert_eq!(app.status, on_status, "{icon:?} on status");
    assert_icon_painted(&shapes, &ctx, icon);
    let (shapes, ctx) = headless_click_icons_frame(app, &[icon], draw_preview_area_only);
    assert!(!on(app), "{icon:?} second click must disarm the state");
    assert_eq!(app.status, off_status, "{icon:?} off status");
    assert_icon_painted(&shapes, &ctx, icon);
}
