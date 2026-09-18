//! CLI-LENSFUN-GPU-1 / GPU-LENSFUN-PARITY-1 (F7, Release 1.0): bind the
//! CPU-precomputed Lensfun warp/gain map of a strict corrector to the CLI's
//! [`GpuContext`] so a corrector recipe renders through the GPU path.
//!
//! Mirrors the committed GUI wiring (`crates/lumina-gui/src/lensfun_gpu.rs`)
//! and is extracted from `main.rs` on purpose (file-size ratchet §8: new logic
//! in a small, coherent file instead of growing the oversized app module).
//!
//! The map itself is produced by `lumina_core::LensfunMap::from_corrector`
//! (the committed core slice); this module owns only the CLI-side bind/classify
//! logic and the loud CPU-route reasons. Lensfun supplies arbitrary
//! distortion/TCA/vignetting models as per-pixel coordinate and gain functions,
//! so the CPU precomputes the map once per source/dimensions and the GPU
//! resample pass consumes it — no WGSL model reimplementation.
//!
//! Routing contract (mirrors the `lumina-gpu` core guards, never a silent
//! corrector loss):
//! - no or identity corrector → the map is cleared, GPU stays eligible;
//! - non-identity corrector whose map is built and bound → GPU-eligible;
//! - map build/bind failure → `UNBOUND_REASON`, the caller keeps the exact
//!   CPU reference;
//! - a distortion corrector **without** an explicit `geometry.crop` →
//!   `DEFAULT_CROP_REASON`, because the CPU oracle derives the data-dependent
//!   maximum-content rectangle (CROP-MAXRECT-1), which the GPU geometry plan
//!   deliberately refuses instead of producing divergent pixels.

#[cfg(any(feature = "lensfun", test))]
use lumina_core::LensfunMap;
use lumina_core::RenderContext;
use lumina_gpu::GpuContext;

/// Reason for a non-identity corrector whose map could not be built or bound.
/// The caller must keep the exact CPU present route, otherwise the recipe-only
/// path would silently apply the manual model instead of the database
/// correction.
#[cfg(feature = "lensfun")]
pub(crate) const UNBOUND_REASON: &str = "lens_correction (Lensfun corrector)";

/// Reason for the `lensfun_map.default_content_crop` core guard: a distortion
/// correction without an explicit crop needs the CPU oracle's content-based
/// default crop, which cannot be planned on the GPU.
#[cfg(any(feature = "lensfun", test))]
pub(crate) const DEFAULT_CROP_REASON: &str =
    "lens_correction (Lensfun corrector; distortion without an explicit crop needs the CPU content default crop)";

/// Whether a built map forces a loud CPU route for `recipe`, or `None` when it
/// is GPU-eligible. Pure decision logic so the routing contract can be pinned
/// without a GPU adapter or a native Lensfun database.
///
/// The only map guard that survives the bind step is the distortion-without-
/// crop case: a map whose dimensions do not match the frame cannot occur here
/// because the caller builds it for exactly the rendered frame dimensions.
#[cfg(any(feature = "lensfun", test))]
pub(crate) fn classify(
    map: &LensfunMap,
    recipe: &lumina_sidecar::EditRecipe,
) -> Option<&'static str> {
    let explicit_crop = recipe
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.as_ref())
        .is_some();
    (map.has_distortion && !explicit_crop).then_some(DEFAULT_CROP_REASON)
}

/// Build/lookup the corrector's map for `width × height` and bind it on `gpu`.
///
/// Returns `None` when the GPU render may proceed (no non-identity corrector,
/// or its map is bound) and `Some(reason)` when an active corrector's map could
/// not be bound — then the caller must keep the exact CPU route, because the
/// recipe-only GPU path would otherwise apply the manual model instead of the
/// database correction (a silent divergence).
///
/// The map is built once per render (which the CLI scopes to one source at
/// `width × height`), so a source/dimension/corrector change always rebuilds
/// it; the bound map in the reusable per-thread [`GpuContext`] is replaced or
/// cleared on every bind, never reused across a different source.
#[cfg(feature = "lensfun")]
pub(crate) fn bind(
    gpu: &mut GpuContext,
    render_ctx: &RenderContext<'_>,
    width: u32,
    height: u32,
) -> Option<&'static str> {
    let Some(corrector) = render_ctx.lensfun else {
        clear(gpu);
        return None;
    };
    // An identity corrector is a no-op on the CPU oracle; no map may stay bound
    // (the manual model / identity is the authoritative GPU behaviour then).
    if corrector.0.is_identity() {
        clear(gpu);
        return None;
    }
    let map = match LensfunMap::from_corrector(corrector.0, width, height) {
        Ok(map) => map,
        Err(error) => {
            log::warn!(
                "lensfun map build failed at {width}x{height}: {error} — \
                 the CLI keeps the exact CPU reference"
            );
            clear(gpu);
            return Some(UNBOUND_REASON);
        }
    };
    if let Some(reason) = classify(&map, render_ctx.recipe) {
        log::warn!(
            "lensfun distortion corrector without an explicit crop at \
             {width}x{height}: the CPU content default crop cannot be planned on \
             the GPU — the CLI keeps the exact CPU reference"
        );
        clear(gpu);
        return Some(reason);
    }
    match gpu.set_lensfun_map(Some(&map)) {
        Ok(()) => None,
        Err(error) => {
            log::warn!(
                "lensfun map bind failed at {width}x{height}: {error} — \
                 the CLI keeps the exact CPU reference"
            );
            // A stale map from another source must not stay bound (a mismatch
            // would be refused anyway, but clearing keeps the state honest).
            clear(gpu);
            Some(UNBOUND_REASON)
        }
    }
}

/// Non-Lensfun build: no corrector can exist, so the GPU route is never blocked
/// by one and no map can be bound. Clearing is still done for honesty in case a
/// context is shared with a feature-unified Lensfun-enabled consumer.
#[cfg(not(feature = "lensfun"))]
pub(crate) fn bind(
    gpu: &mut GpuContext,
    _render_ctx: &RenderContext<'_>,
    _width: u32,
    _height: u32,
) -> Option<&'static str> {
    clear(gpu);
    None
}

/// Clear any bound map (no active corrector / unbound fallback).
fn clear(gpu: &mut GpuContext) {
    if let Err(error) = gpu.set_lensfun_map(None) {
        log::warn!("lensfun map clear failed: {error}");
    }
}
