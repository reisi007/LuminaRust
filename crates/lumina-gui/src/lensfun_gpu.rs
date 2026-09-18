//! GPU-LENSFUN-PARITY-1 (GUI-Wiring, Release 1.0): bind the CPU-precomputed
//! Lensfun warp/gain map of the strict auto-corrector to the GUI's
//! [`lumina_gpu::GpuContext`] so a corrector recipe presents through the
//! readback-free VRAM path.
//!
//! The map itself is produced by `lumina_core::LensfunMap::from_corrector` (the
//! committed core slice); this module owns only the GUI-side cache/bind logic
//! and the badge text for the remaining loud CPU routes. It is extracted from
//! `lib.rs` on purpose (file-size ratchet §8 — new logic in a small, coherent
//! file, no growth of the oversized app module).
//!
//! Routing contract (mirrors the `lumina-gpu` core guards, never a silent
//! divergence):
//! - no non-identity corrector → the manual model applies on both paths and the
//!   map is cleared;
//! - non-identity corrector with a bound map → GPU-eligible;
//! - non-identity corrector whose map cannot be built or bound → the caller
//!   keeps the exact CPU present route and surfaces [`UNBOUND_REASON`];
//! - a bound map that is dimension-mismatched or would need the CPU
//!   content-based default crop is refused by `GpuContext::render_to_vram`
//!   itself; [`classify_refusal`] names that core-guard refusal for the badge.

use lumina_core::LensfunMap;
use lumina_gpu::{GpuContext, GpuError};

use super::CachedLensCorrector;

/// Badge reason for a non-identity corrector whose map could not be bound.
pub(crate) const UNBOUND_REASON: &str = "lens_correction (Lensfun corrector)";

/// Build/lookup the corrector's map for `width × height` and bind it on `gpu`.
///
/// Returns `None` when the GPU present may proceed (no non-identity corrector,
/// or its map is bound) and `Some(reason)` when an active corrector's map could
/// not be bound — then the caller must keep the exact CPU present route, because
/// the recipe-only VRAM path would otherwise apply the manual model instead of
/// the database correction (a silent divergence).
pub(crate) fn bind(
    gpu: &mut GpuContext,
    cached: Option<&mut CachedLensCorrector>,
    width: u32,
    height: u32,
) -> Option<&'static str> {
    let Some(cached) = cached else {
        clear(gpu);
        return None;
    };
    if !cached.active {
        clear(gpu);
        return None;
    }
    let Some(map) = map_for(cached, width, height) else {
        clear(gpu);
        return Some(UNBOUND_REASON);
    };
    match gpu.set_lensfun_map(Some(map)) {
        Ok(()) => None,
        Err(error) => {
            log::warn!(
                "lensfun map bind failed at {width}x{height}: {error} — \
                 the present keeps the exact CPU reference"
            );
            // Best effort: a stale map from another frame must not stay bound
            // (a mismatch would be refused anyway, but clearing keeps the state
            // honest for the next bind).
            clear(gpu);
            Some(UNBOUND_REASON)
        }
    }
}

/// Name a `render_to_vram` refusal that stems from the Lensfun map guard, or
/// `None` for every other failure. The two map guards are the only corrector
/// routes that survive [`bind`] (a matching, non-default-crop map renders on
/// the GPU), so the badge text stays bound to the core guard instead of being
/// re-derived in the GUI.
pub(crate) fn classify_refusal(error: &GpuError) -> Option<String> {
    let message = error.to_string();
    if message.contains("lensfun_map.default_content_crop") {
        return Some(
            "lens_correction (Lensfun corrector; distortion without an explicit crop needs the \
             CPU content default crop)"
                .into(),
        );
    }
    if message.contains("lensfun_map.dimensions") {
        return Some(
            "lens_correction (Lensfun corrector; map dimensions do not match the frame)".into(),
        );
    }
    None
}

/// The map for `width × height`, built from the cached corrector on first use
/// and reused while the dimensions match. The per-pixel FFI build is expensive
/// (`from_corrector` walks every row), so it must not run per frame; a source
/// or dimension change replaces the whole `CachedLensCorrector` (and with it
/// this map), so no manual invalidation is needed.
fn map_for(cached: &mut CachedLensCorrector, width: u32, height: u32) -> Option<&LensfunMap> {
    let fresh = cached
        .gpu_map
        .as_ref()
        .is_some_and(|map| map.width == width && map.height == height);
    if !fresh {
        match LensfunMap::from_corrector(&cached.corrector, width, height) {
            Ok(map) => cached.gpu_map = Some(map),
            Err(error) => {
                log::warn!(
                    "lensfun map build failed at {width}x{height}: {error} — \
                     the present keeps the exact CPU reference"
                );
                cached.gpu_map = None;
                return None;
            }
        }
    }
    cached.gpu_map.as_ref()
}

/// Clear any bound map (no active corrector / unbound fallback).
fn clear(gpu: &mut GpuContext) {
    if let Err(error) = gpu.set_lensfun_map(None) {
        log::warn!("lensfun map clear failed: {error}");
    }
}
