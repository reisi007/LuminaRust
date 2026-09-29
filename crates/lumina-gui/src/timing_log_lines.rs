//! Pure log-line builders — the single source of truth for every R3-LOG-1
//! trace format (file-size-ratchet extraction from `timing.rs`).
//!
//! Extracted as its own module because these are the one cohesive block in
//! `timing.rs`: every function here is **pure** — no `LuminaApp` state, no
//! throttle, no side effect — and each one exists only to render a line. The
//! format therefore lives in one place that a reader can check against a
//! captured log, and the stateful half of the instrumentation (anchors,
//! throttles, emission) stays behind in `timing.rs` next to the state it
//! drives.
//!
//! Relocation only: `timing.rs` re-exports every builder, so no call site
//! changed. The doc comments move with their functions because several of them
//! *are* the specification of a gate (e.g. THUMB-HASH-PERF-35's counted hash).

use std::path::Path;

use super::timing::format_ms;
use super::Module;

pub(crate) fn module_switch_event_line(module: Module) -> String {
    format!("GUI timing: module switch event module={module:?}")
}

pub(crate) fn module_first_paint_line(module: Module, ms: f64) -> String {
    format!(
        "GUI timing: module switch first paint module={module:?} switch_to_paint_ms={}",
        format_ms(ms)
    )
}

pub(crate) fn decode_start_line(path: &str) -> String {
    format!("GUI timing: decode start path={path}")
}

pub(crate) fn decode_done_line(path: &str, ms: f64, width: u32, height: u32) -> String {
    format!(
        "GUI timing: decode done path={path} decode_ms={} resolution={width}x{height}",
        format_ms(ms)
    )
}

pub(crate) fn decode_failed_line(path: &str, ms: f64) -> String {
    format!(
        "GUI timing: decode failed path={path} decode_ms={}",
        format_ms(ms)
    )
}

pub(crate) fn preview_index_line(folder: &Path, entries: usize, ms: f64) -> String {
    format!(
        "GUI timing: preview index built folder={} entries={entries} build_ms={}",
        folder.display(),
        format_ms(ms)
    )
}

/// THUMB-HASH-PERF-35: one whole-file source identity that was really computed
/// (a memo miss). `hash_ms` is the wall clock of the read+BLAKE3 pass, so a
/// manual `RUST_LOG=trace` acceptance run (Agents.md R5-LOG-1) can *count* the
/// hashes a browse session spent and see that they stop after the first frame.
pub(crate) fn source_identity_hashed_line(path: &Path, bytes: u64, ms: f64) -> String {
    format!(
        "GUI source identity hashed (cache miss) path={} bytes={bytes} hash_ms={}",
        path.display(),
        format_ms(ms)
    )
}

/// R4-SWITCH-2: the depth-limited RAW count of one folder-tree node. This walk
/// runs synchronously on the UI thread the first time a node is shown and was
/// the uninstrumented block behind the first Library paint; the line makes it
/// visible in the trace (`files` is the counted number, not the walk size).
pub(crate) fn folder_scan_line(path: &Path, files: usize, ms: f64) -> String {
    format!(
        "GUI timing: folder raw count folder={} files={files} scan_ms={}",
        path.display(),
        format_ms(ms)
    )
}

/// R4-SWITCH-2: the one-shot cold-start warmup was armed at native startup.
pub(crate) fn warmup_armed_line() -> String {
    "GUI timing: warmup armed".to_string()
}

/// R4-SWITCH-2: the armed warmup was deferred this frame; `reason` is the
/// concrete gate (`pointer down`, `no listing yet`) so a late warmup is
/// explainable from the trace instead of an uninstrumented gap.
pub(crate) fn warmup_deferred_line(reason: &str) -> String {
    format!("GUI timing: warmup deferred reason={reason}")
}

pub(crate) fn thumbnail_ready_line(key: &str, ms: f64) -> String {
    format!(
        "GUI timing: thumbnail ready key={key} enqueue_to_ready_ms={}",
        format_ms(ms)
    )
}

pub(crate) fn full_render_line(ms: f64, width: u32, height: u32) -> String {
    format!(
        "GUI timing: full render done render_ms={} output={width}x{height}",
        format_ms(ms)
    )
}

pub(crate) fn texture_upload_line(target: &str, bytes: usize) -> String {
    format!("GUI timing: texture upload target={target} bytes={bytes}")
}

pub(crate) fn texture_upload_skip_line(target: &str, saved_bytes: usize) -> String {
    format!("GUI timing: texture upload skipped target={target} saved_bytes={saved_bytes}")
}
