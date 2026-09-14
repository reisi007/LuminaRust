#![allow(clippy::identity_op)]
#![allow(clippy::field_reassign_with_default)]
//! Generative canvas + keep_generative_content logic (GEN-FILL-03).
//! Plus GEN-FILL-01 heuristic auto-fill for transparent pixels after lens correction.

use std::cell::RefCell;
use std::collections::HashMap;

use log::trace;
use lumina_sidecar::{Crop, GenerativeCanvas, GenerativeEdit};

use crate::{CoreError, ImageFrame};

pub fn has_transparent_pixels(frame: &ImageFrame) -> bool {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0))
}

// ---------------------------------------------------------------------------
// GEN-EXPAND-CACHE-1: persistent-free expand/auto-fill result cache.
//
// The heuristic fill (`fill_transparent_heuristic`) is a sequential global BFS
// that dominates the render cost of the generative stage. The user condition
// (2026-09-14) is explicit: the one-time expand is fine, but it must not run on
// every rendering. This cache reuses a completed BFS result for an identical
// generative identity.
//
// Identity (analogous to the AI-mask identity in `Agents.md`) is *complete*: it
// is the digest of the exact BFS input frame (which already folds in source
// content, decode context and the full geometry/lens/perspective context)
// combined with the role discriminator, the `seed` and the target `canvas`.
// Any change to source, decode, recipe, seed or canvas therefore produces a
// different key => a miss => a loud recomputation. A stale result can never be
// served silently: there is no timestamp/partial-match lookup, only exact
// identity equality.
//
// The cache is deliberately RAM-only and process-local (per thread). It is a
// pure performance layer, fully deletable and rebuildable from source+recipe:
// no new on-disk format is invented. The already existing `.lumina.zdata`
// `kind = 2` record (with its recipe link) remains the durable artifact format;
// its identity verification lives in `lumina-sidecar` (see
// `generative_artifact_status`).
// ---------------------------------------------------------------------------

/// Which generative operation a cached result belongs to. Auto-fill and expand
/// share the same BFS but must never serve each other's result, even if the
/// input digest and seed were identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerativeRole {
    /// `auto_fill_transparent`: fills transparent pixels after lens correction.
    AutoFillTransparent,
    /// `expand_beyond_image`: composites the source into a larger canvas and
    /// fills the expanded border.
    Expand,
}

impl GenerativeRole {
    fn tag(self) -> u8 {
        match self {
            Self::AutoFillTransparent => 1,
            Self::Expand => 2,
        }
    }
}

/// Complete identity of one BFS run. Two keys with equal [`Self::digest`] are
/// guaranteed to describe the exact same BFS input and parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerativeCacheKey {
    pub role: GenerativeRole,
    pub seed: u64,
    /// Target canvas `(output_width, output_height, source_offset_x,
    /// source_offset_y)` for [`GenerativeRole::Expand`]; `None` for auto-fill.
    pub canvas: Option<(u32, u32, i32, i32)>,
    /// BLAKE3 digest of the exact input frame the BFS consumes (dimensions +
    /// RGBA8 pixels).
    pub input_digest: String,
}

impl GenerativeCacheKey {
    /// Identity of an auto-fill run over `frame` with `seed`.
    #[must_use]
    pub fn auto_fill(frame: &ImageFrame, seed: u64) -> Self {
        Self {
            role: GenerativeRole::AutoFillTransparent,
            seed,
            canvas: None,
            input_digest: generative_input_digest(frame),
        }
    }

    /// Identity of an expand run that composites `frame` into `canvas` with
    /// `seed`.
    #[must_use]
    pub fn expand(frame: &ImageFrame, canvas: &GenerativeCanvas, seed: u64) -> Self {
        Self {
            role: GenerativeRole::Expand,
            seed,
            canvas: Some((
                canvas.output_width,
                canvas.output_height,
                canvas.source_offset_x,
                canvas.source_offset_y,
            )),
            input_digest: generative_input_digest(frame),
        }
    }

    /// Stable cache digest; every identity component participates.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"generative-cache");
        hasher.update(&[self.role.tag()]);
        hasher.update(&self.seed.to_le_bytes());
        match self.canvas {
            None => {
                hasher.update(&[0]);
            }
            Some((w, h, ox, oy)) => {
                hasher.update(&[1]);
                hasher.update(&w.to_le_bytes());
                hasher.update(&h.to_le_bytes());
                hasher.update(&ox.to_le_bytes());
                hasher.update(&oy.to_le_bytes());
            }
        }
        hasher.update(self.input_digest.as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// BLAKE3 digest of the exact BFS input frame. Folding the pixels in makes the
/// cache identity independent of *how* the frame was produced (source decode,
/// lensfun/manual lens, perspective): identical pixels always share a result,
/// different pixels always miss.
#[must_use]
pub fn generative_input_digest(frame: &ImageFrame) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"generative-input");
    hasher.update(&frame.width.to_le_bytes());
    hasher.update(&frame.height.to_le_bytes());
    hasher.update(&frame.pixels);
    hasher.finalize().to_hex().to_string()
}

/// Observability counters for the generative cache. `bfs_runs` counts every
/// actual `fill_transparent_heuristic` invocation (cache miss); a second render
/// with identical identity leaves it unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenerativeCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub bfs_runs: u64,
    pub entries: usize,
    pub used_bytes: usize,
}

#[derive(Debug)]
struct GenerativeCacheSlot {
    frame: ImageFrame,
    stamp: u64,
}

/// Byte-budgeted LRU cache mapping [`GenerativeCacheKey`] digests to completed
/// BFS results. Clone-on-read so callers can mutate the returned frame without
/// poisoning the cached result.
#[derive(Debug)]
pub struct GenerativeCache {
    entries: HashMap<String, GenerativeCacheSlot>,
    max_bytes: usize,
    used_bytes: usize,
    clock: u64,
    stats: GenerativeCacheStats,
}

impl Default for GenerativeCache {
    fn default() -> Self {
        // Multiple full-resolution canvases (~180 MB for 45 MP RGBA8) fit; the
        // LRU evicts beyond that. A single frame larger than the budget is
        // refused (`insert` returns `false`) and the render continues correctly
        // without it — a documented capacity limit, not a silent fallback.
        Self::new(1_500_000_000)
    }
}

impl GenerativeCache {
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_bytes,
            used_bytes: 0,
            clock: 0,
            stats: GenerativeCacheStats::default(),
        }
    }

    /// Exact-identity lookup. A miss (including any identity mismatch) is
    /// counted and returns `None`; a stale result can never be returned.
    pub fn get(&mut self, key: &GenerativeCacheKey) -> Option<ImageFrame> {
        let digest = key.digest();
        let Some(slot) = self.entries.get_mut(&digest) else {
            self.stats.misses += 1;
            // DoD §4 hot path: `trace!` only (guard prevents formatting when off).
            trace!(
                "generative cache MISS role={:?} seed={} canvas={:?} key={} misses={}",
                key.role,
                key.seed,
                key.canvas,
                digest,
                self.stats.misses
            );
            return None;
        };
        self.clock += 1;
        slot.stamp = self.clock;
        self.stats.hits += 1;
        trace!(
            "generative cache HIT role={:?} seed={} canvas={:?} key={} hits={}",
            key.role,
            key.seed,
            key.canvas,
            digest,
            self.stats.hits
        );
        Some(slot.frame.clone())
    }

    /// Inserts (or replaces) a result, evicting least-recently-used entries
    /// until the budget fits. Returns `false` — without storing anything — when
    /// the frame alone exceeds the configured budget.
    pub fn insert(&mut self, key: &GenerativeCacheKey, frame: ImageFrame) -> bool {
        let bytes = frame.pixels.len();
        if bytes > self.max_bytes {
            return false;
        }
        let digest = key.digest();
        if let Some(previous) = self.entries.remove(&digest) {
            self.used_bytes -= previous.frame.pixels.len();
        }
        self.clock += 1;
        while self.used_bytes + bytes > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, slot)| slot.stamp)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.used_bytes -= evicted.frame.pixels.len();
            }
        }
        self.used_bytes += bytes;
        trace!(
            "generative cache INSERT role={:?} key={} bytes={} entries={} used_bytes={}/{}",
            key.role,
            digest,
            bytes,
            self.entries.len() + 1,
            self.used_bytes,
            self.max_bytes
        );
        self.entries.insert(
            digest,
            GenerativeCacheSlot {
                frame,
                stamp: self.clock,
            },
        );
        true
    }

    /// Records one actual BFS execution (cache miss path).
    pub fn note_bfs_run(&mut self) {
        self.stats.bfs_runs += 1;
    }

    #[must_use]
    pub fn stats(&self) -> GenerativeCacheStats {
        GenerativeCacheStats {
            entries: self.entries.len(),
            used_bytes: self.used_bytes,
            ..self.stats
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }
}

thread_local! {
    /// Process-local (per-thread) cache used by the shared render pipeline.
    /// Every UI/CLI render on a given thread reuses the previous expand/auto-fill
    /// result; worker threads own their own instance (pure performance layer).
    static GENERATIVE_CACHE: RefCell<GenerativeCache> =
        RefCell::new(GenerativeCache::default());
}

/// Drops every entry of the current thread's render cache (e.g. on a source or
/// document switch). Counters are preserved for observability.
pub fn clear_generative_cache() {
    GENERATIVE_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// Observability snapshot of the current thread's render cache.
#[must_use]
pub fn generative_cache_stats() -> GenerativeCacheStats {
    GENERATIVE_CACHE.with(|cache| cache.borrow().stats())
}

/// Outcome of [`fill_transparent_cached`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillOutcome {
    /// Whether any transparent pixel was filled.
    pub filled: bool,
    /// Whether the result was served from the cache (no BFS executed).
    pub cache_hit: bool,
}

/// Cached auto-fill: returns the cached result for an identical identity
/// without running the BFS; otherwise runs the BFS, caches the result and
/// returns it.
pub fn fill_transparent_cached(
    cache: &mut GenerativeCache,
    frame: &mut ImageFrame,
    seed: u64,
) -> FillOutcome {
    let key = GenerativeCacheKey::auto_fill(frame, seed);
    if let Some(cached) = cache.get(&key) {
        *frame = cached;
        return FillOutcome {
            filled: true,
            cache_hit: true,
        };
    }
    cache.note_bfs_run();
    trace!(
        "generative cache BFS run role={:?} seed={} frame={}x{} key={}",
        key.role,
        key.seed,
        frame.width,
        frame.height,
        key.digest()
    );
    let filled = fill_transparent_heuristic(frame, seed);
    if filled {
        cache.insert(&key, frame.clone());
    }
    FillOutcome {
        filled,
        cache_hit: false,
    }
}

/// Auto-fill through the current thread's render cache. This is the path the
/// shared pipeline ([`ImageFrame::apply_auto_fill_transparent`]) uses.
pub fn fill_transparent_cached_global(frame: &mut ImageFrame, seed: u64) -> bool {
    GENERATIVE_CACHE
        .with(|cache| fill_transparent_cached(&mut cache.borrow_mut(), frame, seed).filled)
}

/// Heuristic fill: transparent pixels (`alpha < 255`) are replaced by the
/// nearest opaque pixel's RGB (Manhattan BFS). `seed` shuffles the BFS tie-break
/// deterministically. Returns `true` iff any pixel was filled. Alpha of filled
/// pixels becomes `255`. Deterministic for identical frame+seed.
pub fn fill_transparent_heuristic(frame: &mut ImageFrame, seed: u64) -> bool {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 || frame.pixels.len() != w * h * 4 {
        return false;
    }
    let mut opaque: Vec<(usize, usize)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            if frame.pixels[idx + 3] == 255
                && !(frame.pixels[idx] == 0
                    && frame.pixels[idx + 1] == 0
                    && frame.pixels[idx + 2] == 0)
            {
                opaque.push((x, y));
            }
        }
    }
    if opaque.is_empty() {
        return false;
    }
    if !frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0))
    {
        return false;
    }
    opaque.sort_by_key(|(x, y)| {
        let mut k = (*x as u64).wrapping_mul(73856093) ^ (*y as u64).wrapping_mul(19349663) ^ seed;
        k = k.wrapping_add(0x9e3779b97f4a7c15);
        k = (k ^ (k >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        k = (k ^ (k >> 27)).wrapping_mul(0x94d049bb133111eb);
        k ^ (k >> 31)
    });
    let mut visited = vec![false; w * h];
    let mut queue: std::collections::VecDeque<(usize, usize)> = std::collections::VecDeque::new();
    for (x, y) in opaque {
        let idx = y * w + x;
        visited[idx] = true;
        queue.push_back((x, y));
    }
    let mut filled = false;
    while let Some((x, y)) = queue.pop_front() {
        let src_idx = (y * w + x) * 4;
        let src_rgb = [
            frame.pixels[src_idx],
            frame.pixels[src_idx + 1],
            frame.pixels[src_idx + 2],
        ];
        let neighbors = [
            (x.wrapping_sub(1), y, x > 0),
            (x + 1, y, x + 1 < w),
            (x, y.wrapping_sub(1), y > 0),
            (x, y + 1, y + 1 < h),
        ];
        for (nx, ny, valid) in neighbors {
            if !valid {
                continue;
            }
            let nidx = ny * w + nx;
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            let dst = nidx * 4;
            if frame.pixels[dst + 3] < 255
                || (frame.pixels[dst] == 0
                    && frame.pixels[dst + 1] == 0
                    && frame.pixels[dst + 2] == 0)
            {
                frame.pixels[dst] = src_rgb[0];
                frame.pixels[dst + 1] = src_rgb[1];
                frame.pixels[dst + 2] = src_rgb[2];
                frame.pixels[dst + 3] = 255;
                filled = true;
            }
            queue.push_back((nx, ny));
        }
    }
    filled
}

pub fn effective_keep(recipe: &lumina_sidecar::EditRecipe) -> bool {
    recipe
        .generative_edit
        .as_ref()
        .map(|g| g.effective_keep())
        .unwrap_or(true)
}

pub fn generative_edit(recipe: &lumina_sidecar::EditRecipe) -> Option<&GenerativeEdit> {
    recipe.generative_edit.as_ref()
}

pub fn generative_canvas(recipe: &lumina_sidecar::EditRecipe) -> Option<&GenerativeCanvas> {
    recipe
        .generative_edit
        .as_ref()
        .and_then(|g| g.canvas.as_ref())
}

pub fn materialize_canvas_for_crop(
    canvas: &GenerativeCanvas,
    crop: Option<&Crop>,
) -> Result<GenerativeCanvas, CoreError> {
    if canvas.output_width == 0 || canvas.output_height == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_canvas.output".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: f64::from(u32::MAX),
        });
    }
    let (cx, cy, cw, ch) = crop_rect_on_canvas(canvas.output_width, canvas.output_height, crop)?;
    let new_offset_x = canvas.source_offset_x - cx as i32;
    let new_offset_y = canvas.source_offset_y - cy as i32;
    let out = GenerativeCanvas {
        output_width: cw,
        output_height: ch,
        source_offset_x: new_offset_x,
        source_offset_y: new_offset_y,
        extras: Default::default(),
    };
    out.validate().map_err(|_| CoreError::InvalidAdjustment {
        name: "generative_canvas.materialized".into(),
        value: 0.0,
        minimum: 1.0,
        maximum: f64::from(u32::MAX),
    })?;
    Ok(out)
}

pub fn materialize_canvas_for_crop_with_source(
    canvas: &GenerativeCanvas,
    crop: Option<&Crop>,
    source_width: u32,
    source_height: u32,
) -> Result<GenerativeCanvas, CoreError> {
    let out = materialize_canvas_for_crop(canvas, crop)?;
    let right = out.source_offset_x as i64 + source_width as i64;
    let bottom = out.source_offset_y as i64 + source_height as i64;
    if right > out.output_width as i64
        || bottom > out.output_height as i64
        || right < 0
        || bottom < 0
    {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_canvas.source_bounds".into(),
            value: right as f64,
            minimum: 0.0,
            maximum: out.output_width as f64,
        });
    }
    Ok(out)
}

pub fn resolve_canvas_for_recipe(
    recipe: &lumina_sidecar::EditRecipe,
) -> Result<Option<GenerativeCanvas>, CoreError> {
    let Some(ge) = &recipe.generative_edit else {
        return Ok(None);
    };
    let Some(canvas) = &ge.canvas else {
        return Ok(None);
    };
    if ge.effective_keep() {
        Ok(Some(canvas.clone()))
    } else {
        let crop = recipe.geometry.as_ref().and_then(|g| g.crop.as_ref());
        Ok(Some(materialize_canvas_for_crop(canvas, crop)?))
    }
}

/// GEN-FILL-02 stub: expand canvas heuristically (no model). Validates canvas
/// bounds. Uses the current thread's [`GenerativeCache`] so a repeated render of
/// the identical identity does not run the BFS again (GEN-EXPAND-CACHE-1).
pub fn apply_generative_expand(
    frame: &ImageFrame,
    recipe: &lumina_sidecar::EditRecipe,
) -> Result<ImageFrame, CoreError> {
    GENERATIVE_CACHE
        .with(|cache| apply_generative_expand_cached(&mut cache.borrow_mut(), frame, recipe))
}

/// Expand through an explicit cache. Deterministic (no thread-local state), so
/// callers that own a cache and tests can prove hit/miss behaviour exactly.
pub fn apply_generative_expand_cached(
    cache: &mut GenerativeCache,
    frame: &ImageFrame,
    recipe: &lumina_sidecar::EditRecipe,
) -> Result<ImageFrame, CoreError> {
    let Some(ge) = recipe.generative_edit.as_ref() else {
        return Ok(frame.clone());
    };
    if !ge.effective_expand() {
        return Ok(frame.clone());
    }
    let Some(canvas) = ge.canvas.as_ref() else {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_expand.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        });
    };
    validate_expand_canvas(frame, canvas)?;
    let seed = ge.seed.unwrap_or(0);
    let key = GenerativeCacheKey::expand(frame, canvas, seed);
    if let Some(cached) = cache.get(&key) {
        return Ok(cached);
    }
    cache.note_bfs_run();
    trace!(
        "generative cache BFS run role={:?} seed={} frame={}x{} canvas={}x{} offset=({},{}) key={}",
        key.role,
        key.seed,
        frame.width,
        frame.height,
        canvas.output_width,
        canvas.output_height,
        canvas.source_offset_x,
        canvas.source_offset_y,
        key.digest()
    );
    let out = build_expanded_canvas(frame, canvas, seed);
    cache.insert(&key, out.clone());
    Ok(out)
}

/// Validates the expand canvas against the current frame. Fails loudly
/// (`InvalidAdjustment`) instead of silently rendering an unexpanded frame.
fn validate_expand_canvas(frame: &ImageFrame, canvas: &GenerativeCanvas) -> Result<(), CoreError> {
    canvas
        .validate()
        .map_err(|_| CoreError::InvalidAdjustment {
            name: "generative_expand.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        })?;
    if canvas.output_width <= frame.width && canvas.output_height <= frame.height {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_expand.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    // Bounds: source must fit inside canvas
    if canvas.source_offset_x < 0
        || canvas.source_offset_y < 0
        || (canvas.source_offset_x as u32 + frame.width) > canvas.output_width
        || (canvas.source_offset_y as u32 + frame.height) > canvas.output_height
    {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_expand.bounds".into(),
            value: 0.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    Ok(())
}

/// The actual expand: composite the source into the larger canvas, mark the
/// border transparent and run the heuristic fill. Pure function of
/// `(frame, canvas, seed)`; the caller wraps it in the cache.
fn build_expanded_canvas(frame: &ImageFrame, canvas: &GenerativeCanvas, seed: u64) -> ImageFrame {
    let mut out = ImageFrame::new(
        canvas.output_width,
        canvas.output_height,
        vec![0; canvas.output_width as usize * canvas.output_height as usize * 4],
    )
    .unwrap();
    // Copy source at offset
    for y in 0..frame.height {
        for x in 0..frame.width {
            let src_idx = (y * frame.width + x) as usize * 4;
            let dst_x = (canvas.source_offset_x + x as i32) as u32;
            let dst_y = (canvas.source_offset_y + y as i32) as u32;
            let dst_idx = (dst_y * canvas.output_width + dst_x) as usize * 4;
            out.pixels[dst_idx..dst_idx + 4].copy_from_slice(&frame.pixels[src_idx..src_idx + 4]);
        }
    }
    // Fill remaining (expanded) area with heuristic (nearest neighbor via fill_transparent)
    // Mark expanded area as transparent then fill
    for y in 0..out.height {
        for x in 0..out.width {
            let dst_idx = (y * out.width + x) as usize * 4;
            let inside_source = x >= canvas.source_offset_x as u32
                && x < canvas.source_offset_x as u32 + frame.width
                && y >= canvas.source_offset_y as u32
                && y < canvas.source_offset_y as u32 + frame.height;
            if !inside_source {
                out.pixels[dst_idx + 3] = 0; // transparent
            }
        }
    }
    fill_transparent_heuristic(&mut out, seed);
    out
}

fn crop_rect_on_canvas(
    width: u32,
    height: u32,
    crop: Option<&Crop>,
) -> Result<(u32, u32, u32, u32), CoreError> {
    let (x, y, w, h) = match crop {
        None => (0.0, 0.0, 1.0, 1.0),
        Some(Crop::Free {
            x,
            y,
            width,
            height,
        }) => (*x as f64, *y as f64, *width as f64, *height as f64),
        Some(Crop::Aspect { preset }) => {
            let ratio = match preset {
                lumina_sidecar::AspectPreset::Original => width as f64 / height as f64,
                lumina_sidecar::AspectPreset::OneToOne => 1.0,
                lumina_sidecar::AspectPreset::FourToFive => 4.0 / 5.0,
                lumina_sidecar::AspectPreset::FiveToFour => 5.0 / 4.0,
                lumina_sidecar::AspectPreset::ThreeToTwo => 3.0 / 2.0,
                lumina_sidecar::AspectPreset::TwoToThree => 2.0 / 3.0,
                lumina_sidecar::AspectPreset::FourToThree => 4.0 / 3.0,
                lumina_sidecar::AspectPreset::ThreeToFour => 3.0 / 4.0,
                lumina_sidecar::AspectPreset::SixteenToNine => 16.0 / 9.0,
                lumina_sidecar::AspectPreset::NineToSixteen => 9.0 / 16.0,
            };
            let source_ratio = width as f64 / height as f64;
            if source_ratio > ratio {
                (
                    (1.0 - ratio / source_ratio) / 2.0,
                    0.0,
                    ratio / source_ratio,
                    1.0,
                )
            } else {
                (
                    0.0,
                    (1.0 - source_ratio / ratio) / 2.0,
                    1.0,
                    source_ratio / ratio,
                )
            }
        }
    };
    if ![x, y, w, h].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || x < 0.0
        || y < 0.0
        || x > 1.0
        || y > 1.0
        || x + w > 1.0 + 1e-6
        || y + h > 1.0 + 1e-6
    {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    if width == 0 || height == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty frame)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let px = (((x * width as f64).round() as i64).clamp(0, i64::from(width) - 1)) as u32;
    let py = (((y * height as f64).round() as i64).clamp(0, i64::from(height) - 1)) as u32;
    let pw = ((w * width as f64).round() as u32).max(1).min(width - px);
    let ph = ((h * height as f64).round() as u32).max(1).min(height - py);
    if pw == 0 || ph == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty crop rectangle)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    Ok((px, py, pw, ph))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{Crop, GenerativeCanvas};

    fn canvas(w: u32, h: u32, ox: i32, oy: i32) -> GenerativeCanvas {
        GenerativeCanvas {
            output_width: w,
            output_height: h,
            source_offset_x: ox,
            source_offset_y: oy,
            extras: Default::default(),
        }
    }

    #[test]
    fn effective_keep_defaults_to_true() {
        let mut recipe = lumina_sidecar::EditRecipe::default();
        assert!(effective_keep(&recipe));
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        assert!(effective_keep(&recipe));
        recipe
            .generative_edit
            .as_mut()
            .unwrap()
            .keep_generative_content = Some(false);
        assert!(!effective_keep(&recipe));
        recipe
            .generative_edit
            .as_mut()
            .unwrap()
            .keep_generative_content = Some(true);
        assert!(effective_keep(&recipe));
    }

    #[test]
    fn keep_true_leaves_canvas_unchanged() {
        let c = canvas(6000, 4000, 500, 0);
        let crop = Crop::Free {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        };
        let mut recipe = lumina_sidecar::EditRecipe::default();
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(c.clone()),
            artifact: None,
            keep_generative_content: Some(true),
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        recipe.geometry = Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(crop),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        let resolved = resolve_canvas_for_recipe(&recipe).unwrap().unwrap();
        assert_eq!(resolved, c);
    }

    #[test]
    fn keep_false_materializes_canvas_translation() {
        let c = canvas(6000, 4000, 500, 0);
        let crop = Crop::Free {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 4800);
        assert_eq!(out.output_height, 2400);
        assert_eq!(out.source_offset_x, 500 - 600);
        assert_eq!(out.source_offset_y, 0 - 800);
    }

    #[test]
    fn keep_false_full_crop_is_identity_translation() {
        let c = canvas(800, 600, 10, 20);
        let out = materialize_canvas_for_crop(&c, None).unwrap();
        assert_eq!(out.output_width, 800);
        assert_eq!(out.output_height, 600);
        assert_eq!(out.source_offset_x, 10);
        assert_eq!(out.source_offset_y, 20);
    }

    #[test]
    fn keep_false_half_crop_translates_correctly() {
        let c = canvas(100, 100, 0, 0);
        let crop = Crop::Free {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 50);
        assert_eq!(out.output_height, 100);
        assert_eq!(out.source_offset_x, -50);
        assert_eq!(out.source_offset_y, 0);
    }

    #[test]
    fn materialize_with_aspect_preset() {
        let c = canvas(400, 200, 0, 0);
        let crop = Crop::Aspect {
            preset: lumina_sidecar::AspectPreset::OneToOne,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 200);
        assert_eq!(out.output_height, 200);
        assert_eq!(out.source_offset_x, -100);
    }

    #[test]
    fn negative_offset_allowed_until_output_shrinks() {
        let c = canvas(200, 200, -50, -50);
        let crop = Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        };
        let out = materialize_canvas_for_crop(&c, Some(&crop)).unwrap();
        assert_eq!(out.output_width, 100);
        assert_eq!(out.output_height, 100);
        assert_eq!(out.source_offset_x, -50);
        assert_eq!(out.source_offset_y, -50);
    }

    #[test]
    fn bounds_check_with_source() {
        let c = canvas(6000, 4000, 500, 0);
        let crop = Crop::Free {
            x: 0.9,
            y: 0.9,
            width: 0.1,
            height: 0.1,
        };
        let err = materialize_canvas_for_crop_with_source(&c, Some(&crop), 4000, 3000).unwrap_err();
        assert!(matches!(err, CoreError::InvalidAdjustment { .. }));
    }

    #[test]
    fn recipe_hash_changes_with_keep_flag() {
        let mut a = lumina_sidecar::EditRecipe::default();
        a.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(canvas(100, 100, 0, 0)),
            artifact: None,
            keep_generative_content: Some(true),
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        let mut b = a.clone();
        b.generative_edit.as_mut().unwrap().keep_generative_content = Some(false);
        let ha = blake3::hash(&serde_json::to_vec(&a).unwrap())
            .to_hex()
            .to_string();
        let hb = blake3::hash(&serde_json::to_vec(&b).unwrap())
            .to_hex()
            .to_string();
        assert_ne!(ha, hb);
        let mut c = a.clone();
        c.generative_edit.as_mut().unwrap().keep_generative_content = None;
        assert!(effective_keep(&c));
    }

    #[test]
    fn fill_transparent_no_opaque_no_fill() {
        let mut frame =
            crate::ImageFrame::new(2, 2, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
        let filled = crate::generative::fill_transparent_heuristic(&mut frame, 0);
        assert!(!filled);
    }

    #[test]
    fn fill_transparent_no_transparent_no_fill() {
        let mut frame = crate::ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(!crate::generative::fill_transparent_heuristic(
            &mut frame, 42
        ));
        assert_eq!(frame.pixels, vec![10, 20, 30, 255]);
    }

    #[test]
    fn fill_transparent_fills_border() {
        let mut pixels = vec![0u8; 3 * 3 * 4];
        for i in 0..9 {
            pixels[i * 4 + 3] = 0;
        }
        pixels[4 * 4] = 100;
        pixels[4 * 4 + 1] = 150;
        pixels[4 * 4 + 2] = 200;
        pixels[4 * 4 + 3] = 255;
        let mut frame = crate::ImageFrame::new(3, 3, pixels).unwrap();
        assert!(crate::generative::has_transparent_pixels(&frame));
        let filled = crate::generative::fill_transparent_heuristic(&mut frame, 0);
        assert!(filled);
        assert!(!crate::generative::has_transparent_pixels(&frame));
        for y in 0..3 {
            for x in 0..3 {
                let idx = (y * 3 + x) * 4;
                assert_eq!(&frame.pixels[idx..idx + 3], &[100, 150, 200]);
                assert_eq!(frame.pixels[idx + 3], 255);
            }
        }
    }

    #[test]
    fn fill_transparent_deterministic_seed() {
        let make = || {
            crate::ImageFrame::new(
                2,
                2,
                vec![10, 10, 10, 255, 0, 0, 0, 0, 0, 0, 0, 0, 20, 20, 20, 255],
            )
            .unwrap()
        };
        let mut a = make();
        let mut b = make();
        crate::generative::fill_transparent_heuristic(&mut a, 123);
        crate::generative::fill_transparent_heuristic(&mut b, 123);
        assert_eq!(a.pixels, b.pixels);
        let mut c = make();
        crate::generative::fill_transparent_heuristic(&mut c, 999);
        assert!(!crate::generative::has_transparent_pixels(&c));
    }

    #[test]
    fn recipe_hash_changes_with_auto_fill_flag() {
        let mut a = lumina_sidecar::EditRecipe::default();
        a.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        let mut b = a.clone();
        b.generative_edit.as_mut().unwrap().auto_fill_transparent = Some(false);
        let ha = blake3::hash(&serde_json::to_vec(&a).unwrap())
            .to_hex()
            .to_string();
        let hb = blake3::hash(&serde_json::to_vec(&b).unwrap())
            .to_hex()
            .to_string();
        assert_ne!(ha, hb);
    }

    // ---- GEN-EXPAND-CACHE-1: BFS result cache (hit / miss / identity) ----

    /// Opaque 4x4 frame with distinct pixels so the BFS has real neighbours.
    fn expand_frame() -> crate::ImageFrame {
        let mut pixels = Vec::with_capacity(4 * 4 * 4);
        for i in 0..16u8 {
            pixels.extend_from_slice(&[i.wrapping_mul(7), i.wrapping_mul(3), i, 255]);
        }
        crate::ImageFrame::new(4, 4, pixels).unwrap()
    }

    /// Transparent 3x3 frame with one opaque centre pixel.
    fn auto_fill_frame() -> crate::ImageFrame {
        let mut pixels = vec![0u8; 3 * 3 * 4];
        let c = 4 * 4;
        pixels[c] = 100;
        pixels[c + 1] = 150;
        pixels[c + 2] = 200;
        pixels[c + 3] = 255;
        crate::ImageFrame::new(3, 3, pixels).unwrap()
    }

    fn expand_recipe(seed: u64, canvas: GenerativeCanvas) -> lumina_sidecar::EditRecipe {
        let mut recipe = lumina_sidecar::EditRecipe::default();
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(canvas),
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: Some(true),
            seed: Some(seed),
            prompt: None,
            extras: Default::default(),
        });
        recipe
    }

    #[test]
    fn expand_cache_serves_second_render_without_bfs() {
        let mut cache = GenerativeCache::new(10_000_000);
        let frame = expand_frame();
        let recipe = expand_recipe(0, canvas(6, 6, 1, 1));

        let first = apply_generative_expand_cached(&mut cache, &frame, &recipe).unwrap();
        assert_eq!(cache.stats().bfs_runs, 1);
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 0);

        let second = apply_generative_expand_cached(&mut cache, &frame, &recipe).unwrap();
        assert_eq!(
            first.pixels, second.pixels,
            "cached result is byte-identical"
        );
        assert_eq!((second.width, second.height), (6, 6));
        assert_eq!(cache.stats().bfs_runs, 1, "second render must not run BFS");
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn expand_cache_misses_on_seed_change() {
        let mut cache = GenerativeCache::new(10_000_000);
        let frame = expand_frame();
        apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(0, canvas(6, 6, 1, 1)))
            .unwrap();
        apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(7, canvas(6, 6, 1, 1)))
            .unwrap();
        assert_eq!(cache.stats().bfs_runs, 2, "a seed change is a loud miss");
        assert_eq!(cache.stats().hits, 0);
    }

    #[test]
    fn expand_cache_misses_on_canvas_change() {
        let mut cache = GenerativeCache::new(10_000_000);
        let frame = expand_frame();
        apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(0, canvas(6, 6, 1, 1)))
            .unwrap();
        apply_generative_expand_cached(&mut cache, &frame, &expand_recipe(0, canvas(6, 6, 0, 0)))
            .unwrap();
        assert_eq!(cache.stats().bfs_runs, 2, "a canvas change is a loud miss");
    }

    #[test]
    fn expand_cache_misses_on_source_change() {
        let mut cache = GenerativeCache::new(10_000_000);
        let recipe = expand_recipe(0, canvas(6, 6, 1, 1));
        let mut other = expand_frame();
        other.pixels[0] ^= 0xFF;
        apply_generative_expand_cached(&mut cache, &expand_frame(), &recipe).unwrap();
        apply_generative_expand_cached(&mut cache, &other, &recipe).unwrap();
        assert_eq!(
            cache.stats().bfs_runs,
            2,
            "changed source pixels are a loud miss"
        );
    }

    #[test]
    fn auto_fill_cache_serves_second_render_without_bfs() {
        let mut cache = GenerativeCache::new(10_000_000);
        let mut first = auto_fill_frame();
        let outcome = fill_transparent_cached(&mut cache, &mut first, 5);
        assert!(outcome.filled);
        assert!(!outcome.cache_hit);
        assert_eq!(cache.stats().bfs_runs, 1);
        assert!(!has_transparent_pixels(&first));

        let mut second = auto_fill_frame();
        let outcome = fill_transparent_cached(&mut cache, &mut second, 5);
        assert!(outcome.filled);
        assert!(outcome.cache_hit, "second render is a cache hit");
        assert_eq!(first.pixels, second.pixels);
        assert_eq!(cache.stats().bfs_runs, 1, "second render must not run BFS");
    }

    #[test]
    fn auto_fill_cache_misses_on_seed_change_without_serving_stale() {
        let mut cache = GenerativeCache::new(10_000_000);
        let mut a = auto_fill_frame();
        fill_transparent_cached(&mut cache, &mut a, 1);
        let mut b = auto_fill_frame();
        let outcome = fill_transparent_cached(&mut cache, &mut b, 2);
        assert!(
            !outcome.cache_hit,
            "seed change never serves the stale result"
        );
        assert_eq!(cache.stats().bfs_runs, 2);
    }

    #[test]
    fn cache_key_digest_separates_role_seed_and_canvas() {
        let frame = expand_frame();
        let base = GenerativeCacheKey::expand(&frame, &canvas(6, 6, 1, 1), 0);
        let seed = GenerativeCacheKey::expand(&frame, &canvas(6, 6, 1, 1), 1);
        let canvas_changed = GenerativeCacheKey::expand(&frame, &canvas(6, 6, 0, 0), 0);
        let auto = GenerativeCacheKey::auto_fill(&frame, 0);
        assert_ne!(base.digest(), seed.digest());
        assert_ne!(base.digest(), canvas_changed.digest());
        assert_ne!(base.digest(), auto.digest(), "roles never alias");
        assert_eq!(base.digest(), base.clone().digest(), "stable digest");
    }

    #[test]
    fn expand_cache_is_bounded_and_clearable() {
        let mut cache = GenerativeCache::new(10_000_000);
        let recipe = expand_recipe(0, canvas(6, 6, 1, 1));
        apply_generative_expand_cached(&mut cache, &expand_frame(), &recipe).unwrap();
        assert!(!cache.is_empty());
        assert!(cache.used_bytes() > 0);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn global_render_paths_reuse_the_thread_cache() {
        // Auto-fill via the shared pipeline method.
        clear_generative_cache();
        let before = generative_cache_stats().bfs_runs;
        let mut a = auto_fill_frame();
        assert!(a.apply_auto_fill_transparent(true, 9));
        let after_first = generative_cache_stats().bfs_runs;
        assert_eq!(after_first, before + 1);
        let mut b = auto_fill_frame();
        assert!(b.apply_auto_fill_transparent(true, 9));
        assert_eq!(
            generative_cache_stats().bfs_runs,
            after_first,
            "second identical auto-fill must not run BFS"
        );
        assert_eq!(a.pixels, b.pixels);

        // Expand via the public entry point.
        clear_generative_cache();
        let before = generative_cache_stats().bfs_runs;
        let recipe = expand_recipe(0, canvas(6, 6, 1, 1));
        let first = apply_generative_expand(&expand_frame(), &recipe).unwrap();
        let after_first = generative_cache_stats().bfs_runs;
        assert_eq!(after_first, before + 1);
        let second = apply_generative_expand(&expand_frame(), &recipe).unwrap();
        assert_eq!(
            generative_cache_stats().bfs_runs,
            after_first,
            "second identical expand must not run BFS"
        );
        assert_eq!(first.pixels, second.pixels);
    }
}
