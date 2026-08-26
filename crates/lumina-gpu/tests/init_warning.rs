//! Regression test for the "no silent fallback" contract of `GpuContext::new`.
//!
//! GUI-WGPU-PRESENT-1 finding: an adapter/device failure during GPU init used
//! to be swallowed. The fix routes the degradation through
//! [`lumina_gpu::log_gpu_init_failure`], which must emit a loud `warn!` so
//! headless or misconfigured machines stay diagnosable.
//!
//! The `log` crate allows one process-wide logger, so this test installs its
//! own capturing logger and drives the helper directly (forcing a real adapter
//! failure deterministically is not possible on machines where Metal/Vulkan
//! simply works).

use lumina_gpu::log_gpu_init_failure;
use std::sync::{Mutex, OnceLock};

/// Minimal capturing logger: records every warn-level message it sees.
struct CapturingLogger {
    records: Mutex<Vec<String>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.records.lock().expect("capture lock").push(format!(
                "{} {}",
                record.level(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

static CAPTURE: OnceLock<&'static CapturingLogger> = OnceLock::new();

/// Install (once) and return the process-wide capturing logger.
fn capture() -> &'static CapturingLogger {
    let leaked: &'static CapturingLogger = CAPTURE.get_or_init(|| {
        Box::leak(Box::new(CapturingLogger {
            records: Mutex::new(Vec::new()),
        }))
    });
    // The "racy" setter is explicitly safe to call concurrently; after the
    // first successful install it simply errors, which we ignore — if another
    // logger was somehow already installed we keep reading our own buffer
    // (the assert below then fails loudly instead of silently).
    unsafe {
        let _ = log::set_logger_racy(leaked);
    }
    log::set_max_level(log::LevelFilter::Warn);
    leaked
}

#[test]
fn gpu_init_failure_is_warned_not_swallowed() {
    let capture = capture();

    // Simulate exactly the error `init_gpu_resources` produces without a Metal
    // adapter and drive the extracted warning seam directly.
    let err = lumina_gpu::GpuError::AdapterUnavailable(
        "no Metal adapter found: test-synthesized failure".into(),
    );
    log_gpu_init_failure(&err);

    let records = capture.records.lock().expect("capture lock");
    assert!(
        records
            .iter()
            .any(|r| { r.contains("WARN") && r.contains("falling back to CPU rendering") }),
        "the adapter-failure degradation must emit a loud warning; got {records:?}"
    );
}
