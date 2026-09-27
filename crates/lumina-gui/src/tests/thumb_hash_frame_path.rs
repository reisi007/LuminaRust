//! THUMB-HASH-PERF-35, second anchor: the **real frame path** reaches the
//! memoized whole-file source identity, and reaches it once per file state —
//! not once per visible cell per frame.
//!
//! ## Why this file exists next to `source_identity_cache.rs`
//!
//! The memo's own semantics are pinned there, but its only "no re-hash"
//! assertion (`unchanged_file_is_hashed_once_across_repeated_lookups`) drives
//! [`crate::source_actions::FileContentIdentity::from_path`] **directly**, in a
//! loop that stands in for frames. That loop calls the memoized *leaf*; it
//! never enters the app. So a change **above** the leaf — a `std::fs::read` of
//! the whole source reintroduced in
//! [`crate::filmstrip::ThumbnailManager::refresh_source`] or in
//! `crate::source_actions::sidecar_bundle_identity`, a frame path that stopped
//! consulting the memo at all, or one that consults it under a second stat key
//! — would not be seen by any existing test. The count is only meaningful if
//! something real reaches the memo.
//!
//! This file closes that gap by driving the actual entry point a frame uses.
//! It hangs off [`super::scheduling`], the filmstrip thumbnail-scheduling test
//! module, so it sits next to the other `ensure_thumbnail_priority` tests
//! without growing the ratcheted `lib.rs`.
//!
//! ## The frame path driven here
//!
//! `LuminaApp::draw_filmstrip` ends every filmstrip frame with
//! `self.ensure_thumbnail_priority(ctx, &raw_indices, visible)`
//! (`filmstrip_frame.rs:223`, the last statement of the frame body). This test
//! calls that same `pub(crate)` entry point with the same
//! [`LuminaApp::raw_entry_indices`] (the one shared index source
//! `draw_filmstrip` builds at `filmstrip_frame.rs:26`) 13 times in a row,
//! passing a visible window that covers the whole strip — so every fixture cell
//! is in the buffered visible range and the bounded off-screen prefetch pass is
//! a no-op (`prefetch_order(3, 0..3)` is empty). Nothing about the chain is
//! reimplemented:
//!
//! ```text
//! ensure_thumbnail_priority   (filmstrip_frame.rs:267)  <- driven here, N times
//!  └─ ensure_thumbnail        (filmstrip_frame.rs:319)
//!     └─ refresh_source       (filmstrip.rs:92)
//!         └─ source_identity  (filmstrip.rs:77)
//!             └─ sidecar_bundle_identity / persisted_action_identity
//!                                  (sidecar_snapshot.rs:234 / :197)
//!                 └─ FileContentIdentity::from_path   (source_actions.rs:55)
//!                     └─ source_identity::content_hash  (source_identity.rs:220)
//!                         └─ resolve()  <- the memoized leaf (source_identity.rs:289)
//! ```
//!
//! ## Every assertion here is a count, never a sleep
//!
//! Same rule as `source_identity_cache.rs`: the memo records how many
//! whole-file hashes it spent, per stat key, so "an unchanged file costs one
//! hash over N frames" is an exact integer comparison and not a wall-clock
//! guess. The counter is read through [`crate::source_identity::hash_count`],
//! the same `#[cfg(test)]` seam the memo-semantics tests use.
//!
//! ## Measured boundary of this anchor — what it does and does not catch
//!
//! Both directions of the count assertion were proven falsifiable by mutation
//! on 2026-09-27 (build-agent run, reverted afterwards; the numbers are the
//! failure output, not a prediction):
//!
//! * **Caught — memo bypass.** Restoring the pre-`THUMB-HASH-PERF-35` whole-file
//!   read *instead of* the memoized identity on the frame path
//!   (`ThumbnailManager::source_identity` → `std::fs::read` + BLAKE3) leaves
//!   the per-key count at `None`: `left: [None, None, None]` vs
//!   `right: [Some(1), Some(1), Some(1)]`. The eight `source_identity_cache`
//!   tests stayed **green** in the same run — they call the leaf directly and
//!   never enter the app. That is the gap this file closes.
//! * **Caught — re-hash per frame.** Removing the memo *lookup* in
//!   `source_identity::resolve` (every lookup re-hashes and re-stores) makes
//!   the second frame read `left: [Some(2), Some(2), Some(2)]`.
//! * **NOT caught — a duplicated *uncounted* read above the leaf.** Adding
//!   `let _ = std::fs::read(source);` to `ThumbnailManager::refresh_source`
//!   leaves this test **green**, and it was measured green: a bypass that does
//!   not go through `resolve()` never reaches the memo and therefore never moves
//!   the per-key counter. No memo-miss counter can see it — not this one and not
//!   a new one — so it is named here as an open residual rather than claimed
//!   as covered (DoD §9/§10). Closing it would need a byte/IO seam at the read
//!   site itself, i.e. production instrumentation, which is out of scope for a
//!   test-only change.

use super::super::source_identity_cache::payload;
use super::super::*;

/// Frames driven after the first one. The brief's floor is 10; 12 keeps the
/// run comfortably under a second (3 visible cells x 12 frames against memo
/// hits is pure map lookups) while making "once per frame" unmistakable.
const FRAMES_AFTER_FIRST: usize = 12;

/// Visible cells per frame. The motivating bug was measured at three visible
/// cells, so the frame path is driven with three: one frame therefore performs
/// three lookups of the frame's visible window.
const VISIBLE_CELLS: usize = 3;

/// Fixture bytes per source. The pre-fix cost was ~115 ms per whole-file hash
/// of a 12 MB RAW; ~300 KB keeps the fixture honest (a re-introduced full read
/// really is a multi-chunk read, not a single in-cache page) while the whole
/// test stays fast and deterministic. Deterministic filler, no RNG, no timing.
const SOURCE_BYTES: usize = 300 * 1024;

/// A `LuminaApp` browsing a tempdir of three real, non-trivial RAW-named
/// sources, plus the entry indices `draw_filmstrip` would hand to
/// `ensure_thumbnail_priority`.
struct Frame {
    app: LuminaApp,
    _dir: tempfile::TempDir,
    indices: Vec<usize>,
    /// The exact source paths as the app holds them, so the memo is read on
    /// the same path string the frame path hashed (the key carries the path
    /// verbatim — see `FileStatKey`).
    sources: Vec<PathBuf>,
}

impl Frame {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bytes = payload(SOURCE_BYTES, 0x5A);
        for index in 0..VISIBLE_CELLS {
            // A `.arw` name makes the entry RAW, which is what
            // `raw_entry_indices` (the filmstrip's index source) keeps. The
            // bytes are deterministic filler, not a decodable RAW: this test
            // pins the *identity* work a frame does on the UI thread, and
            // decoding happens in the worker pool, never here — the same split
            // `tests::modswitch` relies on for its fabricated-RAW entries.
            std::fs::write(dir.path().join(format!("frame{index}.arw")), &bytes).unwrap();
        }
        let mut app = new_app();
        app.set_directory(dir.path().display().to_string());
        let indices = app.raw_entry_indices();
        assert_eq!(
            indices.len(),
            VISIBLE_CELLS,
            "every fixture source must reach the filmstrip's RAW index"
        );
        let sources: Vec<PathBuf> = indices
            .iter()
            .map(|&index| app.entries()[index].path.clone())
            .collect();
        assert!(
            sources.iter().all(|path| path.is_file()),
            "the frame path must resolve to real files: {sources:?}"
        );
        Self {
            app,
            _dir: dir,
            indices,
            sources,
        }
    }

    /// One filmstrip frame's scheduling call, exactly as `draw_filmstrip` ends
    /// a frame (`filmstrip_frame.rs:223`): the whole strip is the visible
    /// window here, so every fixture cell is in the buffered visible range.
    fn frame(&mut self, ctx: &egui::Context) {
        self.app
            .ensure_thumbnail_priority(ctx, &self.indices, 0..self.indices.len());
    }

    /// Whole-file hashes the memo has spent per source, by path.
    fn hashes(&self) -> Vec<Option<u64>> {
        self.sources
            .iter()
            .map(|path| crate::source_identity::hash_count(path))
            .collect()
    }
}

/// The frame path must reach the memoized leaf, and must cost exactly **one**
/// whole-file hash per source for the whole run — no matter how many frames
/// and how many visible cells look at it.
///
/// The falsifiable part is the *count*, and it is falsifiable in both
/// directions:
///
/// * a frame path that stopped consulting the memo (a reintroduced
///   `std::fs::read` + BLAKE3 **instead of** the memoized identity, or a
///   bypass around `refresh_source`) leaves the per-key count at `None`
///   forever — the `Some(1)` assertions below fail;
/// * a frame path that consults the memo **more** than once per file state
///   (a second stat key per frame, a per-frame re-store) pushes the count
///   above `1` — the `Some(1)` assertions below fail.
///
/// Only the leaf-direct loop in `source_identity_cache.rs` existed before, and
/// it drives neither of those.
#[test]
fn real_frame_path_hashes_an_unchanged_source_once_across_many_frames() {
    let mut frame = Frame::new();
    let ctx = egui::Context::default();

    // Nothing may have been hashed before the first frame: this pins the
    // baseline so a later "still one hash" is a real statement and not an
    // artefact of a hash that some other step already paid for.
    assert_eq!(
        frame.hashes(),
        vec![None; VISIBLE_CELLS],
        "no source may be hashed before the first frame"
    );

    // The first frame pays for the cold pass: exactly one hash per source.
    // `prefetch_order(3, 0..3)` is empty (see `viewport`'s
    // `prefetch_order_fully_visible_yields_nothing`), so the whole frame
    // resolves each visible source's identity exactly once, through
    // `ensure_thumbnail` -> `refresh_source`.
    frame.frame(&ctx);
    assert_eq!(
        frame.hashes(),
        vec![Some(1); VISIBLE_CELLS],
        "the cold frame must cost exactly one whole-file hash per source"
    );

    // Every further frame over the unchanged sources must add nothing.
    for index in 0..FRAMES_AFTER_FIRST {
        frame.frame(&ctx);
        assert_eq!(
            frame.hashes(),
            vec![Some(1); VISIBLE_CELLS],
            "frame {} of an unchanged source must not re-hash it (THUMB-HASH-PERF-35)",
            index + 2
        );
    }
}
