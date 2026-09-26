//! Row-batch wrappers of [`Corrector`] (R2-LENS-01).
//!
//! Extracted from `lib.rs` (file-size ratchet, User-Vorgabe 2026-09-17). These
//! three methods are one concern — *batching the FFI boundary* — and they carry
//! the long rationale for why the geometry path is **not** a single multi-pixel
//! native call, which belongs next to the code it explains rather than in the
//! crate root.
//!
//! The block below is moved verbatim; only the `Corrector` fields it reads
//! became `pub(crate)` so the `impl` can live in this module. No behaviour,
//! signature or doc claim changed.

#![cfg(feature = "native")]

use crate::ffi::{
    lf_modifier_apply_color_modification, lf_modifier_apply_geometry_distortion,
    lf_modifier_apply_subpixel_geometry_distortion, Corrector, SubpixelTriple, LF_CR_RGB,
};
use std::os::raw::{c_float, c_int, c_void};

impl Corrector {
    // -----------------------------------------------------------------------
    // Row-batch wrappers (R2-LENS-01).
    //
    // The per-pixel `geometry` / `color_gain` methods each cross the FFI
    // boundary once per destination pixel (two transitions per pixel → ~48
    // million FFI crossings for a 24 MP frame). Lensfun's colour batch API
    // (`lf_modifier_apply_color_modification`) computes a whole *block* of
    // pixels (`width × height`) in one call. Feeding it one row at a time
    // (`height = 1`) reduces the vignetting FFI crossings to ~1 per row
    // (~8k for 24 MP); see `apply_vignetting_row`.
    //
    // # Why `geometry_row` is NOT a single native batch call
    //
    // lensfun 0.3.4's x86 SSE geometry callbacks
    // (`libs/lensfun/mod-coord-sse.cpp`: `ModifyCoord_Dist_PTLens_SSE`,
    // `ModifyCoord_UnDist_PTLens_SSE`, `ModifyCoord_Dist_Poly3_SSE`) are
    // mathematically wrong for multi-pixel blocks: they shuffle four
    // pixels' interleaved `(x, y)` lanes apart to compute one correction
    // factor per pixel, but then multiply the per-pixel factor vector
    // directly with the still-interleaved coordinate vector
    // (`_mm_store_ps(&iocoord[8*i], _mm_mul_ps(poly3, c0))`), so pixel 0's
    // factor scales pixel 0's x but pixel 1's factor scales pixel 0's y,
    // and so on. On a horizontal row every input y is identical, hence the
    // signature pairwise-duplicated output y
    // (observed on x86_64: `geometry_row(0, 0, width=5)` yields
    // y = [a, b, a, b, _], off by up to ~1 px, while width=1 calls match
    // `geometry` bit-exactly). The scalar tail (`remain = count % 4`) and
    // every width=1 call stay correct, and non-x86 builds (e.g. ARM, where
    // `VECTORIZATION_SSE` is undefined) never take the SSE path — which is
    // why the bug is x86_64-only.
    //
    // `geometry_row` therefore issues one native width=1 call per column
    // (each provably on the scalar path: `count/4 == 0`), i.e. it is
    // bit-identical to `out.len()` calls to [`Self::geometry`] on every
    // platform. The geometry FFI rate stays at one transition per pixel;
    // only the vignetting pass keeps the one-call-per-row batching. If a
    // future lensfun fixes the SSE lane shuffle, the single-call batch
    // can be re-enabled (the `geometry_row_*` tests pin the contract).
    //
    // # Documented numeric divergence of `apply_vignetting_row`
    // (not byte-identical)
    //
    // The batch colour path advances the vignette polynomial's `r²`
    // incrementally (`r2 += 2·ns·x + ns²`) instead of recomputing `x² + y²`
    // per pixel (`mod-color.cpp::ModifyColor_Vignetting_PA`). The first
    // column is bit-identical to [`Self::color_gain`]; later columns drift
    // by float rounding that grows with the row width but stays far below
    // one output unit. (Note: with our `LF_CR_RGB` 3-component role the
    // colour SSE fast path is never taken — it requires 4 components per
    // pixel plus 16-byte alignment and falls back to the scalar code — so
    // the colour batch is exact up to the documented `r²` accumulation.)
    //
    // Switching the pipeline to the row wrappers changes the exact output
    // bytes, which is why it requires a Golden rebaseline (F-043), not a
    // silent output change (see the `apply_lens` comment in `lumina-core`
    // and R2-LENS-01 in `docs/reviews/2026-08-26-full-review.md`).
    // -------------------------------------------------------------------

    /// Map a whole destination row to the source pixels it samples
    /// (R2-LENS-01).
    ///
    /// `out[i]` receives the destination→source mapping of the destination
    /// pixel `(x_start + i, y)` (for `i` in `0..out.len()`), so one call
    /// replaces `out.len()` calls to [`Self::geometry`]. `out` must have
    /// exactly as many entries as the row has pixels (its length is the row
    /// width).
    ///
    /// For a vignetting-only profile (`has_distortion() == false`) the row
    /// is filled with the exact identity mapping (as with
    /// [`Self::geometry`], review REVIEW-LENSFUN-VIGN-1).
    ///
    /// Bit-identity vs. [`Self::geometry`] contract: EVERY column is
    /// bit-identical to the corresponding per-pixel call on every
    /// platform. This is implemented as one native width=1 call per
    /// column, never as a single multi-pixel native batch call, because
    /// lensfun 0.3.4's x86 SSE geometry callbacks apply each pixel's
    /// correction factor to its neighbour's lane (see the module-level
    /// "Why `geometry_row` is NOT a single native batch call" block).
    pub fn geometry_row(&self, x_start: f64, y: f64, out: &mut [(f64, f64)]) {
        let width = out.len();
        debug_assert!(width > 0, "geometry_row requires at least one pixel");
        // Prefill with the identity mapping: for a vignetting-only profile
        // the early return below keeps these passthrough values — exactly
        // like [`Self::geometry`].
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (x_start + i as f64, y);
        }
        if !self.has_distortion {
            return;
        }
        // One native width=1 call per column: each call takes lensfun's
        // scalar path (`count/4 == 0`, plus the width=1 result matches
        // `geometry` bit-exactly), so the SSE lane-shuffle bug of the
        // multi-pixel batch path can never trigger, on any platform.
        unsafe {
            for (i, slot) in out.iter_mut().enumerate() {
                let x = x_start + i as f64;
                let mut res = [x as c_float, y as c_float];
                let ok = lf_modifier_apply_geometry_distortion(
                    self.modifier,
                    x as c_float,
                    y as c_float,
                    1,
                    1,
                    res.as_mut_ptr(),
                );
                if ok != 0 {
                    *slot = (res[0] as f64, res[1] as f64);
                }
                // `ok == 0`: no distortion callback — keep the prefilled
                // identity mapping (never a silent fallback onto (0, 0)).
            }
        }
    }

    /// Map a whole destination row to the per-channel source pixels it
    /// samples (G-06 Lensfun-Vollausbau, TCA row batch).
    ///
    /// `out[i]` receives the `(red, green, blue)` destination→source
    /// mappings of the destination pixel `(x_start + i, y)`, so one call
    /// replaces `out.len()` calls to [`Self::subpixel`]. Without TCA
    /// calibration (`has_tca() == false`) the row is filled with the
    /// exact identity triple (as with [`Self::subpixel`]).
    ///
    /// Bit-identity vs. [`Self::subpixel`] contract: EVERY column is
    /// bit-identical to the corresponding per-pixel call on every
    /// platform — implemented as one native width=1/height=1 call per
    /// column (scalar path), never as a single multi-pixel native batch
    /// call (same SSE lane-shuffle concern as `geometry_row`).
    pub fn subpixel_row(&self, x_start: f64, y: f64, out: &mut [SubpixelTriple]) {
        // Prefill with the identity triple (no-TCA passthrough).
        for (i, slot) in out.iter_mut().enumerate() {
            let p = (x_start + i as f64, y);
            *slot = (p, p, p);
        }
        if !self.has_tca {
            return;
        }
        unsafe {
            for (i, slot) in out.iter_mut().enumerate() {
                let x = x_start + i as f64;
                let mut res = [
                    x as c_float,
                    y as c_float,
                    x as c_float,
                    y as c_float,
                    x as c_float,
                    y as c_float,
                ];
                let ok = lf_modifier_apply_subpixel_geometry_distortion(
                    self.modifier,
                    x as c_float,
                    y as c_float,
                    1,
                    1,
                    res.as_mut_ptr(),
                );
                if ok != 0 {
                    *slot = (
                        (res[0] as f64, res[1] as f64),
                        (res[2] as f64, res[3] as f64),
                        (res[4] as f64, res[5] as f64),
                    );
                }
                // `ok == 0`: keep the prefilled identity triple (never a
                // silent fallback onto (0, 0)).
            }
        }
    }

    /// Apply the vignetting correction to a whole row of packed RGB pixels
    /// **in place**, in a single lensfun batch call (R2-LENS-01).
    ///
    /// `rgb` holds `width * 3` consecutive `f32`s (three channels per
    /// pixel, RGB order — lensfun walks the buffer one RGB triple per
    /// pixel via `LF_CR_RGB`); `x_start`/`y` are the destination
    /// coordinates of the row's first pixel, used for the radial position.
    /// One call replaces `width` calls to [`Self::color_gain`].
    ///
    /// The buffer is modified in place, exactly like lensfun's own
    /// `lf_modifier_apply_color_modification`. Callers wanting to keep the
    /// geometry pass separate must pass a buffer that holds only the RGB
    /// of the row (not, e.g., an RGBA frame — `LF_CR_RGB` consumes three
    /// components per pixel and would walk an RGBA buffer ragged).
    ///
    /// On a distortion-only profile (no colour callback) lensfun reports
    /// `false` and leaves the buffer untouched, matching [`Self::color_gain`].
    ///
    /// See the module-level "Documented numeric divergence of
    /// `apply_vignetting_row`" block above for the bit-identity vs.
    /// [`Self::color_gain`] contract.
    pub fn apply_vignetting_row(&self, rgb: &mut [f32], x_start: f64, y: f64) {
        debug_assert!(
            rgb.len().is_multiple_of(3),
            "apply_vignetting_row requires whole RGB triples, got {} floats",
            rgb.len()
        );
        let width = rgb.len() / 3;
        if width == 0 || !self.has_vignetting {
            return;
        }
        unsafe {
            // `row_stride = 0` → lensfun treats the block as packed;
            // with `height = 1` the row stride is unused anyway (matches
            // `color_gain`). The 16-byte alignment hint in the lensfun
            // header is a performance note, not a correctness contract;
            // `Vec<f32>`/`[f32]` buffers are fine (same as the per-pixel
            // stack array today).
            lf_modifier_apply_color_modification(
                self.modifier,
                rgb.as_mut_ptr() as *mut c_void,
                x_start as c_float,
                y as c_float,
                width as c_int,
                1,
                LF_CR_RGB,
                0,
            );
        }
    }
}
