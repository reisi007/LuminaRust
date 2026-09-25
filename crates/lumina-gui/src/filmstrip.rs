//! Filmstrip thumbnails for the Develop/Library browser.
//!
//! Per F-103-N1 the filmstrip is the bottom-of-window file browser in Develop;
//! it is driven by small previews.  Each source is tried against the on-disk
//! folder cache ([`lumina_core::cache::disk::DiskFolderCache`]) first; on a miss
//! a background job decodes/render the source at a small resolution, stores the
//! preview and uses it.  Until a thumbnail is ready a placeholder cell is shown
//! — there is deliberately no silent fallback to a wrong/sized-up image.
//!
//! Disk cache probes and RAW decode back the thumbnail jobs above; in-file
//! IO is a desktop capability
//! (`feature/platform/cli-gui-wasm.md` § WASM-ENTFERNT).

use eframe::egui;
// R2-MODSWITCH-1 F7: `filmstrip_preview_cached` is the test-only reference
// probe now (production goes through `thumb_cache::PreviewIndexCache`).
#[cfg(test)]
use lumina_core::cache::PreviewKind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Maximum edge length (px) of a generated filmstrip thumbnail.
///
/// Filmstrip cells are Lightroom-like 140x110 — a 160px cap left the texture
/// noticeably soft / "switching too small"; raised to 200px so the bigger cell
/// keeps crisp pixels.
pub const THUMBNAIL_MAX_DIM: u32 = 200;

/// Holds generated thumbnail textures and remembers which source/artifact
/// identities have already been probed against the disk cache (so unchanged
/// state enqueues a background job at most once per session).
///
/// Keys are **stable thumbnail keys** (canonicalized absolute paths — see
/// [`crate::thumbnail_key`]), never bare filenames: two folders may contain the
/// same filename and must never share a cell (REVIEW-GUI-THUMB-1).
///
/// Failure handling (REVIEW-GUI-THUMB-2): a worker that cannot decode/render a
/// source always reports back (`ThumbnailOutcome::Failed`). Failed sources are
/// retried up to [`THUMBNAIL_MAX_ATTEMPTS`] times and then keep a *visible*
/// error state in the cell — there is deliberately no silent gray fallback for
/// the rest of the session.
#[derive(Default)]
pub struct ThumbnailManager {
    textures: BTreeMap<String, egui::TextureHandle>,
    /// Keys with a successfully produced texture.
    probed: BTreeSet<String>,
    /// Keys with an enqueued job whose worker result has not arrived yet, and
    /// the exact identity that job is allowed to complete.
    in_flight: BTreeMap<String, Option<crate::source_actions::SidecarBundleIdentity>>,
    /// Key -> (last error message, decode attempts so far).
    failed: BTreeMap<String, (String, u32)>,
    /// Deterministic sidecar+zdata content identity captured when a key was
    /// last scheduled. Changed bytes invalidate every old state; unchanged
    /// bytes preserve the same-session hit.
    source_identities: BTreeMap<String, crate::source_actions::SidecarBundleIdentity>,
    /// Keys whose source identity changed since the last schedule. The next
    /// job bypasses the recipe-blind disk preview and rebuilds from source.
    source_rebuilds: BTreeSet<String>,
    /// Directory the cached entries belong to; switching it clears the cache
    /// so thumbnails of a previous folder can neither linger nor grow without
    /// bound (REVIEW-GUI-THUMB-1).
    directory: Option<String>,
}

/// Maximum decode attempts per thumbnail before the cell shows the persistent,
/// visible failure state instead of retrying forever.
pub const THUMBNAIL_MAX_ATTEMPTS: u32 = 3;

impl ThumbnailManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the deterministic identity for one source. Exact source-image,
    /// sidecar, and referenced source-action bytes are hashed; mtime and length
    /// never authorize reuse.
    pub(crate) fn source_identity(source: &Path) -> crate::source_actions::SidecarBundleIdentity {
        crate::source_actions::sidecar_bundle_identity(
            source,
            crate::thumb_cache::THUMB_VIRTUAL_COPY,
        )
    }

    /// Refresh the persisted-artifact identity for `key` and return it.
    ///
    /// This is deliberately an explicit lifecycle hook rather than a new
    /// filename-derived key: callers can keep the stable path key used by the
    /// Library and neighbor scheduler, while a changed sidecar/bundle drops
    /// every old texture, retry, and in-flight marker before it can be read. A
    /// pre-existing cache with no captured identity is treated conservatively;
    /// it cannot be proven current.
    pub(crate) fn refresh_source(
        &mut self,
        key: &str,
        source: &Path,
    ) -> crate::source_actions::SidecarBundleIdentity {
        let identity = Self::source_identity(source);
        let changed = match self.source_identities.get(key) {
            Some(previous) => previous != &identity,
            None => {
                // The source-image hash is part of every identity, even when
                // no sidecar or zdata exists. A source-action-aware identity
                // also starts untrusted: its legacy disk record has no
                // source-content identity and must be rebuilt once.
                self.textures.contains_key(key)
                    || self.in_flight.contains_key(key)
                    || self.failed.contains_key(key)
                    || identity.has_source_action_bundle()
            }
        };
        if changed {
            self.clear_key_state(key);
            self.source_rebuilds.insert(key.to_owned());
        }
        self.source_identities
            .insert(key.to_owned(), identity.clone());
        identity
    }

    /// Drop all state associated with one stable source key. The identity is
    /// retained by [`Self::refresh_source`] when this is an automatic change.
    fn clear_key_state(&mut self, key: &str) {
        self.textures.remove(key);
        self.probed.remove(key);
        self.in_flight.remove(key);
        self.failed.remove(key);
    }

    /// Drop every thumbnail pixel, retry, and in-flight marker. Decode
    /// conflicts use this fail-closed reset so a worker that was already
    /// running cannot repopulate a texture after the active source was kept.
    pub(crate) fn invalidate_all(&mut self) {
        self.source_rebuilds.extend(
            self.source_identities
                .keys()
                .cloned()
                .chain(self.textures.keys().cloned())
                .chain(self.in_flight.keys().cloned())
                .chain(self.failed.keys().cloned()),
        );
        self.textures.clear();
        self.probed.clear();
        self.in_flight.clear();
        self.failed.clear();
        self.source_identities.clear();
        self.directory = None;
    }

    pub(crate) fn source_rebuild_pending(&self, key: &str) -> bool {
        self.source_rebuilds.contains(key)
    }

    /// Clear the barrier only after a fresh worker result has been accepted.
    pub(crate) fn clear_source_rebuild(&mut self, key: &str) {
        self.source_rebuilds.remove(key);
    }

    /// Whether a worker result still belongs to the current persisted state.
    /// A result that raced an external sidecar/bundle write is discarded
    /// rather than being inserted under the unchanged path key.
    pub(crate) fn accepts_result(
        &mut self,
        key: &str,
        source: &Path,
        enqueued_identity: &crate::source_actions::SidecarBundleIdentity,
        prepared_identity: Option<&crate::source_actions::SidecarBundleIdentity>,
    ) -> bool {
        let current = self.refresh_source(key, source);
        let matches = &current == enqueued_identity
            && prepared_identity.is_none_or(|identity| identity == &current);
        let owns_in_flight = self
            .in_flight
            .get(key)
            .is_some_and(|active| active.as_ref() == Some(enqueued_identity));
        if !matches && owns_in_flight {
            self.clear_key_state(key);
        }
        matches && owns_in_flight
    }

    /// Return the cached texture for a key, if one has been produced.
    pub fn get(&self, key: &str) -> Option<&egui::TextureHandle> {
        self.textures.get(key)
    }

    /// Store a freshly produced texture. This is the only path that marks a
    /// source as finally probed (REVIEW-GUI-THUMB-2: probed is set on success
    /// only, never on enqueue).
    pub fn insert(&mut self, key: &str, texture: egui::TextureHandle) {
        self.textures.insert(key.to_owned(), texture);
        self.probed.insert(key.to_owned());
        self.in_flight.remove(key);
        self.failed.remove(key);
        // A rebuild marker is an untrusted-disk-cache barrier. Only a fresh
        // resolver-backed worker result reaches this method; a failed retry
        // must leave the marker in place so legacy pixels cannot return.
        self.clear_source_rebuild(key);
    }

    /// Whether another job for this key must NOT be enqueued right now:
    /// either a texture exists, a job is in flight, or the retry budget
    /// ([`THUMBNAIL_MAX_ATTEMPTS`]) is exhausted.
    pub fn needs_job(&self, key: &str) -> bool {
        !self.textures.contains_key(key)
            && !self.in_flight.contains_key(key)
            && self.attempts(key) < THUMBNAIL_MAX_ATTEMPTS
    }

    /// Register an enqueued job (called after a successful channel send).
    pub fn begin_job(&mut self, key: &str) {
        self.in_flight
            .insert(key.to_owned(), self.source_identities.get(key).cloned());
    }

    /// Release the in-flight slot because the job could not be dispatched
    /// (channel closed); the caller retries on a later frame.
    pub fn job_dispatch_failed(&mut self, key: &str) {
        self.in_flight.remove(key);
    }

    /// Record a worker failure with its visible error message and consume one
    /// unit of the retry budget.
    pub fn mark_failed(&mut self, key: &str, message: impl Into<String>) {
        self.in_flight.remove(key);
        // A failed refresh must not leave the previous recipe-only/raw texture
        // paintable beside the error badge. The bounded retry can rebuild it
        // from the current source/action identity.
        self.textures.remove(key);
        self.probed.remove(key);
        let attempts = self.attempts(key) + 1;
        self.failed
            .insert(key.to_owned(), (message.into(), attempts));
    }

    /// The visible failure message for a key, if it has permanently failed.
    pub fn failure(&self, key: &str) -> Option<&str> {
        self.failed.get(key).map(|(message, _)| message.as_str())
    }

    fn attempts(&self, key: &str) -> u32 {
        self.failed.get(key).map_or(0, |(_, attempts)| *attempts)
    }

    /// Whether this key has already been completed successfully (test-only
    /// introspection; the UI checks [`Self::get`] instead).
    #[cfg(test)]
    pub fn probed(&self, key: &str) -> bool {
        self.probed.contains(key)
    }

    /// Drop all cached state when the browsed directory changes; a no-op while
    /// the same directory keeps being re-listed (e.g. after a sidecar save).
    pub fn ensure_directory(&mut self, directory: &str) {
        if self.directory.as_deref() != Some(directory) {
            self.textures.clear();
            self.probed.clear();
            self.in_flight.clear();
            self.failed.clear();
            self.source_identities.clear();
            self.source_rebuilds.clear();
            self.directory = Some(directory.to_owned());
        }
    }
}

/// Nearest-neighbour downscale of an RGBA8 buffer to at most `max_dim` on its
/// longest edge.  Returns the original buffer unchanged when it is already
/// smaller or one dimension is zero.
pub fn downscale_rgba(pixels: &[u8], width: u32, height: u32, max_dim: u32) -> (Vec<u8>, u32, u32) {
    if width == 0 || height == 0 {
        return (pixels.to_vec(), width, height);
    }
    let longest = width.max(height);
    if longest <= max_dim {
        return (pixels.to_vec(), width, height);
    }
    let scale = max_dim as f64 / longest as f64;
    let new_width = (width as f64 * scale).max(1.0).round() as u32;
    let new_height = (height as f64 * scale).max(1.0).round() as u32;
    let mut out = vec![0u8; (new_width * new_height * 4) as usize];
    for y in 0..new_height {
        for x in 0..new_width {
            let sx = (x as f64 / new_width as f64 * width as f64) as u32;
            let sy = (y as f64 / new_height as f64 * height as f64) as u32;
            let src = ((sy * width + sx) * 4) as usize;
            let dst = ((y * new_width + x) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    (out, new_width, new_height)
}

/// Test-only reference cache probe: is a standard preview already on disk?
///
/// A `None` result (I/O or settings gate) is treated as "not cached" so the
/// caller falls back to generating a thumbnail rather than assuming a stale hit.
/// Production no longer probes the cache on the UI thread
/// (R2-MODSWITCH-1 F7: `crate::thumb_cache::PreviewIndexCache` does the
/// metadata-only probe; the worker owns load/decode); this helper pins the
/// core cache layout for the tests.
#[cfg(test)]
pub fn filmstrip_preview_cached(
    cache: &lumina_core::cache::disk::DiskFolderCache,
    source: &str,
    virtual_copy: &str,
) -> bool {
    cache
        .load_preview(source, virtual_copy, PreviewKind::Standard)
        .map(|loaded| loaded.is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(ctx: &egui::Context, id: &str) -> egui::TextureHandle {
        ctx.load_texture(
            format!("test-{id}"),
            egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]),
            egui::TextureOptions::LINEAR,
        )
    }

    /// REVIEW-GUI-THUMB-1: identical filenames in different folders must map to
    /// different manager entries.
    #[test]
    fn keys_are_path_scoped_not_name_scoped() {
        let mut manager = ThumbnailManager::new();
        let ctx = egui::Context::default();
        manager.insert("/a/IMG_0001.ARW", texture(&ctx, "a"));
        assert!(manager.get("/b/IMG_0001.ARW").is_none());
        assert!(manager.get("/a/IMG_0001.ARW").is_some());
    }

    /// REVIEW-GUI-THUMB-2: a failed worker must not strand the cell as a gray
    /// placeholder for the session; the source is retried up to
    /// [`THUMBNAIL_MAX_ATTEMPTS`] times and then shows a visible error.
    #[test]
    fn worker_failure_retries_bounded_then_visible_error() {
        let mut manager = ThumbnailManager::new();
        assert!(manager.needs_job("k"));
        manager.begin_job("k");
        // While in flight no duplicate job is enqueued.
        assert!(!manager.needs_job("k"));
        for attempt in 1..THUMBNAIL_MAX_ATTEMPTS {
            manager.mark_failed("k", format!("boom {attempt}"));
            assert!(manager.needs_job("k"), "attempt {attempt} must retry");
            manager.begin_job("k");
        }
        manager.mark_failed("k", "boom final");
        // Retry budget exhausted: no more jobs, but a visible error remains.
        assert!(!manager.needs_job("k"));
        assert_eq!(manager.failure("k"), Some("boom final"));
    }

    /// REVIEW-GUI-THUMB-2: only a successful insert marks a key probed and
    /// clears any earlier failure.
    #[test]
    fn insert_marks_probed_and_clears_failure() {
        let mut manager = ThumbnailManager::new();
        manager.begin_job("k");
        manager.mark_failed("k", "first failure");
        assert!(!manager.probed("k"));
        let ctx = egui::Context::default();
        manager.insert("k", texture(&ctx, "done"));
        assert!(manager.probed("k"));
        assert_eq!(manager.failure("k"), None);
        assert!(!manager.needs_job("k"));
    }

    /// REVIEW-GUI-THUMB-1: switching directories drops stale entries; relisting
    /// the same directory (e.g. after a sidecar save) keeps them.
    #[test]
    fn ensure_directory_clears_only_on_change() {
        let mut manager = ThumbnailManager::new();
        let ctx = egui::Context::default();
        manager.ensure_directory("/a");
        manager.insert("/a/x.png", texture(&ctx, "x"));
        manager.ensure_directory("/a");
        assert!(manager.get("/a/x.png").is_some());
        manager.ensure_directory("/b");
        assert!(manager.get("/a/x.png").is_none());
        assert!(!manager.probed("/a/x.png"));
    }

    /// A job that could not be dispatched releases its in-flight slot so the
    /// next frame retries instead of stranding the key forever.
    #[test]
    fn dispatch_failure_allows_retry() {
        let mut manager = ThumbnailManager::new();
        manager.begin_job("k");
        assert!(!manager.needs_job("k"));
        manager.job_dispatch_failed("k");
        assert!(manager.needs_job("k"));
        assert_eq!(manager.failure("k"), None);
    }
}
