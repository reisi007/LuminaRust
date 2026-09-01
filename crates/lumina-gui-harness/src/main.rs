//! Autonomous GUI verification harness for the native Lumina desktop app.
//!
//! Drives the **real on-screen** `lumina-gui` window on macOS entirely without
//! manual intervention:
//!
//! 1. Spawns `cargo run -p lumina-gui -- <dir>` in the background, capturing
//!    stdout/stderr to a log file.
//! 2. Waits for the GUI to finish starting (we detect the `Lumina` window via
//!    the macOS Accessibility API through `osascript`).
//! 3. Captures screenshots at a few checkpoints using `/usr/sbin/screencapture
//!    -R` (the window's on-screen rectangle).
//! 4. Drives real input through [`enigo`]:
//!    - clicks the first *filmstrip* thumbnail (bottom of the window),
//!    - drags the Exposure slider in the right Develop panel,
//!    - switches Library/Develop modules with the `G` / `D` keys.
//! 5. Verifies from the GUI's own stderr log that an image was actually
//!    loaded / rendered (`loaded image …`) and that no panic (crash) occurred.
//! 6. Writes a JSON report and exits 0/1 accordingly.
//!
//! The checkpoints are turned into `screencapture` PNGs (under
//! `target/gui-verify/`) so the screenshots can be *reviewed by a human or a
//! vision tool* afterwards — the harness itself asserts on log markers and
//! exit code (see the task spec: "at least via log/exit-code dass kein Crash
//! und Preview vorhanden").
//!
//! ## Prerequisites (macOS permissions)
//!
//! The process needs **Accessibility** (for `osascript` to query/manipulate the
//! window and for enigo to post synthetic input) and **Screen Recording** (for
//! `screencapture`). Grant these to the terminal/app that runs this binary in
//! *System Settings → Privacy & Security*. See `README.md` for details.
//!
//! ## Platform
//!
//! This binary is macOS-only. On any other platform it compiles to a stub that
//! explains this and exits 2. All macOS-specific machinery (enigo, screenshots,
//! AppleScript, JSON reporting) is `#[cfg(target_os = "macos")]`-gated so the
//! cross-platform CI (`cargo check --workspace --all-targets` + `-D warnings`)
//! stays clean on Linux without `allow`-attributes.

fn main() {
    std::process::exit(real_main());
}

#[cfg(target_os = "macos")]
fn real_main() -> i32 {
    crate::macos::run_macos_harness()
}

#[cfg(not(target_os = "macos"))]
fn real_main() -> i32 {
    eprintln!(
        "gui-verify: this harness is macOS-only (enigo + screencapture + osascript); \
         it cannot run on this platform."
    );
    2
}

// All actual logic lives in the macOS-gated module.
#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Output, Stdio};
    use std::time::{Duration, Instant};

    // -----------------------------------------------------------------------
    // External command helpers
    // -----------------------------------------------------------------------

    fn run_capture(cmd: &mut Command) -> std::io::Result<Output> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output()
    }

    fn cmd_stdout(cmd: &mut Command) -> std::io::Result<String> {
        let out = run_capture(cmd)?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn sleep(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    // -----------------------------------------------------------------------
    // Window / screenshot helpers (macOS)
    // -----------------------------------------------------------------------

    /// A window's on-screen rectangle in **points**, top-left origin, y-down
    /// (matches both `screencapture -R` and enigo `Coordinate::Abs` on macOS).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct Rect {
        pub(crate) x: f64,
        pub(crate) y: f64,
        pub(crate) w: f64,
        pub(crate) h: f64,
    }

    /// Query the `Lumina` window rectangle through the macOS Accessibility API.
    /// We look up the process by its friendly name (eframe sets the title
    /// "Lumina") and fall back to scanning all running processes for a window
    /// titled "Lumina". Returns `None` when no such window is (yet) found.
    fn find_lumina_window() -> Option<Rect> {
        // Try the process named "Lumina" first (eframe's run_native title).
        let script = r#"
tell application "System Events"
  try
    set theProcess to first process whose name is "Lumina"
    set theWindow to front window of theProcess
    set {x, y} to position of theWindow
    set {w, h} to size of theWindow
    return (x as text) & "," & (y as text) & "," & (w as text) & "," & (h as text)
  on error
    return ""
  end try
end tell
"#;
        if let Some(r) = parse_rect(&run_osascript(script)) {
            return Some(r);
        }
        // Fallback: scan every process for a window whose title starts with
        // Lumina (robust against a different process name).
        let scan = r#"
tell application "System Events"
  repeat with theProcess in (every process whose background only is false)
    try
      repeat with theWindow in (every window of theProcess)
        if (name of theWindow) starts with "Lumina" then
          set {x, y} to position of theWindow
          set {w, h} to size of theWindow
          return (x as text) & "," & (y as text) & "," & (w as text) & "," & (h as text)
        end if
      end repeat
    end try
  end repeat
  return ""
end tell
"#;
        parse_rect(&run_osascript(scan))
    }

    fn run_osascript(script: &str) -> String {
        let mut cmd = Command::new("osascript");
        cmd.arg("-e").arg(script);
        cmd_stdout(&mut cmd).unwrap_or_default()
    }

    pub(crate) fn parse_rect(s: &str) -> Option<Rect> {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        if parts.len() != 4 {
            return None;
        }
        let nums: Vec<f64> = parts.iter().filter_map(|p| p.parse().ok()).collect();
        if nums.len() != 4 || nums[2] <= 0.0 || nums[3] <= 0.0 {
            return None;
        }
        Some(Rect {
            x: nums[0],
            y: nums[1],
            w: nums[2],
            h: nums[3],
        })
    }

    /// Bring the Lumina window to the front (so clicks land on it) and make it
    /// active for keyboard input.
    fn activate_lumina() -> bool {
        let script = r#"
tell application "System Events"
  try
    set theProcess to first process whose name is "Lumina"
    set frontmost of theProcess to true
    return "ok"
  on error
    return ""
  end try
end tell
"#;
        run_osascript(script) == "ok"
    }

    /// Capture the given on-screen rectangle to a PNG via `screencapture -R`.
    /// Returns the path on success.
    fn screenshot_rect(rect: Rect, out_dir: &Path, name: &str) -> Option<PathBuf> {
        let path = out_dir.join(format!("{name}.png"));
        let x = rect.x.round() as i64;
        let y = rect.y.round() as i64;
        let w = rect.w.round() as i64;
        let h = rect.h.round() as i64;
        let region = format!("{x},{y},{w},{h}");
        // `-x` silences the shutter sound; `-R` captures the given rect in points.
        let status = Command::new("screencapture")
            .args(["-x", "-R", &region, &path.display().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(s) if s.success() => Some(path),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // enigo input helpers
    // -----------------------------------------------------------------------

    fn enigo_new() -> Result<enigo::Enigo, String> {
        use enigo::Settings;
        // Give the OS a moment to process each synthetic key/button event so the
        // GUI reliably sees the final state (one long enough delay per action).
        let settings = Settings {
            mac_delay: 80,
            release_keys_when_dropped: true,
            ..Default::default()
        };
        enigo::Enigo::new(&settings).map_err(|e| format!("cannot connect to enigo: {e:?}"))
    }

    /// Click (left button) at an absolute screen coordinate.
    fn click_at(enigo: &mut enigo::Enigo, x: i32, y: i32) -> Result<(), String> {
        use enigo::{Button, Coordinate, Direction, Mouse};
        enigo
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|e| format!("move_mouse failed: {e:?}"))?;
        sleep(120);
        enigo
            .button(Button::Left, Direction::Click)
            .map_err(|e| format!("button click failed: {e:?}"))
    }

    /// Drag (press left, move, release) between two absolute screen
    /// coordinates — a slider drag.
    fn drag(enigo: &mut enigo::Enigo, from: (i32, i32), to: (i32, i32)) -> Result<(), String> {
        use enigo::{Button, Coordinate, Direction, Mouse};
        enigo
            .move_mouse(from.0, from.1, Coordinate::Abs)
            .map_err(|e| format!("drag move start failed: {e:?}"))?;
        sleep(120);
        enigo
            .button(Button::Left, Direction::Press)
            .map_err(|e| format!("drag press failed: {e:?}"))?;
        sleep(120);
        // Move in a few steps so egui's slider picks up the drag trajectory.
        let (x0, y0) = from;
        let (x1, y1) = to;
        for i in 1..=6 {
            let t = i as f64 / 6.0;
            let x = (x0 as f64 + (x1 - x0) as f64 * t).round() as i32;
            let y = (y0 as f64 + (y1 - y0) as f64 * t).round() as i32;
            enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(|e| format!("drag move step failed: {e:?}"))?;
            sleep(50);
        }
        sleep(80);
        enigo
            .button(Button::Left, Direction::Release)
            .map_err(|e| format!("drag release failed: {e:?}"))
    }

    /// Press a single key once (used for the `G` / `D` module shortcuts).
    fn tap_key(enigo: &mut enigo::Enigo, ch: char) -> Result<(), String> {
        use enigo::{Direction, Keyboard};
        enigo
            .key(enigo::Key::Unicode(ch), Direction::Click)
            .map_err(|e| format!("key {ch} failed: {e:?}"))
    }

    // -----------------------------------------------------------------------
    // Report
    // -----------------------------------------------------------------------

    #[derive(serde::Serialize)]
    struct Report {
        passed: bool,
        checkpoints: Vec<Checkpoint>,
        files: BTreeMap<String, String>,
        log_markers: LogMarkers,
    }

    #[derive(serde::Serialize)]
    struct Checkpoint {
        name: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    }

    #[derive(serde::Serialize, Default)]
    pub(crate) struct LogMarkers {
        pub(crate) gui_started: bool,
        pub(crate) image_loaded: bool,
        /// How many times we saw the "loaded image" marker (should be ≥1).
        pub(crate) image_loads: usize,
        pub(crate) panic_seen: bool,
        /// Any ERROR-level lines captured from the GUI stderr (informational).
        pub(crate) error_lines: Vec<String>,
    }

    // -----------------------------------------------------------------------
    // The harness run
    // -----------------------------------------------------------------------

    pub fn run_macos_harness() -> i32 {
        let root = workspace_root();
        let out_dir = root.join("target").join("gui-verify");
        let _ = std::fs::create_dir_all(&out_dir);

        let mut report = Report {
            passed: false,
            checkpoints: Vec::new(),
            files: BTreeMap::new(),
            log_markers: LogMarkers::default(),
        };

        let mut cp = |name: &str, status: &str, note: Option<String>| {
            report.checkpoints.push(Checkpoint {
                name: name.into(),
                status: status.into(),
                note,
            });
        };

        // ---- 1. Spawn the GUI ------------------------------------------------
        let workdir = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("sample-data").join("raw"));
        let workdir = if workdir.is_absolute() {
            workdir
        } else {
            root.join(workdir)
        };
        println!("[gui-verify] workdir = {}", workdir.display());

        let log_path = out_dir.join("gui.log");
        let log_file = std::fs::File::create(&log_path).expect("create gui log");
        // Clone the File so stdout and stderr both feed the same log file.
        let out_file = log_file.try_clone().expect("clone log file for stdout");
        let err_file = log_file; // stderr handle

        let mut gui = match Command::new("cargo")
            .args(["run", "-p", "lumina-gui", "--"])
            .arg(&workdir)
            .stdout(Stdio::from(out_file))
            .stderr(Stdio::from(err_file))
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[gui-verify] could not spawn `cargo run -p lumina-gui`: {e}");
                print_report(&report, &out_dir);
                return 1;
            }
        };
        // RAII: ensure the GUI is terminated on every exit path so the harness
        // never leaves the app running against real files (guards by PID so the
        // owned `Child` above stays usable for `try_wait`/`kill`).
        let _guard = KillOnDrop(gui.id());

        // ---- 2. Wait for the GUI window to appear ------------------------------
        cp("gui_start", "pending", None);
        let wait_start = Instant::now();
        let window = loop {
            if let Some(r) = find_lumina_window() {
                break Some(r);
            }
            if read_log_new(&log_path).contains("PANIC") || !is_child_alive(&mut gui) {
                break None;
            }
            if wait_start.elapsed() > Duration::from_secs(120) {
                break None;
            }
            sleep(300);
        };

        let Some(window) = window else {
            eprintln!(
                "[gui-verify] Lumina window did not appear within 120s \
                 (check permissions / launch failure)."
            );
            cp(
                "gui_start",
                "failed",
                Some("window not found within timeout".into()),
            );
            let markers = scan_log(&log_path);
            report.log_markers = markers;
            let _ = gui.kill();
            print_report(&report, &out_dir);
            return 1;
        };
        cp("gui_start", "ok", Some(format!("window rect = {window:?}")));
        println!("[gui-verify] Lumina window found at {window:?}");

        // ---- 3. Activate, let the first image auto-load, screenshot ------------
        activate_lumina();
        // Give the GUI a moment to auto-load the first RAW entry and render it.
        sleep(6000);
        let markers = scan_log(&log_path);
        let auto_loaded = markers.image_loaded;
        report.log_markers.gui_started = markers.gui_started;
        report.log_markers.image_loaded = markers.image_loaded;
        report.log_markers.image_loads = markers.image_loads;
        report.log_markers.panic_seen = markers.panic_seen;
        report.log_markers.error_lines = markers.error_lines;

        // Screenshot the initial Develop view (baseline).
        if let Some(p) = screenshot_rect(window, &out_dir, "01_develop_initial") {
            report
                .files
                .insert("01_develop_initial".into(), p.display().to_string());
            println!("[gui-verify] screenshot: {}", p.display());
        }
        cp("initial_preview", "pending", None);

        // ---- 4. Interaction: filmstrip click + Exposure drag + module keys ----
        // The filmstrip sits at the BOTTOM of the window. The first thumbnail
        // is near the left edge (the folder tree is not present in Develop, and
        // the left navigator rail is collapsed by default, so the strip starts
        // at the window's left padding).
        let filmstrip_pos = (
            (window.x + 24.0).round() as i32,
            (window.y + window.h - 40.0).round() as i32,
        );

        // The right Develop "Basic" panel: Exposure is the first slider of the
        // first (Basic) collapsible section. Approximate its slider centre
        // within the right panel: aim for a point in the right panel, ~65% of
        // its width in from the panel's left edge, and ~260px below the window
        // top (Basic occupies the upper part of the right panel).
        let panel_left_w = 320.0; // right panel default width (see lib.rs)
        let slider_x = (window.x + window.w - panel_left_w + panel_left_w * 0.65).round() as i32;
        let exposure_from = (slider_x, (window.y + 260.0).round() as i32);
        // Drag to the right by ~120 px to increase exposure.
        let exposure_to = (slider_x + 120, exposure_from.1);

        let mut enigo = match enigo_new() {
            Ok(e) => e,
            Err(msg) => {
                eprintln!(
                    "[gui-verify] enigo init failed: {msg} \
                     (grant Accessibility permission?)"
                );
                cp("enigo_init", "failed", Some(msg));
                let _ = gui.kill();
                print_report(&report, &out_dir);
                return 1;
            }
        };

        // 4a. Click the filmstrip thumbnail.
        let filmstrip_ok = click_at(&mut enigo, filmstrip_pos.0, filmstrip_pos.1).is_ok();
        cp(
            "filmstrip_click",
            if filmstrip_ok { "ok" } else { "failed" },
            Some(format!(
                "clicked ({}, {})",
                filmstrip_pos.0, filmstrip_pos.1
            )),
        );
        sleep(4000);

        // 4b. Drag the Exposure slider.
        let exposure_ok = drag(&mut enigo, exposure_from, exposure_to).is_ok();
        cp(
            "exposure_drag",
            if exposure_ok { "ok" } else { "failed" },
            Some(format!("from {exposure_from:?} to {exposure_to:?}")),
        );
        sleep(4000);

        // Screenshot after the exposure edit.
        if let Some(p) = screenshot_rect(window, &out_dir, "02_develop_exposure") {
            report
                .files
                .insert("02_develop_exposure".into(), p.display().to_string());
        }

        // 4c. Switch to Library (G), then back to Develop (D).
        let g_ok = tap_key(&mut enigo, 'g').is_ok();
        sleep(2000);
        if let Some(p) = screenshot_rect(window, &out_dir, "03_library_g") {
            report
                .files
                .insert("03_library_g".into(), p.display().to_string());
            println!("[gui-verify] screenshot (Library): {}", p.display());
        }
        let d_ok = tap_key(&mut enigo, 'd').is_ok();
        sleep(2000);
        cp(
            "module_keys_g_d",
            match (g_ok, d_ok) {
                (true, true) => "ok",
                _ => "failed",
            },
            Some(format!("G={g_ok} D={d_ok}")),
        );

        // Screenshot final Develop view after D.
        if let Some(p) = screenshot_rect(window, &out_dir, "04_develop_final") {
            report
                .files
                .insert("04_develop_final".into(), p.display().to_string());
        }

        // ---- 5. Verification from the GUI's stderr log --------------------------
        let final_markers = scan_log(&log_path);
        report.log_markers.gui_started = final_markers.gui_started;
        report.log_markers.image_loaded = final_markers.image_loaded;
        report.log_markers.image_loads = final_markers.image_loads;
        report.log_markers.panic_seen = final_markers.panic_seen;
        report.log_markers.error_lines = final_markers.error_lines.clone();

        // "Preview vorhanden / kein Crash" is verified via the GUI's own log:
        // `loaded image <name> (raw=..., ...)` is emitted by apply_decoded_frame.
        // A panic triggers logger::install_panic_hook which prints `PANIC at ...`.
        let preview_ok = final_markers.image_loaded && !final_markers.panic_seen;
        cp(
            "preview_present",
            if preview_ok { "ok" } else { "failed" },
            Some(format!(
                "image_loaded={} panic_seen={}",
                final_markers.image_loaded, final_markers.panic_seen
            )),
        );

        let still_alive = is_child_alive(&mut gui);
        cp(
            "no_crash",
            if still_alive && !final_markers.panic_seen {
                "ok"
            } else {
                "failed"
            },
            Some(format!(
                "child_alive={still_alive} panic_seen={}",
                final_markers.panic_seen
            )),
        );

        // A failed decode surfaces an ERROR line; surface those in the report,
        // but they are informational (a decode of the sample data should
        // succeed).
        if !final_markers.error_lines.is_empty() {
            println!(
                "[gui-verify] GUI logged {} ERROR line(s):",
                final_markers.error_lines.len()
            );
            for l in &final_markers.error_lines {
                println!("    {l}");
            }
        }

        report.passed = preview_ok && still_alive && !final_markers.panic_seen && auto_loaded;

        // Clean up: kill the GUI (the sample-data must not be modified, so we
        // never leave the app running against real user files).
        let _ = gui.kill();
        let _ = gui.wait();

        print_report(&report, &out_dir);
        if report.passed {
            println!("[gui-verify] PASSED");
            0
        } else {
            println!("[gui-verify] FAILED — see report and screenshots above.");
            1
        }
    }

    /// Parse interesting markers out of the GUI's stderr log file.
    pub(crate) fn scan_log(path: &Path) -> LogMarkers {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        let mut m = LogMarkers::default();
        let mut load_count = 0usize;
        for line in contents.lines() {
            if line.contains("Lumina logging initialised") {
                m.gui_started = true;
            }
            // apply_decoded_frame always logs this after a successful decode.
            if line.contains("loaded image ") {
                load_count += 1;
            }
            if line.contains("PANIC") {
                m.panic_seen = true;
            }
            if line.contains("[ERROR]") || line.starts_with("ERROR ") {
                m.error_lines.push(line.to_owned());
            }
        }
        m.image_loads = load_count;
        m.image_loaded = load_count > 0;
        m
    }

    /// Read only newly appended lines of the log (used for the startup poll).
    fn read_log_new(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// The repository root (parent of `crates` via the manifest location).
    fn workspace_root() -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest
            .parent()
            .and_then(|crates| crates.parent())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }

    fn is_child_alive(child: &mut Child) -> bool {
        child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    /// Print the report as pretty JSON plus a human-readable summary.
    fn print_report(report: &Report, out_dir: &Path) {
        println!("\n[gui-verify] checkpoints:");
        for cp in &report.checkpoints {
            let note = cp.note.as_deref().unwrap_or("");
            println!("    [{:>7}] {}  {note}", cp.status, cp.name);
        }
        println!("[gui-verify] screenshots:");
        for (name, path) in &report.files {
            println!("    {name}: {path}");
        }
        let report_path = out_dir.join("report.json");
        let json = serde_json::to_string_pretty(report).unwrap_or_default();
        if let Ok(mut f) = std::fs::File::create(&report_path) {
            let _ = f.write_all(json.as_bytes());
            println!("[gui-verify] report written to {}", report_path.display());
        } else {
            println!("[gui-verify] report (could not write file):\n{json}");
        }
    }

    /// RAII guard to ensure we never leave the GUI running against real files.
    /// Holds the child's PID (SIGTERM on drop); the owned `Child` elsewhere
    /// remains usable for liveness checks and an explicit kill at the end.
    struct KillOnDrop(u32);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = Command::new("kill")
                .arg("-TERM")
                .arg(self.0.to_string())
                .status();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (macOS-only: they exercise the target-specific logic)
// ---------------------------------------------------------------------------

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::macos::{parse_rect, scan_log, LogMarkers, Rect};
    use serde_json::Value;

    #[test]
    fn parse_rect_accepts_canonical() {
        let s = "100, 50, 200, 100";
        let r = parse_rect(s).unwrap();
        assert_eq!(
            r,
            Rect {
                x: 100.0,
                y: 50.0,
                w: 200.0,
                h: 100.0
            }
        );
    }

    #[test]
    fn parse_rect_rejects_junk_and_zero() {
        assert!(parse_rect("").is_none());
        assert!(parse_rect("not,numbers").is_none());
        assert!(parse_rect("1,2,0,3").is_none());
        assert!(parse_rect("1,2,3").is_none());
    }

    #[test]
    fn scan_log_detects_markers() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("gui-scan-test-{}.log", std::process::id()));
        std::fs::write(
            &p,
            "Lumina logging initialised; level=Info (stderr)\n\
             [INFO] lumina_gui: loaded image aircraft.cr3 (raw=true, camera_white_balance=Some(...))\n\
             [ERROR] lumina_gui: something bad\n",
        )
        .unwrap();
        let m = scan_log(&p);
        std::fs::remove_file(&p).ok();
        assert!(m.gui_started);
        assert!(m.image_loaded);
        assert_eq!(m.image_loads, 1);
        assert!(!m.panic_seen);
        assert_eq!(m.error_lines.len(), 1);
    }

    #[test]
    fn log_markers_default_is_well_formed() {
        let m = LogMarkers::default();
        assert!(!m.gui_started);
        assert!(!m.image_loaded);
        assert_eq!(m.image_loads, 0);
    }

    #[test]
    fn report_json_shape_is_consistent() {
        // Ensure the serialised LogMarkers field naming is stable for tooling.
        let value = serde_json::to_value(LogMarkers {
            gui_started: true,
            image_loaded: true,
            image_loads: 2,
            panic_seen: false,
            error_lines: vec![],
        })
        .unwrap();
        assert_eq!(value["image_loads"], 2);
        assert!(value.get("panic_seen").is_some());
        let _: Value = value;
    }
}
