//! Minimal logging foundation for the native Lumina GUI.
//!
//! Logs to **stderr only** (the caller is expected to redirect stderr to a file
//! during manual testing, e.g. `lumina-gui <dir> 2> run.log`). A small ring
//! buffer of recent messages is kept so the panic hook can emit "what happened
//! before the crash" to stderr. No file handling in the binary itself.
//!
//! This depends only on the `log` facade crate.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use log::{LevelFilter, Log, Metadata, Record};

const RING_CAPACITY: usize = 512;

static RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn ring() -> &'static Mutex<VecDeque<String>> {
    RING.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING_CAPACITY)))
}

struct StderrLogger {
    level: LevelFilter,
}

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
        if let Ok(mut ring) = ring().lock() {
            ring.push_back(line.clone());
            while ring.len() > RING_CAPACITY {
                ring.pop_front();
            }
        }
        eprintln!("{line}");
    }

    fn flush(&self) {}
}

/// Initialise stderr logging. Honours `RUST_LOG` for the level, falling back to
/// `default_level`. Returns the effective level (for diagnostics).
pub fn init_logging(default_level: LevelFilter) -> LevelFilter {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse::<LevelFilter>().ok())
        .unwrap_or(default_level);

    let logger = StderrLogger { level };
    // A second call (e.g. re-entry) is harmless; keep the first logger.
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(level);
    log::info!("Lumina logging initialised; level={level} (stderr)");
    level
}

/// Install a panic hook that dumps the recent log ring buffer to stderr, so a
/// crash is analysable after stderr is redirected to a file.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = match (
            info.payload().downcast_ref::<&str>(),
            info.payload().downcast_ref::<String>(),
        ) {
            (Some(s), _) => (*s).to_string(),
            (_, Some(s)) => s.clone(),
            _ => "<no payload>".to_string(),
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let recent = ring()
            .lock()
            .map(|r| r.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        eprintln!(
            "PANIC at {loc}: {msg}\n--- last {n} log lines ---\n{lines}",
            n = recent.len(),
            lines = recent.join("\n")
        );
    }));
}
