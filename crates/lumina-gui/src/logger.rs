//! Minimal logging foundation for the native Lumina GUI.
//!
//! Logs to **stderr only** (the caller is expected to redirect stderr to a file
//! during manual testing, e.g. `lumina-gui <dir> 2> run.log`). A small ring
//! buffer of recent messages is kept so the panic hook can emit "what happened
//! before the crash" to stderr. No file handling in the binary itself.
//!
//! This depends only on the `log` facade crate.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use log::{LevelFilter, Log, Metadata, Record};

const RING_CAPACITY: usize = 512;

static RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

// Crash-Fix Runde 2: best-effort counters for degraded logging. Logging is a
// diagnostic side channel — it must never take the app down. These make the
// degradation observable instead of silent.
static RING_POISON_RECOVERIES: AtomicU64 = AtomicU64::new(0);
static STDERR_WRITE_FAILURES: AtomicU64 = AtomicU64::new(0);

fn ring() -> &'static Mutex<VecDeque<String>> {
    RING.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING_CAPACITY)))
}

/// Best-effort insert of one already-formatted line into `ring`. A poisoned
/// mutex (a previous panic held the lock) is recovered via `into_inner` rather
/// than `unwrap`, so a logging call can never panic. Returns `true` when a
/// poisoned lock had to be recovered.
fn push_ring(ring: &Mutex<VecDeque<String>>, line: String) -> bool {
    let (mut guard, recovered) = match ring.lock() {
        Ok(guard) => (guard, false),
        Err(poisoned) => (poisoned.into_inner(), true),
    };
    guard.push_back(line);
    while guard.len() > RING_CAPACITY {
        guard.pop_front();
    }
    recovered
}

/// Write one line to `writer`, returning the io error instead of panicking.
/// `eprintln!` panics on a broken stderr; `writeln!` does not, which is the
/// whole point of this indirection. Extracted so the "swallow the error"
/// contract is unit-testable with a failing writer.
fn write_line<W: std::io::Write>(writer: &mut W, line: &str) -> std::io::Result<()> {
    writeln!(writer, "{line}")
}

/// Panic-free stderr emission. A broken (or poisoned) stderr must never abort
/// the app, so the io error is swallowed and counted; `catch_unwind` contains a
/// residual panic. The installed panic hook is itself panic-free
/// ([`install_panic_hook`]), so this cannot recurse into an abort.
fn emit_stderr(line: &str) {
    let result = std::panic::catch_unwind(|| {
        let mut stderr = std::io::stderr();
        write_line(&mut stderr, line)
    });
    if !matches!(result, Ok(Ok(()))) {
        STDERR_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
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
        // R2-GUIMOD-07: emit first, then move the line into the ring — the
        // previous `push_back(line.clone())` duplicated every formatted
        // message on the hot logging path.
        //
        // Crash-Fix Runde 2: emission is panic-free (`emit_stderr`) and the
        // ring recovers a poisoned lock (`push_ring`). A logging failure is
        // never a crash reason.
        emit_stderr(&line);
        if push_ring(ring(), line) {
            RING_POISON_RECOVERIES.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn flush(&self) {}
}

/// Initialise stderr logging. Honours `RUST_LOG` for the level, falling back to
/// `default_level` (the binary passes `Info`; use `RUST_LOG=trace` for full
/// diagnosis, R2-GUIMOD-07). Returns the effective level (for diagnostics).
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

/// Snapshot of the recent log ring. Recovers a poisoned lock instead of
/// unwrapping, so it is safe to call from the panic hook.
fn recent_lines() -> Vec<String> {
    match ring().lock() {
        Ok(ring) => ring.iter().cloned().collect(),
        Err(poisoned) => poisoned.into_inner().iter().cloned().collect(),
    }
}

/// Install a panic hook that dumps the recent log ring buffer to stderr, so a
/// crash is analysable after stderr is redirected to a file.
///
/// Crash-Fix Runde 2: the hook MUST NOT panic — a second panic while the hook
/// runs aborts the process. Every operation here is panic-free by construction:
/// the ring lock recovers poison and the write goes through
/// [`emit_stderr`], which swallows io errors instead of panicking like
/// `eprintln!`.
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
        let recent = recent_lines();
        // Crash-Fix Runde 2: read the degraded-logging counters so a crash dump
        // also shows whether stderr/ring failures occurred before it.
        let degraded = format!(
            "\ndegraded logging: ring_poison_recoveries={} stderr_write_failures={}",
            RING_POISON_RECOVERIES.load(Ordering::Relaxed),
            STDERR_WRITE_FAILURES.load(Ordering::Relaxed),
        );
        emit_stderr(&format!(
            "PANIC at {loc}: {msg}\n--- last {n} log lines ---\n{lines}{degraded}",
            n = recent.len(),
            lines = recent.join("\n")
        ));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Crash-Fix Runde 2: a failing writer surfaces the io error as a value —
    /// it must NOT panic (the pre-fix `eprintln!` panicked here and took the
    /// process down via a recursive panic-hook panic).
    struct FailingWriter;

    impl std::io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stderr closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stderr closed",
            ))
        }
    }

    #[test]
    fn write_line_returns_the_io_error_instead_of_panicking() {
        assert!(write_line(&mut FailingWriter, "boom").is_err());
    }

    /// A poisoned ring lock is recovered (`into_inner`) instead of unwrapped;
    /// the line is still buffered and nothing panics.
    #[test]
    fn push_ring_recovers_a_poisoned_lock() {
        let ring = Mutex::new(VecDeque::new());
        let joined = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = ring.lock().unwrap();
                    panic!("poison the ring on purpose");
                })
                .join()
        });
        assert!(joined.is_err(), "the poisoning thread must have panicked");
        assert!(ring.is_poisoned(), "the lock must be poisoned");

        let recovered = push_ring(&ring, "after-poison".to_string());
        assert!(recovered, "a poisoned lock must be reported as recovered");
        let guard = ring.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(guard.front().map(String::as_str), Some("after-poison"));
    }

    /// The logger itself never panics and still records the line.
    #[test]
    fn log_never_panics_and_records_the_line() {
        let logger = StderrLogger {
            level: LevelFilter::Trace,
        };
        let record = Record::builder()
            .args(format_args!("logger-hardening-test-line"))
            .level(log::Level::Error)
            .target("logger::tests")
            .build();
        logger.log(&record);
        assert!(
            recent_lines()
                .iter()
                .any(|line| line.contains("logger-hardening-test-line")),
            "the log line must be buffered"
        );
    }
}
