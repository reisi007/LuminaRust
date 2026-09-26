//! LENSFUN-CALLER-37 (GUI half): the G-06 EXIF auto-corrector cache.
//!
//! [`LuminaApp::ensure_lensfun_cache`] and [`LuminaApp::lensfun_render_ref`]
//! were moved here verbatim out of the crate root so the ratchet-baselined
//! `lib.rs` does not grow (DoD §8). They are the GUI's only Lensfun call site,
//! and they now hand the database lookup to the app's own diagnostics sink in
//! [`super::lensfun_diag`] instead of `LensfunDb::load_system()`, whose
//! unlevelled `stderr` lines a Dock-launched app never displays.
//!
//! # The contract this cache keeps
//!
//! * A rebuild whose **database load** misses caches **nothing**: every rebuild
//!   re-runs the system lookup ([`LOOKUP_ATTEMPTS`]), so a Lensfun database
//!   installed while the session runs is picked up on the next rebuild instead
//!   of being pinned as a stale miss. The de-duplication lives in the *sink*
//!   ([`super::lensfun_diag`]), never in this cache.
//! * The lookup is **strict**: no loose profile matching, never a guessed
//!   correction — same contract as the CLI's `build_lensfun_corrector`.
//! * The corrector is the only identity that matters, so a profile that resolves
//!   to identity is a cache entry like any other.
//!
//! # What is **not** pinned: the corrector miss
//!
//! The clause above is about the *database load*, and the tests only prove that
//! much. Whether a `for_camera` miss — a camera whose profile is simply not in
//! the database *yet* — is searched again on every rebuild is **not** pinned: a
//! memo placed *between* the load and `for_camera` would leave this cache
//! correct, would keep the attempt counter moving (the load did happen) and
//! would add no log record (the skipped re-lookup could only repeat an
//! already de-duplicated one). Nothing at the call site memoises the profile
//! lookup, so the code has no such memo — but that placement is an **untested
//! gap**, not a guarantee. `tests::lensfun_diagnostics` states the same limit
//! from the test side.

use super::lensfun_diag::with_diagnostics;
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// How many times the system Lensfun database has actually been looked up.
///
/// LENSFUN-CALLER-37: the cache above deliberately caches **no** miss, so a
/// render whose key changed retries the lookup. That contract has **no other
/// observable**: `lensfun_cache == None` after a miss is equally consistent with
/// "retried and missed again" and with "gave up after one try", and a
/// de-duplicating sink makes both look identical in the log — "no second
/// record" cannot tell a retried lookup from an abandoned one.
///
/// So the lookups are counted **where they happen**: inside the same closure
/// as the load, immediately after `load_system_with` returns (see
/// [`LuminaApp::ensure_lensfun_cache`]). Every path that skips the load also
/// skips the add — a memo in front of the call, and equally a memo *inside* the
/// closure, which is where an "have we already asked?" check would naturally be
/// written (the sink's own `seen` set). A counter read *before* the load, or an
/// add *after* the closure returned, would instead count a load that never
/// happened and hide exactly that regression.
///
/// What the counter does **not** cover: the corrector lookup behind the load. It
/// counts *database loads*, not profile searches — see the module doc's
/// "What is not pinned".
///
/// The reader is currently test-only (hence the `cfg`), so the counter's only
/// production cost is one relaxed atomic add per rebuild. It is kept in
/// production rather than pushed into the test because a counter the test
/// maintains itself would prove nothing about the production call site.
static LOOKUP_ATTEMPTS: AtomicU64 = AtomicU64::new(0);

/// This process's Lensfun lookup count (see [`LOOKUP_ATTEMPTS`]).
#[cfg(test)]
pub(crate) fn lensfun_lookup_attempts() -> u64 {
    LOOKUP_ATTEMPTS.load(Ordering::Relaxed)
}

/// Cached Lensfun auto-corrector pair (G-06).
///
/// Moved here from the crate root by LENSFUN-CALLER-37: the type is the cache's
/// own value, so it belongs with the code that fills and reads it rather than in
/// the binary root next to unrelated app state.
#[cfg(feature = "lensfun")]
pub(crate) struct CachedLensCorrector {
    pub(crate) corrector: lumina_lensfun::Corrector,
    /// The database handle is kept alive alongside the corrector because the
    /// modifier references DB-owned lens data.
    ///
    /// `corrector` is declared **first** so that it is dropped first: Rust
    /// drops struct fields in declaration order, and the modifier must be
    /// destroyed while the database it points into is still alive. Reversing
    /// these two fields compiles and leaves the whole suite green — provoking a
    /// use-after-free through liblensfun is not something a test can do
    /// cheaply — so this is a **maintainer invariant carried by the
    /// declaration order, not by a test**. Do not reorder them.
    pub(crate) _db: lumina_lensfun::LensfunDb,
    /// Identity + frame dimensions this corrector was built for.
    pub(crate) key: (
        Option<String>,
        Option<String>,
        Option<String>,
        u32,
        u32,
        u32,
        u32,
    ),
    /// GUI-LENSFUN-GATE-1 / GPU-LENSFUN-PARITY-1: whether this corrector
    /// changes pixels (`!Corrector::is_identity()`, probing
    /// distortion/vignetting/TCA). Computed once at build time so neither the
    /// present gate nor the per-frame map bind pays the FFI probe; mirrors the
    /// CLI's `lensfun_corrector_active`. An inactive (identity) corrector is a
    /// no-op on the CPU oracle and binds no map, so the manual model stays in
    /// effect on both paths. GPU-only: the non-GPU build has no bind path that
    /// could consume it.
    #[cfg(feature = "gpu")]
    pub(crate) active: bool,
    /// GPU-LENSFUN-PARITY-1: CPU-precomputed warp/gain map for this corrector at
    /// the dimensions it was last built for (`LensfunMap::from_corrector`).
    /// Built lazily by [`lensfun_gpu::bind`] on the GPU present path and reused
    /// across frames/tool moves (the per-pixel FFI build is expensive); `None`
    /// until first use. Fine to keep on the CPU: the map is a pure derived
    /// artifact, the recipe/sidecar stay authoritative.
    #[cfg(feature = "gpu")]
    pub(crate) gpu_map: Option<lumina_core::LensfunMap>,
}

impl LuminaApp {
    /// Lensfun auto-corrector cache refresh for a render at `width`×`height`
    /// (G-06, `lensfun` feature only): rebuilds the cached corrector when
    /// the identity/dimensions key changed. Split from
    /// [`Self::lensfun_render_ref`] so renders can refresh under `&mut`
    /// first and then build the `RenderContext` under shared borrows.
    #[cfg(feature = "lensfun")]
    pub(crate) fn ensure_lensfun_cache(&mut self, width: u32, height: u32) {
        let Some(identity) = self.loaded_lens_identity.clone() else {
            return;
        };
        let (Some(make), Some(model), Some(focal), Some(aperture)) = (
            identity.camera_make.clone(),
            identity.camera_model.clone(),
            identity.focal_length.filter(|v| v.is_finite()),
            identity.aperture.filter(|v| v.is_finite()),
        ) else {
            return;
        };
        let key = (
            Some(make.clone()),
            Some(model.clone()),
            identity.lens.clone(),
            width,
            height,
            focal.to_bits(),
            aperture.to_bits(),
        );
        let fresh = match &self.lensfun_cache {
            Some(cached) => cached.key != key,
            None => true,
        };
        if !fresh {
            return;
        }
        // Rebuild: a new source (or new dimensions) needs a new modifier.
        // A rebuild whose database load misses caches NOTHING, so every render
        // retries the load instead of pinning a stale miss across a DB install
        // — the lookup itself is strict (never a guessed correction, same
        // contract as the CLI `build_lensfun_corrector`). LENSFUN-CALLER-37: the
        // load goes through the app's own sink, so the outcome is a real log
        // record at a real level. The *result* is still not cached — only the
        // reporting is de-duplicated.
        //
        // The attempt is counted **inside this closure**, by the same code path
        // that performs the load and immediately after it returns, so the
        // counter means "lookups actually performed" and not merely "reached
        // this line". That placement is load-bearing: any "already tried" memo
        // that returns before `load_system_with` — in front of the call, or
        // inside the closure (the sink's own `seen` set is the natural home for
        // one) — freezes the counter and turns the retry assertion red. The
        // obvious ways to get this wrong both pass: an add *after* the closure
        // (where the counter used to sit) counts a load that was skipped, and
        // an add *before* the load counts a lookup that never happened.
        let db = with_diagnostics(|sink| {
            let db = lumina_lensfun::LensfunDb::load_system_with(sink);
            LOOKUP_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
            db
        });
        let Some(db) = db else {
            return;
        };
        let Some(corrector) = db.for_camera(
            &make,
            &model,
            identity.lens.as_deref(),
            width,
            height,
            focal,
            aperture,
            10.0,
        ) else {
            return;
        };
        super::info!(
            "lensfun auto: profile matched for {make} {model} (distortion={} vignetting={} tca={})",
            corrector.has_distortion(),
            corrector.has_vignetting(),
            corrector.has_tca()
        );
        // GUI-LENSFUN-GATE-1 / GPU-LENSFUN-PARITY-1: snapshot the
        // pixel-relevance once. A non-identity corrector is bound on the GPU as
        // a precomputed `LensfunMap` (`lensfun_gpu::bind`); only an identity
        // one is a no-op that leaves the manual model in effect on both paths.
        #[cfg(feature = "gpu")]
        let active = !corrector.is_identity();
        self.lensfun_cache = Some(CachedLensCorrector {
            corrector,
            _db: db,
            key,
            #[cfg(feature = "gpu")]
            active,
            #[cfg(feature = "gpu")]
            gpu_map: None,
        });
    }

    /// Shared borrow of the cached Lensfun auto-corrector for a render
    /// (G-06, `lensfun` feature only). Call [`Self::ensure_lensfun_cache`]
    /// first so the cache matches the rendered frame.
    #[cfg(feature = "lensfun")]
    pub(crate) fn lensfun_render_ref(&self) -> Option<lumina_core::LensfunCorrectorRef<'_>> {
        self.lensfun_cache
            .as_ref()
            .map(|cached| lumina_core::LensfunCorrectorRef(&cached.corrector))
    }
}
