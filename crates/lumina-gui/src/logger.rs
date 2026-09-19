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
use std::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

const RING_CAPACITY: usize = 512;

static RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

/// R3-LOG-1: wall-clock timestamp prefix for every log line (all levels), so a
/// manual `RUST_LOG=trace` run can order and delta the `trace!` events (switch
/// deferral, F1 throttle, `GUI timing:` lines) without a separate clock. Pure
/// `std` (no `chrono`): UTC `YYYY-MM-DDTHH:MM:SS.mmmZ`, computed from
/// `SystemTime::now()`.
fn timestamp_prefix() -> String {
    format_timestamp(SystemTime::now())
}

/// Pure formatter for [`timestamp_prefix`] (unit-tested with an injected time).
/// A pre-epoch clock (`duration_since` error) clamps to the Unix epoch rather
/// than panicking — a logging prefix must never take the app down.
fn format_timestamp(now: SystemTime) -> String {
    let millis = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = millis / 1000;
    let sub_millis = millis % 1000;
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let seconds_of_day = secs % 86_400;
    let (hour, minute, second) = (
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60,
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{sub_millis:03}Z")
}

/// Civil date from a count of days since 1970-01-01 (Howard Hinnant's
/// `civil_from_days` algorithm). UTC, no leap seconds — the standard proleptic
/// Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

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
            "[{}] [{}] {}: {}",
            timestamp_prefix(),
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

    /// R3-LOG-1: the timestamp prefix is a pure UTC formatter — pinned with
    /// injected times (epoch, a known civil instant, sub-second millis).
    #[test]
    fn timestamp_prefix_is_iso8601_utc() {
        assert_eq!(format_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_timestamp(UNIX_EPOCH + std::time::Duration::from_millis(1)),
            "1970-01-01T00:00:00.001Z"
        );
        // 1700000000 s = 2023-11-14T22:13:20Z (known Unix instant).
        assert_eq!(
            format_timestamp(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)),
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(
            format_timestamp(UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_123)),
            "2023-11-14T22:13:20.123Z"
        );
    }

    /// R3-LOG-1: every logged line (all levels) carries the timestamp prefix,
    /// so durationless `trace!` events stay orderable in a manual run. The
    /// regex pins the exact shape.
    #[test]
    fn log_line_carries_a_timestamp_prefix() {
        let logger = StderrLogger {
            level: LevelFilter::Trace,
        };
        let record = Record::builder()
            .args(format_args!("timestamp-prefix-test-line"))
            .level(log::Level::Trace)
            .target("logger::tests")
            .build();
        logger.log(&record);
        let line = recent_lines()
            .into_iter()
            .find(|line| line.contains("timestamp-prefix-test-line"))
            .expect("the line must be buffered");
        // `[YYYY-MM-DDTHH:MM:SS.mmmZ] [LEVEL] target: message`.
        let re_ok = {
            let bytes = line.as_bytes();
            bytes.len() > 25
                && bytes[0] == b'['
                && bytes[25] == b']'
                && line[1..25].chars().enumerate().all(|(i, c)| match i {
                    4 | 7 => c == '-',
                    10 => c == 'T',
                    13 | 16 => c == ':',
                    19 => c == '.',
                    23 => c == 'Z',
                    _ => c.is_ascii_digit(),
                })
        };
        assert!(re_ok, "timestamp prefix missing/malformed in {line:?}");
        assert!(line.contains("[TRACE] logger::tests: timestamp-prefix-test-line"));
    }

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
