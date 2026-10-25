//! The CLI's stderr logger and its once-per-process backend log line.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, logging slice; the root keeps the
//! `enum Command` dispatch surface because `scripts/gen_doc_facts.sh` derives
//! the documented command count from `src/main.rs`). Pure move: bodies are
//! byte-identical; only the `pub(super)`/`pub(crate)` tokens and this header
//! are new.

use super::*;
/// Minimal stderr logger installed once so the backend-selection `info!` is
/// actually visible. It only installs when no other logger has been registered
/// in this process (so an embedding application that installs its own is
/// respected). Output goes to stderr and therefore never corrupts the
/// `--json` payloads on stdout.
struct StderrLogger {
    level: log::LevelFilter,
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[lumina][{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Installs the stderr logger once (no-op if another logger is already set).
pub(super) fn init_cli_logger() {
    let level = log::LevelFilter::Info;
    if log::set_boxed_logger(Box::new(StderrLogger { level })).is_ok() {
        log::set_max_level(level);
    }
}

/// Logs the chosen render backend exactly once per process so GPU-vs-CPU
/// selection is always visible (Agents.md: no silent fallback).
pub(crate) fn log_backend(message: &str) {
    use std::sync::OnceLock;
    static LOGGED: OnceLock<()> = OnceLock::new();
    if LOGGED.set(()).is_ok() {
        info!("{message}");
    }
}
