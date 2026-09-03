//! GUI-STARTUP-FOLLOWUP-1 (B2): `--module` misuse must fail LOUDLY as a real
//! child process — exit code 2 with usage on stderr — before any windowing
//! code runs. The parse sits ahead of `run_native` in `main()`, so these
//! paths need no display and no GPU.

use std::path::PathBuf;
use std::process::Command;

/// Locate the built `lumina-gui` binary: the `CARGO_BIN_EXE_*` environment
/// (set by cargo when building integration tests) first, sibling-of-the-test-
/// binary fallback second.
fn gui_binary() -> PathBuf {
    for name in ["CARGO_BIN_EXE_lumina_gui", "CARGO_BIN_EXE_lumina-gui"] {
        if let Ok(path) = std::env::var(name) {
            let path = PathBuf::from(path);
            if path.is_file() {
                return path;
            }
        }
    }
    // Fallback: `<target>/<profile>/deps/<test-binary>` → `<target>/<profile>/lumina-gui`.
    let mut path = std::env::current_exe().expect("current test binary path");
    path.pop(); // test binary file name
    if path.file_name().is_some_and(|name| name == "deps") {
        path.pop(); // deps/
    }
    #[cfg(windows)]
    path.push("lumina-gui.exe");
    #[cfg(not(windows))]
    path.push("lumina-gui");
    assert!(
        path.is_file(),
        "lumina-gui binary not found at {} (run `cargo build -p lumina-gui` first)",
        path.display()
    );
    path
}

fn run_gui(args: &[&str]) -> std::process::Output {
    Command::new(gui_binary())
        .args(args)
        .output()
        .expect("spawn lumina-gui")
}

/// B2: `--module bogus` exits 2 with the offending value and usage on stderr
/// (no window is attempted — the parse rejects before `run_native`).
#[test]
fn bogus_module_exits_2_with_usage_on_stderr() {
    let output = run_gui(&["--module", "bogus"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "bogus --module must exit 2, got {:?} (stdout: {:?})",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bogus"),
        "stderr must name the offending value, got: {stderr:?}"
    );
    assert!(
        stderr.contains("library|develop|export"),
        "stderr must name the valid set, got: {stderr:?}"
    );
    assert!(
        stderr.contains("usage:"),
        "stderr must print usage, got: {stderr:?}"
    );
}

/// B2: `--module` without a value exits 2 with usage on stderr.
#[test]
fn missing_module_value_exits_2_with_usage_on_stderr() {
    let output = run_gui(&["--module"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "missing --module value must exit 2, got {:?} (stdout: {:?})",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing value for --module"),
        "stderr must report the missing value, got: {stderr:?}"
    );
    assert!(
        stderr.contains("usage:"),
        "stderr must print usage, got: {stderr:?}"
    );
}
