//! GUI-INSTRDBG-17: core debug-action instrumentation tests. Moved out of
//! `gui_action.rs` (GUI-INSTRDBG-17c) so the extracted action-name table stays
//! inside the file-size ratchet. These tests pin the log-line format, the
//! nested-suppression contract, the unique snake_case name table and the
//! release passthrough (no instrumentation compiled in).
//!
//! The per-action logging tests live in `tests/instrdbg.rs` (17b),
//! `tests/instrdbg_rest.rs` (17b-REST), `tests/instrdbg_last.rs` (17c) and
//! `tests/instrdbg_rework.rs` (17c-Rework, incl. the F-1 filmstrip buttons).

use super::*;

fn new_app() -> LuminaApp {
    LuminaApp::new(egui_context())
}

fn egui_context() -> eframe::egui::Context {
    eframe::egui::Context::default()
}

#[test]
#[cfg(debug_assertions)]
fn instrdbg_action_log_line_format_and_single_line() {
    let mut app = new_app();
    let _ = take_gui_action_log();
    app.toggle_crop_mode();
    let lines = take_gui_action_log();
    assert_eq!(lines.len(), 1, "exactly one line per action, got {lines:?}");
    let line = &lines[0];
    assert!(line.starts_with("action=toggle_crop_mode "), "{line}");
    assert!(line.contains(" duration_ms="), "{line}");
    let route = line.rsplit_once("gpu_route=").expect("gpu_route field").1;
    assert!(
        route == GPU_ROUTE_PRESENT || route == GPU_ROUTE_CPU_FALLBACK || route == GPU_ROUTE_NA,
        "unexpected gpu_route in {line}"
    );
    assert_eq!(
        gui_action_log_line(GuiAction::ToggleCropMode, 3, GPU_ROUTE_NA),
        "action=toggle_crop_mode duration_ms=3 gpu_route=n/a"
    );
}

#[test]
#[cfg(debug_assertions)]
fn instrdbg_nested_action_logs_once_for_the_outer_action() {
    let mut app = new_app();
    let _ = take_gui_action_log();
    // `toggle_fullscreen` calls the instrumented `set_zoom_mode`; the
    // nested call must not add a second line.
    app.toggle_fullscreen();
    let lines = take_gui_action_log();
    assert_eq!(
        lines.len(),
        1,
        "nested instrumentation must not add a line: {lines:?}"
    );
    assert!(
        lines[0].starts_with("action=toggle_fullscreen "),
        "{}",
        lines[0]
    );
}

/// The central action-name table stays unique and snake_case (the format
/// contract of `action=<name>`), and `ALL_GUI_ACTIONS` covers every variant.
#[test]
#[cfg(debug_assertions)]
fn instrdbg_action_names_are_unique_and_snake_case() {
    let mut names: Vec<&str> = ALL_GUI_ACTIONS.iter().map(|action| action.name()).collect();
    for name in &names {
        assert!(!name.is_empty());
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name} is not snake_case"
        );
        assert!(!name.starts_with('_') && !name.ends_with('_'), "{name}");
    }
    let count = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), count, "action names must be unique");
}

/// Release builds compile the timer, the log and the capture buffer out.
/// This test only exists in non-debug builds so `cargo test --release`
/// proves the action still runs without instrumentation.
#[test]
#[cfg(not(debug_assertions))]
fn instrdbg_release_action_is_uninstrumented() {
    let mut app = new_app();
    app.toggle_crop_mode();
    assert!(app.crop_mode);
}
