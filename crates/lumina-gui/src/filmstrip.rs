//! Filmstrip thumbnails for the Develop/Library browser.
//!
//! Per F-103-N1 the filmstrip is the bottom-of-window file browser in Develop;
//! it is driven by small previews.  Each source is tried against the on-disk
//! folder cache ([`lumina_core::cache::disk::DiskFolderCache`]) first; on a miss
//! a background job decodes/render the source at a small resolution, stores the
//! preview and uses it.  Until a thumbnail is ready a placeholder cell is shown
//! — there is deliberately no silent fallback to a wrong/sized-up image.
//!
//! The disk cache and native RAW decode are gated to `not(target_arch =
//! "wasm32")`; under wasm the filmstrip shows placeholders only (RAW/in-file IO
//! are a documented native capability).

use eframe::egui;
use lumina_core::cache::PreviewKind;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum edge length (px) of a generated filmstrip thumbnail.
pub const THUMBNAIL_MAX_DIM: u32 = 160;

/// Holds generated thumbnail textures and remembers which sources have already
/// been probed against the disk cache (so we enqueue a background job at most
/// once per source).
#[derive(Default)]
pub struct ThumbnailManager {
    textures: BTreeMap<String, egui::TextureHandle>,
    probed: BTreeSet<String>,
}

impl ThumbnailManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached texture for a source, if one has been produced.
    pub fn get(&self, source: &str) -> Option<&egui::TextureHandle> {
        self.textures.get(source)
    }

    /// Store a freshly produced texture.
    pub fn insert(&mut self, source: &str, texture: egui::TextureHandle) {
        self.textures.insert(source.to_owned(), texture);
    }

    /// Whether this source has already been probed against the disk cache.
    pub fn probed(&self, source: &str) -> bool {
        self.probed.contains(source)
    }

    /// Mark a source as probed (so we don't probe it again next frame).
    pub fn mark_probed(&mut self, source: &str) {
        self.probed.insert(source.to_owned());
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

/// Headless-testable cache probe: is a standard preview already on disk?
///
/// A `None` result (I/O or settings gate) is treated as "not cached" so the
/// caller falls back to generating a thumbnail rather than assuming a stale hit.
#[cfg(not(target_arch = "wasm32"))]
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
